//! Application state and logic for slow-rs.
//!
//! This module contains the main [`App`] struct which coordinates
//! metrics collection, logging, and the user interface.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::path::Path;

use chrono::Utc;
use sysinfo::System;

use crate::availability::MetricAvailability;
use crate::benchmarks::{self, IoBenchmarkResult};
use crate::collectors::{self, CpuStats, DiskStats, NetStats, VmStats};
use crate::config::Config;
use crate::ipmi::IpmiSensors;
use crate::metrics::{DiskTempReading, Metrics};
use crate::smart::SmartHealth;
use crate::temperature::valid_sensor_temperature_celsius;
use crate::thresholds::Thresholds;

/// Main application state.
///
/// Holds configuration, system state, metrics history, and handles
/// coordination between data collection and logging.
pub struct App {
    /// Application configuration from CLI
    pub config: Config,

    /// Historical metrics for plotting
    pub metrics_history: VecDeque<Metrics>,

    /// CSV writer for logging
    csv_writer: Option<csv::Writer<File>>,

    /// System information collector
    sys: System,

    /// Previous disk stats for delta calculation
    last_disk_stats: Option<DiskStats>,

    /// Previous network stats for delta calculation
    last_net_stats: Option<NetStats>,

    /// Previous CPU stats for delta calculation
    last_cpu_stats: Option<CpuStats>,

    /// Previous VM stats for delta calculation
    last_vm_stats: Option<VmStats>,

    /// Metric source availability
    pub availability: MetricAvailability,

    /// Threshold configuration
    pub thresholds: Thresholds,

    /// Cached SMART health (collected less frequently)
    last_smart_health: Option<SmartHealth>,

    /// Counter for SMART collection interval
    smart_collection_counter: u32,

    /// Cached IPMI sensors (collected less frequently)
    last_ipmi_sensors: Option<IpmiSensors>,

    /// Counter for IPMI collection interval
    ipmi_collection_counter: u32,
}

impl App {
    /// Create a new application instance.
    ///
    /// This initializes the CSV writer, system info collector, and
    /// prepares for metrics collection.
    ///
    /// # Arguments
    ///
    /// * `config` - Application configuration
    ///
    /// # Errors
    ///
    /// Returns an error if the CSV file cannot be opened.
    pub fn new(config: Config) -> std::io::Result<Self> {
        // Initialize CSV writer (append mode, write header for new/empty files).
        let write_headers = should_write_csv_headers(Path::new(&config.csv_file));
        let csv_file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&config.csv_file)?;

        let csv_writer = csv::WriterBuilder::new()
            .has_headers(write_headers)
            .from_writer(csv_file);

        let history_size = config.history_size;

        // Probe metric availability at startup
        let availability = MetricAvailability::probe();

