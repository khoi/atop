mod cpu;
mod dashboard;
mod iokit;
mod ioreport_perf;
mod memory;
mod time_graph;
mod utils;

use cpu::CpuMetrics;
use iokit::PowerMetrics;
use ioreport_perf::IOReportPerf;
use memory::MemoryMetrics;
use serde::Serialize;
use std::env;

// Sampler struct to hold cached resources
struct FastSampler {
    cpu_metrics: CpuMetrics,
    perf_monitor: Option<IOReportPerf>,
}

impl FastSampler {
    fn new() -> Result<Self, String> {
        let cpu_metrics =
            cpu::get_cpu_metrics().map_err(|e| format!("Error getting CPU metrics: {}", e))?;

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

        // Get power metrics with the same interval
        let power_metrics = iokit::get_power_metrics_with_interval(interval_ms as u64).ok();

        // Get performance metrics using cached monitor
        let perf_sample = self
            .perf_monitor
            .as_ref()
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
    eprintln!("When run without arguments, launches an interactive dashboard.");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("    --json               Output as JSON");
    eprintln!(
        "    --sample, -s <N>     Number of samples to collect (0 = infinite, only with --json)"
    );
    eprintln!("    --interval, -i <MS>  Update interval in milliseconds (default: 1000, min: 100)");
    eprintln!("    --help               Print this help message");
    eprintln!();
    eprintln!("DASHBOARD CONTROLS:");
    eprintln!("    +/-                  Adjust refresh rate");
    eprintln!("    q/ESC                Quit");
}

fn collect_metrics(interval_ms: u32) -> Result<SystemMetrics, String> {
    // Get real memory metrics
    let memory_metrics =
        memory::get_memory_metrics().map_err(|e| format!("Error getting memory metrics: {}", e))?;

    // Get CPU metrics
    let cpu_metrics =
        cpu::get_cpu_metrics().map_err(|e| format!("Error getting CPU metrics: {}", e))?;

    // Get power metrics
    let power_metrics = iokit::get_power_metrics_with_interval(interval_ms as u64).ok();

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

    // If no arguments provided, launch the dashboard
    if args.len() == 1 {
        let mut dashboard = match dashboard::Dashboard::new() {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Error initializing dashboard: {}", e);
                std::process::exit(1);
            }
        };
        if let Err(e) = dashboard.run() {
            eprintln!("Error running dashboard: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // Parse arguments
    let mut json_output = false;
    let mut sample_count: Option<u32> = None;
    let mut interval_ms: u32 = 1000; // Default 1 second

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json_output = true,
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
