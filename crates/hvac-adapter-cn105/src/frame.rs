//! Mitsubishi Electric CN105 packet framing, checksums, and frame validation.

use hvac_core::ports::HvacDriverError;

// ============================================================================
// Public Constants
// ============================================================================

/// Sync / Header start magic byte (`0xFC`) present on every CN105 frame.
pub const HEADER_MAGIC: u8 = 0xFC;

/// Total byte length of a standard CN105 packet (22 bytes: 5 header + 16 payload + 1 checksum).
pub const PACKET_LEN: usize = 22;

/// Total byte length of a CN105 connect packet (8 bytes: 5 header + 2 payload + 1 checksum).
pub const CONNECT_LEN: usize = 8;

/// Length of the CN105 header before payload data (5 bytes: sync, cmd, sub1, sub2, len).
pub const HEADER_LEN: usize = 5;

/// Length of standard CN105 payload data (16 bytes).
pub const PAYLOAD_LEN: usize = 16;

/// Standard sub-header byte 1 (`0x01`).
pub const SUBHEADER_1: u8 = 0x01;

/// Standard sub-header byte 2 (`0x30`).
pub const SUBHEADER_2: u8 = 0x30;

/// Set request command ID (`0x41`).
pub const CMD_SET_REQUEST: u8 = 0x41;

/// Get request command ID (`0x42`).
pub const CMD_GET_REQUEST: u8 = 0x42;

/// Connect request command ID (`0x5A`).
pub const CMD_CONNECT_REQUEST: u8 = 0x5A;

/// Set response (ACK) command ID (`0x61`).
pub const CMD_SET_RESPONSE: u8 = 0x61;

/// Get response (Information) command ID (`0x62`).
pub const CMD_GET_RESPONSE: u8 = 0x62;

/// Connect response command ID (`0x7A`).
pub const CMD_CONNECT_RESPONSE: u8 = 0x7A;

// ============================================================================
// Public Functions
// ============================================================================

/// Calculate CN105 checksum for a sequence of preceding bytes.
///
/// Formula: `0xFC - sum(bytes) (mod 256)`.
pub fn checksum(bytes: &[u8]) -> u8 {
    0xFCu8.wrapping_sub(bytes.iter().copied().fold(0u8, u8::wrapping_add))
}

/// Verify that a byte slice forms a structurally valid CN105 frame with a matching checksum.
///
/// # Errors
/// - [`HvacDriverError::InvalidFrame`]: Buffer is shorter than minimum header, start byte is not
///   `0xFC`, subheaders do not match `[0x01, 0x30]`, or total length does not equal `5 + payload_len + 1`.
/// - [`HvacDriverError::ChecksumMismatch`]: Checksum byte does not match `checksum(&frame[..len - 1])`.
pub fn verify_frame(frame: &[u8]) -> Result<(), HvacDriverError> {
    if frame.len() < HEADER_LEN + 1 {
        return Err(HvacDriverError::InvalidFrame);
    }

    if frame[0] != HEADER_MAGIC || frame[2] != SUBHEADER_1 || frame[3] != SUBHEADER_2 {
        return Err(HvacDriverError::InvalidFrame);
    }

    let payload_len = frame[4] as usize;
    let expected_len = HEADER_LEN + payload_len + 1;
    if frame.len() != expected_len {
        return Err(HvacDriverError::InvalidFrame);
    }

    let expected_checksum = checksum(&frame[..frame.len() - 1]);
    if expected_checksum != frame[frame.len() - 1] {
        return Err(HvacDriverError::ChecksumMismatch);
    }

    Ok(())
}

/// Build the standard 8-byte CN105 connect packet (`0x5A`).
///
/// Packet bytes: `[0xFC, 0x5A, 0x01, 0x30, 0x02, 0xCA, 0x01, 0xA8]`.
pub const fn build_connect_packet() -> [u8; CONNECT_LEN] {
    [0xFC, 0x5A, 0x01, 0x30, 0x02, 0xCA, 0x01, 0xA8]
}

/// Build a standard 22-byte CN105 get request packet (`0x42`).
pub fn build_get_packet(packet_type: crate::codec::GetPacketType) -> [u8; PACKET_LEN] {
    build_get_packet_raw(packet_type.packet_type_code())
}

/// Build a standard 22-byte CN105 get request packet (`0x42`) with raw subtype in `data[0]`.
pub fn build_get_packet_raw(subtype: u8) -> [u8; PACKET_LEN] {
    let mut pkt = [0u8; PACKET_LEN];
    pkt[0] = HEADER_MAGIC;
    pkt[1] = CMD_GET_REQUEST;
    pkt[2] = SUBHEADER_1;
    pkt[3] = SUBHEADER_2;
    pkt[4] = PAYLOAD_LEN as u8;
    pkt[HEADER_LEN] = subtype; // data[0]
    pkt[PACKET_LEN - 1] = checksum(&pkt[..PACKET_LEN - 1]);
    pkt
}

/// Build a standard 8-byte CN105 connect response packet (`0x7A`).
pub fn build_connect_response_packet() -> [u8; CONNECT_LEN] {
    let mut pkt = [0xFC, CMD_CONNECT_RESPONSE, SUBHEADER_1, SUBHEADER_2, 0x02, 0xCA, 0x01, 0];
    pkt[CONNECT_LEN - 1] = checksum(&pkt[..CONNECT_LEN - 1]);
    pkt
}

