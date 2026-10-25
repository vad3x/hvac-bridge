import type { SystemDiagnosticsBase } from '@embedded-device-foundation/dashboard-core';

export type {
  ActiveEcosystem,
  EndpointInfo,
  FabricInfo,
} from '@embedded-device-foundation/dashboard-core';

export type HvacMode = 'heat' | 'cool' | 'auto' | 'dry' | 'fan_only';

export type FanSpeed = 'auto' | 'quiet' | '1' | '2' | '3' | '4';

export type VaneVertical = 'auto' | '1' | '2' | '3' | '4' | '5' | 'swing';

export type VaneHorizontal =
  | 'auto'
  | 'left_max'
  | 'left'
  | 'center'
  | 'right'
  | 'right_max'
  | 'split'
  | 'swing';

export interface HvacTelemetry {
  connected: boolean;
  power: boolean;
  mode: HvacMode;
  target_temp_c: string; // e.g. "22.0"
  room_temp_c: string | null; // e.g. "21.5"
  fan_speed: FanSpeed;
  vane_vertical: VaneVertical;
  vane_horizontal: VaneHorizontal;
  wide_vane_adjustment?: boolean;
  operating: boolean;
  compressor_freq_hz: number | null;
}

export interface SystemDiagnostics extends SystemDiagnosticsBase {
  hvac: HvacTelemetry;
}

export interface HvacCommandParams {
  power?: 0 | 1 | boolean;
  mode?: HvacMode;
  temp?: number; // Centidegrees e.g. 2250 or float
  fan?: FanSpeed;
  vane_v?: VaneVertical;
  vane_h?: VaneHorizontal;
}
