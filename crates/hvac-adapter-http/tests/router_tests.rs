//! Integration and unit tests for the HTTP request router and JSON formatter.
//!
//! Follows the Arrange -> Act -> Assert pattern strictly.

use flate2::read::GzDecoder;
use hvac_adapter_http::router::{format_status_json, parse_hvac_patch_query, parse_route, Route};
use hvac_adapter_http::DASHBOARD_HTML_GZ;
use hvac_core::domain::diagnostics::{SystemDiagnostics, WifiSsid};
use hvac_core::domain::ecosystem::{
    EcosystemId, EcosystemStatus, EcosystemValue, EndpointInfo, FabricInfo, FabricLabel,
};
use hvac_core::domain::hvac::{
    FanSpeed, HvacMode, HvacPatch, HvacState, TemperatureCentiC, VaneHorizontal, VaneVertical,
};
use hvac_core::events::SystemState;
use serde_json::Value;
use std::io::Read;
use std::string::String;

#[test]
fn test_dashboard_gzip_decompresses_to_valid_html_under_50_kib() {
    // Arrange
    let mut decoder = GzDecoder::new(DASHBOARD_HTML_GZ);
    let mut html = String::new();

    // Act
    decoder
        .read_to_string(&mut html)
        .expect("Decompression failed");

    // Assert
    assert!(
        html.to_lowercase().contains("<!doctype html>"),
        "Missing DOCTYPE"
    );
    assert!(html.contains("HVAC Bridge"), "Missing HVAC Bridge title");
    assert!(html.contains("root"), "Missing root container");
    assert!(
        DASHBOARD_HTML_GZ.len() < 50 * 1024,
        "Gzip asset exceeds 50 KiB limit: {} bytes",
        DASHBOARD_HTML_GZ.len()
    );
}

#[test]
fn test_parse_route_dashboard_root() {
    // Arrange
    let request_line = "GET / HTTP/1.1\r\n";

    // Act
    let route = parse_route(request_line);

    // Assert
    assert_eq!(route, Route::Dashboard);
}

#[test]
fn test_parse_route_status() {
    // Arrange
    let req1 = "GET /api/status HTTP/1.1\r\nHost: 192.168.1.100\r\n";
    let req2 = "GET /api/status?timestamp=123456 HTTP/1.1\r\n";

    // Act
    let r1 = parse_route(req1);
    let r2 = parse_route(req2);

    // Assert
    assert_eq!(r1, Route::Status);
    assert_eq!(r2, Route::Status);
}

#[test]
fn test_parse_route_logs() {
    // Arrange
    let request_line = "GET /api/logs HTTP/1.1\r\n";

    // Act
    let route = parse_route(request_line);

    // Assert
    assert_eq!(route, Route::Logs);
}

#[test]
fn test_parse_route_action_hvac_with_query() {
    // Arrange
    let req = "POST /api/action/hvac?power=1&mode=cool&temp=2250&fan=quiet&vane_v=swing&vane_h=center HTTP/1.1\r\n";

    // Act
    let route = parse_route(req);

    // Assert
    assert_eq!(
        route,
        Route::ActionHvac(HvacPatch {
            power: Some(true),
            mode: Some(HvacMode::Cool),
            target_temp: Some(TemperatureCentiC(2250)),
            fan_speed: Some(FanSpeed::Quiet),
            vane_vertical: Some(VaneVertical::Swing),
            vane_horizontal: Some(VaneHorizontal::Center),
        })
    );
}

#[test]
fn test_parse_route_action_hvac_empty_query() {
    // Arrange
    let req = "POST /api/action/hvac HTTP/1.1\r\n";

    // Act
    let route = parse_route(req);

    // Assert
    assert_eq!(route, Route::ActionHvac(HvacPatch::default()));
}

#[test]
fn test_parse_route_ota_flash() {
    // Arrange
    let req = "POST /api/v1/ota/flash HTTP/1.1\r\nContent-Length: 1024\r\n";

    // Act
    let route = parse_route(req);

    // Assert
    assert_eq!(route, Route::OtaFlash);
}

#[test]
fn test_parse_route_action_reset_matter() {
    // Arrange
    let req = "POST /api/action/reset/matter HTTP/1.1\r\nHost: 192.168.1.150\r\n\r\n";

    // Act
    let route = parse_route(req);

    // Assert
    assert_eq!(route, Route::ActionResetMatter);
}

