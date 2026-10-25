//! Mitsubishi Electric CN105 HVAC Protocol Adapter
//!
//! Framing, checksums, packet codec, and serial driver for the CN105 interface.

#![cfg_attr(not(test), no_std)]

pub mod codec;
pub mod config;
pub mod driver;
pub mod frame;

#[cfg(target_arch = "riscv32")]
pub mod uart;

pub use codec::{decode_response_packet, encode_set_packet, Cn105CodecState, GetPacketType};
pub use driver::{Cn105Driver, Cn105Serial};
pub use frame::{
    build_connect_packet, build_connect_response_packet, build_get_packet,
    build_set_response_packet, checksum, verify_frame, CONNECT_LEN, HEADER_MAGIC, PACKET_LEN,
};


#[cfg(target_arch = "riscv32")]
pub use uart::EspUartSerial;
