//! HVAC Bridge - Matter Commissioning QR & Pairing Code Tool
//!
//! Generates official Matter manual pairing codes and setup QR code
//! for a given device MAC address using the shared device foundation.

use std::fs;
use std::process::Command;

use edf_pairing_tool::{generate, parse_mac as parse_foundation_mac, PairingData, PairingOptions};
use rs_matter::pairing::qr::{Qr, QrTextType};

const VID: u16 = 0xfff1;
const PID: u16 = 0x8001;

fn print_usage() {
    eprintln!(
        r#"Usage: cargo qr [OPTIONS]

Options:
  --mac <MAC>     6-byte hardware MAC address (e.g. 98:a3:16:a6:78:44 or 98A316A67844)
  --port <PORT>   Serial port of the connected ESP32-C6 (auto-detected if omitted)
  --json          Output results as JSON
  -h, --help      Print this help message
"#
    );
}

fn find_serial_port() -> Option<String> {
    let mut ports = Vec::new();
    if let Ok(entries) = fs::read_dir("/dev") {
        for entry in entries.flatten() {
            if let Ok(name) = entry.file_name().into_string() {
                if name.starts_with("cu.usbmodem") || name.starts_with("cu.usbserial") {
                    ports.push(format!("/dev/{}", name));
                }
            }
        }
    }
    ports.sort();
    ports.into_iter().next()
}

fn query_mac_from_espflash(port: &str) -> Result<String, String> {
    eprintln!("Querying device MAC address on {}...", port);

    // Try "espflash" first, then "mise exec -- espflash"
    let output = Command::new("espflash")
        .args(["board-info", "--port", port])
        .output()
        .or_else(|_| {
            Command::new("mise")
                .args(["exec", "--", "espflash", "board-info", "--port", port])
                .output()
        })
        .map_err(|e| format!("Failed to execute espflash: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    if !output.status.success() {
        if combined.contains("Resource busy") || combined.contains("Device or resource busy") {
            return Err(format!(
                "Serial port {} is busy. Ensure other monitors (e.g. monitor.py) are stopped.",
                port
            ));
        }
        return Err(format!("espflash board-info failed:\n{}", combined));
    }

    for line in combined.lines() {
        if line.contains("MAC address:") {
            if let Some(mac_part) = line.split("MAC address:").nth(1) {
                let trimmed = mac_part.trim();
                if !trimmed.is_empty() {
                    return Ok(trimmed.to_string());
                }
            }
        }
    }

    Err(format!(
        "Could not find MAC address in espflash output:\n{}",
        combined
    ))
}

fn parse_mac(s: &str) -> Result<[u8; 6], &'static str> {
    let clean: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();

    if clean.len() != 12 {
        return Err("MAC address must be 12 hex characters (e.g. AA:BB:CC:DD:EE:FF)");
    }

    parse_foundation_mac(&clean).map_err(|_| "Invalid hex in MAC address")
}