#[test]
fn test_parse_route_action_reboot() {
    let req = "POST /api/action/reboot HTTP/1.1\r\nHost: 192.168.1.150\r\n\r\n";
    let route = parse_route(req);
    assert_eq!(route, Route::ActionReboot);
}

#[test]
fn test_parse_route_action_reset_factory() {
    let req = "POST /api/action/reset/factory HTTP/1.1\r\nHost: 192.168.1.150\r\n\r\n";
    let route = parse_route(req);
    assert_eq!(route, Route::ActionResetFactory);
}

#[test]
fn test_parse_route_not_found_on_unknown_or_wrong_method() {
    // Arrange
    let wrong_method = "GET /api/action/hvac HTTP/1.1";
    let wrong_method_status = "POST /api/status HTTP/1.1";
    let unknown_path = "GET /api/unknown HTTP/1.1";
    let empty_req = "";
    let malformed = "NOT_A_VALID_HTTP_REQUEST";

    // Act
    let r1 = parse_route(wrong_method);
    let r2 = parse_route(wrong_method_status);
    let r3 = parse_route(unknown_path);
    let r4 = parse_route(empty_req);
    let r5 = parse_route(malformed);

    // Assert
    assert_eq!(r1, Route::NotFound);
    assert_eq!(r2, Route::NotFound);
    assert_eq!(r3, Route::NotFound);
    assert_eq!(r4, Route::NotFound);
    assert_eq!(r5, Route::NotFound);
}

#[test]
fn test_parse_hvac_patch_query_all_fields() {
    // Arrange
    let query = "power=1&mode=heat&temp=2400&fan=3&vane_v=pos2&vane_h=right_max";

    // Act
    let patch = parse_hvac_patch_query(query);

    // Assert
    assert_eq!(patch.power, Some(true));
    assert_eq!(patch.mode, Some(HvacMode::Heat));
    assert_eq!(patch.target_temp, Some(TemperatureCentiC(2400)));
    assert_eq!(patch.fan_speed, Some(FanSpeed::Speed3));
    assert_eq!(patch.vane_vertical, Some(VaneVertical::Pos2));
    assert_eq!(patch.vane_horizontal, Some(VaneHorizontal::RightMax));
}

#[test]
fn test_parse_hvac_patch_query_power_variants() {
    // Arrange & Act
    let p1 = parse_hvac_patch_query("power=1");
    let p2 = parse_hvac_patch_query("power=true");
    let p3 = parse_hvac_patch_query("power=on");
    let p4 = parse_hvac_patch_query("power=0");
    let p5 = parse_hvac_patch_query("power=false");
    let p6 = parse_hvac_patch_query("power=off");

    // Assert
    assert_eq!(p1.power, Some(true));
    assert_eq!(p2.power, Some(true));
    assert_eq!(p3.power, Some(true));
    assert_eq!(p4.power, Some(false));
    assert_eq!(p5.power, Some(false));
    assert_eq!(p6.power, Some(false));
}

#[test]
fn test_parse_hvac_patch_query_mode_variants() {
    // Arrange & Act
    let p_heat = parse_hvac_patch_query("mode=heat");
    let p_cool = parse_hvac_patch_query("mode=cool");
    let p_auto = parse_hvac_patch_query("mode=auto");
    let p_dry = parse_hvac_patch_query("mode=dry");
    let p_fan = parse_hvac_patch_query("mode=fan");
    let p_fan_only = parse_hvac_patch_query("mode=fan_only");

    // Assert
    assert_eq!(p_heat.mode, Some(HvacMode::Heat));
    assert_eq!(p_cool.mode, Some(HvacMode::Cool));
    assert_eq!(p_auto.mode, Some(HvacMode::Auto));
    assert_eq!(p_dry.mode, Some(HvacMode::Dry));
    assert_eq!(p_fan.mode, Some(HvacMode::FanOnly));
    assert_eq!(p_fan_only.mode, Some(HvacMode::FanOnly));
}

