//! HVAC Bridge - Matter Controller and Multi-Endpoint Node Driver
//!
//! `MatterController` is a builder and runner for the full rs-matter-embassy WiFi
//! stack in concurrent (coex) commissioning mode on ESP32-C6.
//!
//! Multi-Endpoint Node Architecture:
//! - Endpoint 0: Root node (`DEV_TYPE_ROOT_NODE`)
//! - Endpoint 1: HVAC Thermostat (`DEV_TYPE_THERMOSTAT` 0x0301 rev 6)
//!   - Clusters: `DescHandler` (0x001D), `Identify` (0x0003), `HvacThermostat` (0x0201)
//! - Endpoint 2: HVAC Fan Control (`DEV_TYPE_FAN` 0x002B rev 4)
//!   - Clusters: `DescHandler` (0x001D), `Identify` (0x0003), `HvacFanControl` (0x0202)

#![allow(unused_imports, dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use core::pin::pin;

use edf_matter::connection::{ConnectionState, FabricPolicy};
use embassy_futures::select::select;
use embassy_time::{Duration, Timer};
use log::{info, warn};

use rs_matter::dm::clusters::basic_info::BasicInfoConfig;
use rs_matter::dm::clusters::desc::{self, ClusterHandler as _};
use rs_matter::dm::clusters::identify::{self, ClusterHandler as _};
use rs_matter::dm::{
    Async, AsyncHandler, ChainedHandler, Cluster, Dataver, DeviceType, EmptyHandler, Endpoint, Node,
};
use rs_matter::error::Error;
use rs_matter::persist::{KvBlobStore, NETWORKS_KEY};
use rs_matter::BasicCommData;

#[cfg(target_arch = "riscv32")]
use rs_matter_embassy::matter::crypto::{default_crypto, Crypto};
#[cfg(target_arch = "riscv32")]
use rs_matter_embassy::matter::dm::devices::test::{DAC_PRIVKEY, TEST_DEV_ATT};
#[cfg(target_arch = "riscv32")]
use rs_matter_embassy::matter::utils::init::InitMaybeUninit;
#[cfg(target_arch = "riscv32")]
use rs_matter_embassy::stack::rand::reseeding_csprng;
#[cfg(target_arch = "riscv32")]
use rs_matter_embassy::stack::UserTask;
#[cfg(target_arch = "riscv32")]
use rs_matter_embassy::wireless::esp::EspWifiDriver;
#[cfg(target_arch = "riscv32")]
use rs_matter_embassy::wireless::{EmbassyWifi, EmbassyWifiMatterStack};

use hvac_core::events::SystemStateSender;
use hvac_core::ports::{ResetScope, SystemResetDriver};

use crate::commissioning;
use crate::readiness::ClusterReadinessBarrier;
use crate::MatterCluster;

// ============================================================================
// Public Constants & Device Identity
// ============================================================================

/// Vendor ID (test/development — 0xFFF1)
pub const VID: u16 = 0xfff1;

/// Product ID (test/development — 0x8001)
pub const PID: u16 = 0x8001;

/// Bump allocator size for the Matter protocol stack.
pub const BUMP_SIZE: usize = 48000;

/// Vendor ID for Apple Inc. HomeKit Accessory Protocol / Home Hub (HomePod, Apple TV).
///
/// In Apple Home, the commissioning iOS device installs a primary fabric (`0x1349` / 4937),
/// then invites the resident Home Hub as an operational admin on a secondary fabric (`0x1384` / 4996).
/// When an accessory is removed from Apple Home, iOS removes its primary fabric (`4937`),
/// leaving the Home Hub fabric (`4996`) orphaned on the accessory.
pub const APPLE_HOME_HUB_VENDOR_ID: u16 = 0x1384;

const CONTROLLER_FABRIC_POLICY: FabricPolicy = FabricPolicy::new(Some(APPLE_HOME_HUB_VENDOR_ID));

/// Counter used to seed descriptor and identify cluster dataver values.
static DESC_DATAVER_COUNTER: portable_atomic::AtomicU32 = portable_atomic::AtomicU32::new(100);

// ============================================================================
// Partitioned Storage
// ============================================================================

/// Composite [`KvBlobStore`] that routes Wi-Fi credentials (`NETWORKS_KEY` = 258)
/// to a dedicated storage partition (`wifi_kvs`) and all other Matter fabrics/keys
/// to the Matter partition (`matter_kvs`).
pub use edf_matter::store::PartitionedWifiStore;

