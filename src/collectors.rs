//! System statistics collectors for cargo-slow.
//!
//! This module provides functions to read various system metrics from
//! the Linux `/proc` filesystem and other system interfaces.
//!
//! # Data Sources
//!
//! - `/proc/meminfo` - Memory statistics
//! - `/proc/stat` - CPU and process statistics
//! - `/proc/diskstats` - Block device I/O statistics
//! - `/proc/net/dev` - Network interface statistics
//! - `/proc/pressure/*` - Pressure Stall Information (PSI)
//! - `/proc/vmstat` - Virtual memory statistics
//! - `/proc/uptime` - System uptime
//! - `/proc/sys/fs/file-nr` - File descriptor usage
//! - `/sys/class/hwmon/*/temp*` - Hardware temperatures

use crate::temperature::valid_sensor_temperature_celsius;

/// Detailed memory information from `/proc/meminfo`.
#[derive(Default, Clone, Debug)]
pub struct MemInfo {
    /// Buffer cache size in MB
    pub buffers: u64,
    /// Page cache size in MB
    pub cached: u64,
    /// Memory waiting to be written to disk in MB
    pub dirty: u64,
    /// Memory being written to disk in MB
    pub writeback: u64,
    /// Anonymous pages in MB
    pub anon_pages: u64,
    /// Mapped pages in MB
    pub mapped: u64,
    /// Shared memory in MB
    pub shmem: u64,
    /// Kernel slab in MB
    pub slab: u64,
    /// Page tables in MB
    pub page_tables: u64,
}

/// CPU time breakdown and process statistics from `/proc/stat`.
#[derive(Clone, Debug, Default)]
pub struct CpuStats {
    /// Time spent in user mode (jiffies)
    pub user: u64,
    /// Time spent in user mode with low priority (jiffies)
    pub nice: u64,
    /// Time spent in system mode (jiffies)
    pub system: u64,
    /// Time spent idle (jiffies)
    pub idle: u64,
    /// Time spent waiting for I/O (jiffies)
    pub iowait: u64,
    /// Time spent servicing hardware interrupts (jiffies)
    pub irq: u64,
    /// Time spent servicing software interrupts (jiffies)
    pub softirq: u64,
    /// Time stolen by hypervisor (jiffies)
    pub steal: u64,
    /// Total context switches since boot
    pub context_switches: u64,
    /// Total interrupts since boot
    pub interrupts: u64,
    /// Number of processes in runnable state
    pub procs_running: u64,
    /// Number of processes blocked on I/O
    pub procs_blocked: u64,
}

/// Disk I/O statistics from `/proc/diskstats`.
#[derive(Clone, Debug, Default)]
pub struct DiskStats {
    /// Reads completed successfully
    pub reads_completed: u64,
    /// Reads merged
    pub reads_merged: u64,
    /// Sectors read (512 bytes each)
    pub sectors_read: u64,
    /// Time spent reading (ms)
    pub read_time_ms: u64,
    /// Writes completed successfully
    pub writes_completed: u64,
    /// Writes merged
    pub writes_merged: u64,
    /// Sectors written
    pub sectors_written: u64,
    /// Time spent writing (ms)
    pub write_time_ms: u64,
    /// I/O operations currently in progress
    pub io_in_progress: u64,
    /// Time spent doing I/O (ms)
    pub io_time_ms: u64,
    /// Weighted time spent doing I/O (ms)
    pub weighted_io_time_ms: u64,
}

/// Network interface statistics from `/proc/net/dev`.
#[derive(Clone, Debug, Default)]
pub struct NetStats {
    /// Bytes received
    pub rx_bytes: u64,
    /// Bytes transmitted
    pub tx_bytes: u64,
    /// Packets received
    pub rx_packets: u64,
    /// Packets transmitted
    pub tx_packets: u64,
    /// Receive errors
    pub rx_errors: u64,
    /// Transmit errors
    pub tx_errors: u64,
}

/// Virtual memory statistics from `/proc/vmstat`.
#[derive(Clone, Debug, Default)]
pub struct VmStats {
    /// Minor page faults
    pub pgfault: u64,
    /// Major page faults (required I/O)
    pub pgmajfault: u64,
    /// Pages paged in
    pub pgpgin: u64,
    /// Pages paged out
    pub pgpgout: u64,
    /// Pages swapped in
    pub pswpin: u64,
    /// Pages swapped out
    pub pswpout: u64,
}

