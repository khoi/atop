mod cpu;
mod iokit;
mod memory;
mod smc;

use cpu::CpuMetrics;
use memory::MemoryMetrics;
use serde::Serialize;
use smc::TemperatureMetrics;
use std::env;

#[derive(Serialize)]
struct SystemMetrics {
    memory: MemoryMetrics,
    cpu: CpuMetrics,
    temperature: Option<TemperatureMetrics>,
}

fn print_usage() {
    eprintln!("Usage: atop [OPTIONS]");
    eprintln!();
    eprintln!("System memory metrics monitoring tool");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("    --json    Output as JSON");
    eprintln!("    --help    Print this help message");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    // Parse arguments
    let mut json_output = false;

    for arg in &args[1..] {
        match arg.as_str() {
            "--json" => json_output = true,
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            _ => {
                eprintln!("Error: Unknown argument '{}'", arg);
                eprintln!();
                print_usage();
                std::process::exit(1);
            }
        }
    }

    // Get real memory metrics
    let memory_metrics = match memory::get_memory_metrics() {
        Ok(metrics) => metrics,
        Err(e) => {
            eprintln!("Error getting memory metrics: {}", e);
            std::process::exit(1);
        }
    };

    // Get CPU metrics
    let cpu_metrics = match cpu::get_cpu_metrics() {
        Ok(metrics) => metrics,
        Err(e) => {
            eprintln!("Error getting CPU metrics: {}", e);
            std::process::exit(1);
        }
    };

    // Get temperature metrics (optional, may fail without permissions)
    let temperature_metrics = smc::get_temperature_metrics().ok();

    let system_metrics = SystemMetrics {
        memory: memory_metrics,
        cpu: cpu_metrics,
        temperature: temperature_metrics,
    };

    if json_output {
        // Output as JSON
        let json = serde_json::to_string_pretty(&system_metrics).unwrap();
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

        if let Some(ref temps) = system_metrics.temperature {
            println!("\nTemperature Metrics:");
            if let Some(cpu_temp) = temps.cpu_temp {
                println!("  CPU: {:.1}°C", cpu_temp);
            }
            if let Some(gpu_temp) = temps.gpu_temp {
                println!("  GPU: {:.1}°C", gpu_temp);
            }
            if !temps.sensors.is_empty() && temps.sensors.len() > 2 {
                println!("  Additional Sensors:");
                for (name, temp) in &temps.sensors {
                    println!("    {}: {:.1}°C", name, temp);
                }
            }
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
    }
}
