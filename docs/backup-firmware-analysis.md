# Reverse-Engineered Analysis of `backup_original_8mb.bin`

This document records the architecture, crate structure, flash layout, Matter data model, and domain/business logic recovered by disassembling the original 8 MB flash dump (`backup_original_8mb.bin`).

---

## 1. Binary & Build Metadata

| Property | Value |
| :--- | :--- |
| **Target MCU** | ESP32-C6-WROOM-1-N8 (`Chip ID = 0x000D`, RISC-V `riscv32imac-unknown-none-elf`) |
| **Board Identifier** | `CN105 Bridge PCB v1.0 (ESP32-C6-WROOM-1)` |
| **Rust Toolchain** | `nightly-aarch64-apple-darwin` (`rustc 1.93.1` via `mise`) |
| **Original Source Workspace** | `/Users/vorekhov/projects/personal/hvac-bridge--network-up-gate` |
| **Local Dependency Checkouts** | `/Users/vorekhov/projects/personal/rs-matter`, `/Users/vorekhov/projects/personal/rs-matter-embassy` |
| **Async Runtime & HAL** | `embassy-executor 0.9.1`, `embassy-sync 0.7.2`, `esp-hal` / `esp-rtos` / `esp-radio` (`e156b54`), `trouble-host 0.6.0`, `smoltcp 0.12.0` |

### Flash & Memory Layout in `backup_original_8mb.bin`

1. **2nd-Stage Bootloader (`0x000000`):** ESP-IDF bootloader header (`magic = 0xE9`, `chip_id = 0x000D`, entry `0x4086b91a`).
2. **Partition Table (`0x008000`):**
   - `nvs` (`type = 0x01`, `subtype = 0x02`, offset `0x009000`, size `0x006000` / 24 KiB)
   - `phy_init` (`type = 0x01`, `subtype = 0x01`, offset `0x00f000`, size `0x001000` / 4 KiB)
   - `factory` (`type = 0x00`, `subtype = 0x00`, offset `0x010000`, size `0x7f0000` / ~7.93 MiB)
3. **Application Image (`0x010000..0x286000`):**
   - ESP32-C6 image with 5 segments, entrypoint `0x42050072`:
     - **Segment 0 (`.rodata`):** Flash `0x020020..0x063f04` $\rightarrow$ Mapped DROM `0x42000020..0x42043f04` (`278,244` bytes)
     - **Segment 1 (`.data`):** Flash `0x063f0c..0x065c8c` $\rightarrow$ Mapped DRAM `0x40800000..0x40801d80` (`7,552` bytes)
     - **Segment 2 (`.iram`):** Flash `0x065c94..0x076eb0` $\rightarrow$ Mapped IRAM `0x40801d80..0x40812f9c` (`70,172` bytes)
     - **Segment 3 (`.text`):** Flash `0x080020..0x22ae34` $\rightarrow$ Mapped IROM `0x42050020..0x421fae34` (`1,748,468` bytes)
4. **Matter Persistent KV Store (`0x310000..0x318000`):**
   - `rs-matter-embassy` `SeqMapKvBlobStore` sectors storing fabric credentials, basic info, event counters, and Wi-Fi credentials (`MyHomeNetwork_IoT`).

---

## 2. Original Workspace & Crate Structure

From embedded Rust panic locations and `log` module targets in `.rodata`, the original workspace was organized into 5 crates using an **Actor + EventBus** architecture:

