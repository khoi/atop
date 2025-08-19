use std::collections::HashMap;
use std::ffi::CString;
use std::mem;

// SMC key types
const SMC_CMD_READ_KEYINFO: u8 = 9;
const SMC_CMD_READ_BYTES: u8 = 5;
const SMC_CMD_READ_INDEX: u8 = 8;

// IOKit error codes
const KIORETURN_NOT_PRIVILEGED: i32 = -536_870_174;

// SMC value types - dynamically determined
#[derive(Debug, Clone)]
pub enum SMCValue {
    Float(f32),
    U8(u8),
    U16(u16),
    U32(u32),
    I8(i8),
    I16(i16),
    Flag(bool),
    String(String),
    Bytes(Vec<u8>),
}

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

// Trait for types that can be read from little-endian bytes
pub trait FromLeBytes: Sized {
    fn from_le_bytes(data: &[u8], key: &str) -> Result<Self, Box<dyn std::error::Error>>;
}

impl FromLeBytes for u16 {
    fn from_le_bytes(data: &[u8], key: &str) -> Result<Self, Box<dyn std::error::Error>> {
        if data.len() >= 2 {
            Ok(u16::from_le_bytes([data[0], data[1]]))
        } else {
            Err(format!("Insufficient data for u16 key {}", key).into())
        }
    }
}

impl FromLeBytes for i16 {
    fn from_le_bytes(data: &[u8], key: &str) -> Result<Self, Box<dyn std::error::Error>> {
        if data.len() >= 2 {
            Ok(i16::from_le_bytes([data[0], data[1]]))
        } else {
            Err(format!("Insufficient data for i16 key {}", key).into())
        }
    }
}

impl FromLeBytes for u32 {
    fn from_le_bytes(data: &[u8], key: &str) -> Result<Self, Box<dyn std::error::Error>> {
        if data.len() >= 4 {
            Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        } else {
            Err(format!("Insufficient data for u32 key {}", key).into())
        }
    }
}

impl FromLeBytes for i32 {
    fn from_le_bytes(data: &[u8], key: &str) -> Result<Self, Box<dyn std::error::Error>> {
        if data.len() >= 4 {
            Ok(i32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        } else {
            Err(format!("Insufficient data for i32 key {}", key).into())
        }
    }
}

pub struct Smc {
    connection: u32,
    key_cache: HashMap<u32, SMCKeyInfoData>,
}

impl Smc {
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
                        return Ok(Smc {
                            connection,
                            key_cache: HashMap::new(),
                        });
                    } else if result == KIORETURN_NOT_PRIVILEGED {
                        // kIOReturnNotPrivileged
                        return Err("SMC access denied. Temperature monitoring may require elevated privileges on some systems.".into());
                    } else if result != 0 {
                        return Err(
                            format!("Failed to open SMC service: error code {}", result).into()
                        );
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

    fn read_key_by_index(&self, index: u32) -> Result<String, Box<dyn std::error::Error>> {
        let input = SMCKeyData {
            data8: SMC_CMD_READ_INDEX,
            data32: index,
            ..Default::default()
        };

        let mut output = input;
        let mut output_size = mem::size_of::<SMCKeyData>();

        unsafe {
            let result = IOConnectCallStructMethod(
                self.connection,
                2,
                &input as *const _ as *const std::ffi::c_void,
                mem::size_of::<SMCKeyData>(),
                &mut output as *mut _ as *mut std::ffi::c_void,
                &mut output_size,
            );

            if result != 0 {
                return Err(format!("Failed to read key at index {}", index).into());
            }
        }

        let key_bytes = output.key.to_be_bytes();
        Ok(std::str::from_utf8(&key_bytes)?.to_string())
    }

    fn read_key_info(&mut self, key: &str) -> Result<SMCKeyInfoData, Box<dyn std::error::Error>> {
        if key.len() != 4 {
            return Err("SMC key must be exactly 4 characters".into());
        }

        let key_bytes = key.as_bytes();
        let key_32 = u32::from_be_bytes([key_bytes[0], key_bytes[1], key_bytes[2], key_bytes[3]]);

        // Check cache first
        if let Some(&cached_info) = self.key_cache.get(&key_32) {
            return Ok(cached_info);
        }

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

        // Cache the result
        self.key_cache.insert(key_32, output.key_info);
        Ok(output.key_info)
    }

    pub fn read_all_keys(&mut self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        // Read the number of keys from #KEY
        let num_keys = self.read_num_keys()?;
        let mut keys = Vec::new();

        for i in 0..num_keys {
            match self.read_key_by_index(i) {
                Ok(key) => {
                    // Filter out invalid keys
                    if !key.chars().all(|c| c.is_ascii_graphic()) {
                        continue;
                    }
                    keys.push(key);
                }
                Err(_) => {}
            }
        }

        Ok(keys)
    }

    fn read_num_keys(&mut self) -> Result<u32, Box<dyn std::error::Error>> {
        let info = self.read_key_info("#KEY")?;
        let data = self.read_key_data("#KEY", &info)?;

        if data.len() >= 4 {
            Ok(u32::from_be_bytes([data[0], data[1], data[2], data[3]]))
        } else {
            Err("Invalid #KEY data".into())
        }
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

            if result != 0 {
                return Err(format!(
                    "Failed to read key data for {} (IOKit error: {})",
                    key, result
                )
                .into());
            }
            if output.result != 0 {
                return Err(format!(
                    "Failed to read key data for {} (SMC error: {})",
                    key, output.result
                )
                .into());
            }
        }

        Ok(output.bytes[0..info.data_size as usize].to_vec())
    }

