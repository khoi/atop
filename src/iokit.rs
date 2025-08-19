use core_foundation::base::{CFAllocatorRef, kCFAllocatorDefault};
use core_foundation::data::{CFDataGetBytes, CFDataGetLength, CFDataRef};
use core_foundation::dictionary::{CFDictionaryRef, CFMutableDictionaryRef};
use core_foundation_sys::base::CFRange;
use std::ffi::CString;
use std::mem::MaybeUninit;

// IOKit bindings
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOServiceMatching(name: *const i8) -> CFMutableDictionaryRef;
    fn IOServiceGetMatchingServices(
        mainPort: u32,
        matching: CFDictionaryRef,
        existing: *mut u32,
    ) -> i32;
    fn IOIteratorNext(iterator: u32) -> u32;
    fn IORegistryEntryGetName(entry: u32, name: *mut i8) -> i32;
    fn IORegistryEntryCreateCFProperties(
        entry: u32,
        properties: *mut CFMutableDictionaryRef,
        allocator: CFAllocatorRef,
        options: u32,
    ) -> i32;
    fn IOObjectRelease(obj: u32) -> u32;
}

// Helper to get a value from CF dictionary
fn cfdict_get_val(dict: CFDictionaryRef, key: &str) -> Option<CFDataRef> {
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    use core_foundation_sys::dictionary::CFDictionaryGetValue;

    unsafe {
        let cf_key = CFString::new(key);
        let val = CFDictionaryGetValue(dict, cf_key.as_CFTypeRef() as _);

        if val.is_null() {
            None
        } else {
            Some(val as CFDataRef)
        }
    }
}

pub struct IOServiceIterator {
    existing: u32,
}

impl IOServiceIterator {
    pub fn new(service_name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let service_name = CString::new(service_name)?;
        let existing = unsafe {
            let service = IOServiceMatching(service_name.as_ptr());
            let mut existing = 0;
            if IOServiceGetMatchingServices(0, service, &mut existing) != 0 {
                return Err(format!("{} not found", service_name.to_string_lossy()).into());
            }
            existing
        };

        Ok(Self { existing })
    }
}

impl Drop for IOServiceIterator {
    fn drop(&mut self) {
        unsafe {
            IOObjectRelease(self.existing);
        }
    }
}

impl Iterator for IOServiceIterator {
    type Item = (u32, String);

    fn next(&mut self) -> Option<Self::Item> {
        let next = unsafe { IOIteratorNext(self.existing) };
        if next == 0 {
            return None;
        }

        let mut name = [0i8; 128];
        if unsafe { IORegistryEntryGetName(next, name.as_mut_ptr()) } != 0 {
            return None;
        }

        let name = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) };
        let name = name.to_string_lossy().to_string();
        Some((next, name))
    }
}

pub fn get_io_props(entry: u32) -> Result<CFDictionaryRef, Box<dyn std::error::Error>> {
    unsafe {
        let mut props: MaybeUninit<CFMutableDictionaryRef> = MaybeUninit::uninit();
        if IORegistryEntryCreateCFProperties(entry, props.as_mut_ptr(), kCFAllocatorDefault, 0) != 0
        {
            return Err("Failed to get properties".into());
        }

        Ok(props.assume_init() as CFDictionaryRef)
    }
}

// Parse voltage-states binary data
pub fn parse_dvfs_mhz(dict: CFDictionaryRef, key: &str) -> Option<Vec<u32>> {
    let data = cfdict_get_val(dict, key)?;

    unsafe {
        let obj_len = CFDataGetLength(data);
        let mut obj_val = vec![0u8; obj_len as usize];
        CFDataGetBytes(
            data,
            CFRange {
                location: 0,
                length: obj_len,
            },
            obj_val.as_mut_ptr(),
        );

        // obj_val is pairs of (freq, voltage) 4 bytes each
        let items_count = (obj_len / 8) as usize;
        let mut freqs = Vec::with_capacity(items_count);

        for chunk in obj_val.chunks_exact(8) {
            let freq = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            if freq > 0 {
                freqs.push(freq);
            }
        }

        if freqs.is_empty() { None } else { Some(freqs) }
    }
}

// Type alias for CPU frequency result
pub type CpuFrequencyResult =
    Result<(Option<Vec<u32>>, Option<Vec<u32>>, Option<String>), Box<dyn std::error::Error>>;

// Get CPU frequencies from IORegistry
pub fn get_cpu_frequencies() -> CpuFrequencyResult {
    let mut ecpu_freqs = None;
    let mut pcpu_freqs = None;
    let mut chip_name = None;

    // Get chip info from system_profiler first to determine scaling
    if let Ok(output) = std::process::Command::new("system_profiler")
        .args(["SPHardwareDataType", "-json"])
        .output()
        && let Ok(json_str) = std::str::from_utf8(&output.stdout)
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str)
    {
        chip_name = json["SPHardwareDataType"][0]["chip_type"]
            .as_str()
            .map(|s| s.to_string());
    }

    // Determine CPU frequency scale based on chip type
    let cpu_scale = if let Some(ref name) = chip_name {
        if name.contains("M1") || name.contains("M2") || name.contains("M3") {
            1000 * 1000 // MHz for M1-M3
        } else {
            1000 // KHz for M4 and later
        }
    } else {
        1000 * 1000 // Default to MHz
    };

    // Find pmgr device in IORegistry
    for (entry, name) in IOServiceIterator::new("AppleARMIODevice")? {
        if name == "pmgr" {
            let props = get_io_props(entry)?;

            // Get efficiency core frequencies (voltage-states1-sram)
            if let Some(freqs) = parse_dvfs_mhz(props, "voltage-states1-sram") {
                ecpu_freqs = Some(freqs.into_iter().map(|f| f / cpu_scale).collect());
            }

            // Get performance core frequencies (voltage-states5-sram)
            if let Some(freqs) = parse_dvfs_mhz(props, "voltage-states5-sram") {
                pcpu_freqs = Some(freqs.into_iter().map(|f| f / cpu_scale).collect());
            }

            // Release the properties dictionary
            unsafe {
                use core_foundation_sys::base::CFRelease;
                CFRelease(props as _);
            }

            break;
        }
    }

    Ok((ecpu_freqs, pcpu_freqs, chip_name))
}