/// Pressure Stall Information from `/proc/pressure/*`.
#[derive(Default, Clone, Debug)]
pub struct PsiInfo {
    /// CPU: some tasks stalled (10s avg)
    pub cpu_some_avg10: Option<f64>,
    /// CPU: some tasks stalled (60s avg)
    pub cpu_some_avg60: Option<f64>,
    /// CPU: some tasks stalled (300s avg)
    pub cpu_some_avg300: Option<f64>,
    /// Memory: some tasks stalled (10s avg)
    pub mem_some_avg10: Option<f64>,
    /// Memory: some tasks stalled (60s avg)
    pub mem_some_avg60: Option<f64>,
    /// Memory: all tasks stalled (10s avg)
    pub mem_full_avg10: Option<f64>,
    /// I/O: some tasks stalled (10s avg)
    pub io_some_avg10: Option<f64>,
    /// I/O: some tasks stalled (60s avg)
    pub io_some_avg60: Option<f64>,
    /// I/O: all tasks stalled (10s avg)
    pub io_full_avg10: Option<f64>,
    /// I/O: all tasks stalled (60s avg)
    pub io_full_avg60: Option<f64>,
}

/// Individual DIMM temperature reading.
#[derive(Clone, Debug)]
pub struct DimmTemp {
    /// Label for the DIMM (e.g., "DIMM0", "Channel A DIMM 0")
    pub label: String,
    /// Temperature in Celsius
    pub temp_celsius: f64,
}

/// Temperature readings from hwmon interfaces.
#[derive(Clone, Debug, Default)]
pub struct TempInfo {
    /// CPU package temperature in Celsius
    pub cpu_temp: Option<f64>,
    /// Source of CPU temperature (e.g., "coretemp", "k10temp", "zenpower")
    pub cpu_temp_source: Option<String>,
    /// Maximum temperature across all sensors
    pub max_temp: Option<f64>,
    /// Individual DIMM temperatures from jc42 sensors
    pub dimm_temps: Vec<DimmTemp>,
    /// NVMe disk temperatures
    pub nvme_temps: Vec<(String, f64)>,
}

/// Read memory information from `/proc/meminfo`.
pub fn read_meminfo() -> MemInfo {
    let mut info = MemInfo::default();

    if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let value: u64 = parts[1].parse().unwrap_or(0) / 1024; // KB to MB
                match parts[0] {
                    "Buffers:" => info.buffers = value,
                    "Cached:" => info.cached = value,
                    "Dirty:" => info.dirty = value,
                    "Writeback:" => info.writeback = value,
                    "AnonPages:" => info.anon_pages = value,
                    "Mapped:" => info.mapped = value,
                    "Shmem:" => info.shmem = value,
                    "Slab:" => info.slab = value,
                    "PageTables:" => info.page_tables = value,
                    _ => {}
                }
            }
        }
    }

    info
}

/// Read CPU statistics from `/proc/stat`.
pub fn read_cpu_stats() -> Option<CpuStats> {
    let content = std::fs::read_to_string("/proc/stat").ok()?;
    let mut stats = CpuStats::default();

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "cpu" if parts.len() >= 9 => {
                stats.user = parts[1].parse().unwrap_or(0);
                stats.nice = parts[2].parse().unwrap_or(0);
                stats.system = parts[3].parse().unwrap_or(0);
                stats.idle = parts[4].parse().unwrap_or(0);
                stats.iowait = parts[5].parse().unwrap_or(0);
                stats.irq = parts[6].parse().unwrap_or(0);
                stats.softirq = parts[7].parse().unwrap_or(0);
                stats.steal = parts.get(8).and_then(|s| s.parse().ok()).unwrap_or(0);
            }
            "ctxt" if parts.len() >= 2 => {
                stats.context_switches = parts[1].parse().unwrap_or(0);
            }
            "intr" if parts.len() >= 2 => {
                stats.interrupts = parts[1].parse().unwrap_or(0);
            }
            "procs_running" if parts.len() >= 2 => {
                stats.procs_running = parts[1].parse().unwrap_or(0);
            }
            "procs_blocked" if parts.len() >= 2 => {
                stats.procs_blocked = parts[1].parse().unwrap_or(0);
            }
            _ => {}
        }
    }

    Some(stats)
}

