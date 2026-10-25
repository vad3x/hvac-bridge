//! Compatibility re-exports for shared diagnostics contracts.

pub use edf_core::domain::diagnostics::{SystemDiagnostics, WifiSsid};

#[cfg(test)]
mod tests {
    use super::SystemDiagnostics;
    use crate::domain::hvac::HvacState;
    use crate::events::SystemState;

    #[test]
    fn diagnostics_contract_keeps_hvac_as_generic_domain_payload() {
        let hvac = HvacState::default();
        let diagnostics = SystemDiagnostics::<HvacState> {
            device_type: "hvac_bridge",
            state: SystemState::Operational,
            domain: hvac,
            uptime_sec: 0,
            wifi_ssid: None,
            wifi_rssi_dbm: None,
            ip: None,
            mac: [0; 6],
            active_slot: "ota_0",
            slot_valid: true,
            free_heap_bytes: 0,
            total_heap_bytes: 0,
            available_ecosystems: &[],
            active_ecosystem: None,
            version: "0.1.0",
            git_commit: "test",
            build_date: "test",
        };

        assert_eq!(diagnostics.domain, hvac);
    }
}
