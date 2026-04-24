//! SMART disk health monitoring for slow-rs.
//!
//! This module provides SMART health data collection via smartctl.
//! Requires smartmontools to be installed and sudo access for full data.

use std::process::Command;

use crate::availability::MetricAvailability;

/// SMART health information for all disks.
#[derive(Clone, Debug, Default)]
pub struct SmartHealth {
    /// Whether SMART data is available
    pub available: bool,
    /// Individual disk health data
    pub devices: Vec<SmartDevice>,
}

/// SMART health data for a single disk.
#[derive(Clone, Debug)]
pub struct SmartDevice {
    /// Overall health test passed
    pub health_passed: bool,
    /// Current temperature in Celsius
    pub temperature: Option<f64>,
    /// Reallocated sector count (bad sectors)
    pub reallocated_sectors: Option<u64>,
    /// Current pending sector count
    pub pending_sectors: Option<u64>,
}

impl SmartHealth {
    /// Collect SMART health data from all disks.
    ///
    /// This requires sudo access to read SMART data. If not available,
    /// returns SmartHealth with available=false.
    pub fn collect() -> Self {
        // Check if we can run smartctl
        if !MetricAvailability::has_elevated_privileges() && !MetricAvailability::has_sudo_access()
        {
            return Self {
                available: false,
                devices: vec![],
            };
        }

        let mut health = Self {
            available: true,
            devices: vec![],
        };

        // List block devices
        if let Ok(output) = Command::new("lsblk")
            .args(["-d", "-n", "-o", "NAME,TYPE"])
            .output()
        {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 && parts[1] == "disk" {
                    let device = format!("/dev/{}", parts[0]);
                    if let Some(smart) = Self::read_device(&device) {
                        health.devices.push(smart);
                    }
                }
            }
        }

