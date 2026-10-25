//! HVAC Bridge - Matter Cluster Readiness Barrier
//!
//! Coordinates Apple Home / Matter subscription priming for HVAC clusters on Endpoint 1,
//! and implements `EcosystemInfoDriver` to report
//! Matter pairing and cluster subscription status.

use core::sync::atomic::{AtomicBool, Ordering};
use edf_matter::readiness::ClusterReadinessBarrier as SharedReadinessBarrier;
use hvac_core::domain::{EcosystemStatus, EcosystemValue, EndpointInfo, FabricInfo, FabricLabel};
use hvac_core::events::SystemStateSender;
use hvac_core::ports::EcosystemInfoDriver;
use log::info;
use portable_atomic::AtomicPtr;

use crate::config::{
    FAN_ENDPOINT_ID, MATTER_ECOSYSTEM, ON_OFF_ENDPOINT_ID, THERMOSTAT_ENDPOINT_ID,
};

// ============================================================================
// Public Structs & Implementation
// ============================================================================

/// Synchronization barrier coordinating Matter pairing and multi-endpoint cluster readiness.
///
/// Prevents premature transitions to `SystemState::Operational` until controllers (such as
/// Apple Home Hubs) have completed the required HVAC attribute subscription reads.
pub struct ClusterReadinessBarrier {
    publication: SharedReadinessBarrier<'static>,
    on_off_ready: AtomicBool,
    thermostat_ready: AtomicBool,
    fan_control_ready: AtomicBool,
    fan_required: AtomicBool,
    matter: AtomicPtr<rs_matter::Matter<'static>>,
}

