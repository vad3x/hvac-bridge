//! HVAC Bridge - Inbound Driving Ports
//!
//! Defines command queue types and the `HvacControllerHandle` primary driving port.
//! Inbound adapters (e.g. Matter cluster handlers, HTTP API) use this handle to command the HVAC core.

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender};
use log::warn;

use crate::domain::{
    FanSpeed, HvacMode, HvacPatch, TemperatureCentiC, VaneHorizontal, VaneVertical,
};

// ============================================================================
// Command Channel Types
// ============================================================================

/// Capacity of the command queue (up to 8 queued patches before backpressure).
pub const CMD_QUEUE_CAP: usize = 8;

/// Channel for queuing inbound HVAC mutation patches.
pub type CommandChannel = Channel<CriticalSectionRawMutex, HvacPatch, CMD_QUEUE_CAP>;

/// Sender endpoint for dispatching HVAC mutation patches.
pub type CommandSender<'a> = Sender<'a, CriticalSectionRawMutex, HvacPatch, CMD_QUEUE_CAP>;

/// Receiver endpoint for consuming HVAC mutation patches.
pub type CommandReceiver<'a> = Receiver<'a, CriticalSectionRawMutex, HvacPatch, CMD_QUEUE_CAP>;

/// Error returned when the inbound command queue is full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueFullError;

// ============================================================================
// Inbound Driving Handle
// ============================================================================

/// Inbound Command Handle — the primary driving port for the HVAC controller.
///
/// Implements value semantics (`Copy`), owns no heap memory, and provides
/// non-blocking, ergonomic dispatch methods for inbound adapters.
#[derive(Clone, Copy)]
pub struct HvacControllerHandle<'a> {
    sender: CommandSender<'a>,
}

impl<'a> HvacControllerHandle<'a> {
    /// Create a new handle wrapping a command channel sender.
    pub const fn new(sender: CommandSender<'a>) -> Self {
        Self { sender }
    }

    /// Dispatch a partial settings patch to the HVAC unit.
    pub fn apply_patch(&self, patch: HvacPatch) {
        if self.try_apply_patch(patch).is_err() {
            warn!("HVAC command dropped: command queue full");
        }
    }

    /// Try to dispatch a partial settings patch, returning Err if the queue is full.
    pub fn try_apply_patch(&self, patch: HvacPatch) -> Result<(), QueueFullError> {
        self.sender.try_send(patch).map_err(|_| QueueFullError)
    }

    /// Turn power on or off.
    pub fn set_power(&self, power: bool) {
        self.apply_patch(HvacPatch {
            power: Some(power),
            ..Default::default()
        });
    }

    /// Set operating mode (e.g. Cool, Heat, Auto, Dry, FanOnly).
    pub fn set_mode(&self, mode: HvacMode) {
        self.apply_patch(HvacPatch {
            mode: Some(mode),
            ..Default::default()
        });
    }

    /// Set power and operating mode atomically.
    pub fn set_power_and_mode(&self, power: bool, mode: HvacMode) {
        self.apply_patch(HvacPatch {
            power: Some(power),
            mode: Some(mode),
            ..Default::default()
        });
    }

    /// Set target temperature setpoint.
    pub fn set_target_temp(&self, target_temp: TemperatureCentiC) {
        self.apply_patch(HvacPatch {
            target_temp: Some(target_temp),
            ..Default::default()
        });
    }

    /// Set blower fan speed.
    pub fn set_fan_speed(&self, fan_speed: FanSpeed) {
        self.apply_patch(HvacPatch {
            fan_speed: Some(fan_speed),
            ..Default::default()
        });
    }

    /// Set vertical vane position or swing.
    pub fn set_vane_vertical(&self, vane_vertical: VaneVertical) {
        self.apply_patch(HvacPatch {
            vane_vertical: Some(vane_vertical),
            ..Default::default()
        });
    }

    /// Set horizontal vane position or swing.
    pub fn set_vane_horizontal(&self, vane_horizontal: VaneHorizontal) {
        self.apply_patch(HvacPatch {
            vane_horizontal: Some(vane_horizontal),
            ..Default::default()
        });
    }

    /// Access the underlying raw sender.
    pub fn sender(&self) -> &CommandSender<'a> {
        &self.sender
    }
}
