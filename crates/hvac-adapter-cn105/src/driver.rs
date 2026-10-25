//! Mitsubishi Electric CN105 async serial driver.
//!
//! Provides the [`Cn105Serial`] abstraction and [`Cn105Driver`], implementing
//! [`HvacUnitDriver`] with connect handshake, round-robin polling (`0x02`, `0x03`, `0x06`),
//! set command mutations (`0x41`), and automatic reconnect after consecutive errors.

use embassy_time::Duration;
use hvac_core::domain::{HvacPatch, HvacState};
use hvac_core::ports::{HvacDriverError, HvacUnitDriver};

use crate::codec::{
    decode_response_packet, encode_set_packet, Cn105CodecState, GetPacketType,
};
use crate::config::{MAX_CONSECUTIVE_TIMEOUTS, RESPONSE_TIMEOUT_MS};
use crate::frame::{
    build_connect_packet, build_get_packet, verify_frame, CMD_CONNECT_RESPONSE, CMD_GET_RESPONSE,
    CMD_SET_RESPONSE, HEADER_LEN, HEADER_MAGIC, PACKET_LEN, SUBHEADER_1, SUBHEADER_2,
};

// ============================================================================
// Public Traits & Types
// ============================================================================

/// Asynchronous serial communication port contract for communicating with a Mitsubishi CN105 bus.
#[allow(async_fn_in_trait)]
pub trait Cn105Serial {
    /// Write the entire byte buffer to the serial port.
    async fn write_all(&mut self, buf: &[u8]) -> Result<(), HvacDriverError>;

    /// Read a single byte from the serial port.
    async fn read_byte(&mut self) -> Result<u8, HvacDriverError>;

    /// Read an exact number of bytes into `buf`.
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), HvacDriverError> {
        for b in buf.iter_mut() {
            *b = self.read_byte().await?;
        }
        Ok(())
    }
}

impl<T: Cn105Serial> Cn105Serial for &mut T {
    async fn write_all(&mut self, buf: &[u8]) -> Result<(), HvacDriverError> {
        (**self).write_all(buf).await
    }

    async fn read_byte(&mut self) -> Result<u8, HvacDriverError> {
        (**self).read_byte().await
    }

    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), HvacDriverError> {
        (**self).read_exact(buf).await
    }
}

/// Mitsubishi CN105 protocol driver managing connection lifecycle, polling, and control.
pub struct Cn105Driver<S> {
    serial: S,
    state: HvacState,
    codec_state: Cn105CodecState,
    connected: bool,
    consecutive_errors: u8,
    poll_step: u8,
    response_timeout_ms: u64,
}

impl<S: Cn105Serial> Cn105Driver<S> {
    /// Create a new CN105 driver instance wrapping the given async serial port.
    pub fn new(serial: S) -> Self {
        Self {
            serial,
            state: HvacState::default(),
            codec_state: Cn105CodecState::new(),
            connected: false,
            consecutive_errors: 0,
            poll_step: 0,
            response_timeout_ms: RESPONSE_TIMEOUT_MS,
        }
    }

    /// Configure the response timeout in milliseconds for frame reception.
    pub fn with_response_timeout(mut self, timeout_ms: u64) -> Self {
        self.response_timeout_ms = timeout_ms;
        self
    }

    /// Return the currently configured response timeout in milliseconds.
    pub const fn response_timeout(&self) -> u64 {
        self.response_timeout_ms
    }

    /// Return a reference to the current cached HVAC state.
    pub const fn state(&self) -> &HvacState {
        &self.state
    }

    /// Return a reference to the dynamic codec tracking state.
    pub const fn codec_state(&self) -> &Cn105CodecState {
        &self.codec_state
    }

    /// Return whether the driver currently considers the physical unit connected.
    pub const fn is_connected(&self) -> bool {
        self.connected
    }

    /// Return the count of consecutive communication errors or timeouts.
    pub const fn consecutive_errors(&self) -> u8 {
        self.consecutive_errors
    }

