//! HVAC Bridge - GPIO Adapter Configuration
//!
//! Electrical pin assignments, timing constants, debounce intervals, and LED
//! blink patterns specific to the CN105 HVAC Matter Bridge hardware (`ESP32-C6`).

// ============================================================================
// Hardware Pin Assignments (cn105-matter-bridge-HEAD Rev 1.0)
// ============================================================================

/// Status LED pin assignment (`LED_STATUS_N`, active-low red LED).
pub const PIN_LED_STATUS: u8 = 15;

/// BOOT button pin assignment (`IO_BOOT`, active-low with internal pull-up).
pub const PIN_BOOT_BUTTON: u8 = 9;

// ============================================================================
// Reset Hold & Debounce Timings
// ============================================================================

/// How long the BOOT button must be held to trigger ecosystem reset (unpair Matter, preserve Wi-Fi).
pub const ECOSYSTEM_RESET_HOLD_MS: u64 = 5000;

/// How long the BOOT button must be held to trigger full factory reset (erase Wi-Fi + Matter).
pub const FACTORY_RESET_HOLD_MS: u64 = 10000;

/// Physical button debounce duration in milliseconds.
pub const BUTTON_DEBOUNCE_MS: u64 = 30;

/// Interval for polling button press duration in milliseconds.
pub const BUTTON_POLL_INTERVAL_MS: u64 = 100;

// ============================================================================
// HVAC Activity Feedback
// ============================================================================

/// Brief activity pulse (wink) duration in milliseconds when HVAC state updates are applied.
pub const ACTIVITY_PULSE_MS: u64 = 150;

// ============================================================================
// LED Blink Patterns — Asymmetric Duty Cycles (on_ms / off_ms)
//
// Duty cycle increases with system readiness:
//   Booting (17%) -> Starting (30%) -> Commissioning (2 Hz) / WifiConnecting (1 Hz)
//   -> EcosystemConnecting (90%) -> Operational (solid ON)
// ============================================================================

/// Booting: slow heartbeat — brief flash, long dark (~17% duty).
pub const BOOTING_ON_MS: u64 = 200;
pub const BOOTING_OFF_MS: u64 = 1000;

/// Starting: faster heartbeat — hardware ready, initializing stacks (~30% duty).
pub const STARTING_ON_MS: u64 = 300;
pub const STARTING_OFF_MS: u64 = 700;

/// Commissioning: rapid 2 Hz blink — awaiting initial Matter pairing (50% duty).
pub const COMMISSIONING_ON_MS: u64 = 250;
pub const COMMISSIONING_OFF_MS: u64 = 250;

/// WifiConnecting: even medium 1 Hz blink — associating with Wi-Fi network (50% duty).
pub const WIFI_CONNECTING_ON_MS: u64 = 500;
pub const WIFI_CONNECTING_OFF_MS: u64 = 500;

/// EcosystemConnecting: mostly on, brief wink — IP acquired, awaiting controller (90% duty).
pub const ECOSYSTEM_CONNECTING_ON_MS: u64 = 900;
pub const ECOSYSTEM_CONNECTING_OFF_MS: u64 = 100;

/// Error: burst pattern — 2 quick flashes then a long pause (irregular rhythm).
pub const ERROR_ON_MS: u64 = 100;
pub const ERROR_OFF_MS: u64 = 100;
pub const ERROR_BURST_COUNT: u8 = 2;
pub const ERROR_PAUSE_MS: u64 = 700;

/// Ecosystem reset: medium 2 Hz blink — unpairing active ecosystem (50% duty).
pub const ECOSYSTEM_RESET_ON_MS: u64 = 250;
pub const ECOSYSTEM_RESET_OFF_MS: u64 = 250;

/// Factory reset: ultra-rapid strobe — critical credential wipe in progress (10 Hz).
pub const FACTORY_RESET_ON_MS: u64 = 50;
pub const FACTORY_RESET_OFF_MS: u64 = 50;

/// Rapid strobe period during OTA update (50ms ON, 50ms OFF = 10 Hz).
pub const UPDATING_STROBE_MS: u64 = 50;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_assignments_match_schematic_rev1() {
        // Arrange & Act & Assert
        assert_eq!(PIN_LED_STATUS, 15);
        assert_eq!(PIN_BOOT_BUTTON, 9);
    }

    #[test]
    fn timing_constants_are_valid() {
        // Arrange & Act & Assert
        assert!(ECOSYSTEM_RESET_HOLD_MS < FACTORY_RESET_HOLD_MS);
        assert_eq!(ECOSYSTEM_RESET_HOLD_MS, 5000);
        assert_eq!(FACTORY_RESET_HOLD_MS, 10000);
        assert_eq!(ACTIVITY_PULSE_MS, 150);
        assert_eq!(UPDATING_STROBE_MS, 50);
    }
}
