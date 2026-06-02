//! Metric availability tracking for cargo-slow.
//!
//! This module tracks which metric sources are available on the system,
//! allowing the UI to show warnings when metrics are missing due to
//! permissions or kernel configuration.

use std::process::Command;

/// Tracks which metric sources are available.
#[derive(Default, Clone, Debug)]
pub struct MetricAvailability {
    /// /proc/pressure/* is available (requires Linux 4.20+ with CONFIG_PSI)
    pub proc_pressure: bool,
    /// jc42 DIMM temperature sensors found
    pub sys_hwmon_dimm: bool,
    /// NVMe temperature sensors found
    pub sys_hwmon_nvme: bool,
    /// smartctl is available
    pub smartctl: bool,
    /// ipmitool is available (for BMC sensors)
    pub ipmitool: bool,
    /// We can run commands that require elevated privileges.
    pub privileged_commands: bool,
}

impl MetricAvailability {
    /// Probe all metric sources and return availability status.
    pub fn probe() -> Self {
        let smartctl = Self::check_command_available("smartctl");
        let ipmitool = Self::check_command_available("ipmitool");
        let needs_privileges = smartctl || ipmitool;
        let privileged_commands =
            Self::has_elevated_privileges() || (needs_privileges && Self::has_sudo_access());

        Self {
            proc_pressure: std::fs::read_to_string("/proc/pressure/cpu").is_ok(),
            sys_hwmon_dimm: Self::check_dimm_sensors(),
            sys_hwmon_nvme: Self::check_nvme_sensors(),
            smartctl,
            ipmitool,
            privileged_commands,
        }
    }

    /// Check for jc42 DIMM temperature sensors.
    fn check_dimm_sensors() -> bool {
        if let Ok(entries) = std::fs::read_dir("/sys/class/hwmon") {
            for entry in entries.flatten() {
                let name_path = entry.path().join("name");
                if let Ok(name) = std::fs::read_to_string(&name_path) {
                    if name.trim() == "jc42" {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check for NVMe temperature sensors.
    fn check_nvme_sensors() -> bool {
        if let Ok(entries) = std::fs::read_dir("/sys/class/hwmon") {
            for entry in entries.flatten() {
                let name_path = entry.path().join("name");
                if let Ok(name) = std::fs::read_to_string(&name_path) {
                    if name.trim() == "nvme" {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if a command is available in PATH.
    fn check_command_available(cmd: &str) -> bool {
        Command::new("which")
            .arg(cmd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Generate warnings for unavailable metrics.
    pub fn get_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if !self.proc_pressure {
            warnings.push("PSI unavailable (requires Linux 4.20+ with CONFIG_PSI)".into());
        }
        if !self.sys_hwmon_dimm {
            warnings.push("RAM temp sensors not found (no jc42 hwmon devices)".into());
        }
        if !self.sys_hwmon_nvme {
            warnings.push("NVMe temp sensors not found".into());
        }
        if !self.smartctl {
            warnings.push("smartctl not found (install smartmontools for disk health)".into());
        } else if !self.privileged_commands {
            warnings.push("SMART health unavailable (run with sudo)".into());
        }
        if !self.ipmitool && self.privileged_commands {
            warnings.push("ipmitool not found (install for BMC/IPMI sensors)".into());
        } else if self.ipmitool && !self.privileged_commands {
            warnings.push("IPMI/BMC sensors unavailable (run with sudo)".into());
        }

        warnings
    }

    /// Check if running with elevated privileges.
    pub fn has_elevated_privileges() -> bool {
        unsafe { libc::geteuid() == 0 }
    }

    /// Check if we have passwordless sudo access.
    pub fn has_sudo_access() -> bool {
        Command::new("sudo")
            .args(["-n", "true"])
            .output()
            .map(|s| s.status.success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::MetricAvailability;

    #[test]
    fn warnings_include_privilege_gaps_for_installed_tools() {
        let availability = MetricAvailability {
            proc_pressure: true,
            sys_hwmon_dimm: true,
            sys_hwmon_nvme: true,
            smartctl: true,
            ipmitool: true,
            privileged_commands: false,
        };

        let warnings = availability.get_warnings();

        assert!(warnings
            .iter()
            .any(|w| w.contains("SMART health unavailable")));
        assert!(warnings.iter().any(|w| w.contains("IPMI/BMC sensors")));
    }
}
