use crate::utils::{cf_dict_get_array, cf_string, cf_string_to_rust};
use core_foundation::array::{CFArrayGetCount, CFArrayGetValueAtIndex};
use core_foundation::base::{CFRelease, CFTypeRef, TCFType, kCFAllocatorDefault};
use core_foundation::dictionary::{
    CFDictionaryCreateMutableCopy, CFDictionaryGetCount, CFDictionaryRef, CFMutableDictionaryRef,
};
#[allow(unused_imports)]
use core_foundation::number::{CFNumberCreate, CFNumberRef, kCFNumberSInt32Type};
use core_foundation::string::CFStringRef;
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

fn get_residencies(item: CFDictionaryRef) -> Vec<(String, i64)> {
    let count = unsafe { IOReportStateGetCount(item) };
    let mut res = vec![];

    for i in 0..count {
        let name = unsafe { IOReportStateGetNameForIndex(item, i) };
        let val = unsafe { IOReportStateGetResidency(item, i) };
        res.push((cf_string_to_rust(name), val));
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

pub struct IOReportPerf {
    subscription: IOReportSubscriptionRef,
    channel_dictionary: CFMutableDictionaryRef,
}

impl IOReportPerf {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Create channels for CPU and GPU performance states
        let channels = vec![
            ("CPU Stats", Some("CPU Core Performance States")),
            ("GPU Stats", Some("GPU Performance States")),
        ];

        let channel_dictionary = create_channels(channels)?;
        let subscription = create_subscription(channel_dictionary)?;

        Ok(Self {
            subscription,
            channel_dictionary,
        })
    }

    /// Get a single sample of performance metrics
    pub fn get_sample(&self, duration_ms: u64) -> PerformanceSample {
        unsafe {
            // Take two samples with specified duration between them
            let sample1 = IOReportCreateSamples(self.subscription, self.channel_dictionary, null());
            std::thread::sleep(std::time::Duration::from_millis(duration_ms));
            let sample2 = IOReportCreateSamples(self.subscription, self.channel_dictionary, null());

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
            CFRelease(self.channel_dictionary as _);
            CFRelease(self.subscription as _);
        }
    }
}

fn create_channels(
    items: Vec<(&str, Option<&str>)>,
) -> Result<CFMutableDictionaryRef, Box<dyn std::error::Error>> {
    let mut channels = vec![];

    for (group, subgroup) in items {
        let gname = cf_string(group);
        let sname_opt = subgroup.map(cf_string);
        let sname_ref = sname_opt
            .as_ref()
            .map(|s| s.as_concrete_TypeRef())
            .unwrap_or(null());
        let chan =
            unsafe { IOReportCopyChannelsInGroup(gname.as_concrete_TypeRef(), sname_ref, 0, 0, 0) };
        channels.push(chan);
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

    if cf_dict_get_array(chan, "IOReportChannels").is_err() {
        return Err("Failed to get channels".into());
    }

    Ok(chan)
}

fn create_subscription(
    channel_dictionary: CFMutableDictionaryRef,
) -> Result<IOReportSubscriptionRef, Box<dyn std::error::Error>> {
    let mut dict: std::mem::MaybeUninit<CFMutableDictionaryRef> = std::mem::MaybeUninit::uninit();
    let subs = unsafe {
        IOReportCreateSubscription(null(), channel_dictionary, dict.as_mut_ptr(), 0, null())
    };

    if subs.is_null() {
        return Err("Failed to create subscription".into());
    }

    unsafe { dict.assume_init() };
    Ok(subs)
}

#[derive(Debug, Default, Clone)]
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
    if let Ok(items) = cf_dict_get_array(data, "IOReportChannels") {
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
        cf_string_to_rust(group)
    }
}

fn get_channel_subgroup(item: CFDictionaryRef) -> String {
    let subgroup = unsafe { IOReportChannelGetSubGroup(item) };
    if subgroup.is_null() {
        String::new()
    } else {
        cf_string_to_rust(subgroup)
    }
}

fn get_channel_name(item: CFDictionaryRef) -> String {
    let name = unsafe { IOReportChannelGetChannelName(item) };
    if name.is_null() {
        String::new()
    } else {
        cf_string_to_rust(name)
    }
}
