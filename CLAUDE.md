# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`atop` is a sudoless system metrics monitoring tool for macOS (Apple Silicon only) that collects CPU, memory, and power metrics using low-level system APIs without requiring elevated privileges.

## Architecture

### Directory Structure

```
src/
├── main.rs          # CLI entry point, argument parsing, FastSampler
├── metrics/         # Metric collection logic
│   ├── cpu.rs       # CPU topology via sysctl + IOKit (cached in sampling mode)
│   ├── memory.rs    # Memory metrics via mach kernel APIs (~30µs per collection)
│   ├── iokit.rs     # Power metrics via IOReport Energy Model (100ms-1000ms samples)
│   └── ioreport_perf.rs  # CPU/GPU frequency and utilization via IOReport
├── ui/              # User interface components
│   ├── dashboard.rs # Interactive TUI dashboard
│   └── time_graph.rs # High-resolution time-series graph widget using Braille characters
└── utils/           # Utility functions
    └── iokit_utils.rs # IOKit helper functions for Core Foundation operations
```

### Metrics Collection Flow

```
atop
├── Dashboard Mode (no args)
│   ├── Continuous collection thread
│   └── TUI with TimeGraph visualizations
├── Single Sample Mode (default with --json)
│   └── collect_metrics() → All metrics fresh
└── Continuous Sampling Mode (--sample N)
    └── FastSampler
        ├── Cached: CPU topology (static hardware info)
        ├── Cached: IOReportPerf instance
        └── Fresh: Memory, Power, Performance metrics
```

### Key Data Structures

- `SystemMetrics`: Main output structure for JSON serialization
- `FastSampler`: Optimized sampler with cached CPU metrics and IOReportPerf instance
- `DashboardState`: Maintains historical data for graph visualizations (128 samples max)
- `TimeGraph`: Custom widget for rendering time-series data without axes

### Dashboard Architecture

The dashboard runs two parallel threads:
1. **Metric Collection Thread**: Continuously samples metrics at the specified interval
2. **UI Thread**: Renders the TUI and handles keyboard events

Communication happens via mpsc channel with `MetricEvent` messages.

## Development Commands

### Build and Run

```bash
cargo build --release           # Optimized build (required for performance testing)
cargo run                       # Launch interactive dashboard
cargo run -- --json             # Single JSON output
cargo run -- --json -s 10 -i 500  # 10 samples at 500ms intervals
```

### Dashboard Mode

```bash
cargo run                       # Launch dashboard (default)
# Controls:
# +/- : Adjust refresh rate
# q/ESC : Quit
```

### Testing Sampling Performance

```bash
# Test sampling performance
time ./target/release/atop --json -s 10 -i 100 2>/dev/null | wc -l
```

### Code Quality

```bash
cargo clippy    # Must pass with no warnings
cargo fmt       # Format code
```

## Critical Implementation Details

### Sampling Intervals

- The `--interval` parameter controls the IOReport sampling window duration, NOT a sleep between samples
- Both power and performance metrics must use the same interval for consistency
- Minimum interval is 100ms (enforced in argument parsing)
- Dashboard refresh rate is controlled independently from metric collection interval

### Performance Optimizations

- CPU metrics are cached in `FastSampler` (static hardware info doesn't change)
- IOReportPerf instance is reused across samples to avoid recreation overhead
- Power metrics interval aligned with performance metrics interval
- Dashboard uses VecDeque for efficient historical data management

### IOReport Timing

- `IOReportCreateSamples` takes two snapshots with a sleep between them
- The sleep duration IS the interval parameter
- Total time per sample = interval + overhead (~5-10ms for memory/CPU collection)

### UI Implementation

- TimeGraph widget uses Braille characters for 2x4 resolution per terminal cell
- All graphs render without axes for cleaner visualization
- Data flows from oldest (left) to newest (right)
- Maximum 128 historical samples per metric

## Development Guidelines

- Use `cargo add` to add new dependencies instead of modifying `Cargo.toml` directly
- Remove all dead code, unused variables
- When adding new metrics, update both `SystemMetrics` struct and dashboard visualization
- Maintain separation between metrics collection (`/metrics`), UI (`/ui`), and utilities (`/utils`)