/// Build a standard 8-byte CN105 set ACK response packet (`0x61`).
pub fn build_set_response_packet() -> [u8; CONNECT_LEN] {
    let mut pkt = [0xFC, CMD_SET_RESPONSE, SUBHEADER_1, SUBHEADER_2, 0x02, 0x00, 0x00, 0];
    pkt[CONNECT_LEN - 1] = checksum(&pkt[..CONNECT_LEN - 1]);
    pkt
}


// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::GetPacketType;

    #[test]
    fn checksum_connect_packet_calculates_expected_value() {
        // Arrange
        let bytes = [0xFC, 0x5A, 0x01, 0x30, 0x02, 0xCA, 0x01];

        // Act
        let chk = checksum(&bytes);

        // Assert
        assert_eq!(chk, 0xA8);
    }

    #[test]
    fn build_connect_packet_matches_canonical_cn105_specification() {
        // Arrange & Act
        let packet = build_connect_packet();

        // Assert
        assert_eq!(
            packet,
            [0xFC, 0x5A, 0x01, 0x30, 0x02, 0xCA, 0x01, 0xA8],
            "connect packet must match canonical CN105 handshake"
        );
        assert_eq!(
            verify_frame(&packet),
            Ok(()),
            "connect packet must pass frame verification"
        );
    }

    #[test]
    fn build_get_packet_generates_valid_frames_for_all_query_types() {
        // Arrange & Act
        let settings_pkt = build_get_packet(GetPacketType::Settings);
        let room_temp_pkt = build_get_packet(GetPacketType::RoomTemp);
        let status_pkt = build_get_packet(GetPacketType::Status);

        // Assert
        // Settings (0x02)
        assert_eq!(settings_pkt[0..6], [0xFC, 0x42, 0x01, 0x30, 0x10, 0x02]);
        assert_eq!(&settings_pkt[6..21], &[0u8; 15]);
        assert_eq!(settings_pkt[21], 0x7B);
        assert_eq!(verify_frame(&settings_pkt), Ok(()));

        // Room Temp (0x03)
        assert_eq!(room_temp_pkt[0..6], [0xFC, 0x42, 0x01, 0x30, 0x10, 0x03]);
        assert_eq!(&room_temp_pkt[6..21], &[0u8; 15]);
        assert_eq!(room_temp_pkt[21], 0x7A);
        assert_eq!(verify_frame(&room_temp_pkt), Ok(()));

        // Status (0x06)
        assert_eq!(status_pkt[0..6], [0xFC, 0x42, 0x01, 0x30, 0x10, 0x06]);
        assert_eq!(&status_pkt[6..21], &[0u8; 15]);
        assert_eq!(status_pkt[21], 0x77);
        assert_eq!(verify_frame(&status_pkt), Ok(()));
    }

    #[test]
    fn verify_frame_rejects_truncated_or_undersized_frames() {
        // Arrange
        let empty: [u8; 0] = [];
        let partial_header = [0xFC, 0x5A, 0x01];

        // Act & Assert
        assert_eq!(verify_frame(&empty), Err(HvacDriverError::InvalidFrame));
        assert_eq!(
            verify_frame(&partial_header),
            Err(HvacDriverError::InvalidFrame)
        );
    }

    #[test]
    fn verify_frame_rejects_invalid_magic_or_subheaders() {
        // Arrange
        let mut bad_magic = build_connect_packet();
        bad_magic[0] = 0xFD;

        let mut bad_sub1 = build_connect_packet();
        bad_sub1[2] = 0x02;

        let mut bad_sub2 = build_connect_packet();
        bad_sub2[3] = 0x31;

        // Act & Assert
        assert_eq!(verify_frame(&bad_magic), Err(HvacDriverError::InvalidFrame));
        assert_eq!(verify_frame(&bad_sub1), Err(HvacDriverError::InvalidFrame));
        assert_eq!(verify_frame(&bad_sub2), Err(HvacDriverError::InvalidFrame));
    }

    #[test]
    fn verify_frame_rejects_length_mismatch() {
        // Arrange
        let mut wrong_len = build_connect_packet();
        wrong_len[4] = 0x03; // Declares 3 payload bytes instead of 2

        // Act & Assert
        assert_eq!(verify_frame(&wrong_len), Err(HvacDriverError::InvalidFrame));
    }

    #[test]
    fn verify_frame_detects_checksum_mismatch() {
        // Arrange
        let mut corrupted = build_connect_packet();
        corrupted[7] = corrupted[7].wrapping_add(1);

        // Act & Assert
        assert_eq!(
            verify_frame(&corrupted),
            Err(HvacDriverError::ChecksumMismatch)
        );
    }

    #[test]
    fn build_connect_response_packet_produces_valid_frame() {
        // Arrange & Act
        let packet = build_connect_response_packet();

        // Assert
        assert_eq!(packet[0], HEADER_MAGIC);
        assert_eq!(packet[1], CMD_CONNECT_RESPONSE);
        assert_eq!(verify_frame(&packet), Ok(()));
    }

    #[test]
    fn build_set_response_packet_produces_valid_frame() {
        // Arrange & Act
        let packet = build_set_response_packet();

        // Assert
        assert_eq!(packet[0], HEADER_MAGIC);
        assert_eq!(packet[1], CMD_SET_RESPONSE);
        assert_eq!(verify_frame(&packet), Ok(()));
    }
}