    // Generic value reading with dynamic type detection
    pub fn read_value(&mut self, key: &str) -> Result<SMCValue, Box<dyn std::error::Error>> {
        let info = self.read_key_info(key)?;
        let data = self.read_key_data(key, &info)?;

        // Convert type to string for easier handling
        let type_bytes = info.data_type.to_be_bytes();
        let type_str = std::str::from_utf8(&type_bytes).unwrap_or("????");

        // Dynamically parse based on type string
        let value = match type_str {
            "flt " => {
                // 32-bit float (little-endian on Apple Silicon)
                if data.len() >= 4 {
                    SMCValue::Float(f32::from_le_bytes([data[0], data[1], data[2], data[3]]))
                } else {
                    return Err("Invalid float data".into());
                }
            }
            "fp1f" | "fp2e" | "fp3d" | "fp4c" | "fp5b" | "fp6a" | "fp79" | "fp88" | "fpa6"
            | "fpc4" | "fpe2" => {
                // Fixed point formats - interpret as scaled integers
                if data.len() >= 2 {
                    let raw = u16::from_be_bytes([data[0], data[1]]);
                    let scale = match type_str {
                        "fp1f" => 32768.0, // 2^15
                        "fp2e" => 16384.0, // 2^14
                        "fp3d" => 8192.0,  // 2^13
                        "fp4c" => 4096.0,  // 2^12
                        "fp5b" => 2048.0,  // 2^11
                        "fp6a" => 1024.0,  // 2^10
                        "fp79" => 512.0,   // 2^9
                        "fp88" => 256.0,   // 2^8
                        "fpa6" => 64.0,    // 2^6
                        "fpc4" => 16.0,    // 2^4
                        "fpe2" => 4.0,     // 2^2
                        _ => 256.0,
                    };
                    SMCValue::Float(raw as f32 / scale)
                } else {
                    return Err("Invalid fixed point data".into());
                }
            }
            "sp1e" | "sp2d" | "sp3c" | "sp4b" | "sp5a" | "sp69" | "sp78" | "sp87" | "sp96"
            | "spb4" | "spf0" => {
                // Signed fixed point formats
                if data.len() >= 2 {
                    let raw = i16::from_be_bytes([data[0], data[1]]);
                    let scale = match type_str {
                        "sp1e" => 16384.0,
                        "sp2d" => 8192.0,
                        "sp3c" => 4096.0,
                        "sp4b" => 2048.0,
                        "sp5a" => 1024.0,
                        "sp69" => 512.0,
                        "sp78" => 256.0,
                        "sp87" => 128.0,
                        "sp96" => 64.0,
                        "spb4" => 16.0,
                        "spf0" => 1.0,
                        _ => 256.0,
                    };
                    SMCValue::Float(raw as f32 / scale)
                } else {
                    return Err("Invalid signed fixed point data".into());
                }
            }
            "ui8 " => {
                // Unsigned 8-bit integer
                if !data.is_empty() {
                    SMCValue::U8(data[0])
                } else {
                    return Err("Invalid ui8 data".into());
                }
            }
            "ui16" => {
                // Unsigned 16-bit integer
                if data.len() >= 2 {
                    SMCValue::U16(u16::from_be_bytes([data[0], data[1]]))
                } else {
                    return Err("Invalid ui16 data".into());
                }
            }
            "ui32" => {
                // Unsigned 32-bit integer
                if data.len() >= 4 {
                    SMCValue::U32(u32::from_be_bytes([data[0], data[1], data[2], data[3]]))
                } else {
                    return Err("Invalid ui32 data".into());
                }
            }
            "si8 " => {
                // Signed 8-bit integer
                if !data.is_empty() {
                    SMCValue::I8(data[0] as i8)
                } else {
                    return Err("Invalid si8 data".into());
                }
            }
            "si16" => {
                // Signed 16-bit integer
                if data.len() >= 2 {
                    SMCValue::I16(i16::from_be_bytes([data[0], data[1]]))
                } else {
                    return Err("Invalid si16 data".into());
                }
            }
            "flag" => {
                // Boolean flag
                if !data.is_empty() {
                    SMCValue::Flag(data[0] != 0)
                } else {
                    return Err("Invalid flag data".into());
                }
            }
            "ch8*" => {
                // 8-character string
                let end = data
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(8.min(data.len()));
                SMCValue::String(String::from_utf8_lossy(&data[..end]).to_string())
            }
            "{fds" => {
                // Fan descriptor struct
                if data.len() >= 16 {
                    // Parse fan descriptor (format may vary)
                    SMCValue::Bytes(data.clone())
                } else {
                    return Err("Invalid fan descriptor".into());
                }
            }
            _ => {
                // Unknown type - return raw bytes
                SMCValue::Bytes(data.clone())
            }
        };

        Ok(value)
    }

