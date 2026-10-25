//! Shared system lifecycle contracts and HVAC-specific state observation.

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::{Receiver as WatchReceiver, Watch};

use crate::domain::hvac::HvacState;

pub use edf_core::events::{
    SystemState, SystemStateReceiver, SystemStateSender, SystemStateWatch, STATE_WATCH_RECEIVERS,
};

/// Maximum concurrent receivers for HVAC unit state.
pub const HVAC_STATE_WATCH_RECEIVERS: usize = 2;

/// Continuous HVAC unit state watch signal (zero-allocation, zero queue overflow).
pub type HvacStateWatch = Watch<CriticalSectionRawMutex, HvacState, HVAC_STATE_WATCH_RECEIVERS>;

/// Watch receiver for observing HVAC unit state changes.
pub type HvacStateReceiver<'a> =
    WatchReceiver<'a, CriticalSectionRawMutex, HvacState, HVAC_STATE_WATCH_RECEIVERS>;

/// Lightweight, Copy handle for sending HVAC unit state updates.
#[derive(Clone, Copy)]
pub struct HvacStateSender<'a>(&'a HvacStateWatch);

impl<'a> HvacStateSender<'a> {
    /// Create a new sender handle wrapping a `HvacStateWatch`.
    pub const fn new(watch: &'a HvacStateWatch) -> Self {
        Self(watch)
    }

    /// Query the current HVAC state, if available.
    pub fn current_state(&self) -> Option<HvacState> {
        self.0.try_get()
    }

    /// Send a new HVAC state.
    pub fn send(&self, state: HvacState) {
        self.0.sender().send(state);
    }
}

#[cfg(test)]
mod tests {
    use super::{HvacStateSender, HvacStateWatch};
    use crate::domain::hvac::HvacState;

    #[test]
    fn hvac_state_sender_and_receiver_flow() {
        let watch = HvacStateWatch::new();
        let sender = HvacStateSender::new(&watch);
        let mut receiver = watch.receiver().expect("receiver available");
        let state = HvacState {
            connected: true,
            power: true,
            ..Default::default()
        };

        assert_eq!(sender.current_state(), None);
        sender.send(state);
        assert_eq!(sender.current_state(), Some(state));
        assert_eq!(receiver.try_get(), Some(state));
    }
}