/// Read disk I/O statistics from `/proc/diskstats`.
///
/// Only counts whole-disk devices (sda, nvme0n1, vda, xvda), not partitions.
pub fn read_disk_stats() -> Option<DiskStats> {
    let content = std::fs::read_to_string("/proc/diskstats").ok()?;
    let mut stats = DiskStats::default();

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 14 {
            continue;
        }

        let device = parts[2];
        // Only count real block devices, not partitions
        let is_disk = (device.starts_with("sd") && device.len() == 3)
            || (device.starts_with("nvme") && device.contains('n') && !device.contains('p'))
            || (device.starts_with("vd") && device.len() == 3)
            || (device.starts_with("xvd") && device.len() == 4);

        if is_disk {
            stats.reads_completed += parts[3].parse::<u64>().unwrap_or(0);
            stats.reads_merged += parts[4].parse::<u64>().unwrap_or(0);
            stats.sectors_read += parts[5].parse::<u64>().unwrap_or(0);
            stats.read_time_ms += parts[6].parse::<u64>().unwrap_or(0);
            stats.writes_completed += parts[7].parse::<u64>().unwrap_or(0);
            stats.writes_merged += parts[8].parse::<u64>().unwrap_or(0);
            stats.sectors_written += parts[9].parse::<u64>().unwrap_or(0);
            stats.write_time_ms += parts[10].parse::<u64>().unwrap_or(0);
            stats.io_in_progress += parts[11].parse::<u64>().unwrap_or(0);
            stats.io_time_ms += parts[12].parse::<u64>().unwrap_or(0);
            stats.weighted_io_time_ms += parts[13].parse::<u64>().unwrap_or(0);
        }
    }

    Some(stats)
}

/// Read network statistics from `/proc/net/dev`.
///
/// Aggregates stats across all interfaces except loopback.
pub fn read_net_stats() -> Option<NetStats> {
    let content = std::fs::read_to_string("/proc/net/dev").ok()?;
    let mut stats = NetStats::default();

    for line in content.lines().skip(2) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 12 {
            continue;
        }

        let iface = parts[0].trim_end_matches(':');
        // Skip loopback
        if iface == "lo" {
            continue;
        }

        stats.rx_bytes += parts[1].parse::<u64>().unwrap_or(0);
        stats.rx_packets += parts[2].parse::<u64>().unwrap_or(0);
        stats.rx_errors += parts[3].parse::<u64>().unwrap_or(0);
        stats.tx_bytes += parts[9].parse::<u64>().unwrap_or(0);
        stats.tx_packets += parts[10].parse::<u64>().unwrap_or(0);
        stats.tx_errors += parts[11].parse::<u64>().unwrap_or(0);
    }

    Some(stats)
}

/// Read Pressure Stall Information from `/proc/pressure/*`.
///
/// PSI is available on Linux 4.20+ with CONFIG_PSI enabled.
pub fn read_psi() -> PsiInfo {
    let mut psi = PsiInfo::default();

    // CPU pressure
    if let Ok(content) = std::fs::read_to_string("/proc/pressure/cpu") {
        for line in content.lines() {
            if line.starts_with("some") {
                psi.cpu_some_avg10 = extract_psi_value(line, "avg10");
                psi.cpu_some_avg60 = extract_psi_value(line, "avg60");
                psi.cpu_some_avg300 = extract_psi_value(line, "avg300");
            }
        }
    }

    // Memory pressure
    if let Ok(content) = std::fs::read_to_string("/proc/pressure/memory") {
        for line in content.lines() {
            if line.starts_with("some") {
                psi.mem_some_avg10 = extract_psi_value(line, "avg10");
                psi.mem_some_avg60 = extract_psi_value(line, "avg60");
            }
            if line.starts_with("full") {
                psi.mem_full_avg10 = extract_psi_value(line, "avg10");
            }
        }
    }

    // I/O pressure
    if let Ok(content) = std::fs::read_to_string("/proc/pressure/io") {
        for line in content.lines() {
            if line.starts_with("some") {
                psi.io_some_avg10 = extract_psi_value(line, "avg10");
                psi.io_some_avg60 = extract_psi_value(line, "avg60");
            }
            if line.starts_with("full") {
                psi.io_full_avg10 = extract_psi_value(line, "avg10");
                psi.io_full_avg60 = extract_psi_value(line, "avg60");
            }
        }
    }

    psi
}

