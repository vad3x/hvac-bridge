//! Unit and integration tests for ResetService and App coordinator.
//!
//! Follows AAA (Arrange, Act, Assert) pattern strictly.

use core::cell::Cell;
use core::sync::atomic::{AtomicBool, Ordering};
use hvac_core::app::App;
use hvac_core::domain::{HvacPatch, HvacState, ResetScope};
use hvac_core::events::{SystemState, SystemStateSender, SystemStateWatch};
use hvac_core::ports::{HvacDriverError, HvacUnitDriver, ResetTriggerDriver, SystemResetDriver};
use hvac_core::service::ResetService;

struct MockTrigger {
    scope: ResetScope,
}

impl ResetTriggerDriver for MockTrigger {
    async fn wait_for_reset(&mut self) -> ResetScope {
        self.scope
    }
}

struct MockReset {
    erased_scope: Cell<Option<ResetScope>>,
    reboot_called: AtomicBool,
}

impl MockReset {
    fn new() -> Self {
        Self {
            erased_scope: Cell::new(None),
            reboot_called: AtomicBool::new(false),
        }
    }
}

impl SystemResetDriver for MockReset {
    fn erase_credentials(&self, scope: ResetScope) {
        self.erased_scope.set(Some(scope));
    }

    fn reboot(&self) -> ! {
        self.reboot_called.store(true, Ordering::SeqCst);
        panic!("Mock reboot");
    }
}

struct MockUnit;

impl HvacUnitDriver for MockUnit {
    async fn apply_patch(&mut self, _patch: &HvacPatch) -> Result<HvacState, HvacDriverError> {
        Ok(HvacState::default())
    }

    async fn poll(&mut self) -> Result<HvacState, HvacDriverError> {
        Ok(HvacState::default())
    }
}

#[test]
fn reset_service_erases_ecosystem_scope_and_reboots() {
    // Arrange
    let trigger = MockTrigger {
        scope: ResetScope::Ecosystem,
    };
    let reset = MockReset::new();
    let mut service = ResetService::new(trigger, reset);
    let state_watch = SystemStateWatch::new();
    let state_sender = SystemStateSender::new(&state_watch);

    // Act
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        futures::executor::block_on(service.run(state_sender));
    }));

    // Assert
    assert_eq!(
        service.reset.erased_scope.get(),
        Some(ResetScope::Ecosystem)
    );
    assert_eq!(state_watch.try_get(), Some(SystemState::EcosystemReset));
}

#[test]
fn reset_service_erases_factory_scope_and_reboots() {
    // Arrange
    let trigger = MockTrigger {
        scope: ResetScope::Factory,
    };
    let reset = MockReset::new();
    let mut service = ResetService::new(trigger, reset);
    let state_watch = SystemStateWatch::new();
    let state_sender = SystemStateSender::new(&state_watch);

    // Act
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        futures::executor::block_on(service.run(state_sender));
    }));

    // Assert
    assert_eq!(service.reset.erased_scope.get(), Some(ResetScope::Factory));
    assert_eq!(state_watch.try_get(), Some(SystemState::FactoryReset));
}

#[test]
fn app_compilation_and_mock_instantiation() {
    // Arrange
    let unit = MockUnit;
    let reporter = ();
    let trigger = MockTrigger {
        scope: ResetScope::Factory,
    };
    let reset = MockReset::new();

    // Act
    let mut app = App::new(unit, reporter, trigger, reset);

    // Assert
    assert!(app.hvac_controller().last_reported_state().is_none());
    assert_eq!(app.reset_service().driver.scope, ResetScope::Factory);
    assert!(app.hvac_controller_mut().last_reported_state().is_none());
}
