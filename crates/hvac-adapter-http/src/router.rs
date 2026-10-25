//! HTTP request router, query-string parser, and zero-allocation JSON serialization.
//!
//! Provides static route parsing and diagnostic telemetry formatting
//! directly into pre-allocated byte slices without heap allocation.

use core::fmt::Write;
use edf_http_diagnostics::{parse_diagnostic_route, DiagnosticRoute};
use edf_http_ota::{parse_ota_route, OtaRoute};
use hvac_core::domain::diagnostics::SystemDiagnostics;
use hvac_core::domain::ecosystem::EcosystemType;
use hvac_core::domain::hvac::{
    FanSpeed, HvacMode, HvacPatch, HvacState, TemperatureCentiC, VaneHorizontal, VaneVertical,
};

// ============================================================================
// Public Constants
// ============================================================================

/// Static HTTP 200 OK JSON response.
pub const HTTP_200_JSON_OK: &[u8] = b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 15\r\n\r\n{\"status\":\"ok\"}";

/// Static HTTP 404 Not Found JSON response.
pub const HTTP_404_NOT_FOUND: &[u8] = b"HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 22\r\n\r\n{\"error\":\"not_found\"}";

/// Static HTTP 400 Bad Request JSON response.
pub const HTTP_400_BAD_REQUEST: &[u8] = b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 24\r\n\r\n{\"error\":\"bad_request\"}";

// ============================================================================
// Public Route Definitions & Parsing
// ============================================================================

/// Identified HTTP routes supported by the HVAC Bridge HTTP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Dashboard,
    Status,
    Logs,
    ActionHvac(HvacPatch),
    ActionResetMatter,
    ActionResetFactory,
    ActionReboot,
    OtaFlash,
    NotFound,
}

/// Parses an HTTP request line (e.g., `GET /api/status HTTP/1.1`) into a `Route`.
pub fn parse_route(request_line: &str) -> Route {
    let mut parts = request_line.split_whitespace();
    let method = match parts.next() {
        Some(m) => m,
        None => return Route::NotFound,
    };
    let raw_uri = match parts.next() {
        Some(u) => u,
        None => return Route::NotFound,
    };

    let (path, query) = match raw_uri.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (raw_uri, None),
    };

    if method == "POST" && path == "/api/action/hvac" {
        let patch = match query {
            Some(q) => parse_hvac_patch_query(q),
            None => HvacPatch::default(),
        };
        return Route::ActionHvac(patch);
    }
    if parse_ota_route(method, path) == Some(OtaRoute::Flash) {
        return Route::OtaFlash;
    }
    match parse_diagnostic_route(method, path) {
        Some(DiagnosticRoute::Dashboard) => Route::Dashboard,
        Some(DiagnosticRoute::Status) => Route::Status,
        Some(DiagnosticRoute::Logs) => Route::Logs,
        Some(DiagnosticRoute::Reboot) => Route::ActionReboot,
        Some(DiagnosticRoute::Reset(hvac_core::ports::ResetScope::Ecosystem)) => {
            Route::ActionResetMatter
        }
        Some(DiagnosticRoute::Reset(hvac_core::ports::ResetScope::Factory)) => {
            Route::ActionResetFactory
        }
        _ => Route::NotFound,
    }
}