#[test]
fn test_parse_hvac_patch_query_temp_formats_and_clamping() {
    // Arrange & Act
    let p_centi = parse_hvac_patch_query("temp=2250");
    let p_decimal = parse_hvac_patch_query("temp=23.5");
    let p_too_low = parse_hvac_patch_query("temp=1200");
    let p_too_high = parse_hvac_patch_query("temp=3500");

    // Assert
    assert_eq!(p_centi.target_temp, Some(TemperatureCentiC(2250)));
    assert_eq!(p_decimal.target_temp, Some(TemperatureCentiC(2350)));
    assert_eq!(p_too_low.target_temp, Some(TemperatureCentiC::MIN_SETPOINT));
    assert_eq!(
        p_too_high.target_temp,
        Some(TemperatureCentiC::MAX_SETPOINT)
    );
}

#[test]
fn test_parse_hvac_patch_query_fan_and_vane_variants() {
    // Arrange & Act
    let p_fan_auto = parse_hvac_patch_query("fan=auto");
    let p_fan_quiet = parse_hvac_patch_query("fan=quiet");
    let p_fan_4 = parse_hvac_patch_query("fan=4");
    let p_vane_v_1 = parse_hvac_patch_query("vane_v=1");
    let p_vane_v_swing = parse_hvac_patch_query("vane_v=swing");
    let p_vane_h_split = parse_hvac_patch_query("vane_h=split");
    let p_vane_h_swing = parse_hvac_patch_query("vane_h=swing");

    // Assert
    assert_eq!(p_fan_auto.fan_speed, Some(FanSpeed::Auto));
    assert_eq!(p_fan_quiet.fan_speed, Some(FanSpeed::Quiet));
    assert_eq!(p_fan_4.fan_speed, Some(FanSpeed::Speed4));
    assert_eq!(p_vane_v_1.vane_vertical, Some(VaneVertical::Pos1));
    assert_eq!(p_vane_v_swing.vane_vertical, Some(VaneVertical::Swing));
    assert_eq!(p_vane_h_split.vane_horizontal, Some(VaneHorizontal::Split));
    assert_eq!(p_vane_h_swing.vane_horizontal, Some(VaneHorizontal::Swing));
}

#[test]
fn test_format_status_json_contains_all_hvac_and_system_fields() {
    // Arrange
    const ECOSYSTEMS: &[EcosystemId] = &[
        EcosystemId::new("Matter"),
        EcosystemId::new("HomeAssistant"),
    ];
    let hvac = HvacState {
        connected: true,
        power: true,
        mode: HvacMode::Cool,
        target_temp: TemperatureCentiC(2250),
        room_temp: Some(TemperatureCentiC(2300)),
        fan_speed: FanSpeed::Auto,
        vane_vertical: VaneVertical::Swing,
        vane_horizontal: VaneHorizontal::Auto,
        wide_vane_adjustment: true,
        operating: true,
        compressor_freq_hz: Some(42),
    };

    let diag = SystemDiagnostics {
        device_type: "hvac_bridge",
        state: SystemState::Operational,
        domain: hvac,
        uptime_sec: 3600,
        wifi_ssid: WifiSsid::new("HomeNet"),
        wifi_rssi_dbm: Some(-58),
        ip: Some([192, 168, 1, 150]),
        mac: [0x98, 0xa3, 0x16, 0xa6, 0x78, 0x44],
        active_slot: "ota_0",
        slot_valid: true,
        free_heap_bytes: 142000,
        total_heap_bytes: 215040,
        available_ecosystems: ECOSYSTEMS,
        active_ecosystem: None,
        version: "0.1.0",
        git_commit: "abc1234",
        build_date: "2026-10-17",
    };
    let mut buf = [0u8; 1024];

    // Act
    let len = format_status_json(&diag, &mut buf).expect("Format status JSON failed");
    let json_str = core::str::from_utf8(&buf[..len]).expect("Invalid UTF-8");
    let val: Value = serde_json::from_str(json_str).expect("Invalid JSON produced");

    // Assert
    assert_eq!(val["device_type"], "hvac_bridge");
    assert_eq!(val["state"], "Operational");
    assert_eq!(val["uptime_sec"], 3600);
    assert_eq!(val["wifi_ssid"], "HomeNet");
    assert_eq!(val["wifi_rssi_dbm"], -58);
    assert_eq!(val["ip"], "192.168.1.150");
    assert_eq!(val["mac"], "98:A3:16:A6:78:44");
    assert_eq!(val["active_slot"], "ota_0");
    assert_eq!(val["slot_valid"], true);
    assert_eq!(val["free_heap_bytes"], 142000);
    assert_eq!(val["total_heap_bytes"], 215040);
    assert_eq!(val["version"], "0.1.0");
    assert_eq!(val["git_commit"], "abc1234");
    assert_eq!(val["build_date"], "2026-10-17");

    // HVAC nested object assertions
    let hvac_val = &val["hvac"];
    assert_eq!(hvac_val["connected"], true);
    assert_eq!(hvac_val["power"], true);
    assert_eq!(hvac_val["mode"], "cool");
    assert_eq!(hvac_val["target_temp_c"], "22.5");
    assert_eq!(hvac_val["room_temp_c"], "23.0");
    assert_eq!(hvac_val["fan_speed"], "auto");
    assert_eq!(hvac_val["vane_vertical"], "swing");
    assert_eq!(hvac_val["vane_horizontal"], "auto");
    assert_eq!(hvac_val["wide_vane_adjustment"], true);
    assert_eq!(hvac_val["operating"], true);
    assert_eq!(hvac_val["compressor_freq_hz"], 42);
}

