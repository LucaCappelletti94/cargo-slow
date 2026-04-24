//! IPMI sensor reading for slow-rs.
//!
//! This module provides IPMI sensor data collection via ipmitool.
//! Requires ipmitool to be installed and sudo access.

use std::process::Command;

use crate::availability::MetricAvailability;
use crate::metrics::IpmiDimmTemp;

/// IPMI sensor information.
#[derive(Clone, Debug, Default)]
pub struct IpmiSensors {
    /// Whether IPMI data is available
    pub available: bool,
    /// Individual sensor readings
    pub sensors: Vec<IpmiSensor>,
}

/// A single IPMI sensor reading.
#[derive(Clone, Debug)]
pub struct IpmiSensor {
    /// Sensor name
    pub name: String,
    /// Current value
    pub value: f64,
    /// Unit of measurement
    pub unit: String,
    /// Status (ok, nc, cr, nr, na)
    pub status: SensorStatus,
}

/// IPMI sensor status levels.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum SensorStatus {
    /// Normal operation
    #[default]
    Ok,
    /// Non-Critical (warning)
    NonCritical,
    /// Critical
    Critical,
    /// Non-Recoverable (system may shut down)
    NonRecoverable,
    /// Not available / disabled
    NotAvailable,
}

impl IpmiSensors {
    /// Collect IPMI sensor data.
    ///
    /// This requires sudo access. If not available,
    /// returns IpmiSensors with available=false.
    pub fn collect() -> Self {
        // Check if we can run ipmitool
        if !MetricAvailability::has_elevated_privileges() && !MetricAvailability::has_sudo_access()
        {
            return Self::default();
        }

        // Run ipmitool
        let output = if MetricAvailability::has_elevated_privileges() {
            Command::new("ipmitool").args(["sensor", "list"]).output()
        } else {
            Command::new("sudo")
                .args(["ipmitool", "sensor", "list"])
                .output()
        };

        match output {
            Ok(out) if out.status.success() => {
                let sensors = Self::parse_sensor_list(&String::from_utf8_lossy(&out.stdout));
                Self {
                    available: true,
                    sensors,
                }
            }
            _ => Self::default(),
        }
    }

    /// Parse ipmitool sensor list output.
    fn parse_sensor_list(output: &str) -> Vec<IpmiSensor> {
        // Format: "Name | Value | Unit | Status | ..."
        output
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
                if parts.len() >= 4 {
                    let value = parts[1].parse().ok()?;
                    let status = Self::parse_status(parts[3]);
                    Some(IpmiSensor {
                        name: parts[0].to_string(),
                        value,
                        unit: parts[2].to_string(),
                        status,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// Parse IPMI status string.
    fn parse_status(s: &str) -> SensorStatus {
        match s.to_lowercase().as_str() {
            "ok" => SensorStatus::Ok,
            "nc" => SensorStatus::NonCritical,
            "cr" => SensorStatus::Critical,
            "nr" => SensorStatus::NonRecoverable,
            _ => SensorStatus::NotAvailable,
        }
    }

    /// Get all DIMM/memory temperature sensors.
    ///
    /// Matches various vendor naming conventions:
    /// - "DIMMA1", "P1-DIMMC1" (Supermicro)
    /// - "MEM Temp", "Memory Temp" (Dell, HP)
    /// - "DRAM Temp" (some vendors)
    pub fn dimm_sensors(&self) -> Vec<&IpmiSensor> {
        self.sensors
            .iter()
            .filter(|s| {
                let name_lower = s.name.to_lowercase();
                let is_memory_sensor = name_lower.contains("dimm")
                    || name_lower.contains("mem")
                    || name_lower.contains("dram");
                let is_temperature =
                    s.unit.contains("degrees") || s.unit.to_lowercase().contains("c");
                is_memory_sensor && is_temperature
            })
            .collect()
    }

    /// Get the worst DIMM status.
    pub fn worst_dimm_status(&self) -> SensorStatus {
        self.dimm_sensors()
            .iter()
            .map(|s| &s.status)
            .max_by_key(|s| match s {
                SensorStatus::NonRecoverable => 4,
                SensorStatus::Critical => 3,
                SensorStatus::NonCritical => 2,
                SensorStatus::Ok => 1,
                SensorStatus::NotAvailable => 0,
            })
            .cloned()
            .unwrap_or(SensorStatus::NotAvailable)
    }

    /// Get maximum DIMM temperature.
    pub fn max_dimm_temp(&self) -> Option<f64> {
        self.dimm_sensors()
            .iter()
            .map(|s| s.value)
            .fold(None, |acc, t| Some(acc.map_or(t, |a: f64| a.max(t))))
    }

    /// Get formatted string of all DIMM temps from IPMI.
    pub fn format_all_dimms(&self) -> Option<String> {
        let dimms = self.dimm_sensors();
        if dimms.is_empty() {
            return None;
        }

        let details: Vec<String> = dimms
            .iter()
            .filter(|s| s.status != SensorStatus::NotAvailable)
            .map(|s| {
                let status_str = match s.status {
                    SensorStatus::NonRecoverable => "NR!",
                    SensorStatus::Critical => "CR!",
                    SensorStatus::NonCritical => "NC",
                    SensorStatus::Ok => "ok",
                    SensorStatus::NotAvailable => "na",
                };
                format!("{}:{:.0}°C[{}]", s.name.trim(), s.value, status_str)
            })
            .collect();

        if details.is_empty() {
            None
        } else {
            Some(details.join(", "))
        }
    }

    /// Get individual DIMM temperatures for plotting.
    pub fn get_dimm_temps(&self) -> Vec<IpmiDimmTemp> {
        self.dimm_sensors()
            .iter()
            .filter(|s| s.status != SensorStatus::NotAvailable)
            .map(|s| IpmiDimmTemp {
                name: s.name.trim().to_string(),
                temp_celsius: s.value,
                status: Self::status_to_string(&s.status),
            })
            .collect()
    }

    /// Convert status to string representation.
    fn status_to_string(status: &SensorStatus) -> String {
        match status {
            SensorStatus::Ok => "ok".to_string(),
            SensorStatus::NonCritical => "nc".to_string(),
            SensorStatus::Critical => "cr".to_string(),
            SensorStatus::NonRecoverable => "nr".to_string(),
            SensorStatus::NotAvailable => "na".to_string(),
        }
    }
}
