//! Matter commissioning identity for CN105 HVAC Matter Bridge.
//!
//! HVAC prefixes are applied to the shared foundation identity derivation so
//! the device and the host pairing tool produce the same commissioning values.

use rs_matter::BasicCommData;

pub use edf_matter::identity::{DeviceIdentity, IdentityError, IdentityParams};

/// HVAC serial-number prefix.
pub const SERIAL_PREFIX: &str = "HV";

/// HVAC DHCP hostname prefix.
pub const HOSTNAME_PREFIX: &str = "hvac-bridge";

/// Derive the HVAC identity using the shared commissioning algorithm.
pub fn derive_hvac_identity(mac: &[u8; 6]) -> Result<DeviceIdentity, IdentityError> {
    edf_matter::identity::derive_device_identity(&IdentityParams {
        mac,
        serial_prefix: SERIAL_PREFIX,
        hostname_prefix: HOSTNAME_PREFIX,
    })
}

/// Derives `BasicCommData` from a 6-byte hardware MAC address.
pub fn comm_data_from_mac(mac: &[u8; 6]) -> BasicCommData {
    derive_hvac_identity(mac)
        .expect("valid HVAC identity prefixes")
        .comm_data()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: [u8; 6] = [0x98, 0xa3, 0x16, 0xa6, 0x78, 0x44];

    #[test]
    fn hvac_identity_preserves_pairing_vectors() {
        let identity = derive_hvac_identity(&MAC).unwrap();
        assert_eq!(
            (
                identity.passcode,
                identity.discriminator,
                identity.serial_str(),
                identity.hostname_str(),
            ),
            (21_710_079, 3096, "HV-98A316A67844", "hvac-bridge-7844")
        );
    }

    #[test]
    fn hvac_comm_data_uses_foundation_identity() {
        let identity = derive_hvac_identity(&MAC).unwrap();
        let comm_data = comm_data_from_mac(&MAC);

        assert_eq!(comm_data.discriminator, identity.discriminator);
        assert_eq!(
            comm_data.password.access(),
            &identity.passcode.to_le_bytes()
        );
    }

    #[test]
    fn different_macs_produce_different_pairing_parameters() {
        let mac1 = [0x98, 0xa3, 0x16, 0xa6, 0x78, 0x44];
        let mac2 = [0x98, 0xa3, 0x16, 0xa6, 0x78, 0x45];

        let id1 = derive_hvac_identity(&mac1).unwrap();
        let id2 = derive_hvac_identity(&mac2).unwrap();

        assert_ne!(
            (id1.passcode, id1.discriminator),
            (id2.passcode, id2.discriminator)
        );
    }
}
