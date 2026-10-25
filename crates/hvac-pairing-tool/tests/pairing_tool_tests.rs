use std::process::Command;

const HVAC_PAIRING_JSON: &str = "{\n  \"mac\": \"98:a3:16:a6:78:44\",\n  \"serial_number\": \"HV-98A316A67844\",\n  \"vendor_id\": 65521,\n  \"product_id\": 32769,\n  \"discriminator\": 3096,\n  \"passcode\": 21710079,\n  \"manual_code\": \"3012-791-3250\",\n  \"manual_code_long\": \"7012-791-3256-55213-27699\",\n  \"qr_code\": \"MT:-24J042C00X1VC4QK3P0OUYH0QN5G1V.X12LJRQ19UZR1OADO1O0\"\n}\n";

#[test]
fn json_cli_preserves_the_hvac_pairing_output_contract() {
    let output = Command::new(env!("CARGO_BIN_EXE_hvac-pairing-tool"))
        .args(["--mac", "98:a3:16:a6:78:44", "--json"])
        .output()
        .expect("pairing CLI should start");

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), HVAC_PAIRING_JSON);
    assert!(output.stderr.is_empty());
}

#[test]
fn text_cli_preserves_hvac_presentation_and_pairing_values() {
    let output = Command::new(env!("CARGO_BIN_EXE_hvac-pairing-tool"))
        .args(["--mac", "98:a3:16:a6:78:44"])
        .output()
        .expect("pairing CLI should start");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    for expected in [
        "HVAC Bridge — Matter Commissioning Credentials",
        "  Device MAC:          98:a3:16:a6:78:44",
        "  Serial Number:       HV-98A316A67844",
        "  Vendor ID / PID:     0xFFF1 (65521) / 0x8001 (32769)",
        "  Discriminator:       3096 (0xC18)",
        "  Passcode (PIN):      21710079",
        "  │  3012-791-3250  │",
        "  Matter Setup QR Code Payload:",
        "  MT:-24J042C00X1VC4QK3P0OUYH0QN5G1V.X12LJRQ19UZR1OADO1O0",
    ] {
        assert!(stdout.contains(expected), "missing CLI output: {expected}");
    }
}

#[test]
fn json_cli_keeps_legacy_mac_spellings() {
    for mac in ["98-a3-16-a6-78-44", "98.a3.16.a6.78.44", "98A316A67844"] {
        let output = Command::new(env!("CARGO_BIN_EXE_hvac-pairing-tool"))
            .args(["--mac", mac, "--json"])
            .output()
            .expect("pairing CLI should start");

        assert!(
            output.status.success(),
            "MAC spelling should be accepted: {mac}"
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap(), HVAC_PAIRING_JSON);
    }
}

#[test]
fn invalid_explicit_mac_is_rejected_before_any_serial_query() {
    let output = Command::new(env!("CARGO_BIN_EXE_hvac-pairing-tool"))
        .args(["--mac", "not-a-mac", "--json"])
        .output()
        .expect("pairing CLI should start");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("MAC address must be 12 hex characters"));
    assert!(!stderr.contains("Querying device MAC address"));
}