    /// Return the current round-robin polling step index (0: Settings, 1: Room Temp, 2: Status).
    pub const fn poll_step(&self) -> u8 {
        self.poll_step
    }

    /// Return an immutable reference to the underlying serial transport.
    pub const fn serial(&self) -> &S {
        &self.serial
    }

    /// Return a mutable reference to the underlying serial transport.
    pub fn serial_mut(&mut self) -> &mut S {
        &mut self.serial
    }

    /// Consume the driver and return the underlying serial transport.
    pub fn into_serial(self) -> S {
        self.serial
    }

    // ========================================================================
    // Internal Helpers
    // ========================================================================

    /// Send 0x5A connect packet and await 0x7A connect response.
    async fn handshake(&mut self) -> Result<HvacState, HvacDriverError> {
        let pkt = build_connect_packet();
        if let Err(e) = self.serial.write_all(&pkt).await {
            return Err(self.handle_error(e));
        }

        let mut buf = [0u8; 32];
        let len = match self.read_frame(&mut buf).await {
            Ok(l) => l,
            Err(e) => return Err(self.handle_error(e)),
        };

        if buf[1] != CMD_CONNECT_RESPONSE {
            return Err(self.handle_error(HvacDriverError::InvalidFrame));
        }

        self.handle_success();
        self.connected = true;
        self.state.connected = true;
        self.poll_step = 0;
        log::info!("CN105 connected handshake verified (frame len: {})", len);
        Ok(self.state)
    }

    /// Send next round-robin 0x42 query, await 0x62 response, and decode state.
    async fn poll_next_query(&mut self) -> Result<HvacState, HvacDriverError> {
        let packet_type = match self.poll_step {
            0 => GetPacketType::Settings,
            1 => GetPacketType::RoomTemp,
            2 => GetPacketType::Status,
            _ => {
                self.poll_step = 0;
                GetPacketType::Settings
            }
        };

        let pkt = build_get_packet(packet_type);
        if let Err(e) = self.serial.write_all(&pkt).await {
            return Err(self.handle_error(e));
        }

        let mut buf = [0u8; 32];
        let len = match self.read_frame(&mut buf).await {
            Ok(l) => l,
            Err(e) => return Err(self.handle_error(e)),
        };

        if len != PACKET_LEN || buf[1] != CMD_GET_RESPONSE {
            return Err(self.handle_error(HvacDriverError::InvalidFrame));
        }

        if let Err(e) = decode_response_packet(&buf[..len], &mut self.state, &mut self.codec_state)
        {
            return Err(self.handle_error(e));
        }

        self.handle_success();
        self.poll_step = (self.poll_step + 1) % 3;
        self.state.connected = true;
        Ok(self.state)
    }

    /// Read a complete, verified CN105 frame from the serial port into `buf` with timeout.
    async fn read_frame(&mut self, buf: &mut [u8]) -> Result<usize, HvacDriverError> {
        let timeout = Duration::from_millis(self.response_timeout_ms);
        match embassy_time::with_timeout(timeout, Self::read_frame_raw(&mut self.serial, buf)).await {
            Ok(res) => res,
            Err(_) => Err(HvacDriverError::Timeout),
        }
    }

    /// Read and verify a raw CN105 frame from the serial port without timeout.
    async fn read_frame_raw(serial: &mut S, buf: &mut [u8]) -> Result<usize, HvacDriverError> {
        const MAX_RESYNC_BYTES: usize = 128;
        let mut skipped = 0;

        // 1. Scan for header magic byte (0xFC)
        loop {
            let b = serial.read_byte().await?;
            if b == HEADER_MAGIC {
                buf[0] = b;
                break;
            }
            skipped += 1;
            if skipped > MAX_RESYNC_BYTES {
                return Err(HvacDriverError::InvalidFrame);
            }
        }

        // 2. Read remaining header bytes (indices 1..5: cmd, sub1, sub2, payload_len)
        serial.read_exact(&mut buf[1..HEADER_LEN]).await?;

        if buf[2] != SUBHEADER_1 || buf[3] != SUBHEADER_2 {
            return Err(HvacDriverError::InvalidFrame);
        }

        let payload_len = buf[4] as usize;
        let total_len = HEADER_LEN + payload_len + 1; // header + payload + checksum

        if total_len > buf.len() {
            return Err(HvacDriverError::InvalidFrame);
        }

        // 3. Read payload and checksum
        serial.read_exact(&mut buf[HEADER_LEN..total_len]).await?;

        // 4. Verify frame framing and checksum
        verify_frame(&buf[..total_len])?;

        Ok(total_len)
    }

