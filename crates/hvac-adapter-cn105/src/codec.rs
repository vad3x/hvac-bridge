//! Mitsubishi Electric CN105 packet encoder and decoder.
//!
//! Encodes mutations ([`HvacPatch`]) into `0x41` set packets and decodes `0x62` information
//! responses into canonical [`HvacState`].

use hvac_core::domain::{
    FanSpeed, HvacMode, HvacPatch, HvacState, TemperatureCentiC, VaneHorizontal, VaneVertical,
};
use hvac_core::ports::HvacDriverError;

use crate::frame::{
    checksum, verify_frame, CMD_GET_RESPONSE, CMD_SET_REQUEST, HEADER_LEN, HEADER_MAGIC,
    PACKET_LEN, PAYLOAD_LEN, SUBHEADER_1, SUBHEADER_2,
};

// ============================================================================
// Public Constants
// ============================================================================

/// Query subtype for Settings (`0x02`): mode, setpoint, fan, vanes, power.
pub const SUBTYPE_SETTINGS: u8 = 0x02;

/// Query subtype for Room Temperature (`0x03`).
pub const SUBTYPE_ROOM_TEMP: u8 = 0x03;

/// Query subtype for Status / Compressor (`0x06`): compressor Hz, operating state.
pub const SUBTYPE_STATUS: u8 = 0x06;

// ============================================================================
// Public Types
// ============================================================================

/// Query subtype for CN105 GET (`0x42`) requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GetPacketType {
    /// Settings request (`0x02`): power, mode, setpoint, fan, vanes.
    Settings,
    /// Room temperature request (`0x03`).
    RoomTemp,
    /// Operating status request (`0x06`): compressor frequency and running state.
    Status,
}

impl GetPacketType {
    /// Return the raw CN105 subtype byte (`data[0]`).
    pub const fn packet_type_code(self) -> u8 {
        match self {
            Self::Settings => SUBTYPE_SETTINGS,
            Self::RoomTemp => SUBTYPE_ROOM_TEMP,
            Self::Status => SUBTYPE_STATUS,
        }
    }

    /// Construct from raw CN105 subtype byte.
    pub const fn from_u8(val: u8) -> Option<Self> {
        match val {
            SUBTYPE_SETTINGS => Some(Self::Settings),
            SUBTYPE_ROOM_TEMP => Some(Self::RoomTemp),
            SUBTYPE_STATUS => Some(Self::Status),
            _ => None,
        }
    }
}

impl TryFrom<u8> for GetPacketType {
    type Error = HvacDriverError;

    fn try_from(val: u8) -> Result<Self, Self::Error> {
        Self::from_u8(val).ok_or(HvacDriverError::InvalidFrame)
    }
}

/// Dynamic codec tracking state for CN105 unit capabilities and protocol flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cn105CodecState {
    /// Whether the unit supports 0.5 °C temperature resolution.
    ///
    /// Detected from non-zero `data[11]` in `0x02` Settings response.
    pub temp_mode: bool,
    /// Whether the wide vane (horizontal) adjustment flag was active on the unit.
    ///
    /// Detected from `(data[10] & 0x80) != 0` in `0x02` Settings response.
    pub wide_vane_adjustment: bool,
}

impl Cn105CodecState {
    /// Create a new default codec state.
    pub const fn new() -> Self {
        Self {
            temp_mode: false,
            wide_vane_adjustment: false,
        }
    }
}

// ============================================================================
// Public Functions
// ============================================================================

