# CLAUDE.md

## Project Overview

`atop` is a sudoless comprehensive system metrics monitoring tool for macOS that retrieves CPU, memory, temperature, and power information using low-level system APIs and SMC (System Management Controller) access.

Note: Apple Silicon only. Intel Macs are not supported.

## Architecture

The codebase is a Rust CLI application with modular metric collection:

- **src/main.rs**: CLI entry point with argument parsing and output formatting (JSON/human-readable)
- **src/memory.rs**: Memory metrics via macOS mach/vm_stat system calls
- **src/cpu.rs**: CPU information using sysctl (cores, frequency, chip details)
- **src/smc.rs**: SMC interface for temperature, power, fans, battery, and voltage metrics
- **src/iokit.rs**: IOReport-based power monitoring (CPU, GPU, ANE, memory power)

## Development Commands

### Build and Run

```bash
cargo build              # Development build
cargo build --release    # Release build (optimized)
cargo run                # Run with default output
cargo run -- --json      # JSON output
cargo run -- --smc       # Debug mode: show raw SMC data
cargo run -- --smc-nice  # Formatted SMC metrics (power, fans, battery)
```

### Code Quality

```bash
cargo fmt                # Format code
cargo clippy             # Run linter
```

## Development Guidelines

1. Always run `cargo clippy` and fix all the warnings before concluding the task.
2. Remove all dead code
3. Always test with `cargo run -- --json` and ensure all JSON values are non-null

## Key Implementation Details

- Direct SMC access requires elevated privileges on some systems
- IOKit power metrics use IOReport framework with fallback to SMC system power
- Temperature sensors dynamically detect available keys (TC0P, TG0D variants)
- Battery metrics include cycle count, health percentage, and charging state
