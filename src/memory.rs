use serde::Serialize;
use std::mem;

#[derive(Debug, Default, Serialize)]
pub struct MemoryMetrics {
    pub total: u64,      // total memory (ram + swap) in bytes
    pub ram_total: u64,  // bytes
    pub ram_usage: u64,  // bytes
    pub swap_total: u64, // bytes
    pub swap_usage: u64, // bytes
}

pub fn get_memory_metrics() -> Result<MemoryMetrics, Box<dyn std::error::Error>> {
    let (ram_usage, ram_total) = get_ram_info()?;
    let (swap_usage, swap_total) = get_swap_info()?;
    
    Ok(MemoryMetrics {
        total: ram_total + swap_total,
        ram_total,
        ram_usage,
        swap_total,
        swap_usage,
    })
}

fn get_ram_info() -> Result<(u64, u64), Box<dyn std::error::Error>> {
    let mut total = 0u64;

    // Get total physical memory using sysctl
    unsafe {
        let mut name = [libc::CTL_HW, libc::HW_MEMSIZE];
        let mut size = mem::size_of::<u64>();
        let ret_code = libc::sysctl(
            name.as_mut_ptr(),
            name.len() as _,
            &mut total as *mut _ as *mut _,
            &mut size,
            std::ptr::null_mut(),
            0,
        );

        if ret_code != 0 {
            return Err("Failed to get total memory".into());
        }
    }

    // Get memory usage statistics
    let usage = unsafe {
        let mut count: u32 = libc::HOST_VM_INFO64_COUNT as _;
        let mut stats = mem::zeroed::<libc::vm_statistics64>();

        #[allow(deprecated)]
        let ret_code = libc::host_statistics64(
            libc::mach_host_self(),
            libc::HOST_VM_INFO64,
            &mut stats as *mut _ as *mut _,
            &mut count,
        );

        if ret_code != 0 {
            return Err("Failed to get memory stats".into());
        }

        let page_size_bytes = libc::sysconf(libc::_SC_PAGESIZE) as u64;

        // Calculate used memory following macmon's formula
        // This includes active, inactive, wired, speculative, and compressed pages
        // but excludes purgeable and external pages
        (stats.active_count as u64
            + stats.inactive_count as u64
            + stats.wire_count as u64
            + stats.speculative_count as u64
            + stats.compressor_page_count as u64
            - stats.purgeable_count as u64
            - stats.external_page_count as u64)
            * page_size_bytes
    };

    Ok((usage, total))
}

fn get_swap_info() -> Result<(u64, u64), Box<dyn std::error::Error>> {
    unsafe {
        let mut name = [libc::CTL_VM, libc::VM_SWAPUSAGE];
        let mut size = mem::size_of::<libc::xsw_usage>();
        let mut xsw: libc::xsw_usage = mem::zeroed::<libc::xsw_usage>();

        let ret_code = libc::sysctl(
            name.as_mut_ptr(),
            name.len() as _,
            &mut xsw as *mut _ as *mut _,
            &mut size,
            std::ptr::null_mut(),
            0,
        );

        if ret_code != 0 {
            return Err("Failed to get swap usage".into());
        }

        Ok((xsw.xsu_used, xsw.xsu_total))
    }
}