#[test]
fn test_format_status_json_with_none_values() {
    // Arrange
    let diag = SystemDiagnostics::<HvacState, EcosystemId> {
        device_type: "hvac_bridge",
        state: SystemState::Booting,
        domain: HvacState::default(),
        uptime_sec: 10,
        wifi_ssid: None,
        wifi_rssi_dbm: None,
        ip: None,
        mac: [0, 0, 0, 0, 0, 0],
        active_slot: "ota_1",
        slot_valid: false,
        free_heap_bytes: 50000,
        total_heap_bytes: 100000,
        available_ecosystems: &[],
        active_ecosystem: None,
        version: "0.1.0",
        git_commit: "none",
        build_date: "2026-10-17",
    };
    let mut buf = [0u8; 1024];

    // Act
    let len = format_status_json(&diag, &mut buf).expect("Format status JSON failed");
    let json_str = core::str::from_utf8(&buf[..len]).expect("Invalid UTF-8");
    let val: Value = serde_json::from_str(json_str).expect("Invalid JSON produced");

    // Assert
    assert_eq!(val["device_type"], "hvac_bridge");
    assert_eq!(val["state"], "Booting");
    assert!(val["wifi_ssid"].is_null());
    assert!(val["wifi_rssi_dbm"].is_null());
    assert!(val["ip"].is_null());
    assert!(val["hvac"]["room_temp_c"].is_null());
    assert!(val["hvac"]["compressor_freq_hz"].is_null());
    assert_eq!(val["hvac"]["connected"], false);
    assert_eq!(val["hvac"]["power"], false);
}

#[test]
fn test_format_status_json_buffer_too_small_returns_err() {
    // Arrange
    let diag = SystemDiagnostics::<HvacState, EcosystemId> {
        device_type: "hvac_bridge",
        state: SystemState::Operational,
        domain: HvacState::default(),
        uptime_sec: 10,
        wifi_ssid: None,
        wifi_rssi_dbm: None,
        ip: None,
        mac: [0, 0, 0, 0, 0, 0],
        active_slot: "ota_0",
        slot_valid: true,
        free_heap_bytes: 50000,
        total_heap_bytes: 100000,
        available_ecosystems: &[],
        active_ecosystem: None,
        version: "0.1.0",
        git_commit: "none",
        build_date: "2026-10-17",
    };
    let mut tiny_buf = [0u8; 32];

    // Act
    let result = format_status_json(&diag, &mut tiny_buf);

    // Assert
    assert_eq!(result, Err(()));
}

