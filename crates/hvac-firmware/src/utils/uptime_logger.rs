//! HVAC composition for the shared retained uptime logger.

use edf_logger::UptimeLogger;

/// Debug/Trace suppression remains a product-owned UART output policy.
const SUPPRESSED_DEBUG_PREFIXES: &[&str] = &["rs_matter::transport::network::mdns", "trouble_host"];
const SUPPRESSED_DEBUG_EXACT: &[&str] = &[];

static LOGGER: UptimeLogger<{ crate::config::LOG_BUFFER_CAPACITY }> = UptimeLogger::new(
    uptime_millis,
    write_uart_record,
    SUPPRESSED_DEBUG_EXACT,
    SUPPRESSED_DEBUG_PREFIXES,
);

/// Installs the HVAC logger and returns any global registration failure.
pub fn init(level: log::LevelFilter) -> Result<(), log::SetLoggerError> {
    LOGGER.init(level)
}

/// Copies the newest retained log bytes in chronological order.
pub fn get_buffered_logs(buf: &mut [u8]) -> usize {
    LOGGER.get_buffered_logs(buf)
}

fn uptime_millis() -> u64 {
    #[cfg(target_arch = "riscv32")]
    {
        embassy_time::Instant::now().as_millis()
    }
    #[cfg(not(target_arch = "riscv32"))]
    {
        0
    }
}

fn write_uart_record(level: log::Level, record: core::fmt::Arguments<'_>) {
    #[cfg(target_arch = "riscv32")]
    {
        let (color, reset) = uart_colors(level);
        esp_println::println!("{color}{record}{reset}");
    }
    #[cfg(not(target_arch = "riscv32"))]
    {
        let _ = (level, record);
    }
}

#[cfg(any(target_arch = "riscv32", test))]
fn uart_colors(level: log::Level) -> (&'static str, &'static str) {
    match level {
        log::Level::Error => ("\x1b[31m", "\x1b[0m"),
        log::Level::Warn => ("\x1b[33m", "\x1b[0m"),
        _ => ("", ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Log;

    #[test]
    fn configured_logger_retains_uncolored_timestamped_records() {
        LOGGER.log(
            &log::Record::builder()
                .args(format_args!("HVAC alert"))
                .level(log::Level::Warn)
                .target("hvac")
                .build(),
        );
        let mut bytes = [0; crate::config::LOG_BUFFER_CAPACITY];
        let count = get_buffered_logs(&mut bytes);
        assert_eq!(&bytes[..count], b"00:00:00.000 WARN  hvac - HVAC alert\n");
    }

    #[test]
    fn configured_logger_suppresses_only_noisy_debug_and_trace_targets() {
        for target in [
            "rs_matter::transport::network::mdns::udp",
            "trouble_host::ble",
        ] {
            for level in [log::Level::Debug, log::Level::Trace] {
                assert!(
                    !LOGGER.enabled(&log::Metadata::builder().target(target).level(level).build())
                );
            }
            assert!(LOGGER.enabled(
                &log::Metadata::builder()
                    .target(target)
                    .level(log::Level::Info)
                    .build()
            ));
        }
        assert!(LOGGER.enabled(
            &log::Metadata::builder()
                .target("hvac_core::service")
                .level(log::Level::Debug)
                .build()
        ));
    }

    #[test]
    fn uart_colors_preserve_error_and_warning_decoration() {
        assert_eq!(uart_colors(log::Level::Error), ("\x1b[31m", "\x1b[0m"));
        assert_eq!(uart_colors(log::Level::Warn), ("\x1b[33m", "\x1b[0m"));
        for level in [log::Level::Info, log::Level::Debug, log::Level::Trace] {
            assert_eq!(uart_colors(level), ("", ""));
        }
    }
}
