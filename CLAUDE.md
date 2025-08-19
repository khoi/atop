# CLAUDE.md

## Project Overview

`atop` is a system memory metrics monitoring tool for macOS that uses low-level system calls to retrieve memory information. It provides both human-readable and JSON output formats.

## Architecture

The codebase is structured as a simple Rust CLI application:

- **src/main.rs**: Entry point handling command-line argument parsing and output formatting
- **src/memory.rs**: Core memory metrics retrieval using macOS system calls (libc and mach2)

## Development Commands

### Build

```bash
cargo build              # Development build
cargo build --release    # Release build (optimized)
```

### Run

```bash
cargo run                # Run with default output
cargo run -- --json      # Run with JSON output
./target/debug/atop      # Run built binary directly
```

### Code Quality

```bash
cargo fmt                # Format code
cargo clippy             # Run linter
cargo test               # Run tests (currently no tests exist)
```

## Key Implementation Details

- The application directly interfaces with macOS system APIs through `libc` and `mach2` crates
