mod cpu;
mod iokit;
mod ioreport_perf;
mod memory;
mod smc;
mod utils;

use cpu::CpuMetrics;
use iokit::PowerMetrics;
use ioreport_perf::IOReportPerf;
use memory::MemoryMetrics;
use serde::Serialize;
use smc::SmcDebugValue;
use std::env;

// Sampler struct to hold cached resources
struct FastSampler {
    cpu_metrics: CpuMetrics,
    perf_monitor: Option<IOReportPerf>,
}

impl FastSampler {
    fn new() -> Result<Self, String> {
        let cpu_metrics = cpu::get_cpu_metrics()
            .map_err(|e| format!("Error getting CPU metrics: {}", e))?;
        
        let perf_monitor = IOReportPerf::new().ok();
        
        Ok(Self {
            cpu_metrics,
            perf_monitor,
        })
    }
    
    fn sample(&self, interval_ms: u32) -> Result<SystemMetrics, String> {
        // Get real memory metrics (dynamic)
        let memory_metrics = memory::get_memory_metrics()
            .map_err(|e| format!("Error getting memory metrics: {}", e))?;
        
        // Use cached CPU metrics
        let cpu_metrics = self.cpu_metrics.clone();
        
        // Get power metrics with the same interval (no SMC fallback)
        let power_metrics = iokit::get_power_metrics_with_interval(None, interval_ms as u64).ok();
        
        // Get performance metrics using cached monitor
        let perf_sample = self.perf_monitor.as_ref()
            .map(|monitor| monitor.get_sample(interval_ms as u64));
        
        Ok(SystemMetrics {
            memory: memory_metrics,
            cpu: cpu_metrics,
            power: power_metrics,
            ecpu_usage: perf_sample.as_ref().map(|p| p.ecpu_usage),
            pcpu_usage: perf_sample.as_ref().map(|p| p.pcpu_usage),
            gpu_usage: perf_sample.as_ref().map(|p| p.gpu_usage),
            unix_time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        })
    }
}

#[derive(Serialize)]
struct SystemMetrics {
    memory: MemoryMetrics,
    cpu: CpuMetrics,
    power: Option<PowerMetrics>,
    ecpu_usage: Option<(u32, f32)>,
    pcpu_usage: Option<(u32, f32)>,
    gpu_usage: Option<(u32, f32)>,
    unix_time: u64,
}

fn print_usage() {
    eprintln!("Usage: atop [OPTIONS]");
    eprintln!();
    eprintln!("System memory metrics monitoring tool");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("    --json               Output as JSON");
    eprintln!("    --sample, -s <N>     Number of samples to collect (0 = infinite, only with --json)");
    eprintln!("    --interval, -i <MS>  Update interval in milliseconds (default: 1000, min: 100)");
    eprintln!("    --smc                Show ALL SMC data for debugging (includes raw values)");
    eprintln!("    --smc-nice           Show formatted SMC metrics (power, fans, battery, etc.)");
    eprintln!("    --help               Print this help message");
}

fn collect_metrics(interval_ms: u32) -> Result<SystemMetrics, String> {
    collect_metrics_internal(interval_ms, false)
}

fn collect_metrics_internal(interval_ms: u32, _skip_smc: bool) -> Result<SystemMetrics, String> {
    // Get real memory metrics
    let memory_metrics = memory::get_memory_metrics()
        .map_err(|e| format!("Error getting memory metrics: {}", e))?;

    // Get CPU metrics
    let cpu_metrics = cpu::get_cpu_metrics()
        .map_err(|e| format!("Error getting CPU metrics: {}", e))?;

    // Get power metrics (no SMC fallback anymore)
    let power_metrics = iokit::get_power_metrics_with_interval(None, interval_ms as u64).ok();

    // Get performance metrics (CPU/GPU frequency and utilization)
    let perf_sample = if let Ok(perf_monitor) = IOReportPerf::new() {
        // Sample for the specified interval to get accurate readings
        Some(perf_monitor.get_sample(interval_ms as u64))
    } else {
        None
    };

    Ok(SystemMetrics {
        memory: memory_metrics,
        cpu: cpu_metrics,
        power: power_metrics,
        ecpu_usage: perf_sample.as_ref().map(|p| p.ecpu_usage),
        pcpu_usage: perf_sample.as_ref().map(|p| p.pcpu_usage),
        gpu_usage: perf_sample.as_ref().map(|p| p.gpu_usage),
        unix_time: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    })
}

