//! Integration tests for async `HttpServer` with injected core services.
//!
//! Follows the Arrange -> Act -> Assert pattern strictly.

use flate2::read::GzDecoder;
use futures::executor::block_on;
use hvac_adapter_http::{HttpServer, HttpServerConfig, DASHBOARD_HTML_GZ};
use hvac_core::domain::diagnostics::WifiSsid;
use hvac_core::domain::ecosystem::EcosystemId;
use hvac_core::domain::hvac::{
    FanSpeed, HvacMode, HvacPatch, HvacState, TemperatureCentiC, VaneHorizontal, VaneVertical,
};
use hvac_core::events::{HvacStateWatch, SystemState, SystemStateWatch};
use hvac_core::ports::diagnostics::HardwareInfoDriver;
use hvac_core::ports::inbound::{CommandChannel, HvacControllerHandle};
use hvac_core::ports::ota::{OtaDriverError, OtaFlashDriver, OtaStream};
use hvac_core::service::DiagnosticsService;
use serde_json::Value;
use std::io::Read;
use std::string::String;
use std::vec::Vec;

// ============================================================================
// Mock Stream Implementation
// ============================================================================

struct MockStream {
    read_data: Vec<u8>,
    read_pos: usize,
    written_data: Vec<u8>,
}

impl MockStream {
    fn new(input: &[u8]) -> Self {
        Self {
            read_data: input.to_vec(),
            read_pos: 0,
            written_data: Vec::new(),
        }
    }

    fn written_bytes(&self) -> &[u8] {
        &self.written_data
    }

    fn written_str(&self) -> &str {
        core::str::from_utf8(&self.written_data).expect("invalid utf-8 in written response")
    }
}

impl OtaStream for MockStream {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        if self.read_pos >= self.read_data.len() {
            return Ok(0);
        }
        let available = self.read_data.len() - self.read_pos;
        let to_copy = core::cmp::min(available, buf.len());
        buf[..to_copy].copy_from_slice(&self.read_data[self.read_pos..self.read_pos + to_copy]);
        self.read_pos += to_copy;
        Ok(to_copy)
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<(), ()> {
        self.written_data.extend_from_slice(buf);
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

// ============================================================================
// Mock Port Drivers
// ============================================================================

struct MockHardwareInfo;

impl HardwareInfoDriver for MockHardwareInfo {
    fn device_type(&self) -> &'static str {
        "hvac_bridge"
    }

    fn uptime_secs(&self) -> u64 {
        1234
    }

    fn free_heap_bytes(&self) -> usize {
        120_000
    }

    fn total_heap_bytes(&self) -> usize {
        320_000
    }

    fn wifi_ssid(&self) -> Option<WifiSsid> {
        WifiSsid::new("TestNet")
    }

    fn wifi_rssi(&self) -> Option<i8> {
        Some(-55)
    }

    fn ip_address(&self) -> Option<[u8; 4]> {
        Some([192, 168, 1, 142])
    }

    fn mac_address(&self) -> [u8; 6] {
        [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]
    }

    fn active_slot(&self) -> &'static str {
        "ota_0"
    }

    fn is_slot_valid(&self) -> bool {
        true
    }

    fn firmware_version(&self) -> &'static str {
        "1.2.3"
    }

    fn git_commit(&self) -> &'static str {
        "deadbeef"
    }

    fn build_date(&self) -> &'static str {
        "2026-10-17"
    }
}

struct MockOtaFlashDriver {
    should_fail: bool,
    received_payload: Vec<u8>,
    call_count: usize,
}

impl MockOtaFlashDriver {
    fn new(should_fail: bool) -> Self {
        Self {
            should_fail,
            received_payload: Vec::new(),
            call_count: 0,
        }
    }
}

impl OtaFlashDriver for MockOtaFlashDriver {
    async fn handle_flash_stream<S: OtaStream>(
        &mut self,
        stream: &mut S,
        content_length: usize,
    ) -> Result<(), OtaDriverError> {
        self.call_count += 1;
        let mut buf = [0u8; 512];
        let mut total = 0;
        while total < content_length {
            let to_read = core::cmp::min(buf.len(), content_length - total);
            let n = stream
                .read(&mut buf[..to_read])
                .await
                .map_err(|_| OtaDriverError::TruncatedStream)?;
            if n == 0 {
                break;
            }
            self.received_payload.extend_from_slice(&buf[..n]);
            total += n;
        }

        if self.should_fail {
            Err(OtaDriverError::VerificationFailed)
        } else {
            Ok(())
        }
    }
}

