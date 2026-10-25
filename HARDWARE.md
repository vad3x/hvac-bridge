# CN105 HVAC Matter Bridge — Hardware Specification

Target Hardware: `cn105-matter-bridge-HEAD` Rev 1.0  
Target MCU: `ESP32-C6-WROOM-1-N8` (8 MB Quad SPI Flash, 32-bit RISC-V @ 160 MHz)

---

## 1. Pin Assignment Summary

| Schematic Net | ESP32-C6 Pin | External Circuitry | Firmware Driver | Description |
|---|---|---|---|---|
| `CN105_TX` | `GPIO18` (Pin 16) | `Q1` (`2N7002` level shifter, 3.3V→5V) → `J1` Pin 5 | `hvac-adapter-cn105` | CN105 UART1 TX (`2400` baud `8E1`) |
| `CN105_RX` | `GPIO19` (Pin 17) | `Q2` (`2N7002` level shifter, 5V→3.3V) ← `J1` Pin 4 | `hvac-adapter-cn105` | CN105 UART1 RX (`2400` baud `8E1`) |
| `LED_STATUS_N` | `GPIO15` (Pin 23) | Red LED (`NCD0805R1`) + `R3` (`2kΩ`) to `+3V3` | `hvac-adapter-gpio::GpioLedDriver` | Status Indicator (Active-Low) |
| `IO_BOOT` | `GPIO9` (Pin 15) | `BOOT` switch (`TS-1088`) + `C3` (`100nF`, see §5.1) to `GND` | `hvac-adapter-gpio::GpioBootButtonDriver` | Dual-stage Reset Button (Active-Low) |
| `PROG_TX` | `GPIO16` (Pin 25) | `J2` Pin 4 (1×4 2.54mm PL2303 / FTDI header) | `esp-println` / UART0 | Serial logging / bootloader output |
| `PROG_RX` | `GPIO17` (Pin 24) | `J2` Pin 3 (1×4 2.54mm PL2303 / FTDI header) | ROM Bootloader / UART0 | Wired flashing input |
| `USB_D-` / `USB_D+` | `GPIO12` / `GPIO13` | Not connected (`NC`) | Unused | No on-board USB-Serial/JTAG |
| `GPIO8_STRAP` | `GPIO8` (Pin 10) | `R5` (`10kΩ`) pull-up to `+3V3` | Unused | Boot strapping pin |

---

## 2. Connectors

### J1 — Mitsubishi CN105 Port (JST-PA 5-pin 2.0mm pitch)

Connects directly to the CN105 connector on the Mitsubishi Electric indoor unit PCB:

```
        ┌───────────────────────────────────┐
        │  [1]   [2]   [3]   [4]   [5]      │  J1 (JST-PA 5-pin 2.0mm)
        └───┬─────┬─────┬─────┬─────┬───────┘
            │     │     │     │     │
            │     │     │     │     └────── Pin 5: CN105_TX_5V (HVAC Unit RX, 5V)
            │     │     │     └──────────── Pin 4: CN105_RX_5V (HVAC Unit TX, 5V)
            │     │     └────────────────── Pin 3: +5V_HVAC (5V Logic Reference)
            │     └──────────────────────── Pin 2: GND
            └────────────────────────────── Pin 1: +12V_HVAC (Indoor Unit Power)
```

### J2 — Programming & Diagnostics Header (1×4 2.54mm / 0.1")

```
        ┌─────────────────────────┐
        │  [1]   [2]   [3]   [4]  │  J2 (4-pin 0.1" Header)
        └───┬─────┬─────┬─────┬───┘
            │     │     │     │
            │     │     │     └────── Pin 4: PROG_TX (ESP32-C6 GPIO16 / UART0 TX)
            │     │     └──────────── Pin 3: PROG_RX (ESP32-C6 GPIO17 / UART0 RX)
            │     └────────────────── Pin 2: +5V_PROG (Optional +5V Input)
            └──────────────────────── Pin 1: GND
```

---

## 3. Level Shifting Circuitry

The CN105 port operates at 5V logic levels, while the ESP32-C6 operates at 3.3V logic levels. Bidirectional N-channel MOSFET level shifters (`2N7002`) provide isolated, voltage-safe signal translation:

