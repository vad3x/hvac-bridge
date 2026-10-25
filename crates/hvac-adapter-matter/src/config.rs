//! HVAC Bridge - Matter Adapter Configuration
//!
//! Protocol-specific endpoint IDs, device serial formatting, subscription
//! interval limits, and partition offsets defined by the Matter Specification
//! and device flash partition layout.

use core::ops::Range;
use hvac_core::domain::EcosystemId;


// ============================================================================
// Ecosystem Identity
// ============================================================================

/// Strongly-typed identifier for the Matter smart home ecosystem.
pub const MATTER_ECOSYSTEM: EcosystemId = EcosystemId::new("Matter");

// ============================================================================
// Matter Endpoints & Device Types
// ============================================================================

/// Matter endpoint ID for the HVAC On/Off cluster (Endpoint 1).
pub const ON_OFF_ENDPOINT_ID: u16 = 1;

/// Matter endpoint ID for the HVAC Thermostat cluster (Endpoint 1).
pub const THERMOSTAT_ENDPOINT_ID: u16 = 1;

/// Matter endpoint ID for the HVAC Fan Control cluster (Endpoint 1).
pub const FAN_ENDPOINT_ID: u16 = 1;

/// Matter Room Air Conditioner device type (`0x0072`, revision 2).
///
/// Hosts `OnOff` (`0x0006`), `Thermostat` (`0x0201`), and `FanControl` (`0x0202`)
/// on a single composed endpoint (Endpoint 1) for native Apple Home support.
pub const DEV_TYPE_ROOM_AIR_CONDITIONER: rs_matter::dm::DeviceType = rs_matter::dm::DeviceType {
    dtype: 0x0072,
    drev: 2,
};

// ============================================================================
// Subscription Interval Policies (Matter Spec 8.5.1)
// ============================================================================

/// Maximum reporting interval granted to Matter subscriptions in seconds.
///
/// Apple Home requests 600s (10 min). Clamping this to 120s (2 min) forces the
/// Apple Home Hub to check in and re-establish CASE within 2 minutes of an ESP32 reboot.
pub const MAX_SUBSCRIPTION_INTERVAL_SECS: u16 = 120;

// ============================================================================
// Flash Partitions (partitions.csv)
// ============================================================================

/// Start offset of the `wifi_kvs` partition in flash (8 KiB = 2 sectors).
pub const WIFI_KVS_OFFSET: u32 = 0x6c0000;

/// Size of the `wifi_kvs` partition (8 KiB = 0x2000).
pub const WIFI_KVS_SIZE: u32 = 0x2000;

/// Flash address range for Wi-Fi credentials persistence.
pub const WIFI_KVS_RANGE: Range<u32> = WIFI_KVS_OFFSET..WIFI_KVS_OFFSET + WIFI_KVS_SIZE;

/// Start offset of the `matter_kvs` partition in flash (24 KiB = 6 sectors).
pub const MATTER_KVS_OFFSET: u32 = 0x6c2000;

/// Size of the `matter_kvs` partition (24 KiB = 0x6000).
pub const MATTER_KVS_SIZE: u32 = 0x6000;

/// Flash address range for Matter operational credentials persistence.
pub const MATTER_KVS_RANGE: Range<u32> = MATTER_KVS_OFFSET..MATTER_KVS_OFFSET + MATTER_KVS_SIZE;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_ids_match_matter_specification() {
        // Arrange & Act & Assert
        assert_eq!(ON_OFF_ENDPOINT_ID, 1);
        assert_eq!(THERMOSTAT_ENDPOINT_ID, 1);
        assert_eq!(FAN_ENDPOINT_ID, 1);
        assert_eq!(DEV_TYPE_ROOM_AIR_CONDITIONER.dtype, 0x0072);
        assert_eq!(DEV_TYPE_ROOM_AIR_CONDITIONER.drev, 2);
    }

    #[test]
    fn subscription_interval_clamps_to_two_minutes() {
        // Arrange & Act & Assert
        assert_eq!(MAX_SUBSCRIPTION_INTERVAL_SECS, 120);
    }

    #[test]
    fn flash_partition_ranges_are_adjacent_and_non_overlapping() {
        // Arrange & Act & Assert
        assert_eq!(WIFI_KVS_RANGE.start, 0x6c0000);
        assert_eq!(WIFI_KVS_RANGE.end, 0x6c2000);
        assert_eq!(MATTER_KVS_RANGE.start, 0x6c2000);
        assert_eq!(MATTER_KVS_RANGE.end, 0x6c8000);
        assert_eq!(WIFI_KVS_RANGE.end, MATTER_KVS_RANGE.start);
    }

    #[test]
    fn serial_and_hostname_format_correctly_from_mac() {
        // Arrange
        let mac = [0x98, 0xa3, 0x16, 0xa6, 0x78, 0x44];

        // Act
        let identity = crate::commissioning::derive_hvac_identity(&mac).unwrap();

        // Assert
        assert_eq!(identity.serial_str(), "HV-98A316A67844");
        assert_eq!(identity.hostname_str(), "hvac-bridge-7844");
    }
}