/// Extract a value from a PSI line (e.g., "avg10=1.23").
fn extract_psi_value(line: &str, key: &str) -> Option<f64> {
    line.split_whitespace()
        .find_map(|w| w.strip_prefix(&format!("{}=", key)))
        .and_then(|v| v.parse().ok())
}

/// Read temperatures from hwmon interfaces.
///
/// Looks for CPU-specific sensors (coretemp, k10temp, zenpower),
/// DIMM sensors (jc42), NVMe sensors, and tracks the maximum
/// temperature across all sensors.
pub fn read_temperatures() -> TempInfo {
    let mut info = TempInfo::default();
    let mut max_temp: Option<f64> = None;
    let mut dimm_index = 0;
    let mut nvme_index = 0;

    if let Ok(entries) = std::fs::read_dir("/sys/class/hwmon") {
        for entry in entries.flatten() {
            let path = entry.path();

            // Check device name for sensor type
            let name_path = path.join("name");
            let name = std::fs::read_to_string(&name_path)
                .unwrap_or_default()
                .trim()
                .to_string();

            let is_cpu =
                name.contains("coretemp") || name.contains("k10temp") || name.contains("zenpower");
            let is_jc42 = name == "jc42";
            let is_nvme = name == "nvme";

            // Read all temperature inputs
            for i in 1..=20 {
                let temp_path = path.join(format!("temp{}_input", i));
                if let Ok(temp_str) = std::fs::read_to_string(&temp_path) {
                    if let Ok(temp_millic) = temp_str.trim().parse::<i64>() {
                        let Some(temp) =
                            valid_sensor_temperature_celsius(temp_millic as f64 / 1000.0)
                        else {
                            continue;
                        };

                        if is_cpu && info.cpu_temp.is_none() {
                            info.cpu_temp = Some(temp);
                            info.cpu_temp_source = Some(name.clone());
                        }

                        // Collect DIMM temperatures from jc42 sensors
                        if is_jc42 {
                            let label_path = path.join(format!("temp{}_label", i));
                            let label = std::fs::read_to_string(&label_path)
                                .map(|s| s.trim().to_string())
                                .unwrap_or_else(|_| format!("DIMM{}", dimm_index));
                            info.dimm_temps.push(DimmTemp {
                                label,
                                temp_celsius: temp,
                            });
                            dimm_index += 1;
                        }

                        // Collect NVMe temperatures (only first/composite temp per device)
                        if is_nvme && i == 1 {
                            let device_path = path.join("device");
                            let device_name = if let Ok(link) = std::fs::read_link(&device_path) {
                                link.file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("nvme")
                                    .to_string()
                            } else {
                                format!("nvme{}", nvme_index)
                            };
                            info.nvme_temps.push((device_name, temp));
                            nvme_index += 1;
                        }

                        max_temp = Some(max_temp.map_or(temp, |m: f64| m.max(temp)));
                    }
                }
            }
        }
    }

    info.max_temp = max_temp;
    info
}

/// Get average DIMM temperature.
pub fn dimm_temp_avg(temps: &[DimmTemp]) -> Option<f64> {
    let (sum, count) = temps
        .iter()
        .filter_map(|d| valid_sensor_temperature_celsius(d.temp_celsius))
        .fold((0.0, 0usize), |(sum, count), temp| (sum + temp, count + 1));

    if count == 0 {
        return None;
    }

    Some(sum / count as f64)
}

/// Get maximum DIMM temperature.
pub fn dimm_temp_max(temps: &[DimmTemp]) -> Option<f64> {
    temps
        .iter()
        .filter_map(|d| valid_sensor_temperature_celsius(d.temp_celsius))
        .fold(None, |acc, t| Some(acc.map_or(t, |a: f64| a.max(t))))
}