    pub fn read_float(&mut self, key: &str) -> Result<f32, Box<dyn std::error::Error>> {
        match self.read_value(key)? {
            SMCValue::Float(f) => Ok(f),
            SMCValue::U8(v) => Ok(v as f32),
            SMCValue::U16(v) => Ok(v as f32),
            SMCValue::U32(v) => Ok(v as f32),
            SMCValue::I8(v) => Ok(v as f32),
            SMCValue::I16(v) => Ok(v as f32),
            _ => Err(format!("Key {} cannot be converted to float", key).into()),
        }
    }

    pub fn read_temperature(&mut self, key: &str) -> Result<f32, Box<dyn std::error::Error>> {
        // Use dynamic type detection - temperatures can be float or fixed point
        self.read_float(key)
    }

    pub fn discover_temperature_sensors(
        &mut self,
    ) -> Result<(Vec<String>, Vec<String>), Box<dyn std::error::Error>> {
        let mut cpu_sensors = Vec::new();
        let mut gpu_sensors = Vec::new();

        // Try to read all keys
        let all_keys = self.read_all_keys().unwrap_or_else(|_| {
            // Fall back to known keys if discovery fails
            vec![
                "Te04", "Te05", "Te06", "Te0K", "Te0L", "Te0M", "Te0P", "Te0Q", "Te0S", "Te0T",
                "Tp04", "Tp05", "Tp06", "Tp0C", "Tp0D", "Tp0E", "Tp0K", "Tp0L", "Tp0M", "Tp0R",
                "Tg03", "Tg04", "Tg05", "Tg08", "Tg0L", "Tg0M",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect()
        });

        for key in &all_keys {
            // Temperature sensors typically start with T and have numeric values
            if key.starts_with("T") {
                // Try to read as temperature (will handle any numeric type)
                if let Ok(temp) = self.read_temperature(key) {
                    // Sanity check for temperature range
                    if temp > -50.0 && temp < 150.0 {
                        // Categorize by prefix
                        if key.starts_with("Tp") || key.starts_with("Te") {
                            cpu_sensors.push(key.clone());
                        } else if key.starts_with("Tg") {
                            gpu_sensors.push(key.clone());
                        }
                        // Could add more categories: Tm (memory), Tb (battery), etc.
                    }
                }
            }
        }

        Ok((cpu_sensors, gpu_sensors))
    }

    pub fn get_cpu_temperature(&mut self) -> Result<f32, Box<dyn std::error::Error>> {
        // Use discovered sensors or fall back to known ones
        let (cpu_sensors, _) = self.discover_temperature_sensors()?;
        let mut temps = Vec::new();

        for key in &cpu_sensors {
            match self.read_temperature(key) {
                Ok(temp) if temp > 0.0 && temp < 150.0 => temps.push(temp),
                _ => {}
            }
        }

        if temps.is_empty() {
            Err("Could not read CPU temperature".into())
        } else {
            // Return average of all CPU sensors
            Ok(temps.iter().sum::<f32>() / temps.len() as f32)
        }
    }

    pub fn get_gpu_temperature(&mut self) -> Result<f32, Box<dyn std::error::Error>> {
        // Use discovered sensors or fall back to known ones
        let (_, gpu_sensors) = self.discover_temperature_sensors()?;
        let mut temps = Vec::new();

        for key in &gpu_sensors {
            match self.read_temperature(key) {
                Ok(temp) if temp > 0.0 && temp < 150.0 => temps.push(temp),
                _ => {}
            }
        }

        if temps.is_empty() {
            Err("Could not read GPU temperature".into())
        } else {
            // Return average of all GPU sensors
            Ok(temps.iter().sum::<f32>() / temps.len() as f32)
        }
    }

    pub fn get_all_temperatures(&mut self) -> Vec<(String, f32)> {
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
            if let Ok(temp) = self.read_temperature(key)
                && temp > 0.0
                && temp < 150.0
            {
                temps.push((description.to_string(), temp));
            }
        }

        temps
    }

