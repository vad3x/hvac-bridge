//! HVAC Bridge - Matter Fan Control Cluster Handler
//!
//! Exposes the Matter Fan Control cluster (`0x0202`, rev 4) on Endpoint 2 (`DEV_TYPE_FAN` `0x002B`).
//! Maps HVAC fan speed and vane swing state to Matter attributes and dispatches controller mutations
//! to `HvacControllerHandle`.

use core::sync::atomic::Ordering;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::signal::Signal;
use log::info;
use portable_atomic::{AtomicBool, AtomicU8};

use rs_matter::dm::clusters::decl::fan_control::{
    self, FanModeEnum, FanModeSequenceEnum, RockBitmap,
};
use rs_matter::dm::{
    AsyncHandler, Cluster, Dataver, DeviceType, HandlerContext, InvokeContext, InvokeReply,
    MatchContext, ReadContext, ReadReply, Reply, WriteContext,
};
use rs_matter::error::{Error, ErrorCode};
use rs_matter::tlv::{FromTLV, Nullable};
use rs_matter::with;

use hvac_core::domain::{FanSpeed, HvacState, VaneHorizontal, VaneVertical};
use hvac_core::ports::HvacControllerHandle;

use crate::config::{DEV_TYPE_ROOM_AIR_CONDITIONER, FAN_ENDPOINT_ID};
use crate::readiness::ClusterReadinessBarrier;
use crate::MatterCluster;

// ============================================================================
// Public Structs & Implementation
// ============================================================================

/// Default initial fan speed percentage (`50%`).
pub const DEFAULT_FAN_PERCENT: u8 = 50;

/// Configuration for [`HvacFanControl`].
pub struct HvacFanControlConfig<'a> {
    /// Inbound driving port handle to dispatch HVAC mutation commands.
    pub hvac: HvacControllerHandle<'a>,
    /// Cluster readiness barrier to track initial subscription priming.
    pub readiness: &'static ClusterReadinessBarrier,
}

/// Matter Fan Control cluster handler on Endpoint 1.
pub struct HvacFanControl<'a> {
    config: HvacFanControlConfig<'a>,
    dataver: Dataver,
    auto_mode: AtomicBool,
    percent: AtomicU8,
    swing_vertical: AtomicBool,
    swing_horizontal: AtomicBool,
    state_changed: Signal<NoopRawMutex, ()>,
}

impl<'a> HvacFanControl<'a> {
    /// Create a new HVAC Fan Control handler with default nominal values.
    pub fn new(config: HvacFanControlConfig<'a>) -> Self {
        Self {
            config,
            dataver: Dataver::new(1),
            auto_mode: AtomicBool::new(true),
            percent: AtomicU8::new(DEFAULT_FAN_PERCENT),
            swing_vertical: AtomicBool::new(false),
            swing_horizontal: AtomicBool::new(false),
            state_changed: Signal::new(),
        }
    }

    /// Update internal state from an out-of-band canonical HVAC state snapshot.
    ///
    /// Updates internal atomic registers, bumps `dataver`, and signals `state_changed`
    /// to notify active Matter subscriptions.
    pub fn update_state(&self, state: &HvacState) {
        let speed_changed = match state.fan_speed {
            FanSpeed::Auto => {
                let prev_auto = self.auto_mode.swap(true, Ordering::SeqCst);
                !prev_auto
            }
            manual => {
                let new_percent = match manual {
                    FanSpeed::Quiet => 20,
                    FanSpeed::Speed1 => 40,
                    FanSpeed::Speed2 => 60,
                    FanSpeed::Speed3 => 80,
                    FanSpeed::Speed4 => 100,
                    FanSpeed::Auto => DEFAULT_FAN_PERCENT,
                };
                let prev_auto = self.auto_mode.swap(false, Ordering::SeqCst);
                let prev_percent = self.percent.swap(new_percent, Ordering::SeqCst);
                prev_auto || prev_percent != new_percent
            }
        };

        let new_swing_v = state.vane_vertical == VaneVertical::Swing;
        let new_swing_h = state.vane_horizontal == VaneHorizontal::Swing;
        let prev_swing_v = self.swing_vertical.swap(new_swing_v, Ordering::SeqCst);
        let prev_swing_h = self.swing_horizontal.swap(new_swing_h, Ordering::SeqCst);

        if speed_changed || prev_swing_v != new_swing_v || prev_swing_h != new_swing_h {
            info!("HvacFanControl state changed -> bumping dataver and signaling subscriber notify");
            self.dataver.changed();
            self.state_changed.signal(());
        }
    }

