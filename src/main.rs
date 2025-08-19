mod cpu;
mod iokit;
mod memory;
mod smc;

use cpu::CpuMetrics;
use iokit::PowerMetrics;
use memory::MemoryMetrics;
use serde::Serialize;
use smc::{SmcDebugValue, TemperatureMetrics};
use std::env;

#[derive(Serialize)]
struct SystemMetrics {
    memory: MemoryMetrics,
    cpu: CpuMetrics,
    temperature: Option<TemperatureMetrics>,
    power: Option<PowerMetrics>,
}

fn print_usage() {
    eprintln!("Usage: atop [OPTIONS]");
    eprintln!();
    eprintln!("System memory metrics monitoring tool");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("    --json      Output as JSON");
    eprintln!("    --smc       Show ALL SMC data for debugging (includes raw values)");
    eprintln!("    --smc-nice  Show formatted SMC metrics (power, fans, battery, etc.)");
    eprintln!("    --help      Print this help message");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    // Parse arguments
    let mut json_output = false;
    let mut debug_smc = false;
    let mut nice_smc = false;

    for arg in &args[1..] {
        match arg.as_str() {
            "--json" => json_output = true,
            "--smc" => debug_smc = true,
            "--smc-nice" => nice_smc = true,
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

    // Get temperature metrics (optional, may fail without permissions)
    let temperature_metrics = smc::get_temperature_metrics().ok();

    // Get power metrics (optional, may fail without permissions)
    // First try to get SMC system power for fallback
    let smc_sys_power = if let Ok(mut smc) = smc::get_smc_connection() {
        smc.read_float("PSTR").ok()
    } else {
        None
    };

    let power_metrics = iokit::get_power_metrics(smc_sys_power).ok();

    let system_metrics = SystemMetrics {
        memory: memory_metrics,
        cpu: cpu_metrics,
        temperature: temperature_metrics,
        power: power_metrics,
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
