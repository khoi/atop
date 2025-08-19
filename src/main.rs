mod memory;

use memory::MemoryMetrics;
use serde::Serialize;
use serde_json;
use std::env;

#[derive(Serialize)]
struct SystemMetrics {
    memory: MemoryMetrics,
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

    let system_metrics = SystemMetrics {
        memory: memory_metrics,
    };

    if json_output {
        // Output as JSON
        let json = serde_json::to_string_pretty(&system_metrics).unwrap();
        println!("{}", json);
    } else {
        // Output as human-readable text
        println!("Memory Metrics:");
        println!("  RAM:");
        println!("    Total: {:.2} GB", system_metrics.memory.ram_total as f64 / (1024.0 * 1024.0 * 1024.0));
        println!("    Usage: {:.2} GB", system_metrics.memory.ram_usage as f64 / (1024.0 * 1024.0 * 1024.0));
        println!("    Used: {:.1}%", (system_metrics.memory.ram_usage as f64 / system_metrics.memory.ram_total as f64) * 100.0);
        println!("  Swap:");
        println!("    Total: {:.2} GB", system_metrics.memory.swap_total as f64 / (1024.0 * 1024.0 * 1024.0));
        println!("    Usage: {:.2} GB", system_metrics.memory.swap_usage as f64 / (1024.0 * 1024.0 * 1024.0));
        if system_metrics.memory.swap_total > 0 {
            println!("    Used: {:.1}%", (system_metrics.memory.swap_usage as f64 / system_metrics.memory.swap_total as f64) * 100.0);
        } else {
            println!("    Used: 0.0%");
        }
    }
}