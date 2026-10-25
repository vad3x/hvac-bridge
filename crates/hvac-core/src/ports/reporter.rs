//! HVAC Bridge - Outbound Reporting Port
//!
//! Defines the protocol-agnostic reporting interface for smart home ecosystems
//! and UI indicators. Outbound adapters (e.g. Matter cluster reporters, GPIO LED indicators,
//! state watch channels) implement this port.

use crate::domain::HvacState;
use crate::events::HvacStateSender;

/// Outbound capability: notifies external observers and smart home ecosystems of HVAC state changes.
///
/// Decoupled from protocol details (e.g. Matter attributes, HTTP SSE, LED blinks).
pub trait HvacReporter {
    /// Report the latest canonical HVAC unit state to external listeners.
    fn report_state(&self, state: &HvacState);
}

impl<T: HvacReporter> HvacReporter for &T {
    fn report_state(&self, state: &HvacState) {
        (*self).report_state(state);
    }
}

impl<A: HvacReporter, B: HvacReporter> HvacReporter for (A, B) {
    fn report_state(&self, state: &HvacState) {
        self.0.report_state(state);
        self.1.report_state(state);
    }
}

impl<'a> HvacReporter for HvacStateSender<'a> {
    fn report_state(&self, state: &HvacState) {
        self.send(*state);
    }
}

impl HvacReporter for () {
    fn report_state(&self, _state: &HvacState) {}
}
