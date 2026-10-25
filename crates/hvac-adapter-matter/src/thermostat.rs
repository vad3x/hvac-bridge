//! HVAC Bridge - Matter Thermostat Cluster Handler
//!
//! Exposes the Matter Thermostat cluster (`0x0201`, rev 7) on Endpoint 1 (`DEV_TYPE_ROOM_AIR_CONDITIONER` `0x0072`).
//! Maps HVAC thermostat state to Matter attributes and dispatches controller mutations to `HvacControllerHandle`.

use core::sync::atomic::Ordering;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::signal::Signal;
use log::info;
use portable_atomic::{AtomicBool, AtomicI16, AtomicU8};

use rs_matter::dm::clusters::decl::thermostat::{
    self, ControlSequenceOfOperationEnum, RelayStateBitmap, SetpointRaiseLowerModeEnum,
    SystemModeEnum, ThermostatRunningModeEnum,
};
use rs_matter::dm::{
    AsyncHandler, Cluster, Dataver, DeviceType, HandlerContext, InvokeContext, InvokeReply,
    MatchContext, ReadContext, ReadReply, Reply, WriteContext,
};
use rs_matter::error::{Error, ErrorCode};
use rs_matter::tlv::{FromTLV, Nullable};
use rs_matter::with;

use hvac_core::domain::{HvacMode, HvacState, TemperatureCentiC};
use hvac_core::ports::HvacControllerHandle;

use crate::config::{DEV_TYPE_ROOM_AIR_CONDITIONER, THERMOSTAT_ENDPOINT_ID};
use crate::readiness::ClusterReadinessBarrier;
use crate::MatterCluster;

// ============================================================================
// Public Constants & Types
// ============================================================================

/// Default initial cooling setpoint in centi-°C (`24.0 °C`).
pub const DEFAULT_COOLING_SETPOINT: i16 = 2400;

/// Default initial heating setpoint in centi-°C (`20.0 °C`).
pub const DEFAULT_HEATING_SETPOINT: i16 = 2000;

/// Absolute minimum heating setpoint limit in centi-°C (`10.0 °C`).
pub const ABS_MIN_HEAT_SETPOINT_LIMIT: i16 = 1000;

/// Configuration for [`HvacThermostat`].
pub struct HvacThermostatConfig<'a> {
    /// Inbound driving port handle to dispatch HVAC mutation commands.
    pub hvac: HvacControllerHandle<'a>,
    /// Cluster readiness barrier to track initial subscription priming.
    pub readiness: &'static ClusterReadinessBarrier,
}

/// Matter Thermostat cluster handler on Endpoint 1.
pub struct HvacThermostat<'a> {
    config: HvacThermostatConfig<'a>,
    dataver: Dataver,
    power: AtomicBool,
    mode: AtomicU8,
    cooling_setpoint: AtomicI16,
    heating_setpoint: AtomicI16,
    room_temp: AtomicI16,
    operating: AtomicBool,
    state_changed: Signal<NoopRawMutex, ()>,
}

impl<'a> HvacThermostat<'a> {
    /// Create a new HVAC Thermostat handler with nominal initial setpoints.
    pub fn new(config: HvacThermostatConfig<'a>) -> Self {
        Self {
            config,
            dataver: Dataver::new(1),
            power: AtomicBool::new(false),
            mode: AtomicU8::new(HvacMode::Cool as u8),
            cooling_setpoint: AtomicI16::new(DEFAULT_COOLING_SETPOINT),
            heating_setpoint: AtomicI16::new(DEFAULT_HEATING_SETPOINT),
            room_temp: AtomicI16::new(TemperatureCentiC::DEFAULT_SETPOINT.0),
            operating: AtomicBool::new(false),
            state_changed: Signal::new(),
        }
    }

