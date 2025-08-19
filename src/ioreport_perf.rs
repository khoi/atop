use core_foundation::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
use core_foundation::base::{CFRelease, CFTypeRef, kCFAllocatorDefault, kCFAllocatorNull};
use core_foundation::dictionary::{
    CFDictionaryCreateMutableCopy, CFDictionaryGetCount, CFDictionaryGetValue, CFDictionaryRef,
    CFMutableDictionaryRef,
};
#[allow(unused_imports)]
use core_foundation::number::{CFNumberCreate, CFNumberRef, kCFNumberSInt32Type};
use core_foundation::string::{
    CFStringCreateWithBytesNoCopy, CFStringGetCString, CFStringRef, kCFStringEncodingUTF8,
};
use std::ffi::c_void;
use std::ptr::null;

// ==============================================================================
// IOReport FFI Bindings
// ==============================================================================

#[repr(C)]
struct IOReportSubscription {
    _data: [u8; 0],
    _phantom: std::marker::PhantomData<(*mut u8, std::marker::PhantomPinned)>,
}

type IOReportSubscriptionRef = *const IOReportSubscription;

#[link(name = "IOReport", kind = "dylib")]
unsafe extern "C" {
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
    fn IOReportStateGetCount(a: CFDictionaryRef) -> i32;
    fn IOReportStateGetNameForIndex(a: CFDictionaryRef, b: i32) -> CFStringRef;
    fn IOReportStateGetResidency(a: CFDictionaryRef, b: i32) -> i64;
}

// ==============================================================================
// Helper Functions
// ==============================================================================

#[allow(dead_code)]
fn cfnum(val: i32) -> CFNumberRef {
    unsafe {
        CFNumberCreate(
            kCFAllocatorDefault,
            kCFNumberSInt32Type,
            &val as *const i32 as _,
        )
    }
}

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

fn from_cfstr(val: CFStringRef) -> String {
    unsafe {
        let mut buf = Vec::with_capacity(128);
        if CFStringGetCString(val, buf.as_mut_ptr(), 128, kCFStringEncodingUTF8) == 0 {
            return String::new();
        }
        std::ffi::CStr::from_ptr(buf.as_ptr())
            .to_string_lossy()
            .to_string()
    }
}

fn cfdict_get_val(dict: CFDictionaryRef, key: &str) -> Option<CFTypeRef> {
    unsafe {
        let key = cfstr(key);
        let val = CFDictionaryGetValue(dict, key as _);
        CFRelease(key as _);

        if val.is_null() { None } else { Some(val) }
    }
}

// ==============================================================================
// Performance State Residency Analysis
// ==============================================================================

fn get_residencies(item: CFDictionaryRef) -> Vec<(String, i64)> {
    let count = unsafe { IOReportStateGetCount(item) };
    let mut res = vec![];

    for i in 0..count {
        let name = unsafe { IOReportStateGetNameForIndex(item, i) };
        let val = unsafe { IOReportStateGetResidency(item, i) };
        res.push((from_cfstr(name), val));
    }

    res
}

/// Calculate frequency and utilization from performance state residencies
fn calc_freq(item: CFDictionaryRef, freqs: &[u32]) -> (u32, f32) {
    let items = get_residencies(item);

    // Find the first active state (skip IDLE/DOWN/OFF states)
    let offset = items
        .iter()
        .position(|x| x.0 != "IDLE" && x.0 != "DOWN" && x.0 != "OFF")
        .unwrap_or(0);

    // Calculate total active time and overall time
    let usage = items.iter().skip(offset).map(|x| x.1 as f64).sum::<f64>();
    let total = items.iter().map(|x| x.1 as f64).sum::<f64>();

    if usage == 0.0 || total == 0.0 || freqs.is_empty() {
        return (0, 0.0);
    }

    // Calculate weighted average frequency
    let mut avg_freq = 0f64;
    for i in 0..freqs.len().min(items.len() - offset) {
        let percent = items[i + offset].1 as f64 / usage;
        avg_freq += percent * freqs[i] as f64;
    }

    // Calculate utilization percentage
    let usage_ratio = usage / total;
    let min_freq = *freqs.first().unwrap() as f64;
    let max_freq = *freqs.last().unwrap() as f64;
    let from_max = (avg_freq.max(min_freq) * usage_ratio) / max_freq;

    (avg_freq as u32, from_max as f32)
}

// ==============================================================================
// IOReport Performance Monitor
// ==============================================================================

pub struct IOReportPerf {
    subs: IOReportSubscriptionRef,
    chan: CFMutableDictionaryRef,
}

impl IOReportPerf {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Create channels for CPU and GPU performance states
        let channels = vec![
            ("CPU Stats", Some("CPU Core Performance States")),
            ("GPU Stats", Some("GPU Performance States")),
        ];

        let chan = create_channels(channels)?;
        let subs = create_subscription(chan)?;

        Ok(Self { subs, chan })
    }

    /// Get a single sample of performance metrics
    pub fn get_sample(&self, duration_ms: u64) -> PerformanceSample {
        unsafe {
            // Take two samples with specified duration between them
            let sample1 = IOReportCreateSamples(self.subs, self.chan, null());
            std::thread::sleep(std::time::Duration::from_millis(duration_ms));
            let sample2 = IOReportCreateSamples(self.subs, self.chan, null());

            // Calculate delta between samples
            let delta = IOReportCreateSamplesDelta(sample1, sample2, null());
            CFRelease(sample1 as _);
            CFRelease(sample2 as _);

            let sample = parse_sample(delta);
            CFRelease(delta as _);
            sample
        }
    }
}