    /// Record a communication error and disconnect if threshold is reached.
    fn handle_error(&mut self, err: HvacDriverError) -> HvacDriverError {
        self.consecutive_errors = self.consecutive_errors.saturating_add(1);
        if self.consecutive_errors >= MAX_CONSECUTIVE_TIMEOUTS as u8 {
            if self.connected {
                log::warn!(
                    "CN105 reached {} consecutive errors, marking disconnected",
                    self.consecutive_errors
                );
            }
            self.connected = false;
            self.state.connected = false;
        }
        err
    }

    /// Reset consecutive error counter on successful communication.
    fn handle_success(&mut self) {
        self.consecutive_errors = 0;
    }
}

impl<S: Cn105Serial> HvacUnitDriver for Cn105Driver<S> {
    /// Apply a settings patch to the physical HVAC unit and return updated state.
    async fn apply_patch(&mut self, patch: &HvacPatch) -> Result<HvacState, HvacDriverError> {
        if !self.connected {
            return Err(HvacDriverError::NotConnected);
        }

        let pkt = encode_set_packet(patch, &self.codec_state);
        if let Err(e) = self.serial.write_all(&pkt).await {
            return Err(self.handle_error(e));
        }

        let mut buf = [0u8; 32];
        let len = match self.read_frame(&mut buf).await {
            Ok(l) => l,
            Err(e) => return Err(self.handle_error(e)),
        };

        if buf[1] != CMD_SET_RESPONSE {
            return Err(self.handle_error(HvacDriverError::InvalidFrame));
        }

        self.handle_success();
        self.state = self.state.with_patch(patch);
        log::debug!("CN105 apply_patch verified with 0x61 ACK (frame len: {})", len);
        Ok(self.state)
    }