/// Parses an `HvacPatch` from URL query parameters.
///
/// Example: `power=1&mode=cool&temp=2250&fan=quiet&vane_v=swing&vane_h=center`
pub fn parse_hvac_patch_query(query: &str) -> HvacPatch {
    let mut patch = HvacPatch::default();

    for param in query.split('&') {
        if param.is_empty() {
            continue;
        }
        let (key, val) = match param.split_once('=') {
            Some((k, v)) => (k, v),
            None => continue,
        };

        match key {
            "power" => {
                if val == "1" || val.eq_ignore_ascii_case("true") || val.eq_ignore_ascii_case("on")
                {
                    patch.power = Some(true);
                } else if val == "0"
                    || val.eq_ignore_ascii_case("false")
                    || val.eq_ignore_ascii_case("off")
                {
                    patch.power = Some(false);
                }
            }
            "mode" => {
                if val.eq_ignore_ascii_case("heat") {
                    patch.mode = Some(HvacMode::Heat);
                } else if val.eq_ignore_ascii_case("cool") {
                    patch.mode = Some(HvacMode::Cool);
                } else if val.eq_ignore_ascii_case("auto") {
                    patch.mode = Some(HvacMode::Auto);
                } else if val.eq_ignore_ascii_case("dry") {
                    patch.mode = Some(HvacMode::Dry);
                } else if val.eq_ignore_ascii_case("fan") || val.eq_ignore_ascii_case("fan_only") {
                    patch.mode = Some(HvacMode::FanOnly);
                }
            }
            "temp" => {
                if let Some(centi) = parse_temp_centi(val) {
                    patch.target_temp = Some(TemperatureCentiC::from_centi(centi).clamp_setpoint());
                }
            }
            "fan" => {
                if val.eq_ignore_ascii_case("auto") {
                    patch.fan_speed = Some(FanSpeed::Auto);
                } else if val.eq_ignore_ascii_case("quiet") {
                    patch.fan_speed = Some(FanSpeed::Quiet);
                } else if val == "1" {
                    patch.fan_speed = Some(FanSpeed::Speed1);
                } else if val == "2" {
                    patch.fan_speed = Some(FanSpeed::Speed2);
                } else if val == "3" {
                    patch.fan_speed = Some(FanSpeed::Speed3);
                } else if val == "4" {
                    patch.fan_speed = Some(FanSpeed::Speed4);
                }
            }
            "vane_v" => {
                if val.eq_ignore_ascii_case("auto") {
                    patch.vane_vertical = Some(VaneVertical::Auto);
                } else if val == "1" || val.eq_ignore_ascii_case("pos1") {
                    patch.vane_vertical = Some(VaneVertical::Pos1);
                } else if val == "2" || val.eq_ignore_ascii_case("pos2") {
                    patch.vane_vertical = Some(VaneVertical::Pos2);
                } else if val == "3" || val.eq_ignore_ascii_case("pos3") {
                    patch.vane_vertical = Some(VaneVertical::Pos3);
                } else if val == "4" || val.eq_ignore_ascii_case("pos4") {
                    patch.vane_vertical = Some(VaneVertical::Pos4);
                } else if val == "5" || val.eq_ignore_ascii_case("pos5") {
                    patch.vane_vertical = Some(VaneVertical::Pos5);
                } else if val.eq_ignore_ascii_case("swing") {
                    patch.vane_vertical = Some(VaneVertical::Swing);
                }
            }
            "vane_h" => {
                if val.eq_ignore_ascii_case("auto") {
                    patch.vane_horizontal = Some(VaneHorizontal::Auto);
                } else if val.eq_ignore_ascii_case("left_max") {
                    patch.vane_horizontal = Some(VaneHorizontal::LeftMax);
                } else if val.eq_ignore_ascii_case("left") {
                    patch.vane_horizontal = Some(VaneHorizontal::Left);
                } else if val.eq_ignore_ascii_case("center") {
                    patch.vane_horizontal = Some(VaneHorizontal::Center);
                } else if val.eq_ignore_ascii_case("right") {
                    patch.vane_horizontal = Some(VaneHorizontal::Right);
                } else if val.eq_ignore_ascii_case("right_max") {
                    patch.vane_horizontal = Some(VaneHorizontal::RightMax);
                } else if val.eq_ignore_ascii_case("split") {
                    patch.vane_horizontal = Some(VaneHorizontal::Split);
                } else if val.eq_ignore_ascii_case("swing") {
                    patch.vane_horizontal = Some(VaneHorizontal::Swing);
                }
            }
            _ => {}
        }
    }

    patch
}

/// Formats `SystemDiagnostics` into caller-provided buffer as compact JSON.
///
/// Returns the number of bytes written, or `Err(())` if the buffer is too small.
pub fn format_status_json<T: EcosystemType>(
    diag: &SystemDiagnostics<HvacState, T>,
    out: &mut [u8],
) -> Result<usize, ()> {
    let shared = shared_snapshot(diag);
    edf_http_diagnostics::format_status_json(&shared, out).map_err(|_| ())
}

pub(crate) fn shared_snapshot<'a, T: EcosystemType>(
    diag: &'a SystemDiagnostics<HvacState, T>,
) -> SystemDiagnostics<HvacPayload<'a>, T> {
    SystemDiagnostics {
        device_type: diag.device_type,
        state: diag.state,
        domain: HvacPayload(&diag.domain),
        uptime_sec: diag.uptime_sec,
        wifi_ssid: diag.wifi_ssid,
        wifi_rssi_dbm: diag.wifi_rssi_dbm,
        ip: diag.ip,
        mac: diag.mac,
        active_slot: diag.active_slot,
        slot_valid: diag.slot_valid,
        free_heap_bytes: diag.free_heap_bytes,
        total_heap_bytes: diag.total_heap_bytes,
        available_ecosystems: diag.available_ecosystems,
        active_ecosystem: diag.active_ecosystem.clone(),
        version: diag.version,
        git_commit: diag.git_commit,
        build_date: diag.build_date,
    }
}