```
        ESP32-C6 (3.3V)                                 CN105 HVAC (5V)
                                  +3.3V       +5V
                                    │          │
                                   [R1]       [R2] (10kΩ pull-ups)
                                    │          │
        GPIO18 (TX) ───────────────┬┴──────────┴┬────────────── Pin 5 (HVAC RX)
                                   S     Q1     D
                                      (2N7002)
                                         G
                                         │
                                       +3.3V

                                  +3.3V       +5V
                                    │          │
                                   [R3]       [R4] (10kΩ pull-ups)
                                    │          │
        GPIO19 (RX) ───────────────┬┴──────────┴┬────────────── Pin 4 (HVAC TX)
                                   S     Q2     D
                                      (2N7002)
                                         G
                                         │
                                       +3.3V
```

---

## 4. Boot & Flashing Modes

Because USB lines `GPIO12`/`GPIO13` are unconnected (`NC`) and `J2` has no DTR/RTS auto-reset circuitry:

1. **Serial Logging & Monitor:** UART0 (`115200` baud, 8N1) via `scripts/monitor.py`.
2. **Entering ROM Bootloader Mode:**
   - Hold physical **`BOOT`** button (`GPIO9`).
   - Press and release physical **`RESET`** button (`EN`).
   - Release **`BOOT`** button.
   - Run `espflash flash ... --port /dev/cu.usbserial*`.

---

## 5. Known Hardware Issues & Errata

### 5.1 HW-01: Intermittent Cold Boot Failure Caused by C3 (100nF) on GPIO9

- **Symptoms:** On initial power-up (cold power cycle), the device occasionally fails to start: the status LED stays dark (never blinks), no Wi-Fi association occurs, and the bridge is unreachable. Reconnecting power physically (or power-cycling quickly) allows the device to boot normally.
- **Root Cause:**
  - `GPIO9` is the primary boot-strapping pin on the ESP32-C6. During chip reset release (`CHIP_PU` / `EN` rising edge), the hardware latches the logic level of `GPIO9`:
    - `GPIO9 = HIGH (1)` $\rightarrow$ **SPI Boot** (normal firmware execution).
    - `GPIO9 = LOW (0)` $\rightarrow$ **ROM Download Boot** (waits indefinitely for serial flashing on UART0; user code never runs).
  - `GPIO9` relies on the ESP32-C6 internal weak pull-up resistor ($\approx 45\text{ k}\Omega$).
  - Capacitor `C3` ($100\text{ nF}$) was placed in parallel with the `BOOT` switch (`TS-1088`) to `GND` for hardware debouncing.
  - On cold boot, `C3` is completely discharged ($0\text{ V}$). The RC charging time constant is:
    $$\tau = R \times C = 45\text{ k}\Omega \times 100\text{ nF} = 4.5\text{ ms}$$
    Reaching the logic-high input threshold ($V_{IH} \approx 0.75 \times 3.3\text{V} \approx 2.48\text{ V}$) requires $\approx 6.2\text{ ms}$.
  - Because `EN` releases within $1\text{ ms} - 2\text{ ms}$, the chip samples `GPIO9` while it is still held LOW ($< 1\text{ V}$) by the charging capacitor, unintentionally entering ROM Download Mode.
  - On quick power-cycles, residual charge on `C3` allows `GPIO9` to exceed $V_{IH}$ before reset releases, masking the issue.
- **Espressif Design Guideline Reference:**
  - *ESP32-C6 Series Hardware Design Guidelines* (Schematic Checklist $\rightarrow$ Strapping Pins):
    > *"Notice: Please do not add high-value capacitors at GPIO9, otherwise the chip might enter Download Boot mode when booted for the first time."*
- **Remediation:**
  1. **Recommended Hardware Fix (Board Rework):** Remove / desolder capacitor `C3` ($100\text{ nF}$).
     - The hardware debounce capacitor is redundant: firmware already implements a 30ms non-blocking software debounce filter (`BUTTON_DEBOUNCE_MS`) in `hvac-adapter-gpio::boot_button`, and reset actions require holding the button for 5s (Ecosystem Reset) or 10s (Factory Reset).
     - Manual entry into download mode remains fully functional by holding the physical `BOOT` button during reset.
  2. **Alternative Hardware Fix:** Solder an external $2.2\text{ k}\Omega - 4.7\text{ k}\Omega$ pull-up resistor from `GPIO9` (Pin 15) to `+3V3` to accelerate the RC rise time ($\tau \le 0.22\text{ ms}$).
  3. **PCB Rev 1.1 Revision Note:** Remove `C3` footprint or leave unpopulated (DNP) and route `GPIO9` directly to the `BOOT` switch with optional external pull-up.

