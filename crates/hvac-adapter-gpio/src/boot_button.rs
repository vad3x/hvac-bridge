//! HVAC-compatible re-exports for the shared BOOT button reset adapter.

#[cfg(not(target_arch = "riscv32"))]
pub use edf_gpio::boot_button::MockButtonPin;
pub use edf_gpio::boot_button::{GpioBootButtonConfig, GpioBootButtonDriver};

/// Apply the reset timing values selected by the HVAC product configuration.
///
/// The firmware composition root calls this explicitly so the shared driver
/// retains generic defaults and no HVAC policy lives in `edf-gpio`.
pub fn apply_hvac_reset_timings(config: GpioBootButtonConfig) -> GpioBootButtonConfig {
    config
        .with_hold_durations(
            crate::config::ECOSYSTEM_RESET_HOLD_MS,
            crate::config::FACTORY_RESET_HOLD_MS,
        )
        .with_input_timings(
            crate::config::BUTTON_DEBOUNCE_MS,
            crate::config::BUTTON_POLL_INTERVAL_MS,
        )
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_all_hvac_reset_timings_to_shared_config() {
        let reset = apply_hvac_reset_timings(GpioBootButtonConfig::new());

        assert_eq!(
            [
                reset.ecosystem_hold_ms,
                reset.factory_hold_ms,
                reset.debounce_ms,
                reset.poll_interval_ms,
            ],
            [
                crate::config::ECOSYSTEM_RESET_HOLD_MS,
                crate::config::FACTORY_RESET_HOLD_MS,
                crate::config::BUTTON_DEBOUNCE_MS,
                crate::config::BUTTON_POLL_INTERVAL_MS,
            ]
        );
    }

    #[test]
    fn config_builder_preserves_host_mock_and_custom_hold_thresholds() {
        let reset = GpioBootButtonConfig::new().with_hold_durations(3000, 6000);

        assert_eq!(reset.ecosystem_hold_ms, 3000);
        assert_eq!(reset.factory_hold_ms, 6000);
        assert!(reset.indicator.is_none());
        assert!(reset.pin.is_high());
    }

    #[test]
    fn shared_button_config_accepts_hvac_led_feedback_adapter() {
        let indicator: &'static crate::led::GpioLedDriver =
            Box::leak(Box::new(crate::led::GpioLedDriver::new()));

        let reset = GpioBootButtonConfig::new().with_indicator(indicator);

        assert!(reset.indicator.is_some());
    }
}