impl Drop for IOReportPerf {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.chan as _);
            CFRelease(self.subs as _);
        }
    }
}

// ==============================================================================
// Channel Creation and Subscription
// ==============================================================================

fn create_channels(
    items: Vec<(&str, Option<&str>)>,
) -> Result<CFMutableDictionaryRef, Box<dyn std::error::Error>> {
    let mut channels = vec![];

    for (group, subgroup) in items {
        let gname = cfstr(group);
        let sname = subgroup.map_or(null(), cfstr);
        let chan = unsafe { IOReportCopyChannelsInGroup(gname, sname, 0, 0, 0) };
        channels.push(chan);

        unsafe { CFRelease(gname as _) };
        if subgroup.is_some() {
            unsafe { CFRelease(sname as _) };
        }
    }

    if channels.is_empty() {
        return Err("No channels found".into());
    }

    // Merge all channels into the first one
    let chan = channels[0];
    for &channel in channels.iter().skip(1) {
        unsafe { IOReportMergeChannels(chan, channel, null()) };
    }

    let size = unsafe { CFDictionaryGetCount(chan) };
    let chan = unsafe { CFDictionaryCreateMutableCopy(kCFAllocatorDefault, size, chan) };

    for channel in channels {
        unsafe { CFRelease(channel as _) };
    }

    if cfdict_get_val(chan, "IOReportChannels").is_none() {
        return Err("Failed to get channels".into());
    }

    Ok(chan)
}

fn create_subscription(
    chan: CFMutableDictionaryRef,
) -> Result<IOReportSubscriptionRef, Box<dyn std::error::Error>> {
    let mut dict: std::mem::MaybeUninit<CFMutableDictionaryRef> = std::mem::MaybeUninit::uninit();
    let subs = unsafe { IOReportCreateSubscription(null(), chan, dict.as_mut_ptr(), 0, null()) };

    if subs.is_null() {
        return Err("Failed to create subscription".into());
    }

    unsafe { dict.assume_init() };
    Ok(subs)
}

// ==============================================================================
// Sample Parsing
// ==============================================================================

#[derive(Debug, Default)]
pub struct PerformanceSample {
    pub ecpu_usage: (u32, f32), // (freq_mhz, utilization_percent)
    pub pcpu_usage: (u32, f32), // (freq_mhz, utilization_percent)
    pub gpu_usage: (u32, f32),  // (freq_mhz, utilization_percent)
}

fn parse_sample(data: CFDictionaryRef) -> PerformanceSample {
    let mut sample = PerformanceSample::default();
    let mut ecpu_usages = Vec::new();
    let mut pcpu_usages = Vec::new();

    // Get CPU frequency lists from our existing cpu module
    let cpu_info = crate::cpu::get_cpu_info().unwrap_or_default();
    let ecpu_freqs = cpu_info.ecpu_freqs_mhz;
    let pcpu_freqs = cpu_info.pcpu_freqs_mhz;
    let gpu_freqs = crate::cpu::get_gpu_freqs().unwrap_or_default();

    // Parse IOReport channels
    if let Some(items) = cfdict_get_val(data, "IOReportChannels") {
        let items = items as CFArrayRef;
        let count = unsafe { CFArrayGetCount(items) };

        for i in 0..count {
            let item = unsafe { CFArrayGetValueAtIndex(items, i) } as CFDictionaryRef;

            let group = get_channel_group(item);
            let subgroup = get_channel_subgroup(item);
            let channel = get_channel_name(item);

            // CPU Core Performance States
            if group == "CPU Stats" && subgroup == "CPU Core Performance States" {
                if channel.contains("ECPU") {
                    ecpu_usages.push(calc_freq(item, &ecpu_freqs));
                } else if channel.contains("PCPU") {
                    pcpu_usages.push(calc_freq(item, &pcpu_freqs));
                }
            }

            // GPU Performance States
            if group == "GPU Stats"
                && subgroup == "GPU Performance States"
                && channel == "GPUPH"
                && !gpu_freqs.is_empty()
            {
                // Skip the first frequency (idle state)
                sample.gpu_usage = calc_freq(item, &gpu_freqs[1..]);
            }
        }
    }

    // Average the per-core measurements
    if !ecpu_usages.is_empty() {
        let avg_freq =
            ecpu_usages.iter().map(|x| x.0 as f32).sum::<f32>() / ecpu_usages.len() as f32;
        let avg_util = ecpu_usages.iter().map(|x| x.1).sum::<f32>() / ecpu_usages.len() as f32;
        sample.ecpu_usage = (avg_freq as u32, avg_util);
    }

    if !pcpu_usages.is_empty() {
        let avg_freq =
            pcpu_usages.iter().map(|x| x.0 as f32).sum::<f32>() / pcpu_usages.len() as f32;
        let avg_util = pcpu_usages.iter().map(|x| x.1).sum::<f32>() / pcpu_usages.len() as f32;
        sample.pcpu_usage = (avg_freq as u32, avg_util);
    }

    sample
}

fn get_channel_group(item: CFDictionaryRef) -> String {
    let group = unsafe { IOReportChannelGetGroup(item) };
    if group.is_null() {
        String::new()
    } else {
        from_cfstr(group)
    }
}

fn get_channel_subgroup(item: CFDictionaryRef) -> String {
    let subgroup = unsafe { IOReportChannelGetSubGroup(item) };
    if subgroup.is_null() {
        String::new()
    } else {
        from_cfstr(subgroup)
    }
}

fn get_channel_name(item: CFDictionaryRef) -> String {
    let name = unsafe { IOReportChannelGetChannelName(item) };
    if name.is_null() {
        String::new()
    } else {
        from_cfstr(name)
    }
}
