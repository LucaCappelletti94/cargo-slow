//! Metrics data structures for slow-rs.
//!
//! This module defines the [`Metrics`] struct which holds all collected
//! system performance data, as well as intermediate data structures used
//! during collection.

use serde::Serialize;

/// Complete snapshot of system metrics at a point in time.
///
/// This struct is serialized to CSV for logging. All fields are designed
/// to help diagnose system slowdowns by providing visibility into various
/// subsystems.
///
/// # Field Categories
///
/// - **Timestamps**: When the measurement was taken
/// - **Benchmarks**: Active performance tests (I/O, compute, memory)
/// - **Memory**: RAM and swap usage details
/// - **CPU**: Usage percentages and time breakdowns
/// - **Disk I/O**: Read/write statistics from the kernel
/// - **Network**: Traffic and error counters
/// - **Pressure (PSI)**: Linux pressure stall information
/// - **Temperatures**: Hardware thermal sensors
/// - **VM Stats**: Virtual memory and paging statistics
#[derive(Serialize, Clone, Debug, Default)]
pub struct Metrics {
    // ===== Timestamps =====
    /// Unix timestamp (seconds since epoch)
    pub timestamp: i64,
    /// ISO 8601 formatted datetime string
    pub datetime: String,

    // ===== Benchmark Results =====
    /// Disk read speed in MB/s (None if I/O benchmark skipped)
    pub io_read_mb_per_sec: Option<f64>,
    /// Disk write speed in MB/s (None if I/O benchmark skipped)
    pub io_write_mb_per_sec: Option<f64>,
    /// Time to read test file + compute SHA256 in milliseconds
    pub sha256_duration_ms: Option<f64>,
    /// Time to allocate and touch 64MB of memory in milliseconds
    pub memory_alloc_duration_ms: f64,
    /// Time to compute 10 rounds of SHA256 on 1MB data in milliseconds
    pub compute_duration_ms: f64,

    // ===== Memory (from sysinfo + /proc/meminfo) =====
    /// Total physical RAM in MB
    pub mem_total_mb: u64,
    /// Used memory in MB (total - free - buffers - cached)
    pub mem_used_mb: u64,
    /// Free memory in MB (completely unused)
    pub mem_free_mb: u64,
    /// Available memory in MB (free + reclaimable)
    pub mem_available_mb: u64,
    /// Total swap space in MB
    pub swap_total_mb: u64,
    /// Used swap space in MB
    pub swap_used_mb: u64,
    /// Memory used for block device buffers in MB
    pub mem_buffers_mb: u64,
    /// Memory used for page cache in MB
    pub mem_cached_mb: u64,

    // ===== CPU =====
    /// Average CPU usage across all cores (0-100%)
    pub cpu_usage_percent: f32,
    /// Number of CPU cores
    pub cpu_count: usize,

    // ===== Load Averages =====
    /// 1-minute load average
    pub load_avg_1: f64,
    /// 5-minute load average
    pub load_avg_5: f64,
    /// 15-minute load average
    pub load_avg_15: f64,

    // ===== Process Statistics =====
    /// Total number of processes
    pub process_count: usize,
    /// Total threads (running + blocked)
    pub thread_count: u64,
    /// Processes currently running on CPU
    pub procs_running: u64,
    /// Processes blocked waiting for I/O
    pub procs_blocked: u64,

    // ===== CPU Time Breakdown (delta since last sample) =====
    /// Jiffies spent in user mode
    pub cpu_user: u64,
    /// Jiffies spent in user mode with low priority (nice)
    pub cpu_nice: u64,
    /// Jiffies spent in kernel mode
    pub cpu_system: u64,
    /// Jiffies spent idle
    pub cpu_idle: u64,
    /// Jiffies spent waiting for I/O (HIGH = disk bottleneck)
    pub cpu_iowait: u64,
    /// Jiffies spent servicing hardware interrupts
    pub cpu_irq: u64,
    /// Jiffies spent servicing software interrupts
    pub cpu_softirq: u64,
    /// Jiffies stolen by hypervisor (HIGH = VM throttled)
    pub cpu_steal: u64,