pub(crate) struct HvacPayload<'a>(&'a HvacState);

impl edf_http_diagnostics::DiagnosticPayload for HvacPayload<'_> {
    fn write_fields<W: core::fmt::Write>(&self, out: &mut W) -> core::fmt::Result {
        let hvac = self.0;
        let (target_whole, target_tenths) = hvac.target_temp.whole_and_tenths();
        write!(
            out,
            ",\"hvac\":{{\"connected\":{},\"power\":{},\"mode\":",
            hvac.connected, hvac.power
        )?;
        edf_http_diagnostics::write_json_string(out, hvac.mode.as_str())?;
        write!(
            out,
            ",\"target_temp_c\":\"{target_whole}.{target_tenths}\",\"room_temp_c\":"
        )?;
        match hvac.room_temp {
            Some(room) => {
                let (whole, tenths) = room.whole_and_tenths();
                write!(out, "\"{whole}.{tenths}\"")?;
            }
            None => out.write_str("null")?,
        }
        out.write_str(",\"fan_speed\":")?;
        edf_http_diagnostics::write_json_string(out, hvac.fan_speed.as_str())?;
        out.write_str(",\"vane_vertical\":")?;
        edf_http_diagnostics::write_json_string(out, hvac.vane_vertical.as_str())?;
        out.write_str(",\"vane_horizontal\":")?;
        edf_http_diagnostics::write_json_string(out, hvac.vane_horizontal.as_str())?;
        write!(
            out,
            ",\"wide_vane_adjustment\":{},\"operating\":{},\"compressor_freq_hz\":",
            hvac.wide_vane_adjustment, hvac.operating
        )?;
        match hvac.compressor_freq_hz {
            Some(hz) => write!(out, "{hz}")?,
            None => out.write_str("null")?,
        }
        out.write_char('}')
    }
}

// ============================================================================
// Private Helpers & Writers
// ============================================================================

fn parse_temp_centi(val: &str) -> Option<i16> {
    if let Some((whole_str, frac_str)) = val.split_once('.') {
        let whole: i16 = whole_str.parse().ok()?;
        let frac_digit: i16 = frac_str.chars().next()?.to_digit(10)? as i16;
        if whole >= 0 {
            Some(whole * 100 + frac_digit * 10)
        } else {
            Some(whole * 100 - frac_digit * 10)
        }
    } else {
        val.parse::<i16>().ok()
    }
}

pub(crate) struct BufferWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> BufferWriter<'a> {
    pub(crate) fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub(crate) fn written(&self) -> usize {
        self.pos
    }
}

impl<'a> Write for BufferWriter<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let remaining = self.buf.len().saturating_sub(self.pos);
        if bytes.len() > remaining {
            return Err(core::fmt::Error);
        }
        self.buf[self.pos..self.pos + bytes.len()].copy_from_slice(bytes);
        self.pos += bytes.len();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hvac_core::domain::ecosystem::{EcosystemId, EcosystemStatus, EndpointInfo};
    use hvac_core::events::SystemState;
    use serde_json::Value;

    #[test]
    fn status_json_escapes_shared_and_hvac_adjacent_strings() {
        let diag = SystemDiagnostics {
            device_type: "hvac\"bridge",
            state: SystemState::Operational,
            domain: HvacState::default(),
            uptime_sec: 0,
            wifi_ssid: None,
            wifi_rssi_dbm: None,
            ip: None,
            mac: [0; 6],
            active_slot: "ota_0",
            slot_valid: true,
            free_heap_bytes: 0,
            total_heap_bytes: 0,
            available_ecosystems: &[] as &[EcosystemId],
            active_ecosystem: Some(
                EcosystemStatus::new(EcosystemId::new("matter"), true).with_endpoint(
                    EndpointInfo {
                        id: 1,
                        name: "HVAC\nzone",
                        cluster: "thermo\\stat",
                        ready: true,
                    },
                ),
            ),
            version: "v\"1",
            git_commit: "hash",
            build_date: "2026",
        };
        let mut out = [0; 2048];
        let len = format_status_json(&diag, &mut out).unwrap();
        let value: Value = serde_json::from_slice(&out[..len]).unwrap();
        assert_eq!(value["device_type"], "hvac\"bridge");
        assert_eq!(
            value["active_ecosystem"]["endpoints"][0]["name"],
            "HVAC\nzone"
        );
        assert_eq!(
            value["active_ecosystem"]["endpoints"][0]["cluster"],
            "thermo\\stat"
        );
        assert_eq!(value["version"], "v\"1");
    }
}
