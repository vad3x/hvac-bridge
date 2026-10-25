//! Vendor-agnostic HVAC domain types, state snapshot, and mutation patch.

// ============================================================================
// Public Domain Types
// ============================================================================

/// Temperature in hundredths of a degree Celsius (0.01 °C, e.g. 2150 = 21.5 °C).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TemperatureCentiC(pub i16);

impl TemperatureCentiC {
    /// Minimum supported HVAC target setpoint (16.0 °C).
    pub const MIN_SETPOINT: Self = Self(1600);
    /// Maximum supported HVAC target setpoint (31.0 °C).
    pub const MAX_SETPOINT: Self = Self(3100);
    /// Default target setpoint on initialization (22.0 °C).
    pub const DEFAULT_SETPOINT: Self = Self(2200);

    /// Construct from hundredths of a degree Celsius (0.01 °C).
    pub const fn from_centi(val: i16) -> Self {
        Self(val)
    }

    /// Construct from half-degree Celsius units (e.g. 43 -> 21.5 °C = 2150).
    pub const fn from_half_degrees(half_deg: i16) -> Self {
        Self(half_deg * 50)
    }

    /// Convert to nearest half-degree Celsius integer (e.g. 2150 -> 43).
    pub const fn to_half_degrees_rounded(self) -> i16 {
        if self.0 >= 0 {
            (self.0 + 25) / 50
        } else {
            (self.0 - 25) / 50
        }
    }

    /// Clamp temperature within the valid HVAC setpoint range (`16.0 °C..=31.0 °C`).
    pub fn clamp_setpoint(self) -> Self {
        Self(self.0.clamp(Self::MIN_SETPOINT.0, Self::MAX_SETPOINT.0))
    }

    /// Return whole degrees and tenths (e.g. `2250` -> `(22, 5)`).
    pub const fn whole_and_tenths(self) -> (i16, u8) {
        let tenths_total = if self.0 >= 0 {
            (self.0 + 5) / 10
        } else {
            (self.0 - 5) / 10
        };
        let whole = tenths_total / 10;
        let frac = (tenths_total.abs() % 10) as u8;
        (whole, frac)
    }
}

/// Operating mode of the HVAC unit (independent of power state).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HvacMode {
    Heat,
    Cool,
    Auto,
    Dry,
    FanOnly,
}

impl HvacMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Heat => "heat",
            Self::Cool => "cool",
            Self::Auto => "auto",
            Self::Dry => "dry",
            Self::FanOnly => "fan_only",
        }
    }
}

/// Fan speed setting of the HVAC unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FanSpeed {
    Auto,
    Quiet,
    Speed1,
    Speed2,
    Speed3,
    Speed4,
}

impl FanSpeed {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Quiet => "quiet",
            Self::Speed1 => "1",
            Self::Speed2 => "2",
            Self::Speed3 => "3",
            Self::Speed4 => "4",
        }
    }
}

/// Vertical (up/down) vane position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaneVertical {
    Auto,
    Pos1,
    Pos2,
    Pos3,
    Pos4,
    Pos5,
    Swing,
}

impl VaneVertical {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Pos1 => "1",
            Self::Pos2 => "2",
            Self::Pos3 => "3",
            Self::Pos4 => "4",
            Self::Pos5 => "5",
            Self::Swing => "swing",
        }
    }
}

/// Horizontal (left/right) vane position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaneHorizontal {
    Auto,
    LeftMax,
    Left,
    Center,
    Right,
    RightMax,
    Split,
    Swing,
}

impl VaneHorizontal {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::LeftMax => "left_max",
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
            Self::RightMax => "right_max",
            Self::Split => "split",
            Self::Swing => "swing",
        }
    }
}

/// Canonical vendor-agnostic state snapshot of the HVAC unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HvacState {
    pub connected: bool,
    pub power: bool,
    pub mode: HvacMode,
    pub target_temp: TemperatureCentiC,
    pub room_temp: Option<TemperatureCentiC>,
    pub fan_speed: FanSpeed,
    pub vane_vertical: VaneVertical,
    pub vane_horizontal: VaneHorizontal,
    pub wide_vane_adjustment: bool,
    pub operating: bool,
    pub compressor_freq_hz: Option<u8>,
}

