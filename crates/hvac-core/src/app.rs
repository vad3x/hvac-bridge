//! HVAC Bridge - Application Coordinator
//!
//! Wires trait-based drivers into application services and runs them concurrently
//! via Embassy async combinators.
//!
//! All dependencies are injected: port drivers, state sender, command receiver.
//! No statics are accessed here — fully testable.

use log::info;

use crate::events::SystemStateSender;
use crate::ports::inbound::CommandReceiver;
use crate::ports::reporter::HvacReporter;
use crate::ports::reset::{ResetTriggerDriver, SystemResetDriver};
use crate::ports::HvacUnitDriver;
use crate::service::hvac::HvacController;
use crate::service::reset::ResetService;

// ============================================================================
// App Coordinator
// ============================================================================

/// The HVAC Bridge core application coordinator.
///
/// Generic over port driver implementations.
/// Owns all core domain services and runs them concurrently.
pub struct App<U, REP, T, RST>
where
    U: HvacUnitDriver,
    REP: HvacReporter,
    T: ResetTriggerDriver,
    RST: SystemResetDriver,
{
    hvac_controller: HvacController<U, REP>,
    reset_service: ResetService<T, RST>,
}

impl<U, REP, T, RST> App<U, REP, T, RST>
where
    U: HvacUnitDriver,
    REP: HvacReporter,
    T: ResetTriggerDriver,
    RST: SystemResetDriver,
{
    /// Create the application from port driver implementations.
    pub fn new(unit: U, reporter: REP, reset_trigger: T, reset: RST) -> Self {
        Self {
            hvac_controller: HvacController::new(unit, reporter),
            reset_service: ResetService::new(reset_trigger, reset),
        }
    }

    /// Access the underlying HVAC controller.
    pub fn hvac_controller(&self) -> &HvacController<U, REP> {
        &self.hvac_controller
    }

    /// Access the underlying HVAC controller mutably.
    pub fn hvac_controller_mut(&mut self) -> &mut HvacController<U, REP> {
        &mut self.hvac_controller
    }

    /// Access the underlying reset service.
    pub fn reset_service(&self) -> &ResetService<T, RST> {
        &self.reset_service
    }

    /// Access the underlying reset service mutably.
    pub fn reset_service_mut(&mut self) -> &mut ResetService<T, RST> {
        &mut self.reset_service
    }

    /// Run all core domain services concurrently (never returns).
    pub async fn run(
        mut self,
        state_sender: SystemStateSender<'_>,
        receiver: &CommandReceiver<'_>,
    ) {
        info!("publishing Starting state");
        state_sender.starting();

        info!("starting core services");

        embassy_futures::join::join(
            self.hvac_controller.run(receiver),
            self.reset_service.run(state_sender),
        )
        .await;
    }
}
