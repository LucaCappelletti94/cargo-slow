//! # slow-rs
//!
//! A comprehensive system slowness diagnostic tool for Linux workstations.
//!
//! ## Overview
//!
//! `slow-rs` helps identify the root cause of mysterious system slowdowns by
//! continuously monitoring and benchmarking various system metrics. It's
//! particularly useful when you're not sure whether a slowdown is caused by:
//!
//! - Disk I/O problems (failing drive, high latency)
//! - Memory pressure (swapping, OOM conditions)
//! - CPU issues (thermal throttling, VM steal time)
//! - General resource exhaustion
//!
//! ## Features
//!
//! - **Active Benchmarks**: Measures I/O throughput, memory allocation speed,
//!   and CPU compute performance at regular intervals
//! - **Passive Monitoring**: Collects detailed stats from `/proc` including
//!   CPU time breakdown, disk I/O, network traffic, and memory details
//! - **Pressure Stall Information (PSI)**: Reports Linux PSI metrics to show
//!   when tasks are waiting for CPU, memory, or I/O
//! - **Temperature Monitoring**: Tracks CPU and system temperatures
//! - **TUI Dashboard**: Real-time terminal UI with charts
//! - **CSV Logging**: All metrics logged for later analysis
//!
//! ## Usage
//!
//! ```bash
//! # Run with TUI (default)
//! slow-rs
//!
//! # Headless mode for logging only
//! slow-rs --headless
//!
//! # Custom interval and enable I/O benchmark
//! slow-rs -i 10 --io-bench
//! ```
//!
//! ## Module Organization
//!
//! - [`config`]: CLI argument parsing and configuration
//! - [`metrics`]: Data structures for collected metrics
//! - [`collectors`]: Functions to read system stats from `/proc`
//! - [`benchmarks`]: Active performance tests
//! - [`app`]: Main application state and coordination
//! - [`ui`]: Terminal user interface

mod app;
mod availability;
mod benchmarks;
mod collectors;
mod config;
mod ipmi;
mod metrics;
mod recommendations;
mod smart;
mod thresholds;
mod ui;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;

use app::App;
use config::Config;

fn main() -> std::io::Result<()> {
    // Platform check - warn on non-Linux systems
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║  WARNING: slow-rs is designed for Linux systems only!        ║");
        eprintln!("║                                                              ║");
        eprintln!("║  Most metrics (CPU, memory, disk, temperatures, PSI, etc.)   ║");
        eprintln!("║  are read from /proc and /sys which don't exist on macOS.    ║");
        eprintln!("║                                                              ║");
        eprintln!("║  Only basic benchmarks will work. For full functionality,    ║");
        eprintln!("║  please run on a Linux system.                               ║");
        eprintln!("╚══════════════════════════════════════════════════════════════╝");
        eprintln!();
    }

    let config = Config::parse();
    let app = App::new(config.clone())?;

    // Create test file if needed
    app.ensure_test_file()?;

    // Setup Ctrl+C / SIGTERM handler
    let running = Arc::new(AtomicBool::new(true));
    setup_signal_handler(running.clone());

    let interval = Duration::from_secs(config.interval);

    // Check if stdout is a TTY - if not, force headless mode
    let use_headless = config.headless || !is_terminal();
    if !config.headless && !is_terminal() {
        eprintln!("Warning: stdout is not a TTY, running in headless mode");
    }

    if use_headless {
        ui::run_headless(app, running, interval)?;
    } else {
        ui::run(app, running, interval)?;
    }

    Ok(())
}

/// Global flag for signal handler (must be static for signal safety).
static SIGNAL_RECEIVED: AtomicBool = AtomicBool::new(false);

/// Set up signal handlers for graceful shutdown.
fn setup_signal_handler(running: Arc<AtomicBool>) {
    // Spawn a thread to monitor the signal flag and propagate to running
    let running_clone = running.clone();
    std::thread::spawn(move || {
        while running_clone.load(Ordering::Relaxed) {
            if SIGNAL_RECEIVED.load(Ordering::Relaxed) {
                running_clone.store(false, Ordering::Relaxed);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });

    unsafe {
        libc::signal(
            libc::SIGINT,
            signal_handler as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGTERM,
            signal_handler as *const () as libc::sighandler_t,
        );
    }
}

/// Signal handler that sets the signal flag (async-signal-safe).
extern "C" fn signal_handler(_: i32) {
    SIGNAL_RECEIVED.store(true, Ordering::Relaxed);
}

/// Check if stdout is connected to a terminal.
fn is_terminal() -> bool {
    unsafe { libc::isatty(libc::STDOUT_FILENO) != 0 }
}