impl Default for HvacState {
    fn default() -> Self {
        Self {
            connected: false,
            power: false,
            mode: HvacMode::Auto,
            target_temp: TemperatureCentiC::DEFAULT_SETPOINT,
            room_temp: None,
            fan_speed: FanSpeed::Auto,
            vane_vertical: VaneVertical::Auto,
            vane_horizontal: VaneHorizontal::Auto,
            wide_vane_adjustment: false,
            operating: false,
            compressor_freq_hz: None,
        }
    }
}

impl HvacState {
    /// Return a new `HvacState` with the given `HvacPatch` applied.
    pub fn with_patch(mut self, patch: &HvacPatch) -> Self {
        if let Some(power) = patch.power {
            self.power = power;
        }
        if let Some(mode) = patch.mode {
            self.mode = mode;
        }
        if let Some(target_temp) = patch.target_temp {
            self.target_temp = target_temp.clamp_setpoint();
        }
        if let Some(fan_speed) = patch.fan_speed {
            self.fan_speed = fan_speed;
        }
        if let Some(vane_vertical) = patch.vane_vertical {
            self.vane_vertical = vane_vertical;
        }
        if let Some(vane_horizontal) = patch.vane_horizontal {
            self.vane_horizontal = vane_horizontal;
        }
        self
    }
}

/// Coalesced settings mutation request for the physical HVAC unit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HvacPatch {
    pub power: Option<bool>,
    pub mode: Option<HvacMode>,
    pub target_temp: Option<TemperatureCentiC>,
    pub fan_speed: Option<FanSpeed>,
    pub vane_vertical: Option<VaneVertical>,
    pub vane_horizontal: Option<VaneHorizontal>,
}

impl HvacPatch {
    /// Returns true if no fields are set in this patch.
    pub const fn is_empty(&self) -> bool {
        self.power.is_none()
            && self.mode.is_none()
            && self.target_temp.is_none()
            && self.fan_speed.is_none()
            && self.vane_vertical.is_none()
            && self.vane_horizontal.is_none()
    }

