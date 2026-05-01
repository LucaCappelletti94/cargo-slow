//! Temperature sanity checks shared by sensor collectors and parsers.

/// Lowest Celsius value accepted from a hardware sensor.
pub const MIN_SENSOR_TEMP_C: f64 = -50.0;
/// Highest Celsius value accepted from a hardware sensor.
///
/// This deliberately sits well below obviously broken readings like 1000C,
/// while leaving room for real overheating events to still be reported.
pub const MAX_SENSOR_TEMP_C: f64 = 150.0;

/// Return the temperature only if it is finite and physically plausible.
pub fn valid_sensor_temperature_celsius(value: f64) -> Option<f64> {
    (value.is_finite() && (MIN_SENSOR_TEMP_C..=MAX_SENSOR_TEMP_C).contains(&value)).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::valid_sensor_temperature_celsius;

    #[test]
    fn rejects_implausible_sensor_temperatures() {
        assert_eq!(valid_sensor_temperature_celsius(42.0), Some(42.0));
        assert_eq!(valid_sensor_temperature_celsius(150.0), Some(150.0));
        assert_eq!(valid_sensor_temperature_celsius(151.0), None);
        assert_eq!(valid_sensor_temperature_celsius(1000.0), None);
        assert_eq!(valid_sensor_temperature_celsius(f64::NAN), None);
    }
}