        Ok(Self {
            config,
            metrics_history: VecDeque::with_capacity(history_size),
            csv_writer: Some(csv_writer),
            sys: System::new_all(),
            last_disk_stats: None,
            last_net_stats: None,
            last_cpu_stats: None,
            last_vm_stats: None,
            availability,
            thresholds: Thresholds::default(),
            last_smart_health: None,
            smart_collection_counter: 0,
            last_ipmi_sensors: None,
            ipmi_collection_counter: 0,
        })
    }

    /// Ensure the I/O benchmark test file exists.
    ///
    /// If the file doesn't exist, creates it with the configured size.
    /// This is skipped unless `--io-bench` was specified.
    pub fn ensure_test_file(&self) -> std::io::Result<()> {
        if !self.config.io_bench {
            return Ok(());
        }

        let path = Path::new(&self.config.test_file);
        if !path.exists() {
            benchmarks::create_test_file(&self.config.test_file, self.config.file_size_mb)?;
        }

        Ok(())
    }

    /// Collect all metrics and run benchmarks.
    ///
    /// This is the main collection function that:
    /// 1. Runs active benchmarks (allocation, compute, I/O)
    /// 2. Reads system stats from sysinfo
    /// 3. Reads detailed stats from /proc
    /// 4. Calculates deltas from previous measurements
    /// 5. Logs the metrics to CSV
    ///
    /// # Returns
    ///
    /// The collected metrics snapshot.
    pub fn collect_metrics(&mut self) -> std::io::Result<Metrics> {
        let now = Utc::now();
        let timestamp = now.timestamp();
        let datetime = now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();

        // Refresh system info
        self.sys.refresh_all();

        // === Run benchmarks ===
        let alloc_duration = benchmarks::benchmark_allocation();
        let compute_duration = benchmarks::benchmark_compute();

        let (io_read, io_write, sha_duration) = if self.config.io_bench {
            match benchmarks::benchmark_io(&self.config.test_file, self.config.file_size_mb) {
                Ok(IoBenchmarkResult {
                    read_mb_per_sec,
                    write_mb_per_sec,
                    sha_duration_ms,
                }) => (
                    Some(read_mb_per_sec),
                    Some(write_mb_per_sec),
                    Some(sha_duration_ms),
                ),
                Err(_) => (None, None, None),
            }
        } else {
            (None, None, None)
        };

        // === System stats from sysinfo ===
        let mem_total = self.sys.total_memory() / 1024 / 1024;
        let mem_used = self.sys.used_memory() / 1024 / 1024;
        let mem_free = self.sys.free_memory() / 1024 / 1024;
        let mem_available = self.sys.available_memory() / 1024 / 1024;
        let swap_total = self.sys.total_swap() / 1024 / 1024;
        let swap_used = self.sys.used_swap() / 1024 / 1024;

        let cpu_usage: f32 = self.sys.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>()
            / self.sys.cpus().len().max(1) as f32;
        let cpu_count = self.sys.cpus().len();

        let load = System::load_average();
        let process_count = self.sys.processes().len();

        // === Stats from /proc ===
        let meminfo = collectors::read_meminfo();
        let cpu_stats = collectors::read_cpu_stats();
        let disk_stats = collectors::read_disk_stats();
        let net_stats = collectors::read_net_stats();
        let psi = collectors::read_psi();
        let temps = collectors::read_temperatures();
        let vm_stats = collectors::read_vmstat();
        let (fd_allocated, fd_max) = collectors::read_fd_stats();
        let uptime = collectors::read_uptime();

        // === Collect SMART health (every 12 iterations = ~1 minute at 5s interval) ===
        self.smart_collection_counter += 1;
        if self.smart_collection_counter >= 12 || self.last_smart_health.is_none() {
            self.last_smart_health = Some(SmartHealth::collect());
            self.smart_collection_counter = 0;
        }
        let smart = self.last_smart_health.as_ref();

        // === Collect IPMI sensors (every 12 iterations = ~1 minute at 5s interval) ===
        self.ipmi_collection_counter += 1;
        if self.ipmi_collection_counter >= 12 || self.last_ipmi_sensors.is_none() {
            self.last_ipmi_sensors = Some(IpmiSensors::collect());
            self.ipmi_collection_counter = 0;
        }
        let ipmi = self.last_ipmi_sensors.as_ref();

        // === Process DIMM temperatures ===
        let dimm_temps_str = if temps.dimm_temps.is_empty() {
            None
        } else {
            Some(
                temps
                    .dimm_temps
                    .iter()
                    .map(|d| format!("{}:{:.1}", d.label, d.temp_celsius))
                    .collect::<Vec<_>>()
                    .join(","),
            )
        };
        let dimm_temp_avg = collectors::dimm_temp_avg(&temps.dimm_temps);
        let dimm_temp_max = collectors::dimm_temp_max(&temps.dimm_temps);

        // === Determine disk temperature (NVMe hwmon plus SMART for non-NVMe disks) ===
        let smart_temps = smart.map(|s| s.device_temperatures()).unwrap_or_default();
        let disk_temps_snapshot = merge_disk_temperatures(&temps.nvme_temps, &smart_temps);

        // === Unsafe shutdown counters (None unless at least one disk reports one) ===
        let unsafe_shutdowns = smart
            .filter(|s| s.available)
            .map(|s| s.unsafe_shutdowns())
            .filter(|readings| !readings.is_empty());

        // === Determine DIMM temperature source ===
        let dimm_temp_source = if !temps.dimm_temps.is_empty() {
            Some("jc42 hwmon".to_string())
        } else if ipmi
            .map(|s| s.available && s.max_dimm_temp().is_some())
            .unwrap_or(false)
        {
            Some("ipmi".to_string())
        } else {
            None
        };

        // === Calculate deltas ===
        let disk_delta = self
            .last_disk_stats
            .as_ref()
            .zip(disk_stats.as_ref())
            .map(|(last, cur)| last.delta(cur));

        let cpu_delta = self
            .last_cpu_stats
            .as_ref()
            .zip(cpu_stats.as_ref())
            .map(|(last, cur)| last.delta(cur));

        let net_delta = self
            .last_net_stats
            .as_ref()
            .zip(net_stats.as_ref())
            .map(|(last, cur)| last.delta(cur));

        let vm_delta = self
            .last_vm_stats
            .as_ref()
            .zip(vm_stats.as_ref())
            .map(|(last, cur)| last.delta(cur));

        // Build metrics struct
        let metrics = Metrics {
            timestamp,
            datetime,

            io_read_mb_per_sec: io_read,
            io_write_mb_per_sec: io_write,
            sha256_duration_ms: sha_duration,
            memory_alloc_duration_ms: alloc_duration,
            compute_duration_ms: compute_duration,

            mem_total_mb: mem_total,
            mem_used_mb: mem_used,
            mem_free_mb: mem_free,
            mem_available_mb: mem_available,
            swap_total_mb: swap_total,
            swap_used_mb: swap_used,
            mem_buffers_mb: meminfo.buffers,
            mem_cached_mb: meminfo.cached,

            cpu_usage_percent: cpu_usage,
            cpu_count,

            load_avg_1: load.one,
            load_avg_5: load.five,
            load_avg_15: load.fifteen,

            process_count,
            thread_count: cpu_stats
                .as_ref()
                .map(|s| s.procs_running + s.procs_blocked)
                .unwrap_or(0),
            procs_running: cpu_delta.as_ref().map(|s| s.procs_running).unwrap_or(0),
            procs_blocked: cpu_delta.as_ref().map(|s| s.procs_blocked).unwrap_or(0),

            cpu_user: cpu_delta.as_ref().map(|s| s.user).unwrap_or(0),
            cpu_nice: cpu_delta.as_ref().map(|s| s.nice).unwrap_or(0),
            cpu_system: cpu_delta.as_ref().map(|s| s.system).unwrap_or(0),
            cpu_idle: cpu_delta.as_ref().map(|s| s.idle).unwrap_or(0),
            cpu_iowait: cpu_delta.as_ref().map(|s| s.iowait).unwrap_or(0),
            cpu_irq: cpu_delta.as_ref().map(|s| s.irq).unwrap_or(0),
            cpu_softirq: cpu_delta.as_ref().map(|s| s.softirq).unwrap_or(0),
            cpu_steal: cpu_delta.as_ref().map(|s| s.steal).unwrap_or(0),

            disk_reads_completed: disk_delta.as_ref().map(|s| s.reads_completed).unwrap_or(0),
            disk_reads_merged: disk_delta.as_ref().map(|s| s.reads_merged).unwrap_or(0),
            disk_sectors_read: disk_delta.as_ref().map(|s| s.sectors_read).unwrap_or(0),
            disk_read_time_ms: disk_delta.as_ref().map(|s| s.read_time_ms).unwrap_or(0),
            disk_writes_completed: disk_delta.as_ref().map(|s| s.writes_completed).unwrap_or(0),
            disk_writes_merged: disk_delta.as_ref().map(|s| s.writes_merged).unwrap_or(0),
            disk_sectors_written: disk_delta.as_ref().map(|s| s.sectors_written).unwrap_or(0),
            disk_write_time_ms: disk_delta.as_ref().map(|s| s.write_time_ms).unwrap_or(0),
            disk_io_in_progress: disk_stats.as_ref().map(|s| s.io_in_progress).unwrap_or(0),
            disk_io_time_ms: disk_delta.as_ref().map(|s| s.io_time_ms).unwrap_or(0),
            disk_weighted_io_time_ms: disk_delta
                .as_ref()
                .map(|s| s.weighted_io_time_ms)
                .unwrap_or(0),

            net_rx_bytes: net_delta.as_ref().map(|s| s.rx_bytes).unwrap_or(0),
            net_tx_bytes: net_delta.as_ref().map(|s| s.tx_bytes).unwrap_or(0),
            net_rx_packets: net_delta.as_ref().map(|s| s.rx_packets).unwrap_or(0),
            net_tx_packets: net_delta.as_ref().map(|s| s.tx_packets).unwrap_or(0),
            net_rx_errors: net_delta.as_ref().map(|s| s.rx_errors).unwrap_or(0),
            net_tx_errors: net_delta.as_ref().map(|s| s.tx_errors).unwrap_or(0),

            cpu_pressure_some_avg10: psi.cpu_some_avg10,
            cpu_pressure_some_avg60: psi.cpu_some_avg60,
            cpu_pressure_some_avg300: psi.cpu_some_avg300,
            mem_pressure_some_avg10: psi.mem_some_avg10,
            mem_pressure_some_avg60: psi.mem_some_avg60,
            mem_pressure_full_avg10: psi.mem_full_avg10,
            io_pressure_some_avg10: psi.io_some_avg10,
            io_pressure_some_avg60: psi.io_some_avg60,
            io_pressure_full_avg10: psi.io_full_avg10,
            io_pressure_full_avg60: psi.io_full_avg60,

            cpu_temp_celsius: temps.cpu_temp,
            cpu_temp_source: temps.cpu_temp_source,
            max_temp_celsius: temps.max_temp,
            dimm_temps: dimm_temps_str,
            dimm_temp_source,
            dimm_temp_avg,
            dimm_temp_max,
            disk_temps: disk_temps_snapshot.temps,
            disk_temp_source: disk_temps_snapshot.source,
            disk_temp_max: disk_temps_snapshot.max,
            disk_temp_readings: disk_temps_snapshot.readings,

            context_switches: cpu_delta.as_ref().map(|s| s.context_switches).unwrap_or(0),
            interrupts: cpu_delta.as_ref().map(|s| s.interrupts).unwrap_or(0),

            dirty_mb: meminfo.dirty,
            writeback_mb: meminfo.writeback,
            anon_pages_mb: meminfo.anon_pages,
            mapped_mb: meminfo.mapped,
            shmem_mb: meminfo.shmem,
            slab_mb: meminfo.slab,
            page_tables_mb: meminfo.page_tables,

            pgfault: vm_delta.as_ref().map(|s| s.pgfault).unwrap_or(0),
            pgmajfault: vm_delta.as_ref().map(|s| s.pgmajfault).unwrap_or(0),
            pgpgin: vm_delta.as_ref().map(|s| s.pgpgin).unwrap_or(0),
            pgpgout: vm_delta.as_ref().map(|s| s.pgpgout).unwrap_or(0),
            pswpin: vm_delta.as_ref().map(|s| s.pswpin).unwrap_or(0),
            pswpout: vm_delta.as_ref().map(|s| s.pswpout).unwrap_or(0),

            fd_allocated,
            fd_max,

            uptime_secs: uptime,

            smart_available: smart.map(|s| s.available),
            smart_health_all_passed: smart.filter(|s| s.available).map(|s| s.all_healthy()),
            smart_reallocated_sectors_total: smart
                .filter(|s| s.available)
                .map(|s| s.total_reallocated_sectors()),
            smart_pending_sectors_total: smart
                .filter(|s| s.available)
                .map(|s| s.total_pending_sectors()),
            smart_unsafe_shutdowns_total: unsafe_shutdowns
                .as_ref()
                .map(|readings| readings.iter().map(|(_, count)| count).sum()),
            smart_unsafe_shutdowns: unsafe_shutdowns.as_ref().map(|readings| {
                readings
                    .iter()
                    .map(|(name, count)| format!("{}:{}", normalize_disk_name(name), count))
                    .collect::<Vec<_>>()
                    .join(",")
            }),

            ipmi_available: ipmi.map(|s| s.available),
            ipmi_dimm_temp_max: ipmi.filter(|s| s.available).and_then(|s| s.max_dimm_temp()),
            ipmi_dimm_status: ipmi
                .filter(|s| s.available)
                .map(|s| match s.worst_dimm_status() {
                    crate::ipmi::SensorStatus::Ok => "ok".to_string(),
                    crate::ipmi::SensorStatus::NonCritical => "nc".to_string(),
                    crate::ipmi::SensorStatus::Critical => "cr".to_string(),
                    crate::ipmi::SensorStatus::NonRecoverable => "nr".to_string(),
                    crate::ipmi::SensorStatus::NotAvailable => "na".to_string(),
                }),
            ipmi_dimm_details: ipmi
                .filter(|s| s.available)
                .and_then(|s| s.format_all_dimms()),
            ipmi_dimm_temps: ipmi
                .filter(|s| s.available)
                .map(|s| s.get_dimm_temps())
                .unwrap_or_default(),
        };

        // Store current stats for next delta calculation
        self.last_disk_stats = disk_stats;
        self.last_net_stats = net_stats;
        self.last_cpu_stats = cpu_stats;
        self.last_vm_stats = vm_stats;

        // Log to CSV
        self.log_metrics(&metrics)?;

        Ok(metrics)
    }

    /// Log metrics to CSV file.
    fn log_metrics(&mut self, metrics: &Metrics) -> std::io::Result<()> {
        if let Some(ref mut writer) = self.csv_writer {
            writer.serialize(metrics).map_err(std::io::Error::other)?;
            writer.flush()?;
        }
        Ok(())
    }
}