    /// Read current Matter FanModeEnum.
    pub fn fan_mode(&self) -> FanModeEnum {
        if self.auto_mode.load(Ordering::Relaxed) {
            return FanModeEnum::Auto;
        }

        match self.percent.load(Ordering::Relaxed) {
            0 => FanModeEnum::Off,
            1..=33 => FanModeEnum::Low,
            34..=66 => FanModeEnum::Medium,
            _ => FanModeEnum::High,
        }
    }

    /// Read FanModeSequence (always OffLowMedHighAuto).
    pub fn fan_mode_sequence(&self) -> FanModeSequenceEnum {
        FanModeSequenceEnum::OffLowMedHighAuto
    }

    /// Read PercentSetting (Nullable<u8>, always non-null percentage 0..=100).
    pub fn percent_setting(&self) -> Nullable<u8> {
        Nullable::some(self.percent.load(Ordering::Relaxed))
    }

    /// Read PercentCurrent (u8).
    pub fn percent_current(&self) -> u8 {
        self.percent.load(Ordering::Relaxed)
    }

    /// Read SpeedMax (100%).
    pub fn speed_max(&self) -> u8 {
        100
    }

    /// Read SpeedSetting (Nullable<u8>, always non-null percentage 0..=100).
    pub fn speed_setting(&self) -> Nullable<u8> {
        Nullable::some(self.percent.load(Ordering::Relaxed))
    }

    /// Read SpeedCurrent (u8).
    pub fn speed_current(&self) -> u8 {
        self.percent.load(Ordering::Relaxed)
    }

    /// Read RockSupport (vertical up/down swing supported).
    pub fn rock_support(&self) -> RockBitmap {
        RockBitmap::ROCK_UP_DOWN
    }

    /// Read RockSetting bitmap.
    pub fn rock_setting(&self) -> RockBitmap {
        if self.swing_vertical.load(Ordering::Relaxed) {
            RockBitmap::ROCK_UP_DOWN
        } else {
            RockBitmap::empty()
        }
    }

    /// Write FanMode attribute and dispatch to handle.
    pub fn write_fan_mode(&self, mode: FanModeEnum) -> Result<(), Error> {
        if matches!(mode, FanModeEnum::Auto | FanModeEnum::Smart) {
            self.auto_mode.store(true, Ordering::SeqCst);
            self.dataver.changed();
            self.config.hvac.set_fan_speed(FanSpeed::Auto);
            return Ok(());
        }

        let p = match mode {
            FanModeEnum::Off => 0,
            FanModeEnum::Low => 33,
            FanModeEnum::Medium => 66,
            FanModeEnum::High => 100,
            FanModeEnum::On => DEFAULT_FAN_PERCENT,
            FanModeEnum::Auto | FanModeEnum::Smart => unreachable!(),
        };
        self.apply_manual_percent(p);
        Ok(())
    }

    /// Write PercentSetting attribute and dispatch to handle.
    pub fn write_percent_setting(&self, val: Nullable<u8>) -> Result<(), Error> {
        match val.into_option() {
            None => {
                self.auto_mode.store(true, Ordering::SeqCst);
                self.dataver.changed();
                self.config.hvac.set_fan_speed(FanSpeed::Auto);
                Ok(())
            }
            Some(p) if p <= 100 => {
                self.apply_manual_percent(p);
                Ok(())
            }
            Some(_) => Err(ErrorCode::ConstraintError.into()),
        }
    }

    /// Write SpeedSetting attribute and dispatch to handle.
    pub fn write_speed_setting(&self, val: Nullable<u8>) -> Result<(), Error> {
        self.write_percent_setting(val)
    }