    /// Update internal state from an out-of-band canonical HVAC state snapshot.
    ///
    /// Updates internal atomic registers, bumps `dataver`, and signals `state_changed`
    /// to notify active Matter subscriptions.
    pub fn update_state(&self, state: &HvacState) {
        let prev_power = self.power.swap(state.power, Ordering::SeqCst);
        let prev_mode = self.mode.swap(state.mode as u8, Ordering::SeqCst);

        let setpoint_changed = match state.mode {
            HvacMode::Heat => {
                let prev = self
                    .heating_setpoint
                    .swap(state.target_temp.0, Ordering::SeqCst);
                prev != state.target_temp.0
            }
            HvacMode::Cool | HvacMode::Dry | HvacMode::FanOnly => {
                let prev = self
                    .cooling_setpoint
                    .swap(state.target_temp.0, Ordering::SeqCst);
                prev != state.target_temp.0
            }
            HvacMode::Auto => {
                let prev_c = self
                    .cooling_setpoint
                    .swap(state.target_temp.0, Ordering::SeqCst);
                let prev_h = self
                    .heating_setpoint
                    .swap(state.target_temp.0, Ordering::SeqCst);
                prev_c != state.target_temp.0 || prev_h != state.target_temp.0
            }
        };

        let room_changed = if let Some(room_temp) = state.room_temp {
            let prev_room = self.room_temp.swap(room_temp.0, Ordering::SeqCst);
            prev_room != room_temp.0
        } else {
            false
        };

        let prev_operating = self.operating.swap(state.operating, Ordering::SeqCst);

        if prev_power != state.power
            || prev_mode != state.mode as u8
            || setpoint_changed
            || room_changed
            || prev_operating != state.operating
        {
            info!("HvacThermostat state changed -> bumping dataver and signaling subscriber notify");
            self.dataver.changed();
            self.state_changed.signal(());
        }
    }

    /// Read the local room temperature as a Matter `Nullable<i16>` in centi-°C.
    pub fn local_temperature(&self) -> Nullable<i16> {
        Nullable::some(self.room_temp.load(Ordering::Relaxed))
    }

    /// Read the occupied cooling setpoint in centi-°C.
    pub fn occupied_cooling_setpoint(&self) -> i16 {
        self.cooling_setpoint.load(Ordering::Relaxed)
    }

    /// Read the occupied heating setpoint in centi-°C.
    pub fn occupied_heating_setpoint(&self) -> i16 {
        self.heating_setpoint.load(Ordering::Relaxed)
    }

    /// Read the current Matter SystemModeEnum.
    ///
    /// Conforms to `Feature::HEATING | Feature::COOLING` (returns `Off`, `Heat`, or `Cool`).
    pub fn system_mode(&self) -> SystemModeEnum {
        if !self.power.load(Ordering::Relaxed) {
            return SystemModeEnum::Off;
        }

        match self.mode.load(Ordering::Relaxed) {
            x if x == HvacMode::Heat as u8 => SystemModeEnum::Heat,
            _ => SystemModeEnum::Cool,
        }
    }

    /// Read the current Matter ThermostatRunningModeEnum.
    pub fn thermostat_running_mode(&self) -> ThermostatRunningModeEnum {
        let power = self.power.load(Ordering::Relaxed);
        let operating = self.operating.load(Ordering::Relaxed);
        if !power || !operating {
            return ThermostatRunningModeEnum::Off;
        }

        match self.mode.load(Ordering::Relaxed) {
            x if x == HvacMode::Heat as u8 => ThermostatRunningModeEnum::Heat,
            x if x == HvacMode::Cool as u8 || x == HvacMode::Dry as u8 => {
                ThermostatRunningModeEnum::Cool
            }
            x if x == HvacMode::Auto as u8 => {
                let target = self.cooling_setpoint.load(Ordering::Relaxed);
                let room = self.room_temp.load(Ordering::Relaxed);
                if target > room {
                    ThermostatRunningModeEnum::Heat
                } else {
                    ThermostatRunningModeEnum::Cool
                }
            }
            _ => ThermostatRunningModeEnum::Off,
        }
    }