fn pairing_data_for_mac(mac: [u8; 6]) -> Result<PairingData, edf_pairing_tool::PairingError> {
    generate(PairingOptions {
        mac,
        prefix: "HV",
        name: "hvac-bridge",
        vid: VID,
        pid: PID,
    })
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut mac_str: Option<String> = None;
    let mut port_str: Option<String> = None;
    let mut json_mode = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--" => {}
            "--mac" | "-m" => {
                mac_str = args.next();
            }
            "--port" | "-p" => {
                port_str = args.next();
            }
            "--json" => {
                json_mode = true;
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            other if !other.starts_with('-') && mac_str.is_none() => {
                mac_str = Some(other.to_string());
            }
            _ => {
                eprintln!("Unknown argument: {}", arg);
                print_usage();
                std::process::exit(1);
            }
        }
    }

    let mac_input = match mac_str {
        Some(m) => m,
        None => {
            let port = match port_str {
                Some(p) => p,
                None => match find_serial_port() {
                    Some(p) => p,
                    None => {
                        eprintln!("Error: No ESP32-C6 USB device detected.");
                        eprintln!("Connect your device via USB or specify a MAC address manually:");
                        eprintln!("    cargo qr --mac 98:a3:16:a6:78:44");
                        std::process::exit(1);
                    }
                },
            };

            match query_mac_from_espflash(&port) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
    };

    let mac_bytes = match parse_mac(&mac_input) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let pairing_data = match pairing_data_for_mac(mac_bytes) {
        Ok(data) => data,
        Err(edf_pairing_tool::PairingError::Qr(error)) => {
            eprintln!("Error generating QR text: {:?}", error);
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("Error generating pairing data: {:?}", error);
            std::process::exit(1);
        }
    };

    if json_mode {
        println!("{}", pairing_data.json());
        return;
    }

    println!("================================================================================");
    println!("  HVAC Bridge — Matter Commissioning Credentials");
    println!("================================================================================");
    println!();
    println!("  Device MAC:          {}", pairing_data.mac);
    println!("  Serial Number:       {}", pairing_data.serial_number);
    println!(
        "  Vendor ID / PID:     0x{:04X} ({}) / 0x{:04X} ({})",
        VID, VID, PID, PID
    );
    println!(
        "  Discriminator:       {} (0x{:03X})",
        pairing_data.discriminator, pairing_data.discriminator
    );
    println!("  Passcode (PIN):      {}", pairing_data.passcode);
    println!();
    println!("  Manual Pairing Code (Standard 11-digit):");
    println!("  ┌─────────────────┐");
    println!("  │  {:^13}  │", pairing_data.manual_code);
    println!("  └─────────────────┘");
    println!();
    println!("  Manual Pairing Code (Long 21-digit with VID/PID):");
    println!("  ┌─────────────────────────────┐");
    println!("  │  {:^25}  │", pairing_data.manual_code_long);
    println!("  └─────────────────────────────┘");
    println!();
    println!("  Matter Setup QR Code Payload:");
    println!("  {}", pairing_data.qr_code);
    println!();
    println!("  Scan with Apple Home / Google Home / SmartThings:");
    println!();

    // Compute and display QR matrix
    let mut tmp_buf = [0u8; 1024];
    let mut out_buf = [0u8; 1024];
    match Qr::compute(&pairing_data.qr_code, &mut tmp_buf, &mut out_buf) {
        Ok(qr) => {
            const BORDER_SIZE: u8 = 2;
            for y in qr.lines_range(QrTextType::Unicode, BORDER_SIZE) {
                let mut line_buf = [0u8; 512];
                if let Ok((line, _)) = qr.line_as_str(
                    QrTextType::Unicode,
                    BORDER_SIZE,
                    false,
                    false,
                    y,
                    &mut line_buf,
                ) {
                    println!("  {}", line);
                }
            }
        }
        Err(e) => {
            eprintln!("  (Failed to render QR matrix in terminal: {:?})", e);
        }
    }

    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foundation_pairing_generator_preserves_hvac_reference_vector() {
        let data = pairing_data_for_mac([0x98, 0xa3, 0x16, 0xa6, 0x78, 0x44])
            .expect("known HVAC identity should be valid");

        assert_eq!(
            data.json(),
            "{\n  \"mac\": \"98:a3:16:a6:78:44\",\n  \"serial_number\": \"HV-98A316A67844\",\n  \"vendor_id\": 65521,\n  \"product_id\": 32769,\n  \"discriminator\": 3096,\n  \"passcode\": 21710079,\n  \"manual_code\": \"3012-791-3250\",\n  \"manual_code_long\": \"7012-791-3256-55213-27699\",\n  \"qr_code\": \"MT:-24J042C00X1VC4QK3P0OUYH0QN5G1V.X12LJRQ19UZR1OADO1O0\"\n}"
        );
    }

    #[test]
    fn test_parse_mac_valid_colon() {
        let mac = parse_mac("98:a3:16:a6:78:44").unwrap();
        assert_eq!(mac, [0x98, 0xa3, 0x16, 0xa6, 0x78, 0x44]);
    }

    #[test]
    fn test_parse_mac_valid_raw_hex() {
        let mac = parse_mac("98A316A67844").unwrap();
        assert_eq!(mac, [0x98, 0xa3, 0x16, 0xa6, 0x78, 0x44]);
    }

    #[test]
    fn test_parse_mac_invalid_length() {
        assert!(parse_mac("98A316A678").is_err());
    }

    #[test]
    fn test_parse_mac_invalid_hex() {
        assert!(parse_mac("98:a3:16:a6:78:ZZ").is_err());
    }
}
