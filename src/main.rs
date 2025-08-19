use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json;

#[derive(Parser)]
#[command(name = "atop")]
#[command(about = "A CLI app that can output JSON", long_about = None)]
struct Args {
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Serialize, Deserialize)]
struct ExampleData {
    id: u32,
    name: String,
    active: bool,
    stats: Stats,
    tags: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Stats {
    cpu_usage: f64,
    memory_mb: u64,
    uptime_seconds: u64,
}

fn main() {
    let args = Args::parse();

    // Example data structure
    let example = ExampleData {
        id: 42,
        name: String::from("example-service"),
        active: true,
        stats: Stats {
            cpu_usage: 45.7,
            memory_mb: 1024,
            uptime_seconds: 86400,
        },
        tags: vec![
            String::from("production"),
            String::from("critical"),
            String::from("monitored"),
        ],
    };

    if args.json {
        // Output as JSON
        let json = serde_json::to_string_pretty(&example).unwrap();
        println!("{}", json);
    } else {
        // Output as human-readable text
        println!("Service Information:");
        println!("  ID: {}", example.id);
        println!("  Name: {}", example.name);
        println!("  Active: {}", example.active);
        println!("  Stats:");
        println!("    CPU Usage: {:.1}%", example.stats.cpu_usage);
        println!("    Memory: {} MB", example.stats.memory_mb);
        println!("    Uptime: {} seconds", example.stats.uptime_seconds);
        println!("  Tags: {}", example.tags.join(", "));
    }
}