#[cfg(target_os = "macos")]
use crate::iokit;

use serde::Serialize;
use std::process::Command;

#[cfg(target_os = "macos")]
use std::mem;

#[derive(Debug, Default, Serialize)]
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

#[cfg(target_os = "macos")]
pub fn get_cpu_info() -> Result<CpuInfo, Box<dyn std::error::Error>> {
    let (ecpu_freqs_mhz, pcpu_freqs_mhz, _) = iokit::get_cpu_frequencies()?;
    Ok(CpuInfo {
        ecpu_freqs_mhz: ecpu_freqs_mhz.unwrap_or_default(),
        pcpu_freqs_mhz: pcpu_freqs_mhz.unwrap_or_default(),
    })
}

#[cfg(target_os = "macos")]
pub fn get_gpu_freqs() -> Result<Vec<u32>, Box<dyn std::error::Error>> {
    let (_, gpu_freqs, _) = iokit::get_gpu_frequencies()?;
    Ok(gpu_freqs.unwrap_or_default())
}

pub fn get_cpu_metrics() -> Result<CpuMetrics, Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    {
        get_cpu_metrics_macos()
    }
    
    #[cfg(target_os = "linux")]
    {
        get_cpu_metrics_linux()
    }
    
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err("Unsupported platform".into())
    }
}

#[cfg(target_os = "macos")]
fn get_cpu_metrics_macos() -> Result<CpuMetrics, Box<dyn std::error::Error>> {
    let physical_cores = get_physical_cores_macos()?;
    let logical_cores = get_logical_cores_macos()?;
    let cpu_brand = get_cpu_brand_macos();

    // Get CPU frequencies directly from IORegistry using IOKit
    let (ecpu_freqs_mhz, pcpu_freqs_mhz, chip_name_from_io) = iokit::get_cpu_frequencies()?;

    // Try to get Apple Silicon specific info from system_profiler for core counts
    let (chip_name_sp, ecpu_cores, pcpu_cores, cpu_freq) = get_apple_silicon_info();

    // Use chip name from system_profiler if available, otherwise from IOKit
    let chip_name = chip_name_sp.or(chip_name_from_io);

    // Use system_profiler frequency if available, otherwise use max freq from IORegistry
    let cpu_frequency_mhz = cpu_freq
        .or_else(|| {
            pcpu_freqs_mhz
                .as_ref()
                .and_then(|f| f.last().copied().map(u64::from))
        })
        .unwrap_or_else(|| get_cpu_frequency_macos().unwrap_or_else(|_| get_cpu_frequency_alt()));

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

#[cfg(target_os = "macos")]
fn get_physical_cores_macos() -> Result<u32, Box<dyn std::error::Error>> {
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

#[cfg(target_os = "macos")]
fn get_logical_cores_macos() -> Result<u32, Box<dyn std::error::Error>> {
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

#[cfg(target_os = "macos")]
fn get_cpu_brand_macos() -> String {
    unsafe {
        let mut mib = [libc::CTL_MACHDEP, 0];
        let mut size = 0;

        // Get the size needed for the brand string
        let brand_name = c"machdep.cpu.brand_string";

        let ret = libc::sysctlnametomib(brand_name.as_ptr(), mib.as_mut_ptr(), &mut { mib.len() });

        if ret != 0 {
            // Fall back to simple model name if brand_string is not available
            return "Apple Processor".to_string();
        }

        // First get the size
        libc::sysctl(
            mib.as_mut_ptr(),
            2,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        );

        if size == 0 {
            return "Apple Processor".to_string();
        }

        // Now get the actual brand string
        let mut brand = vec![0u8; size];
        let ret = libc::sysctl(
            mib.as_mut_ptr(),
            2,
            brand.as_mut_ptr() as *mut _,
            &mut size,
            std::ptr::null_mut(),
            0,
        );

        if ret != 0 {
            return "Apple Processor".to_string();
        }

        // Remove null terminator and convert to string
        if let Some(null_pos) = brand.iter().position(|&c| c == 0) {
            brand.truncate(null_pos);
        }

        String::from_utf8_lossy(&brand).to_string()
    }
}

#[cfg(target_os = "macos")]
fn get_cpu_frequency_macos() -> Result<u64, Box<dyn std::error::Error>> {
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

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "linux")]
fn get_cpu_metrics_linux() -> Result<CpuMetrics, Box<dyn std::error::Error>> {
    use std::fs;
    use std::collections::HashSet;
    
    let cpuinfo = fs::read_to_string("/proc/cpuinfo")
        .map_err(|_| "Failed to read /proc/cpuinfo")?;
    
    let mut physical_cores = 0u32;
    let mut logical_cores = 0u32;
    let mut cpu_brand = String::from("Unknown");
    let mut cpu_frequency_mhz = 0u64;
    let mut core_ids = HashSet::new();
    
    // Parse /proc/cpuinfo
    for line in cpuinfo.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            
            match key {
                "processor" => {
                    logical_cores += 1;
                }
                "core id" => {
                    if let Ok(core_id) = value.parse::<u32>() {
                        core_ids.insert(core_id);
                    }
                }
                "model name" => {
                    if cpu_brand == "Unknown" {
                        cpu_brand = value.to_string();
                    }
                }
                "cpu MHz" => {
                    if let Ok(freq) = value.parse::<f64>() {
                        cpu_frequency_mhz = cpu_frequency_mhz.max(freq as u64);
                    }
                }
                _ => {}
            }
        }
    }
    
    // Physical cores is the number of unique core IDs, or logical cores if core IDs not available
    physical_cores = if core_ids.is_empty() {
        logical_cores
    } else {
        core_ids.len() as u32
    };
    
    // Try to get current frequency from /sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq
    if cpu_frequency_mhz == 0 {
        if let Ok(freq_str) = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq") {
            if let Ok(freq_khz) = freq_str.trim().parse::<u64>() {
                cpu_frequency_mhz = freq_khz / 1000; // Convert KHz to MHz
            }
        }
    }
    
    // Fall back to a reasonable default if we couldn't detect frequency
    if cpu_frequency_mhz == 0 {
        cpu_frequency_mhz = 2000; // 2GHz default
    }
    
    Ok(CpuMetrics {
        physical_cores,
        logical_cores,
        cpu_brand,
        cpu_frequency_mhz,
        chip_name: None, // Not applicable on Linux
        ecpu_cores: None, // Not applicable on Linux
        pcpu_cores: None, // Not applicable on Linux
        ecpu_freqs_mhz: None, // Not applicable on Linux
        pcpu_freqs_mhz: None, // Not applicable on Linux
    })
}
