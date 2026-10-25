//! Asynchronous HTTP Web and REST API Server
//!
//! Provides an Embassy-compatible HTTP server serving the pre-compressed web dashboard
//! and REST API endpoints for telemetry, domain actions, and firmware OTA flashing.

use core::fmt::Write;
use edf_http_diagnostics::{handle_diagnostic_route, DiagnosticRoute};
use edf_http_ota::{
    complete_ota_response, handle_ota_flash, OtaPolicy, PrefixedStream, MAX_CONTENT_LENGTH,
    MIN_CONTENT_LENGTH,
};
use embassy_net::tcp::TcpSocket;
use embassy_net::Stack;
use embassy_time::{Duration, Timer};
use hvac_core::domain::ecosystem::{EcosystemId, EcosystemType};
use hvac_core::events::SystemStateSender;
use hvac_core::ports::diagnostics::HardwareInfoDriver;
use hvac_core::ports::ecosystem::EcosystemInfoDriver;
use hvac_core::ports::inbound::HvacControllerHandle;
use hvac_core::ports::ota::{OtaDriverError, OtaFlashDriver, OtaStream};
use hvac_core::ports::{ResetScope, SystemResetDriver};
use hvac_core::service::DiagnosticsService;
use log::{info, warn};

use crate::config::{
    HttpServerConfig, HTTP_BUFFER_SIZE, JSON_BUFFER_SIZE, LOG_BUFFER_SIZE, TCP_RX_BUFFER_SIZE,
    TCP_TX_BUFFER_SIZE,
};
use crate::router::{
    parse_route, shared_snapshot, BufferWriter, Route, HTTP_200_JSON_OK, HTTP_400_BAD_REQUEST,
    HTTP_404_NOT_FOUND,
};
use crate::DASHBOARD_HTML_GZ;

// ============================================================================
// Main Public Server Struct & Implementation
// ============================================================================

/// Asynchronous HTTP Server with injected core services.
///
/// Binds a single-connection TCP socket on the configured port (default 8080).
/// Dispatches incoming requests to the pre-compressed dashboard HTML,
/// system diagnostics snapshots, live logs, HVAC controller actions,
/// and streaming OTA firmware updates.
pub struct HttpServer<
    'a,
    H: HardwareInfoDriver,
    E: EcosystemInfoDriver,
    O: OtaFlashDriver,
    R: SystemResetDriver = (),
    T: EcosystemType = EcosystemId,
> {
    stack: Option<Stack<'static>>,
    hvac_handle: HvacControllerHandle<'a>,
    diagnostics: &'a DiagnosticsService<'a, H, E, T>,
    ota_driver: &'a mut O,
    reset_driver: Option<&'a R>,
    state_sender: Option<SystemStateSender<'a>>,
    log_provider: fn(&mut [u8]) -> usize,
    config: HttpServerConfig,
}

impl<'a, H: HardwareInfoDriver, E: EcosystemInfoDriver, O: OtaFlashDriver, T: EcosystemType>
    HttpServer<'a, H, E, O, (), T>
{
    /// Create an unbound `HttpServer` without a network stack or reset driver, suitable for testing.
    pub fn new_unbound(
        hvac_handle: HvacControllerHandle<'a>,
        diagnostics: &'a DiagnosticsService<'a, H, E, T>,
        ota_driver: &'a mut O,
        log_provider: fn(&mut [u8]) -> usize,
        mut config: HttpServerConfig,
    ) -> Self {
        config.tarpit_delay = Duration::from_millis(0);
        Self {
            stack: None,
            hvac_handle,
            diagnostics,
            ota_driver,
            reset_driver: None,
            state_sender: None,
            log_provider,
            config,
        }
    }
}