fn main() {
    let args: Vec<String> = env::args().collect();

    // Parse arguments
    let mut json_output = false;
    let mut debug_smc = false;
    let mut nice_smc = false;
    let mut sample_count: Option<u32> = None;
    let mut interval_ms: u32 = 1000; // Default 1 second

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json_output = true,
            "--smc" => debug_smc = true,
            "--smc-nice" => nice_smc = true,
            "--sample" | "-s" => {
                if i + 1 < args.len() {
                    match args[i + 1].parse::<u32>() {
                        Ok(n) => {
                            sample_count = Some(n);
                            i += 1; // Skip the next argument since we consumed it
                        }
                        Err(_) => {
                            eprintln!("Error: Invalid sample count '{}'", args[i + 1]);
                            std::process::exit(1);
                        }
                    }
                } else {
                    eprintln!("Error: --sample requires a numeric argument");
                    std::process::exit(1);
                }
            }
            "--interval" | "-i" => {
                if i + 1 < args.len() {
                    match args[i + 1].parse::<u32>() {
                        Ok(n) if n >= 100 => {
                            interval_ms = n;
                            i += 1; // Skip the next argument since we consumed it
                        }
                        Ok(_) => {
                            eprintln!("Error: Interval must be at least 100ms");
                            std::process::exit(1);
                        }
                        Err(_) => {
                            eprintln!("Error: Invalid interval '{}'", args[i + 1]);
                            std::process::exit(1);
                        }
                    }
                } else {
                    eprintln!("Error: --interval requires a numeric argument");
                    std::process::exit(1);
                }
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            _ => {
                eprintln!("Error: Unknown argument '{}'", args[i]);
                eprintln!();
                print_usage();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    // Validate sample flag is only used with JSON
    if sample_count.is_some() && !json_output {
        eprintln!("Error: --sample can only be used with --json");
        std::process::exit(1);
    }


    // If debug SMC flag is set, show ALL SMC data
    if debug_smc {
        match smc::get_all_smc_debug_data() {
            Ok(debug_data) => {
                if json_output {
                    let json = serde_json::to_string_pretty(&debug_data).unwrap();
                    println!("{}", json);
                } else {
                    println!("=== SMC Debug Data ===");
                    println!("Total Keys: {}", debug_data.total_keys);
                    println!("Successfully Read: {}\n", debug_data.keys.len());

                    for key_data in &debug_data.keys {
                        println!(
                            "Key: {} (type: {}, size: {})",
                            key_data.key, key_data.type_str, key_data.size
                        );

                        if let Some(ref value) = key_data.value {
                            print!("  Value: ");
                            match value {
                                SmcDebugValue::Float(f) => println!("{:.3}", f),
                                SmcDebugValue::U8(v) => println!("{}", v),
                                SmcDebugValue::U16(v) => println!("{}", v),
                                SmcDebugValue::U32(v) => println!("{}", v),
                                SmcDebugValue::I8(v) => println!("{}", v),
                                SmcDebugValue::I16(v) => println!("{}", v),
                                SmcDebugValue::Bool(b) => println!("{}", b),
                                SmcDebugValue::String(s) => println!("\"{}\"", s),
                                SmcDebugValue::Bytes(_) => println!("<binary data>"),
                            }
                        }

                        if !key_data.raw_bytes.is_empty() {
                            print!("  Raw: ");
                            for byte in &key_data.raw_bytes {
                                print!("{:02x} ", byte);
                            }
                            println!();
                        }

                        if let Some(ref error) = key_data.error {
                            println!("  Error: {}", error);
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Error getting SMC debug data: {}", e);
                eprintln!("This may require elevated privileges or SMC access permissions.");
                std::process::exit(1);
            }
        }
        return;
    }

    // If nice SMC flag is set, show formatted SMC data
    if nice_smc {
        match smc::get_comprehensive_smc_metrics() {
            Ok(metrics) => {
                if json_output {
                    let json = serde_json::to_string_pretty(&metrics).unwrap();
                    println!("{}", json);
                } else {
                    println!("=== Comprehensive SMC Metrics ===\n");

                    // Temperature
                    println!("Temperature:");
                    if let Some(cpu) = metrics.temperature.cpu_temp {
                        println!("  CPU Average: {:.1}°C", cpu);
                    }
                    if let Some(gpu) = metrics.temperature.gpu_temp {
                        println!("  GPU Average: {:.1}°C", gpu);
                    }

                    // Power
                    println!("\nPower:");
                    if let Some(sys_power) = metrics.power.system_power {
                        println!("  System Total: {:.2} W", sys_power);
                    }

                    // Fans
                    if !metrics.fans.fans.is_empty() {
                        println!("\nFans:");
                        for fan in &metrics.fans.fans {
                            println!("  Fan {}:", fan.id);
                            if let Some(rpm) = fan.actual_rpm {
                                println!("    Current: {:.0} RPM", rpm);
                            }
                            if let Some(target) = fan.target_rpm {
                                println!("    Target: {:.0} RPM", target);
                            }
                            if let Some(min) = fan.minimum_rpm {
                                println!("    Min: {:.0} RPM", min);
                            }
                            if let Some(max) = fan.maximum_rpm {
                                println!("    Max: {:.0} RPM", max);
                            }
                        }
                    }

                    // Battery
                    println!("\nBattery:");
                    if let Some(cc) = metrics.battery.current_capacity {
                        println!("  Current Capacity: {:.1} mAh", cc);
                    }
                    if let Some(fc) = metrics.battery.full_charge_capacity {
                        println!("  Full Charge Capacity: {:.1} mAh", fc);
                    }
                    if let Some(health) = metrics.battery.health_percent {
                        println!("  Health: {:.1}%", health);
                    }
                    if let Some(voltage) = metrics.battery.voltage {
                        println!("  Voltage: {:.2} V", voltage);
                    }
                    if let Some(current) = metrics.battery.current {
                        println!("  Current: {:.2} A", current);
                    }
                    if let Some(temp) = metrics.battery.temperature {
                        println!("  Temperature: {:.1}°C", temp);
                    }
                    if let Some(cycles) = metrics.battery.cycle_count {
                        println!("  Cycle Count: {}", cycles);
                    }

                    // Voltages (summarized)
                    if !metrics.voltage.cpu_voltages.is_empty() {
                        println!(
                            "\nCPU Voltages: {} sensors detected",
                            metrics.voltage.cpu_voltages.len()
                        );
                        let avg: f32 = metrics
                            .voltage
                            .cpu_voltages
                            .iter()
                            .map(|(_, v)| v)
                            .sum::<f32>()
                            / metrics.voltage.cpu_voltages.len() as f32;
                        println!("  Average: {:.3} V", avg);
                    }

                    // Currents (summarized)
                    if !metrics.current.cpu_currents.is_empty() {
                        println!(
                            "\nCPU Currents: {} sensors detected",
                            metrics.current.cpu_currents.len()
                        );
                        let total: f32 = metrics.current.cpu_currents.iter().map(|(_, i)| i).sum();
                        println!("  Total: {:.2} A", total);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error getting comprehensive SMC metrics: {}", e);
                eprintln!("This may require elevated privileges or SMC access permissions.");
                std::process::exit(1);
            }
        }
        return;
    }

    // Handle sampling mode for JSON output
    if let Some(samples) = sample_count
        && json_output
    {
        // Create sampler with cached resources
        let sampler = match FastSampler::new() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error initializing sampler: {}", e);
                std::process::exit(1);
            }
        };
        
        let mut counter = 0u32;
        
        loop {
            match sampler.sample(interval_ms) {
                Ok(metrics) => {
                    // Output JSON without pretty printing for streaming
                    let json = serde_json::to_string(&metrics).unwrap();
                    println!("{}", json);
                    
                    counter += 1;
                    if samples > 0 && counter >= samples {
                        break;
                    }
                    
                    // No sleep - the interval is controlled by the sampling duration
                }
                Err(e) => {
                    eprintln!("Error collecting metrics: {}", e);
                    std::process::exit(1);
                }
            }
        }
        return;
    }
    
    // Single collection mode
    let system_metrics = match collect_metrics(interval_ms) {
        Ok(metrics) => metrics,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    if json_output {
        // Output as JSON (not prettified for consistency with sampling mode)
        let json = serde_json::to_string(&system_metrics).unwrap();
        println!("{}", json);
    } else {
        // Output as human-readable text
        println!("CPU Metrics:");
        if let Some(ref chip) = system_metrics.cpu.chip_name {
            println!("  Chip: {}", chip);
        }
        println!("  Brand: {}", system_metrics.cpu.cpu_brand);
        println!("  Physical Cores: {}", system_metrics.cpu.physical_cores);
        println!("  Logical Cores: {}", system_metrics.cpu.logical_cores);
        if let Some(ecpu) = system_metrics.cpu.ecpu_cores {
            println!("  Efficiency Cores: {}", ecpu);
        }
        if let Some(pcpu) = system_metrics.cpu.pcpu_cores {
            println!("  Performance Cores: {}", pcpu);
        }
        println!("  Frequency: {} MHz", system_metrics.cpu.cpu_frequency_mhz);

        // Performance metrics
        if let Some((freq, util)) = system_metrics.ecpu_usage {
            println!("  E-Core Usage: {} MHz ({:.1}%)", freq, util);
        }
        if let Some((freq, util)) = system_metrics.pcpu_usage {
            println!("  P-Core Usage: {} MHz ({:.1}%)", freq, util);
        }
        if let Some((freq, util)) = system_metrics.gpu_usage {
            println!("  GPU Usage: {} MHz ({:.1}%)", freq, util);
        }

        println!("\nMemory Metrics:");
        println!("  RAM:");
        println!(
            "    Total: {:.2} GB",
            system_metrics.memory.ram_total as f64 / (1024.0 * 1024.0 * 1024.0)
        );
        println!(
            "    Usage: {:.2} GB",
            system_metrics.memory.ram_usage as f64 / (1024.0 * 1024.0 * 1024.0)
        );
        println!(
            "    Used: {:.1}%",
            (system_metrics.memory.ram_usage as f64 / system_metrics.memory.ram_total as f64)
                * 100.0
        );
        println!("  Swap:");
        println!(
            "    Total: {:.2} GB",
            system_metrics.memory.swap_total as f64 / (1024.0 * 1024.0 * 1024.0)
        );
        println!(
            "    Usage: {:.2} GB",
            system_metrics.memory.swap_usage as f64 / (1024.0 * 1024.0 * 1024.0)
        );
        if system_metrics.memory.swap_total > 0 {
            println!(
                "    Used: {:.1}%",
                (system_metrics.memory.swap_usage as f64 / system_metrics.memory.swap_total as f64)
                    * 100.0
            );
        } else {
            println!("    Used: 0.0%");
        }

        if let Some(ref power) = system_metrics.power {
            println!("\nPower Metrics:");
            println!("  System Total: {:.2} W", power.sys_power);
            println!("  CPU: {:.2} W", power.cpu_power);
            println!("  GPU: {:.2} W", power.gpu_power);
            if power.ane_power > 0.0 {
                println!("  ANE (Neural Engine): {:.2} W", power.ane_power);
            }
            println!("  Memory: {:.2} W", power.ram_power);
            if power.gpu_ram_power > 0.0 {
                println!("  GPU Memory: {:.2} W", power.gpu_ram_power);
            }
            println!("  Combined (CPU+GPU+ANE): {:.2} W", power.all_power);
        }
    }
}