    /// Read the current Matter RelayStateBitmap (running state).
    pub fn thermostat_running_state(&self) -> RelayStateBitmap {
        let power = self.power.load(Ordering::Relaxed);
        let operating = self.operating.load(Ordering::Relaxed);
        if !power || !operating {
            return RelayStateBitmap::empty();
        }

        match self.mode.load(Ordering::Relaxed) {
            x if x == HvacMode::Heat as u8 => RelayStateBitmap::HEAT | RelayStateBitmap::FAN,
            x if x == HvacMode::Cool as u8 || x == HvacMode::Dry as u8 => {
                RelayStateBitmap::COOL | RelayStateBitmap::FAN
            }
            x if x == HvacMode::FanOnly as u8 => RelayStateBitmap::FAN,
            x if x == HvacMode::Auto as u8 => {
                let target = self.cooling_setpoint.load(Ordering::Relaxed);
                let room = self.room_temp.load(Ordering::Relaxed);
                if target > room {
                    RelayStateBitmap::HEAT | RelayStateBitmap::FAN
                } else {
                    RelayStateBitmap::COOL | RelayStateBitmap::FAN
                }
            }
            _ => RelayStateBitmap::empty(),
        }
    }

    /// Write cooling setpoint attribute in centi-°C and dispatch to handle.
    pub fn write_cooling_setpoint(&self, val: i16) {
        let clamped = val.clamp(
            TemperatureCentiC::MIN_SETPOINT.0,
            TemperatureCentiC::MAX_SETPOINT.0,
        );
        self.cooling_setpoint.store(clamped, Ordering::SeqCst);
        self.dataver.changed();
        self.config.hvac.set_target_temp(TemperatureCentiC(clamped));
    }

    /// Write heating setpoint attribute in centi-°C and dispatch to handle.
    pub fn write_heating_setpoint(&self, val: i16) {
        let clamped = val.clamp(
            TemperatureCentiC::MIN_SETPOINT.0,
            TemperatureCentiC::MAX_SETPOINT.0,
        );
        self.heating_setpoint.store(clamped, Ordering::SeqCst);
        self.dataver.changed();
        self.config.hvac.set_target_temp(TemperatureCentiC(clamped));
    }

    /// Write system mode attribute and dispatch to handle.
    pub fn write_system_mode(&self, mode: SystemModeEnum) -> Result<(), Error> {
        match mode {
            SystemModeEnum::Off => {
                self.power.store(false, Ordering::SeqCst);
                self.dataver.changed();
                self.config.hvac.set_power(false);
                Ok(())
            }
            SystemModeEnum::Heat => {
                self.power.store(true, Ordering::SeqCst);
                self.mode.store(HvacMode::Heat as u8, Ordering::SeqCst);
                self.dataver.changed();
                self.config.hvac.set_power_and_mode(true, HvacMode::Heat);
                Ok(())
            }
            SystemModeEnum::Cool => {
                self.power.store(true, Ordering::SeqCst);
                self.mode.store(HvacMode::Cool as u8, Ordering::SeqCst);
                self.dataver.changed();
                self.config.hvac.set_power_and_mode(true, HvacMode::Cool);
                Ok(())
            }
            _ => Err(ErrorCode::ConstraintError.into()),
        }
    }

    /// Process the `SetpointRaiseLower` command with steps of 0.1 °C (10 centi-°C).
    pub fn handle_raise_lower_command(&self, amount: i8) {
        self.handle_raise_lower_mode_command(SetpointRaiseLowerModeEnum::Both, amount);
    }