#[test]
fn test_format_status_json_with_multiple_fabrics_and_endpoints() {
    // Arrange
    const AVAILABLE: &[EcosystemId] = &[EcosystemId::new("Matter")];
    let active = EcosystemStatus::new(EcosystemId::new("Matter"), true)
        .with_property("thermostat_subscribed", EcosystemValue::Bool(true))
        .with_property("thermostat_endpoint", EcosystemValue::Int(1))
        .with_property("on_off_subscribed", EcosystemValue::Bool(true))
        .with_property("on_off_endpoint", EcosystemValue::Int(1))
        .with_property("fan_subscribed", EcosystemValue::Bool(false))
        .with_property("fan_endpoint", EcosystemValue::Int(2))
        .with_endpoint(EndpointInfo {
            id: 1,
            name: "Thermostat",
            cluster: "Thermostat",
            ready: true,
        })
        .with_endpoint(EndpointInfo {
            id: 1,
            name: "Power",
            cluster: "OnOff",
            ready: true,
        })
        .with_endpoint(EndpointInfo {
            id: 2,
            name: "Fan & Vane",
            cluster: "FanControl",
            ready: false,
        })
        .with_fabric(FabricInfo {
            fab_idx: 1,
            vendor_id: 4996,
            fabric_id: 0x1B8246E1309A4D12,
            node_id: 0x0000000000000002,
            label: FabricLabel::from_str("Living Room HomePod"),
        })
        .with_fabric(FabricInfo {
            fab_idx: 2,
            vendor_id: 4877,
            fabric_id: 0x4F92A0019C21EE08,
            node_id: 0x0000000000000005,
            label: FabricLabel::from_str("Home Assistant Core"),
        });

    let diag = SystemDiagnostics {
        device_type: "hvac_bridge",
        state: SystemState::Operational,
        domain: HvacState::default(),
        uptime_sec: 100,
        wifi_ssid: None,
        wifi_rssi_dbm: None,
        ip: None,
        mac: [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
        active_slot: "ota_0",
        slot_valid: true,
        free_heap_bytes: 100000,
        total_heap_bytes: 200000,
        available_ecosystems: AVAILABLE,
        active_ecosystem: Some(active),
        version: "1.0.0",
        git_commit: "abcdef",
        build_date: "2026-10-17",
    };
    let mut buf = [0u8; 2048];

    // Act
    let written = format_status_json(&diag, &mut buf).expect("serialization should succeed");
    let json_str = core::str::from_utf8(&buf[..written]).expect("valid utf-8");
    let val: Value = serde_json::from_str(json_str).expect("valid json format");

    // Assert
    let fabrics = &val["active_ecosystem"]["fabrics"];
    assert_eq!(fabrics.as_array().unwrap().len(), 2);

    assert_eq!(fabrics[0]["fab_idx"], 1);
    assert_eq!(fabrics[0]["vendor_id"], 4996);
    assert_eq!(fabrics[0]["fabric_id"], "0x1B8246E1309A4D12");
    assert_eq!(fabrics[0]["node_id"], "0x0000000000000002");
    assert_eq!(fabrics[0]["label"], "Living Room HomePod");

    assert_eq!(fabrics[1]["fab_idx"], 2);
    assert_eq!(fabrics[1]["vendor_id"], 4877);
    assert_eq!(fabrics[1]["fabric_id"], "0x4F92A0019C21EE08");
    assert_eq!(fabrics[1]["node_id"], "0x0000000000000005");
    assert_eq!(fabrics[1]["label"], "Home Assistant Core");

    let endpoints = &val["active_ecosystem"]["endpoints"];
    assert_eq!(val["active_ecosystem"]["properties"]["thermostat_subscribed"], true);
    assert_eq!(val["active_ecosystem"]["properties"]["thermostat_endpoint"], 1);
    assert_eq!(val["active_ecosystem"]["properties"]["on_off_subscribed"], true);
    assert_eq!(val["active_ecosystem"]["properties"]["on_off_endpoint"], 1);
    assert_eq!(val["active_ecosystem"]["properties"]["fan_subscribed"], false);
    assert_eq!(val["active_ecosystem"]["properties"]["fan_endpoint"], 2);
    assert_eq!(endpoints.as_array().unwrap().len(), 3);
    assert_eq!(endpoints[0]["name"], "Thermostat");
    assert_eq!(endpoints[0]["cluster"], "Thermostat");
    assert_eq!(endpoints[0]["ready"], true);
    assert_eq!(endpoints[1]["name"], "Power");
    assert_eq!(endpoints[1]["cluster"], "OnOff");
    assert_eq!(endpoints[1]["ready"], true);
    assert_eq!(endpoints[2]["name"], "Fan & Vane");
    assert_eq!(endpoints[2]["cluster"], "FanControl");
    assert_eq!(endpoints[2]["ready"], false);
}
