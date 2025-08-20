// Utilities: CoreFoundation helpers and sysctl helpers consolidated

// ===== CoreFoundation helpers =====
use core_foundation::array::CFArrayRef;
use core_foundation::base::TCFType;
use core_foundation::data::CFDataRef;
use core_foundation::dictionary::CFDictionaryRef;
use core_foundation::string::{CFString, CFStringGetCString, CFStringRef, kCFStringEncodingUTF8};
use core_foundation_sys::dictionary::CFDictionaryGetValue;

/// Create a CoreFoundation string from a Rust &str (owned CFString)
pub fn cf_string(val: &str) -> CFString {
    CFString::new(val)
}

/// Convert CFStringRef to Rust String (lossy). Returns empty string on failure.
pub fn cf_string_to_rust(cf_str: CFStringRef) -> String {
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

/// Get a CFArray value from a CFDictionary by key. Returns Err if key missing.
pub fn cf_dict_get_array(
    dict: CFDictionaryRef,
    key: &str,
) -> Result<CFArrayRef, Box<dyn std::error::Error>> {
    unsafe {
        let k = CFString::new(key);
        let val = CFDictionaryGetValue(dict, k.as_CFTypeRef());
        if val.is_null() {
            Err(format!("Key '{}' not found in CFDictionary", key).into())
        } else {
            Ok(val as CFArrayRef)
        }
    }
}

/// Get a CFData value from a CFDictionary by key. Returns Err if key missing.
pub fn cf_dict_get_data(
    dict: CFDictionaryRef,
    key: &str,
) -> Result<CFDataRef, Box<dyn std::error::Error>> {
    unsafe {
        let k = CFString::new(key);
        let val = CFDictionaryGetValue(dict, k.as_CFTypeRef());
        if val.is_null() {
            Err(format!("Key '{}' not found in CFDictionary", key).into())
        } else {
            Ok(val as CFDataRef)
        }
    }
}

// ===== sysctl helpers =====
use std::ffi::CString;

/// Read a sysctl value as raw bytes using sysctlbyname
#[allow(dead_code)]
pub fn sysctl_bytes(name: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    unsafe {
        let cname = CString::new(name)?;
        let mut size: libc::size_t = 0;
        let ret = libc::sysctlbyname(
            cname.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        );
        if ret != 0 || size == 0 {
            return Err(format!("sysctlbyname probe failed for {}", name).into());
        }

        let mut buf = vec![0u8; size as usize];
        let ret2 = libc::sysctlbyname(
            cname.as_ptr(),
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        );
        if ret2 != 0 {
            return Err(format!("sysctlbyname read failed for {}", name).into());
        }
        Ok(buf)
    }
}

/// Read a sysctl value as UTF-8 string (strips trailing NUL if present)
#[allow(dead_code)]
pub fn sysctl_string(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut bytes = sysctl_bytes(name)?;
    if let Some(pos) = bytes.iter().position(|&b| b == 0) {
        bytes.truncate(pos);
    }
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

/// Read a sysctl numeric value (u64) via sysctlbyname
#[allow(dead_code)]
pub fn sysctl_u64(name: &str) -> Result<u64, Box<dyn std::error::Error>> {
    let bytes = sysctl_bytes(name)?;
    if bytes.len() == std::mem::size_of::<u64>() {
        let mut raw = [0u8; 8];
        raw.copy_from_slice(&bytes);
        Ok(u64::from_ne_bytes(raw))
    } else if bytes.len() == std::mem::size_of::<u32>() {
        let mut raw = [0u8; 4];
        raw.copy_from_slice(&bytes[..4]);
        Ok(u32::from_ne_bytes(raw) as u64)
    } else {
        Err(format!("unexpected size {} for sysctl {}", bytes.len(), name).into())
    }
}