```text
hvac-bridge/
├── crates/
│   ├── hvac-bridge-core/                 # Domain model, EventBus pub/sub, and actors
│   │   └── src/
│   │       ├── app.rs                    # EventBus & actor orchestrator
│   │       └── actors/
│   │           ├── hvac.rs               # HvacActor (CN105 polling & command execution)
│   │           ├── hvac_bridge.rs        # HvacBridgeActor (state synchronization bridge)
│   │           ├── matter_controller.rs  # MatterControllerActor (readiness & network events)
│   │           ├── indicator.rs          # IndicatorActor (status LED state machine)
│   │           └── factory_reset.rs      # FactoryResetActor (button hold & KVS wipe)
│   ├── hvac-bridge-matter-core/          # Matter cluster handlers (no_std)
│   │   └── src/
│   │       ├── on_off.rs                 # HvacOnOff (Cluster 0x0006 on Endpoint 1)
│   │       ├── thermostat.rs             # HvacThermostat (Cluster 0x0201 on Endpoint 1)
│   │       └── fan_control.rs            # HvacFanControl (Cluster 0x0202 on Endpoint 1)
│   ├── hvac-bridge-matter-firmware/      # ESP32-C6 Matter stack runner & UDP remote logger
│   │   └── src/
│   │       ├── lib.rs                    # MatterController builder, Node descriptor, KV store
│   │       └── remote_log.rs             # UDP log forwarder (HVAC_LOG_COLLECTOR)
│   ├── hvac-bridge-mitsubishi-firmware/  # Mitsubishi Electric CN105 UART driver
│   │   └── src/
│   │       └── lib.rs                    # Frame codec, 0x5A connect, 0x42 poll, 0x41 set
│   └── hvac-bridge/                      # Binary crate (composition root & board drivers)
│       └── src/
│           ├── main.rs                   # Hardware init, actor wiring, Matter & UART setup
│           └── drivers/
│               ├── flash.rs              # ROM SPI flash wrapper for Matter KVS
│               └── gpio/
│                   ├── led.rs            # Active-low status LED driver
│                   ├── blink_engine.rs   # LED blink pattern engine
│                   └── boot_button.rs    # Active-low BOOT button driver
```

---

## 3. Matter Data Model & Apple Home Compatibility

### 3.1 Basic Information & Commissioning Identity

| Field | Value in Original Firmware |
| :--- | :--- |
| **Vendor ID (`VID`)** | `0xFFF1` (Test Vendor ID) |
| **Product ID (`PID`)** | `0x8001` |
| **Vendor Name** | `"AC Controller"` |
| **Product Name** | `"CN105 Matter Bridge"` |
| **Device Name (`NodeLabel`)** | `"Mitsubishi AC"` |
| **Hardware Version** | `1` (`"1.0"`) |
| **Software Version** | `1` (`"0.1.0"`) |
| **Serial Number** | `"CN105-00001"` |
| **Commissioning Mode** | Concurrent BLE + Wi-Fi (`run_coex`) with deterministic MAC-derived passcode & discriminator |

### 3.2 Single Composed Endpoint (`Endpoint 1` = Room Air Conditioner)

The most critical design choice for Apple Home compatibility in the original firmware is its **single-endpoint composed device model**:

- **Endpoint 0:** Root Node (`DEV_TYPE_ROOT_NODE`, `0x0016`)
- **Endpoint 1:** **Room Air Conditioner** (`dtype: 0x0072`, `drev: 2`)
  ```rust
  pub const DEV_TYPE_ROOM_AIR_CONDITIONER: DeviceType = DeviceType {
      dtype: 0x0072,
      drev: 2,
  };
  ```
- **Why Single Endpoint `0x0072`:**
  - `MatterController::register` (disassembled at `0x420db4f4..0x420db80e`) checks if an `EndpointMeta` with `id == endpoint_id` (`1`) already exists.
  - The first registered cluster creates Endpoint 1 with `Descriptor` (`0x001D`, rev 3) and `Identify` (`0x0003`, rev 6).
  - Subsequent clusters registered with `ENDPOINT_ID = 1` append their `Cluster<'static>` definition onto Endpoint 1's cluster slice without duplicating `Descriptor` or `Identify`.
  - Endpoint 1's `Descriptor::ServerList` therefore exposes all 5 clusters on a single endpoint:
    1. `0x001D` — `Descriptor` (rev 3)
    2. `0x0003` — `Identify` (rev 6)
    3. `0x0006` — `OnOff` (rev 6)
    4. `0x0201` — `Thermostat` (rev 7)
    5. `0x0202` — `FanControl` (rev 5)

---

### 3.3 Cluster 1: `HvacOnOff` (`0x0006`, rev 6)

Recovered from `hvac-bridge-matter-core/src/on_off.rs` (`.rodata` at `0x4201468c`, `.text` at `0x42099d00`):

