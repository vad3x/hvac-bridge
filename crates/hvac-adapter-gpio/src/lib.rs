//! CN105 HVAC Matter Bridge - Hardware GPIO & Flash Adapter
//!
//! Provides concrete drivers implementing `hvac_core::ports` traits
//! for physical board peripherals (`cn105-matter-bridge-HEAD` Rev 1.0)
//! on the `ESP32-C6-WROOM-1-N8`.

#![cfg_attr(not(test), no_std)]
#![allow(async_fn_in_trait)]

extern crate alloc;

pub mod boot_button;
pub mod config;
pub mod flash;
pub mod led;

pub use boot_button::{GpioBootButtonConfig, GpioBootButtonDriver};
pub use flash::{EspFlash, EspFlashError};
pub use led::{GpioLedDriver, HvacLedProfile};
