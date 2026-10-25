//! CN105 HVAC Matter Bridge - Firmware Composition Root Configuration
//!
//! Target hardware version, product identification, and flash memory layout
//! specific to the ESP32-C6 HVAC Bridge firmware.

use core::ops::Range;
use embassy_time::Duration;

// ============================================================================
// Device identity (Basic Information cluster)
// ============================================================================

/// Hardware version string.
pub const HW_VER_STR: &str = "1.0";

/// Numeric hardware version derived from `HW_VER_STR`.
pub const HW_VER: u16 = parse_version(HW_VER_STR) as u16;

/// Vendor name shown in smart home ecosystem controllers.
pub const VENDOR_NAME: &str = "CN105 HVAC Bridge";

/// Product name shown in smart home ecosystem controllers.
pub const PRODUCT_NAME: &str = "CN105 HVAC Bridge";

/// Device name shown in smart home ecosystem controllers.
pub const DEVICE_NAME: &str = "CN105 HVAC Bridge";

/// Canonical device type identifier used for telemetry and OTA target validation.
pub const DEVICE_TYPE: &str = "hvac_bridge";

// ============================================================================
// Flash persistence & Partition Layout (from partitions.csv)
// ============================================================================

/// Start offset of the `otadata` partition in flash.
#[allow(dead_code)]
pub const OTA_DATA_OFFSET: u32 = 0x00f000;
/// Size of the `otadata` partition (8 KiB = 2 sectors).
#[allow(dead_code)]
pub const OTA_DATA_SIZE: u32 = 0x002000;

/// Start offset of `ota_0` app partition (Slot A).
#[allow(dead_code)]
pub const OTA_0_OFFSET: u32 = 0x020000;
/// Start offset of `ota_1` app partition (Slot B).
#[allow(dead_code)]
pub const OTA_1_OFFSET: u32 = 0x370000;
/// Size of each app partition (3.4 MiB = 896 sectors × 4 KiB).
#[allow(dead_code)]
pub const OTA_SLOT_SIZE: u32 = 0x350000;

/// Start offset of the `wifi_kvs` partition in flash (from partitions.csv).
/// Placed directly ABOVE `matter_kvs`!
pub const WIFI_KVS_OFFSET: u32 = 0x6c0000;

/// Size of the `wifi_kvs` partition (8 KiB = 2 sectors × 4 KiB).
pub const WIFI_KVS_SIZE: u32 = 0x2000;

/// Flash range for the dedicated Wi-Fi credentials store.
pub const WIFI_KVS_RANGE: Range<u32> =
    WIFI_KVS_OFFSET..WIFI_KVS_OFFSET + WIFI_KVS_SIZE;

/// Start offset of the `matter_kvs` partition in flash (from partitions.csv).
pub const MATTER_KVS_OFFSET: u32 = 0x6c2000;

/// Size of the `matter_kvs` partition (24 KiB = 6 sectors × 4 KiB).
pub const MATTER_KVS_SIZE: u32 = 0x6000;

/// Flash range for the Matter KV blob store.
pub const MATTER_KVS_RANGE: Range<u32> =
    MATTER_KVS_OFFSET..MATTER_KVS_OFFSET + MATTER_KVS_SIZE;

// ============================================================================
// Memory & Logging Configuration
// ============================================================================

/// Total heap size allocated to the esp-alloc runtime (120 KiB).
pub const HEAP_SIZE: usize = 120 * 1024;

/// Global in-memory log ring buffer capacity (6 KiB).
pub const LOG_BUFFER_CAPACITY: usize = 6144;

// ============================================================================
// HTTP Server & Web Dashboard Configuration
// ============================================================================

/// TCP port on which the HTTP web dashboard and REST API listen.
pub const HTTP_PORT: u16 = 8080;

/// Delay on failed OTA transfer (tarpit to prevent brute-force DoS).
pub const HTTP_TARPIT_DELAY: Duration = Duration::from_secs(5);

/// Timeout for waiting for the first byte of a new request.
pub const HTTP_START_READ_TIMEOUT: Duration = Duration::from_secs(10);

/// Timeout for waiting for the first byte of a persistent keep-alive request.
pub const HTTP_PERSISTENT_START_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Maximum duration permitted for an entire HTTP request lifecycle (including OTA firmware flash).
pub const HTTP_READ_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Timeout for writing HTTP responses.
pub const HTTP_WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// Socket-level timeout on accepted TCP connection.
pub const HTTP_SOCKET_TIMEOUT: Duration = Duration::from_secs(60);