/// Encode an [`HvacPatch`] into a standard 22-byte CN105 `0x41` set packet.
///
/// Sets the appropriate control bitmasks in `data[1]` and `data[2]` and packs
/// target parameters into the corresponding payload bytes. Target temperature is
/// formatted as 0.5 °C in `data[14]` when `codec_state.temp_mode` is true, or as
/// legacy integer fallback in `data[5]`.
pub fn encode_set_packet(patch: &HvacPatch, codec_state: &Cn105CodecState) -> [u8; PACKET_LEN] {
    let mut pkt = [0u8; PACKET_LEN];

    // Header (5 bytes)
    pkt[0] = HEADER_MAGIC;
    pkt[1] = CMD_SET_REQUEST;
    pkt[2] = SUBHEADER_1;
    pkt[3] = SUBHEADER_2;
    pkt[4] = PAYLOAD_LEN as u8;

    // Payload (16 bytes, indices 5..21)
    let data = &mut pkt[HEADER_LEN..HEADER_LEN + PAYLOAD_LEN];
    data[0] = 0x01; // Constant request subtype for 0x41 set packet

    // Control bitmask 1 (data[1])
    if let Some(power) = patch.power {
        data[1] |= 0x01;
        data[3] = if power { 0x01 } else { 0x00 };
    }

    if let Some(mode) = patch.mode {
        data[1] |= 0x02;
        data[4] = match mode {
            HvacMode::Heat => 0x01,
            HvacMode::Dry => 0x02,
            HvacMode::Cool => 0x03,
            HvacMode::FanOnly => 0x07,
            HvacMode::Auto => 0x08,
        };
    }

    if let Some(target_temp) = patch.target_temp {
        data[1] |= 0x04;
        let clamped = target_temp.clamp_setpoint();
        if codec_state.temp_mode {
            data[14] = (clamped.to_half_degrees_rounded() as u8) | 0x80;
            data[5] = 0x00;
        } else {
            let deg = clamped.0 / 100;
            data[5] = (31i16 - deg) as u8;
            data[14] = 0x00;
        }
    }

    if let Some(fan_speed) = patch.fan_speed {
        data[1] |= 0x08;
        data[6] = match fan_speed {
            FanSpeed::Auto => 0x00,
            FanSpeed::Quiet => 0x01,
            FanSpeed::Speed1 => 0x02,
            FanSpeed::Speed2 => 0x03,
            FanSpeed::Speed3 => 0x05,
            FanSpeed::Speed4 => 0x06,
        };
    }

    if let Some(vane_vertical) = patch.vane_vertical {
        data[1] |= 0x10;
        data[7] = match vane_vertical {
            VaneVertical::Auto => 0x00,
            VaneVertical::Pos1 => 0x01,
            VaneVertical::Pos2 => 0x02,
            VaneVertical::Pos3 => 0x03,
            VaneVertical::Pos4 => 0x04,
            VaneVertical::Pos5 => 0x05,
            VaneVertical::Swing => 0x07,
        };
    }

    // Control bitmask 2 (data[2])
    if let Some(vane_horizontal) = patch.vane_horizontal {
        data[2] |= 0x01;
        let mut val = match vane_horizontal {
            VaneHorizontal::Auto => 0x00,
            VaneHorizontal::LeftMax => 0x01,
            VaneHorizontal::Left => 0x02,
            VaneHorizontal::Center => 0x03,
            VaneHorizontal::Right => 0x04,
            VaneHorizontal::RightMax => 0x05,
            VaneHorizontal::Split => 0x08,
            VaneHorizontal::Swing => 0x0C,
        };
        if codec_state.wide_vane_adjustment {
            val |= 0x80;
        }
        data[13] = val;
    }

    // Checksum (byte 21)
    pkt[PACKET_LEN - 1] = checksum(&pkt[..PACKET_LEN - 1]);
    pkt
}

