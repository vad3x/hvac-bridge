//! Integration tests for HvacController and Ports.
//!
//! Follows AAA (Arrange, Act, Assert) pattern strictly.

use core::cell::RefCell;
use hvac_core::domain::{
    FanSpeed, HvacMode, HvacPatch, HvacState, TemperatureCentiC, VaneHorizontal, VaneVertical,
};
use hvac_core::events::{HvacStateSender, HvacStateWatch};
use hvac_core::ports::{
    CommandChannel, HvacControllerHandle, HvacDriverError, HvacReporter, HvacUnitDriver,
};
use hvac_core::service::HvacController;

struct MockUnitDriver {
    state: HvacState,
    applied_patches: Vec<HvacPatch>,
    poll_count: usize,
}

impl MockUnitDriver {
    fn new() -> Self {
        Self {
            state: HvacState {
                connected: true,
                ..Default::default()
            },
            applied_patches: Vec::new(),
            poll_count: 0,
        }
    }
}

impl HvacUnitDriver for MockUnitDriver {
    async fn apply_patch(&mut self, patch: &HvacPatch) -> Result<HvacState, HvacDriverError> {
        self.applied_patches.push(*patch);
        self.state = self.state.with_patch(patch);
        Ok(self.state)
    }

    async fn poll(&mut self) -> Result<HvacState, HvacDriverError> {
        self.poll_count += 1;
        if self.state.room_temp.is_none() {
            self.state.room_temp = Some(TemperatureCentiC::from_centi(2300));
        }
        Ok(self.state)
    }
}

struct MockReporter {
    reported: RefCell<Vec<HvacState>>,
}

impl MockReporter {
    fn new() -> Self {
        Self {
            reported: RefCell::new(Vec::new()),
        }
    }
}

impl HvacReporter for MockReporter {
    fn report_state(&self, state: &HvacState) {
        self.reported.borrow_mut().push(*state);
    }
}

#[test]
fn hvac_controller_coalesces_queued_commands_into_single_patch() {
    // Arrange
    let channel = CommandChannel::new();
    let handle = HvacControllerHandle::new(channel.sender());
    let receiver = channel.receiver();
    let unit = MockUnitDriver::new();
    let reporter = MockReporter::new();
    let mut controller = HvacController::new(unit, &reporter);

    handle.set_power_and_mode(true, HvacMode::Cool);
    handle.set_target_temp(TemperatureCentiC::from_centi(2150));
    handle.set_fan_speed(FanSpeed::Speed3);

    // Act
    let result = futures::executor::block_on(controller.process_pending_commands(&receiver));

    // Assert
    assert_eq!(result, Ok(true));
    assert_eq!(controller.unit().applied_patches.len(), 1);
    let applied = controller.unit().applied_patches[0];
    assert_eq!(applied.power, Some(true));
    assert_eq!(applied.mode, Some(HvacMode::Cool));
    assert_eq!(applied.target_temp, Some(TemperatureCentiC(2150)));
    assert_eq!(applied.fan_speed, Some(FanSpeed::Speed3));
    assert_eq!(reporter.reported.borrow().len(), 1);
}

#[test]
fn hvac_controller_poll_reports_only_when_state_changes() {
    // Arrange
    let unit = MockUnitDriver::new();
    let reporter = MockReporter::new();
    let mut controller = HvacController::new(unit, &reporter);

    // Act - First poll returns state with room_temp = 23.0°C (changes from initial None)
    let first = futures::executor::block_on(controller.poll_step());

    // Assert
    assert!(first.is_ok());
    assert_eq!(reporter.reported.borrow().len(), 1);
    assert_eq!(
        reporter.reported.borrow()[0].room_temp,
        Some(TemperatureCentiC(2300))
    );

    // Act - Second poll returns same state
    let second = futures::executor::block_on(controller.poll_step());

    // Assert - No duplicate report sent
    assert!(second.is_ok());
    assert_eq!(reporter.reported.borrow().len(), 1);

    // Act - Unit driver changes room temp to 24.5°C
    controller.unit_mut().state.room_temp = Some(TemperatureCentiC(2450));
    let third = futures::executor::block_on(controller.poll_step());

    // Assert - New state reported
    assert!(third.is_ok());
    assert_eq!(reporter.reported.borrow().len(), 2);
    assert_eq!(
        reporter.reported.borrow()[1].room_temp,
        Some(TemperatureCentiC(2450))
    );
}

#[test]
fn hvac_controller_empty_queue_does_nothing() {
    // Arrange
    let channel = CommandChannel::new();
    let receiver = channel.receiver();
    let unit = MockUnitDriver::new();
    let reporter = MockReporter::new();
    let mut controller = HvacController::new(unit, &reporter);

    // Act
    let result = futures::executor::block_on(controller.process_pending_commands(&receiver));

    // Assert
    assert_eq!(result, Ok(false));
    assert_eq!(controller.unit().applied_patches.len(), 0);
    assert_eq!(reporter.reported.borrow().len(), 0);
}

#[test]
fn hvac_controller_handle_dispatches_all_attributes() {
    // Arrange
    let channel = CommandChannel::new();
    let handle = HvacControllerHandle::new(channel.sender());
    let receiver = channel.receiver();
    let unit = MockUnitDriver::new();
    let reporter = MockReporter::new();
    let mut controller = HvacController::new(unit, &reporter);

    // Act - Dispatch individual setters
    handle.set_power(true);
    handle.set_mode(HvacMode::Heat);
    handle.set_target_temp(TemperatureCentiC(2200));
    handle.set_fan_speed(FanSpeed::Speed4);
    handle.set_vane_vertical(VaneVertical::Swing);
    handle.set_vane_horizontal(VaneHorizontal::Center);

    let result = futures::executor::block_on(controller.process_pending_commands(&receiver));

    // Assert
    assert_eq!(result, Ok(true));
    assert_eq!(controller.unit().applied_patches.len(), 1);
    let patch = controller.unit().applied_patches[0];
    assert_eq!(patch.power, Some(true));
    assert_eq!(patch.mode, Some(HvacMode::Heat));
    assert_eq!(patch.target_temp, Some(TemperatureCentiC(2200)));
    assert_eq!(patch.fan_speed, Some(FanSpeed::Speed4));
    assert_eq!(patch.vane_vertical, Some(VaneVertical::Swing));
    assert_eq!(patch.vane_horizontal, Some(VaneHorizontal::Center));
}

#[test]
fn hvac_reporter_tuple_and_sender_fanout() {
    // Arrange
    let reporter_a = MockReporter::new();
    let reporter_b = MockReporter::new();
    let watch = HvacStateWatch::new();
    let state_sender = HvacStateSender::new(&watch);
    let mut state_receiver = watch.receiver().expect("Receiver available");

    // Fanout tuple: (reporter_a, (reporter_b, state_sender))
    let fanout = (&reporter_a, (&reporter_b, state_sender));
    let test_state = HvacState {
        connected: true,
        power: true,
        mode: HvacMode::Cool,
        target_temp: TemperatureCentiC(2000),
        ..Default::default()
    };

    // Act
    fanout.report_state(&test_state);

    // Assert
    assert_eq!(reporter_a.reported.borrow().len(), 1);
    assert_eq!(reporter_a.reported.borrow()[0], test_state);
    assert_eq!(reporter_b.reported.borrow().len(), 1);
    assert_eq!(reporter_b.reported.borrow()[0], test_state);
    assert_eq!(state_receiver.try_get(), Some(test_state));
}
