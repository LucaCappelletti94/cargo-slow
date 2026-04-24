//! Actionable recommendations for slow-rs.
//!
//! This module analyzes metrics and generates actionable advice
//! when issues are detected.

use crate::metrics::Metrics;
use crate::thresholds::{Severity, Thresholds};

/// A recommendation with severity and actionable advice.
#[derive(Clone, Debug)]
pub struct Recommendation {
    /// Severity level of the issue
    pub severity: Severity,
    /// Short title for the issue
    pub title: String,
    /// Actionable advice for resolving the issue
    pub advice: String,
}

/// Generate recommendations based on current metrics.
pub fn generate_recommendations(metrics: &Metrics, thresholds: &Thresholds) -> Vec<Recommendation> {
    let mut recs = Vec::new();

    // I/O pressure
    if let Some(io) = metrics.io_pressure_some_avg10 {
        let severity = thresholds.io_pressure_severity(io);
        if severity == Severity::Critical {
            recs.push(Recommendation {
                severity,
                title: "High I/O Pressure".into(),
                advice: "Check: iotop, iostat -x 1, dmesg for disk errors".into(),
            });
        } else if severity == Severity::Warning {
            recs.push(Recommendation {
                severity,
                title: "Elevated I/O Pressure".into(),
                advice: "Monitor: iotop -o to identify I/O-heavy processes".into(),
            });
        }
    }

    // Memory pressure
    if let Some(mem) = metrics.mem_pressure_some_avg10 {
        let severity = thresholds.mem_pressure_severity(mem);
        if severity == Severity::Critical {
            recs.push(Recommendation {
                severity,
                title: "High Memory Pressure".into(),
                advice: "Check: ps aux --sort=-%mem | head, consider adding RAM".into(),
            });
        } else if severity == Severity::Warning {
            recs.push(Recommendation {
                severity,
                title: "Memory Pressure Detected".into(),
                advice: "Monitor: free -h, check for memory-hungry processes".into(),
            });
        }
    }

    // Swap activity
    if metrics.pswpin > 0 || metrics.pswpout > 0 {
        recs.push(Recommendation {
            severity: Severity::Warning,
            title: "Swap Activity".into(),
            advice: format!(
                "Swapping in:{} out:{}. Check: ps aux --sort=-%mem",
                metrics.pswpin, metrics.pswpout
            ),
        });
    }

    // Low available memory
    let mem_severity = thresholds.memory_available_severity(metrics.mem_available_mb);
    if mem_severity == Severity::Critical {
        recs.push(Recommendation {
            severity: Severity::Critical,
            title: "Critically Low Memory".into(),
            advice: format!(
                "Only {} MB available. Kill processes or add RAM immediately",
                metrics.mem_available_mb
            ),
        });
    } else if mem_severity == Severity::Warning {
        recs.push(Recommendation {
            severity: Severity::Warning,
            title: "Low Available Memory".into(),
            advice: format!(
                "{} MB available. Monitor memory usage closely",
                metrics.mem_available_mb
            ),
        });
    }

    // CPU temperature
    if let Some(temp) = metrics.cpu_temp_celsius {
        let severity = thresholds.cpu_temp_severity(temp);
        if severity == Severity::Critical {
            recs.push(Recommendation {
                severity,
                title: "CPU Overheating".into(),
                advice: format!(
                    "CPU at {:.0}C. Check cooling, clean dust, verify thermal paste",
                    temp
                ),
            });
        } else if severity == Severity::Warning {
            recs.push(Recommendation {
                severity,
                title: "CPU Running Hot".into(),
                advice: format!("CPU at {:.0}C. Consider improving cooling", temp),
            });
        }
    }

    // DIMM temperature
    if let Some(temp) = metrics.dimm_temp_max {
        let severity = thresholds.dimm_temp_severity(temp);
        if severity == Severity::Critical {
            recs.push(Recommendation {
                severity,
                title: "RAM Overheating".into(),
                advice: format!(
                    "DIMM at {:.0}C. Check case airflow, consider RAM cooling",
                    temp
                ),
            });
        } else if severity == Severity::Warning {
            recs.push(Recommendation {
                severity,
                title: "RAM Running Warm".into(),
                advice: format!("DIMM at {:.0}C. Ensure adequate airflow", temp),
            });
        }
    }

    // Disk temperature
    if let Some(temp) = metrics.disk_temp_max {
        let severity = thresholds.disk_temp_severity(temp);
        if severity == Severity::Critical {
            recs.push(Recommendation {
                severity,
                title: "Disk Overheating".into(),
                advice: format!("Disk at {:.0}C. Check cooling, may cause data loss", temp),
            });
        } else if severity == Severity::Warning {
            recs.push(Recommendation {
                severity,
                title: "Disk Running Hot".into(),
                advice: format!("Disk at {:.0}C. Consider better cooling", temp),
            });
        }
    }

    // High iowait (disk bottleneck indicator)
    let total_cpu = metrics.cpu_user + metrics.cpu_system + metrics.cpu_idle + metrics.cpu_iowait;
    if total_cpu > 0 {
        let iowait_pct = (metrics.cpu_iowait as f64 / total_cpu as f64) * 100.0;
        let severity = thresholds.iowait_severity(iowait_pct);
        if severity == Severity::Critical {
            recs.push(Recommendation {
                severity,
                title: "Severe I/O Wait".into(),
                advice: format!(
                    "{:.0}% CPU waiting for I/O. Disk is severe bottleneck",
                    iowait_pct
                ),
            });
        } else if severity == Severity::Warning {
            recs.push(Recommendation {
                severity,
                title: "High I/O Wait".into(),
                advice: format!(
                    "{:.0}% CPU waiting for I/O. Disk may be bottleneck",
                    iowait_pct
                ),
            });
        }
    }

    // High CPU usage
    let cpu_severity = thresholds.cpu_usage_severity(metrics.cpu_usage_percent);
    if cpu_severity == Severity::Critical {
        recs.push(Recommendation {
            severity: Severity::Critical,
            title: "CPU Saturated".into(),
            advice: format!(
                "CPU at {:.0}%. Check: top, htop for CPU-intensive processes",
                metrics.cpu_usage_percent
            ),
        });
    }

    // Major page faults (thrashing indicator)
    if metrics.pgmajfault > 100 {
        recs.push(Recommendation {
            severity: Severity::Warning,
            title: "High Major Faults".into(),
            advice: format!(
                "{} major faults. System may be thrashing. Add RAM or reduce load",
                metrics.pgmajfault
            ),
        });
    }

    // High dirty pages (write backlog)
    if metrics.dirty_mb > 1024 {
        recs.push(Recommendation {
            severity: Severity::Warning,
            title: "High Dirty Pages".into(),
            advice: format!(
                "{} MB waiting to be written. I/O may be backed up",
                metrics.dirty_mb
            ),
        });
    }

    // IPMI DIMM status (from BMC sensors)
    if let Some(ref status) = metrics.ipmi_dimm_status {
        let details = metrics
            .ipmi_dimm_details
            .as_deref()
            .unwrap_or("Check: sudo ipmitool sensor list | grep -i dimm");
        match status.as_str() {
            "nr" => {
                recs.push(Recommendation {
                    severity: Severity::Critical,
                    title: "DIMM NON-RECOVERABLE".into(),
                    advice: format!("{}. Check BMC logs: sudo ipmitool sel list", details),
                });
            }
            "cr" => {
                recs.push(Recommendation {
                    severity: Severity::Critical,
                    title: "DIMM CRITICAL".into(),
                    advice: format!("{}. Check cooling immediately", details),
                });
            }
            "nc" => {
                recs.push(Recommendation {
                    severity: Severity::Warning,
                    title: "DIMM Warning".into(),
                    advice: format!("{}. Monitor closely", details),
                });
            }
            _ => {}
        }
    }

    // Sort by severity (critical first)
    recs.sort_by_key(|r| match r.severity {
        Severity::Critical => 0,
        Severity::Warning => 1,
        Severity::Normal => 2,
    });

    recs
}

