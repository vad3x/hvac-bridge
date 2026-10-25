//! HVAC Bridge - System Reset Service
//!
//! Coordinates reset hold times with `SystemStateSender` and `SystemResetDriver`.

use embassy_time::{Duration, Timer};
use log::info;

use crate::config::FACTORY_RESET_REBOOT_DELAY_MS;
use crate::events::SystemStateSender;
use crate::ports::reset::{ResetScope, ResetTriggerDriver, SystemResetDriver};

// ============================================================================
// ResetService
// ============================================================================

/// System reset lifecycle service.
pub struct ResetService<T: ResetTriggerDriver, R: SystemResetDriver> {
    pub driver: T,
    pub reset: R,
}

impl<T: ResetTriggerDriver, R: SystemResetDriver> ResetService<T, R> {
    /// Create a new ResetService from trigger and reset drivers.
    pub fn new(driver: T, reset: R) -> Self {
        Self { driver, reset }
    }

    /// Run the reset monitoring loop. Awaits trigger, publishes state, erases credentials,
    /// waits the reboot delay, and triggers hardware reboot.
    pub async fn run(&mut self, state_sender: SystemStateSender<'_>) {
        info!("monitoring for reset trigger");

        let scope = self.driver.wait_for_reset().await;

        match scope {
            ResetScope::Ecosystem => {
                info!("=== ECOSYSTEM RESET TRIGGERED ===");
                state_sender.ecosystem_reset();
            }
            ResetScope::Factory => {
                info!("=== FACTORY RESET TRIGGERED ===");
                state_sender.factory_reset();
            }
        }

        self.reset.erase_credentials(scope);

        Timer::after(Duration::from_millis(FACTORY_RESET_REBOOT_DELAY_MS)).await;

        info!("Rebooting...");
        self.reset.reboot();
    }
}
