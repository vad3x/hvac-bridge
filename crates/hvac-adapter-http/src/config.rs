//! HVAC Bridge - HTTP Adapter Configuration
//!
//! Port, timeouts, delays, and buffer sizes for the HTTP server,
//! web dashboard, and REST API.

use embassy_time::Duration;

// ============================================================================
// Public Configuration Struct & Defaults
// ============================================================================

/// Configuration for `HttpServer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpServerConfig {
    /// TCP port to listen on (default: 8080).
    pub port: u16,
    /// Delay on failed OTA transfer (tarpit to prevent brute-force DoS, default: 5s).
    pub tarpit_delay: Duration,
    /// Timeout for waiting for the first byte of a new request (default: 10s).
    pub start_read_timeout: Duration,
    /// Timeout for waiting for the first byte of a persistent keep-alive request (default: 5s).
    pub persistent_start_read_timeout: Duration,
    /// Maximum duration allowed for an entire HTTP request lifecycle (default: 120s).
    pub read_request_timeout: Duration,
    /// Timeout for writing an HTTP response (default: 10s).
    pub write_timeout: Duration,
    /// Idle timeout for an accepted TCP socket (default: 60s).
    pub socket_timeout: Duration,
    /// Delay before initiating hardware reboot after OTA commit (default: 500ms).
    pub reboot_delay: Duration,
    /// Backoff delay after a socket accept failure before re-listening (default: 100ms).
    pub accept_retry_delay: Duration,
}

impl HttpServerConfig {
    /// Create a new configuration with default timeouts and the specified TCP port.
    pub const fn new(port: u16) -> Self {
        Self {
            port,
            tarpit_delay: DEFAULT_TARPIT_DELAY,
            start_read_timeout: DEFAULT_START_READ_TIMEOUT,
            persistent_start_read_timeout: DEFAULT_PERSISTENT_START_READ_TIMEOUT,
            read_request_timeout: DEFAULT_READ_REQUEST_TIMEOUT,
            write_timeout: DEFAULT_WRITE_TIMEOUT,
            socket_timeout: DEFAULT_SOCKET_TIMEOUT,
            reboot_delay: DEFAULT_REBOOT_DELAY,
            accept_retry_delay: DEFAULT_ACCEPT_RETRY_DELAY,
        }
    }

    /// Set the tarpit delay for failed authentication or invalid payloads.
    pub const fn with_tarpit_delay(mut self, delay: Duration) -> Self {
        self.tarpit_delay = delay;
        self
    }

    /// Set the total request lifecycle timeout (including body streaming).
    pub const fn with_read_request_timeout(mut self, timeout: Duration) -> Self {
        self.read_request_timeout = timeout;
        self
    }

    /// Set the idle socket timeout.
    pub const fn with_socket_timeout(mut self, timeout: Duration) -> Self {
        self.socket_timeout = timeout;
        self
    }

    /// Set the post-OTA reboot delay.
    pub const fn with_reboot_delay(mut self, delay: Duration) -> Self {
        self.reboot_delay = delay;
        self
    }
}

impl Default for HttpServerConfig {
    fn default() -> Self {
        Self::new(DEFAULT_PORT)
    }
}

impl From<u16> for HttpServerConfig {
    fn from(port: u16) -> Self {
        Self::new(port)
    }
}

// ============================================================================
// Default Constant Values
// ============================================================================

/// Default TCP port for the HTTP server (8080).
pub const DEFAULT_PORT: u16 = 8080;

/// Default delay on failed OTA transfer (tarpit to prevent brute-force DoS).
pub const DEFAULT_TARPIT_DELAY: Duration = Duration::from_secs(5);

/// Default timeout for waiting for the first byte of a new request.
pub const DEFAULT_START_READ_TIMEOUT: Duration = Duration::from_secs(10);

/// Default timeout for waiting for the first byte of a persistent keep-alive request.
pub const DEFAULT_PERSISTENT_START_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Default maximum total duration allowed for an HTTP request lifecycle.
pub const DEFAULT_READ_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Default timeout for writing an HTTP response.
pub const DEFAULT_WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// Default idle timeout for an accepted TCP socket.
pub const DEFAULT_SOCKET_TIMEOUT: Duration = Duration::from_secs(60);

/// Delay before initiating hardware reboot after OTA commit to allow HTTP response flush.
pub const DEFAULT_REBOOT_DELAY: Duration = Duration::from_millis(500);

/// Backoff delay after a socket accept failure before re-listening.
pub const DEFAULT_ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);

// ============================================================================
// Static Buffer Sizes (Zero-Allocation Architecture)
// ============================================================================

/// Size of the TCP receive socket buffer (4 KiB).
pub const TCP_RX_BUFFER_SIZE: usize = 4096;

/// Size of the TCP transmit socket buffer (1 KiB).
pub const TCP_TX_BUFFER_SIZE: usize = 1024;

/// Size of the HTTP request parsing buffer (2 KiB).
pub const HTTP_BUFFER_SIZE: usize = 2048;

/// Size of the stack buffer used for JSON telemetry serialization (2 KiB).
pub const JSON_BUFFER_SIZE: usize = 2048;

/// Size of the temporary buffer used to fetch system logs (4 KiB).
pub const LOG_BUFFER_SIZE: usize = 4096;

/// Size of the temporary chunk buffer used when writing response streams (1 KiB).
pub const STREAM_WRITE_BUFFER_SIZE: usize = 1024;
