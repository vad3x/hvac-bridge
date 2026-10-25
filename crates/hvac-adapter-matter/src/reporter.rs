//! HVAC Bridge - Matter HVAC Outbound Reporter
//!
//! Bridges canonical HVAC state changes from `HvacController` to all Matter cluster handlers
//! (`HvacOnOff`, `HvacThermostat`, and `HvacFanControl` on Endpoint 1).

use hvac_core::domain::HvacState;
use hvac_core::ports::HvacReporter;

use crate::fan_control::HvacFanControl;
use crate::on_off::HvacOnOff;
use crate::thermostat::HvacThermostat;

// ============================================================================
// Public Structs & Implementation
// ============================================================================

/// Outbound HVAC state reporter that fans out canonical state updates to Matter cluster handlers.
pub struct MatterHvacReporter<'a> {
    on_off: Option<&'a HvacOnOff<'a>>,
    thermostat: &'a HvacThermostat<'a>,
    fan_control: Option<&'a HvacFanControl<'a>>,
}

impl<'a> MatterHvacReporter<'a> {
    /// Create a new reporter wrapping references to the on_off, thermostat, and fan control cluster handlers.
    pub const fn new(
        on_off: &'a HvacOnOff<'a>,
        thermostat: &'a HvacThermostat<'a>,
        fan_control: &'a HvacFanControl<'a>,
    ) -> Self {
        Self {
            on_off: Some(on_off),
            thermostat,
            fan_control: Some(fan_control),
        }
    }

    /// Create a new reporter wrapping only the thermostat cluster handler.
    pub const fn thermostat_only(thermostat: &'a HvacThermostat<'a>) -> Self {
        Self {
            on_off: None,
            thermostat,
            fan_control: None,
        }
    }
}

impl<'a> HvacReporter for MatterHvacReporter<'a> {
    fn report_state(&self, state: &HvacState) {
        if let Some(on_off) = self.on_off {
            on_off.update_state(state);
        }
        self.thermostat.update_state(state);
        if let Some(fan) = self.fan_control {
            fan.update_state(state);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fan_control::HvacFanControlConfig;
    use crate::on_off::HvacOnOffConfig;
    use crate::readiness::ClusterReadinessBarrier;
    use crate::thermostat::HvacThermostatConfig;
    use hvac_core::domain::{
        FanSpeed, HvacMode, TemperatureCentiC, VaneHorizontal, VaneVertical,
    };
    use hvac_core::events::SystemStateWatch;
    use hvac_core::ports::{CommandChannel, HvacControllerHandle};
    use rs_matter::dm::clusters::decl::fan_control::FanModeEnum;
    use rs_matter::dm::clusters::decl::thermostat::SystemModeEnum;

    static WATCH: SystemStateWatch = SystemStateWatch::new();
    static BARRIER: ClusterReadinessBarrier =
        ClusterReadinessBarrier::new(hvac_core::events::SystemStateSender::new(&WATCH));

    #[test]
    fn reporter_updates_all_three_clusters() {
        // Arrange
        let channel = CommandChannel::new();
        let handle = HvacControllerHandle::new(channel.sender());
        let on_off = HvacOnOff::new(HvacOnOffConfig {
            hvac: handle,
            readiness: &BARRIER,
        });
        let thermostat = HvacThermostat::new(HvacThermostatConfig {
            hvac: handle,
            readiness: &BARRIER,
        });
        let fan_control = HvacFanControl::new(HvacFanControlConfig {
            hvac: handle,
            readiness: &BARRIER,
        });
        let reporter = MatterHvacReporter::new(&on_off, &thermostat, &fan_control);

        let state = HvacState {
            connected: true,
            power: true,
            mode: HvacMode::Cool,
            target_temp: TemperatureCentiC::from_centi(2300),
            room_temp: Some(TemperatureCentiC::from_centi(2450)),
            fan_speed: FanSpeed::Speed3,
            vane_vertical: VaneVertical::Swing,
            vane_horizontal: VaneHorizontal::Auto,
            wide_vane_adjustment: true,
            operating: true,
            compressor_freq_hz: Some(42),
        };

        // Act
        reporter.report_state(&state);

        // Assert - on_off updated
        assert!(on_off.is_on());

        // Assert - thermostat updated
        assert_eq!(thermostat.system_mode(), SystemModeEnum::Cool);
        assert_eq!(thermostat.occupied_cooling_setpoint(), 2300);

        // Assert - fan control updated
        assert_eq!(fan_control.fan_mode(), FanModeEnum::High);
        assert_eq!(fan_control.percent_current(), 80);
    }
}
