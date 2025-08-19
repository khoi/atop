use core_foundation::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
use core_foundation::base::{
    CFAllocatorRef, CFRelease, CFTypeRef, kCFAllocatorDefault, kCFAllocatorNull,
};
use core_foundation::data::{CFDataGetBytes, CFDataGetLength, CFDataRef};
use core_foundation::dictionary::{
    CFDictionaryCreateMutableCopy, CFDictionaryGetCount, CFDictionaryRef, CFMutableDictionaryRef,
};
use core_foundation::string::{
    CFStringCreateWithBytesNoCopy, CFStringGetCString, CFStringRef, kCFStringEncodingUTF8,
};
use core_foundation_sys::base::CFRange;
use serde::Serialize;
use std::ffi::{CString, c_void};
use std::marker::{PhantomData, PhantomPinned};
use std::mem::MaybeUninit;
use std::ptr::null;

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

// IOReport framework bindings
#[repr(C)]
struct IOReportSubscription {
    _data: [u8; 0],
    _phantom: PhantomData<(*mut u8, PhantomPinned)>,
}

type IOReportSubscriptionRef = *const IOReportSubscription;

#[link(name = "IOReport", kind = "dylib")]
unsafe extern "C" {
    fn IOReportCopyAllChannels(a: u64, b: u64) -> CFDictionaryRef;
    fn IOReportCopyChannelsInGroup(
        group: CFStringRef,
        subgroup: CFStringRef,
        a: u64,
        b: u64,
        c: u64,
    ) -> CFDictionaryRef;
    fn IOReportMergeChannels(a: CFDictionaryRef, b: CFDictionaryRef, nil: CFTypeRef);
    fn IOReportCreateSubscription(
        a: *const c_void,
        b: CFMutableDictionaryRef,
        c: *mut CFMutableDictionaryRef,
        d: u64,
        e: CFTypeRef,
    ) -> IOReportSubscriptionRef;
    fn IOReportCreateSamples(
        a: IOReportSubscriptionRef,
        b: CFMutableDictionaryRef,
        c: CFTypeRef,
    ) -> CFDictionaryRef;
    fn IOReportCreateSamplesDelta(
        a: CFDictionaryRef,
        b: CFDictionaryRef,
        c: CFTypeRef,
    ) -> CFDictionaryRef;
    fn IOReportChannelGetGroup(a: CFDictionaryRef) -> CFStringRef;
    fn IOReportChannelGetSubGroup(a: CFDictionaryRef) -> CFStringRef;
    fn IOReportChannelGetChannelName(a: CFDictionaryRef) -> CFStringRef;
    fn IOReportSimpleGetIntegerValue(a: CFDictionaryRef, b: i32) -> i64;
    fn IOReportChannelGetUnitLabel(a: CFDictionaryRef) -> CFStringRef;
    #[allow(dead_code)]
    fn IOReportStateGetCount(a: CFDictionaryRef) -> i32;
    #[allow(dead_code)]
    fn IOReportStateGetNameForIndex(a: CFDictionaryRef, b: i32) -> CFStringRef;
    #[allow(dead_code)]
    fn IOReportStateGetResidency(a: CFDictionaryRef, b: i32) -> i64;
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

// IOReport utility functions

// Create a CFString from a Rust string
fn cfstr(val: &str) -> CFStringRef {
    unsafe {
        CFStringCreateWithBytesNoCopy(
            kCFAllocatorDefault,
            val.as_ptr(),
            val.len() as isize,
            kCFStringEncodingUTF8,
            0,
            kCFAllocatorNull,
        )
    }
}

// Convert CFString to Rust String
fn from_cfstr(cf_str: CFStringRef) -> String {
    if cf_str.is_null() {
        return String::new();
    }

    unsafe {
        let mut buffer = [0u8; 256];
        let success = CFStringGetCString(
            cf_str,
            buffer.as_mut_ptr() as *mut i8,
            buffer.len() as isize,
            kCFStringEncodingUTF8,
        );

        if success != 0 {
            std::ffi::CStr::from_ptr(buffer.as_ptr() as *const i8)
                .to_string_lossy()
                .to_string()
        } else {
            String::new()
        }
    }
}

// Get channel group name
fn get_channel_group(item: CFDictionaryRef) -> String {
    match unsafe { IOReportChannelGetGroup(item) } {
        x if x.is_null() => String::new(),
        x => from_cfstr(x),
    }
}

// Get channel subgroup name
fn get_channel_subgroup(item: CFDictionaryRef) -> String {
    match unsafe { IOReportChannelGetSubGroup(item) } {
        x if x.is_null() => String::new(),
        x => from_cfstr(x),
    }
}

// Get channel name
fn get_channel_name(item: CFDictionaryRef) -> String {
    match unsafe { IOReportChannelGetChannelName(item) } {
        x if x.is_null() => String::new(),
        x => from_cfstr(x),
    }
}

// Get channel unit label
fn get_unit_label(item: CFDictionaryRef) -> String {
    match unsafe { IOReportChannelGetUnitLabel(item) } {
        x if x.is_null() => String::new(),
        x => from_cfstr(x).trim().to_string(),
    }
}

// Convert energy value to watts based on unit
fn energy_to_watts(
    item: CFDictionaryRef,
    unit: &str,
    duration_ms: u64,
) -> Result<f32, Box<dyn std::error::Error>> {
    let raw_value = unsafe { IOReportSimpleGetIntegerValue(item, 0) } as f32;
    let time_factor = duration_ms as f32 / 1000.0; // Convert ms to seconds
    let value_per_second = raw_value / time_factor;

    let watts = match unit {
        "mJ" => value_per_second / 1000.0, // millijoules to watts
        "uJ" | "μJ" => value_per_second / 1_000_000.0, // microjoules to watts
        "nJ" => value_per_second / 1_000_000_000.0, // nanojoules to watts
        _ => return Err(format!("Unknown energy unit: {}", unit).into()),
    };

    Ok(watts)
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

// Get GPU frequencies from IORegistry
pub fn get_gpu_frequencies() -> CpuFrequencyResult {
    let mut gpu_freqs = None;

    // Try to get frequency info from pmgr device
    for (entry, name) in IOServiceIterator::new("AppleARMIODevice")? {
        if name == "pmgr" {
            let props = get_io_props(entry)?;

            // GPU frequencies are in voltage-states9
            if let Some(freqs) = parse_dvfs_mhz(props, "voltage-states9") {
                // Convert to MHz (from Hz)
                let freqs_mhz: Vec<u32> = freqs.iter().map(|&f| f / 1_000_000).collect();
                gpu_freqs = Some(freqs_mhz);
            }

            unsafe { CFRelease(props as _) };
            break;
        }
    }

    Ok((None, gpu_freqs, None))
}

// Get CPU frequencies from IORegistry
pub fn get_cpu_frequencies() -> CpuFrequencyResult {
    let mut ecpu_freqs = None;
    let mut pcpu_freqs = None;
    let mut chip_name = None;

    // Get chip info from system_profiler (optional, for display purposes)
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

    // Find pmgr device in IORegistry
    for (entry, name) in IOServiceIterator::new("AppleARMIODevice")? {
        if name == "pmgr" {
            let props = get_io_props(entry)?;

            // Get raw frequency values to determine scale
            let mut cpu_scale = 1000 * 1000; // Default to Hz->MHz

            // Check a sample frequency to determine if values are in Hz or KHz
            if let Some(sample_freqs) = parse_dvfs_mhz(props, "voltage-states1-sram")
                .or_else(|| parse_dvfs_mhz(props, "voltage-states5-sram"))
                && let Some(&first_freq) = sample_freqs.first()
            {
                // If raw value is > 100 MHz (100_000_000 Hz), it's in Hz
                // If raw value is < 10 MHz (10_000 KHz), it's in KHz
                if first_freq > 100_000_000 {
                    cpu_scale = 1000 * 1000; // Hz to MHz
                } else if first_freq < 10_000 {
                    cpu_scale = 1000; // KHz to MHz
                }
            }

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

// IOReport channel iterator
pub struct IOReportIterator {
    sample: CFDictionaryRef,
    index: isize,
    items: CFArrayRef,
    items_size: isize,
}

impl IOReportIterator {
    pub fn new(data: CFDictionaryRef) -> Self {
        let items = cfdict_get_val(data, "IOReportChannels").unwrap() as CFArrayRef;
        let items_size = unsafe { CFArrayGetCount(items) } as isize;
        Self {
            sample: data,
            items,
            items_size,
            index: 0,
        }
    }
}

impl Drop for IOReportIterator {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.sample as _);
        }
    }
}

#[derive(Debug)]
pub struct IOReportChannel {
    pub group: String,
    #[allow(dead_code)]
    pub subgroup: String,
    pub channel: String,
    pub unit: String,
    pub item: CFDictionaryRef,
}

impl Iterator for IOReportIterator {
    type Item = IOReportChannel;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.items_size {
            return None;
        }

        let item = unsafe { CFArrayGetValueAtIndex(self.items, self.index) } as CFDictionaryRef;

        let group = get_channel_group(item);
        let subgroup = get_channel_subgroup(item);
        let channel = get_channel_name(item);
        let unit = get_unit_label(item);

        self.index += 1;
        Some(IOReportChannel {
            group,
            subgroup,
            channel,
            unit,
            item,
        })
    }
}

// Main IOReport interface
pub struct IOReport {
    subscription: IOReportSubscriptionRef,
    channels: CFMutableDictionaryRef,
}

impl IOReport {
    // Create IOReport instance for specific channel groups
    pub fn new(groups: Vec<(&str, Option<&str>)>) -> Result<Self, Box<dyn std::error::Error>> {
        let channels = if groups.is_empty() {
            // Get all channels if no specific groups requested
            unsafe {
                let all_channels = IOReportCopyAllChannels(0, 0);
                CFDictionaryCreateMutableCopy(
                    kCFAllocatorDefault,
                    CFDictionaryGetCount(all_channels),
                    all_channels,
                )
            }
        } else {
            // Get specific channel groups
            let mut channel_dicts = Vec::new();

            for (group, subgroup) in groups {
                let group_str = cfstr(group);
                let subgroup_str = match subgroup {
                    Some(sg) => cfstr(sg),
                    None => null() as CFStringRef,
                };

                let channels =
                    unsafe { IOReportCopyChannelsInGroup(group_str, subgroup_str, 0, 0, 0) };
                channel_dicts.push(channels);

                unsafe {
                    CFRelease(group_str as _);
                    if !subgroup_str.is_null() {
                        CFRelease(subgroup_str as _);
                    }
                }
            }

            // Merge all channel dictionaries
            let first_channels = channel_dicts[0];
            for channels in channel_dicts.iter().skip(1) {
                unsafe {
                    IOReportMergeChannels(first_channels, *channels, null());
                }
            }

            let size = unsafe { CFDictionaryGetCount(first_channels) };
            let merged =
                unsafe { CFDictionaryCreateMutableCopy(kCFAllocatorDefault, size, first_channels) };

            // Clean up individual channel dictionaries
            for channels in channel_dicts {
                unsafe {
                    CFRelease(channels as _);
                }
            }

            merged
        };

        // Verify we got channels
        if cfdict_get_val(channels, "IOReportChannels").is_none() {
            return Err("Failed to get IOReport channels".into());
        }

        // Create subscription
        let mut subscription_dict: MaybeUninit<CFMutableDictionaryRef> = MaybeUninit::uninit();
        let subscription = unsafe {
            IOReportCreateSubscription(null(), channels, subscription_dict.as_mut_ptr(), 0, null())
        };

        if subscription.is_null() {
            return Err("Failed to create IOReport subscription".into());
        }

        Ok(Self {
            subscription,
            channels,
        })
    }

    // Take a power measurement sample over a duration
    pub fn sample_power(
        &self,
        duration_ms: u64,
    ) -> Result<IOReportIterator, Box<dyn std::error::Error>> {
        unsafe {
            // Take first sample
            let sample1 = IOReportCreateSamples(self.subscription, self.channels, null());

            // Wait for the specified duration
            std::thread::sleep(std::time::Duration::from_millis(duration_ms));

            // Take second sample
            let sample2 = IOReportCreateSamples(self.subscription, self.channels, null());

            // Calculate delta
            let delta = IOReportCreateSamplesDelta(sample1, sample2, null());

            // Clean up intermediate samples
            CFRelease(sample1 as _);
            CFRelease(sample2 as _);

            Ok(IOReportIterator::new(delta))
        }
    }
}

impl Drop for IOReport {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.channels as _);
            CFRelease(self.subscription as _);
        }
    }
}