    /// Write RockSetting attribute and dispatch to handle.
    pub fn write_rock_setting(&self, val: RockBitmap) -> Result<(), Error> {
        if !(val - RockBitmap::ROCK_UP_DOWN).is_empty() {
            return Err(ErrorCode::ConstraintError.into());
        }

        let v_swing = val.contains(RockBitmap::ROCK_UP_DOWN);
        self.swing_vertical.store(v_swing, Ordering::SeqCst);
        self.dataver.changed();

        let new_vane_v = if v_swing {
            VaneVertical::Swing
        } else {
            VaneVertical::Auto
        };
        self.config.hvac.set_vane_vertical(new_vane_v);
        Ok(())
    }

    fn apply_manual_percent(&self, p: u8) {
        self.auto_mode.store(false, Ordering::SeqCst);
        self.percent.store(p, Ordering::SeqCst);
        self.dataver.changed();

        let speed = match p {
            0 => FanSpeed::Auto,
            1..=20 => FanSpeed::Quiet,
            21..=40 => FanSpeed::Speed1,
            41..=60 => FanSpeed::Speed2,
            61..=80 => FanSpeed::Speed3,
            _ => FanSpeed::Speed4,
        };
        self.config.hvac.set_fan_speed(speed);
    }

    /// Cluster definition with MULTI_SPEED, AUTO, and ROCKING features (`0x0007`, rev 5).
    pub const CLUSTER: Cluster<'static> = fan_control::FULL_CLUSTER
        .with_revision(5)
        .with_features(
            fan_control::Feature::MULTI_SPEED.bits()
                | fan_control::Feature::AUTO.bits()
                | fan_control::Feature::ROCKING.bits(),
        )
        .with_attrs(with!(
            system;
            fan_control::AttributeId::FanMode
                | fan_control::AttributeId::FanModeSequence
                | fan_control::AttributeId::PercentSetting
                | fan_control::AttributeId::PercentCurrent
                | fan_control::AttributeId::SpeedMax
                | fan_control::AttributeId::SpeedSetting
                | fan_control::AttributeId::SpeedCurrent
                | fan_control::AttributeId::RockSupport
                | fan_control::AttributeId::RockSetting
        ))
        .with_cmds(with!());
}

impl<'a> MatterCluster for HvacFanControl<'a> {
    const ENDPOINT_ID: u16 = FAN_ENDPOINT_ID;
    const CLUSTER_DEF: Cluster<'static> = Self::CLUSTER;
    const DEVICE_TYPE: DeviceType = DEV_TYPE_ROOM_AIR_CONDITIONER;
}

impl<'a> AsyncHandler for HvacFanControl<'a> {
    async fn read(&self, ctx: impl ReadContext, reply: impl ReadReply) -> Result<(), Error> {
        self.config.readiness.mark_fan_control_ready();

        if let Some(writer) = reply.with_dataver(self.dataver.get())? {
            if ctx.attr().is_system() {
                Self::CLUSTER.read(ctx.attr(), writer)
            } else {
                match ctx.attr().attr_id as u16 {
                    x if x == fan_control::AttributeId::FanMode as u16 => {
                        Reply::set(writer, self.fan_mode())
                    }
                    x if x == fan_control::AttributeId::FanModeSequence as u16 => {
                        Reply::set(writer, self.fan_mode_sequence())
                    }
                    x if x == fan_control::AttributeId::PercentSetting as u16 => {
                        Reply::set(writer, self.percent_setting())
                    }
                    x if x == fan_control::AttributeId::PercentCurrent as u16 => {
                        Reply::set(writer, self.percent_current())
                    }
                    x if x == fan_control::AttributeId::SpeedMax as u16 => {
                        Reply::set(writer, self.speed_max())
                    }
                    x if x == fan_control::AttributeId::SpeedSetting as u16 => {
                        Reply::set(writer, self.speed_setting())
                    }
                    x if x == fan_control::AttributeId::SpeedCurrent as u16 => {
                        Reply::set(writer, self.speed_current())
                    }
                    x if x == fan_control::AttributeId::RockSupport as u16 => {
                        Reply::set(writer, self.rock_support())
                    }
                    x if x == fan_control::AttributeId::RockSetting as u16 => {
                        Reply::set(writer, self.rock_setting())
                    }
                    _ => Err(ErrorCode::AttributeNotFound.into()),
                }
            }
        } else {
            Ok(())
        }
    }

