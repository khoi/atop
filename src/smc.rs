use std::ffi::CString;
use std::mem;

// SMC key types
const SMC_CMD_READ_KEYINFO: u8 = 9;
const SMC_CMD_READ_BYTES: u8 = 5;

// SMC data structures
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
struct SMCVersion {
    major: u8,
    minor: u8,
    build: u8,
    reserved: u8,
    release: u16,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
struct SMCPLimitData {
    version: u16,
    length: u16,
    cpu_p_limit: u32,
    gpu_p_limit: u32,
    mem_p_limit: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
struct SMCKeyInfoData {
    data_size: u32,
    data_type: u32,
    data_attributes: u8,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
struct SMCKeyData {
    key: u32,
    vers: SMCVersion,
    p_limit_data: SMCPLimitData,
    key_info: SMCKeyInfoData,
    result: u8,
    status: u8,
    data8: u8,
    data32: u32,
    bytes: [u8; 32],
}

// IOKit bindings
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOServiceMatching(name: *const i8) -> core_foundation::dictionary::CFMutableDictionaryRef;
    fn IOServiceGetMatchingServices(
        main_port: u32,
        matching: core_foundation::dictionary::CFDictionaryRef,
        existing: *mut u32,
    ) -> i32;
    fn IOIteratorNext(iterator: u32) -> u32;
    fn IORegistryEntryGetName(entry: u32, name: *mut i8) -> i32;
    fn IOServiceOpen(service: u32, owning_task: u32, conn_type: u32, connection: *mut u32) -> i32;
    fn IOServiceClose(connection: u32) -> i32;
    fn IOObjectRelease(object: u32) -> u32;
    fn IOConnectCallStructMethod(
        connection: u32,
        selector: u32,
        input: *const std::ffi::c_void,
        input_size: usize,
        output: *mut std::ffi::c_void,
        output_size: *mut usize,
    ) -> i32;
    fn mach_task_self() -> u32;
}

// IOService iterator for finding SMC endpoints
struct IOServiceIterator {
    iterator: u32,
}

impl IOServiceIterator {
    fn new(service_name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let c_name = CString::new(service_name)?;
        let mut iterator = 0u32;

        unsafe {
            let matching = IOServiceMatching(c_name.as_ptr());
            if matching.is_null() {
                return Err(
                    format!("Failed to create matching dictionary for {}", service_name).into(),
                );
            }

            let result = IOServiceGetMatchingServices(0, matching, &mut iterator);
            if result != 0 {
                return Err(format!("{} service not found", service_name).into());
            }
        }

        Ok(IOServiceIterator { iterator })
    }
}

impl Iterator for IOServiceIterator {
    type Item = (u32, String);

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            let entry = IOIteratorNext(self.iterator);
            if entry == 0 {
                return None;
            }

            let mut name_buf = [0i8; 128];
            if IORegistryEntryGetName(entry, name_buf.as_mut_ptr()) != 0 {
                return None;
            }

            let name = std::ffi::CStr::from_ptr(name_buf.as_ptr())
                .to_string_lossy()
                .to_string();

            Some((entry, name))
        }
    }
}

impl Drop for IOServiceIterator {
    fn drop(&mut self) {
        if self.iterator != 0 {
            unsafe {
                IOObjectRelease(self.iterator);
            }
        }
    }
}

pub struct SMC {
    connection: u32,
}

impl SMC {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut connection = 0u32;

        // Iterate through AppleSMC services to find the right endpoint
        let iterator = IOServiceIterator::new("AppleSMC")?;
        for (device, name) in iterator {
            // On Apple Silicon, we need AppleSMCKeysEndpoint
            if name == "AppleSMCKeysEndpoint" {
                unsafe {
                    let result = IOServiceOpen(device, mach_task_self(), 0, &mut connection);
                    IOObjectRelease(device);

                    if result == 0 && connection != 0 {
                        return Ok(SMC { connection });
                    }
                }
            } else {
                // Clean up this device reference
                unsafe {
                    IOObjectRelease(device);
                }
            }
        }

