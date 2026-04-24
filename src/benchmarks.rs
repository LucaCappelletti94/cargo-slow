//! Performance benchmarks for slow-rs.
//!
//! This module provides active performance tests that run periodically
//! to measure actual system performance, as opposed to the passive
//! statistics collected from `/proc`.
//!
//! # Benchmarks
//!
//! - **Memory Allocation**: Allocates and touches 64MB of memory
//! - **Compute**: CPU-bound SHA256 hashing
//! - **I/O**: Disk read/write throughput
//!
//! These benchmarks help identify performance degradation that might
//! not be visible in system statistics alone.

use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Instant;

/// Benchmark memory allocation performance.
///
/// Allocates 64MB of memory and touches every page to ensure the memory
/// is actually allocated by the OS (not just reserved).
///
/// # Returns
///
/// Time taken in milliseconds.
///
/// # What This Tests
///
/// - Memory allocator performance
/// - Available physical memory
/// - Memory pressure conditions
///
/// High values (>100ms) may indicate memory pressure or swap activity.
pub fn benchmark_allocation() -> f64 {
    let start = Instant::now();

    let size = 64 * 1024 * 1024; // 64MB
    let mut v: Vec<u8> = vec![0; size];

    // Touch every page (4KB) to ensure actual allocation
    for i in (0..size).step_by(4096) {
        v[i] = (i & 0xFF) as u8;
    }

    // Prevent compiler optimization from eliminating the allocation
    std::hint::black_box(&v);
    drop(v);

    start.elapsed().as_secs_f64() * 1000.0
}

/// Benchmark CPU compute performance.
///
/// Computes 10 rounds of SHA256 on 10MB of deterministic data.
/// This is entirely CPU-bound and doesn't touch disk or network.
///
/// # Returns
///
/// Time taken in milliseconds.
///
/// # What This Tests
///
/// - CPU performance
/// - Thermal throttling
/// - CPU steal time (in VMs)
///
/// Consistent high values or increasing trend may indicate thermal
/// throttling or VM CPU contention.
pub fn benchmark_compute() -> f64 {
    let start = Instant::now();

    let mut hasher = Sha256::new();

    // Generate deterministic pseudo-random data
    let data: Vec<u8> = (0..1_000_000u32)
        .map(|i| (i.wrapping_mul(2654435761) & 0xFF) as u8)
        .collect();

    // Hash 10 times for a more stable measurement
    for _ in 0..10 {
        hasher.update(&data);
    }

    let _ = std::hint::black_box(hasher.finalize());

    start.elapsed().as_secs_f64() * 1000.0
}

/// Result of the I/O benchmark.
pub struct IoBenchmarkResult {
    /// Read speed in MB/s
    pub read_mb_per_sec: f64,
    /// Write speed in MB/s
    pub write_mb_per_sec: f64,
    /// Time to read + hash in milliseconds
    pub sha_duration_ms: f64,
}

/// Benchmark disk I/O performance.
///
/// Reads the entire test file while computing SHA256, then writes
/// a smaller test file. The SHA256 ensures the read isn't optimized
/// away and adds a realistic workload.
///
/// # Arguments
///
/// * `test_file` - Path to the test file for reading
/// * `file_size_mb` - Size of the test file in MB
///
/// # Returns
///
/// I/O benchmark results, or an error if the file can't be accessed.
///
/// # What This Tests
///
/// - Sequential disk read/write throughput
/// - Disk latency under load
/// - Filesystem and block layer performance
///
/// Low or decreasing values may indicate:
/// - Failing disk
/// - Heavy I/O from other processes
/// - Filesystem issues
pub fn benchmark_io(test_file: &str, file_size_mb: usize) -> std::io::Result<IoBenchmarkResult> {
    // Try to drop caches for accurate measurement (requires root)
    let _ = std::fs::write("/proc/sys/vm/drop_caches", b"3");

    let file_size = file_size_mb as f64;

    // === Read benchmark with SHA256 ===
    let start = Instant::now();
    let mut file = File::open(test_file)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024]; // 1MB buffer

    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    let _ = hasher.finalize();

    let read_duration = start.elapsed();
    let read_mb_per_sec = file_size / read_duration.as_secs_f64();
    let sha_duration_ms = read_duration.as_secs_f64() * 1000.0;

    // === Write benchmark ===
    let write_test_file = write_benchmark_path(test_file);
    let _cleanup = RemoveFileOnDrop {
        path: write_test_file.clone(),
    };
    let write_size_mb = (file_size_mb / 4).max(16); // Smaller write test

    let start = Instant::now();
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&write_test_file)?;
        let chunk = vec![0xCDu8; 1024 * 1024];
        for _ in 0..write_size_mb {
            f.write_all(&chunk)?;
        }
        f.sync_all()?; // Ensure data hits disk
    }
    let write_duration = start.elapsed();
    let write_mb_per_sec = write_size_mb as f64 / write_duration.as_secs_f64();

    Ok(IoBenchmarkResult {
        read_mb_per_sec,
        write_mb_per_sec,
        sha_duration_ms,
    })
}

/// Create the test file for I/O benchmarks.
///
/// Creates a file filled with a repeating pattern. The pattern helps
/// detect corruption if the file is read back incorrectly.
///
/// # Arguments
///
/// * `path` - Path where the file should be created
/// * `size_mb` - Size of the file in megabytes
pub fn create_test_file(path: &str, size_mb: usize) -> std::io::Result<()> {
    eprintln!("Creating {} MB test file at {}...", size_mb, path);

    let mut f = File::create(path)?;
    let chunk = vec![0xABu8; 1024 * 1024]; // 1MB of pattern

    for i in 0..size_mb {
        f.write_all(&chunk)?;
        if i % 64 == 0 {
            eprint!("\r  Progress: {}%", (i * 100) / size_mb);
        }
    }

    eprintln!("\r  Done creating test file.              ");
    f.sync_all()?;

    Ok(())
}

fn write_benchmark_path(test_file: &str) -> PathBuf {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    PathBuf::from(format!(
        "{}.write_test.{}.{}",
        test_file,
        std::process::id(),
        suffix
    ))
}

struct RemoveFileOnDrop {
    path: PathBuf,
}

impl Drop for RemoveFileOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::{create_test_file, write_benchmark_path};

    #[test]
    fn write_benchmark_path_does_not_target_fixed_sibling_file() {
        let path = write_benchmark_path("/tmp/slowtest.bin");
        let path = path.to_string_lossy();

        assert!(path.starts_with("/tmp/slowtest.bin.write_test."));
        assert_ne!(path, "/tmp/slowtest.bin.write_test");
    }

    #[test]
    fn create_test_file_writes_requested_pattern_and_size() {
        let path = std::env::temp_dir().join(format!(
            "slow-rs-create-test-file-{}.bin",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        create_test_file(path.to_str().unwrap(), 1).unwrap();

        let contents = std::fs::read(&path).unwrap();
        assert_eq!(contents.len(), 1024 * 1024);
        assert!(contents.iter().all(|byte| *byte == 0xAB));

        let _ = std::fs::remove_file(&path);
    }
}