/// Create the partitioned persistence layer spanning Matter KVS and Wi-Fi KVS.
#[cfg(target_arch = "riscv32")]
pub fn create_kv_blob_store(
    matter_range: core::ops::Range<u32>,
    wifi_range: core::ops::Range<u32>,
) -> PartitionedWifiStore<
    rs_matter_embassy::persist::SeqMapKvBlobStore<hvac_adapter_gpio::flash::EspFlash>,
    rs_matter_embassy::persist::SeqMapKvBlobStore<hvac_adapter_gpio::flash::EspFlash>,
> {
    let matter_flash = hvac_adapter_gpio::flash::EspFlash::new(matter_range.clone());
    let wifi_flash = hvac_adapter_gpio::flash::EspFlash::new(wifi_range.clone());
    PartitionedWifiStore::new(
        rs_matter_embassy::persist::SeqMapKvBlobStore::new(matter_flash, matter_range),
        rs_matter_embassy::persist::SeqMapKvBlobStore::new(wifi_flash, wifi_range),
    )
}

// ============================================================================
// Configuration & Peripherals
// ============================================================================

/// Hardware peripherals required by the WiFi+BLE Matter stack.
#[cfg(target_arch = "riscv32")]
pub struct MatterPeripherals {
    pub wifi: esp_hal::peripherals::WIFI<'static>,
    pub bt: esp_hal::peripherals::BT<'static>,
    pub rng: esp_hal::peripherals::RNG<'static>,
    pub adc1: esp_hal::peripherals::ADC1<'static>,
}

#[cfg(not(target_arch = "riscv32"))]
pub struct MatterPeripherals;

/// Device identity exposed via Matter Basic Information cluster and DHCPv4.
pub struct DeviceInfo {
    pub hw_ver: u16,
    pub hw_ver_str: &'static str,
    pub sw_ver: u32,
    pub sw_ver_str: &'static str,
    pub serial_no: Option<&'static str>,
    pub hostname: Option<&'static str>,
    pub vendor_name: &'static str,
    pub product_name: &'static str,
    pub device_name: &'static str,
}

/// Configuration for the Matter controller.
pub struct MatterControllerConfig<S: KvBlobStore> {
    pub peripherals: MatterPeripherals,
    pub device_info: DeviceInfo,
    pub store: S,
    pub comm_data: Option<BasicCommData>,
}

struct EndpointMeta {
    id: u16,
    device_type: DeviceType,
    clusters: Vec<Cluster<'static>>,
}

// ============================================================================
// MatterController
// ============================================================================

/// Builder and runner for the Matter node stack.
pub struct MatterController<S: KvBlobStore, R: SystemResetDriver, H = EmptyHandler> {
    handler: H,
    endpoint_metas: Vec<EndpointMeta>,
    config: MatterControllerConfig<S>,
    state_sender: SystemStateSender<'static>,
    readiness: &'static ClusterReadinessBarrier,
    reset: R,
}

impl<S: KvBlobStore, R: SystemResetDriver> MatterController<S, R> {
    /// Create a new MatterController builder with an empty handler chain.
    pub fn new(
        config: MatterControllerConfig<S>,
        state_sender: SystemStateSender<'static>,
        readiness: &'static ClusterReadinessBarrier,
        reset: R,
    ) -> Self {
        Self {
            handler: EmptyHandler,
            endpoint_metas: Vec::new(),
            config,
            state_sender,
            readiness,
            reset,
        }
    }
}