    // Power metrics
    pub fn get_power_metrics(&mut self) -> PowerMetrics {
        PowerMetrics {
            system_power: self.read_float("PSTR").ok(),
            cpu_power: None,    // Would need IOReport for accurate CPU power
            gpu_power: None,    // Would need IOReport for accurate GPU power
            memory_power: None, // Would need IOReport for accurate memory power
        }
    }

    // Fan metrics
    pub fn get_fan_metrics(&mut self) -> FanMetrics {
        let mut fans = Vec::new();

        // Check for up to 10 fans (most Macs have 0-2)
        for i in 0..10 {
            let prefix = format!("F{}", i);
            let ac_key = format!("{}Ac", prefix);

            // Check if this fan exists
            if let Ok(actual_rpm) = self.read_float(&ac_key) {
                let fan = FanInfo {
                    id: i,
                    actual_rpm: Some(actual_rpm),
                    minimum_rpm: self.read_float(&format!("{}Mn", prefix)).ok(),
                    maximum_rpm: self.read_float(&format!("{}Mx", prefix)).ok(),
                    target_rpm: self.read_float(&format!("{}Tg", prefix)).ok(),
                };
                fans.push(fan);
            }
        }

        FanMetrics { fans }
    }

    // Generic helper function to read values in little-endian format
    // (Some SMC keys like battery-related ones use little-endian instead of big-endian)
    pub fn read_le<T>(&mut self, key: &str) -> Result<T, Box<dyn std::error::Error>>
    where
        T: FromLeBytes,
    {
        let info = self.read_key_info(key)?;
        let data = self.read_key_data(key, &info)?;

        T::from_le_bytes(&data, key)
    }

    // Battery metrics with dynamic type handling and proper unit conversion
    pub fn get_battery_metrics(&mut self) -> BatteryMetrics {
        // Battery capacity - read as little-endian u16, usually in mAh
        let current_capacity = self.read_le::<u16>("B0CC").ok().map(|v| v as f32);
        let full_charge_capacity = self.read_le::<u16>("B0FC").ok().map(|v| v as f32);

        let health_percent = match (current_capacity, full_charge_capacity) {
            (Some(cc), Some(fc)) if fc > 0.0 => Some((cc / fc) * 100.0),
            _ => None,
        };

        // Battery voltage - read as little-endian u16, convert from mV to V
        let voltage = self.read_le::<u16>("B0AV").ok().map(|v| v as f32 / 1000.0);

        // Battery current - read as little-endian i16, convert from mA to A
        let current = self.read_le::<i16>("B0AC").ok().map(|v| v as f32 / 1000.0);

        BatteryMetrics {
            current_capacity,
            full_charge_capacity,
            voltage,
            current,
            temperature: self
                .read_temperature("TB0T")
                .ok()
                .or_else(|| self.read_temperature("B0TE").ok()),
            cycle_count: {
                // B0CT (battery cycle count) is stored in little-endian format
                // unlike most other SMC values which are big-endian
                self.read_le::<u16>("B0CT").ok().map(|v| v as u32)
            },
            health_percent,
        }
    }

