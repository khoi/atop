use crate::iokit;
use serde::Serialize;
use std::mem;
use std::process::Command;

#[derive(Debug, Serialize)]
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

pub fn get_cpu_metrics() -> Result<CpuMetrics, Box<dyn std::error::Error>> {
    let physical_cores = get_physical_cores()?;
    let logical_cores = get_logical_cores()?;
    let cpu_brand = get_cpu_brand()?;

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
                .and_then(|f| f.last().copied().map(|v| v as u64))
        })
        .unwrap_or_else(|| get_cpu_frequency().unwrap_or(0));

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

fn get_cpu_brand() -> Result<String, Box<dyn std::error::Error>> {
    unsafe {
        let mut mib = [libc::CTL_MACHDEP, 0];
        let mut size = 0;

        // Get the size needed for the brand string
        let brand_name = c"machdep.cpu.brand_string";

        let ret = libc::sysctlnametomib(brand_name.as_ptr(), mib.as_mut_ptr(), &mut { mib.len() });

        if ret != 0 {
            // Fall back to simple model name if brand_string is not available
            return Ok("Apple Processor".to_string());
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
            return Ok("Apple Processor".to_string());
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
            return Ok("Apple Processor".to_string());
        }

        // Remove null terminator and convert to string
        if let Some(null_pos) = brand.iter().position(|&c| c == 0) {
            brand.truncate(null_pos);
        }

        Ok(String::from_utf8_lossy(&brand).to_string())
    }
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
            return get_cpu_frequency_alt();
        }

        Ok(freq / 1_000_000) // Convert to MHz
    }
}

fn get_cpu_frequency_alt() -> Result<u64, Box<dyn std::error::Error>> {
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
            return Ok(freq / 1_000_000); // Convert to MHz
        }
    }

    // Return 0 to indicate unknown frequency
    Ok(0)
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
            .map(|s| s.to_string());

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