    // ===== Disk I/O (delta since last sample) =====
    /// Number of read operations completed
    pub disk_reads_completed: u64,
    /// Number of read operations merged
    pub disk_reads_merged: u64,
    /// Number of 512-byte sectors read
    pub disk_sectors_read: u64,
    /// Milliseconds spent reading
    pub disk_read_time_ms: u64,
    /// Number of write operations completed
    pub disk_writes_completed: u64,
    /// Number of write operations merged
    pub disk_writes_merged: u64,
    /// Number of 512-byte sectors written
    pub disk_sectors_written: u64,
    /// Milliseconds spent writing
    pub disk_write_time_ms: u64,
    /// Current I/O operations in flight (instantaneous)
    pub disk_io_in_progress: u64,
    /// Milliseconds spent doing I/O
    pub disk_io_time_ms: u64,
    /// Weighted milliseconds spent doing I/O (queue depth × time)
    pub disk_weighted_io_time_ms: u64,

    // ===== Network (delta since last sample) =====
    /// Bytes received across all interfaces
    pub net_rx_bytes: u64,
    /// Bytes transmitted across all interfaces
    pub net_tx_bytes: u64,
    /// Packets received
    pub net_rx_packets: u64,
    /// Packets transmitted
    pub net_tx_packets: u64,
    /// Receive errors
    pub net_rx_errors: u64,
    /// Transmit errors
    pub net_tx_errors: u64,

    // ===== Pressure Stall Information (PSI) =====
    /// CPU pressure: % of time some tasks stalled (10s avg)
    pub cpu_pressure_some_avg10: Option<f64>,
    /// CPU pressure: % of time some tasks stalled (60s avg)
    pub cpu_pressure_some_avg60: Option<f64>,
    /// CPU pressure: % of time some tasks stalled (300s avg)
    pub cpu_pressure_some_avg300: Option<f64>,
    /// Memory pressure: % of time some tasks stalled (10s avg)
    pub mem_pressure_some_avg10: Option<f64>,
    /// Memory pressure: % of time some tasks stalled (60s avg)
    pub mem_pressure_some_avg60: Option<f64>,
    /// Memory pressure: % of time ALL tasks stalled (10s avg)
    pub mem_pressure_full_avg10: Option<f64>,
    /// I/O pressure: % of time some tasks stalled (10s avg)
    pub io_pressure_some_avg10: Option<f64>,
    /// I/O pressure: % of time some tasks stalled (60s avg)
    pub io_pressure_some_avg60: Option<f64>,
    /// I/O pressure: % of time ALL tasks stalled (10s avg)
    pub io_pressure_full_avg10: Option<f64>,
    /// I/O pressure: % of time ALL tasks stalled (60s avg)
    pub io_pressure_full_avg60: Option<f64>,

    // ===== Temperatures =====
    /// CPU package temperature in Celsius
    pub cpu_temp_celsius: Option<f64>,
    /// Source of CPU temperature (e.g., "coretemp", "k10temp", "zenpower")
    pub cpu_temp_source: Option<String>,
    /// Maximum temperature across all sensors in Celsius
    pub max_temp_celsius: Option<f64>,
    /// DIMM temperatures as comma-separated string (e.g., "DIMM0:45.5,DIMM1:46.0")
    pub dimm_temps: Option<String>,
    /// Source of DIMM temperature (e.g., "jc42", "ipmi")
    pub dimm_temp_source: Option<String>,
    /// Average DIMM temperature in Celsius
    pub dimm_temp_avg: Option<f64>,
    /// Maximum DIMM temperature in Celsius
    pub dimm_temp_max: Option<f64>,
    /// Disk temperatures as comma-separated string (e.g., "nvme0:55.0,nvme1:52.0")
    pub disk_temps: Option<String>,
    /// Source of disk temperature (e.g., "nvme hwmon", "smartctl")
    pub disk_temp_source: Option<String>,
    /// Maximum disk temperature in Celsius (from NVMe or SMART)
    pub disk_temp_max: Option<f64>,

    // ===== Context Switches and Interrupts (delta) =====
    /// Number of context switches
    pub context_switches: u64,
    /// Number of interrupts serviced
    pub interrupts: u64,

    // ===== Memory Details (from /proc/meminfo) =====
    /// Memory waiting to be written to disk in MB
    pub dirty_mb: u64,
    /// Memory currently being written to disk in MB
    pub writeback_mb: u64,
    /// Anonymous (non-file-backed) memory in MB
    pub anon_pages_mb: u64,
    /// Memory mapped into processes in MB
    pub mapped_mb: u64,
    /// Shared memory (tmpfs, etc.) in MB
    pub shmem_mb: u64,
    /// Kernel slab allocator memory in MB
    pub slab_mb: u64,
    /// Memory used for page tables in MB
    pub page_tables_mb: u64,

