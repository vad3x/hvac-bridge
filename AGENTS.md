# AGENTS.md

Guidelines for AI agents working on the CN105 HVAC Matter Bridge codebase.

## Rust Usage

For Rust and Cargo commands, use `mise`. The repositories pin Rust 1.99.1 and Bun 1.3.14 in `mise.toml`; the firmware target is `riscv32imac-unknown-none-elf` for ESP32-C6.

## Project Overview

**CN105 HVAC Matter Bridge** (`hvac-bridge`) — ESP32-C6 bare-metal firmware bridging Mitsubishi Electric HVAC heat pumps and air conditioners via the CN105 serial port to smart home networks (**Matter over Wi-Fi** for Apple Home, Home Assistant, Google Home, Aqara Home) and an embedded **HTTP Web Dashboard + OTA server**.

Designed with **Hexagonal Architecture (Ports & Adapters)** to keep universal domain logic completely decoupled from hardware and protocol implementations:
1. **Single Source of Truth:** Canonical state lives in `hvac-core` and synchronizes continuously with the physical unit over UART1 (`2400` baud `8E1`).
2. **Room Air Conditioner Device Type:** Exposes Endpoint 1 as `DEV_TYPE_ROOM_AIR_CONDITIONER` (`0x0072`, rev 2) hosting `OnOff`, `Thermostat`, and `FanControl` clusters with vane swing oscillation.
3. **Dual-Bank Fail-Safe OTA:** Ed25519-signed firmware updates flashed to inactive slot with automatic bootloader rollback watchdog.

### Tech Stack

- **Target MCU:** ESP32-C6-WROOM-1-N8 (8 MB Quad SPI Flash)
- **Language:** Rust (`no_std`, bare-metal)
- **Architecture:** Hexagonal Architecture (Ports & Adapters) Virtual Workspace
- **Async Runtime:** Embassy (replaces FreeRTOS)
- **HAL:** `esp-hal` + `esp-radio` (WiFi/BLE)
- **Matter:** `rs-matter` + `rs-matter-embassy` (bare-metal Matter over WiFi)
- **HVAC Protocol:** Mitsubishi Electric CN105 (packet framing, checksum, connect handshake, 0x42 polling, 0x41 mutations)
- **OTA Updates:** HTTP streaming over Wi-Fi (port 8080) with Ed25519 signature verification and dual-bank bootloader rollback

---

## Matter Guidelines

### Critical: Always Include `system;` in `with_attrs`
When defining a cluster with `with_attrs(with!(...))`, **always prefix the attribute list with `system;`**:

```rust
pub const CLUSTER: Cluster<'static> = fan_control::FULL_CLUSTER
    .with_revision(5)
    .with_features(
        fan_control::Feature::MULTI_SPEED.bits()
            | fan_control::Feature::AUTO.bits()
            | fan_control::Feature::ROCKING.bits(),
    )
    .with_attrs(with!(
        system;
        fan_control::AttributeId::FanMode
            | fan_control::AttributeId::FanModeSequence
            | fan_control::AttributeId::PercentSetting
            | fan_control::AttributeId::PercentCurrent
            | fan_control::AttributeId::SpeedMax
            | fan_control::AttributeId::SpeedSetting
            | fan_control::AttributeId::SpeedCurrent
            | fan_control::AttributeId::RockSupport
            | fan_control::AttributeId::RockSetting
    ))
    .with_cmds(with!());
```

**Why this matters:**
- Without `system;`, `rs-matter`'s `with!` macro filters out all global system attributes (`0xFFF8` - `0xFFFD`).
- This causes `AttributeList` (`0xFFFB`), `FeatureMap` (`0xFFFC`), `AcceptedCommandList` (`0xFFF9`), and `GeneratedCommandList` (`0xFFF8`) to return `AttributeNotFound`.
- Strict ecosystem controllers like **Aqara Hub M3** query `FeatureMap` and `AttributeList` during interview. If `FeatureMap` is missing or 0, Aqara rejects the cluster as invalid and will silently hide the device component from the UI.
- Ensure device type revisions also match standard spec requirements (e.g. `RoomAirConditioner` is `dtype: 0x0072, drev: 2`).

---

## Workspace Structure

