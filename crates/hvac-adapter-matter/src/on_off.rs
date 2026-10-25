//! HVAC Bridge - Matter On/Off Cluster Handler
//!
//! Exposes the Matter On/Off cluster (`0x0006`, rev 6) on Endpoint 1 (`DEV_TYPE_ROOM_AIR_CONDITIONER` `0x0072`).
//! Maps HVAC power state to the Matter `OnOff` attribute and dispatches `Off`, `On`, and `Toggle`
//! commands to `HvacControllerHandle`.

use core::sync::atomic::Ordering;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::signal::Signal;
use log::info;
use portable_atomic::AtomicBool;

use rs_matter::dm::clusters::decl::on_off;
use rs_matter::dm::{
    AsyncHandler, Cluster, Dataver, DeviceType, HandlerContext, InvokeContext, InvokeReply,
    MatchContext, ReadContext, ReadReply, Reply, WriteContext,
};
use rs_matter::error::{Error, ErrorCode};
use rs_matter::with;

use hvac_core::domain::HvacState;
use hvac_core::ports::HvacControllerHandle;

use crate::config::{DEV_TYPE_ROOM_AIR_CONDITIONER, ON_OFF_ENDPOINT_ID};
use crate::readiness::ClusterReadinessBarrier;
use crate::MatterCluster;

// ============================================================================
// Public Structs & Implementation
// ============================================================================

/// Configuration for [`HvacOnOff`].
pub struct HvacOnOffConfig<'a> {
    /// Inbound driving port handle to dispatch HVAC mutation commands.
    pub hvac: HvacControllerHandle<'a>,
    /// Cluster readiness barrier to track initial subscription priming.
    pub readiness: &'static ClusterReadinessBarrier,
}

/// Matter On/Off cluster handler on Endpoint 1.
pub struct HvacOnOff<'a> {
    config: HvacOnOffConfig<'a>,
    dataver: Dataver,
    power: AtomicBool,
    state_changed: Signal<NoopRawMutex, ()>,
}

impl<'a> HvacOnOff<'a> {
    /// Create a new HVAC On/Off handler with power initially off.
    pub fn new(config: HvacOnOffConfig<'a>) -> Self {
        Self {
            config,
            dataver: Dataver::new(1),
            power: AtomicBool::new(false),
            state_changed: Signal::new(),
        }
    }

    /// Update internal state from an out-of-band canonical HVAC state snapshot.
    pub fn update_state(&self, state: &HvacState) {
        let prev_power = self.power.swap(state.power, Ordering::SeqCst);
        if prev_power != state.power {
            info!("HvacOnOff state changed -> bumping dataver and signaling subscriber notify");
            self.dataver.changed();
            self.state_changed.signal(());
        }
    }

    /// Read current power state.
    pub fn is_on(&self) -> bool {
        self.power.load(Ordering::Relaxed)
    }

    /// Execute the `Off` command (`0x00`).
    pub fn handle_off(&self) {
        self.power.store(false, Ordering::SeqCst);
        self.dataver.changed();
        self.config.hvac.set_power(false);
    }

    /// Execute the `On` command (`0x01`).
    pub fn handle_on(&self) {
        self.power.store(true, Ordering::SeqCst);
        self.dataver.changed();
        self.config.hvac.set_power(true);
    }

    /// Execute the `Toggle` command (`0x02`).
    pub fn handle_toggle(&self) -> bool {
        let new_power = !self.power.load(Ordering::Relaxed);
        self.power.store(new_power, Ordering::SeqCst);
        self.dataver.changed();
        self.config.hvac.set_power(new_power);
        new_power
    }

    /// Cluster definition matching the Room Air Conditioner specification (`0x0006`, rev 6).
    pub const CLUSTER: Cluster<'static> = on_off::FULL_CLUSTER
        .with_revision(6)
        .with_features(on_off::Feature::DEAD_FRONT_BEHAVIOR.bits())
        .with_attrs(with!(system; on_off::AttributeId::OnOff))
        .with_cmds(with!(
            on_off::CommandId::Off | on_off::CommandId::On | on_off::CommandId::Toggle
        ));
}

impl<'a> MatterCluster for HvacOnOff<'a> {
    const ENDPOINT_ID: u16 = ON_OFF_ENDPOINT_ID;
    const CLUSTER_DEF: Cluster<'static> = Self::CLUSTER;
    const DEVICE_TYPE: DeviceType = DEV_TYPE_ROOM_AIR_CONDITIONER;
}

