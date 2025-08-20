use crate::iokit;
use serde::Serialize;
use std::ffi::CString;
use std::mem;
use std::process::Command;

#[derive(Debug, Default, Clone, Serialize)]
pub struct CpuMetrics {
    pub physical_cores: u32,
    pub logical_cores: u32,
    pub cpu_brand: String,
    pub cpu_frequency_mhz: u64,
    pub chip_name: Option<String>,
    pub ecpu_cores: Option<u32>,
    pub pcpu_cores: Option<u32>,
    pub ecpu_freqs_mhz: Option<Vec<u32>>,
    pub pcpu_freqs_mhz: Option<Vec<u32>>,
}

#[derive(Debug, Default)]
pub struct CpuInfo {
    pub ecpu_freqs_mhz: Vec<u32>,
    pub pcpu_freqs_mhz: Vec<u32>,
}

pub fn get_cpu_info() -> Result<CpuInfo, Box<dyn std::error::Error>> {
    let (ecpu_freqs_mhz, pcpu_freqs_mhz, _) = iokit::get_cpu_frequencies()?;
    Ok(CpuInfo {
        ecpu_freqs_mhz: ecpu_freqs_mhz.unwrap_or_default(),
        pcpu_freqs_mhz: pcpu_freqs_mhz.unwrap_or_default(),
    })
}

pub fn get_gpu_freqs() -> Result<Vec<u32>, Box<dyn std::error::Error>> {
    let (_, gpu_freqs, _) = iokit::get_gpu_frequencies()?;
    Ok(gpu_freqs.unwrap_or_default())
}

pub fn get_cpu_metrics() -> Result<CpuMetrics, Box<dyn std::error::Error>> {
    let physical_cores = get_physical_cores()?;
    let logical_cores = get_logical_cores()?;
    let cpu_brand = get_cpu_brand();

    // Get CPU frequencies directly from IORegistry using IOKit (no system_profiler by default)
    let (ecpu_freqs_mhz, pcpu_freqs_mhz, _chip_name_unused) = iokit::get_cpu_frequencies()?;

    // Use brand as chip name when informative
    let mut chip_name = if cpu_brand != "Apple Processor" {
        Some(cpu_brand.clone())
    } else {
        None
    };
    // Try to read E/P core counts via sysctl fast path
    let (ecpu_fast, pcpu_fast) = get_perflevel_core_counts();
    let mut ecpu_cores: Option<u32> = ecpu_fast;
    let mut pcpu_cores: Option<u32> = pcpu_fast;

    // Derive CPU frequency from IORegistry max P-core freq or sysctl fallback
    let mut cpu_frequency_mhz = pcpu_freqs_mhz
        .as_ref()
        .and_then(|f| f.last().copied().map(u64::from))
        .unwrap_or_else(|| get_cpu_frequency().unwrap_or_else(|_| get_cpu_frequency_alt()));

    // Fallback: only call system_profiler if we still lack useful info
    if (chip_name.is_none() || chip_name.as_deref() == Some("Apple Processor"))
        && cpu_frequency_mhz == 0
    {
        let (chip_sp, ec_sp, pc_sp, sp_freq) = get_apple_silicon_info();
        if chip_name.is_none() {
            chip_name = chip_sp;
        }
        if ecpu_cores.is_none() {
            ecpu_cores = ec_sp;
        }
        if pcpu_cores.is_none() {
            pcpu_cores = pc_sp;
        }
        if cpu_frequency_mhz == 0 {
            cpu_frequency_mhz = sp_freq.unwrap_or(0);
        }
    }

    Ok(CpuMetrics {
        physical_cores,
        logical_cores,
        cpu_brand,
        cpu_frequency_mhz,
        chip_name,
        ecpu_cores,
        pcpu_cores,
        ecpu_freqs_mhz,
        pcpu_freqs_mhz,
    })
}

fn get_physical_cores() -> Result<u32, Box<dyn std::error::Error>> {
    unsafe {
        // Try HW_PHYSICALCPU first
        const HW_PHYSICALCPU: i32 = 104;
        let mut mib = [libc::CTL_HW, HW_PHYSICALCPU];
        let mut cores: i32 = 0;
        let mut size = mem::size_of::<i32>();

        let ret = libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as _,
            &mut cores as *mut _ as *mut _,
            &mut size,
            std::ptr::null_mut(),
            0,
        );

        if ret == 0 && cores > 0 {
            return Ok(cores as u32);
        }

        // Fall back to HW_NCPU if HW_PHYSICALCPU fails
        let mut mib = [libc::CTL_HW, libc::HW_NCPU];
        let mut cores: i32 = 0;
        let mut size = mem::size_of::<i32>();

        let ret = libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as _,
            &mut cores as *mut _ as *mut _,
            &mut size,
            std::ptr::null_mut(),
            0,
        );

        if ret != 0 {
            return Err("Failed to get physical CPU count".into());
        }

        Ok(cores as u32)
    }
}

fn get_logical_cores() -> Result<u32, Box<dyn std::error::Error>> {
    unsafe {
        let mut mib = [libc::CTL_HW, libc::HW_NCPU];
        let mut cores: i32 = 0;
        let mut size = mem::size_of::<i32>();

        let ret = libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as _,
            &mut cores as *mut _ as *mut _,
            &mut size,
            std::ptr::null_mut(),
            0,
        );

        if ret != 0 {
            return Err("Failed to get logical CPU count".into());
        }

        Ok(cores as u32)
    }
}