        health.available = !health.devices.is_empty();
        health
    }

    /// Read SMART data from a single device.
    fn read_device(device: &str) -> Option<SmartDevice> {
        // Run smartctl with sudo if not root
        let output = if MetricAvailability::has_elevated_privileges() {
            Command::new("smartctl")
                .args(["-a", "-j", device])
                .output()
                .ok()?
        } else {
            Command::new("sudo")
                .args(["smartctl", "-a", "-j", device])
                .output()
                .ok()?
        };

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Self::parse_smartctl_json(&stdout, device)
    }

    /// Parse smartctl JSON output.
    fn parse_smartctl_json(json: &str, _device: &str) -> Option<SmartDevice> {
        let health_passed = json.contains("\"passed\": true")
            || json.contains("\"smart_status\": { \"passed\": true }");

        let temperature = Self::extract_json_number(json, "temperature")
            .or_else(|| Self::extract_json_number(json, "current"));

        // Look for reallocated sectors in attributes
        let reallocated_sectors = Self::extract_smart_attribute_raw(json, "Reallocated_Sector_Ct")
            .or_else(|| Self::extract_smart_attribute_raw(json, "Reallocated_Event_Count"));

        let pending_sectors = Self::extract_smart_attribute_raw(json, "Current_Pending_Sector");

        Some(SmartDevice {
            health_passed,
            temperature,
            reallocated_sectors,
            pending_sectors,
        })
    }

    /// Extract a numeric value from JSON.
    fn extract_json_number(json: &str, key: &str) -> Option<f64> {
        let pattern = format!("\"{}\": ", key);
        if let Some(start) = json.find(&pattern) {
            let rest = &json[start + pattern.len()..];
            let end = rest
                .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
                .unwrap_or(rest.len());
            return rest[..end].parse().ok();
        }
        None
    }

    /// Extract raw value from a SMART attribute.
    fn extract_smart_attribute_raw(json: &str, attr_name: &str) -> Option<u64> {
        // Look for attribute in ata_smart_attributes section
        if let Some(attr_start) = json.find(&format!("\"name\": \"{}\"", attr_name)) {
            let section = &json[attr_start..];
            // Find raw value
            if let Some(raw_start) = section.find("\"raw\": {") {
                let raw_section = &section[raw_start..];
                if let Some(value_start) = raw_section.find("\"value\": ") {
                    let rest = &raw_section[value_start + 9..];
                    let end = rest
                        .find(|c: char| !c.is_ascii_digit())
                        .unwrap_or(rest.len());
                    return rest[..end].parse().ok();
                }
            }
        }
        None
    }

    /// Get the maximum temperature across all devices.
    pub fn max_temperature(&self) -> Option<f64> {
        self.devices
            .iter()
            .filter_map(|d| d.temperature)
            .fold(None, |acc, t| Some(acc.map_or(t, |a: f64| a.max(t))))
    }

    /// Check if all devices passed health check.
    pub fn all_healthy(&self) -> bool {
        !self.devices.is_empty() && self.devices.iter().all(|d| d.health_passed)
    }

    /// Get total reallocated sectors across all devices.
    pub fn total_reallocated_sectors(&self) -> u64 {
        self.devices
            .iter()
            .filter_map(|d| d.reallocated_sectors)
            .sum()
    }

    /// Get total pending sectors across all devices.
    pub fn total_pending_sectors(&self) -> u64 {
        self.devices.iter().filter_map(|d| d.pending_sectors).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::{SmartDevice, SmartHealth};

    #[test]
    fn empty_smart_results_are_not_healthy() {
        let health = SmartHealth::default();

        assert!(!health.all_healthy());
    }

    #[test]
    fn all_healthy_requires_every_device_to_pass() {
        let mut health = SmartHealth {
            available: true,
            devices: vec![SmartDevice {
                health_passed: true,
                temperature: None,
                reallocated_sectors: None,
                pending_sectors: None,
            }],
        };

        assert!(health.all_healthy());

        health.devices.push(SmartDevice {
            health_passed: false,
            temperature: None,
            reallocated_sectors: None,
            pending_sectors: None,
        });

        assert!(!health.all_healthy());
    }

    #[test]
    fn parses_smartctl_json_health_temperature_and_attributes() {
        let json = r#"
        {
          "smart_status": { "passed": true },
          "temperature": { "current": 42 },
          "ata_smart_attributes": {
            "table": [
              { "name": "Reallocated_Sector_Ct", "raw": { "value": 7 } },
              { "name": "Current_Pending_Sector", "raw": { "value": 2 } }
            ]
          }
        }
        "#;

        let device = SmartHealth::parse_smartctl_json(json, "/dev/sda").unwrap();

        assert!(device.health_passed);
        assert_eq!(device.temperature, Some(42.0));
        assert_eq!(device.reallocated_sectors, Some(7));
        assert_eq!(device.pending_sectors, Some(2));
    }

    #[test]
    fn smart_helpers_aggregate_devices() {
        let health = SmartHealth {
            available: true,
            devices: vec![
                SmartDevice {
                    health_passed: true,
                    temperature: Some(41.0),
                    reallocated_sectors: Some(1),
                    pending_sectors: Some(0),
                },
                SmartDevice {
                    health_passed: true,
                    temperature: Some(47.0),
                    reallocated_sectors: Some(4),
                    pending_sectors: Some(3),
                },
            ],
        };

        assert_eq!(health.max_temperature(), Some(47.0));
        assert_eq!(health.total_reallocated_sectors(), 5);
        assert_eq!(health.total_pending_sectors(), 3);
    }

    #[test]
    fn smart_attribute_parser_uses_reallocated_event_fallback() {
        let json = r#"
        {
          "passed": false,
          "current": 38,
          "ata_smart_attributes": {
            "table": [
              { "name": "Reallocated_Event_Count", "raw": { "value": 9 } }
            ]
          }
        }
        "#;

        let device = SmartHealth::parse_smartctl_json(json, "/dev/nvme0").unwrap();

        assert!(!device.health_passed);
        assert_eq!(device.temperature, Some(38.0));
        assert_eq!(device.reallocated_sectors, Some(9));
        assert_eq!(device.pending_sectors, None);
    }
}