    // ===== Virtual Memory Stats (delta since last sample) =====
    /// Minor page faults (no disk I/O needed)
    pub pgfault: u64,
    /// Major page faults (required disk I/O - HIGH = thrashing)
    pub pgmajfault: u64,
    /// Pages paged in from disk
    pub pgpgin: u64,
    /// Pages paged out to disk
    pub pgpgout: u64,
    /// Pages swapped in (HIGH = memory pressure)
    pub pswpin: u64,
    /// Pages swapped out (HIGH = memory pressure)
    pub pswpout: u64,

    // ===== File Descriptors =====
    /// Number of allocated file descriptors
    pub fd_allocated: u64,
    /// Maximum allowed file descriptors
    pub fd_max: u64,

    // ===== System =====
    /// System uptime in seconds
    pub uptime_secs: f64,

    // ===== SMART Health =====
    /// Whether SMART data is available
    pub smart_available: Option<bool>,
    /// Whether all disks passed health check
    pub smart_health_all_passed: Option<bool>,
    /// Total reallocated sectors across all disks
    pub smart_reallocated_sectors_total: Option<u64>,
    /// Total pending sectors across all disks
    pub smart_pending_sectors_total: Option<u64>,

    // ===== IPMI Sensors =====
    /// Whether IPMI data is available
    pub ipmi_available: Option<bool>,
    /// IPMI DIMM temperature (max across all DIMMs)
    pub ipmi_dimm_temp_max: Option<f64>,
    /// IPMI DIMM status (ok, nc, cr, nr)
    pub ipmi_dimm_status: Option<String>,
    /// Detailed IPMI DIMM info (e.g., "DIMMC1:99°C[NR], DIMMD1:100°C[NR]")
    pub ipmi_dimm_details: Option<String>,
    /// Individual IPMI DIMM temperatures for in-memory plotting.
    ///
    /// This is intentionally skipped in CSV output because CSV rows must have
    /// a stable scalar schema. The summarized `ipmi_dimm_details` field is
    /// the CSV representation.
    #[serde(skip_serializing)]
    pub ipmi_dimm_temps: Vec<IpmiDimmTemp>,
}

/// Individual DIMM temperature from IPMI.
#[derive(Serialize, Clone, Debug, Default)]
pub struct IpmiDimmTemp {
    /// Sensor name (e.g., "DIMMA1", "P1-DIMMC1")
    pub name: String,
    /// Temperature in Celsius
    pub temp_celsius: f64,
    /// Sensor status (ok, nc, cr, nr, na)
    pub status: String,
}

#[cfg(test)]
mod tests {
    use super::{IpmiDimmTemp, Metrics};

    #[test]
    fn csv_serialization_keeps_optional_columns_stable() {
        let mut writer = csv::Writer::from_writer(Vec::new());

        writer.serialize(Metrics::default()).unwrap();

        let with_optional_values = Metrics {
            smart_available: Some(true),
            smart_health_all_passed: Some(false),
            smart_reallocated_sectors_total: Some(7),
            smart_pending_sectors_total: Some(2),
            ipmi_available: Some(true),
            ipmi_dimm_temp_max: Some(64.0),
            ipmi_dimm_status: Some("ok".to_string()),
            ipmi_dimm_details: Some("DIMMA1:64C[ok]".to_string()),
            ..Metrics::default()
        };
        writer.serialize(with_optional_values).unwrap();

        let csv = String::from_utf8(writer.into_inner().unwrap()).unwrap();
        let mut rows = csv.lines();
        let header_len = rows.next().unwrap().split(',').count();

        for row in rows {
            assert_eq!(row.split(',').count(), header_len);
        }
    }

    #[test]
    fn csv_serialization_skips_in_memory_ipmi_vectors() {
        let metrics = Metrics {
            ipmi_dimm_details: Some("DIMMA1:64C[ok]".to_string()),
            ipmi_dimm_temps: vec![IpmiDimmTemp {
                name: "DIMMA1".to_string(),
                temp_celsius: 64.0,
                status: "ok".to_string(),
            }],
            ..Metrics::default()
        };

        let mut writer = csv::Writer::from_writer(Vec::new());
        writer.serialize(metrics).unwrap();
        let csv = String::from_utf8(writer.into_inner().unwrap()).unwrap();

        assert!(csv.contains("ipmi_dimm_details"));
        assert!(csv.contains("DIMMA1:64C[ok]"));
        assert!(!csv.contains("ipmi_dimm_temps"));
    }
}