// Power metrics structure
#[derive(Debug, Default, Serialize)]
pub struct PowerMetrics {
    pub cpu_power: f32,     // Watts
    pub gpu_power: f32,     // Watts
    pub ane_power: f32,     // Watts (Apple Neural Engine)
    pub ram_power: f32,     // Watts
    pub gpu_ram_power: f32, // Watts
    pub all_power: f32,     // Combined CPU+GPU+ANE
    pub sys_power: f32,     // Total system power from SMC
}

// Collect power metrics using IOReport
pub fn get_power_metrics(
    smc_sys_power: Option<f32>,
) -> Result<PowerMetrics, Box<dyn std::error::Error>> {
    // Create IOReport instance for Energy Model group
    let ioreport = IOReport::new(vec![("Energy Model", None)])?;

    // Take a 1000ms sample to get power readings
    let sample = ioreport.sample_power(1000)?;

    let mut metrics = PowerMetrics::default();

    // Process each channel in the sample
    for channel in sample {
        if channel.group == "Energy Model" {
            let power_result = energy_to_watts(channel.item, &channel.unit, 1000);

            match power_result {
                Ok(watts) => {
                    match channel.channel.as_str() {
                        "GPU Energy" => metrics.gpu_power += watts,
                        // Handle different CPU energy patterns for different chip types
                        c if c.ends_with("CPU Energy") => metrics.cpu_power += watts,
                        // Handle ANE (Apple Neural Engine) patterns
                        c if c.starts_with("ANE") => metrics.ane_power += watts,
                        // Handle memory power patterns
                        c if c.starts_with("DRAM") => metrics.ram_power += watts,
                        c if c.starts_with("GPU SRAM") => metrics.gpu_ram_power += watts,
                        _ => {}
                    }
                }
                Err(_) => {
                    // Skip channels with unknown units or conversion errors
                }
            }
        }
    }

    // Calculate combined power
    metrics.all_power = metrics.cpu_power + metrics.gpu_power + metrics.ane_power;

    // Use SMC system power if available, otherwise fall back to calculated total
    metrics.sys_power = smc_sys_power.unwrap_or(metrics.all_power);

    Ok(metrics)
}
