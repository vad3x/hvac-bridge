//! HVAC Bridge - Domain Logic
//!
//! Pure, synchronous business logic decoupled from async runtimes and I/O.

pub mod diagnostics;
pub mod ecosystem;
pub mod hvac;
pub mod reset;

pub use diagnostics::{SystemDiagnostics, WifiSsid};
pub use ecosystem::{
    EcosystemId, EcosystemStatus, EcosystemType, EcosystemValue, EndpointInfo, FabricInfo,
    FabricLabel, MAX_ECOSYSTEM_ENDPOINTS, MAX_ECOSYSTEM_FABRICS, MAX_ECOSYSTEM_PROPERTIES,
};
pub use hvac::{
    FanSpeed, HvacMode, HvacPatch, HvacState, TemperatureCentiC, VaneHorizontal, VaneVertical,
};
pub use reset::{classify_reset_hold, ResetScope};