impl ClusterReadinessBarrier {
    /// Create a new cluster readiness barrier.
    pub const fn new(state_sender: SystemStateSender<'static>) -> Self {
        Self {
            publication: SharedReadinessBarrier::new(1, state_sender),
            on_off_ready: AtomicBool::new(false),
            thermostat_ready: AtomicBool::new(false),
            fan_control_ready: AtomicBool::new(false),
            fan_required: AtomicBool::new(false),
            matter: AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    /// Attach the running Matter protocol stack instance.
    pub fn attach_matter(&self, matter: &'static rs_matter::Matter<'static>) {
        self.matter
            .store(matter as *const _ as *mut _, Ordering::Release);
    }

    /// Configure whether Fan Control cluster readiness is required before transitioning to Operational.
    pub fn set_fan_required(&self, required: bool) {
        self.fan_required.store(required, Ordering::Release);
    }

    /// Update whether the Matter node is commissioned with an active controller fabric.
    pub fn set_paired(&self, paired: bool) {
        self.publication.set_paired(paired);
    }

    /// Returns true if the Matter node is commissioned with an active controller fabric.
    pub fn is_paired(&self) -> bool {
        self.publication.is_paired()
    }

    /// Mark the On/Off cluster (Endpoint 1) as subscription-primed.
    pub fn mark_on_off_ready(&self) {
        let was_ready = self.is_ready();
        self.on_off_ready.store(true, Ordering::Release);
        self.publish_readiness_transition(was_ready);
    }

    /// Mark the Thermostat cluster (Endpoint 1) as subscription-primed.
    pub fn mark_thermostat_ready(&self) {
        let was_ready = self.is_ready();
        self.thermostat_ready.store(true, Ordering::Release);
        self.publish_readiness_transition(was_ready);
    }

    /// Mark the Fan Control cluster (Endpoint 1) as subscription-primed.
    pub fn mark_fan_control_ready(&self) {
        let was_ready = self.is_ready();
        self.fan_control_ready.store(true, Ordering::Release);
        self.publish_readiness_transition(was_ready);
    }

    fn publish_readiness_transition(&self, was_ready: bool) {
        if !was_ready && self.is_ready() {
            info!("Matter clusters ready -> publishing Operational state");
            // The shared mask is immutable; HVAC permits changing fan_required without
            // publishing. Treat each local false-to-true mark as one complete shared
            // readiness cycle, including transitions after a fan configuration change.
            self.publication.reset();
            self.publication.mark_ready(1);
        }
    }

    /// Returns true if the On/Off cluster has been subscription-primed.
    pub fn is_on_off_ready(&self) -> bool {
        self.on_off_ready.load(Ordering::Acquire)
    }

    /// Returns true if the Thermostat cluster has been subscription-primed.
    pub fn is_thermostat_ready(&self) -> bool {
        self.thermostat_ready.load(Ordering::Acquire)
    }

    /// Returns true if the Fan Control cluster has been subscription-primed.
    pub fn is_fan_control_ready(&self) -> bool {
        self.fan_control_ready.load(Ordering::Acquire)
    }

    /// Reset cluster subscription readiness state on connection loss or reconnect.
    pub fn reset(&self) {
        self.on_off_ready.store(false, Ordering::Release);
        self.thermostat_ready.store(false, Ordering::Release);
        self.fan_control_ready.store(false, Ordering::Release);
        self.publication.reset();
    }

    /// Returns true if required clusters have served initial subscription reads.
    pub fn is_ready(&self) -> bool {
        if self.fan_required.load(Ordering::Acquire) {
            self.is_thermostat_ready() && self.is_fan_control_ready()
        } else {
            self.is_thermostat_ready()
        }
    }
}

impl EcosystemInfoDriver for ClusterReadinessBarrier {
    fn status(&self) -> Option<EcosystemStatus> {
        let mut status = EcosystemStatus::new(MATTER_ECOSYSTEM, self.is_paired())
            .with_endpoint(EndpointInfo {
                id: THERMOSTAT_ENDPOINT_ID,
                name: "Thermostat",
                cluster: "Thermostat",
                ready: self.is_thermostat_ready(),
            })
            .with_endpoint(EndpointInfo {
                id: ON_OFF_ENDPOINT_ID,
                name: "Power",
                cluster: "OnOff",
                ready: self.is_on_off_ready(),
            });

        if self.fan_required.load(Ordering::Acquire) || self.is_fan_control_ready() {
            status = status.with_endpoint(EndpointInfo {
                id: FAN_ENDPOINT_ID,
                name: "Fan & Vane",
                cluster: "FanControl",
                ready: self.is_fan_control_ready(),
            });
        }

        status = status
            .with_property(
                "thermostat_subscribed",
                EcosystemValue::Bool(self.is_thermostat_ready()),
            )
            .with_property(
                "thermostat_endpoint",
                EcosystemValue::Int(THERMOSTAT_ENDPOINT_ID as i64),
            );
        if self.is_on_off_ready() {
            status = status
                .with_property(
                    "on_off_subscribed",
                    EcosystemValue::Bool(self.is_on_off_ready()),
                )
                .with_property(
                    "on_off_endpoint",
                    EcosystemValue::Int(ON_OFF_ENDPOINT_ID as i64),
                );
        }
        if self.fan_required.load(Ordering::Acquire) || self.is_fan_control_ready() {
            status = status
                .with_property(
                    "fan_subscribed",
                    EcosystemValue::Bool(self.is_fan_control_ready()),
                )
                .with_property(
                    "fan_endpoint",
                    EcosystemValue::Int(FAN_ENDPOINT_ID as i64),
                );
        }

        let matter_ptr = self.matter.load(Ordering::Acquire);
        if !matter_ptr.is_null() {
            let matter = unsafe { &*matter_ptr };
            matter.with_state(|state| {
                for fab in state.fabrics.iter() {
                    status = status.with_fabric(FabricInfo {
                        fab_idx: fab.fab_idx().get(),
                        vendor_id: fab.vendor_id(),
                        fabric_id: fab.fabric_id(),
                        node_id: fab.node_id(),
                        label: FabricLabel::from_str(fab.label()),
                    });
                }
            });
        }

        Some(status)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use hvac_core::events::{SystemState, SystemStateWatch};

    #[test]
    fn thermostat_alone_gates_default_readiness_and_on_off_is_diagnostic() {
        static WATCH: SystemStateWatch = SystemStateWatch::new();
        let sender = SystemStateSender::new(&WATCH);
        let barrier = ClusterReadinessBarrier::new(sender);
        barrier.mark_on_off_ready();
        assert!(!barrier.is_ready());
        assert_eq!(sender.current_state(), None);
        barrier.reset();
        barrier.mark_thermostat_ready();
        assert!(barrier.is_ready());
        assert!(!barrier.is_on_off_ready());
        assert_eq!(sender.current_state(), Some(SystemState::Operational));
        sender.ecosystem_connecting();
        barrier.mark_on_off_ready();
        barrier.mark_thermostat_ready();
        barrier.mark_fan_control_ready();
        assert_eq!(
            sender.current_state(),
            Some(SystemState::EcosystemConnecting)
        );
    }

    #[test]
    fn fan_configuration_is_silent_and_rearms_readiness_publication() {
        static WATCH: SystemStateWatch = SystemStateWatch::new();
        let sender = SystemStateSender::new(&WATCH);
        let barrier = ClusterReadinessBarrier::new(sender);
        barrier.mark_thermostat_ready();
        sender.ecosystem_connecting();
        barrier.set_fan_required(true);
        assert!(!barrier.is_ready());
        barrier.mark_on_off_ready();
        assert_eq!(
            sender.current_state(),
            Some(SystemState::EcosystemConnecting)
        );
        barrier.mark_fan_control_ready();
        assert_eq!(sender.current_state(), Some(SystemState::Operational));

        barrier.reset();
        sender.ecosystem_connecting();
        barrier.mark_thermostat_ready();
        barrier.set_fan_required(false);
        assert!(barrier.is_ready());
        assert_eq!(
            sender.current_state(),
            Some(SystemState::EcosystemConnecting)
        );
        barrier.mark_on_off_ready();
        barrier.mark_thermostat_ready();
        assert_eq!(
            sender.current_state(),
            Some(SystemState::EcosystemConnecting)
        );
        barrier.set_fan_required(true);
        barrier.mark_fan_control_ready();
        assert_eq!(sender.current_state(), Some(SystemState::Operational));
    }

    #[test]
    fn diagnostics_keep_hvac_endpoint_names_and_optional_properties() {
        static WATCH: SystemStateWatch = SystemStateWatch::new();
        let barrier = ClusterReadinessBarrier::new(SystemStateSender::new(&WATCH));
        let status = barrier.status().unwrap();
        assert_eq!(status.endpoints[0].as_ref().unwrap().name, "Thermostat");
        assert_eq!(status.endpoints[1].as_ref().unwrap().name, "Power");
        assert_eq!(status.property("on_off_subscribed"), None);
        assert_eq!(status.property("fan_subscribed"), None);
        barrier.set_fan_required(true);
        let status = barrier.status().unwrap();
        assert_eq!(status.endpoints[2].as_ref().unwrap().name, "Fan & Vane");
        assert_eq!(
            status.property("fan_subscribed"),
            Some(&EcosystemValue::Bool(false))
        );
    }

    #[test]
    fn readiness_barrier_initial_state() {
        // Arrange
        static WATCH: SystemStateWatch = SystemStateWatch::new();
        let sender = SystemStateSender::new(&WATCH);
        let barrier = ClusterReadinessBarrier::new(sender);

        // Act & Assert
        assert!(!barrier.is_paired());
        assert!(!barrier.is_thermostat_ready());
        assert!(!barrier.is_fan_control_ready());
        assert!(!barrier.is_ready());
        assert_eq!(sender.current_state(), None);
    }

    #[test]
    fn readiness_barrier_marks_operational_when_both_clusters_ready() {
        // Arrange
        static WATCH: SystemStateWatch = SystemStateWatch::new();
        let sender = SystemStateSender::new(&WATCH);
        let barrier = ClusterReadinessBarrier::new(sender);
        barrier.set_fan_required(true);

        // Act - mark thermostat only
        barrier.mark_thermostat_ready();

        // Assert
        assert!(barrier.is_thermostat_ready());
        assert!(!barrier.is_fan_control_ready());
        assert!(!barrier.is_ready());
        assert_eq!(sender.current_state(), None);

        // Act - mark fan control
        barrier.mark_fan_control_ready();

        // Assert
        assert!(barrier.is_thermostat_ready());
        assert!(barrier.is_fan_control_ready());
        assert!(barrier.is_ready());
        assert_eq!(sender.current_state(), Some(SystemState::Operational));
    }

    #[test]
    fn readiness_barrier_reset_clears_clusters_but_preserves_paired() {
        // Arrange
        static WATCH: SystemStateWatch = SystemStateWatch::new();
        let sender = SystemStateSender::new(&WATCH);
        let barrier = ClusterReadinessBarrier::new(sender);
        barrier.set_paired(true);
        barrier.mark_thermostat_ready();
        barrier.mark_fan_control_ready();

        // Act
        barrier.reset();

        // Assert
        assert!(barrier.is_paired());
        assert!(!barrier.is_thermostat_ready());
        assert!(!barrier.is_fan_control_ready());
        assert!(!barrier.is_ready());
    }

    #[test]
    fn readiness_barrier_ecosystem_status_reflects_live_state() {
        // Arrange
        static WATCH: SystemStateWatch = SystemStateWatch::new();
        let sender = SystemStateSender::new(&WATCH);
        let barrier = ClusterReadinessBarrier::new(sender);
        barrier.set_fan_required(true);

        // Act
        barrier.set_paired(true);
        barrier.mark_thermostat_ready();
        let status = barrier.status().unwrap();

        // Assert
        assert_eq!(status.ecosystem_type, MATTER_ECOSYSTEM);
        assert!(status.paired);
        assert_eq!(
            status.property("thermostat_subscribed"),
            Some(&EcosystemValue::Bool(true))
        );
        assert_eq!(
            status.property("fan_subscribed"),
            Some(&EcosystemValue::Bool(false))
        );
    }

    #[test]
    fn readiness_barrier_deduplicates_repeated_marks() {
        // Arrange
        static WATCH: SystemStateWatch = SystemStateWatch::new();
        let sender = SystemStateSender::new(&WATCH);
        let barrier = ClusterReadinessBarrier::new(sender);
        barrier.mark_thermostat_ready();
        barrier.mark_fan_control_ready();
        assert_eq!(sender.current_state(), Some(SystemState::Operational));

        // Act - repeated calls when already ready
        barrier.mark_thermostat_ready();
        barrier.mark_fan_control_ready();

        // Assert
        assert!(barrier.is_ready());
        assert_eq!(sender.current_state(), Some(SystemState::Operational));
    }

    #[test]
    fn readiness_barrier_rearms_after_reset() {
        // Arrange
        static WATCH: SystemStateWatch = SystemStateWatch::new();
        let sender = SystemStateSender::new(&WATCH);
        let barrier = ClusterReadinessBarrier::new(sender);
        barrier.set_fan_required(true);
        barrier.mark_thermostat_ready();
        barrier.mark_fan_control_ready();
        assert_eq!(sender.current_state(), Some(SystemState::Operational));

        // Act - reset clears readiness
        barrier.reset();
        sender.ecosystem_connecting();
        assert_eq!(
            sender.current_state(),
            Some(SystemState::EcosystemConnecting)
        );
        assert!(!barrier.is_ready());

        // Marking thermostat only does not transition yet
        barrier.mark_thermostat_ready();
        assert_eq!(
            sender.current_state(),
            Some(SystemState::EcosystemConnecting)
        );

        // Marking fan completes readiness and re-arms Operational
        barrier.mark_fan_control_ready();

        // Assert
        assert!(barrier.is_ready());
        assert_eq!(sender.current_state(), Some(SystemState::Operational));
    }
}