/// Decode a CN105 `0x62` information response packet, updating `state` and `codec_state`.
///
/// Supports query subtypes:
/// - `0x02` (Settings): power, mode, setpoint (0.5 °C or legacy), fan, vanes.
/// - `0x03` (Room Temperature): 0.5 °C in `data[6]` or integer fallback in `data[3]`.
/// - `0x06` (Status): compressor frequency Hz and operating flag.
///
/// # Errors
/// Returns [`HvacDriverError::InvalidFrame`] if frame length is not [`PACKET_LEN`], frame header,
/// command byte (`0x62`), or subtype is invalid, or [`HvacDriverError::ChecksumMismatch`] if the
/// packet checksum does not match.
pub fn decode_response_packet(
    bytes: &[u8],
    state: &mut HvacState,
    codec_state: &mut Cn105CodecState,
) -> Result<GetPacketType, HvacDriverError> {
    verify_frame(bytes)?;

    if bytes.len() != PACKET_LEN || bytes[1] != CMD_GET_RESPONSE {
        return Err(HvacDriverError::InvalidFrame);
    }

    let data = &bytes[HEADER_LEN..HEADER_LEN + PAYLOAD_LEN];
    let packet_type = GetPacketType::from_u8(data[0]).ok_or(HvacDriverError::InvalidFrame)?;

    state.connected = true;

    match packet_type {
        GetPacketType::Settings => {
            // Power (data[3])
            state.power = data[3] != 0;

            // Mode (data[4]): handle i-See flag where mode > 0x08
            let raw_mode = data[4];
            let mode_code = if raw_mode > 0x08 {
                raw_mode - 0x08
            } else {
                raw_mode
            };
            match mode_code {
                0x01 => state.mode = HvacMode::Heat,
                0x02 => state.mode = HvacMode::Dry,
                0x03 => state.mode = HvacMode::Cool,
                0x07 => state.mode = HvacMode::FanOnly,
                0x08 => state.mode = HvacMode::Auto,
                _ => {}
            }

            // Target Setpoint: 0.5 °C mode (data[11]) vs legacy (data[5])
            if data[11] != 0x00 {
                codec_state.temp_mode = true;
                let half_deg = (data[11] & 0x7F) as i16;
                state.target_temp = TemperatureCentiC::from_half_degrees(half_deg).clamp_setpoint();
            } else {
                codec_state.temp_mode = false;
                let deg = 31i16 - (data[5] as i16);
                state.target_temp = TemperatureCentiC::from_centi(deg * 100).clamp_setpoint();
            }

            // Fan Speed (data[6])
            match data[6] {
                0x00 => state.fan_speed = FanSpeed::Auto,
                0x01 => state.fan_speed = FanSpeed::Quiet,
                0x02 => state.fan_speed = FanSpeed::Speed1,
                0x03 => state.fan_speed = FanSpeed::Speed2,
                0x05 => state.fan_speed = FanSpeed::Speed3,
                0x06 => state.fan_speed = FanSpeed::Speed4,
                _ => {}
            }

            // Vertical Vane (data[7])
            match data[7] {
                0x00 => state.vane_vertical = VaneVertical::Auto,
                0x01 => state.vane_vertical = VaneVertical::Pos1,
                0x02 => state.vane_vertical = VaneVertical::Pos2,
                0x03 => state.vane_vertical = VaneVertical::Pos3,
                0x04 => state.vane_vertical = VaneVertical::Pos4,
                0x05 => state.vane_vertical = VaneVertical::Pos5,
                0x07 => state.vane_vertical = VaneVertical::Swing,
                _ => {}
            }

            // Horizontal Vane (data[10])
            codec_state.wide_vane_adjustment = (data[10] & 0x80) != 0;
            state.wide_vane_adjustment = codec_state.wide_vane_adjustment;
            match data[10] & 0x0F {
                0x00 => state.vane_horizontal = VaneHorizontal::Auto,
                0x01 => state.vane_horizontal = VaneHorizontal::LeftMax,
                0x02 => state.vane_horizontal = VaneHorizontal::Left,
                0x03 => state.vane_horizontal = VaneHorizontal::Center,
                0x04 => state.vane_horizontal = VaneHorizontal::Right,
                0x05 => state.vane_horizontal = VaneHorizontal::RightMax,
                0x08 => state.vane_horizontal = VaneHorizontal::Split,
                0x0C => state.vane_horizontal = VaneHorizontal::Swing,
                _ => {}
            }
        }
        GetPacketType::RoomTemp => {
            // Room Temperature: 0.5 °C mode in data[6], otherwise fallback to data[3] + 10
            if data[6] != 0x00 {
                let half_deg = (data[6] & 0x7F) as i16;
                state.room_temp = Some(TemperatureCentiC::from_half_degrees(half_deg));
            } else if data[3] != 0x00 {
                let deg = (data[3] as i16) + 10;
                state.room_temp = Some(TemperatureCentiC::from_centi(deg * 100));
            }
        }
        GetPacketType::Status => {
            // Compressor frequency Hz (data[3]) & operating flag (data[4])
            state.compressor_freq_hz = Some(data[3]);
            state.operating = data[4] != 0;
        }
    }

    Ok(packet_type)
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::CONNECT_LEN;

    /// Helper to assemble a valid 22-byte 0x62 response frame from 16 payload bytes.
    fn make_response_frame(payload: [u8; 16]) -> [u8; PACKET_LEN] {
        let mut pkt = [0u8; PACKET_LEN];
        pkt[0] = HEADER_MAGIC;
        pkt[1] = CMD_GET_RESPONSE;
        pkt[2] = SUBHEADER_1;
        pkt[3] = SUBHEADER_2;
        pkt[4] = PAYLOAD_LEN as u8;
        pkt[HEADER_LEN..HEADER_LEN + PAYLOAD_LEN].copy_from_slice(&payload);
        pkt[PACKET_LEN - 1] = checksum(&pkt[..PACKET_LEN - 1]);
        pkt
    }

    #[test]
    fn decode_0x62_settings_with_half_degree_temp_mode() {
        // Arrange
        let mut payload = [0u8; 16];
        payload[0] = SUBTYPE_SETTINGS; // 0x02
        payload[3] = 0x01; // Power On
        payload[4] = 0x03; // Mode Cool
        payload[5] = 0x00; // Legacy temp not used
        payload[6] = 0x05; // Fan Speed 3
        payload[7] = 0x07; // Vane Vertical Swing
        payload[10] = 0x8C; // Vane Horizontal Swing + wide_vane_adjustment flag (0x80)
        payload[11] = 43; // 43 * 0.5 °C = 21.5 °C (2150 centi-C)

        let packet = make_response_frame(payload);
        let mut state = HvacState::default();
        let mut codec_state = Cn105CodecState::new();

        // Act
        let result = decode_response_packet(&packet, &mut state, &mut codec_state);

        // Assert
        assert_eq!(result, Ok(GetPacketType::Settings));
        assert!(state.connected);
        assert!(state.power);
        assert_eq!(state.mode, HvacMode::Cool);
        assert_eq!(state.target_temp, TemperatureCentiC(2150));
        assert_eq!(state.fan_speed, FanSpeed::Speed3);
        assert_eq!(state.vane_vertical, VaneVertical::Swing);
        assert_eq!(state.vane_horizontal, VaneHorizontal::Swing);
        assert!(codec_state.temp_mode);
        assert!(codec_state.wide_vane_adjustment);
        assert!(state.wide_vane_adjustment);
    }

    #[test]
    fn decode_0x62_settings_with_legacy_temp_fallback_and_i_see_mode() {
        // Arrange
        let mut payload = [0u8; 16];
        payload[0] = SUBTYPE_SETTINGS; // 0x02
        payload[3] = 0x00; // Power Off
        payload[4] = 0x09; // i-See flag active (> 0x08): 0x09 - 0x08 = 0x01 (Heat)
        payload[5] = 9; // 31 - 9 = 22 °C (2200 centi-C)
        payload[6] = 0x01; // Fan Quiet
        payload[7] = 0x02; // Vane Vertical Pos2
        payload[10] = 0x03; // Vane Horizontal Center (no 0x80 flag)
        payload[11] = 0x00; // 0.5 °C not supported

        let packet = make_response_frame(payload);
        let mut state = HvacState::default();
        let mut codec_state = Cn105CodecState::new();

        // Act
        let result = decode_response_packet(&packet, &mut state, &mut codec_state);

        // Assert
        assert_eq!(result, Ok(GetPacketType::Settings));
        assert!(state.connected);
        assert!(!state.power);
        assert_eq!(state.mode, HvacMode::Heat);
        assert_eq!(state.target_temp, TemperatureCentiC(2200));
        assert_eq!(state.fan_speed, FanSpeed::Quiet);
        assert_eq!(state.vane_vertical, VaneVertical::Pos2);
        assert_eq!(state.vane_horizontal, VaneHorizontal::Center);
        assert!(!codec_state.temp_mode);
        assert!(!codec_state.wide_vane_adjustment);
        assert!(!state.wide_vane_adjustment);
    }

    #[test]
    fn decode_0x62_room_temp_half_degree_mode() {
        // Arrange
        let mut payload = [0u8; 16];
        payload[0] = SUBTYPE_ROOM_TEMP; // 0x03
        payload[6] = 47 | 0x80; // 47 * 0.5 °C = 23.5 °C (2350 centi-C), with 0x80 flag

        let packet = make_response_frame(payload);
        let mut state = HvacState::default();
        let mut codec_state = Cn105CodecState::new();

        // Act
        let result = decode_response_packet(&packet, &mut state, &mut codec_state);

        // Assert
        assert_eq!(result, Ok(GetPacketType::RoomTemp));
        assert_eq!(state.room_temp, Some(TemperatureCentiC(2350)));
    }

    #[test]
    fn decode_0x62_room_temp_integer_fallback() {
        // Arrange
        let mut payload = [0u8; 16];
        payload[0] = SUBTYPE_ROOM_TEMP; // 0x03
        payload[3] = 13; // 13 + 10 = 23 °C (2300 centi-C)
        payload[6] = 0x00; // No 0.5 °C data

        let packet = make_response_frame(payload);
        let mut state = HvacState::default();
        let mut codec_state = Cn105CodecState::new();

        // Act
        let result = decode_response_packet(&packet, &mut state, &mut codec_state);

        // Assert
        assert_eq!(result, Ok(GetPacketType::RoomTemp));
        assert_eq!(state.room_temp, Some(TemperatureCentiC(2300)));
    }

    #[test]
    fn decode_0x62_status_operating_and_compressor_freq() {
        // Arrange
        let mut payload = [0u8; 16];
        payload[0] = SUBTYPE_STATUS; // 0x06
        payload[3] = 42; // 42 Hz
        payload[4] = 0x01; // Operating true

        let packet = make_response_frame(payload);
        let mut state = HvacState::default();
        let mut codec_state = Cn105CodecState::new();

        // Act
        let result = decode_response_packet(&packet, &mut state, &mut codec_state);

        // Assert
        assert_eq!(result, Ok(GetPacketType::Status));
        assert!(state.connected);
        assert_eq!(state.compressor_freq_hz, Some(42));
        assert!(state.operating);
    }

    #[test]
    fn encode_0x41_set_packet_with_half_degree_temp_mode() {
        // Arrange
        let patch = HvacPatch {
            power: Some(true),
            mode: Some(HvacMode::Cool),
            target_temp: Some(TemperatureCentiC(2150)), // 21.5 °C -> 43 half-degrees
            fan_speed: Some(FanSpeed::Speed3),
            vane_vertical: Some(VaneVertical::Pos1),
            vane_horizontal: Some(VaneHorizontal::Right),
        };
        let codec_state = Cn105CodecState {
            temp_mode: true,
            wide_vane_adjustment: true,
        };

        // Act
        let packet = encode_set_packet(&patch, &codec_state);

        // Assert
        assert_eq!(packet[0..5], [0xFC, 0x41, 0x01, 0x30, 0x10]);
        let data = &packet[5..21];
        assert_eq!(data[0], 0x01);
        assert_eq!(data[1], 0x1F); // Power (0x01) | Mode (0x02) | Temp (0x04) | Fan (0x08) | VaneV (0x10)
        assert_eq!(data[2], 0x01); // VaneH (0x01)
        assert_eq!(data[3], 0x01); // Power On
        assert_eq!(data[4], 0x03); // Cool
        assert_eq!(data[5], 0x00); // Legacy temp zeroed when temp_mode active
        assert_eq!(data[6], 0x05); // Speed3
        assert_eq!(data[7], 0x01); // Pos1
        assert_eq!(data[13], 0x84); // Right (0x04) | wide_vane_adj (0x80)
        assert_eq!(data[14], 43 | 0x80); // 43 | 0x80 = 171
        assert_eq!(verify_frame(&packet), Ok(()));
    }

    #[test]
    fn encode_0x41_set_packet_with_legacy_temp_mode() {
        // Arrange
        let patch = HvacPatch {
            power: Some(false),
            target_temp: Some(TemperatureCentiC(2200)), // 22 °C -> 31 - 22 = 9
            ..Default::default()
        };
        let codec_state = Cn105CodecState {
            temp_mode: false,
            wide_vane_adjustment: false,
        };

        // Act
        let packet = encode_set_packet(&patch, &codec_state);

        // Assert
        assert_eq!(packet[0..5], [0xFC, 0x41, 0x01, 0x30, 0x10]);
        let data = &packet[5..21];
        assert_eq!(data[0], 0x01);
        assert_eq!(data[1], 0x05); // Power (0x01) | Temp (0x04)
        assert_eq!(data[2], 0x00); // No horizontal vane
        assert_eq!(data[3], 0x00); // Power Off
        assert_eq!(data[5], 9); // 31 - 22 = 9
        assert_eq!(data[14], 0x00); // Half-degree temp zeroed in legacy mode
        assert_eq!(verify_frame(&packet), Ok(()));
    }

    #[test]
    fn decode_response_packet_rejects_non_0x62_or_unrecognized_subtype() {
        // Arrange
        let mut bad_cmd = make_response_frame([0u8; 16]);
        bad_cmd[1] = 0x41; // Set request instead of get response
        bad_cmd[PACKET_LEN - 1] = checksum(&bad_cmd[..PACKET_LEN - 1]);

        let mut bad_subtype = [0u8; 16];
        bad_subtype[0] = 0x09; // Unknown subtype
        let bad_subtype_pkt = make_response_frame(bad_subtype);

        let mut state = HvacState::default();
        let mut codec_state = Cn105CodecState::new();

        // Act & Assert
        assert_eq!(
            decode_response_packet(&bad_cmd, &mut state, &mut codec_state),
            Err(HvacDriverError::InvalidFrame)
        );
        assert_eq!(
            decode_response_packet(&bad_subtype_pkt, &mut state, &mut codec_state),
            Err(HvacDriverError::InvalidFrame)
        );
    }

    #[test]
    fn decode_response_packet_rejects_truncated_validly_framed_packet() {
        // Arrange: valid 8-byte frame (e.g. connect-sized frame) with CMD_GET_RESPONSE
        let mut truncated = [0u8; CONNECT_LEN];
        truncated[0] = HEADER_MAGIC;
        truncated[1] = CMD_GET_RESPONSE;
        truncated[2] = SUBHEADER_1;
        truncated[3] = SUBHEADER_2;
        truncated[4] = 2; // payload len
        truncated[5] = SUBTYPE_SETTINGS;
        truncated[6] = 0x00;
        truncated[CONNECT_LEN - 1] = checksum(&truncated[..CONNECT_LEN - 1]);

        assert_eq!(verify_frame(&truncated), Ok(()));

        let mut state = HvacState::default();
        let mut codec_state = Cn105CodecState::new();

        // Act & Assert: must return Err(HvacDriverError::InvalidFrame) rather than panicking on bounds
        assert_eq!(
            decode_response_packet(&truncated, &mut state, &mut codec_state),
            Err(HvacDriverError::InvalidFrame)
        );
    }

    #[test]
    fn get_packet_type_code_and_conversions() {
        // Arrange & Act & Assert
        assert_eq!(GetPacketType::Settings.packet_type_code(), 0x02);
        assert_eq!(GetPacketType::RoomTemp.packet_type_code(), 0x03);
        assert_eq!(GetPacketType::Status.packet_type_code(), 0x06);

        assert_eq!(GetPacketType::from_u8(0x02), Some(GetPacketType::Settings));
        assert_eq!(GetPacketType::from_u8(0x03), Some(GetPacketType::RoomTemp));
        assert_eq!(GetPacketType::from_u8(0x06), Some(GetPacketType::Status));
        assert_eq!(GetPacketType::from_u8(0x99), None);

        assert_eq!(GetPacketType::try_from(0x02), Ok(GetPacketType::Settings));
        assert_eq!(GetPacketType::try_from(0x03), Ok(GetPacketType::RoomTemp));
        assert_eq!(GetPacketType::try_from(0x06), Ok(GetPacketType::Status));
        assert_eq!(
            GetPacketType::try_from(0xFF),
            Err(HvacDriverError::InvalidFrame)
        );
    }

    #[test]
    fn encode_set_packet_setpoint_clamping() {
        // Arrange
        let too_low = HvacPatch {
            target_temp: Some(TemperatureCentiC(1200)), // 12 °C -> clamped to 16 °C (32 half-degrees)
            ..Default::default()
        };
        let too_high = HvacPatch {
            target_temp: Some(TemperatureCentiC(3500)), // 35 °C -> clamped to 31 °C (62 half-degrees)
            ..Default::default()
        };
        let codec_state = Cn105CodecState {
            temp_mode: true,
            wide_vane_adjustment: false,
        };

        // Act
        let pkt_low = encode_set_packet(&too_low, &codec_state);
        let pkt_high = encode_set_packet(&too_high, &codec_state);

        // Assert
        assert_eq!(pkt_low[5 + 14], 32 | 0x80); // 16 °C in half-degrees = 32
        assert_eq!(pkt_high[5 + 14], 62 | 0x80); // 31 °C in half-degrees = 62
        assert_eq!(verify_frame(&pkt_low), Ok(()));
        assert_eq!(verify_frame(&pkt_high), Ok(()));
    }

    #[test]
    fn encode_set_packet_empty_patch_produces_valid_frame() {
        // Arrange
        let patch = HvacPatch::default();
        let codec_state = Cn105CodecState::new();

        // Act
        let packet = encode_set_packet(&patch, &codec_state);

        // Assert
        assert_eq!(packet[5 + 1], 0x00); // Bitmask 1 empty
        assert_eq!(packet[5 + 2], 0x00); // Bitmask 2 empty
        assert_eq!(verify_frame(&packet), Ok(()));
    }

    #[test]
    fn decode_0x62_settings_all_fan_speeds_and_vanes() {
        // Arrange
        let fan_cases = [
            (0x00, FanSpeed::Auto),
            (0x01, FanSpeed::Quiet),
            (0x02, FanSpeed::Speed1),
            (0x03, FanSpeed::Speed2),
            (0x05, FanSpeed::Speed3),
            (0x06, FanSpeed::Speed4),
        ];

        for (byte_val, expected_speed) in fan_cases {
            let mut payload = [0u8; 16];
            payload[0] = SUBTYPE_SETTINGS;
            payload[6] = byte_val;
            let packet = make_response_frame(payload);
            let mut state = HvacState::default();
            let mut codec_state = Cn105CodecState::new();

            // Act
            let res = decode_response_packet(&packet, &mut state, &mut codec_state);

            // Assert
            assert_eq!(res, Ok(GetPacketType::Settings));
            assert_eq!(state.fan_speed, expected_speed);
        }

        let vane_v_cases = [
            (0x00, VaneVertical::Auto),
            (0x01, VaneVertical::Pos1),
            (0x02, VaneVertical::Pos2),
            (0x03, VaneVertical::Pos3),
            (0x04, VaneVertical::Pos4),
            (0x05, VaneVertical::Pos5),
            (0x07, VaneVertical::Swing),
        ];

        for (byte_val, expected_vane) in vane_v_cases {
            let mut payload = [0u8; 16];
            payload[0] = SUBTYPE_SETTINGS;
            payload[7] = byte_val;
            let packet = make_response_frame(payload);
            let mut state = HvacState::default();
            let mut codec_state = Cn105CodecState::new();

            // Act
            let res = decode_response_packet(&packet, &mut state, &mut codec_state);

            // Assert
            assert_eq!(res, Ok(GetPacketType::Settings));
            assert_eq!(state.vane_vertical, expected_vane);
        }

        let vane_h_cases = [
            (0x00, VaneHorizontal::Auto),
            (0x01, VaneHorizontal::LeftMax),
            (0x02, VaneHorizontal::Left),
            (0x03, VaneHorizontal::Center),
            (0x04, VaneHorizontal::Right),
            (0x05, VaneHorizontal::RightMax),
            (0x08, VaneHorizontal::Split),
            (0x0C, VaneHorizontal::Swing),
        ];

        for (byte_val, expected_vane) in vane_h_cases {
            let mut payload = [0u8; 16];
            payload[0] = SUBTYPE_SETTINGS;
            payload[10] = byte_val;
            let packet = make_response_frame(payload);
            let mut state = HvacState::default();
            let mut codec_state = Cn105CodecState::new();

            // Act
            let res = decode_response_packet(&packet, &mut state, &mut codec_state);

            // Assert
            assert_eq!(res, Ok(GetPacketType::Settings));
            assert_eq!(state.vane_horizontal, expected_vane);
        }
    }

    #[test]
    fn decode_0x62_room_temp_zeroes_preserves_existing_value() {
        // Arrange
        let mut payload = [0u8; 16];
        payload[0] = SUBTYPE_ROOM_TEMP;
        payload[3] = 0x00;
        payload[6] = 0x00;

        let packet = make_response_frame(payload);
        let mut state = HvacState {
            room_temp: Some(TemperatureCentiC(2100)),
            ..Default::default()
        };
        let mut codec_state = Cn105CodecState::new();

        // Act
        let res = decode_response_packet(&packet, &mut state, &mut codec_state);

        // Assert
        assert_eq!(res, Ok(GetPacketType::RoomTemp));
        assert_eq!(state.room_temp, Some(TemperatureCentiC(2100)));
    }
}
