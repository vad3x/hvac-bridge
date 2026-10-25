//! HVAC Bridge - Matter Protocol Adapter
//!
//! Exposes the CN105 HVAC Bridge as a Matter-over-WiFi Room Air Conditioner (`0x0072`, rev 2)
//! on Endpoint 1 with composed clusters:
//! - `HvacOnOff` (`0x0006`, rev 6)
//! - `HvacThermostat` (`0x0201`, rev 7)
//! - `HvacFanControl` (`0x0202`, rev 5)
//!
//! Provides `MatterController` builder and runner, `PartitionedWifiStore` flash persistence,
//! and `MatterHvacReporter` outbound state bridge.

#![no_std]
#![allow(async_fn_in_trait)]

extern crate alloc;

pub mod commissioning;
pub mod config;
pub mod controller;
pub mod fan_control;
pub mod on_off;
pub mod readiness;
pub mod reporter;
pub mod thermostat;

use rs_matter::dm::{AsyncHandler, Cluster, DeviceType};

// Re-exports
pub use commissioning::{comm_data_from_mac, derive_hvac_identity};
pub use config::{
    DEV_TYPE_ROOM_AIR_CONDITIONER, FAN_ENDPOINT_ID, MATTER_ECOSYSTEM, MATTER_KVS_RANGE,
    MAX_SUBSCRIPTION_INTERVAL_SECS, ON_OFF_ENDPOINT_ID, THERMOSTAT_ENDPOINT_ID, WIFI_KVS_RANGE,
};
pub use controller::{
    has_active_controller_fabric, DeviceInfo, MatterController, MatterControllerConfig,
    MatterPeripherals, PartitionedWifiStore, APPLE_HOME_HUB_VENDOR_ID, BUMP_SIZE, PID, VID,
};
#[cfg(target_arch = "riscv32")]
pub use controller::{create_kv_blob_store, ConnectionMonitor};

pub use fan_control::{HvacFanControl, HvacFanControlConfig};
pub use on_off::{HvacOnOff, HvacOnOffConfig};
pub use readiness::ClusterReadinessBarrier;
pub use reporter::MatterHvacReporter;
pub use thermostat::{HvacThermostat, HvacThermostatConfig};

// ============================================================================
// MatterCluster Trait
// ============================================================================

/// Trait for Matter cluster handlers registered with [`MatterController`].
///
/// Bridges cluster handlers and `MatterController::register()`.
/// Supplies cluster metadata, endpoint ID, and the application device type so `register()`
/// can automatically build the handler chain and node descriptor without leaking
/// low-level `rs-matter` types into application composition roots.
pub trait MatterCluster: AsyncHandler {
    /// The Matter endpoint ID this cluster is hosted on.
    const ENDPOINT_ID: u16;

    /// The cluster definition (attributes, commands, revision, features).
    const CLUSTER_DEF: Cluster<'static>;

    /// The Matter device type for the endpoint hosting this cluster.
    const DEVICE_TYPE: DeviceType;
}