    // Voltage metrics
    pub fn get_voltage_metrics(&mut self) -> VoltageMetrics {
        let mut cpu_voltages = Vec::new();
        let mut gpu_voltages = Vec::new();

        // CPU voltages (VC00-VC43)
        for i in 0..44 {
            let key = format!("VC{:02}", i);
            if let Ok(voltage) = self.read_float(&key) {
                cpu_voltages.push((key, voltage));
            }
        }

        // GPU voltages (VG0*)
        for i in 0..10 {
            let key = format!("VG0{}", i);
            if let Ok(voltage) = self.read_float(&key) {
                gpu_voltages.push((key, voltage));
            }
        }

        VoltageMetrics {
            cpu_voltages,
            gpu_voltages,
            memory_voltage: self.read_float("VDMM").ok(),
        }
    }

    // Current metrics
    pub fn get_current_metrics(&mut self) -> CurrentMetrics {
        let mut cpu_currents = Vec::new();
        let mut gpu_currents = Vec::new();

        // CPU currents (IC00-IC43)
        for i in 0..44 {
            let key = format!("IC{:02}", i);
            if let Ok(current) = self.read_float(&key) {
                cpu_currents.push((key, current));
            }
        }

        // GPU currents (IG0*)
        for i in 0..10 {
            let key = format!("IG0{}", i);
            if let Ok(current) = self.read_float(&key) {
                gpu_currents.push((key, current));
            }
        }

        CurrentMetrics {
            cpu_currents,
            gpu_currents,
            battery_current: self.read_le::<i16>("B0AC").ok().map(|v| v as f32 / 1000.0),
        }
    }

    // Get all comprehensive metrics
    pub fn get_comprehensive_metrics(&mut self) -> ComprehensiveSMCMetrics {
        ComprehensiveSMCMetrics {
            temperature: TemperatureMetrics {
                cpu_temp: self.get_cpu_temperature().ok(),
                gpu_temp: self.get_gpu_temperature().ok(),
                sensors: self.get_all_temperatures(),
            },
            power: self.get_power_metrics(),
            fans: self.get_fan_metrics(),
            battery: self.get_battery_metrics(),
            voltage: self.get_voltage_metrics(),
            current: self.get_current_metrics(),
        }
    }

    // Debug method to read ALL SMC keys with their raw values
    pub fn get_all_smc_data(&mut self) -> Result<SmcDebugData, Box<dyn std::error::Error>> {
        let num_keys = self.read_num_keys()?;
        let mut keys_data = Vec::new();

        // Read all available keys
        for i in 0..num_keys {
            match self.read_key_by_index(i) {
                Ok(key) => {
                    // Filter out invalid keys
                    if !key.chars().all(|c| c.is_ascii_graphic()) || key.len() != 4 {
                        continue;
                    }

                    // Try to read key info and data
                    let mut key_data = SmcKeyData {
                        key: key.clone(),
                        type_str: String::new(),
                        size: 0,
                        value: None,
                        raw_bytes: Vec::new(),
                        error: None,
                    };

                    match self.read_key_info(&key) {
                        Ok(info) => {
                            // Convert type to string
                            let type_bytes = info.data_type.to_be_bytes();
                            key_data.type_str = String::from_utf8_lossy(&type_bytes).to_string();
                            key_data.size = info.data_size;

                            // Try to read the raw data
                            match self.read_key_data(&key, &info) {
                                Ok(data) => {
                                    key_data.raw_bytes = data.clone();

                                    // Try to parse the value
                                    match self.read_value(&key) {
                                        Ok(value) => {
                                            key_data.value = Some(match value {
                                                SMCValue::Float(f) => SmcDebugValue::Float(f),
                                                SMCValue::U8(v) => SmcDebugValue::U8(v),
                                                SMCValue::U16(v) => SmcDebugValue::U16(v),
                                                SMCValue::U32(v) => SmcDebugValue::U32(v),
                                                SMCValue::I8(v) => SmcDebugValue::I8(v),
                                                SMCValue::I16(v) => SmcDebugValue::I16(v),
                                                SMCValue::Flag(b) => SmcDebugValue::Bool(b),
                                                SMCValue::String(s) => SmcDebugValue::String(s),
                                                SMCValue::Bytes(b) => SmcDebugValue::Bytes(b),
                                            });
                                        }
                                        Err(e) => {
                                            key_data.error = Some(format!("Parse error: {}", e));
                                        }
                                    }
                                }
                                Err(e) => {
                                    key_data.error = Some(format!("Read error: {}", e));
                                }
                            }
                        }
                        Err(e) => {
                            key_data.error = Some(format!("Info error: {}", e));
                        }
                    }

                    keys_data.push(key_data);
                }
                Err(_) => {}
            }
        }

        // Sort keys alphabetically for easier reading
        keys_data.sort_by(|a, b| a.key.cmp(&b.key));

        Ok(SmcDebugData {
            total_keys: num_keys,
            keys: keys_data,
        })
    }
}

