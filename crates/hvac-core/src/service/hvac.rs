//! HVAC Bridge - HVAC Controller Service
//!
//! Deep application service that owns the physical unit driver and reporter.
//! Coalesces bursty incoming commands, runs periodic polling, and notifies
//! reporters only when canonical unit state changes.

use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Timer};
use log::warn;

use crate::config::{COMMAND_COALESCE_WINDOW_MS, POLL_STEP_INTERVAL_MS};
use crate::domain::{HvacPatch, HvacState};
use crate::ports::inbound::CommandReceiver;
use crate::ports::reporter::HvacReporter;
use crate::ports::{HvacDriverError, HvacUnitDriver};

// ============================================================================
// HvacController Service
// ============================================================================

/// HVAC Controller service coordinating command coalescing, polling, and state reporting.
pub struct HvacController<U: HvacUnitDriver, REP: HvacReporter> {
    unit: U,
    reporter: REP,
    last_reported_state: Option<HvacState>,
}

impl<U: HvacUnitDriver, REP: HvacReporter> HvacController<U, REP> {
    /// Create a new HVAC Controller instance.
    pub fn new(unit: U, reporter: REP) -> Self {
        Self {
            unit,
            reporter,
            last_reported_state: None,
        }
    }

    /// Immutable reference to the underlying unit driver.
    pub fn unit(&self) -> &U {
        &self.unit
    }

    /// Mutable reference to the underlying unit driver.
    pub fn unit_mut(&mut self) -> &mut U {
        &mut self.unit
    }

    /// Immutable reference to the reporter.
    pub fn reporter(&self) -> &REP {
        &self.reporter
    }

    /// Mutable reference to the reporter.
    pub fn reporter_mut(&mut self) -> &mut REP {
        &mut self.reporter
    }

    /// The last state reported to external listeners, if any.
    pub fn last_reported_state(&self) -> Option<HvacState> {
        self.last_reported_state
    }

    /// Apply a patch to the unit and report state if it changed.
    pub async fn apply_patch(&mut self, patch: &HvacPatch) -> Result<HvacState, HvacDriverError> {
        let state = self.unit.apply_patch(patch).await?;
        self.maybe_report(&state);
        Ok(state)
    }

    /// Perform a single polling step and report state if it changed.
    pub async fn poll_step(&mut self) -> Result<HvacState, HvacDriverError> {
        let state = self.unit.poll().await?;
        self.maybe_report(&state);
        Ok(state)
    }

    /// Drain all queued commands from the receiver, coalesce them into a single patch,
    /// and apply them to the physical unit. Returns `Ok(true)` if commands were processed,
    /// `Ok(false)` if the queue was empty.
    pub async fn process_pending_commands(
        &mut self,
        receiver: &CommandReceiver<'_>,
    ) -> Result<bool, HvacDriverError> {
        let mut merged = HvacPatch::default();
        let mut has_commands = false;

        while let Ok(cmd) = receiver.try_receive() {
            merged.merge(cmd);
            has_commands = true;
        }

        if !has_commands {
            return Ok(false);
        }

        self.apply_patch(&merged).await?;
        Ok(true)
    }

    /// Run the continuous controller loop (never returns).
    ///
    /// Selects between incoming commands on `receiver` and the periodic polling timer.
    /// When a command arrives, a 50ms coalesce window allows consecutive attribute
    /// updates to merge into a single hardware transaction.
    pub async fn run(&mut self, receiver: &CommandReceiver<'_>) -> ! {
        loop {
            let poll_timer = Timer::after(Duration::from_millis(POLL_STEP_INTERVAL_MS));
            let receive_cmd = receiver.receive();

            match select(poll_timer, receive_cmd).await {
                Either::First(_) => {
                    if let Err(err) = self.poll_step().await {
                        warn!("HVAC poll error: {:?}", err);
                    }
                }
                Either::Second(first_cmd) => {
                    // Coalesce window: allow rapid burst commands to queue up
                    Timer::after(Duration::from_millis(COMMAND_COALESCE_WINDOW_MS)).await;
                    let mut merged = first_cmd;
                    while let Ok(cmd) = receiver.try_receive() {
                        merged.merge(cmd);
                    }
                    if let Err(err) = self.apply_patch(&merged).await {
                        warn!("HVAC apply_patch error: {:?}", err);
                    }
                }
            }
        }
    }

    // ========================================================================
    // Internal Helpers
    // ========================================================================

    fn maybe_report(&mut self, state: &HvacState) {
        if self.last_reported_state != Some(*state) {
            self.reporter.report_state(state);
            self.last_reported_state = Some(*state);
        }
    }
}