fn get_cpu_brand() -> String {
    // Try fast path via sysctlbyname (Intel Macs)
    unsafe {
        if let Ok(name) = CString::new("machdep.cpu.brand_string") {
            let mut size: libc::size_t = 0;
            let ret = libc::sysctlbyname(
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            );
            if ret == 0 && size > 1 {
                let mut buf = vec![0u8; size as usize];
                let ret2 = libc::sysctlbyname(
                    name.as_ptr(),
                    buf.as_mut_ptr() as *mut libc::c_void,
                    &mut size,
                    std::ptr::null_mut(),
                    0,
                );
                if ret2 == 0 {
                    if let Some(pos) = buf.iter().position(|&b| b == 0) {
                        buf.truncate(pos);
                    }
                    if let Ok(s) = std::str::from_utf8(&buf)
                        && !s.is_empty()
                    {
                        return s.to_string();
                    }
                }
            }
        }
    }

    // Fall back to system_profiler (Apple Silicon or missing key)
    let (chip_name, _ec, _pc, _freq) = get_apple_silicon_info();
    chip_name.unwrap_or_else(|| "Apple Processor".to_string())
}

fn get_cpu_frequency() -> Result<u64, Box<dyn std::error::Error>> {
    unsafe {
        let mut mib = [libc::CTL_HW, libc::HW_CPU_FREQ];
        let mut freq: u64 = 0;
        let mut size = mem::size_of::<u64>();

        let ret = libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as _,
            &mut freq as *mut _ as *mut _,
            &mut size,
            std::ptr::null_mut(),
            0,
        );

        if ret != 0 || freq == 0 {
            // Try alternative method for Apple Silicon
            return Ok(get_cpu_frequency_alt());
        }

        Ok(freq / 1_000_000) // Convert to MHz
    }
}

fn get_cpu_frequency_alt() -> u64 {
    // Try to get CPU frequency max from sysctl
    unsafe {
        const HW_CPUFREQUENCY_MAX: i32 = 107;
        let mut mib = [libc::CTL_HW, HW_CPUFREQUENCY_MAX];
        let mut freq: u64 = 0;
        let mut size = mem::size_of::<u64>();

        let ret = libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as _,
            &mut freq as *mut _ as *mut _,
            &mut size,
            std::ptr::null_mut(),
            0,
        );

        if ret == 0 && freq > 0 {
            return freq / 1_000_000; // Convert to MHz
        }
    }

    // Return 0 to indicate unknown frequency
    0
}

fn get_apple_silicon_info() -> (Option<String>, Option<u32>, Option<u32>, Option<u64>) {
    // Try to run system_profiler to get detailed chip info
    let output = Command::new("system_profiler")
        .args(["SPHardwareDataType", "-json"])
        .output();

    if let Ok(out) = output
        && let Ok(json_str) = std::str::from_utf8(&out.stdout)
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str)
    {
        // Extract chip_type
        let chip_name = json["SPHardwareDataType"][0]["chip_type"]
            .as_str()
            .map(std::string::ToString::to_string);

        // Extract processor cores info
        let processor_info = json["SPHardwareDataType"][0]["number_processors"]
            .as_str()
            .and_then(|s| s.strip_prefix("proc "))
            .unwrap_or("");

        let cores: Vec<u32> = processor_info
            .split(':')
            .filter_map(|s| s.parse().ok())
            .collect();

        let (ecpu_cores, pcpu_cores) = if cores.len() == 3 {
            (Some(cores[2]), Some(cores[1]))
        } else {
            (None, None)
        };

        // Try to extract CPU speed from current_processor_speed (Intel Macs)
        let cpu_speed = json["SPHardwareDataType"][0]["current_processor_speed"]
            .as_str()
            .and_then(|s| {
                // Parse strings like "4.05 GHz" or "3.2 GHz"
                if let Some(ghz_str) = s.strip_suffix(" GHz") {
                    ghz_str.parse::<f64>().ok().map(|ghz| (ghz * 1000.0) as u64)
                } else {
                    None
                }
            });

        return (chip_name, ecpu_cores, pcpu_cores, cpu_speed);
    }

    (None, None, None, None)
}

fn get_perflevel_core_counts() -> (Option<u32>, Option<u32>) {
    unsafe {
        let read_u32 = |name: &str| -> Option<u32> {
            if let Ok(cname) = CString::new(name) {
                let mut val: u32 = 0;
                let mut size = std::mem::size_of::<u32>();
                let ret = libc::sysctlbyname(
                    cname.as_ptr(),
                    &mut val as *mut _ as *mut libc::c_void,
                    &mut size,
                    std::ptr::null_mut(),
                    0,
                );
                if ret == 0 && size == std::mem::size_of::<u32>() && val > 0 {
                    return Some(val);
                }
            }
            None
        };

        // perflevel0 = performance cores, perflevel1 = efficiency cores (on newer macOS)
        let pcpu = read_u32("hw.perflevel0.physicalcpu");
        let ecpu = read_u32("hw.perflevel1.physicalcpu");
        (ecpu, pcpu)
    }
}