impl<
        'a,
        H: HardwareInfoDriver,
        E: EcosystemInfoDriver,
        O: OtaFlashDriver,
        R: SystemResetDriver,
        T: EcosystemType,
    > HttpServer<'a, H, E, O, R, T>
{
    /// Create a new `HttpServer` bound to an `embassy_net::Stack`.
    pub fn new(
        stack: Stack<'static>,
        hvac_handle: HvacControllerHandle<'a>,
        diagnostics: &'a DiagnosticsService<'a, H, E, T>,
        ota_driver: &'a mut O,
        reset_driver: &'a R,
        state_sender: Option<SystemStateSender<'a>>,
        log_provider: fn(&mut [u8]) -> usize,
        config: HttpServerConfig,
    ) -> Self {
        Self {
            stack: Some(stack),
            hvac_handle,
            diagnostics,
            ota_driver,
            reset_driver: Some(reset_driver),
            state_sender,
            log_provider,
            config,
        }
    }

    /// Attach a reset driver to the server (for reboot on OTA completion).
    pub fn with_reset_driver<R2: SystemResetDriver>(
        self,
        reset_driver: &'a R2,
    ) -> HttpServer<'a, H, E, O, R2, T> {
        HttpServer {
            stack: self.stack,
            hvac_handle: self.hvac_handle,
            diagnostics: self.diagnostics,
            ota_driver: self.ota_driver,
            reset_driver: Some(reset_driver),
            state_sender: self.state_sender,
            log_provider: self.log_provider,
            config: self.config,
        }
    }

    /// Attach a system state sender for broadcasting lifecycle transitions (e.g. Updating).
    pub fn with_state_sender(mut self, state_sender: SystemStateSender<'a>) -> Self {
        self.state_sender = Some(state_sender);
        self
    }

    /// Process a single incoming client connection on an abstract `OtaStream`.
    pub async fn handle_request<S: OtaStream>(&mut self, stream: &mut S) -> Result<(), ()> {
        let mut req_buf = [0u8; HTTP_BUFFER_SIZE];
        let mut total_read = 0;
        let mut header_end = None;

        while total_read < req_buf.len() {
            let n = stream
                .read(&mut req_buf[total_read..])
                .await
                .map_err(|_| ())?;
            if n == 0 {
                break;
            }
            total_read += n;
            if let Some(pos) = req_buf[..total_read]
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
            {
                header_end = Some(pos);
                break;
            }
        }

        if total_read == 0 {
            return Ok(());
        }

        let header_len = header_end.unwrap_or(total_read);
        let header_bytes = &req_buf[..header_len];
        let header_str = core::str::from_utf8(header_bytes).map_err(|_| ())?;
        let first_line = header_str.lines().next().unwrap_or("");
        let route = parse_route(first_line);

        match route {
            Route::Dashboard | Route::Status | Route::Logs => {
                let diagnostic_route = match route {
                    Route::Dashboard => DiagnosticRoute::Dashboard,
                    Route::Status => DiagnosticRoute::Status,
                    Route::Logs => DiagnosticRoute::Logs,
                    _ => return Err(()),
                };
                let diag = self.diagnostics.snapshot();
                let shared = shared_snapshot(&diag);
                let mut scratch = [0u8; LOG_BUFFER_SIZE];
                let scratch = if route == Route::Status {
                    &mut scratch[..JSON_BUFFER_SIZE]
                } else {
                    &mut scratch[..]
                };
                let response = handle_diagnostic_route(
                    diagnostic_route,
                    &shared,
                    DASHBOARD_HTML_GZ,
                    self.log_provider,
                    scratch,
                )
                .map_err(|_| ())?;
                let extra_header = match response.content_encoding {
                    Some("gzip") => Some("Content-Encoding: gzip"),
                    None => None,
                    _ => return Err(()),
                };
                let mut header_buf = [0u8; 160];
                let h_len = write_header(
                    &mut header_buf,
                    "200 OK",
                    response.content_type,
                    extra_header,
                    response.body.len(),
                )?;
                stream
                    .write_all(&header_buf[..h_len])
                    .await
                    .map_err(|_| ())?;
                stream.write_all(response.body).await.map_err(|_| ())?;
                stream.flush().await.map_err(|_| ())?;
                Ok(())
            }
            Route::ActionHvac(patch) => {
                self.hvac_handle.apply_patch(patch);
                stream.write_all(HTTP_200_JSON_OK).await.map_err(|_| ())?;
                stream.flush().await.map_err(|_| ())?;
                Ok(())
            }
            Route::ActionResetMatter => {
                if let Some(sender) = self.state_sender {
                    sender.ecosystem_reset();
                }
                stream.write_all(HTTP_200_JSON_OK).await.map_err(|_| ())?;
                stream.flush().await.map_err(|_| ())?;

                if let Some(reset) = self.reset_driver {
                    info!(
                        "Reset requested (Ecosystem); erasing credentials and rebooting in {:?}...",
                        self.config.reboot_delay
                    );
                    Timer::after(self.config.reboot_delay).await;
                    reset.erase_credentials(ResetScope::Ecosystem);
                    reset.reboot();
                }
                Ok(())
            }
            Route::ActionReboot => {
                stream.write_all(HTTP_200_JSON_OK).await.map_err(|_| ())?;
                stream.flush().await.map_err(|_| ())?;

                if let Some(reset) = self.reset_driver {
                    info!(
                        "Reboot requested; rebooting in {:?}...",
                        self.config.reboot_delay
                    );
                    Timer::after(self.config.reboot_delay).await;
                    reset.reboot();
                }
                Ok(())
            }
            Route::ActionResetFactory => {
                if let Some(sender) = self.state_sender {
                    sender.ecosystem_reset();
                }
                stream.write_all(HTTP_200_JSON_OK).await.map_err(|_| ())?;
                stream.flush().await.map_err(|_| ())?;

                if let Some(reset) = self.reset_driver {
                    info!(
                        "Reset requested (Factory); erasing credentials and rebooting in {:?}...",
                        self.config.reboot_delay
                    );
                    Timer::after(self.config.reboot_delay).await;
                    reset.erase_credentials(ResetScope::Factory);
                    reset.reboot();
                }
                Ok(())
            }
            Route::OtaFlash => {
                let content_len = parse_content_length(header_str);
                let policy = OtaPolicy {
                    max_content_length: MAX_CONTENT_LENGTH,
                    failure_delay: self.config.tarpit_delay,
                    reboot_delay: self.config.reboot_delay,
                };
                let body_offset = match header_end {
                    Some(pos) => pos + 4,
                    None => total_read,
                };
                let buffered_body = if body_offset < total_read {
                    &req_buf[body_offset..total_read]
                } else {
                    &[]
                };
                let mut prefixed_stream = PrefixedStream::new(stream, buffered_body);
                // Preserve HVAC's driver-owned validation: every parseable length is
                // forwarded to the driver, including values outside the shared policy.
                let mut driver = LegacyLengthAdapter {
                    driver: self.ota_driver,
                    content_length: content_len.unwrap_or(0),
                };
                let response = handle_ota_flash(
                    content_len.map(|len| len.clamp(MIN_CONTENT_LENGTH, MAX_CONTENT_LENGTH)),
                    &mut prefixed_stream,
                    &mut driver,
                    self.state_sender,
                    policy,
                )
                .await;
                if response.status == 200 {
                    prefixed_stream
                        .write_all(HTTP_200_JSON_OK)
                        .await
                        .map_err(|_| ())?;
                    prefixed_stream.flush().await.map_err(|_| ())?;
                    if let Some(reset) = self.reset_driver {
                        info!(
                            "OTA update committed; rebooting in {:?}...",
                            self.config.reboot_delay
                        );
                        complete_ota_response(&response, reset, policy).await;
                    }
                    Ok(())
                } else {
                    prefixed_stream
                        .write_all(HTTP_400_BAD_REQUEST)
                        .await
                        .map_err(|_| ())?;
                    prefixed_stream.flush().await.map_err(|_| ())?;
                    Err(())
                }
            }
            Route::NotFound => {
                stream.write_all(HTTP_404_NOT_FOUND).await.map_err(|_| ())?;
                stream.flush().await.map_err(|_| ())?;
                Ok(())
            }
        }
    }

    /// Run the sequential HTTP server accept loop forever.
    pub async fn run(&mut self) -> ! {
        let stack = self
            .stack
            .expect("HttpServer requires a network stack to run");
        let mut rx_buffer = [0u8; TCP_RX_BUFFER_SIZE];
        let mut tx_buffer = [0u8; TCP_TX_BUFFER_SIZE];

        info!("HTTP Server listening on TCP port {}", self.config.port);

        loop {
            let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
            socket.set_timeout(Some(self.config.socket_timeout));

            if let Err(e) = socket.accept(self.config.port).await {
                warn!("HTTP socket accept error: {:?}", e);
                Timer::after(self.config.accept_retry_delay).await;
                continue;
            }

            let mut adapter = SocketAdapter::new(&mut socket);
            let _ = self.handle_request(&mut adapter).await;
        }
    }
}