impl<S: KvBlobStore, R: SystemResetDriver, H: AsyncHandler> MatterController<S, R, H> {
    /// Register a cluster handler on its endpoint.
    ///
    /// Automatically wires `DescHandler` (0x001D), `Identify` (0x0003), and the application
    /// cluster handler into the asynchronous dispatch chain. When multiple clusters share
    /// the same endpoint ID, merges them into a single `Endpoint` descriptor.
    pub fn register<'b, C: MatterCluster>(
        mut self,
        cluster: &'b C,
    ) -> MatterController<S, R, impl AsyncHandler + 'b>
    where
        H: 'b,
        S: 'b,
        R: 'b,
    {
        let endpoint_id = C::ENDPOINT_ID;

        let is_new_endpoint =
            if let Some(meta) = self.endpoint_metas.iter_mut().find(|m| m.id == endpoint_id) {
                meta.clusters.push(C::CLUSTER_DEF);
                false
            } else {
                self.endpoint_metas.push(EndpointMeta {
                    id: endpoint_id,
                    device_type: C::DEVICE_TYPE,
                    clusters: vec![
                        desc::DescHandler::CLUSTER,
                        identify::IdentifyHandler::<()>::CLUSTER,
                        C::CLUSTER_DEF,
                    ],
                });
                true
            };

        let desc_seed = DESC_DATAVER_COUNTER.fetch_add(1, portable_atomic::Ordering::Relaxed);
        let identify_seed = DESC_DATAVER_COUNTER.fetch_add(1, portable_atomic::Ordering::Relaxed);
        let identify_handler = identify::IdentifyHandler::<()>::new(Dataver::new(identify_seed));

        let new_handler = ChainedHandler::new_with_matcher(
            move |e, c| is_new_endpoint && e == endpoint_id && c == desc::DescHandler::CLUSTER.id,
            Async(desc::DescHandler::new(Dataver::new(desc_seed)).adapt()),
            ChainedHandler::new_with_matcher(
                move |e, c| {
                    is_new_endpoint
                        && e == endpoint_id
                        && c == identify::IdentifyHandler::<()>::CLUSTER.id
                },
                Async(identify_handler.adapt()),
                ChainedHandler::new_with_matcher(
                    move |e, c| e == endpoint_id && c == C::CLUSTER_DEF.id,
                    cluster,
                    self.handler,
                ),
            ),
        );

        MatterController::<S, R, _> {
            handler: new_handler,
            endpoint_metas: self.endpoint_metas,
            config: self.config,
            state_sender: self.state_sender,
            readiness: self.readiness,
            reset: self.reset,
        }
    }

    /// Run the Matter stack forever over WiFi (RISC-V hardware implementation).
    #[cfg(target_arch = "riscv32")]
    pub async fn run(self) {
        let config = self.config;

        let dtype_arrays: Vec<[DeviceType; 1]> = self
            .endpoint_metas
            .iter()
            .map(|meta| [meta.device_type])
            .collect();

        let mut endpoints: Vec<Endpoint> = Vec::with_capacity(1 + self.endpoint_metas.len());
        endpoints.push(EmbassyWifiMatterStack::<0, ()>::root_endpoint());
        for (i, meta) in self.endpoint_metas.iter().enumerate() {
            endpoints.push(Endpoint::new(meta.id, &dtype_arrays[i], &meta.clusters));
        }

        let node = Node {
            endpoints: &endpoints,
        };

        let mac = esp_hal::efuse::base_mac_address();
        let mac_bytes: [u8; 6] = mac.as_bytes().try_into().unwrap();

        static IDENTITY_CELL: static_cell::StaticCell<commissioning::DeviceIdentity> =
            static_cell::StaticCell::new();
        let identity = &*IDENTITY_CELL.init(
            commissioning::derive_hvac_identity(&mac_bytes).expect("valid HVAC identity prefixes"),
        );

        let comm_data = if let Some(custom_comm) = config.comm_data {
            info!(
                "Using custom commissioning parameters (discriminator: {})",
                custom_comm.discriminator
            );
            custom_comm
        } else {
            let data = identity.comm_data();
            info!(
                "Generated commissioning parameters from MAC {}: passcode {}, discriminator {}",
                mac, identity.passcode, data.discriminator
            );
            data
        };

        let serial_no: &'static str = if let Some(custom_serial) = config.device_info.serial_no {
            info!("Using custom serial number: {}", custom_serial);
            custom_serial
        } else {
            let s = identity.serial_str();
            info!("Generated serial number from MAC {}: {}", mac, s);
            s
        };

        let hostname: &'static str = if let Some(custom_hostname) = config.device_info.hostname {
            info!("Using custom DHCP hostname: {}", custom_hostname);
            custom_hostname
        } else {
            let h = identity.hostname_str();
            info!("Generated DHCP hostname from MAC {}: {}", mac, h);
            h
        };

        static DEV_DET_CELL: static_cell::StaticCell<BasicInfoConfig> =
            static_cell::StaticCell::new();
        let dev_det = DEV_DET_CELL.init(dev_det(&config.device_info, serial_no));

        static STACK_CELL: static_cell::StaticCell<EmbassyWifiMatterStack<BUMP_SIZE, ()>> =
            static_cell::StaticCell::new();
        let stack = STACK_CELL.uninit().init_with(EmbassyWifiMatterStack::init(
            dev_det,
            comm_data,
            &TEST_DEV_ATT,
        ));

        let _trng_source =
            esp_hal::rng::TrngSource::new(config.peripherals.rng, config.peripherals.adc1);
        let crypto = default_crypto(
            reseeding_csprng(esp_hal::rng::Trng::try_new().unwrap(), 1000).unwrap(),
            DAC_PRIVKEY,
        );
        let weak_rand = crypto.weak_rand().unwrap();

        let handler = ChainedHandler::new_with_matcher(
            |e, _| e == rs_matter::dm::endpoints::ROOT_ENDPOINT_ID,
            Async(EmbassyWifiMatterStack::<0, ()>::root_handler(
                &(),
                crypto.weak_rand().unwrap(),
            )),
            self.handler,
        );

        let mut kv = config.store;
        if let Err(e) = stack.startup(&crypto, &mut kv).await {
            warn!(
                "Failed to load persisted state ({:?}), resetting KV store",
                e
            );
            stack
                .reset(&crypto, (&node, &handler), &mut kv)
                .await
                .unwrap();
            stack.startup(&crypto, &mut kv).await.unwrap();
        }

        let kv = stack.matter().kv(kv);

        self.readiness.attach_matter(stack.matter());

        info!("Matter stack initialized, starting concurrent commissioning (BLE + WiFi coex)");

        let is_commissioned = has_active_controller_fabric(stack.matter());
        self.readiness.set_paired(is_commissioned);
        if is_commissioned {
            self.state_sender.wifi_connecting();
        } else {
            self.state_sender.commissioning();
        }

        let mut monitor = ConnectionMonitor {
            state_sender: self.state_sender,
            readiness: self.readiness,
            reset: self.reset,
            matter: stack.matter(),
        };

        stack
            .run_coex(
                EmbassyWifi::new(
                    EspWifiDriver::new(config.peripherals.wifi, config.peripherals.bt),
                    weak_rand,
                    true,
                    stack,
                )
                .with_hostname(hostname),
                &crypto,
                (node, handler),
                kv,
                &mut monitor,
            )
            .await
            .expect("Matter stack fatal error");
    }

    /// Host stub for `run()` (only hardware runs the wireless stack).
    #[cfg(not(target_arch = "riscv32"))]
    pub async fn run(self) {
        panic!("MatterController::run is only supported on target hardware (riscv32)");
    }
}

