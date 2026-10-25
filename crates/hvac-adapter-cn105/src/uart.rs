//! ESP32-C6 UART1 Transport for Mitsubishi CN105 protocol.

#![cfg(target_arch = "riscv32")]

use esp_hal::gpio::interconnect::{PeripheralInput, PeripheralOutput};
use esp_hal::peripherals::UART1;
use esp_hal::uart::{Config, DataBits, Parity, StopBits, Uart};
use esp_hal::Async;
use hvac_core::ports::HvacDriverError;

use crate::config::BAUD_RATE;
use crate::driver::Cn105Serial;

/// Hardware async serial driver backed by ESP32-C6 UART peripheral.
pub struct EspUartSerial<'a> {
    uart: Uart<'a, Async>,
}

impl<'a> EspUartSerial<'a> {
    /// Initialize UART1 with standard CN105 serial parameters (2400 baud, 8E1)
    /// and map to the specified TX and RX GPIO pins.
    pub fn new<TX: PeripheralOutput<'a>, RX: PeripheralInput<'a>>(
        uart: UART1<'a>,
        tx: TX,
        rx: RX,
    ) -> Result<Self, HvacDriverError> {
        let config = Config::default()
            .with_baudrate(BAUD_RATE)
            .with_parity(Parity::Even)
            .with_data_bits(DataBits::_8)
            .with_stop_bits(StopBits::_1);

        let uart = Uart::new(uart, config)
            .map_err(|_| HvacDriverError::IoError)?
            .with_tx(tx)
            .with_rx(rx)
            .into_async();

        Ok(Self { uart })
    }

    /// Construct `EspUartSerial` from an already configured async `Uart` instance.
    pub fn from_uart(uart: Uart<'a, Async>) -> Self {
        Self { uart }
    }
}

impl<'a> Cn105Serial for EspUartSerial<'a> {
    async fn write_all(&mut self, mut buf: &[u8]) -> Result<(), HvacDriverError> {
        while !buf.is_empty() {
            match self.uart.write_async(buf).await {
                Ok(0) => return Err(HvacDriverError::IoError),
                Ok(n) => buf = &buf[n..],
                Err(_) => return Err(HvacDriverError::IoError),
            }
        }
        let _ = self.uart.flush_async().await;
        Ok(())
    }

    async fn read_byte(&mut self) -> Result<u8, HvacDriverError> {
        let mut buf = [0u8; 1];
        loop {
            match self.uart.read_async(&mut buf).await {
                Ok(0) => continue,
                Ok(_) => return Ok(buf[0]),
                Err(_) => return Err(HvacDriverError::IoError),
            }
        }
    }

    async fn read_exact(&mut self, mut buf: &mut [u8]) -> Result<(), HvacDriverError> {
        while !buf.is_empty() {
            match self.uart.read_async(buf).await {
                Ok(0) => continue,
                Ok(n) => buf = &mut buf[n..],
                Err(_) => return Err(HvacDriverError::IoError),
            }
        }
        Ok(())
    }
}