impl<'a> AsyncHandler for HvacOnOff<'a> {
    async fn read(&self, ctx: impl ReadContext, reply: impl ReadReply) -> Result<(), Error> {
        self.config.readiness.mark_on_off_ready();

        if let Some(writer) = reply.with_dataver(self.dataver.get())? {
            if ctx.attr().is_system() {
                Self::CLUSTER.read(ctx.attr(), writer)
            } else {
                match ctx.attr().attr_id as u16 {
                    x if x == on_off::AttributeId::OnOff as u16 => {
                        Reply::set(writer, self.is_on())
                    }
                    _ => Err(ErrorCode::AttributeNotFound.into()),
                }
            }
        } else {
            Ok(())
        }
    }

    async fn write(&self, _ctx: impl WriteContext) -> Result<(), Error> {
        Err(ErrorCode::InvalidAction.into())
    }

    async fn invoke(&self, ctx: impl InvokeContext, _reply: impl InvokeReply) -> Result<(), Error> {
        match ctx.cmd().cmd_id as u16 {
            x if x == on_off::CommandId::Off as u16 => {
                self.handle_off();
                ctx.notify_attr_changed(
                    ctx.cmd().endpoint_id,
                    ctx.cmd().cluster_id,
                    on_off::AttributeId::OnOff as _,
                );
                Ok(())
            }
            x if x == on_off::CommandId::On as u16 => {
                self.handle_on();
                ctx.notify_attr_changed(
                    ctx.cmd().endpoint_id,
                    ctx.cmd().cluster_id,
                    on_off::AttributeId::OnOff as _,
                );
                Ok(())
            }
            x if x == on_off::CommandId::Toggle as u16 => {
                self.handle_toggle();
                ctx.notify_attr_changed(
                    ctx.cmd().endpoint_id,
                    ctx.cmd().cluster_id,
                    on_off::AttributeId::OnOff as _,
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
                info!("on_off run() woke -> notifying cluster attributes changed");
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
    use hvac_core::domain::{
        FanSpeed, HvacMode, TemperatureCentiC, VaneHorizontal, VaneVertical,
    };
    use hvac_core::events::SystemStateWatch;
    use hvac_core::ports::CommandChannel;

    static WATCH: SystemStateWatch = SystemStateWatch::new();
    static BARRIER: ClusterReadinessBarrier =
        ClusterReadinessBarrier::new(hvac_core::events::SystemStateSender::new(&WATCH));

    fn create_test_on_off<'a>(channel: &'a CommandChannel) -> HvacOnOff<'a> {
        let handle = HvacControllerHandle::new(channel.sender());
        HvacOnOff::new(HvacOnOffConfig {
            hvac: handle,
            readiness: &BARRIER,
        })
    }

    #[test]
    fn initial_state_is_off() {
        // Arrange
        let channel = CommandChannel::new();
        let on_off = create_test_on_off(&channel);

        // Act & Assert
        assert!(!on_off.is_on());
    }

    #[test]
    fn commands_update_state_and_dispatch_power_patches() {
        // Arrange
        let channel = CommandChannel::new();
        let on_off = create_test_on_off(&channel);

        // Act - On
        on_off.handle_on();

        // Assert
        assert!(on_off.is_on());
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.power, Some(true));

        // Act - Off
        on_off.handle_off();

        // Assert
        assert!(!on_off.is_on());
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.power, Some(false));

        // Act - Toggle
        let toggled = on_off.handle_toggle();

        // Assert
        assert!(toggled);
        assert!(on_off.is_on());
        let patch = channel.receiver().try_receive().unwrap();
        assert_eq!(patch.power, Some(true));
    }

    #[test]
    fn update_state_syncs_power() {
        // Arrange
        let channel = CommandChannel::new();
        let on_off = create_test_on_off(&channel);
        let state = HvacState {
            connected: true,
            power: true,
            mode: HvacMode::Cool,
            target_temp: TemperatureCentiC::DEFAULT_SETPOINT,
            room_temp: Some(TemperatureCentiC::DEFAULT_SETPOINT),
            fan_speed: FanSpeed::Auto,
            vane_vertical: VaneVertical::Auto,
            vane_horizontal: VaneHorizontal::Auto,
            wide_vane_adjustment: true,
            operating: true,
            compressor_freq_hz: Some(30),
        };

        // Act
        on_off.update_state(&state);

        // Assert
        assert!(on_off.is_on());
        assert!(HvacOnOff::CLUSTER.attribute(0xFFF8).is_some());
        assert!(HvacOnOff::CLUSTER.attribute(0xFFF9).is_some());
        assert!(HvacOnOff::CLUSTER.attribute(0xFFFB).is_some());
        assert!(HvacOnOff::CLUSTER.attribute(0xFFFC).is_some());
        assert!(HvacOnOff::CLUSTER.attribute(0xFFFD).is_some());
    }
}