- **Endpoint:** `1` (`DEV_TYPE_ROOM_AIR_CONDITIONER`)
- **Cluster Revision:** `6`
- **FeatureMap:** `0x0002` (`on_off::Feature::DEAD_FRONT_BEHAVIOR`)
- **Attributes:**
  - `0x0000` (`OnOff`: `bool`) — reflects `state.power`
- **Accepted Commands:**
  - `0x00` (`Off`) — sets `power = false`, bumps `dataver`, notifies `OnOff` attribute changed, sends power-off command to HVAC
  - `0x01` (`On`) — sets `power = true`, bumps `dataver`, notifies `OnOff` attribute changed, sends power-on command to HVAC
  - `0x02` (`Toggle`) — inverts `power`, bumps `dataver`, notifies `OnOff` attribute changed, sends new power state to HVAC

---

### 3.4 Cluster 2: `HvacThermostat` (`0x0201`, rev 7)

Recovered from `hvac-bridge-matter-core/src/thermostat.rs` (`.rodata` at `0x42014b5c`, `.text` at `0x4209a9c4`):

- **Endpoint:** `1` (`DEV_TYPE_ROOM_AIR_CONDITIONER`)
- **Cluster Revision:** `7`
- **FeatureMap:** `0x0003` (`thermostat::Feature::HEATING | thermostat::Feature::COOLING`)
  - **Crucial Apple Home Invariant:** `Feature::AUTO_MODE` (`0x0020`) is **not** set. Enabling `AUTO_MODE` in Matter requires `MinSetpointDeadBand` and strict deadband separation (`OccupiedHeatingSetpoint <= OccupiedCoolingSetpoint - DeadBand`), and causes Apple Home to reject the accessory if both setpoints share a single target temperature.
- **Attributes (13 application attributes + 6 global attributes):**
  1. `0x0000` `LocalTemperature` (`Nullable<i16>`): Initialized to **`2200`** (`22.0 °C`, non-null!). Updated whenever CN105 reports a valid room temperature. Never returns `null`, ensuring Apple Home always instantiates the Thermostat service even before the first CN105 poll completes.
  2. `0x0003` `AbsMinHeatSetpointLimit` (`i16`): `1000` (`10.0 °C`)
  3. `0x0004` `AbsMaxHeatSetpointLimit` (`i16`): `3100` (`31.0 °C`)
  4. `0x0005` `AbsMinCoolSetpointLimit` (`i16`): `1600` (`16.0 °C`)
  5. `0x0006` `AbsMaxCoolSetpointLimit` (`i16`): `3100` (`31.0 °C`)
  6. `0x0011` `OccupiedCoolingSetpoint` (`i16`, writable): Separate `AtomicI16` initialized to **`2400`** (`24.0 °C`)
  7. `0x0012` `OccupiedHeatingSetpoint` (`i16`, writable): Separate `AtomicI16` initialized to **`2000`** (`20.0 °C`)
  8. `0x0015` `MinHeatSetpointLimit` (`i16`): `1600` (`16.0 °C`)
  9. `0x0016` `MaxHeatSetpointLimit` (`i16`): `3100` (`31.0 °C`)
  10. `0x0017` `MinCoolSetpointLimit` (`i16`): `1600` (`16.0 °C`)
  11. `0x0018` `MaxCoolSetpointLimit` (`i16`): `3100` (`31.0 °C`)
  12. `0x001B` `ControlSequenceOfOperation` (`u8`): `4` (`CoolingAndHeating`)
  13. `0x001C` `SystemMode` (`u8`, writable): Initialized to `0` (`Off`). Because `FeatureMap == HEATING | COOLING`, reads only return `Off` (`0`), `Cool` (`3`), or `Heat` (`4`).
- **Accepted Commands:**
  - `0x00` `SetpointRaiseLower`: Adjusts `OccupiedHeatingSetpoint`, `OccupiedCoolingSetpoint`, or both by `amount * 10` centi-°C (clamped to `1600..=3100`) and dispatches `SetTargetTemp` to the HVAC unit.