#[cfg(test)]
mod tests {
    use super::generate_recommendations;
    use crate::metrics::Metrics;
    use crate::thresholds::{Severity, Thresholds};

    fn baseline_metrics() -> Metrics {
        Metrics {
            mem_available_mb: 8192,
            cpu_idle: 100,
            ..Metrics::default()
        }
    }

    fn titles(metrics: &Metrics) -> Vec<String> {
        generate_recommendations(metrics, &Thresholds::default())
            .into_iter()
            .map(|r| r.title)
            .collect()
    }

    #[test]
    fn healthy_metrics_have_no_recommendations() {
        assert!(generate_recommendations(&baseline_metrics(), &Thresholds::default()).is_empty());
    }

    #[test]
    fn pressure_memory_and_cpu_recommendations_are_generated_and_sorted() {
        let mut metrics = baseline_metrics();
        metrics.io_pressure_some_avg10 = Some(30.0);
        metrics.mem_pressure_some_avg10 = Some(12.0);
        metrics.pswpout = 3;
        metrics.mem_available_mb = 128;
        metrics.cpu_usage_percent = 99.0;

        let recommendations = generate_recommendations(&metrics, &Thresholds::default());

        assert!(recommendations
            .iter()
            .any(|r| r.title == "High I/O Pressure"));
        assert!(recommendations
            .iter()
            .any(|r| r.title == "Memory Pressure Detected"));
        assert!(recommendations.iter().any(|r| r.title == "Swap Activity"));
        assert!(recommendations
            .iter()
            .any(|r| r.title == "Critically Low Memory"));
        assert!(recommendations.iter().any(|r| r.title == "CPU Saturated"));
        assert_eq!(
            recommendations.first().unwrap().severity,
            Severity::Critical
        );
    }