    /// Perform one polling step and return the current unit state.
    async fn poll(&mut self) -> Result<HvacState, HvacDriverError> {
        if !self.connected {
            self.handshake().await
        } else {
            self.poll_next_query().await
        }
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::vec::Vec;

    use hvac_core::domain::{
        FanSpeed, HvacMode, TemperatureCentiC, VaneHorizontal, VaneVertical,
    };

    use crate::codec::{SUBTYPE_ROOM_TEMP, SUBTYPE_SETTINGS, SUBTYPE_STATUS};
    use crate::frame::{
        build_connect_response_packet, build_set_response_packet, checksum, CONNECT_LEN,
        PAYLOAD_LEN,
    };

    /// Mock serial device recording written bytes and serving queued responses or errors.
    struct MockCn105Serial {
        written: Vec<Vec<u8>>,
        read_queue: VecDeque<Result<u8, HvacDriverError>>,
        write_error: Option<HvacDriverError>,
        stall_reads: bool,
    }

    impl MockCn105Serial {
        fn new() -> Self {
            Self {
                written: Vec::new(),
                read_queue: VecDeque::new(),
                write_error: None,
                stall_reads: false,
            }
        }

        fn set_stall(&mut self, stall: bool) {
            self.stall_reads = stall;
        }

        fn push_response(&mut self, bytes: &[u8]) {
            for &b in bytes {
                self.read_queue.push_back(Ok(b));
            }
        }

        fn push_error(&mut self, err: HvacDriverError) {
            self.read_queue.push_back(Err(err));
        }

        fn last_written(&self) -> Option<&[u8]> {
            self.written.last().map(|v| v.as_slice())
        }
    }

    impl Cn105Serial for MockCn105Serial {
        async fn write_all(&mut self, buf: &[u8]) -> Result<(), HvacDriverError> {
            if let Some(err) = self.write_error {
                return Err(err);
            }
            self.written.push(buf.to_vec());
            Ok(())
        }

        async fn read_byte(&mut self) -> Result<u8, HvacDriverError> {
            if self.stall_reads {
                core::future::pending::<()>().await;
            }
            match self.read_queue.pop_front() {
                Some(Ok(b)) => Ok(b),
                Some(Err(e)) => Err(e),
                None => Err(HvacDriverError::Timeout),
            }
        }
    }

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
    fn test_connect_handshake_and_round_robin_polling() {
        futures::executor::block_on(async {
            // Arrange
            let mut serial = MockCn105Serial::new();
            // 1. Connect handshake response
            serial.push_response(&build_connect_response_packet());

            // 2. Settings response (0x02)
            let mut settings_payload = [0u8; 16];
            settings_payload[0] = SUBTYPE_SETTINGS;
            settings_payload[3] = 0x01; // Power On
            settings_payload[4] = 0x03; // Mode Cool
            settings_payload[6] = 0x05; // Fan Speed 3
            settings_payload[7] = 0x07; // Vane Vertical Swing
            settings_payload[10] = 0x8C; // Vane Horizontal Swing + wide_vane
            settings_payload[11] = 43; // 21.5 °C (half-degree mode)
            let settings_resp = make_response_frame(settings_payload);

            // 3. Room temp response (0x03)
            let mut room_payload = [0u8; 16];
            room_payload[0] = SUBTYPE_ROOM_TEMP;
            room_payload[6] = 46; // 23.0 °C (46 half-degrees)
            let room_resp = make_response_frame(room_payload);

            // 4. Status response (0x06)
            let mut status_payload = [0u8; 16];
            status_payload[0] = SUBTYPE_STATUS;
            status_payload[3] = 42; // Compressor 42 Hz
            status_payload[4] = 0x01; // Operating true
            let status_resp = make_response_frame(status_payload);

            let mut driver = Cn105Driver::new(serial);

            // Act & Assert - Step 0: Handshake connect
            assert!(!driver.is_connected());
            let state_conn = driver.poll().await.expect("connect poll should succeed");
            assert!(state_conn.connected);
            assert!(driver.is_connected());
            assert_eq!(driver.poll_step(), 0);
            assert_eq!(driver.serial().written.len(), 1);
            assert_eq!(
                driver.serial().last_written(),
                Some(build_connect_packet().as_slice())
            );

            // Act & Assert - Step 1: Settings Query (poll_step 0 -> 1)
            driver.serial_mut().push_response(&settings_resp);
            let state_settings = driver.poll().await.expect("settings poll should succeed");
            assert!(state_settings.power);
            assert_eq!(state_settings.mode, HvacMode::Cool);
            assert_eq!(state_settings.target_temp, TemperatureCentiC(2150));
            assert_eq!(state_settings.fan_speed, FanSpeed::Speed3);
            assert_eq!(state_settings.vane_vertical, VaneVertical::Swing);
            assert_eq!(state_settings.vane_horizontal, VaneHorizontal::Swing);
            assert!(state_settings.wide_vane_adjustment);
            assert!(driver.codec_state().temp_mode);
            assert_eq!(driver.poll_step(), 1);
            assert_eq!(
                driver.serial().last_written(),
                Some(build_get_packet(GetPacketType::Settings).as_slice())
            );

            // Act & Assert - Step 2: Room Temp Query (poll_step 1 -> 2)
            driver.serial_mut().push_response(&room_resp);
            let state_room = driver.poll().await.expect("room temp poll should succeed");
            assert_eq!(state_room.room_temp, Some(TemperatureCentiC(2300)));
            assert_eq!(driver.poll_step(), 2);
            assert_eq!(
                driver.serial().last_written(),
                Some(build_get_packet(GetPacketType::RoomTemp).as_slice())
            );

            // Act & Assert - Step 3: Status Query (poll_step 2 -> 0 wrap-around)
            driver.serial_mut().push_response(&status_resp);
            let state_status = driver.poll().await.expect("status poll should succeed");
            assert!(state_status.operating);
            assert_eq!(state_status.compressor_freq_hz, Some(42));
            assert_eq!(driver.poll_step(), 0);
            assert_eq!(
                driver.serial().last_written(),
                Some(build_get_packet(GetPacketType::Status).as_slice())
            );
        });
    }

    #[test]
    fn test_apply_patch_sends_0x41_and_verifies_0x61_ack() {
        futures::executor::block_on(async {
            // Arrange
            let mut serial = MockCn105Serial::new();
            serial.push_response(&build_connect_response_packet());
            let mut driver = Cn105Driver::new(serial);
            driver.poll().await.expect("handshake must succeed");

            let patch = HvacPatch {
                power: Some(true),
                mode: Some(HvacMode::Heat),
                target_temp: Some(TemperatureCentiC(2400)),
                fan_speed: Some(FanSpeed::Speed2),
                vane_vertical: Some(VaneVertical::Pos3),
                vane_horizontal: Some(VaneHorizontal::Center),
            };
            driver.serial_mut().push_response(&build_set_response_packet());

            // Act
            let state = driver.apply_patch(&patch).await.expect("apply_patch must succeed");

            // Assert
            assert!(state.power);
            assert_eq!(state.mode, HvacMode::Heat);
            assert_eq!(state.target_temp, TemperatureCentiC(2400));
            assert_eq!(state.fan_speed, FanSpeed::Speed2);
            assert_eq!(state.vane_vertical, VaneVertical::Pos3);
            assert_eq!(state.vane_horizontal, VaneHorizontal::Center);

            let last_pkt = driver.serial().last_written().expect("0x41 packet must be written");
            assert_eq!(last_pkt[0], HEADER_MAGIC);
            assert_eq!(last_pkt[1], crate::frame::CMD_SET_REQUEST);
            assert_eq!(verify_frame(last_pkt), Ok(()));
        });
    }

    #[test]
    fn test_apply_patch_fails_when_not_connected() {
        futures::executor::block_on(async {
            // Arrange
            let serial = MockCn105Serial::new();
            let mut driver = Cn105Driver::new(serial);
            let patch = HvacPatch {
                power: Some(true),
                ..Default::default()
            };

            // Act
            let result = driver.apply_patch(&patch).await;

            // Assert
            assert_eq!(result, Err(HvacDriverError::NotConnected));
            assert!(driver.serial().written.is_empty());
        });
    }

    #[test]
    fn test_reconnect_after_5_consecutive_timeouts() {
        futures::executor::block_on(async {
            // Arrange
            let mut serial = MockCn105Serial::new();
            serial.push_response(&build_connect_response_packet());
            let mut driver = Cn105Driver::new(serial);
            driver.poll().await.expect("handshake must succeed");
            assert!(driver.is_connected());

            // Act & Assert - First 4 timeouts: remains connected, increments error counter
            for i in 1..=4 {
                let res = driver.poll().await;
                assert_eq!(res, Err(HvacDriverError::Timeout));
                assert!(driver.is_connected(), "driver should still be connected after {} timeouts", i);
                assert_eq!(driver.consecutive_errors(), i);
            }

            // 5th timeout: hits MAX_CONSECUTIVE_TIMEOUTS, disconnects
            let res5 = driver.poll().await;
            assert_eq!(res5, Err(HvacDriverError::Timeout));
            assert!(!driver.is_connected(), "driver must be disconnected after 5 timeouts");
            assert!(!driver.state().connected);
            assert_eq!(driver.consecutive_errors(), 5);

            // Next poll: must attempt 0x5A connect handshake
            driver.serial_mut().push_response(&build_connect_response_packet());
            let reconn_state = driver.poll().await.expect("reconnect handshake must succeed");

            assert!(reconn_state.connected);
            assert!(driver.is_connected());
            assert_eq!(driver.consecutive_errors(), 0);
            assert_eq!(driver.poll_step(), 0);
            assert_eq!(
                driver.serial().last_written(),
                Some(build_connect_packet().as_slice())
            );
        });
    }

    #[test]
    fn test_resync_on_garbage_bytes_before_header_magic() {
        futures::executor::block_on(async {
            // Arrange
            let mut serial = MockCn105Serial::new();
            // Prepend 4 bytes of garbage before the valid connect response
            let garbage = [0x55, 0xAA, 0x12, 0x34];
            let connect_resp = build_connect_response_packet();
            let mut stream = Vec::new();
            stream.extend_from_slice(&garbage);
            stream.extend_from_slice(&connect_resp);
            serial.push_response(&stream);

            let mut driver = Cn105Driver::new(serial);

            // Act
            let state = driver.poll().await.expect("resyncing header magic must succeed");

            // Assert
            assert!(state.connected);
            assert!(driver.is_connected());
        });
    }

    #[test]
    fn test_checksum_mismatch_in_response_increments_consecutive_errors() {
        futures::executor::block_on(async {
            // Arrange
            let mut serial = MockCn105Serial::new();
            let mut corrupted_resp = build_connect_response_packet();
            corrupted_resp[CONNECT_LEN - 1] = corrupted_resp[CONNECT_LEN - 1].wrapping_add(1);
            serial.push_response(&corrupted_resp);

            let mut driver = Cn105Driver::new(serial);

            // Act
            let res = driver.poll().await;

            // Assert
            assert_eq!(res, Err(HvacDriverError::ChecksumMismatch));
            assert_eq!(driver.consecutive_errors(), 1);
            assert!(!driver.is_connected());
        });
    }

    #[test]
    fn test_apply_patch_invalid_ack_fails() {
        futures::executor::block_on(async {
            // Arrange
            let mut serial = MockCn105Serial::new();
            serial.push_response(&build_connect_response_packet());
            let mut driver = Cn105Driver::new(serial);
            driver.poll().await.expect("handshake must succeed");

            // Respond to set packet with a connect response (0x7A) instead of 0x61 ACK
            driver.serial_mut().push_response(&build_connect_response_packet());

            let patch = HvacPatch {
                power: Some(true),
                ..Default::default()
            };

            // Act
            let res = driver.apply_patch(&patch).await;

            // Assert
            assert_eq!(res, Err(HvacDriverError::InvalidFrame));
            assert_eq!(driver.consecutive_errors(), 1);
        });
    }

    #[test]
    fn test_serial_write_error_increments_consecutive_errors() {
        futures::executor::block_on(async {
            // Arrange
            let mut serial = MockCn105Serial::new();
            serial.write_error = Some(HvacDriverError::IoError);
            let mut driver = Cn105Driver::new(serial);

            // Act
            let res = driver.poll().await;

            // Assert
            assert_eq!(res, Err(HvacDriverError::IoError));
            assert_eq!(driver.consecutive_errors(), 1);
            assert!(!driver.is_connected());
        });
    }

    #[test]
    fn test_serial_read_error_increments_consecutive_errors() {
        futures::executor::block_on(async {
            // Arrange
            let mut serial = MockCn105Serial::new();
            serial.push_error(HvacDriverError::IoError);
            let mut driver = Cn105Driver::new(serial);

            // Act
            let res = driver.poll().await;

            // Assert
            assert_eq!(res, Err(HvacDriverError::IoError));
            assert_eq!(driver.consecutive_errors(), 1);
            assert!(!driver.is_connected());
        });
    }

    #[test]
    fn test_configurable_response_timeout_and_read_stall() {
        futures::executor::block_on(async {
            // Arrange
            let mut serial = MockCn105Serial::new();
            serial.set_stall(true);

            let driver = Cn105Driver::new(serial);
            assert_eq!(driver.response_timeout(), RESPONSE_TIMEOUT_MS);

            let mut driver = driver.with_response_timeout(20);
            assert_eq!(driver.response_timeout(), 20);

            // Act
            let res = driver.poll().await;

            // Assert
            assert_eq!(res, Err(HvacDriverError::Timeout));
            assert_eq!(driver.consecutive_errors(), 1);
            assert!(!driver.is_connected());
        });
    }
}