- **State Synchronization (`update_state` at `0x42101f20`):**
  - When `state.power == false` $\rightarrow$ `system_mode = SystemModeEnum::Off (0)`.
  - When `state.power == true`:
    - `HvacMode::Heat` $\rightarrow$ updates `heating_setpoint = state.target_temp`, sets `system_mode = SystemModeEnum::Heat (4)`.
    - `HvacMode::Cool` $\rightarrow$ updates `cooling_setpoint = state.target_temp`, sets `system_mode = SystemModeEnum::Cool (3)`.
    - `HvacMode::Auto` $\rightarrow$ updates both `cooling_setpoint` and `heating_setpoint` to `state.target_temp`.

---

### 3.5 Cluster 3: `HvacFanControl` (`0x0202`, rev 5)

Recovered from `hvac-bridge-matter-core/src/fan_control.rs` (`.rodata` at `0x42014834`, `.text` at `0x4209a166` and `0x420c5000..0x420c5350`):

- **Endpoint:** `1` (`DEV_TYPE_ROOM_AIR_CONDITIONER`)
- **Cluster Revision:** `5`
- **FeatureMap:** `0x0007` (`MULTI_SPEED | AUTO | ROCKING`)
- **Internal Atomic State (`HvacFanControl::new` at `0x4209a166`):**
  - `auto_mode: AtomicBool` — initialized to `true` (`1`)
  - `percent: AtomicU8` — initialized to `50` (`50%`)
  - `rock_up_down: AtomicBool` — initialized to `false` (`0`)
- **Attributes (9 application attributes + 6 global attributes):**
  1. `0x0000` `FanMode` (`u8`, writable) — returns `Auto` (`5`) when `auto_mode == true`; otherwise maps `percent` (`0` $\rightarrow$ `Off`, `1..=33` $\rightarrow$ `Low`, `34..=66` $\rightarrow$ `Medium`, `67..=100` $\rightarrow$ `High`)
  2. `0x0001` `FanModeSequence` (`u8`) — `2` (`OffLowMedHighAuto`)
  3. `0x0002` `PercentSetting` (`Nullable<u8>`, writable) — **always** returns `Nullable::some(percent)` (`0..=100`, never `Nullable::none()`, even in `Auto` mode)
  4. `0x0003` `PercentCurrent` (`u8`) — `0..=100` (`percent`)
  5. `0x0004` `SpeedMax` (`u8`) — `100`
  6. `0x0005` `SpeedSetting` (`Nullable<u8>`, writable) — **always** returns `Nullable::some(percent)` (`0..=100`, never `Nullable::none()`)
  7. `0x0006` `SpeedCurrent` (`u8`) — `0..=100` (`percent`)
  8. `0x0007` `RockSupport` (`RockBitmap`) — `0x02` (`RockBitmap::ROCK_UP_DOWN`)
  9. `0x0008` `RockSetting` (`RockBitmap`, writable) — `0x02` (`ROCK_UP_DOWN`) when vertical vane is `Swing`, else `0x00`
- **Decoupled from AC Power:**
  - Writing `FanMode`, `PercentSetting`, or `SpeedSetting` only dispatches `SetFanSpeed` (`Auto`, `Quiet`, `Speed1..Speed4`) and never mutates the AC's `power` state.

---

### 3.6 Global System Attribute Filter (`rs_matter::with!(system; ...)`)

Disassembling the `Cluster::with_attrs` closures in `backup_original_8mb.bin` (`0x4210905a` for `HvacThermostat`, `0x421090de` for `HvacFanControl`, and `0x42109134` for `HvacOnOff`) revealed a critical `rs-matter` invariant required by Apple Home:

- Every cluster definition uses `with!(system; ...)`:
  - At `0x4210905a`, the filter closure first invokes `Attribute::is_system()` (`0x4210d66a`), which returns `true` for all Matter Global System Attributes (`0xFFF8` `GeneratedCommandList`, `0xFFF9` `AcceptedCommandList`, `0xFFFA` `EventList`, `0xFFFB` `AttributeList`, `0xFFFC` `FeatureMap`, `0xFFFD` `ClusterRevision`).