    /// Process the `SetpointRaiseLower` command for a specific mode (`Heat`, `Cool`, or `Both`).
    pub fn handle_raise_lower_mode_command(
        &self,
        raise_mode: SetpointRaiseLowerModeEnum,
        amount: i8,
    ) {
        let delta = (amount as i16) * 10;
        let current_mode = self.mode.load(Ordering::Relaxed);

        match raise_mode {
            SetpointRaiseLowerModeEnum::Heat => {
                let current = self.heating_setpoint.load(Ordering::Relaxed);
                let new_temp = (current + delta).clamp(
                    ABS_MIN_HEAT_SETPOINT_LIMIT,
                    TemperatureCentiC::MAX_SETPOINT.0,
                );
                self.heating_setpoint.store(new_temp, Ordering::SeqCst);
                self.dataver.changed();
                self.config
                    .hvac
                    .set_target_temp(TemperatureCentiC(new_temp));
            }
            SetpointRaiseLowerModeEnum::Cool => {
                let current = self.cooling_setpoint.load(Ordering::Relaxed);
                let new_temp = (current + delta).clamp(
                    TemperatureCentiC::MIN_SETPOINT.0,
                    TemperatureCentiC::MAX_SETPOINT.0,
                );
                self.cooling_setpoint.store(new_temp, Ordering::SeqCst);
                self.dataver.changed();
                self.config
                    .hvac
                    .set_target_temp(TemperatureCentiC(new_temp));
            }
            SetpointRaiseLowerModeEnum::Both => {
                let new_cool = (self.cooling_setpoint.load(Ordering::Relaxed) + delta).clamp(
                    TemperatureCentiC::MIN_SETPOINT.0,
                    TemperatureCentiC::MAX_SETPOINT.0,
                );
                let new_heat = (self.heating_setpoint.load(Ordering::Relaxed) + delta).clamp(
                    ABS_MIN_HEAT_SETPOINT_LIMIT,
                    TemperatureCentiC::MAX_SETPOINT.0,
                );
                self.cooling_setpoint.store(new_cool, Ordering::SeqCst);
                self.heating_setpoint.store(new_heat, Ordering::SeqCst);
                self.dataver.changed();
                let active_temp = if current_mode == HvacMode::Heat as u8 {
                    new_heat
                } else {
                    new_cool
                };
                self.config
                    .hvac
                    .set_target_temp(TemperatureCentiC(active_temp));
            }
        }
    }

    /// Cluster definition with Heating and Cooling features (`0x0003`, rev 7).
    pub const CLUSTER: Cluster<'static> = thermostat::FULL_CLUSTER
        .with_revision(7)
        .with_features(
            thermostat::Feature::HEATING.bits() | thermostat::Feature::COOLING.bits(),
        )
        .with_attrs(with!(
            system;
            thermostat::AttributeId::LocalTemperature
                | thermostat::AttributeId::AbsMinHeatSetpointLimit
                | thermostat::AttributeId::AbsMaxHeatSetpointLimit
                | thermostat::AttributeId::AbsMinCoolSetpointLimit
                | thermostat::AttributeId::AbsMaxCoolSetpointLimit
                | thermostat::AttributeId::OccupiedCoolingSetpoint
                | thermostat::AttributeId::OccupiedHeatingSetpoint
                | thermostat::AttributeId::MinHeatSetpointLimit
                | thermostat::AttributeId::MaxHeatSetpointLimit
                | thermostat::AttributeId::MinCoolSetpointLimit
                | thermostat::AttributeId::MaxCoolSetpointLimit
                | thermostat::AttributeId::ControlSequenceOfOperation
                | thermostat::AttributeId::SystemMode
        ))
        .with_cmds(with!(thermostat::CommandId::SetpointRaiseLower));
}

impl<'a> MatterCluster for HvacThermostat<'a> {
    const ENDPOINT_ID: u16 = THERMOSTAT_ENDPOINT_ID;
    const CLUSTER_DEF: Cluster<'static> = Self::CLUSTER;
    const DEVICE_TYPE: DeviceType = DEV_TYPE_ROOM_AIR_CONDITIONER;
}