    async fn write(&self, ctx: impl WriteContext) -> Result<(), Error> {
        ctx.attr().check_dataver(self.dataver.get())?;
        match ctx.attr().attr_id as u16 {
            x if x == fan_control::AttributeId::FanMode as u16 => {
                let mode: FanModeEnum = FromTLV::from_tlv(ctx.data())?;
                self.write_fan_mode(mode)?;
                ctx.notify_changed();
                Ok(())
            }
            x if x == fan_control::AttributeId::PercentSetting as u16 => {
                let val: Nullable<u8> = FromTLV::from_tlv(ctx.data())?;
                self.write_percent_setting(val)?;
                ctx.notify_changed();
                Ok(())
            }
            x if x == fan_control::AttributeId::SpeedSetting as u16 => {
                let val: Nullable<u8> = FromTLV::from_tlv(ctx.data())?;
                self.write_speed_setting(val)?;
                ctx.notify_changed();
                Ok(())
            }
            x if x == fan_control::AttributeId::RockSetting as u16 => {
                let val: RockBitmap = FromTLV::from_tlv(ctx.data())?;
                self.write_rock_setting(val)?;
                ctx.notify_changed();
                Ok(())
            }
            _ => Err(ErrorCode::AttributeNotFound.into()),
        }
    }

    async fn invoke(
        &self,
        _ctx: impl InvokeContext,
        _reply: impl InvokeReply,
    ) -> Result<(), Error> {
        Err(ErrorCode::CommandNotFound.into())
    }

    fn bump_dataver(&self, ctx: impl MatchContext) {
        if ctx.endpt() == Some(Self::ENDPOINT_ID) && ctx.cluster() == Some(Self::CLUSTER.id) {
            self.dataver.changed();
        }
    }