fn test_log_provider(buf: &mut [u8]) -> usize {
    let msg = b"00:01:23.456 INFO  hvac - bridge running smoothly\n";
    let len = msg.len().min(buf.len());
    buf[..len].copy_from_slice(&msg[..len]);
    len
}

// ============================================================================
// Server Tests
// ============================================================================

#[test]
fn test_get_dashboard_returns_gzipped_html() {
    // Arrange
    let cmd_channel = CommandChannel::new();
    let hvac_handle = HvacControllerHandle::new(cmd_channel.sender());
    let hardware = MockHardwareInfo;
    let state_watch = SystemStateWatch::new();
    state_watch.sender().send(SystemState::Operational);
    let hvac_watch = HvacStateWatch::new();
    hvac_watch.sender().send(HvacState::default());
    let diagnostics = DiagnosticsService::new(&hardware, &state_watch, &hvac_watch);
    let mut ota_driver = MockOtaFlashDriver::new(false);
    let mut server = HttpServer::new_unbound(
        hvac_handle,
        &diagnostics,
        &mut ota_driver,
        test_log_provider,
        HttpServerConfig::default(),
    );
    let mut stream = MockStream::new(b"GET / HTTP/1.1\r\nHost: 192.168.1.142\r\n\r\n");

    // Act
    let result = block_on(server.handle_request(&mut stream));

    // Assert
    assert!(result.is_ok());
    let response = stream.written_bytes();
    let body_marker = b"\r\n\r\n";
    let header_end = response
        .windows(4)
        .position(|w| w == body_marker)
        .expect("Missing header delimiter");
    let header_str = core::str::from_utf8(&response[..header_end]).expect("Header is not UTF-8");
    assert!(header_str.starts_with("HTTP/1.1 200 OK"));
    assert!(header_str.contains("Content-Encoding: gzip"));
    assert!(header_str.contains("Content-Type: text/html"));
    assert!(header_str.contains(&format!("Content-Length: {}", DASHBOARD_HTML_GZ.len())));

    let body = &response[header_end + 4..];
    assert_eq!(body, DASHBOARD_HTML_GZ);

    let mut decoder = GzDecoder::new(body);
    let mut html = String::new();
    decoder.read_to_string(&mut html).unwrap();
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("HVAC Bridge"));
}

#[test]
fn test_get_status_returns_json_with_hvac_telemetry() {
    // Arrange
    let cmd_channel = CommandChannel::new();
    let hvac_handle = HvacControllerHandle::new(cmd_channel.sender());
    let hardware = MockHardwareInfo;
    let state_watch = SystemStateWatch::new();
    state_watch.sender().send(SystemState::Operational);
    let hvac_watch = HvacStateWatch::new();
    hvac_watch.sender().send(HvacState {
        connected: true,
        power: true,
        mode: HvacMode::Cool,
        target_temp: TemperatureCentiC(2250),
        room_temp: Some(TemperatureCentiC(2300)),
        fan_speed: FanSpeed::Auto,
        vane_vertical: VaneVertical::Swing,
        vane_horizontal: VaneHorizontal::Center,
        wide_vane_adjustment: true,
        operating: true,
        compressor_freq_hz: Some(38),
    });
    const ECOSYSTEMS: &[EcosystemId] = &[EcosystemId::new("Matter")];
    let diagnostics = DiagnosticsService::with_ecosystem(
        &hardware,
        &(),
        ECOSYSTEMS,
        &state_watch,
        &hvac_watch,
    );
    let mut ota_driver = MockOtaFlashDriver::new(false);
    let mut server = HttpServer::new_unbound(
        hvac_handle,
        &diagnostics,
        &mut ota_driver,
        test_log_provider,
        HttpServerConfig::default(),
    );
    let mut stream = MockStream::new(b"GET /api/status HTTP/1.1\r\n\r\n");

    // Act
    let result = block_on(server.handle_request(&mut stream));

    // Assert
    assert!(result.is_ok());
    let response_str = stream.written_str();
    assert!(response_str.starts_with("HTTP/1.1 200 OK"));
    assert!(response_str.contains("Content-Type: application/json"));

    let header_end = response_str.find("\r\n\r\n").expect("Missing header separator");
    let json_body = &response_str[header_end + 4..];
    let val: Value = serde_json::from_str(json_body).expect("Invalid JSON");
    assert_eq!(val["device_type"], "hvac_bridge");
    assert_eq!(val["state"], "Operational");
    assert_eq!(val["uptime_sec"], 1234);
    assert_eq!(val["hvac"]["connected"], true);
    assert_eq!(val["hvac"]["power"], true);
    assert_eq!(val["hvac"]["mode"], "cool");
    assert_eq!(val["hvac"]["target_temp_c"], "22.5");
    assert_eq!(val["hvac"]["room_temp_c"], "23.0");
    assert_eq!(val["hvac"]["vane_vertical"], "swing");
    assert_eq!(val["hvac"]["vane_horizontal"], "center");
    assert_eq!(val["hvac"]["wide_vane_adjustment"], true);
    assert_eq!(val["hvac"]["compressor_freq_hz"], 38);
}