impl<'a> AsyncHandler for HvacThermostat<'a> {
    async fn read(&self, ctx: impl ReadContext, reply: impl ReadReply) -> Result<(), Error> {
        self.config.readiness.mark_thermostat_ready();

        if let Some(writer) = reply.with_dataver(self.dataver.get())? {
            if ctx.attr().is_system() {
                Self::CLUSTER.read(ctx.attr(), writer)
            } else {
                match ctx.attr().attr_id as u16 {
                    x if x == thermostat::AttributeId::LocalTemperature as u16 => {
                        Reply::set(writer, self.local_temperature())
                    }
                    x if x == thermostat::AttributeId::AbsMinHeatSetpointLimit as u16 => {
                        Reply::set(writer, ABS_MIN_HEAT_SETPOINT_LIMIT)
                    }
                    x if x == thermostat::AttributeId::AbsMaxHeatSetpointLimit as u16 => {
                        Reply::set(writer, TemperatureCentiC::MAX_SETPOINT.0)
                    }
                    x if x == thermostat::AttributeId::AbsMinCoolSetpointLimit as u16 => {
                        Reply::set(writer, TemperatureCentiC::MIN_SETPOINT.0)
                    }
                    x if x == thermostat::AttributeId::AbsMaxCoolSetpointLimit as u16 => {
                        Reply::set(writer, TemperatureCentiC::MAX_SETPOINT.0)
                    }
                    x if x == thermostat::AttributeId::OccupiedCoolingSetpoint as u16 => {
                        Reply::set(writer, self.occupied_cooling_setpoint())
                    }
                    x if x == thermostat::AttributeId::OccupiedHeatingSetpoint as u16 => {
                        Reply::set(writer, self.occupied_heating_setpoint())
                    }
                    x if x == thermostat::AttributeId::MinHeatSetpointLimit as u16 => {
                        Reply::set(writer, TemperatureCentiC::MIN_SETPOINT.0)
                    }
                    x if x == thermostat::AttributeId::MaxHeatSetpointLimit as u16 => {
                        Reply::set(writer, TemperatureCentiC::MAX_SETPOINT.0)
                    }
                    x if x == thermostat::AttributeId::MinCoolSetpointLimit as u16 => {
                        Reply::set(writer, TemperatureCentiC::MIN_SETPOINT.0)
                    }
                    x if x == thermostat::AttributeId::MaxCoolSetpointLimit as u16 => {
                        Reply::set(writer, TemperatureCentiC::MAX_SETPOINT.0)
                    }
                    x if x == thermostat::AttributeId::ControlSequenceOfOperation as u16 => {
                        Reply::set(
                            writer,
                            ControlSequenceOfOperationEnum::CoolingAndHeating,
                        )
                    }
                    x if x == thermostat::AttributeId::SystemMode as u16 => {
                        Reply::set(writer, self.system_mode())
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
            x if x == thermostat::AttributeId::OccupiedCoolingSetpoint as u16 => {
                let val: i16 = FromTLV::from_tlv(ctx.data())?;
                self.write_cooling_setpoint(val);
                ctx.notify_changed();
                Ok(())
            }
            x if x == thermostat::AttributeId::OccupiedHeatingSetpoint as u16 => {
                let val: i16 = FromTLV::from_tlv(ctx.data())?;
                self.write_heating_setpoint(val);
                ctx.notify_changed();
                Ok(())
            }
            x if x == thermostat::AttributeId::SystemMode as u16 => {
                let mode: SystemModeEnum = FromTLV::from_tlv(ctx.data())?;
                self.write_system_mode(mode)?;
                ctx.notify_changed();
                Ok(())
            }
            _ => Err(ErrorCode::AttributeNotFound.into()),
        }
    }

    async fn invoke(&self, ctx: impl InvokeContext, _reply: impl InvokeReply) -> Result<(), Error> {
        match ctx.cmd().cmd_id as u16 {
            x if x == thermostat::CommandId::SetpointRaiseLower as u16 => {
                let req: thermostat::SetpointRaiseLowerRequest = FromTLV::from_tlv(ctx.data())?;
                let raise_mode = req.mode()?;
                let amount = req.amount()?;
                self.handle_raise_lower_mode_command(raise_mode, amount);
                ctx.notify_attr_changed(
                    ctx.cmd().endpoint_id,
                    ctx.cmd().cluster_id,
                    thermostat::AttributeId::OccupiedCoolingSetpoint as _,
                );
                ctx.notify_attr_changed(
                    ctx.cmd().endpoint_id,
                    ctx.cmd().cluster_id,
                    thermostat::AttributeId::OccupiedHeatingSetpoint as _,
                );
                Ok(())
            }
            _ => Err(ErrorCode::CommandNotFound.into()),
        }
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
                info!("thermostat run() woke -> notifying cluster attributes changed");
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
    use hvac_core::domain::{FanSpeed, VaneHorizontal, VaneVertical};
    use hvac_core::events::SystemStateWatch;
    use hvac_core::ports::CommandChannel;

    static WATCH: SystemStateWatch = SystemStateWatch::new();
    static BARRIER: ClusterReadinessBarrier =
        ClusterReadinessBarrier::new(hvac_core::events::SystemStateSender::new(&WATCH));

    fn create_test_thermostat<'a>(channel: &'a CommandChannel) -> HvacThermostat<'a> {
        let handle = HvacControllerHandle::new(channel.sender());
        HvacThermostat::new(HvacThermostatConfig {
            hvac: handle,
            readiness: &BARRIER,
        })
    }

    #[test]
    fn initial_state_matches_defaults() {
        // Arrange
        let channel = CommandChannel::new();
        let thermostat = create_test_thermostat(&channel);

        // Act & Assert
        assert_eq!(thermostat.system_mode(), SystemModeEnum::Off);
        assert_eq!(
            thermostat.thermostat_running_mode(),
            ThermostatRunningModeEnum::Off
        );
        assert_eq!(
            thermostat.thermostat_running_state(),
            RelayStateBitmap::empty()
        );
        assert_eq!(
            thermostat.occupied_cooling_setpoint(),
            DEFAULT_COOLING_SETPOINT
        );
        assert_eq!(
            thermostat.occupied_heating_setpoint(),
            DEFAULT_HEATING_SETPOINT
        );
        assert_eq!(
            thermostat.local_temperature(),
            Nullable::some(TemperatureCentiC::DEFAULT_SETPOINT.0)
        );
    }

    #[test]
    fn update_state_reflects_room_temp_and_setpoint() {
        // Arrange
        let channel = CommandChannel::new();
        let thermostat = create_test_thermostat(&channel);
        let state = HvacState {
            connected: true,
            power: true,
            mode: HvacMode::Cool,
            target_temp: TemperatureCentiC::from_centi(2350),
            room_temp: Some(TemperatureCentiC::from_centi(2400)),
            fan_speed: FanSpeed::Speed2,
            vane_vertical: VaneVertical::Auto,
            vane_horizontal: VaneHorizontal::Auto,
            wide_vane_adjustment: true,
            operating: true,
            compressor_freq_hz: Some(45),
        };

        // Act
        thermostat.update_state(&state);

        // Assert
        assert_eq!(thermostat.local_temperature(), Nullable::some(2400));
        assert_eq!(thermostat.occupied_cooling_setpoint(), 2350);
        assert_eq!(
            thermostat.occupied_heating_setpoint(),
            DEFAULT_HEATING_SETPOINT
        );
        assert_eq!(thermostat.system_mode(), SystemModeEnum::Cool);
        assert_eq!(
            thermostat.thermostat_running_mode(),
            ThermostatRunningModeEnum::Cool
        );
        assert_eq!(
            thermostat.thermostat_running_state(),
            RelayStateBitmap::COOL | RelayStateBitmap::FAN
        );
    }

    #[test]
    fn running_mode_and_state_for_heating_operating() {
        // Arrange
        let channel = CommandChannel::new();
        let thermostat = create_test_thermostat(&channel);
        let state = HvacState {
            connected: true,
            power: true,
            mode: HvacMode::Heat,
            target_temp: TemperatureCentiC::from_centi(2100),
            room_temp: Some(TemperatureCentiC::from_centi(1900)),
            fan_speed: FanSpeed::Auto,
            vane_vertical: VaneVertical::Auto,
            vane_horizontal: VaneHorizontal::Auto,
            wide_vane_adjustment: true,
            operating: true,
            compressor_freq_hz: Some(50),
        };

        // Act
        thermostat.update_state(&state);

        // Assert
        assert_eq!(thermostat.system_mode(), SystemModeEnum::Heat);
        assert_eq!(thermostat.occupied_heating_setpoint(), 2100);
        assert_eq!(
            thermostat.thermostat_running_mode(),
            ThermostatRunningModeEnum::Heat
        );
        assert_eq!(
            thermostat.thermostat_running_state(),
            RelayStateBitmap::HEAT | RelayStateBitmap::FAN
        );
    }

    #[test]
    fn running_mode_and_state_when_idle_with_power_on() {
        // Arrange
        let channel = CommandChannel::new();
        let thermostat = create_test_thermostat(&channel);
        let state = HvacState {
            connected: true,
            power: true,
            mode: HvacMode::Cool,
            target_temp: TemperatureCentiC::from_centi(2200),
            room_temp: Some(TemperatureCentiC::from_centi(2200)),
            fan_speed: FanSpeed::Quiet,
            vane_vertical: VaneVertical::Auto,
            vane_horizontal: VaneHorizontal::Auto,
            wide_vane_adjustment: true,
            operating: false,
            compressor_freq_hz: None,
        };

        // Act
        thermostat.update_state(&state);

        // Assert
        assert_eq!(thermostat.system_mode(), SystemModeEnum::Cool);
        assert_eq!(
            thermostat.thermostat_running_mode(),
            ThermostatRunningModeEnum::Off
        );
        assert_eq!(
            thermostat.thermostat_running_state(),
            RelayStateBitmap::empty()
        );
    }

    #[test]
    fn write_setpoint_clamps_and_dispatches_command() {
        // Arrange
        let channel = CommandChannel::new();
        let thermostat = create_test_thermostat(&channel);

        // Act - write within bounds
        thermostat.write_cooling_setpoint(2500);

        // Assert
        assert_eq!(thermostat.occupied_cooling_setpoint(), 2500);
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.target_temp, Some(TemperatureCentiC::from_centi(2500)));

        // Act - write above max limit (3100)
        thermostat.write_heating_setpoint(3500);

        // Assert
        assert_eq!(thermostat.occupied_heating_setpoint(), 3100);
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.target_temp, Some(TemperatureCentiC::from_centi(3100)));
    }