/// Delay before initiating hardware reboot after OTA commit to allow HTTP response flush.
pub const HTTP_REBOOT_DELAY: Duration = Duration::from_millis(500);

/// Backoff delay after a socket accept failure before re-listening.
pub const HTTP_ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);

// ============================================================================
// OTA Firmware Update Security
// ============================================================================

/// Ed25519 public key used to verify incoming OTA firmware images.
/// Loaded at compile time from the gitignored `ota_public_key.bin` file.
/// If missing, generate using: `python3 scripts/generate_keys.py`
pub const OTA_PUBLIC_KEY: [u8; 32] = *include_bytes!("../ota_public_key.bin");

// ============================================================================
// Smart Home Ecosystems
// ============================================================================

#[cfg(target_arch = "riscv32")]
use hvac_adapter_matter::MATTER_ECOSYSTEM;
#[cfg(not(target_arch = "riscv32"))]
const MATTER_ECOSYSTEM: EcosystemId = EcosystemId::new("Matter");

use hvac_core::domain::{EcosystemId, EcosystemType};

/// Smart home ecosystems supported by this product configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportedEcosystem {
    Matter,
    HomeAssistant,
}

impl EcosystemType for SupportedEcosystem {
    fn id(&self) -> EcosystemId {
        match self {
            Self::Matter => MATTER_ECOSYSTEM,
            Self::HomeAssistant => EcosystemId::new("HomeAssistant"),
        }
    }
}

/// Ecosystems exposed in system diagnostics and the web dashboard.
pub const AVAILABLE_ECOSYSTEMS: &[SupportedEcosystem] = &[
    SupportedEcosystem::Matter,
    SupportedEcosystem::HomeAssistant,
];

// ============================================================================
// Private helpers
// ============================================================================

/// Parse a dotted version string (e.g. `"1.2.3"`, `"1.0"`) into a numeric
/// value at compile time: `major * 10000 + minor * 100 + patch`.
const fn parse_version(version: &str) -> u32 {
    let bytes = version.as_bytes();
    let mut i = 0;
    let mut parts = [0u32; 3];
    let mut part = 0;
    while i < bytes.len() {
        if bytes[i] == b'.' {
            part += 1;
        } else {
            parts[part] = parts[part] * 10 + (bytes[i] - b'0') as u32;
        }
        i += 1;
    }
    parts[0] * 10000 + parts[1] * 100 + parts[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_ecosystem_ids() {
        // Arrange
        let matter = SupportedEcosystem::Matter;
        let ha = SupportedEcosystem::HomeAssistant;

        // Act
        let matter_id = matter.id();
        let ha_id = ha.id();

        // Assert
        assert_eq!(matter_id.as_str(), "Matter");
        assert_eq!(ha_id.as_str(), "HomeAssistant");
    }

    #[test]
    fn test_available_ecosystems_list() {
        // Arrange
        let expected_count = 2;

        // Act
        let ecosystems = AVAILABLE_ECOSYSTEMS;

        // Assert
        assert_eq!(ecosystems.len(), expected_count);
        assert_eq!(ecosystems[0], SupportedEcosystem::Matter);
        assert_eq!(ecosystems[1], SupportedEcosystem::HomeAssistant);
    }

    #[test]
    fn test_flash_partitions_are_sector_aligned_and_non_overlapping() {
        // Arrange
        const SECTOR_SIZE: u32 = 4096;
        const FLASH_TOTAL_SIZE: u32 = 0x800000; // 8 MB

        // Act
        let wifi_offset_aligned = WIFI_KVS_OFFSET % SECTOR_SIZE == 0;
        let wifi_size_aligned = WIFI_KVS_SIZE % SECTOR_SIZE == 0;
        let matter_offset_aligned = MATTER_KVS_OFFSET % SECTOR_SIZE == 0;
        let matter_size_aligned = MATTER_KVS_SIZE % SECTOR_SIZE == 0;
        let partitions_adjacent = WIFI_KVS_OFFSET + WIFI_KVS_SIZE == MATTER_KVS_OFFSET;
        let non_overlapping = WIFI_KVS_RANGE.end <= MATTER_KVS_RANGE.start;
        let fits_in_flash = MATTER_KVS_RANGE.end <= FLASH_TOTAL_SIZE;

        // Assert
        assert!(wifi_offset_aligned);
        assert!(wifi_size_aligned);
        assert!(matter_offset_aligned);
        assert!(matter_size_aligned);
        assert!(partitions_adjacent);
        assert!(non_overlapping);
        assert!(fits_in_flash);
    }
}
