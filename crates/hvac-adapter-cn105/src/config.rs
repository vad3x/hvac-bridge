//! Mitsubishi Electric CN105 protocol and UART configuration constants.

/// UART baud rate for Mitsubishi CN105 serial port (2400 bps).
pub const BAUD_RATE: u32 = 2400;

/// Default timeout for awaiting response to a request packet (ms).
pub const RESPONSE_TIMEOUT_MS: u64 = 1000;

/// Maximum consecutive response timeouts before marking unit as disconnected.
pub const MAX_CONSECUTIVE_TIMEOUTS: usize = 5;

/// Periodic polling interval between successive get queries (ms).
pub const POLL_STEP_INTERVAL_MS: u64 = 300;

/// Delay after sending initial connect packet before polling starts (ms).
pub const CONNECT_DELAY_MS: u64 = 1000;

/// Minimum delay spacing between successive packets sent to the unit (ms).
pub const INTER_PACKET_DELAY_MS: u64 = 100;

/// Default TX GPIO pin number for CN105 interface (GPIO18).
pub const DEFAULT_TX_PIN: u8 = 18;

/// Default RX GPIO pin number for CN105 interface (GPIO19).
pub const DEFAULT_RX_PIN: u8 = 19;

