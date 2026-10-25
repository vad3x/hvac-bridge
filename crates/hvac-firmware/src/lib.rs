//! CN105 HVAC Matter Bridge - Firmware Library
//!
//! Exposes utility modules, configurations, and version metadata.
//! This library target enables host-based unit testing of non-hardware logic.
#![no_std]

#[cfg(test)]
extern crate std;

pub mod config;
pub mod hardware_info;
pub mod reset;
pub mod utils;
pub mod version;
