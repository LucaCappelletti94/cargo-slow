//! SMART disk health monitoring for cargo-slow.
//!
//! This module provides SMART health data collection via smartctl.
//! Requires smartmontools to be installed and sudo access for full data.

use std::process::Command;

use crate::availability::MetricAvailability;
use crate::temperature::valid_sensor_temperature_celsius;

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
    /// Device path, e.g. "/dev/sda".
    pub name: String,
    /// Overall health test passed
    pub health_passed: bool,
    /// Current temperature in Celsius
    pub temperature: Option<f64>,
    /// Reallocated sector count (bad sectors)
    pub reallocated_sectors: Option<u64>,
    /// Current pending sector count
    pub pending_sectors: Option<u64>,
    /// Cumulative count of unsafe (ungraceful) shutdowns.
    ///
    /// This is the NVMe `unsafe_shutdowns` lifetime counter. It only ever
    /// increases and rising while the host stays up (no reboot or power loss)
    /// is a sign the drive controller is dropping off the bus on its own.
    pub unsafe_shutdowns: Option<u64>,
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
        let output = Self::run_smartctl(&["-a", "-j", device])?;
        let parsed = Self::parse_smartctl_output(&output.stdout, output.status.success(), device);

        if parsed
            .as_ref()
            .and_then(|device| device.temperature)
            .is_some()
        {
            return parsed;
        }

        if normalize_device_name(device).starts_with("sd") {
            if let Some(sat_output) = Self::run_smartctl(&["-a", "-j", "-d", "sat", device]) {
                let sat_parsed = Self::parse_smartctl_output(
                    &sat_output.stdout,
                    sat_output.status.success(),
                    device,
                );
                if sat_parsed
                    .as_ref()
                    .and_then(|device| device.temperature)
                    .is_some()
                {
                    return sat_parsed;
                }
                let parsed = sat_parsed.or(parsed);
                return Self::read_text_temperature(device, parsed);
            }

            return Self::read_text_temperature(device, parsed);
        }

        parsed
    }

    fn read_text_temperature(device: &str, parsed: Option<SmartDevice>) -> Option<SmartDevice> {
        if let Some(output) = Self::run_smartctl(&["-A", device]) {
            if let Some(device_with_temp) =
                Self::parse_smartctl_text_output(&output.stdout, device, parsed.clone())
            {
                return Some(device_with_temp);
            }
        }

        if let Some(output) = Self::run_smartctl(&["-A", "-d", "sat", device]) {
            if let Some(device_with_temp) =
                Self::parse_smartctl_text_output(&output.stdout, device, parsed.clone())
            {
                return Some(device_with_temp);
            }
        }

        parsed
    }

    fn run_smartctl(args: &[&str]) -> Option<std::process::Output> {
        if MetricAvailability::has_elevated_privileges() {
            Command::new("smartctl").args(args).output().ok()
        } else {
            let mut sudo_args = Vec::with_capacity(args.len() + 1);
            sudo_args.push("smartctl");
            sudo_args.extend_from_slice(args);
            Command::new("sudo").args(sudo_args).output().ok()
        }
    }

    fn parse_smartctl_output(
        stdout: &[u8],
        status_success: bool,
        device: &str,
    ) -> Option<SmartDevice> {
        let stdout = String::from_utf8_lossy(stdout);
        let trimmed = stdout.trim_start();
        if trimmed.is_empty() || (!status_success && !trimmed.starts_with('{')) {
            return None;
        }

        Self::parse_smartctl_json(&stdout, device)
    }

    fn parse_smartctl_text_output(
        stdout: &[u8],
        device: &str,
        parsed: Option<SmartDevice>,
    ) -> Option<SmartDevice> {
        let stdout = String::from_utf8_lossy(stdout);
        let temperature = Self::extract_text_temperature(&stdout)?;

        let mut device = parsed.unwrap_or_else(|| SmartDevice {
            name: device.to_string(),
            health_passed: true,
            temperature: None,
            reallocated_sectors: None,
            pending_sectors: None,
            unsafe_shutdowns: None,
        });
        device.temperature = Some(temperature);
        Some(device)
    }

    /// Parse smartctl JSON output.
    fn parse_smartctl_json(json: &str, device: &str) -> Option<SmartDevice> {
        let health_passed = json.contains("\"passed\": true")
            || json.contains("\"smart_status\": { \"passed\": true }");

        let temperature = Self::extract_temperature(json);

        // Look for reallocated sectors in attributes
        let reallocated_sectors = Self::extract_smart_attribute_raw(json, "Reallocated_Sector_Ct")
            .or_else(|| Self::extract_smart_attribute_raw(json, "Reallocated_Event_Count"));

        let pending_sectors = Self::extract_smart_attribute_raw(json, "Current_Pending_Sector");

        // NVMe drives expose this directly in the health log. SATA drives may
        // surface an equivalent power-loss attribute under different names.
        let unsafe_shutdowns = Self::extract_json_number(json, "unsafe_shutdowns")
            .map(|value| value as u64)
            .or_else(|| Self::extract_smart_attribute_raw(json, "Power-Off_Retract_Count"))
            .or_else(|| Self::extract_smart_attribute_raw(json, "Unexpect_Power_Loss_Ct"));

        Some(SmartDevice {
            name: device.to_string(),
            health_passed,
            temperature,
            reallocated_sectors,
            pending_sectors,
            unsafe_shutdowns,
        })
    }

    /// Extract a numeric value from JSON.
    fn extract_json_number(json: &str, key: &str) -> Option<f64> {
        let pattern = format!("\"{}\"", key);
        let start = json.find(&pattern)?;
        let rest = &json[start + pattern.len()..];
        let colon = rest.find(':')?;
        let value = rest[colon + 1..].trim_start();
        let end = value
            .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
            .unwrap_or(value.len());
        if end == 0 {
            return None;
        }
        value[..end].parse().ok()
    }

    fn extract_temperature(json: &str) -> Option<f64> {
        Self::extract_object_number(json, "temperature", "current")
            .or_else(|| Self::extract_json_number(json, "temperature"))
            .and_then(valid_sensor_temperature_celsius)
            .or_else(|| Self::extract_smart_attribute_temperature(json, "Temperature_Celsius"))
            .or_else(|| Self::extract_smart_attribute_temperature(json, "Airflow_Temperature_Cel"))
            .or_else(|| Self::extract_smart_attribute_temperature(json, "Temperature_Internal"))
    }

    fn extract_object_number(json: &str, object_key: &str, number_key: &str) -> Option<f64> {
        let pattern = format!("\"{}\"", object_key);
        let start = json.find(&pattern)?;
        let rest = &json[start + pattern.len()..];
        let colon = rest.find(':')?;
        let value = rest[colon + 1..].trim_start();
        if !value.starts_with('{') {
            return None;
        }
        let object = Self::balanced_json_object(value)?;
        Self::extract_json_number(object, number_key)
    }

    fn balanced_json_object(json: &str) -> Option<&str> {
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;

        for (idx, ch) in json.char_indices() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
                continue;
            }

            match ch {
                '"' => in_string = true,
                '{' => depth += 1,
                '}' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(&json[..=idx]);
                    }
                }
                _ => {}
            }
        }

        None
    }

    fn extract_smart_attribute_temperature(json: &str, attr_name: &str) -> Option<f64> {
        let section = Self::smart_attribute_section(json, attr_name)?;
        Self::extract_raw_string_temperature(section).or_else(|| {
            Self::extract_smart_attribute_raw(section, attr_name)
                .map(|raw| raw as f64)
                .and_then(valid_sensor_temperature_celsius)
        })
    }

    fn smart_attribute_section<'a>(json: &'a str, attr_name: &str) -> Option<&'a str> {
        let start = json.find(&format!("\"name\": \"{}\"", attr_name))?;
        let section = &json[start..];
        let next_attr = section
            .get(1..)
            .and_then(|rest| rest.find("\"name\": "))
            .map(|idx| idx + 1)
            .unwrap_or(section.len());
        Some(&section[..next_attr])
    }

    fn extract_raw_string_temperature(section: &str) -> Option<f64> {
        let raw_start = section.find("\"raw\"")?;
        let raw_section = &section[raw_start..];
        let string_start = raw_section.find("\"string\"")?;
        let rest = &raw_section[string_start + "\"string\"".len()..];
        let colon = rest.find(':')?;
        let value = rest[colon + 1..].trim_start();
        if !value.starts_with('"') {
            return None;
        }
        let value = &value[1..];
        let end_quote = value.find('"')?;
        Self::first_number(value[..end_quote].trim()).and_then(valid_sensor_temperature_celsius)
    }

    fn first_number(text: &str) -> Option<f64> {
        let start = text.find(|ch: char| ch.is_ascii_digit() || ch == '-' || ch == '+')?;
        let rest = &text[start..];
        let end = rest
            .find(|ch: char| !ch.is_ascii_digit() && ch != '.' && ch != '-' && ch != '+')
            .unwrap_or(rest.len());
        rest[..end].parse().ok()
    }

    fn extract_text_temperature(output: &str) -> Option<f64> {
        [
            "Temperature_Celsius",
            "Airflow_Temperature_Cel",
            "Temperature_Internal",
        ]
        .iter()
        .find_map(|attr| Self::extract_text_attribute_temperature(output, attr))
    }

    fn extract_text_attribute_temperature(output: &str, attr_name: &str) -> Option<f64> {
        output
            .lines()
            .find(|line| line.split_whitespace().nth(1) == Some(attr_name))
            .and_then(Self::extract_text_attribute_raw_value)
    }

    fn extract_text_attribute_raw_value(line: &str) -> Option<f64> {
        let mut parts = line.split_whitespace();
        while let Some(part) = parts.next() {
            if part == "-" {
                return parts
                    .next()
                    .and_then(Self::first_number)
                    .and_then(valid_sensor_temperature_celsius);
            }
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

    /// Get temperatures for all devices that reported them.
    pub fn device_temperatures(&self) -> Vec<(String, f64)> {
        self.devices
            .iter()
            .filter_map(|device| {
                device
                    .temperature
                    .map(|temperature| (device.name.clone(), temperature))
            })
            .collect()
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

    /// Get unsafe shutdown counts for every device that reported one.
    pub fn unsafe_shutdowns(&self) -> Vec<(String, u64)> {
        self.devices
            .iter()
            .filter_map(|device| {
                device
                    .unsafe_shutdowns
                    .map(|count| (device.name.clone(), count))
            })
            .collect()
    }
}

fn normalize_device_name(name: &str) -> String {
    name.trim_start_matches("/dev/").to_string()
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
                name: "/dev/sda".to_string(),
                health_passed: true,
                temperature: None,
                reallocated_sectors: None,
                pending_sectors: None,
                unsafe_shutdowns: None,
            }],
        };

        assert!(health.all_healthy());

        health.devices.push(SmartDevice {
            name: "/dev/sdb".to_string(),
            health_passed: false,
            temperature: None,
            reallocated_sectors: None,
            pending_sectors: None,
            unsafe_shutdowns: None,
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
        assert_eq!(device.name, "/dev/sda");
        assert_eq!(device.temperature, Some(42.0));
        assert_eq!(device.reallocated_sectors, Some(7));
        assert_eq!(device.pending_sectors, Some(2));
    }

    #[test]
    fn parses_smartctl_json_even_when_exit_status_is_nonzero() {
        let json = r#"
        {
          "smartctl": { "exit_status": 8 },
          "smart_status": { "passed": false },
          "temperature": { "current": 37 }
        }
        "#;

        let device =
            SmartHealth::parse_smartctl_output(json.as_bytes(), false, "/dev/sda").unwrap();

        assert_eq!(device.name, "/dev/sda");
        assert_eq!(device.temperature, Some(37.0));
    }

    #[test]
    fn parses_ata_temperature_attribute_when_top_level_temperature_is_missing() {
        let json = r#"
        {
          "smart_status": { "passed": true },
          "ata_smart_attributes": {
            "table": [
              { "name": "Temperature_Celsius", "raw": { "value": 34 } }
            ]
          }
        }
        "#;

        let device = SmartHealth::parse_smartctl_json(json, "/dev/sdb").unwrap();

        assert_eq!(device.temperature, Some(34.0));
    }

    #[test]
    fn parses_ata_temperature_string_instead_of_packed_raw_value() {
        let json = r#"
        {
          "smart_status": { "passed": true },
          "ata_smart_attributes": {
            "table": [
              {
                "name": "Temperature_Celsius",
                "raw": {
                  "value": 240519020585,
                  "string": "41 (Min/Max 20/54)"
                }
              }
            ]
          }
        }
        "#;

        let device = SmartHealth::parse_smartctl_json(json, "/dev/sda").unwrap();

        assert_eq!(device.temperature, Some(41.0));
    }

    #[test]
    fn ignores_implausible_packed_temperature_raw_value() {
        let json = r#"
        {
          "smart_status": { "passed": true },
          "ata_smart_attributes": {
            "table": [
              {
                "name": "Temperature_Celsius",
                "raw": { "value": 240519020585 }
              }
            ]
          }
        }
        "#;

        let device = SmartHealth::parse_smartctl_json(json, "/dev/sda").unwrap();

        assert_eq!(device.temperature, None);
    }

    #[test]
    fn parses_sata_smart_attribute_text_temperature() {
        let output = r#"
190 Airflow_Temperature_Cel 0x0022   071   047   000    Old_age   Always       -       29 (Min/Max 24/30)
194 Temperature_Celsius     0x0022   029   053   000    Old_age   Always       -       29 (0 17 0 0 0)
"#;

        let device =
            SmartHealth::parse_smartctl_text_output(output.as_bytes(), "/dev/sda", None).unwrap();

        assert_eq!(device.name, "/dev/sda");
        assert_eq!(device.temperature, Some(29.0));
    }

    #[test]
    fn text_temperature_fallback_preserves_json_health_fields() {
        let output = r#"
194 Temperature_Celsius     0x0022   029   053   000    Old_age   Always       -       29 (0 17 0 0 0)
"#;
        let parsed = SmartDevice {
            name: "/dev/sda".to_string(),
            health_passed: false,
            temperature: None,
            reallocated_sectors: Some(3),
            pending_sectors: Some(1),
            unsafe_shutdowns: None,
        };

        let device =
            SmartHealth::parse_smartctl_text_output(output.as_bytes(), "/dev/sda", Some(parsed))
                .unwrap();

        assert!(!device.health_passed);
        assert_eq!(device.temperature, Some(29.0));
        assert_eq!(device.reallocated_sectors, Some(3));
        assert_eq!(device.pending_sectors, Some(1));
    }

    #[test]
    fn smart_helpers_aggregate_devices() {
        let health = SmartHealth {
            available: true,
            devices: vec![
                SmartDevice {
                    name: "/dev/sda".to_string(),
                    health_passed: true,
                    temperature: Some(41.0),
                    reallocated_sectors: Some(1),
                    pending_sectors: Some(0),
                    unsafe_shutdowns: Some(44),
                },
                SmartDevice {
                    name: "/dev/sdb".to_string(),
                    health_passed: true,
                    temperature: Some(47.0),
                    reallocated_sectors: Some(4),
                    pending_sectors: Some(3),
                    unsafe_shutdowns: None,
                },
            ],
        };

        assert_eq!(
            health.device_temperatures(),
            vec![
                ("/dev/sda".to_string(), 41.0),
                ("/dev/sdb".to_string(), 47.0)
            ]
        );
        assert_eq!(health.total_reallocated_sectors(), 5);
        assert_eq!(health.total_pending_sectors(), 3);
        assert_eq!(
            health.unsafe_shutdowns(),
            vec![("/dev/sda".to_string(), 44)]
        );
    }

    #[test]
    fn parses_nvme_unsafe_shutdowns_from_health_log() {
        let json = r#"
        {
          "smart_status": { "passed": true },
          "temperature": { "current": 38 },
          "nvme_smart_health_information_log": {
            "critical_warning": 0,
            "temperature": 38,
            "unsafe_shutdowns": 44,
            "media_errors": 0
          }
        }
        "#;

        let device = SmartHealth::parse_smartctl_json(json, "/dev/nvme0").unwrap();

        assert_eq!(device.unsafe_shutdowns, Some(44));
    }

    #[test]
    fn smart_attribute_parser_uses_reallocated_event_fallback() {
        let json = r#"
        {
          "passed": false,
          "temperature": { "current": 38 },
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