// ============================================================================
// ConnectionMonitor & Helpers
// ============================================================================

/// Check if the Matter node has at least one active, user-facing controller fabric.
///
/// Returns `false` if either:
/// - There are no fabrics at all (`state.fabrics` is empty).
/// - The only remaining fabric is an orphaned Apple Home Hub (`0x1384` / 4996) whose
///   controlling iOS device has already removed the accessory.
pub fn has_active_controller_fabric(matter: &rs_matter::Matter<'_>) -> bool {
    edf_matter::connection::has_active_controller_fabric(matter, CONTROLLER_FABRIC_POLICY)
}

/// Watches the network interface and signals connectivity changes.
/// Also monitors for decommissioning (primary controller fabric removed in Apple Home)
/// and triggers credential erasure and reboot.
#[cfg(target_arch = "riscv32")]
pub struct ConnectionMonitor<'a, R: SystemResetDriver> {
    pub state_sender: SystemStateSender<'static>,
    pub readiness: &'static ClusterReadinessBarrier,
    pub reset: R,
    pub matter: &'a rs_matter::Matter<'a>,
}

#[cfg(target_arch = "riscv32")]
impl<R: SystemResetDriver> UserTask for ConnectionMonitor<'_, R> {
    async fn run<S, N>(&mut self, _net_stack: S, netif: N) -> Result<(), rs_matter::error::Error>
    where
        S: rs_matter_embassy::stack::nal::NetStack,
        N: rs_matter::dm::clusters::gen_diag::NetifDiag + rs_matter::dm::networks::NetChangeNotif,
    {
        let has_any_fabrics = self.matter.has_fabrics();
        let has_primary_fabric = has_active_controller_fabric(self.matter);
        if ConnectionState::orphan_only_boot(has_any_fabrics, has_primary_fabric) {
            warn!("Device booted with only an orphaned hub fabric — triggering decommission reset");
            self.reset.erase_credentials(ResetScope::Ecosystem);
            self.reset.reboot();
        }

        let mut connection = ConnectionState::new(has_primary_fabric);

        loop {
            let mut is_operational = false;
            netif.netifs(&mut |ni| {
                if ni.operational {
                    is_operational = true;
                }
                Ok(())
            })?;

            let is_commissioned = has_active_controller_fabric(self.matter);
            self.readiness.set_paired(is_commissioned);

            let effects =
                connection.step(is_operational, is_commissioned, self.readiness.is_ready());
            if effects.reset_readiness {
                info!("network down -> reconnecting");
                self.readiness.reset();
            }
            if let Some(lifecycle) = effects.lifecycle {
                self.state_sender.send(lifecycle);
            }
            if effects.erase_ecosystem_and_reboot {
                warn!(
                    "Primary controller fabric removed — device decommissioned, triggering reset"
                );
                self.reset.erase_credentials(ResetScope::Ecosystem);
                self.reset.reboot();
            }

            let net_changed = pin!(netif.wait_changed());
            let tick = pin!(Timer::after(Duration::from_secs(3)));

            select(net_changed, tick).await;
        }
    }
}

