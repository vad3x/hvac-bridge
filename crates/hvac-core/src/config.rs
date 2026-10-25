//! HVAC Bridge - Domain Configuration Constants
//!
//! Protocol-agnostic and hardware-agnostic timing policies for the core domain.

// ============================================================================
// HVAC Controller & Service Policies
// ============================================================================

/// Periodic HVAC unit polling interval (ms).
pub const POLL_STEP_INTERVAL_MS: u64 = 300;

/// Command coalescing window duration (ms).
pub const COMMAND_COALESCE_WINDOW_MS: u64 = 50;

/// Delay before reboot after factory credential erasure (ms) — allows feedback propagation.
pub const FACTORY_RESET_REBOOT_DELAY_MS: u64 = 2000;
