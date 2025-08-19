mod memory;

use clap::Parser;
use memory::MemoryMetrics;
use serde::Serialize;
use serde_json;

#[derive(Parser)]
#[command(name = "atop")]
#[command(about = "System memory metrics monitoring tool", long_about = None)]
struct Args {
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Serialize)]
struct SystemMetrics {
    memory: MemoryMetrics,
}

fn main() {
    let args = Args::parse();

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

    if args.json {
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