```text
hvac-bridge/
├── crates/
│   ├── hvac-core/            # Vendor-agnostic HVAC domain, ports, & services (no_std, host-testable)
│   ├── hvac-adapter-cn105/   # Mitsubishi CN105 packet codec, polling state machine, & ESP32-C6 UART1 driver
│   ├── hvac-adapter-gpio/    # Board-level GPIO & flash drivers: Status LED, Boot Button, EspFlash
│   ├── hvac-adapter-matter/  # Matter over Wi-Fi stack: Endpoint 1 (Thermostat) + Endpoint 2 (FanControl)
│   ├── hvac-adapter-http/    # Pre-gzipped Web Dashboard & REST API on port 8080
│   ├── hvac-pairing-tool/    # Host CLI (`cargo qr`) for Matter QR & manual pairing codes
│   └── hvac-firmware/        # Binary composition root (`[[bin]] name = "hvac-bridge"`)
├── scripts/
│   ├── generate_keys.py      # Ed25519 signing key generator (`~/.hvac/ota_key.pem` & `ota_public_key.bin`)
│   ├── ota.py                # Multi-device mDNS discovery & OTA push CLI
│   ├── monitor.py            # Non-intrusive serial monitor over UART0
│   └── qr.py                 # Wrapper for pairing code utility
├── partitions.csv            # Dual-bank 8 MB flash layout (wifi_kvs above matter_kvs)
├── espflash.toml             # Bootloader and partition table configuration
├── rust-toolchain.toml       # Rust 1.99.1 + riscv32imac-unknown-none-elf target
└── Cargo.toml                # Virtual workspace manifest
```

---

## Build, Test & Flash

```bash
# Run host-capable tests on macOS without hardware
mise exec -- cargo test -p hvac-core -p hvac-adapter-cn105 -p hvac-adapter-gpio -p hvac-adapter-matter -p hvac-adapter-http -p hvac-pairing-tool --target x86_64-apple-darwin

# Build debug firmware binary (hvac-bridge)
mise exec -- cargo build -p hvac-firmware

# Build optimized release firmware binary (recommended for deployment & OTA)
mise exec -- cargo build -p hvac-firmware --release

# Flash directly via USB-to-UART adapter (UART0: GPIO16 PROG_TX, GPIO17 PROG_RX)
espflash flash target/riscv32imac-unknown-none-elf/release/hvac-bridge --port /dev/cu.usbserial* --partition-table partitions.csv --erase-parts otadata
```

### Manual ROM Bootloader Entry

Because board USB pins (`GPIO12`/`GPIO13`) are unconnected (`NC`) and `J2` uses a 4-pin UART without auto-reset lines:
1. Hold the physical **`BOOT`** button (`GPIO9`).
2. Press and release the physical **`RESET`** button (`EN`).
3. Release the **`BOOT`** button.
4. Run `espflash flash ...`.

---

## Viewing Commissioning QR & Pairing Codes

Display the Matter commissioning QR code and manual pairing codes:

```bash
# Auto-detect connected serial device and show QR code + pairing codes:
cargo qr

# Or manually specify a MAC address without serial connection:
cargo qr --mac 98:a3:16:a6:78:44

# Or output raw JSON format:
cargo qr --json
```

---

## Frontend Web Dashboard (Solid.js + DaisyUI + Bun)

The embedded HTTP server serves a single-file dashboard (`assets/dashboard.html`). Root cargo aliases run all frontend tasks seamlessly from anywhere in the repository:

```bash
# Start local hot-reloading dev server (proxies /api to test board):
cargo fe-dev      # or `cargo fe dev`

# Run nonmutating TypeScript type checking and Biome linting:
cargo fe-check    # or `cargo fe check`

# Apply frontend formatting explicitly from the frontend directory:
bun run format

# Build single-file production asset for firmware (assets/dashboard.html):
cargo fe-build    # or `cargo fe build`

# Install frontend dependencies:
cargo fe-install  # or `cargo fe install`
```

---

## Remote Firmware Updates (OTA over Wi-Fi)

The CN105 HVAC Bridge supports secure, wireless Over-The-Air firmware updates over the local Wi-Fi network without physical access:

### 1. Generate Signing Key (One-Time Setup)

```bash
python3 scripts/generate_keys.py
```

The private key is stored in `~/.hvac/ota_key.pem` (mode `0600`). The matching 32-byte public key is written to the gitignored `crates/hvac-firmware/ota_public_key.bin` and loaded at compile time by `crates/hvac-firmware/src/config.rs`.

### 2. Build Release Firmware

```bash
mise exec -- cargo build -p hvac-firmware --release
```

### 3. Push OTA Update via Wi-Fi

```bash
# Auto-discover devices via mDNS and prompt for target:
python3 scripts/ota.py

# Or push directly to a specific IP:
python3 scripts/ota.py --target 192.168.1.150
```