    fn run(
        &self,
        ctx: impl HandlerContext,
    ) -> impl core::future::Future<Output = Result<(), Error>> {
        async move {
            loop {
                self.state_changed.wait().await;
                info!("fan_control run() woke -> notifying cluster attributes changed");
                ctx.notify_cluster_changed(Self::ENDPOINT_ID, Self::CLUSTER.id);
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use hvac_core::domain::{HvacMode, TemperatureCentiC};
    use hvac_core::events::SystemStateWatch;
    use hvac_core::ports::CommandChannel;

    static WATCH: SystemStateWatch = SystemStateWatch::new();
    static BARRIER: ClusterReadinessBarrier =
        ClusterReadinessBarrier::new(hvac_core::events::SystemStateSender::new(&WATCH));

    fn create_test_fan_control<'a>(
        channel: &'a CommandChannel,
    ) -> HvacFanControl<'a> {
        let handle = HvacControllerHandle::new(channel.sender());
        HvacFanControl::new(HvacFanControlConfig {
            hvac: handle,
            readiness: &BARRIER,
        })
    }

    #[test]
    fn initial_state_matches_defaults() {
        // Arrange
        let channel = CommandChannel::new();
        let fan = create_test_fan_control(&channel);

        // Act & Assert
        assert_eq!(fan.fan_mode(), FanModeEnum::Auto);
        assert_eq!(fan.fan_mode_sequence(), FanModeSequenceEnum::OffLowMedHighAuto);
        assert_eq!(fan.percent_setting(), Nullable::some(DEFAULT_FAN_PERCENT));
        assert_eq!(fan.percent_current(), DEFAULT_FAN_PERCENT);
        assert_eq!(fan.speed_max(), 100);
        assert_eq!(fan.speed_setting(), Nullable::some(DEFAULT_FAN_PERCENT));
        assert_eq!(fan.speed_current(), DEFAULT_FAN_PERCENT);
        assert_eq!(fan.rock_support(), RockBitmap::ROCK_UP_DOWN);
        assert_eq!(fan.rock_setting(), RockBitmap::empty());
    }

    #[test]
    fn update_state_reflects_fan_speed_and_rocking() {
        // Arrange
        let channel = CommandChannel::new();
        let fan = create_test_fan_control(&channel);
        let state = HvacState {
            connected: true,
            power: true,
            mode: HvacMode::Cool,
            target_temp: TemperatureCentiC::DEFAULT_SETPOINT,
            room_temp: Some(TemperatureCentiC::DEFAULT_SETPOINT),
            fan_speed: FanSpeed::Speed3,
            vane_vertical: VaneVertical::Swing,
            vane_horizontal: VaneHorizontal::Swing,
            wide_vane_adjustment: true,
            operating: true,
            compressor_freq_hz: Some(30),
        };

        // Act
        fan.update_state(&state);

        // Assert
        assert_eq!(fan.fan_mode(), FanModeEnum::High);
        assert_eq!(fan.percent_setting(), Nullable::some(80));
        assert_eq!(fan.percent_current(), 80);
        assert_eq!(fan.speed_setting(), Nullable::some(80));
        assert_eq!(fan.speed_current(), 80);
        assert_eq!(fan.rock_setting(), RockBitmap::ROCK_UP_DOWN);
    }

    #[test]
    fn update_state_reflects_auto_fan_mode() {
        // Arrange
        let channel = CommandChannel::new();
        let fan = create_test_fan_control(&channel);
        let state = HvacState {
            connected: true,
            power: true,
            mode: HvacMode::Heat,
            target_temp: TemperatureCentiC::DEFAULT_SETPOINT,
            room_temp: Some(TemperatureCentiC::DEFAULT_SETPOINT),
            fan_speed: FanSpeed::Auto,
            vane_vertical: VaneVertical::Pos3,
            vane_horizontal: VaneHorizontal::Center,
            wide_vane_adjustment: true,
            operating: true,
            compressor_freq_hz: Some(40),
        };

        // Act
        fan.update_state(&state);

        // Assert
        assert_eq!(fan.fan_mode(), FanModeEnum::Auto);
        assert_eq!(fan.percent_setting(), Nullable::some(DEFAULT_FAN_PERCENT));
        assert_eq!(fan.percent_current(), DEFAULT_FAN_PERCENT);
        assert_eq!(fan.speed_setting(), Nullable::some(DEFAULT_FAN_PERCENT));
        assert_eq!(fan.speed_current(), DEFAULT_FAN_PERCENT);
        assert_eq!(fan.rock_setting(), RockBitmap::empty());
    }

    #[test]
    fn write_fan_mode_dispatches_correct_patch() {
        // Arrange
        let channel = CommandChannel::new();
        let fan = create_test_fan_control(&channel);

        // Act - Off (maps 0% -> Auto without mutating AC power)
        assert!(fan.write_fan_mode(FanModeEnum::Off).is_ok());

        // Assert
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.power, None);
        assert_eq!(patch.fan_speed, Some(FanSpeed::Auto));

        // Act - Low (33% -> Speed1)
        assert!(fan.write_fan_mode(FanModeEnum::Low).is_ok());

        // Assert
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.power, None);
        assert_eq!(patch.fan_speed, Some(FanSpeed::Speed1));

        // Act - Medium (66% -> Speed3)
        assert!(fan.write_fan_mode(FanModeEnum::Medium).is_ok());