struct DiskTempSnapshot {
    temps: Option<String>,
    max: Option<f64>,
    source: Option<String>,
    readings: Vec<DiskTempReading>,
}

fn merge_disk_temperatures(
    nvme_temps: &[(String, f64)],
    smart_temps: &[(String, f64)],
) -> DiskTempSnapshot {
    let mut merged = nvme_temps.to_vec();

    for (name, temp) in smart_temps {
        if !nvme_temps.is_empty() && is_nvme_disk_name(name) {
            continue;
        }

        let normalized = normalize_disk_name(name);
        if !merged
            .iter()
            .any(|(existing_name, _)| normalize_disk_name(existing_name) == normalized)
        {
            merged.push((name.clone(), *temp));
        }
    }

    let readings = disk_temp_readings_from_pairs(&merged);
    let max = merged
        .iter()
        .filter_map(|(_, temp)| valid_sensor_temperature_celsius(*temp))
        .fold(None, |acc, temp| {
            Some(acc.map_or(temp, |current: f64| current.max(temp)))
        });
    let temps = if readings.is_empty() {
        None
    } else {
        Some(format_disk_temp_readings(&readings))
    };

    let has_nvme = !nvme_temps.is_empty();
    let has_smart = merged
        .iter()
        .any(|(name, _)| smart_temps.iter().any(|(smart_name, _)| smart_name == name));
    let source = match (has_nvme, has_smart) {
        (true, true) => Some("nvme hwmon + smartctl".to_string()),
        (true, false) => Some("nvme hwmon".to_string()),
        (false, true) => Some("smartctl".to_string()),
        (false, false) => None,
    };

    DiskTempSnapshot {
        temps,
        max,
        source,
        readings,
    }
}

