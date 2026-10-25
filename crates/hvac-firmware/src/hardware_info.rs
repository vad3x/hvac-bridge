//! Concrete hardware information provider for ESP32-C6.
//!
//! Queries runtime heap, bootloader active/valid slot, efuse MAC, timers,
//! and Wi-Fi RSSI via ESP-IDF / esp-radio runtime APIs.

use hvac_core::domain::diagnostics::WifiSsid;
use hvac_core::ports::diagnostics::HardwareInfoDriver;

/// Concrete hardware information provider querying runtime heap, bootloader, efuse, and timers.
pub struct FirmwareHardwareInfo;

impl HardwareInfoDriver for FirmwareHardwareInfo {
    fn device_type(&self) -> &'static str {
        crate::config::DEVICE_TYPE
    }

    fn uptime_secs(&self) -> u64 {
        embassy_time::Instant::now().as_secs()
    }

    fn free_heap_bytes(&self) -> usize {
        #[cfg(target_arch = "riscv32")]
        {
            esp_alloc::HEAP.free()
        }
        #[cfg(not(target_arch = "riscv32"))]
        {
            0
        }
    }

    fn total_heap_bytes(&self) -> usize {
        crate::config::HEAP_SIZE
    }

    fn wifi_ssid(&self) -> Option<WifiSsid> {
        #[cfg(target_arch = "riscv32")]
        {
            use esp_wifi_sys_esp32c6::include::{
                esp_wifi_get_config, esp_wifi_sta_get_ap_info, wifi_ap_record_t, wifi_config_t,
                wifi_interface_t_WIFI_IF_STA, ESP_OK,
            };

            // Query active AP record first
            let mut record = core::mem::MaybeUninit::<wifi_ap_record_t>::uninit();
            if unsafe { esp_wifi_sta_get_ap_info(record.as_mut_ptr()) } == ESP_OK as i32 {
                let record = unsafe { record.assume_init() };
                let len = record.ssid.iter().position(|&b| b == 0).unwrap_or(record.ssid.len());
                if len > 0 {
                    if let Some(ssid) = WifiSsid::from_bytes(&record.ssid[..len]) {
                        return Some(ssid);
                    }
                }
            }

            // Fallback to configured station SSID
            let mut cfg = core::mem::MaybeUninit::<wifi_config_t>::uninit();
            if unsafe { esp_wifi_get_config(wifi_interface_t_WIFI_IF_STA, cfg.as_mut_ptr()) } == ESP_OK as i32 {
                let cfg = unsafe { cfg.assume_init() };
                let ssid_bytes = unsafe { cfg.sta.ssid };
                let len = ssid_bytes.iter().position(|&b| b == 0).unwrap_or(ssid_bytes.len());
                if len > 0 {
                    if let Some(ssid) = WifiSsid::from_bytes(&ssid_bytes[..len]) {
                        return Some(ssid);
                    }
                }
            }
        }
        None
    }

    fn wifi_rssi(&self) -> Option<i8> {
        #[cfg(target_arch = "riscv32")]
        {
            let mut rssi: i32 = 0;
            if unsafe { esp_wifi_sta_get_rssi(&mut rssi) } == 0 {
                return Some(rssi as i8);
            }
        }
        None
    }

    fn ip_address(&self) -> Option<[u8; 4]> {
        #[cfg(target_arch = "riscv32")]
        {
            let stack = rs_matter_embassy::enet::global_stack()?;
            let config = stack.config_v4()?;
            Some(config.address.address().octets())
        }
        #[cfg(not(target_arch = "riscv32"))]
        {
            None
        }
    }

    fn mac_address(&self) -> [u8; 6] {
        #[cfg(target_arch = "riscv32")]
        {
            let mac = esp_hal::efuse::base_mac_address();
            mac.as_bytes().try_into().unwrap_or([0u8; 6])
        }
        #[cfg(not(target_arch = "riscv32"))]
        {
            [0u8; 6]
        }
    }

    fn active_slot(&self) -> &'static str {
        #[cfg(target_arch = "riscv32")]
        {
            ensure_bootloader_cached();
            match SLOT_CACHE.load(core::sync::atomic::Ordering::Relaxed) {
                2 => "ota_1",
                _ => "ota_0",
            }
        }
        #[cfg(not(target_arch = "riscv32"))]
        {
            "ota_0"
        }
    }

    fn is_slot_valid(&self) -> bool {
        #[cfg(target_arch = "riscv32")]
        {
            ensure_bootloader_cached();
            VALID_CACHE.load(core::sync::atomic::Ordering::Relaxed) == 1
        }
        #[cfg(not(target_arch = "riscv32"))]
        {
            true
        }
    }

    fn firmware_version(&self) -> &'static str {
        crate::version::SW_VER_STR
    }

    fn git_commit(&self) -> &'static str {
        crate::version::GIT_HASH
    }

    fn build_date(&self) -> &'static str {
        crate::version::BUILD_DATE
    }
}

#[cfg(target_arch = "riscv32")]
static SLOT_CACHE: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
#[cfg(target_arch = "riscv32")]
static VALID_CACHE: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

#[cfg(target_arch = "riscv32")]
fn ensure_bootloader_cached() {
    if SLOT_CACHE.load(core::sync::atomic::Ordering::Relaxed) != 0 {
        return;
    }
    match edf_ota::OtaBootloader::load() {
        Ok(bl) => {
            let slot_val = match bl.get_running_slot() {
                edf_ota::Slot::Ota0 => 1,
                edf_ota::Slot::Ota1 => 2,
            };
            SLOT_CACHE.store(slot_val, core::sync::atomic::Ordering::Relaxed);
            let valid_val = if bl.get_active().state == edf_ota::OtaState::Valid {
                1
            } else {
                2
            };
            VALID_CACHE.store(valid_val, core::sync::atomic::Ordering::Relaxed);
        }
        Err(_) => {
            SLOT_CACHE.store(1, core::sync::atomic::Ordering::Relaxed);
            VALID_CACHE.store(1, core::sync::atomic::Ordering::Relaxed);
        }
    }
}

#[cfg(target_arch = "riscv32")]
extern "C" {
    fn esp_wifi_sta_get_rssi(rssi: *mut i32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_firmware_hardware_info_version_and_heap() {
        // Arrange
        let info = FirmwareHardwareInfo;

        // Act
        let version = info.firmware_version();
        let commit = info.git_commit();
        let date = info.build_date();
        let total_heap = info.total_heap_bytes();

        // Assert
        assert!(!version.is_empty());
        assert!(!commit.is_empty());
        assert!(!date.is_empty());
        assert_eq!(total_heap, crate::config::HEAP_SIZE);
    }
}