    /// Merge `newer` on top of `self`, overwriting any fields present in `newer`.
    pub fn merge(&mut self, newer: HvacPatch) {
        if newer.power.is_some() {
            self.power = newer.power;
        }
        if newer.mode.is_some() {
            self.mode = newer.mode;
        }
        if newer.target_temp.is_some() {
            self.target_temp = newer.target_temp;
        }
        if newer.fan_speed.is_some() {
            self.fan_speed = newer.fan_speed;
        }
        if newer.vane_vertical.is_some() {
            self.vane_vertical = newer.vane_vertical;
        }
        if newer.vane_horizontal.is_some() {
            self.vane_horizontal = newer.vane_horizontal;
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperature_half_degree_conversions_round_trip() {
        // Arrange
        let t = TemperatureCentiC::from_half_degrees(45); // 22.5 °C = 2250

        // Act
        let half_deg = t.to_half_degrees_rounded();
        let (whole, tenths) = t.whole_and_tenths();

        // Assert
        assert_eq!(t.0, 2250);
        assert_eq!(half_deg, 45);
        assert_eq!((whole, tenths), (22, 5));
    }

    #[test]
    fn temperature_negative_conversion_and_rounding() {
        // Arrange
        let neg = TemperatureCentiC::from_centi(-1050); // -10.5 °C

        // Act
        let half_deg = neg.to_half_degrees_rounded();
        let (whole, tenths) = neg.whole_and_tenths();

        // Assert
        assert_eq!(half_deg, -21);
        assert_eq!((whole, tenths), (-10, 5));
    }

    #[test]
    fn temperature_clamp_setpoint_enforces_bounds() {
        // Arrange
        let too_low = TemperatureCentiC::from_centi(1200);
        let too_high = TemperatureCentiC::from_centi(3500);
        let valid = TemperatureCentiC::from_centi(2150);

        // Act
        let clamped_low = too_low.clamp_setpoint();
        let clamped_high = too_high.clamp_setpoint();
        let clamped_valid = valid.clamp_setpoint();

        // Assert
        assert_eq!(clamped_low, TemperatureCentiC::MIN_SETPOINT);
        assert_eq!(clamped_high, TemperatureCentiC::MAX_SETPOINT);
        assert_eq!(clamped_valid, valid);
    }

    #[test]
    fn hvac_patch_is_empty_checks_all_fields() {
        // Arrange
        let empty = HvacPatch::default();
        let with_power = HvacPatch {
            power: Some(true),
            ..Default::default()
        };

        // Act
        let empty_result = empty.is_empty();
        let non_empty_result = with_power.is_empty();

        // Assert
        assert!(empty_result);
        assert!(!non_empty_result);
    }

    #[test]
    fn hvac_patch_merges_newer_fields_and_applies_to_state() {
        // Arrange
        let mut patch = HvacPatch {
            power: Some(true),
            mode: Some(HvacMode::Cool),
            ..Default::default()
        };
        let second = HvacPatch {
            mode: Some(HvacMode::Heat),
            target_temp: Some(TemperatureCentiC::from_centi(2350)),
            fan_speed: Some(FanSpeed::Quiet),
            vane_vertical: Some(VaneVertical::Pos3),
            vane_horizontal: Some(VaneHorizontal::Swing),
            ..Default::default()
        };

        // Act
        patch.merge(second);
        let state = HvacState::default().with_patch(&patch);

        // Assert
        assert!(!patch.is_empty());
        assert!(state.power);
        assert_eq!(state.mode, HvacMode::Heat);
        assert_eq!(state.target_temp, TemperatureCentiC(2350));
        assert_eq!(state.fan_speed, FanSpeed::Quiet);
        assert_eq!(state.vane_vertical, VaneVertical::Pos3);
        assert_eq!(state.vane_horizontal, VaneHorizontal::Swing);
    }

    #[test]
    fn enum_as_str_representations_match_spec() {
        // Arrange & Act & Assert
        assert_eq!(HvacMode::Heat.as_str(), "heat");
        assert_eq!(HvacMode::Cool.as_str(), "cool");
        assert_eq!(HvacMode::Auto.as_str(), "auto");
        assert_eq!(HvacMode::Dry.as_str(), "dry");
        assert_eq!(HvacMode::FanOnly.as_str(), "fan_only");

        assert_eq!(FanSpeed::Auto.as_str(), "auto");
        assert_eq!(FanSpeed::Quiet.as_str(), "quiet");
        assert_eq!(FanSpeed::Speed1.as_str(), "1");
        assert_eq!(FanSpeed::Speed2.as_str(), "2");
        assert_eq!(FanSpeed::Speed3.as_str(), "3");
        assert_eq!(FanSpeed::Speed4.as_str(), "4");

        assert_eq!(VaneVertical::Auto.as_str(), "auto");
        assert_eq!(VaneVertical::Pos1.as_str(), "1");
        assert_eq!(VaneVertical::Pos2.as_str(), "2");
        assert_eq!(VaneVertical::Pos3.as_str(), "3");
        assert_eq!(VaneVertical::Pos4.as_str(), "4");
        assert_eq!(VaneVertical::Pos5.as_str(), "5");
        assert_eq!(VaneVertical::Swing.as_str(), "swing");

        assert_eq!(VaneHorizontal::Auto.as_str(), "auto");
        assert_eq!(VaneHorizontal::LeftMax.as_str(), "left_max");
        assert_eq!(VaneHorizontal::Left.as_str(), "left");
        assert_eq!(VaneHorizontal::Center.as_str(), "center");
        assert_eq!(VaneHorizontal::Right.as_str(), "right");
        assert_eq!(VaneHorizontal::RightMax.as_str(), "right_max");
        assert_eq!(VaneHorizontal::Split.as_str(), "split");
        assert_eq!(VaneHorizontal::Swing.as_str(), "swing");
    }
}
