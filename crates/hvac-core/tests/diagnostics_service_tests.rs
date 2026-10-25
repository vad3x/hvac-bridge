//! Integration tests for DiagnosticsService.
//!
//! Follows AAA (Arrange, Act, Assert) pattern strictly.

use hvac_core::domain::diagnostics::{SystemDiagnostics, WifiSsid};
use hvac_core::domain::ecosystem::{EcosystemId, EcosystemStatus, EcosystemType, EcosystemValue};
use hvac_core::domain::hvac::{HvacMode, HvacState, TemperatureCentiC};
use hvac_core::events::{
    HvacStateSender, HvacStateWatch, SystemState, SystemStateSender, SystemStateWatch,
};
use hvac_core::ports::diagnostics::HardwareInfoDriver;
use hvac_core::ports::ecosystem::EcosystemInfoDriver;
use hvac_core::service::DiagnosticsService;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestEcosystem {
    Matter,
    HomeAssistant,
}

impl EcosystemType for TestEcosystem {
    fn id(&self) -> EcosystemId {
        match self {
            Self::Matter => EcosystemId::new("Matter"),
            Self::HomeAssistant => EcosystemId::new("HomeAssistant"),
        }
    }
}

const TEST_AVAILABLE_ECOSYSTEMS: &[TestEcosystem] =
    &[TestEcosystem::Matter, TestEcosystem::HomeAssistant];

pub struct MockHardwareInfoDriver {
    pub device_type: &'static str,
    pub uptime_secs: u64,
    pub free_heap_bytes: usize,
    pub total_heap_bytes: usize,
    pub wifi_ssid: Option<WifiSsid>,
    pub wifi_rssi: Option<i8>,
    pub ip_address: Option<[u8; 4]>,
    pub mac_address: [u8; 6],
    pub active_slot: &'static str,
    pub is_slot_valid: bool,
    pub firmware_version: &'static str,
    pub git_commit: &'static str,
    pub build_date: &'static str,
}

impl MockHardwareInfoDriver {
    pub fn new() -> Self {
        Self {
            device_type: "hvac_bridge",
            uptime_secs: 12345,
            free_heap_bytes: 45000,
            total_heap_bytes: 80000,
            wifi_ssid: WifiSsid::new("HomeNet-HVAC"),
            wifi_rssi: Some(-62),
            ip_address: Some([192, 168, 1, 100]),
            mac_address: [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc],
            active_slot: "ota_0",
            is_slot_valid: true,
            firmware_version: "0.1.0",
            git_commit: "abcdef0",
            build_date: "2026-10-17",
        }
    }
}

impl Default for MockHardwareInfoDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl HardwareInfoDriver for MockHardwareInfoDriver {
    fn device_type(&self) -> &'static str {
        self.device_type
    }

    fn uptime_secs(&self) -> u64 {
        self.uptime_secs
    }

    fn free_heap_bytes(&self) -> usize {
        self.free_heap_bytes
    }

    fn total_heap_bytes(&self) -> usize {
        self.total_heap_bytes
    }

    fn wifi_ssid(&self) -> Option<WifiSsid> {
        self.wifi_ssid
    }

    fn wifi_rssi(&self) -> Option<i8> {
        self.wifi_rssi
    }

    fn ip_address(&self) -> Option<[u8; 4]> {
        self.ip_address
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac_address
    }

    fn active_slot(&self) -> &'static str {
        self.active_slot
    }

    fn is_slot_valid(&self) -> bool {
        self.is_slot_valid
    }

    fn firmware_version(&self) -> &'static str {
        self.firmware_version
    }

    fn git_commit(&self) -> &'static str {
        self.git_commit
    }

    fn build_date(&self) -> &'static str {
        self.build_date
    }
}

pub struct MockEcosystemInfoDriver {
    pub status: Option<EcosystemStatus>,
}

impl MockEcosystemInfoDriver {
    pub fn new(status: Option<EcosystemStatus>) -> Self {
        Self { status }
    }
}

impl EcosystemInfoDriver for MockEcosystemInfoDriver {
    fn status(&self) -> Option<EcosystemStatus> {
        self.status
    }
}

#[test]
fn diagnostics_service_snapshot_with_default_ecosystem_and_live_hvac() {
    // Arrange
    let hardware = MockHardwareInfoDriver::new();
    let state_watch = SystemStateWatch::new();
    let state_sender = SystemStateSender::new(&state_watch);
    state_sender.operational();

    let hvac_watch = HvacStateWatch::new();
    let hvac_sender = HvacStateSender::new(&hvac_watch);
    let test_hvac_state = HvacState {
        connected: true,
        power: true,
        mode: HvacMode::Cool,
        target_temp: TemperatureCentiC(2250),
        room_temp: Some(TemperatureCentiC(2300)),
        ..Default::default()
    };
    hvac_sender.send(test_hvac_state);

    let service = DiagnosticsService::new(&hardware, &state_watch, &hvac_watch);

    // Act
    let snapshot = service.snapshot();

    // Assert
    assert_eq!(
        snapshot,
        SystemDiagnostics {
            device_type: "hvac_bridge",
            state: SystemState::Operational,
            domain: test_hvac_state,
            uptime_sec: 12345,
            wifi_ssid: WifiSsid::new("HomeNet-HVAC"),
            wifi_rssi_dbm: Some(-62),
            ip: Some([192, 168, 1, 100]),
            mac: [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc],
            active_slot: "ota_0",
            slot_valid: true,
            free_heap_bytes: 45000,
            total_heap_bytes: 80000,
            available_ecosystems: &[],
            active_ecosystem: None,
            version: "0.1.0",
            git_commit: "abcdef0",
            build_date: "2026-10-17",
        }
    );
}

#[test]
fn diagnostics_service_snapshot_with_explicit_ecosystem() {
    // Arrange
    let hardware = MockHardwareInfoDriver::new();
    let expected_status = EcosystemStatus::new(TestEcosystem::Matter.id(), true)
        .with_property("thermostat_subscribed", EcosystemValue::Bool(true))
        .with_property("fabrics", EcosystemValue::Int(2));
    let ecosystem = MockEcosystemInfoDriver::new(Some(expected_status));

    let state_watch = SystemStateWatch::new();
    let state_sender = SystemStateSender::new(&state_watch);
    state_sender.ecosystem_connecting();

    let hvac_watch = HvacStateWatch::new();
    let service = DiagnosticsService::with_ecosystem(
        &hardware,
        &ecosystem,
        TEST_AVAILABLE_ECOSYSTEMS,
        &state_watch,
        &hvac_watch,
    );

    // Act
    let snapshot = service.snapshot();

    // Assert
    assert_eq!(snapshot.state, SystemState::EcosystemConnecting);
    assert_eq!(snapshot.available_ecosystems, TEST_AVAILABLE_ECOSYSTEMS);
    assert_eq!(snapshot.active_ecosystem, Some(expected_status));
    assert_eq!(snapshot.domain, HvacState::default());
}