    #[test]
    fn thermal_and_storage_recommendations_cover_warning_paths() {
        let mut metrics = baseline_metrics();
        metrics.cpu_temp_celsius = Some(76.0);
        metrics.dimm_temp_max = Some(71.0);
        metrics.disk_temp_max = Some(51.0);
        metrics.cpu_user = 10;
        metrics.cpu_system = 10;
        metrics.cpu_idle = 60;
        metrics.cpu_iowait = 20;
        metrics.pgmajfault = 101;
        metrics.dirty_mb = 2048;

        let titles = titles(&metrics);

        assert!(titles.contains(&"CPU Running Hot".to_string()));
        assert!(titles.contains(&"RAM Running Warm".to_string()));
        assert!(titles.contains(&"Disk Running Hot".to_string()));
        assert!(titles.contains(&"High I/O Wait".to_string()));
        assert!(titles.contains(&"High Major Faults".to_string()));
        assert!(titles.contains(&"High Dirty Pages".to_string()));
    }

    #[test]
    fn ipmi_dimm_status_recommendations_use_status_severity() {
        let mut metrics = baseline_metrics();
        metrics.ipmi_dimm_status = Some("nr".to_string());
        metrics.ipmi_dimm_details = Some("DIMMA1:99C[NR!]".to_string());
        let recommendations = generate_recommendations(&metrics, &Thresholds::default());
        assert!(recommendations
            .iter()
            .any(|r| r.title == "DIMM NON-RECOVERABLE" && r.severity == Severity::Critical));

        metrics.ipmi_dimm_status = Some("nc".to_string());
        metrics.ipmi_dimm_details = None;
        let recommendations = generate_recommendations(&metrics, &Thresholds::default());
        assert!(recommendations
            .iter()
            .any(|r| r.title == "DIMM Warning" && r.severity == Severity::Warning));
    }
}