    #[test]
    fn write_system_mode_dispatches_correct_power_and_mode() {
        // Arrange
        let channel = CommandChannel::new();
        let thermostat = create_test_thermostat(&channel);

        // Act - Off
        assert!(thermostat.write_system_mode(SystemModeEnum::Off).is_ok());

        // Assert
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.power, Some(false));

        // Act - Heat
        assert!(thermostat.write_system_mode(SystemModeEnum::Heat).is_ok());

        // Assert
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.power, Some(true));
        assert_eq!(patch.mode, Some(HvacMode::Heat));

        // Act - Cool
        assert!(thermostat.write_system_mode(SystemModeEnum::Cool).is_ok());

        // Assert
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.power, Some(true));
        assert_eq!(patch.mode, Some(HvacMode::Cool));
    }

    #[test]
    fn handle_raise_lower_command_adjusts_by_tenth_degree_steps() {
        // Arrange
        let channel = CommandChannel::new();
        let thermostat = create_test_thermostat(&channel);
        thermostat.cooling_setpoint.store(2200, Ordering::Relaxed);

        // Act - raise by 5 (+0.5 °C = +50 centi-°C)
        thermostat.handle_raise_lower_command(5);

        // Assert
        assert_eq!(thermostat.occupied_cooling_setpoint(), 2250);
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.target_temp, Some(TemperatureCentiC::from_centi(2250)));

        // Act - lower by 10 (-1.0 °C = -100 centi-°C)
        thermostat.handle_raise_lower_command(-10);

        // Assert
        assert_eq!(thermostat.occupied_cooling_setpoint(), 2150);
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.target_temp, Some(TemperatureCentiC::from_centi(2150)));
    }

    #[test]
    fn thermostat_cluster_revision_and_device_type_conform_to_room_air_conditioner() {
        // Arrange & Act & Assert
        assert_eq!(HvacThermostat::CLUSTER.revision, 7);
        assert_eq!(<HvacThermostat as MatterCluster>::DEVICE_TYPE.dtype, 0x0072);
        assert_eq!(<HvacThermostat as MatterCluster>::DEVICE_TYPE.drev, 2);
        assert!(HvacThermostat::CLUSTER.attribute(0xFFF8).is_some());
        assert!(HvacThermostat::CLUSTER.attribute(0xFFF9).is_some());
        assert!(HvacThermostat::CLUSTER.attribute(0xFFFB).is_some());
        assert!(HvacThermostat::CLUSTER.attribute(0xFFFC).is_some());
        assert!(HvacThermostat::CLUSTER.attribute(0xFFFD).is_some());
    }
}