// ============================================================================
// Private Helpers & Stream Adapters
// ============================================================================

fn write_header(
    buf: &mut [u8],
    status: &str,
    content_type: &str,
    extra_header: Option<&str>,
    content_len: usize,
) -> Result<usize, ()> {
    let mut writer = BufferWriter::new(buf);
    match extra_header {
        Some(extra) => {
            write!(
                writer,
                "HTTP/1.1 {}\r\nConnection: close\r\nContent-Type: {}\r\n{}\r\nContent-Length: {}\r\n\r\n",
                status, content_type, extra, content_len
            )
            .map_err(|_| ())?;
        }
        None => {
            write!(
                writer,
                "HTTP/1.1 {}\r\nConnection: close\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n",
                status, content_type, content_len
            )
            .map_err(|_| ())?;
        }
    }
    Ok(writer.written())
}

fn parse_content_length(header_str: &str) -> Option<usize> {
    for line in header_str.lines() {
        if let Some((name, val)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                return val.trim().parse::<usize>().ok();
            }
        }
    }
    None
}

struct LegacyLengthAdapter<'a, D> {
    driver: &'a mut D,
    content_length: usize,
}

impl<D: OtaFlashDriver> OtaFlashDriver for LegacyLengthAdapter<'_, D> {
    async fn handle_flash_stream<S: OtaStream>(
        &mut self,
        stream: &mut S,
        _: usize,
    ) -> Result<(), OtaDriverError> {
        self.driver
            .handle_flash_stream(stream, self.content_length)
            .await
    }
}

struct SocketAdapter<'s, 'a> {
    socket: &'s mut TcpSocket<'a>,
}

impl<'s, 'a> SocketAdapter<'s, 'a> {
    fn new(socket: &'s mut TcpSocket<'a>) -> Self {
        Self { socket }
    }
}

impl<'s, 'a> OtaStream for SocketAdapter<'s, 'a> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        self.socket.read(buf).await.map_err(|_| ())
    }

    async fn write_all(&mut self, mut buf: &[u8]) -> Result<(), ()> {
        while !buf.is_empty() {
            let n = self.socket.write(buf).await.map_err(|_| ())?;
            if n == 0 {
                return Err(());
            }
            buf = &buf[n..];
        }
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), ()> {
        self.socket.flush().await.map_err(|_| ())
    }
}
