# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`atop` is a sudoless system metrics monitoring tool for macOS (Apple Silicon only) that collects CPU, memory, and power metrics using low-level system APIs without requiring elevated privileges.

## Architecture

### Metrics Collection Flow

```
atop
├── Single Sample Mode (default)
│   └── collect_metrics() → All metrics fresh
└── Continuous Sampling Mode (--sample N)
    └── FastSampler
        ├── Cached: CPU topology (static hardware info)
        ├── Cached: IOReportPerf instance
        └── Fresh: Memory, Power, Performance metrics
```

### Module Structure

- **src/main.rs**: CLI entry point, argument parsing, `FastSampler` for optimized sampling
- **src/memory.rs**: Memory metrics via mach kernel APIs (~30µs per collection)
- **src/cpu.rs**: CPU topology via sysctl + IOKit (cached in sampling mode)
- **src/iokit.rs**: Power metrics via IOReport Energy Model (100ms-1000ms samples)
- **src/ioreport_perf.rs**: CPU/GPU frequency and utilization via IOReport
- **src/utils.rs**: IOKit utilities for CPU frequency detection

### Key Data Structures

- `SystemMetrics`: Main output structure (no temperature field as of latest version)
- `FastSampler`: Optimized sampler with cached CPU metrics and IOReportPerf instance
- Power/Performance metrics use IOReport's two-snapshot delta approach

## Development Commands

### Build and Run

```bash
cargo build --release           # Optimized build (required for performance testing)
cargo run -- --json             # Single JSON output
cargo run -- --json -s 10 -i 500  # 10 samples at 500ms intervals
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

### Performance Optimizations

- CPU metrics are cached in `FastSampler` (static hardware info doesn't change)
- IOReportPerf instance is reused across samples to avoid recreation overhead
- Power metrics interval aligned with performance metrics interval

### IOReport Timing

- `IOReportCreateSamples` takes two snapshots with a sleep between them
- The sleep duration IS the interval parameter
- Total time per sample = interval + overhead (~5-10ms for memory/CPU collection)

## Development Guidelines

- Use `cargo add` to add new dependencies instead of modifying `Cargo.toml` directly.
- Remove all dead code, unused variables.
