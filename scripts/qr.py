#!/usr/bin/env python3
"""CN105 HVAC Bridge - Matter Commissioning QR & Pairing Code Runner.

Queries the connected ESP32-C6 via serial (or accepts manual --mac)
and invokes the host hvac-pairing-tool to render Matter QR and pairing codes.
"""

from __future__ import annotations

import argparse
import glob
import os
import platform
import re
import subprocess
import sys


def find_serial_port() -> str | None:
    ports = glob.glob("/dev/cu.usbserial*") + glob.glob("/dev/cu.usbmodem*")
    return ports[0] if ports else None


def get_host_target() -> str:
    try:
        out = subprocess.check_output(
            ["rustc", "-vV"], stderr=subprocess.DEVNULL, text=True
        )
        for line in out.splitlines():
            if line.startswith("host:"):
                return line.split(":", 1)[1].strip()
    except Exception:
        pass

    arch = platform.machine().lower()
    system = platform.system().lower()
    if system == "darwin":
        if arch in ("arm64", "aarch64"):
            return "aarch64-apple-darwin"
        return "x86_64-apple-darwin"
    elif system == "linux":
        if arch in ("arm64", "aarch64"):
            return "aarch64-unknown-linux-gnu"
        return "x86_64-unknown-linux-gnu"

    return "x86_64-apple-darwin"


def get_mac_from_espflash(port: str) -> str:
    cmd = ["mise", "exec", "--", "espflash", "board-info", "--port", port]
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True)
    except FileNotFoundError:
        # Fallback to direct espflash
        cmd = ["espflash", "board-info", "--port", port]
        proc = subprocess.run(cmd, capture_output=True, text=True)

    combined_output = proc.stdout + "\n" + proc.stderr

    if proc.returncode != 0:
        if "Resource busy" in combined_output or "Device or resource busy" in combined_output:
            sys.stderr.write(
                f"\nError: Serial port {port} is busy. "
                "Ensure other serial monitors (e.g. scripts/monitor.py) are stopped.\n\n"
            )
        else:
            sys.stderr.write(f"\nFailed to query device info via espflash:\n{combined_output}\n")
        sys.exit(1)

    match = re.search(r"MAC address:\s+([0-9a-fA-F:]{17})", combined_output)
    if not match:
        sys.stderr.write(f"\nCould not parse MAC address from espflash output:\n{combined_output}\n")
        sys.exit(1)

    return match.group(1).strip()


def main():
    parser = argparse.ArgumentParser(
        description="Show Matter Commissioning QR & Manual Pairing Code"
    )
    parser.add_argument(
        "--port", "-p", default=None, help="Serial port (auto-detected if omitted)"
    )
    parser.add_argument(
        "--mac", "-m", default=None, help="Hardware MAC address (skips serial query if specified)"
    )
    parser.add_argument(
        "--json", action="store_true", help="Output results in JSON format"
    )
    args = parser.parse_args()

    mac = args.mac
    if not mac:
        port = args.port or find_serial_port()
        if not port:
            sys.stderr.write(
                "\nError: No ESP32-C6 serial device detected.\n"
                "Connect your device or specify a MAC address manually:\n"
                "    cargo qr -- --mac 98:a3:16:a6:78:44\n\n"
            )
            sys.exit(1)

        print(f"Querying device MAC address on {port}...", file=sys.stderr)
        mac = get_mac_from_espflash(port)

    host_target = get_host_target()
    cargo_cmd = [
        "mise",
        "exec",
        "--",
        "cargo",
        "run",
        "-q",
        "-p",
        "hvac-pairing-tool",
        "--target",
        host_target,
        "--",
        "--mac",
        mac,
    ]
    if args.json:
        cargo_cmd.append("--json")

    res = subprocess.run(cargo_cmd)
    sys.exit(res.returncode)


if __name__ == "__main__":
    main()