        Err("Failed to find and connect to SMC endpoint".into())
    }

    fn read_key_info(&self, key: &str) -> Result<SMCKeyInfoData, Box<dyn std::error::Error>> {
        if key.len() != 4 {
            return Err("SMC key must be exactly 4 characters".into());
        }

        let key_bytes = key.as_bytes();
        let key_32 = u32::from_be_bytes([key_bytes[0], key_bytes[1], key_bytes[2], key_bytes[3]]);

        let input = SMCKeyData {
            key: key_32,
            data8: SMC_CMD_READ_KEYINFO,
            ..Default::default()
        };

        let mut output = input;
        let mut output_size = mem::size_of::<SMCKeyData>();

        unsafe {
            let result = IOConnectCallStructMethod(
                self.connection,
                2, // kSMCHandleYPCEvent
                &input as *const _ as *const std::ffi::c_void,
                mem::size_of::<SMCKeyData>(),
                &mut output as *mut _ as *mut std::ffi::c_void,
                &mut output_size,
            );

            if result != 0 {
                return Err(format!(
                    "Failed to read key info for {} (IOConnect: {})",
                    key, result
                )
                .into());
            }

            if output.result != 0 {
                return Err(format!("SMC error for key {}: {}", key, output.result).into());
            }
        }

        Ok(output.key_info)
    }

    fn read_key_data(
        &self,
        key: &str,
        info: &SMCKeyInfoData,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        if key.len() != 4 {
            return Err("SMC key must be exactly 4 characters".into());
        }

        let key_bytes = key.as_bytes();
        let key_32 = u32::from_be_bytes([key_bytes[0], key_bytes[1], key_bytes[2], key_bytes[3]]);

        let input = SMCKeyData {
            key: key_32,
            data8: SMC_CMD_READ_BYTES,
            key_info: *info,
            ..Default::default()
        };

        let mut output = input;
        let mut output_size = mem::size_of::<SMCKeyData>();

        unsafe {
            let result = IOConnectCallStructMethod(
                self.connection,
                2, // kSMCHandleYPCEvent
                &input as *const _ as *const std::ffi::c_void,
                mem::size_of::<SMCKeyData>(),
                &mut output as *mut _ as *mut std::ffi::c_void,
                &mut output_size,
            );

            if result != 0 || output.result != 0 {
                return Err(format!("Failed to read key data for {}", key).into());
            }
        }

        Ok(output.bytes[0..info.data_size as usize].to_vec())
    }

    pub fn read_temperature(&self, key: &str) -> Result<f32, Box<dyn std::error::Error>> {
        let info = self.read_key_info(key)?;
        let data = self.read_key_data(key, &info)?;

        // Temperature keys on Apple Silicon use "flt " format
        // The type is stored in key_info.data_type
        match info.data_type {
            0x666c7420 => {
                // "flt " - 32-bit float (most common on Apple Silicon)
                if data.len() >= 4 {
                    let bytes = [data[0], data[1], data[2], data[3]];
                    Ok(f32::from_le_bytes(bytes)) // Little-endian!
                } else {
                    Err("Invalid temperature data".into())
                }
            }
            0x73703738 => {
                // "sp78" - 16.16 fixed point (older format)
                if data.len() >= 2 {
                    let raw = u16::from_be_bytes([data[0], data[1]]);
                    Ok(raw as f32 / 256.0)
                } else {
                    Err("Invalid temperature data".into())
                }
            }
            _ => {
                // Unknown format - for debugging
                Err(format!(
                    "Unknown data type: 0x{:08x} for key {}",
                    info.data_type, key
                )
                .into())
            }
        }
    }

    pub fn get_cpu_temperature(&self) -> Result<f32, Box<dyn std::error::Error>> {
        // Collect all CPU temperature sensors (Tp* for P-cores, Te* for E-cores)
        let mut temps = Vec::new();

        // Try known CPU temperature keys (found on M3 Max)
        let known_keys = [
            // E-core temperatures
            "Te04", "Te05", "Te06", "Te0K", "Te0L", "Te0M", "Te0P", "Te0Q", "Te0S", "Te0T",
            // P-core temperatures
            "Tp04", "Tp05", "Tp06", "Tp0C", "Tp0D", "Tp0E", "Tp0K", "Tp0L", "Tp0M", "Tp0R", "Tp0S",
            "Tp0T", "Tp0U", "Tp0V", "Tp0W", "Tp0a", "Tp0b", "Tp0c", "Tp16", "Tp17", "Tp18", "Tp1E",
            "Tp1F", "Tp1G", "Tp1I", "Tp1J", "Tp1K", "Tp25", "Tp26", "Tp27", "Tp29", "Tp2A", "Tp2B",
            "Tp2H", "Tp2I", "Tp2J", "Tp33", "Tp34", "Tp35", "Tp3B", "Tp3C", "Tp3D",
        ];

        for key in &known_keys {
            if let Ok(temp) = self.read_temperature(key) {
                if temp > 0.0 && temp < 150.0 {
                    temps.push(temp);
                }
            }
        }

        if temps.is_empty() {
            Err("Could not read CPU temperature".into())
        } else {
            // Return average of all CPU sensors
            Ok(temps.iter().sum::<f32>() / temps.len() as f32)
        }
    }

    pub fn get_gpu_temperature(&self) -> Result<f32, Box<dyn std::error::Error>> {
        // Collect all GPU temperature sensors
        let mut temps = Vec::new();

        // Try known GPU temperature keys (found on M3 Max)
        let known_keys = [
            "Tg00", "Tg01", "Tg04", "Tg05", "Tg0C", "Tg0D", "Tg0K", "Tg0L", "Tg0y", "Tg0z", "Tg16",
            "Tg17", "Tg1E", "Tg1F", "Tg1s", "Tg1t", "Tg21", "Tg22", "Tg29", "Tg2A", "Tg2H", "Tg2I",
            "Tg33", "Tg34", "Tg3B", "Tg3C", "Tg3J", "Tg3K", "Tg3x", "Tg3y",
        ];

        for key in &known_keys {
            if let Ok(temp) = self.read_temperature(key) {
                if temp > 0.0 && temp < 150.0 {
                    // Sanity check
                    temps.push(temp);
                }
            }
        }

        if temps.is_empty() {
            Err("Could not read GPU temperature".into())
        } else {
            // Return average of all GPU sensors
            Ok(temps.iter().sum::<f32>() / temps.len() as f32)
        }
    }

    pub fn get_all_temperatures(&self) -> Vec<(String, f32)> {
        let mut temps = Vec::new();

        // Common temperature sensor keys
        let known_keys = [
            ("TC0P", "CPU Proximity"),
            ("Tp01", "CPU P-Core 1"),
            ("Tp05", "CPU P-Core 2"),
            ("Tp09", "CPU P-Core 3"),
            ("Tp0D", "CPU P-Core 4"),
            ("Te05", "CPU E-Core 1"),
            ("Te0L", "CPU E-Core 2"),
            ("TG0P", "GPU Proximity"),
            ("Tg05", "GPU Die"),
            ("Tm02", "Memory Bank 1"),
            ("Tm08", "Memory Bank 2"),
            ("TB1T", "Battery 1"),
            ("TB2T", "Battery 2"),
            ("TW0P", "Wireless Module"),
        ];

        for (key, description) in &known_keys {
            if let Ok(temp) = self.read_temperature(key) {
                if temp > 0.0 && temp < 150.0 {
                    temps.push((description.to_string(), temp));
                }
            }
        }

        temps
    }
}

impl Drop for SMC {
    fn drop(&mut self) {
        if self.connection != 0 {
            unsafe {
                IOServiceClose(self.connection);
            }
        }
    }
}

// Public interface for temperature metrics
#[derive(Debug, Clone, serde::Serialize)]
pub struct TemperatureMetrics {
    pub cpu_temp: Option<f32>,
    pub gpu_temp: Option<f32>,
    pub sensors: Vec<(String, f32)>,
}

pub fn get_temperature_metrics() -> Result<TemperatureMetrics, Box<dyn std::error::Error>> {
    let smc = match SMC::new() {
        Ok(s) => s,
        Err(_e) => {
            // Return empty metrics if SMC connection fails
            // This is common on macOS without proper permissions
            return Ok(TemperatureMetrics {
                cpu_temp: None,
                gpu_temp: None,
                sensors: Vec::new(),
            });
        }
    };

    let cpu_temp = smc.get_cpu_temperature().ok();
    let gpu_temp = smc.get_gpu_temperature().ok();
    let sensors = smc.get_all_temperatures();

    Ok(TemperatureMetrics {
        cpu_temp,
        gpu_temp,
        sensors,
    })
}
