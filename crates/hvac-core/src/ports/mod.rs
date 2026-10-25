//! HVAC Bridge - Outbound & Inbound Port Traits
//!
//! In Hexagonal Architecture (Ports & Adapters), ports define abstract capability contracts
//! required or provided by the core domain application. Edge adapters implement these traits.

pub mod diagnostics;
pub mod ecosystem;
pub mod inbound;
pub mod ota;
pub mod reporter;
pub mod reset;

pub use diagnostics::HardwareInfoDriver;
pub use ecosystem::EcosystemInfoDriver;
pub use inbound::{
    CommandChannel, CommandReceiver, CommandSender, HvacControllerHandle, QueueFullError,
    CMD_QUEUE_CAP,
};
pub use ota::{OtaDriverError, OtaFlashDriver, OtaStream};
pub use reporter::HvacReporter;
pub use reset::{ResetScope, ResetTriggerDriver, SystemResetDriver};

use crate::domain::{HvacPatch, HvacState};

// ============================================================================
// HVAC Unit Driver Port
// ============================================================================

/// Errors originating from HVAC unit hardware communication or protocol decoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HvacDriverError {
    /// Unit is not physically connected or UART link is unresponsive.
    NotConnected,
    /// Response packet timed out.
    Timeout,
    /// Packet checksum failed verification.
    ChecksumMismatch,
    /// Packet structure, command header, or length was invalid.
    InvalidFrame,
    /// Lower-level serial I/O error.
    IoError,
}

/// Outbound hardware driver port for communicating with a physical HVAC indoor unit.
///
/// Implemented by vendor protocol adapters (e.g. `hvac-adapter-cn105`).
#[allow(async_fn_in_trait)]
pub trait HvacUnitDriver {
    /// Apply a coalesced settings patch to the physical HVAC unit and return updated state.
    async fn apply_patch(&mut self, patch: &HvacPatch) -> Result<HvacState, HvacDriverError>;

    /// Perform one polling step/cycle and return the current unit state.
    async fn poll(&mut self) -> Result<HvacState, HvacDriverError>;
}

impl<T: HvacUnitDriver> HvacUnitDriver for &mut T {
    async fn apply_patch(&mut self, patch: &HvacPatch) -> Result<HvacState, HvacDriverError> {
        (**self).apply_patch(patch).await
    }

    async fn poll(&mut self) -> Result<HvacState, HvacDriverError> {
        (**self).poll().await
    }
}