impl Drop for Smc {
    fn drop(&mut self) {
        if self.connection != 0 {
            unsafe {
                IOServiceClose(self.connection);
            }
        }
    }
}

// Public interface for SMC metrics
#[derive(Debug, Clone, serde::Serialize)]
pub struct TemperatureMetrics {
    pub cpu_temp: Option<f32>,
    pub gpu_temp: Option<f32>,
    pub sensors: Vec<(String, f32)>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PowerMetrics {
    pub system_power: Option<f32>, // PSTR - total system power in watts
    pub cpu_power: Option<f32>,    // Various PC** keys
    pub gpu_power: Option<f32>,    // PG** keys
    pub memory_power: Option<f32>, // PM** keys
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FanMetrics {
    pub fans: Vec<FanInfo>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FanInfo {
    pub id: u8,
    pub actual_rpm: Option<f32>,  // F*Ac
    pub minimum_rpm: Option<f32>, // F*Mn
    pub maximum_rpm: Option<f32>, // F*Mx
    pub target_rpm: Option<f32>,  // F*Tg
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BatteryMetrics {
    pub current_capacity: Option<f32>,     // B0CC
    pub full_charge_capacity: Option<f32>, // B0FC
    pub voltage: Option<f32>,              // B0AV
    pub current: Option<f32>,              // B0AC
    pub temperature: Option<f32>,          // B0TE or TB0T
    pub cycle_count: Option<u32>,          // B0CT
    pub health_percent: Option<f32>,       // Calculated from FC/DC
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VoltageMetrics {
    pub cpu_voltages: Vec<(String, f32)>, // VC** keys
    pub gpu_voltages: Vec<(String, f32)>, // VG** keys
    pub memory_voltage: Option<f32>,      // VDMM
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CurrentMetrics {
    pub cpu_currents: Vec<(String, f32)>, // IC** keys
    pub gpu_currents: Vec<(String, f32)>, // IG** keys
    pub battery_current: Option<f32>,     // B0AC
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ComprehensiveSMCMetrics {
    pub temperature: TemperatureMetrics,
    pub power: PowerMetrics,
    pub fans: FanMetrics,
    pub battery: BatteryMetrics,
    pub voltage: VoltageMetrics,
    pub current: CurrentMetrics,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SmcDebugData {
    pub total_keys: u32,
    pub keys: Vec<SmcKeyData>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SmcKeyData {
    pub key: String,
    pub type_str: String,
    pub size: u32,
    pub value: Option<SmcDebugValue>,
    pub raw_bytes: Vec<u8>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(untagged)]
pub enum SmcDebugValue {
    Float(f32),
    U8(u8),
    U16(u16),
    U32(u32),
    I8(i8),
    I16(i16),
    Bool(bool),
    String(String),
    Bytes(Vec<u8>),
}

pub fn get_temperature_metrics() -> Result<TemperatureMetrics, Box<dyn std::error::Error>> {
    let mut smc = match Smc::new() {
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

pub fn get_comprehensive_smc_metrics() -> Result<ComprehensiveSMCMetrics, Box<dyn std::error::Error>>
{
    let mut smc = Smc::new()?;
    Ok(smc.get_comprehensive_metrics())
}

pub fn get_all_smc_debug_data() -> Result<SmcDebugData, Box<dyn std::error::Error>> {
    let mut smc = Smc::new()?;
    smc.get_all_smc_data()
}

pub fn get_smc_connection() -> Result<Smc, Box<dyn std::error::Error>> {
    Smc::new()
}
