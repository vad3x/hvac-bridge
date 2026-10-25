//! HVAC Bridge - Diagnostics Application Service
//!
//! Produces point-in-time system diagnostics snapshots combining hardware telemetry,
//! system lifecycle state, live HVAC unit state, and active ecosystem status.

use crate::domain::diagnostics::SystemDiagnostics;
use crate::domain::ecosystem::{EcosystemId, EcosystemType};
use crate::domain::hvac::HvacState;
use crate::events::{HvacStateWatch, SystemState, SystemStateWatch};
use crate::ports::diagnostics::HardwareInfoDriver;
use crate::ports::ecosystem::EcosystemInfoDriver;

// ============================================================================
// DiagnosticsService
// ============================================================================

/// Service responsible for aggregating system telemetry into point-in-time snapshots.
pub struct DiagnosticsService<
    'a,
    H: HardwareInfoDriver,
    E: EcosystemInfoDriver = (),
    T: EcosystemType = EcosystemId,
> {
    hardware: &'a H,
    ecosystem: &'a E,
    available_ecosystems: &'static [T],
    state_watch: &'a SystemStateWatch,
    hvac_watch: &'a HvacStateWatch,
}

impl<'a, H: HardwareInfoDriver> DiagnosticsService<'a, H, (), EcosystemId> {
    /// Create a new DiagnosticsService instance without a dedicated smart home ecosystem driver.
    pub fn new(
        hardware: &'a H,
        state_watch: &'a SystemStateWatch,
        hvac_watch: &'a HvacStateWatch,
    ) -> Self {
        Self {
            hardware,
            ecosystem: &(),
            available_ecosystems: &[],
            state_watch,
            hvac_watch,
        }
    }
}

impl<'a, H: HardwareInfoDriver, E: EcosystemInfoDriver, T: EcosystemType>
    DiagnosticsService<'a, H, E, T>
{
    /// Create a new DiagnosticsService instance with a dedicated smart home ecosystem driver
    /// and a slice of available ecosystems provided by the composition root.
    pub fn with_ecosystem(
        hardware: &'a H,
        ecosystem: &'a E,
        available_ecosystems: &'static [T],
        state_watch: &'a SystemStateWatch,
        hvac_watch: &'a HvacStateWatch,
    ) -> Self {
        Self {
            hardware,
            ecosystem,
            available_ecosystems,
            state_watch,
            hvac_watch,
        }
    }

    /// Aggregate current domain state, ecosystem status, and hardware telemetry into a snapshot.
    pub fn snapshot(&self) -> SystemDiagnostics<HvacState, T> {
        let state = self.state_watch.try_get().unwrap_or(SystemState::Booting);
        let hvac = self.hvac_watch.try_get().unwrap_or_default();

        SystemDiagnostics {
            device_type: self.hardware.device_type(),
            state,
            domain: hvac,
            uptime_sec: self.hardware.uptime_secs(),
            wifi_ssid: self.hardware.wifi_ssid(),
            wifi_rssi_dbm: self.hardware.wifi_rssi(),
            ip: self.hardware.ip_address(),
            mac: self.hardware.mac_address(),
            active_slot: self.hardware.active_slot(),
            slot_valid: self.hardware.is_slot_valid(),
            free_heap_bytes: self.hardware.free_heap_bytes(),
            total_heap_bytes: self.hardware.total_heap_bytes(),
            available_ecosystems: self.available_ecosystems,
            active_ecosystem: self.ecosystem.status(),
            version: self.hardware.firmware_version(),
            git_commit: self.hardware.git_commit(),
            build_date: self.hardware.build_date(),
        }
    }

    /// Reference to the underlying hardware info driver.
    pub fn hardware(&self) -> &H {
        self.hardware
    }

    /// Reference to the underlying ecosystem info driver.
    pub fn ecosystem(&self) -> &E {
        self.ecosystem
    }

    /// Reference to the system state watch signal.
    pub fn state_watch(&self) -> &SystemStateWatch {
        self.state_watch
    }

    /// Reference to the HVAC state watch signal.
    pub fn hvac_watch(&self) -> &HvacStateWatch {
        self.hvac_watch
    }
}