/// Read virtual memory statistics from `/proc/vmstat`.
pub fn read_vmstat() -> Option<VmStats> {
    let content = std::fs::read_to_string("/proc/vmstat").ok()?;
    let mut stats = VmStats::default();

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let value: u64 = parts[1].parse().unwrap_or(0);
            match parts[0] {
                "pgfault" => stats.pgfault = value,
                "pgmajfault" => stats.pgmajfault = value,
                "pgpgin" => stats.pgpgin = value,
                "pgpgout" => stats.pgpgout = value,
                "pswpin" => stats.pswpin = value,
                "pswpout" => stats.pswpout = value,
                _ => {}
            }
        }
    }

    Some(stats)
}

/// Read file descriptor statistics from `/proc/sys/fs/file-nr`.
///
/// Returns (allocated, max).
pub fn read_fd_stats() -> (u64, u64) {
    if let Ok(content) = std::fs::read_to_string("/proc/sys/fs/file-nr") {
        let parts: Vec<&str> = content.split_whitespace().collect();
        if parts.len() >= 3 {
            let allocated: u64 = parts[0].parse().unwrap_or(0);
            let max: u64 = parts[2].parse().unwrap_or(0);
            return (allocated, max);
        }
    }
    (0, 0)
}

/// Read system uptime from `/proc/uptime`.
pub fn read_uptime() -> f64 {
    if let Ok(content) = std::fs::read_to_string("/proc/uptime") {
        if let Some(uptime_str) = content.split_whitespace().next() {
            return uptime_str.parse().unwrap_or(0.0);
        }
    }
    0.0
}

impl DiskStats {
    /// Calculate the difference between two disk stats snapshots.
    pub fn delta(&self, other: &Self) -> Self {
        Self {
            reads_completed: other.reads_completed.saturating_sub(self.reads_completed),
            reads_merged: other.reads_merged.saturating_sub(self.reads_merged),
            sectors_read: other.sectors_read.saturating_sub(self.sectors_read),
            read_time_ms: other.read_time_ms.saturating_sub(self.read_time_ms),
            writes_completed: other.writes_completed.saturating_sub(self.writes_completed),
            writes_merged: other.writes_merged.saturating_sub(self.writes_merged),
            sectors_written: other.sectors_written.saturating_sub(self.sectors_written),
            write_time_ms: other.write_time_ms.saturating_sub(self.write_time_ms),
            io_in_progress: other.io_in_progress,
            io_time_ms: other.io_time_ms.saturating_sub(self.io_time_ms),
            weighted_io_time_ms: other
                .weighted_io_time_ms
                .saturating_sub(self.weighted_io_time_ms),
        }
    }
}

impl CpuStats {
    /// Calculate the difference between two CPU stats snapshots.
    pub fn delta(&self, other: &Self) -> Self {
        Self {
            user: other.user.saturating_sub(self.user),
            nice: other.nice.saturating_sub(self.nice),
            system: other.system.saturating_sub(self.system),
            idle: other.idle.saturating_sub(self.idle),
            iowait: other.iowait.saturating_sub(self.iowait),
            irq: other.irq.saturating_sub(self.irq),
            softirq: other.softirq.saturating_sub(self.softirq),
            steal: other.steal.saturating_sub(self.steal),
            context_switches: other.context_switches.saturating_sub(self.context_switches),
            interrupts: other.interrupts.saturating_sub(self.interrupts),
            procs_running: other.procs_running,
            procs_blocked: other.procs_blocked,
        }
    }
}

impl NetStats {
    /// Calculate the difference between two network stats snapshots.
    pub fn delta(&self, other: &Self) -> Self {
        Self {
            rx_bytes: other.rx_bytes.saturating_sub(self.rx_bytes),
            tx_bytes: other.tx_bytes.saturating_sub(self.tx_bytes),
            rx_packets: other.rx_packets.saturating_sub(self.rx_packets),
            tx_packets: other.tx_packets.saturating_sub(self.tx_packets),
            rx_errors: other.rx_errors.saturating_sub(self.rx_errors),
            tx_errors: other.tx_errors.saturating_sub(self.tx_errors),
        }
    }
}

