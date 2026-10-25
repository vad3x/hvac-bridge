# CN105 HVAC Matter Bridge

Universal bare-metal Rust (`no_std` + Embassy async) firmware for the ESP32-C6 bridging Mitsubishi Electric HVAC heat pumps and air conditioners via the CN105 serial port to modern smart home ecosystems (**Matter over Wi-Fi** for Apple Home, Home Assistant, Google Home, and Aqara Home) and an embedded **HTTP Web Dashboard + OTA server**.

---

## Features

- ❄️ **Thermostat Cluster (Endpoint 1)** — Full control over HVAC Power, Mode (Heat, Cool, Auto, Dry, Fan), Target Temperature setpoint (`16.0 °C` to `31.0 °C` in `0.5 °C` steps), and live Room Temperature telemetry.
- 💨 **Fan Control Cluster (Endpoint 2)** — Fan speed adjustment (Auto, Quiet, Speeds 1–4) and vane oscillation toggling (vertical and horizontal swing).
- 📱 **Multi-Service Apple Home Model** — Discovered natively in Apple Home as an integrated climate tile with interactive dial and multi-speed fan controls.
- 🌐 **Pre-Gzipped Web Dashboard & REST API** — High-performance embedded web dashboard (< 5 KiB gzipped) on port 8080 with live telemetry, manual controls, and uptime logs.
- 🔒 **Dual-Bank Fail-Safe OTA** — Over-The-Air wireless firmware updates via HTTP with Ed25519 signature verification and automatic bootloader rollback.
- ⚡ **Hexagonal Architecture (Ports & Adapters)** — Core HVAC domain is completely decoupled from Mitsubishi CN105 protocol and Matter stack. Host tests execute in ~1 second without hardware.
- 🔘 **Dual-Stage Reset Button** — Hold `BOOT` button for 5–10s to wipe Matter credentials (ecosystem unpair while keeping Wi-Fi); hold > 10s for full factory reset.

---

## Architecture Overview

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                              hvac-firmware                                  │
│                 (Composition Root / Binary: hvac-bridge)                    │
└──────────────┬───────────────────────────────┬──────────────────────────────┘
               │                               │
               ▼                               ▼
┌──────────────────────────────┐ ┌──────────────────────────────┐
│     hvac-adapter-cn105       │ │    hvac-adapter-matter       │
│      (HVAC Unit Port)        │ │       (Protocol Port)        │
│  - Mitsubishi CN105 Codec    │ │  - HvacThermostat (EP 1)     │
│  - Polling State Machine     │ │  - HvacFanControl (EP 2)     │
│  - EspUartSerial (UART1 8E1) │ │  - MatterHvacReporter        │
└──────────────┬───────────────┘ └──────────────┬───────────────┘
               │                                │
               │ implements                     │ implements
               ▼                                ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                               hvac-core                                     │
│                        (Universal HVAC Domain)                              │
│                                                                             │
│  Inbound & Outbound Ports:                                                  │
│   ├── HvacControllerHandle (Inbound)    ├── HvacReporter (Outbound)         │
│   ├── HvacUnitDriver (Outbound)         ├── ResetTriggerDriver (Outbound)   │
│   ├── HardwareInfoDriver (Outbound)     └── SystemResetDriver (Outbound)    │
│   └── EcosystemInfoDriver (Outbound)                                        │
│                                                                             │
│  Application Services & Domain Core:                                        │
│   ├── HvacController (Command coalescing & polling loop)                    │
│   ├── DiagnosticsService (Point-in-time telemetry snapshots)                │
│   └── ResetService (Credential erasure & hardware reboot)                   │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Workspace Crates

| Crate | Target | Description |
|---|---|---|
| [`hvac-core`](crates/hvac-core) | Universal (`no_std`) | Vendor-agnostic HVAC domain types, commands, and port contracts. |
| [`hvac-adapter-cn105`](crates/hvac-adapter-cn105) | Universal / RISC-V | Mitsubishi CN105 protocol packet framing, checksums, polling state machine, and UART1 driver. |
| [`hvac-adapter-gpio`](crates/hvac-adapter-gpio) | Universal / RISC-V | Board-level GPIO drivers: status LED (GPIO15), BOOT button (GPIO9), and flash drivers. |
| [`hvac-adapter-matter`](crates/hvac-adapter-matter) | Universal / RISC-V | Matter over Wi-Fi stack: Endpoint 1 (Thermostat) + Endpoint 2 (FanControl). |
| [`hvac-adapter-http`](crates/hvac-adapter-http) | Universal (`no_std`) | Embedded web dashboard and REST API server on TCP port 8080. |
| [`hvac-pairing-tool`](crates/hvac-pairing-tool) | Host CLI (`cargo qr`) | Generates Matter setup QR codes and manual 11-digit / 21-digit pairing codes from device MAC. |
| [`hvac-firmware`](crates/hvac-firmware) | RISC-V (`esp32c6`) | Firmware composition root compiling the `hvac-bridge` binary. |

---

## Quick Start

### 1. Host Unit Tests

Run host-capable tests on macOS without embedded hardware. Rust 1.99.1 and Bun 1.3.14 are pinned in `mise.toml`; the firmware target is `riscv32imac-unknown-none-elf`.

```bash
mise exec -- cargo test -p hvac-core -p hvac-adapter-cn105 -p hvac-adapter-gpio -p hvac-adapter-matter -p hvac-adapter-http -p hvac-pairing-tool --target x86_64-apple-darwin
```

### 2. Pairing Tool (`cargo qr`)

Display Matter commissioning QR code and manual PIN codes:

```bash
cargo qr --mac 98:a3:16:a6:78:44
cargo qr --json
```

### 3. Build & Flash Firmware

```bash
# Build release binary:
mise exec -- cargo build -p hvac-firmware --release

# Flash over UART0:
espflash flash target/riscv32imac-unknown-none-elf/release/hvac-bridge --port /dev/cu.usbserial* --partition-table partitions.csv --erase-parts otadata
```

### 4. Over-The-Air (OTA) Updates

```bash
# Generate signing keys (one-time):
python3 scripts/generate_keys.py

# Push update over local Wi-Fi:
python3 scripts/ota.py
```

For the dashboard, `cargo fe-check` runs nonmutating type checking and linting. Run `bun run format` from `crates/hvac-adapter-http/frontend` when you want to apply formatting.
