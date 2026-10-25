//! HVAC Bridge - Universal Core
//!
//! Domain logic, state lifecycle watch, application services, and abstract port traits.
//! Zero hardware or network stack dependencies.

#![cfg_attr(not(test), no_std)]
#![allow(async_fn_in_trait)]

pub mod app;
pub mod config;
pub mod domain;
pub mod events;
pub mod ports;
pub mod service;

pub use app::App;
pub use domain::{
    classify_reset_hold, EcosystemId, EcosystemStatus, EcosystemType, EcosystemValue, EndpointInfo,
    FabricInfo, FabricLabel, FanSpeed, HvacMode, HvacPatch, HvacState, ResetScope,
    SystemDiagnostics, TemperatureCentiC, VaneHorizontal, VaneVertical, WifiSsid,
    MAX_ECOSYSTEM_ENDPOINTS, MAX_ECOSYSTEM_FABRICS, MAX_ECOSYSTEM_PROPERTIES,
};
pub use events::{
    HvacStateReceiver, HvacStateSender, HvacStateWatch, SystemState, SystemStateReceiver,
    SystemStateSender, SystemStateWatch,
};
pub use ports::{
    CommandChannel, CommandReceiver, CommandSender, EcosystemInfoDriver, HardwareInfoDriver,
    HvacControllerHandle, HvacDriverError, HvacReporter, HvacUnitDriver, OtaDriverError,
    OtaFlashDriver, OtaStream, QueueFullError, ResetTriggerDriver, SystemResetDriver,
};
pub use service::{DiagnosticsService, HvacController, ResetService};