- **Why This Matters for Apple Home:**
  - In `rs-matter`, writing `with!(Attr1 | Attr2)` *without* the leading `system;` keyword generates a closure that attempts `attr.id.try_into()` into the cluster's local `AttributeId` enum, which fails (`false`) for `0xFFF8..=0xFFFD`.
  - Without `system;`, `rs-matter`'s wildcard read handler skips `FeatureMap` (`0xFFFC`) and `AttributeList` (`0xFFFB`) when responding to Apple Home's subscription.
  - When Apple Home (`homed`) does not receive `FeatureMap = 0x03` (`HEATING | COOLING`) and `AttributeList` on the `Thermostat` cluster (`0x0201`), it assumes cooling/heating setpoints are unsupported—displaying **`Cooling to (null)`** on the tile and rendering a blank detail screen with no temperature dial or Off/Heat/Cool mode controls.

---

## 4. Domain & Business Logic Details

### 4.1 Network-Up Gate & Cluster Readiness
The original workspace directory was named `hvac-bridge--network-up-gate`, reflecting two startup synchronization mechanisms:
1. **Network-Up Gate in `HvacActor`:**
   - Log message: `"network up -> starting AC connect loop"`
   - `HvacActor` waited for the Wi-Fi network interface to become operational before starting the UART1 CN105 `0x5A` handshake loop (`"connected to AC"`), preventing UART noise or power-rail brownouts during initial Wi-Fi RF calibration.
2. **Cluster Subscription Readiness (`MatterControllerActor`):**
   - Log messages: `"thermostat ready"`, `"fan control ready"`
   - Tracked initial attribute reads on `HvacThermostat` and `HvacFanControl` to transition the system status LED from `Connecting` to `Ready`.

### 4.2 Mitsubishi CN105 Protocol Details (`hvac-bridge-mitsubishi-firmware`)
- **Serial Configuration:** UART1 at `2400` baud, 8 data bits, Even parity, 1 stop bit (`8E1`).
- **Dual Temperature Resolution Logging:**
  - Setpoint log: `"CN105 setpoint bytes: legacy(data[5])=0x{:02x} enhanced(data[11])=0x{:02x}"`
  - Room temperature log: `"CN105 room temp bytes: legacy(data[3])=0x{:02x} enhanced(data[6])=0x{:02x}"`
  - Supports both 0.5 °C enhanced encoding (`((b - 0x80) as f32) * 0.5`) when `data[11] != 0` / `data[6] != 0` and legacy integer fallback (`31 - data[5]` for setpoint, `data[3] + 10` for room temp).
- **Supported Mutation Commands (`HvacCommand` enum):**
  - `SetMode` (Off, Heat, Dry, Cool, FanOnly, Auto)
  - `SetTargetTemp`
  - `SetFanSpeed` (Auto, Quiet, Speed 1..4)
  - `SetVane` (Vertical vane: Auto, Pos 1..5, Swing)
  - `SetWideVane` (Horizontal vane: Left..Right, Split, Swing, Auto)

### 4.3 Decommissioning & Factory Reset (`FactoryResetActor`)
- **Automatic Decommission Detection:**
  - Log message: `"All fabrics removed — device decommissioned, rebooting to re-enter commissioning mode"`
  - When all operational fabrics are removed via a Matter controller, the firmware erases the Matter KVS flash sectors (`"Erasing Matter KVS: 0x... ..0x..."`, `"Matter KVS erased successfully"`) and triggers a software reset.
- **Physical BOOT Button Reset:**
  - Log messages: `"BOOT button held for ... -> factory reset!"`, `"BOOT button released (ignored)"`
  - Holding the active-low BOOT button (`GPIO9`) beyond the factory reset threshold publishes `FactoryReset` to the `EventBus`, erases the KVS sectors, and reboots.

### 4.4 Remote UDP Log Collector (`remote_log.rs`)
- Included a network logger in `hvac-bridge-matter-firmware/src/remote_log.rs` configured at compile time via the `HVAC_LOG_COLLECTOR` environment variable (defaulting to `"homeassistant.local:9000"`).
- Streamed formatted log records over UDP once the Wi-Fi interface acquired an IPv4 lease.
