//! HVAC Bridge - GPIO LED Indicator Driver
//!
//! HVAC LED profile and adapter around the shared lifecycle indicator.

use edf_gpio::led::{GpioLedDriver as SharedGpioLedDriver, LedProfile, LedTimings};
use hvac_core::domain::HvacState;
use hvac_core::events::{SystemState, SystemStateReceiver};
use hvac_core::ports::HvacReporter;

use crate::config;

// ============================================================================
// Public Structs & Implementation
// ============================================================================

/// LED pattern timings selected by the HVAC product.
pub struct HvacLedProfile;

impl HvacLedProfile {
    /// Product timings mapped from [`crate::config`].
    pub const TIMINGS: LedTimings = LedTimings {
        booting_on_ms: config::BOOTING_ON_MS,
        booting_off_ms: config::BOOTING_OFF_MS,
        starting_on_ms: config::STARTING_ON_MS,
        starting_off_ms: config::STARTING_OFF_MS,
        commissioning_on_ms: config::COMMISSIONING_ON_MS,
        commissioning_off_ms: config::COMMISSIONING_OFF_MS,
        wifi_connecting_on_ms: config::WIFI_CONNECTING_ON_MS,
        wifi_connecting_off_ms: config::WIFI_CONNECTING_OFF_MS,
        ecosystem_connecting_on_ms: config::ECOSYSTEM_CONNECTING_ON_MS,
        ecosystem_connecting_off_ms: config::ECOSYSTEM_CONNECTING_OFF_MS,
        error_on_ms: config::ERROR_ON_MS,
        error_off_ms: config::ERROR_OFF_MS,
        error_burst_count: config::ERROR_BURST_COUNT,
        error_pause_ms: config::ERROR_PAUSE_MS,
        ecosystem_reset_on_ms: config::ECOSYSTEM_RESET_ON_MS,
        ecosystem_reset_off_ms: config::ECOSYSTEM_RESET_OFF_MS,
        factory_reset_on_ms: config::FACTORY_RESET_ON_MS,
        factory_reset_off_ms: config::FACTORY_RESET_OFF_MS,
        updating_strobe_ms: config::UPDATING_STROBE_MS,
        activity_pulse_ms: config::ACTIVITY_PULSE_MS,
    };
}

impl LedProfile for HvacLedProfile {
    const TIMINGS: LedTimings = Self::TIMINGS;
}

/// HVAC adapter for the shared active-low GPIO status LED driver.
///
/// It retains the HVAC-facing API and implements [`HvacReporter`] for activity pulses.
pub struct GpioLedDriver {
    inner: SharedGpioLedDriver<HvacLedProfile>,
}

impl GpioLedDriver {
    /// Create a new LED driver with the target hardware output pin on RISC-V.
    #[cfg(target_arch = "riscv32")]
    pub fn new(pin: impl esp_hal::gpio::OutputPin + 'static) -> Self {
        Self {
            inner: SharedGpioLedDriver::with_profile(pin),
        }
    }

    /// Create a new LED driver with simulated pin for host testing.
    #[cfg(not(target_arch = "riscv32"))]
    pub fn new() -> Self {
        Self {
            inner: SharedGpioLedDriver::with_profile(),
        }
    }

    /// Driver event loop — observes lifecycle states and drives hardware pin timing.
    /// Runs concurrently with other firmware tasks in the composition root.
    pub async fn run(&self, state_rx: SystemStateReceiver<'_>) {
        self.inner.run(state_rx).await;
    }

    /// Indicate that the reset/BOOT button is currently being pressed (stage 0: 0s-5s).
    pub fn hold_feedback_press(&self) {
        self.inner.hold_feedback_press();
    }

    /// Indicate that stage 1 (Ecosystem reset, 5s) has been reached.
    pub fn hold_feedback_stage1(&self) {
        self.inner.hold_feedback_stage1();
    }

    /// Indicate that stage 2 (Factory reset, 10s) has been reached.
    pub fn hold_feedback_stage2(&self) {
        self.inner.hold_feedback_stage2();
    }

    /// Re-apply the current system lifecycle state (e.g. after button is released early).
    pub fn restore_state(&self) {
        self.inner.restore_state();
    }

    /// Query whether the LED is currently powered ON (pin is LOW).
    pub fn is_on(&self) -> bool {
        self.inner.is_on()
    }

    /// Query the last observed system lifecycle state.
    pub fn observed_state(&self) -> Option<SystemState> {
        self.inner.observed_state()
    }
}

#[cfg(not(target_arch = "riscv32"))]
impl Default for GpioLedDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl HvacReporter for GpioLedDriver {
    fn report_state(&self, _state: &HvacState) {
        self.inner.activity_pulse();
    }
}

impl edf_gpio::led::HoldFeedback for GpioLedDriver {
    fn hold_feedback_press(&self) {
        GpioLedDriver::hold_feedback_press(self);
    }

    fn hold_feedback_stage1(&self) {
        GpioLedDriver::hold_feedback_stage1(self);
    }

    fn hold_feedback_stage2(&self) {
        GpioLedDriver::hold_feedback_stage2(self);
    }

    fn restore_state(&self) {
        GpioLedDriver::restore_state(self);
    }
}

/// Shared simulated output pin retained at the historical HVAC module path.
#[cfg(not(target_arch = "riscv32"))]
pub use edf_gpio::led::MockLedPin;

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hvac_led_profile_matches_existing_hvac_timings() {
        let timings = HvacLedProfile::TIMINGS;
        let actual = [
            timings.booting_on_ms,
            timings.booting_off_ms,
            timings.starting_on_ms,
            timings.starting_off_ms,
            timings.commissioning_on_ms,
            timings.commissioning_off_ms,
            timings.wifi_connecting_on_ms,
            timings.wifi_connecting_off_ms,
            timings.ecosystem_connecting_on_ms,
            timings.ecosystem_connecting_off_ms,
            timings.error_on_ms,
            timings.error_off_ms,
            u64::from(timings.error_burst_count),
            timings.error_pause_ms,
            timings.ecosystem_reset_on_ms,
            timings.ecosystem_reset_off_ms,
            timings.factory_reset_on_ms,
            timings.factory_reset_off_ms,
            timings.updating_strobe_ms,
            timings.activity_pulse_ms,
        ];
        let expected = [
            config::BOOTING_ON_MS,
            config::BOOTING_OFF_MS,
            config::STARTING_ON_MS,
            config::STARTING_OFF_MS,
            config::COMMISSIONING_ON_MS,
            config::COMMISSIONING_OFF_MS,
            config::WIFI_CONNECTING_ON_MS,
            config::WIFI_CONNECTING_OFF_MS,
            config::ECOSYSTEM_CONNECTING_ON_MS,
            config::ECOSYSTEM_CONNECTING_OFF_MS,
            config::ERROR_ON_MS,
            config::ERROR_OFF_MS,
            u64::from(config::ERROR_BURST_COUNT),
            config::ERROR_PAUSE_MS,
            config::ECOSYSTEM_RESET_ON_MS,
            config::ECOSYSTEM_RESET_OFF_MS,
            config::FACTORY_RESET_ON_MS,
            config::FACTORY_RESET_OFF_MS,
            config::UPDATING_STROBE_MS,
            config::ACTIVITY_PULSE_MS,
        ];

        assert_eq!(actual, expected);
    }
}