#[test]
fn test_get_logs_returns_log_text() {
    // Arrange
    let cmd_channel = CommandChannel::new();
    let hvac_handle = HvacControllerHandle::new(cmd_channel.sender());
    let hardware = MockHardwareInfo;
    let state_watch = SystemStateWatch::new();
    let hvac_watch = HvacStateWatch::new();
    let diagnostics = DiagnosticsService::new(&hardware, &state_watch, &hvac_watch);
    let mut ota_driver = MockOtaFlashDriver::new(false);
    let mut server = HttpServer::new_unbound(
        hvac_handle,
        &diagnostics,
        &mut ota_driver,
        test_log_provider,
        HttpServerConfig::default(),
    );
    let mut stream = MockStream::new(b"GET /api/logs HTTP/1.1\r\n\r\n");

    // Act
    let result = block_on(server.handle_request(&mut stream));

    // Assert
    assert!(result.is_ok());
    let response_str = stream.written_str();
    assert!(response_str.starts_with("HTTP/1.1 200 OK"));
    assert!(response_str.contains("Content-Type: text/plain"));
    assert!(response_str.contains("bridge running smoothly"));
}

#[test]
fn test_post_action_hvac_dispatches_patch() {
    // Arrange
    let cmd_channel = CommandChannel::new();
    let hvac_handle = HvacControllerHandle::new(cmd_channel.sender());
    let hardware = MockHardwareInfo;
    let state_watch = SystemStateWatch::new();
    let hvac_watch = HvacStateWatch::new();
    let diagnostics = DiagnosticsService::new(&hardware, &state_watch, &hvac_watch);
    let mut ota_driver = MockOtaFlashDriver::new(false);
    let mut server = HttpServer::new_unbound(
        hvac_handle,
        &diagnostics,
        &mut ota_driver,
        test_log_provider,
        HttpServerConfig::default(),
    );
    let req = b"POST /api/action/hvac?power=1&mode=heat&temp=2400&fan=2 HTTP/1.1\r\nHost: 192.168.1.142\r\n\r\n";
    let mut stream = MockStream::new(req);

    // Act
    let result = block_on(server.handle_request(&mut stream));

    // Assert
    assert!(result.is_ok());
    let response_str = stream.written_str();
    assert!(response_str.starts_with("HTTP/1.1 200 OK"));
    assert!(response_str.contains("{\"status\":\"ok\"}"));

    let patch = cmd_channel
        .receiver()
        .try_receive()
        .expect("Patch was not dispatched to channel");
    assert_eq!(
        patch,
        HvacPatch {
            power: Some(true),
            mode: Some(HvacMode::Heat),
            target_temp: Some(TemperatureCentiC(2400)),
            fan_speed: Some(FanSpeed::Speed2),
            vane_vertical: None,
            vane_horizontal: None,
        }
    );
}

#[test]
fn test_post_ota_flash_success() {
    // Arrange
    let cmd_channel = CommandChannel::new();
    let hvac_handle = HvacControllerHandle::new(cmd_channel.sender());
    let hardware = MockHardwareInfo;
    let state_watch = SystemStateWatch::new();
    let hvac_watch = HvacStateWatch::new();
    let diagnostics = DiagnosticsService::new(&hardware, &state_watch, &hvac_watch);
    let mut ota_driver = MockOtaFlashDriver::new(false);
    let mut server = HttpServer::new_unbound(
        hvac_handle,
        &diagnostics,
        &mut ota_driver,
        test_log_provider,
        HttpServerConfig::default(),
    );
    let payload = b"FAKE_FIRMWARE_BINARY_DATA";
    let mut req = Vec::new();
    req.extend_from_slice(
        format!(
            "POST /api/v1/ota/flash HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            payload.len()
        )
        .as_bytes(),
    );
    req.extend_from_slice(payload);
    let mut stream = MockStream::new(&req);

    // Act
    let result = block_on(server.handle_request(&mut stream));

    // Assert
    assert!(result.is_ok());
    let response_str = stream.written_str();
    assert!(response_str.starts_with("HTTP/1.1 200 OK"));
    assert!(response_str.contains("{\"status\":\"ok\"}"));
    assert_eq!(ota_driver.call_count, 1);
    assert_eq!(ota_driver.received_payload, payload);
}

