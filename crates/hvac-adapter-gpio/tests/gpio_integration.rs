use embedded_storage_async::nor_flash::{NorFlash, ReadNorFlash};
use hvac_adapter_gpio::config::{
    ACTIVITY_PULSE_MS, ECOSYSTEM_RESET_HOLD_MS, FACTORY_RESET_HOLD_MS, PIN_BOOT_BUTTON,
    PIN_LED_STATUS,
};
use hvac_adapter_gpio::{
    EspFlash, EspFlashError, GpioBootButtonConfig, GpioBootButtonDriver, GpioLedDriver,
};
use hvac_core::domain::{classify_reset_hold, HvacState, ResetScope};
use hvac_core::events::{SystemState, SystemStateSender, SystemStateWatch};
use hvac_core::ports::HvacReporter;

// ============================================================================
// Integration Tests: LED Driver & HVAC Reporting
// ============================================================================

#[test]
fn led_driver_reports_hvac_activity_and_responds_to_system_states() {
    // Arrange
    let driver = GpioLedDriver::new();
    let state_watch = SystemStateWatch::new();
    let state_sender = SystemStateSender::new(&state_watch);
    let state_receiver = state_watch.receiver().unwrap();

    // Act & Assert: Run driver event loop concurrently with lifecycle transitions and HVAC activity
    futures::executor::block_on(async {
        let test_steps = async {
            // Act 1: State transition to Operational
            state_sender.send(SystemState::Operational);
            embassy_time::Timer::after(embassy_time::Duration::from_millis(50)).await;

            assert_eq!(driver.observed_state(), Some(SystemState::Operational));
            assert!(driver.is_on()); // Active-low LED is ON (pin is LOW)

            // Act 2: Verify reporter dispatch generates activity pulse (wink)
            let state = HvacState::default();
            driver.report_state(&state);

            // Give driver event loop a tick to process pulse signal (LED turns off for 150ms wink)
            embassy_time::Timer::after(embassy_time::Duration::from_millis(20)).await;
            assert!(!driver.is_on());

            // After pulse duration (ACTIVITY_PULSE_MS = 150ms) plus margin, LED returns ON
            embassy_time::Timer::after(embassy_time::Duration::from_millis(180)).await;
            assert!(driver.is_on());
        };

        match embassy_futures::select::select(driver.run(state_receiver), test_steps).await {
            embassy_futures::select::Either::First(_) => unreachable!(),
            embassy_futures::select::Either::Second(()) => {}
        }
    });

    assert_eq!(PIN_LED_STATUS, 15);
    assert_eq!(ACTIVITY_PULSE_MS, 150);
}

#[test]
fn boot_button_hold_classification_integration() {
    // Arrange
    let config = GpioBootButtonConfig::new();
    let _driver = GpioBootButtonDriver::new(config);

    // Act & Assert 1: Short press (<5s) ignored
    let short = classify_reset_hold(
        embassy_time::Duration::from_millis(2000),
        true,
        embassy_time::Duration::from_millis(ECOSYSTEM_RESET_HOLD_MS),
        embassy_time::Duration::from_millis(FACTORY_RESET_HOLD_MS),
    );
    assert_eq!(short, None);

    // Act & Assert 2: Ecosystem hold (5s-10s)
    let eco = classify_reset_hold(
        embassy_time::Duration::from_millis(6000),
        true,
        embassy_time::Duration::from_millis(ECOSYSTEM_RESET_HOLD_MS),
        embassy_time::Duration::from_millis(FACTORY_RESET_HOLD_MS),
    );
    assert_eq!(eco, Some(ResetScope::Ecosystem));

    // Act & Assert 3: Factory hold (>=10s)
    let factory = classify_reset_hold(
        embassy_time::Duration::from_millis(10500),
        false,
        embassy_time::Duration::from_millis(ECOSYSTEM_RESET_HOLD_MS),
        embassy_time::Duration::from_millis(FACTORY_RESET_HOLD_MS),
    );
    assert_eq!(factory, Some(ResetScope::Factory));

    assert_eq!(PIN_BOOT_BUTTON, 9);
}

#[test]
fn esp_flash_partitions_read_write_erase_lifecycle() {
    // Arrange: Use partitions from partitions.csv
    // wifi_kvs: 0x6c0000, size 0x2000 (8 KiB)
    let mut wifi_flash = EspFlash::new(0x6c0000..0x6c2000);
    // matter_kvs: 0x6c2000, size 0x6000 (24 KiB)
    let mut matter_flash = EspFlash::new(0x6c2000..0x6c8000);

    assert_eq!(wifi_flash.capacity(), 0x2000);
    assert_eq!(matter_flash.capacity(), 0x6000);

    // Act 1: Erase and write in wifi_kvs
    futures::executor::block_on(wifi_flash.erase(0x6c0000, 0x6c1000)).unwrap();
    let wifi_payload = [0xAA, 0xBB, 0xCC, 0xDD];
    futures::executor::block_on(wifi_flash.write(0x6c0000, &wifi_payload)).unwrap();

    let mut read_buf = [0u8; 4];
    futures::executor::block_on(wifi_flash.read(0x6c0000, &mut read_buf)).unwrap();
    assert_eq!(read_buf, wifi_payload);

    // Act 2: Erase and write in matter_kvs
    futures::executor::block_on(matter_flash.erase(0x6c2000, 0x6c4000)).unwrap();
    let matter_payload = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
    futures::executor::block_on(matter_flash.write(0x6c2000, &matter_payload)).unwrap();

    let mut read_buf8 = [0u8; 8];
    futures::executor::block_on(matter_flash.read(0x6c2000, &mut read_buf8)).unwrap();
    assert_eq!(read_buf8, matter_payload);

    // Act 3: Isolation check - wifi_flash cannot access matter_flash range
    let cross_access = futures::executor::block_on(wifi_flash.read(0x6c2000, &mut read_buf));
    assert_eq!(cross_access, Err(EspFlashError::OutOfBounds));
}