        // Assert
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.power, None);
        assert_eq!(patch.fan_speed, Some(FanSpeed::Speed3));

        // Act - High (100% -> Speed4)
        assert!(fan.write_fan_mode(FanModeEnum::High).is_ok());

        // Assert
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.power, None);
        assert_eq!(patch.fan_speed, Some(FanSpeed::Speed4));

        // Act - Auto
        assert!(fan.write_fan_mode(FanModeEnum::Auto).is_ok());

        // Assert
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.power, None);
        assert_eq!(patch.fan_speed, Some(FanSpeed::Auto));
    }

    #[test]
    fn write_percent_setting_maps_to_speed_tiers() {
        // Arrange
        let channel = CommandChannel::new();
        let fan = create_test_fan_control(&channel);

        // Act - Null -> Auto
        assert!(fan.write_percent_setting(Nullable::none()).is_ok());
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.fan_speed, Some(FanSpeed::Auto));

        // Act - 0 -> Auto (without turning AC off)
        assert!(fan.write_percent_setting(Nullable::some(0)).is_ok());
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.power, None);
        assert_eq!(patch.fan_speed, Some(FanSpeed::Auto));

        // Act - 20% -> Quiet
        assert!(fan.write_percent_setting(Nullable::some(20)).is_ok());
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.fan_speed, Some(FanSpeed::Quiet));

        // Act - 40% -> Speed1
        assert!(fan.write_percent_setting(Nullable::some(40)).is_ok());
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.fan_speed, Some(FanSpeed::Speed1));

        // Act - 60% -> Speed2
        assert!(fan.write_percent_setting(Nullable::some(60)).is_ok());
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.fan_speed, Some(FanSpeed::Speed2));

        // Act - 80% -> Speed3
        assert!(fan.write_percent_setting(Nullable::some(80)).is_ok());
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.fan_speed, Some(FanSpeed::Speed3));

        // Act - 100% -> Speed4
        assert!(fan.write_percent_setting(Nullable::some(100)).is_ok());
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.fan_speed, Some(FanSpeed::Speed4));

        // Act - > 100% -> Error
        assert!(fan.write_percent_setting(Nullable::some(101)).is_err());
    }

    #[test]
    fn write_speed_setting_maps_speeds_correctly() {
        // Arrange
        let channel = CommandChannel::new();
        let fan = create_test_fan_control(&channel);

        // Act - Null -> Auto
        assert!(fan.write_speed_setting(Nullable::none()).is_ok());
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.fan_speed, Some(FanSpeed::Auto));

        // Act - 20 -> Quiet
        assert!(fan.write_speed_setting(Nullable::some(20)).is_ok());
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.fan_speed, Some(FanSpeed::Quiet));

        // Act - 100 -> Speed4
        assert!(fan.write_speed_setting(Nullable::some(100)).is_ok());
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.fan_speed, Some(FanSpeed::Speed4));

        // Act - 101 -> Error
        assert!(fan.write_speed_setting(Nullable::some(101)).is_err());
    }

    #[test]
    fn write_rock_setting_enables_or_disables_vertical_swing() {
        // Arrange
        let channel = CommandChannel::new();
        let fan = create_test_fan_control(&channel);

        // Act - Unsupported horizontal rock bit returns ConstraintError
        assert!(fan
            .write_rock_setting(RockBitmap::ROCK_UP_DOWN | RockBitmap::ROCK_LEFT_RIGHT)
            .is_err());

        // Act - Enable vertical swing
        assert!(fan.write_rock_setting(RockBitmap::ROCK_UP_DOWN).is_ok());

        // Assert
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.vane_vertical, Some(VaneVertical::Swing));
        assert_eq!(fan.rock_setting(), RockBitmap::ROCK_UP_DOWN);

        // Act - Disable vertical swing
        assert!(fan.write_rock_setting(RockBitmap::empty()).is_ok());

        // Assert
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.vane_vertical, Some(VaneVertical::Auto));
        assert_eq!(fan.rock_setting(), RockBitmap::empty());
    }

    #[test]
    fn cluster_includes_global_system_attributes() {
        // Arrange & Act & Assert
        assert!(HvacFanControl::CLUSTER.attribute(0xFFF8).is_some());
        assert!(HvacFanControl::CLUSTER.attribute(0xFFF9).is_some());
        assert!(HvacFanControl::CLUSTER.attribute(0xFFFB).is_some());
        assert!(HvacFanControl::CLUSTER.attribute(0xFFFC).is_some());
        assert!(HvacFanControl::CLUSTER.attribute(0xFFFD).is_some());
    }
}