#[test]
fn test_post_ota_flash_failure_returns_400() {
    // Arrange
    let cmd_channel = CommandChannel::new();
    let hvac_handle = HvacControllerHandle::new(cmd_channel.sender());
    let hardware = MockHardwareInfo;
    let state_watch = SystemStateWatch::new();
    let hvac_watch = HvacStateWatch::new();
    let diagnostics = DiagnosticsService::new(&hardware, &state_watch, &hvac_watch);
    let mut ota_driver = MockOtaFlashDriver::new(true); // Fails verification
    let mut server = HttpServer::new_unbound(
        hvac_handle,
        &diagnostics,
        &mut ota_driver,
        test_log_provider,
        HttpServerConfig::default(),
    );
    let payload = b"BAD_PAYLOAD";
    let mut req = Vec::new();
    req.extend_from_slice(
        format!(
            "POST /api/v1/ota/flash HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            payload.len()
        )
        .as_bytes(),
    );
    req.extend_from_slice(payload);
    let mut stream = MockStream::new(&req);

    // Act
    let result = block_on(server.handle_request(&mut stream));

    // Assert
    assert!(result.is_err());
    let response_str = stream.written_str();
    assert!(response_str.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(response_str.contains("{\"error\":\"bad_request\"}"));
}

#[test]
fn test_not_found_on_unknown_endpoint() {
    // Arrange
    let cmd_channel = CommandChannel::new();
    let hvac_handle = HvacControllerHandle::new(cmd_channel.sender());
    let hardware = MockHardwareInfo;
    let state_watch = SystemStateWatch::new();
    let hvac_watch = HvacStateWatch::new();
    let diagnostics = DiagnosticsService::new(&hardware, &state_watch, &hvac_watch);
    let mut ota_driver = MockOtaFlashDriver::new(false);
    let mut server = HttpServer::new_unbound(
        hvac_handle,
        &diagnostics,
        &mut ota_driver,
        test_log_provider,
        HttpServerConfig::default(),
    );
    let mut stream = MockStream::new(b"GET /api/nonexistent HTTP/1.1\r\n\r\n");

    // Act
    let result = block_on(server.handle_request(&mut stream));

    // Assert
    assert!(result.is_ok());
    let response_str = stream.written_str();
    assert!(response_str.starts_with("HTTP/1.1 404 Not Found"));
    assert!(response_str.contains("{\"error\":\"not_found\"}"));
}

struct RecordingResetDriver {
    erased_scope: std::sync::atomic::AtomicU8,
}

impl hvac_core::ports::SystemResetDriver for RecordingResetDriver {
    fn erase_credentials(&self, scope: hvac_core::ports::ResetScope) {
        let val = match scope {
            hvac_core::ports::ResetScope::Ecosystem => 1,
            hvac_core::ports::ResetScope::Factory => 2,
        };
        self.erased_scope.store(val, std::sync::atomic::Ordering::SeqCst);
    }
    fn reboot(&self) -> ! {
        assert_eq!(
            self.erased_scope.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "expected ResetScope::Ecosystem to be erased before reboot"
        );
        panic!("MOCK_ECOSYSTEM_RESET_REBOOT_TRIGGERED");
    }
}

#[test]
#[should_panic(expected = "MOCK_ECOSYSTEM_RESET_REBOOT_TRIGGERED")]
fn test_post_action_reset_matter_erases_ecosystem_and_reboots() {
    let cmd_channel = CommandChannel::new();
    let hvac_handle = HvacControllerHandle::new(cmd_channel.sender());
    let hardware = MockHardwareInfo;
    let state_watch = SystemStateWatch::new();
    state_watch.sender().send(SystemState::Operational);
    let hvac_watch = HvacStateWatch::new();
    let diagnostics = DiagnosticsService::new(&hardware, &state_watch, &hvac_watch);
    let mut ota_driver = MockOtaFlashDriver::new(false);
    let reset_driver = RecordingResetDriver {
        erased_scope: std::sync::atomic::AtomicU8::new(0),
    };
    let mut server = HttpServer::new_unbound(
        hvac_handle,
        &diagnostics,
        &mut ota_driver,
        test_log_provider,
        HttpServerConfig::default(),
    )
    .with_reset_driver(&reset_driver);

    let mut stream =
        MockStream::new(b"POST /api/action/reset/matter HTTP/1.1\r\nHost: 192.168.1.150\r\n\r\n");

    let _ = block_on(server.handle_request(&mut stream));
}

#[test]
fn test_post_action_reset_matter_sets_ecosystem_reset_state() {
    let cmd_channel = CommandChannel::new();
    let hvac_handle = HvacControllerHandle::new(cmd_channel.sender());
    let hardware = MockHardwareInfo;
    let state_watch = SystemStateWatch::new();
    state_watch.sender().send(SystemState::Operational);
    let state_sender = hvac_core::events::SystemStateSender::new(&state_watch);
    let hvac_watch = HvacStateWatch::new();
    let diagnostics = DiagnosticsService::new(&hardware, &state_watch, &hvac_watch);
    let mut ota_driver = MockOtaFlashDriver::new(false);
    let mut server = HttpServer::new_unbound(
        hvac_handle,
        &diagnostics,
        &mut ota_driver,
        test_log_provider,
        HttpServerConfig::default(),
    )
    .with_state_sender(state_sender);

    let mut stream =
        MockStream::new(b"POST /api/action/reset/matter HTTP/1.1\r\nHost: 192.168.1.150\r\n\r\n");

    let result = block_on(server.handle_request(&mut stream));

    assert!(result.is_ok());
    assert!(stream.written_str().contains("HTTP/1.1 200 "));
    assert_eq!(
        state_sender.current_state(),
        Some(SystemState::EcosystemReset)
    );
}

struct MockRebootDriver;

impl hvac_core::ports::SystemResetDriver for MockRebootDriver {
    fn erase_credentials(&self, _scope: hvac_core::ports::ResetScope) {}
    fn reboot(&self) -> ! {
        panic!("MOCK_REBOOT_TRIGGERED");
    }
}

#[test]
#[should_panic(expected = "MOCK_REBOOT_TRIGGERED")]
fn test_post_action_reboot_invokes_reboot() {
    let cmd_channel = CommandChannel::new();
    let hvac_handle = HvacControllerHandle::new(cmd_channel.sender());
    let hardware = MockHardwareInfo;
    let state_watch = SystemStateWatch::new();
    let hvac_watch = HvacStateWatch::new();
    let diagnostics = DiagnosticsService::new(&hardware, &state_watch, &hvac_watch);
    let mut ota_driver = MockOtaFlashDriver::new(false);
    let reset_driver = MockRebootDriver;
    let mut server = HttpServer::new_unbound(
        hvac_handle,
        &diagnostics,
        &mut ota_driver,
        test_log_provider,
        HttpServerConfig::default(),
    )
    .with_reset_driver(&reset_driver);

    let mut stream =
        MockStream::new(b"POST /api/action/reboot HTTP/1.1\r\nHost: 192.168.1.150\r\n\r\n");

    let _ = block_on(server.handle_request(&mut stream));
}

struct RecordingFactoryResetDriver {
    erased_scope: std::sync::atomic::AtomicU8,
}

impl hvac_core::ports::SystemResetDriver for RecordingFactoryResetDriver {
    fn erase_credentials(&self, scope: hvac_core::ports::ResetScope) {
        let val = match scope {
            hvac_core::ports::ResetScope::Ecosystem => 1,
            hvac_core::ports::ResetScope::Factory => 2,
        };
        self.erased_scope.store(val, std::sync::atomic::Ordering::SeqCst);
    }
    fn reboot(&self) -> ! {
        assert_eq!(
            self.erased_scope.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "expected ResetScope::Factory to be erased before reboot"
        );
        panic!("MOCK_FACTORY_RESET_REBOOT_TRIGGERED");
    }
}

#[test]
#[should_panic(expected = "MOCK_FACTORY_RESET_REBOOT_TRIGGERED")]
fn test_post_action_reset_factory_erases_and_reboots() {
    let cmd_channel = CommandChannel::new();
    let hvac_handle = HvacControllerHandle::new(cmd_channel.sender());
    let hardware = MockHardwareInfo;
    let state_watch = SystemStateWatch::new();
    let hvac_watch = HvacStateWatch::new();
    let diagnostics = DiagnosticsService::new(&hardware, &state_watch, &hvac_watch);
    let mut ota_driver = MockOtaFlashDriver::new(false);
    let reset_driver = RecordingFactoryResetDriver {
        erased_scope: std::sync::atomic::AtomicU8::new(0),
    };
    let mut server = HttpServer::new_unbound(
        hvac_handle,
        &diagnostics,
        &mut ota_driver,
        test_log_provider,
        HttpServerConfig::default(),
    )
    .with_reset_driver(&reset_driver);

    let mut stream =
        MockStream::new(b"POST /api/action/reset/factory HTTP/1.1\r\nHost: 192.168.1.150\r\n\r\n");

    let _ = block_on(server.handle_request(&mut stream));
}