/// Build device identity configuration for the Basic Information cluster.
#[allow(dead_code)]
fn dev_det(info: &DeviceInfo, serial_no: &'static str) -> BasicInfoConfig<'static> {
    BasicInfoConfig {
        vid: VID,
        pid: PID,
        hw_ver: info.hw_ver,
        hw_ver_str: info.hw_ver_str,
        sw_ver: info.sw_ver,
        sw_ver_str: info.sw_ver_str,
        serial_no,
        unique_id: serial_no,
        vendor_name: info.vendor_name,
        product_name: info.product_name,
        device_name: info.device_name,
        max_subscription_max_interval_secs: Some(crate::config::MAX_SUBSCRIPTION_INTERVAL_SECS),
        ..BasicInfoConfig::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partitioned_store_is_compatible_with_foundation_store() {
        let store = PartitionedWifiStore::new(MockStore::new(), MockStore::new());
        let mut shared: edf_matter::store::PartitionedWifiStore<_, _> = store;
        let mut scratch = [0; 16];
        shared.store(NETWORKS_KEY, b"wifi", &mut scratch).unwrap();
        assert_eq!(
            shared.load(NETWORKS_KEY, &mut scratch).unwrap(),
            Some(b"wifi".as_slice())
        );
    }

    #[test]
    fn controller_policy_excludes_only_apple_home_hubs() {
        assert!(!CONTROLLER_FABRIC_POLICY.has_active_controller_fabric([]));
        assert!(!CONTROLLER_FABRIC_POLICY.has_active_controller_fabric([0x1384]));
        assert!(CONTROLLER_FABRIC_POLICY.has_active_controller_fabric([0x1384, 0x1349]));
        assert!(CONTROLLER_FABRIC_POLICY.has_active_controller_fabric([0xFFF1]));
    }

    #[test]
    fn connection_policy_preserves_hvac_readiness_and_reset_transitions() {
        use hvac_core::events::{SystemState, SystemStateWatch};

        static WATCH: SystemStateWatch = SystemStateWatch::new();
        let readiness = ClusterReadinessBarrier::new(SystemStateSender::new(&WATCH));
        readiness.set_fan_required(true);
        let mut connection = ConnectionState::new(false);
        assert_eq!(
            connection.step(true, false, readiness.is_ready()).lifecycle,
            None
        );
        assert_eq!(
            connection.step(true, true, readiness.is_ready()).lifecycle,
            Some(SystemState::EcosystemConnecting)
        );
        readiness.mark_thermostat_ready();
        let down = connection.step(false, true, readiness.is_ready());
        assert_eq!(down.lifecycle, Some(SystemState::WifiConnecting));
        assert!(down.reset_readiness);
        readiness.reset();
        readiness.mark_thermostat_ready();
        readiness.mark_fan_control_ready();
        assert_eq!(
            connection.step(true, true, readiness.is_ready()).lifecycle,
            Some(SystemState::Operational)
        );
        let removed = connection.step(false, false, readiness.is_ready());
        assert_eq!(removed.lifecycle, Some(SystemState::Commissioning));
        assert!(removed.reset_readiness);
        assert!(removed.erase_ecosystem_and_reboot);
        assert!(ConnectionState::orphan_only_boot(true, false));
        assert!(!ConnectionState::orphan_only_boot(false, false));
        assert!(!ConnectionState::orphan_only_boot(true, true));
    }

    #[test]
    fn basic_info_preserves_custom_device_values_and_hvac_subscription_policy() {
        let info = DeviceInfo {
            hw_ver: 7,
            hw_ver_str: "board-seven",
            sw_ver: 42,
            sw_ver_str: "release-custom",
            serial_no: Some("custom-serial"),
            hostname: Some("custom-host"),
            vendor_name: "custom-vendor",
            product_name: "custom-product",
            device_name: "custom-device",
        };
        let details = dev_det(&info, info.serial_no.unwrap());
        assert_eq!((details.vid, details.pid), (VID, PID));
        assert_eq!((details.hw_ver, details.hw_ver_str), (7, "board-seven"));
        assert_eq!((details.sw_ver, details.sw_ver_str), (42, "release-custom"));
        assert_eq!(
            (details.serial_no, details.unique_id),
            ("custom-serial", "custom-serial")
        );
        assert_eq!(details.vendor_name, "custom-vendor");
        assert_eq!(details.product_name, "custom-product");
        assert_eq!(details.device_name, "custom-device");
        assert_eq!(details.max_subscription_max_interval_secs, Some(120));
    }

    struct MockStore {
        entries: [Option<(u16, Vec<u8>)>; 8],
    }

    impl MockStore {
        const fn new() -> Self {
            Self {
                entries: [None, None, None, None, None, None, None, None],
            }
        }
    }

    impl KvBlobStore for MockStore {
        fn load<'a>(&mut self, key: u16, buf: &'a mut [u8]) -> Result<Option<&'a [u8]>, Error> {
            for entry in &self.entries {
                if let Some((k, data)) = entry {
                    if *k == key {
                        let len = data.len();
                        buf[..len].copy_from_slice(data.as_slice());
                        return Ok(Some(&buf[..len]));
                    }
                }
            }
            Ok(None)
        }

        fn store(&mut self, key: u16, data: &[u8], _buf: &mut [u8]) -> Result<(), Error> {
            for entry in &mut self.entries {
                if let Some((k, d)) = entry {
                    if *k == key {
                        d.clear();
                        d.extend_from_slice(data);
                        return Ok(());
                    }
                }
            }
            for entry in &mut self.entries {
                if entry.is_none() {
                    let mut vec = Vec::new();
                    vec.extend_from_slice(data);
                    *entry = Some((key, vec));
                    return Ok(());
                }
            }
            Err(rs_matter::error::ErrorCode::NoMemory.into())
        }

        fn remove(&mut self, key: u16, _buf: &mut [u8]) -> Result<(), Error> {
            for entry in &mut self.entries {
                if let Some((k, _)) = entry {
                    if *k == key {
                        *entry = None;
                        return Ok(());
                    }
                }
            }
            Ok(())
        }
    }

    #[test]
    fn partitioned_store_routes_networks_key_to_wifi_store() {
        // Arrange
        let matter_store = MockStore::new();
        let wifi_store = MockStore::new();
        let mut partitioned = PartitionedWifiStore::new(matter_store, wifi_store);

        let wifi_data = b"HomeNetworkSSID";
        let matter_data = b"FabricOperationalCert";
        let mut scratch = [0u8; 64];

        // Act - store networks key (258) and a fabric key (1)
        partitioned
            .store(NETWORKS_KEY, wifi_data, &mut scratch)
            .unwrap();
        partitioned.store(1, matter_data, &mut scratch).unwrap();

        // Assert - wifi_store has NETWORKS_KEY, matter_store has key 1
        let mut load_buf = [0u8; 64];
        let loaded_wifi = partitioned
            .wifi_store
            .load(NETWORKS_KEY, &mut load_buf)
            .unwrap()
            .unwrap();
        assert_eq!(loaded_wifi, wifi_data);

        let mut load_buf2 = [0u8; 64];
        let loaded_matter = partitioned
            .matter_store
            .load(1, &mut load_buf2)
            .unwrap()
            .unwrap();
        assert_eq!(loaded_matter, matter_data);

        // Assert - cross stores do NOT contain opposite keys
        let mut empty_buf = [0u8; 64];
        assert_eq!(
            partitioned
                .matter_store
                .load(NETWORKS_KEY, &mut empty_buf)
                .unwrap(),
            None
        );
        assert_eq!(
            partitioned.wifi_store.load(1, &mut empty_buf).unwrap(),
            None
        );

        // Act - remove networks key
        partitioned.remove(NETWORKS_KEY, &mut scratch).unwrap();

        // Assert
        assert_eq!(
            partitioned
                .wifi_store
                .load(NETWORKS_KEY, &mut load_buf)
                .unwrap(),
            None
        );
        assert_eq!(
            partitioned.matter_store.load(1, &mut load_buf2).unwrap(),
            Some(matter_data.as_slice())
        );
    }
}