impl VmStats {
    /// Calculate the difference between two VM stats snapshots.
    pub fn delta(&self, other: &Self) -> Self {
        Self {
            pgfault: other.pgfault.saturating_sub(self.pgfault),
            pgmajfault: other.pgmajfault.saturating_sub(self.pgmajfault),
            pgpgin: other.pgpgin.saturating_sub(self.pgpgin),
            pgpgout: other.pgpgout.saturating_sub(self.pgpgout),
            pswpin: other.pswpin.saturating_sub(self.pswpin),
            pswpout: other.pswpout.saturating_sub(self.pswpout),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        dimm_temp_avg, dimm_temp_max, extract_psi_value, CpuStats, DimmTemp, DiskStats, NetStats,
        VmStats,
    };

    #[test]
    fn extracts_psi_values_by_key() {
        let line = "some avg10=1.23 avg60=4.56 avg300=7.89 total=42";

        assert_eq!(extract_psi_value(line, "avg10"), Some(1.23));
        assert_eq!(extract_psi_value(line, "avg60"), Some(4.56));
        assert_eq!(extract_psi_value(line, "missing"), None);
    }

    #[test]
    fn temperature_helpers_handle_empty_and_non_empty_inputs() {
        assert_eq!(dimm_temp_avg(&[]), None);
        assert_eq!(dimm_temp_max(&[]), None);

        let dimms = vec![
            DimmTemp {
                label: "A1".to_string(),
                temp_celsius: 40.0,
            },
            DimmTemp {
                label: "B1".to_string(),
                temp_celsius: 50.0,
            },
            DimmTemp {
                label: "dead".to_string(),
                temp_celsius: 1000.0,
            },
        ];
        assert_eq!(dimm_temp_avg(&dimms), Some(45.0));
        assert_eq!(dimm_temp_max(&dimms), Some(50.0));
    }

    #[test]
    fn disk_delta_saturates_counters_and_keeps_instantaneous_depth() {
        let last = DiskStats {
            reads_completed: 10,
            sectors_read: 20,
            writes_completed: 30,
            io_in_progress: 99,
            io_time_ms: 100,
            weighted_io_time_ms: 200,
            ..DiskStats::default()
        };
        let current = DiskStats {
            reads_completed: 15,
            sectors_read: 5,
            writes_completed: 40,
            io_in_progress: 3,
            io_time_ms: 140,
            weighted_io_time_ms: 260,
            ..DiskStats::default()
        };

        let delta = last.delta(&current);

        assert_eq!(delta.reads_completed, 5);
        assert_eq!(delta.sectors_read, 0);
        assert_eq!(delta.writes_completed, 10);
        assert_eq!(delta.io_in_progress, 3);
        assert_eq!(delta.io_time_ms, 40);
        assert_eq!(delta.weighted_io_time_ms, 60);
    }

    #[test]
    fn cpu_net_and_vm_deltas_use_saturating_counters() {
        let cpu_delta = CpuStats {
            user: 5,
            procs_running: 99,
            ..CpuStats::default()
        }
        .delta(&CpuStats {
            user: 9,
            idle: 1,
            procs_running: 2,
            procs_blocked: 1,
            ..CpuStats::default()
        });
        assert_eq!(cpu_delta.user, 4);
        assert_eq!(cpu_delta.procs_running, 2);
        assert_eq!(cpu_delta.procs_blocked, 1);

        let net_delta = NetStats {
            rx_bytes: 100,
            tx_bytes: 100,
            ..NetStats::default()
        }
        .delta(&NetStats {
            rx_bytes: 150,
            tx_bytes: 90,
            ..NetStats::default()
        });
        assert_eq!(net_delta.rx_bytes, 50);
        assert_eq!(net_delta.tx_bytes, 0);

        let vm_delta = VmStats {
            pgfault: 10,
            pswpout: 5,
            ..VmStats::default()
        }
        .delta(&VmStats {
            pgfault: 11,
            pswpout: 3,
            ..VmStats::default()
        });
        assert_eq!(vm_delta.pgfault, 1);
        assert_eq!(vm_delta.pswpout, 0);
    }
}
