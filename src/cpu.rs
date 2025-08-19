use libc;
use serde::Serialize;
use serde_json;
use std::ffi::CStr;
use std::mem;
use std::process::Command;
use std::collections::HashMap;

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
    
    // Try to get Apple Silicon specific info from system_profiler
    let (chip_name, ecpu_cores, pcpu_cores, cpu_freq) = get_apple_silicon_info();
    
    // Get CPU frequencies from IORegistry
    let (ecpu_freqs_mhz, pcpu_freqs_mhz) = get_cpu_frequencies_from_ioreg(&chip_name);
    
    // Use system_profiler frequency if available, otherwise try sysctl or use max freq from ioreg
    let cpu_frequency_mhz = cpu_freq
        .or_else(|| pcpu_freqs_mhz.as_ref().and_then(|f| f.last().copied().map(|v| v as u64)))
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
        let brand_name = CStr::from_bytes_with_nul(b"machdep.cpu.brand_string\0")
            .map_err(|_| "Invalid C string")?;
        
        let ret = libc::sysctlnametomib(
            brand_name.as_ptr(),
            mib.as_mut_ptr(),
            &mut (mib.len() as usize),
        );
        
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
        .args(&["SPHardwareDataType", "-json"])
        .output();
    
    match output {
        Ok(out) => {
            if let Ok(json_str) = std::str::from_utf8(&out.stdout) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
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
            }
        }
        Err(_) => {}
    }
    
    (None, None, None, None)
}

fn get_cpu_frequencies_from_ioreg(chip_name: &Option<String>) -> (Option<Vec<u32>>, Option<Vec<u32>>) {
    // Run ioreg to get pmgr device properties
    let output = Command::new("ioreg")
        .args(&["-rn", "pmgr"])
        .output();
    
    match output {
        Ok(out) => {
            if let Ok(ioreg_str) = std::str::from_utf8(&out.stdout) {
                // Parse voltage-states from ioreg output
                let mut voltage_states = HashMap::new();
                
                for line in ioreg_str.lines() {
                    if let Some(pos) = line.find("\"voltage-states") {
                        if let Some(end_quote) = line[pos+1..].find("\"") {
                            let key = &line[pos+1..pos+1+end_quote];
                            
                            // Extract hex data between < and >
                            if let Some(start) = line.find("<") {
                                if let Some(end) = line.find(">") {
                                    let hex_str = &line[start+1..end];
                                    voltage_states.insert(key.to_string(), hex_str.to_string());
                                }
                            }
                        }
                    }
                }
                
                // Determine CPU frequency scale based on chip type
                let cpu_scale = if let Some(name) = chip_name {
                    if name.contains("M1") || name.contains("M2") || name.contains("M3") {
                        1000 * 1000  // MHz before M4
                    } else {
                        1000  // KHz for M4 and later
                    }
                } else {
                    1000 * 1000  // Default to MHz
                };
                
                // Parse efficiency core frequencies (voltage-states1-sram)
                let ecpu_freqs = voltage_states.get("voltage-states1-sram")
                    .and_then(|hex| parse_voltage_states(hex, cpu_scale));
                
                // Parse performance core frequencies (voltage-states5-sram)
                let pcpu_freqs = voltage_states.get("voltage-states5-sram")
                    .and_then(|hex| parse_voltage_states(hex, cpu_scale));
                
                return (ecpu_freqs, pcpu_freqs);
            }
        }
        Err(_) => {}
    }
    
    (None, None)
}

fn parse_voltage_states(hex_str: &str, scale: u32) -> Option<Vec<u32>> {
    let hex_str = hex_str.replace(" ", "");
    let bytes = hex::decode(&hex_str).ok()?;
    
    // Each entry is 8 bytes: 4 bytes frequency, 4 bytes voltage
    if bytes.len() % 8 != 0 {
        return None;
    }
    
    let mut frequencies = Vec::new();
    for chunk in bytes.chunks_exact(8) {
        let freq_bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
        let freq = u32::from_le_bytes(freq_bytes);
        
        // Convert to MHz
        let freq_mhz = freq / scale;
        if freq_mhz > 0 {
            frequencies.push(freq_mhz);
        }
    }
    
    // Remove duplicates and sort
    frequencies.sort_unstable();
    frequencies.dedup();
    
    if frequencies.is_empty() {
        None
    } else {
        Some(frequencies)
    }
}