fn normalize_disk_name(name: &str) -> String {
    name.trim_start_matches("/dev/").to_string()
}

fn is_nvme_disk_name(name: &str) -> bool {
    normalize_disk_name(name).starts_with("nvme")
}

fn disk_temp_readings_from_pairs(temps: &[(String, f64)]) -> Vec<DiskTempReading> {
    temps
        .iter()
        .filter_map(|(name, temp)| {
            valid_sensor_temperature_celsius(*temp).map(|temp| DiskTempReading {
                name: name.clone(),
                temp_celsius: temp,
            })
        })
        .collect()
}

fn format_disk_temp_readings(readings: &[DiskTempReading]) -> String {
    readings
        .iter()
        .map(|reading| format!("{}:{:.1}", reading.name, reading.temp_celsius))
        .collect::<Vec<_>>()
        .join(",")
}

fn should_write_csv_headers(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.len() == 0)
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;

    use crate::config::Config;

    use super::{merge_disk_temperatures, should_write_csv_headers, App};

    #[test]
    fn csv_headers_are_written_for_new_or_empty_files() {
        let path = std::env::temp_dir().join(format!(
            "slow-rs-empty-csv-{}-{}.csv",
            std::process::id(),
            "headers"
        ));
        let _ = std::fs::remove_file(&path);

        assert!(should_write_csv_headers(&path));

        File::create(&path).unwrap();
        assert!(should_write_csv_headers(&path));

        let mut file = File::create(&path).unwrap();
        writeln!(file, "timestamp,datetime").unwrap();
        assert!(!should_write_csv_headers(&path));

        let _ = std::fs::remove_file(&path);
    }

    fn temp_path(name: &str, extension: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "slow-rs-{}-{}.{}",
            name,
            std::process::id(),
            extension
        ))
    }

    fn test_config(csv_file: PathBuf, test_file: PathBuf, io_bench: bool) -> Config {
        Config {
            interval: 5,
            csv_file: csv_file.to_string_lossy().into_owned(),
            test_file: test_file.to_string_lossy().into_owned(),
            file_size_mb: 1,
            history_size: 8,
            headless: true,
            io_bench,
        }
    }

    #[test]
    fn ensure_test_file_skips_when_io_bench_is_disabled() {
        let csv_path = temp_path("skip-io-csv", "csv");
        let test_path = temp_path("skip-io-test", "bin");
        let _ = std::fs::remove_file(&csv_path);
        let _ = std::fs::remove_file(&test_path);

        let app = App::new(test_config(csv_path.clone(), test_path.clone(), false)).unwrap();

        app.ensure_test_file().unwrap();
        assert!(!test_path.exists());

        let _ = std::fs::remove_file(csv_path);
        let _ = std::fs::remove_file(test_path);
    }

    #[test]
    fn ensure_test_file_creates_missing_io_benchmark_file() {
        let csv_path = temp_path("create-io-csv", "csv");
        let test_path = temp_path("create-io-test", "bin");
        let _ = std::fs::remove_file(&csv_path);
        let _ = std::fs::remove_file(&test_path);

        let app = App::new(test_config(csv_path.clone(), test_path.clone(), true)).unwrap();

        app.ensure_test_file().unwrap();
        assert_eq!(std::fs::metadata(&test_path).unwrap().len(), 1024 * 1024);

        let _ = std::fs::remove_file(csv_path);
        let _ = std::fs::remove_file(test_path);
    }

    #[test]
    fn disk_temperatures_merge_nvme_hwmon_with_sata_smart() {
        let nvme_temps = vec![("nvme0".to_string(), 41.0)];
        let smart_temps = vec![
            ("/dev/nvme0n1".to_string(), 40.0),
            ("/dev/sda".to_string(), 35.0),
            ("/dev/sdb".to_string(), 36.0),
            ("/dev/sdc".to_string(), 1000.0),
        ];

        let snapshot = merge_disk_temperatures(&nvme_temps, &smart_temps);

        assert_eq!(snapshot.max, Some(41.0));
        assert_eq!(snapshot.source.as_deref(), Some("nvme hwmon + smartctl"));
        assert_eq!(
            snapshot.temps.as_deref(),
            Some("nvme0:41.0,/dev/sda:35.0,/dev/sdb:36.0")
        );
        assert_eq!(snapshot.readings.len(), 3);
        assert!(snapshot.readings.iter().any(|r| r.name == "nvme0"));
        assert!(snapshot.readings.iter().any(|r| r.name == "/dev/sda"));
        assert!(snapshot.readings.iter().any(|r| r.name == "/dev/sdb"));
        assert!(!snapshot.readings.iter().any(|r| r.name == "/dev/sdc"));
        assert!(!snapshot.readings.iter().any(|r| r.name == "/dev/nvme0n1"));
    }

    #[test]
    fn disk_temperatures_use_all_smart_devices_without_nvme_hwmon() {
        let smart_temps = vec![
            ("/dev/nvme0n1".to_string(), 40.0),
            ("/dev/sda".to_string(), 35.0),
            ("/dev/sdb".to_string(), 36.0),
        ];

        let snapshot = merge_disk_temperatures(&[], &smart_temps);

        assert_eq!(snapshot.max, Some(40.0));
        assert_eq!(snapshot.source.as_deref(), Some("smartctl"));
        assert_eq!(snapshot.readings.len(), 3);
    }
}
