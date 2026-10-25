import { Card, CardBody, CardTitle } from '@embedded-device-foundation/dashboard-core';
import { Droplets, Flame, Minus, Plus, Power, RefreshCw, Snowflake, Wind } from 'lucide-solid';
import type { Component } from 'solid-js';
import { For, Show } from 'solid-js';
import type {
  FanSpeed,
  HvacCommandParams,
  HvacMode,
  HvacTelemetry,
  VaneHorizontal,
  VaneVertical,
} from '../types';

export interface HvacControlsProps {
  hvac: HvacTelemetry | undefined;
  onControl: (params: HvacCommandParams) => void;
}

export const HvacControls: Component<HvacControlsProps> = (props) => {
  const modes: {
    id: HvacMode;
    label: string;
    icon: Component<{ class?: string }>;
    colorClass: string;
  }[] = [
    { id: 'heat', label: 'Heat', icon: Flame, colorClass: 'btn-warning' },
    { id: 'cool', label: 'Cool', icon: Snowflake, colorClass: 'btn-info' },
    { id: 'auto', label: 'Auto', icon: RefreshCw, colorClass: 'btn-primary' },
    { id: 'dry', label: 'Dry', icon: Droplets, colorClass: 'btn-secondary' },
    { id: 'fan_only', label: 'Fan', icon: Wind, colorClass: 'btn-accent' },
  ];

  const fans: { id: FanSpeed; label: string }[] = [
    { id: 'auto', label: 'Auto' },
    { id: 'quiet', label: 'Quiet' },
    { id: '1', label: '1' },
    { id: '2', label: '2' },
    { id: '3', label: '3' },
    { id: '4', label: '4' },
  ];

  const vanesV: { id: VaneVertical; label: string }[] = [
    { id: 'auto', label: 'Auto' },
    { id: '1', label: '1' },
    { id: '2', label: '2' },
    { id: '3', label: '3' },
    { id: '4', label: '4' },
    { id: '5', label: '5' },
    { id: 'swing', label: 'Swing' },
  ];

  const vanesH: { id: VaneHorizontal; label: string }[] = [
    { id: 'auto', label: 'Auto' },
    { id: 'left_max', label: 'Left Max' },
    { id: 'left', label: 'Left' },
    { id: 'center', label: 'Center' },
    { id: 'right', label: 'Right' },
    { id: 'right_max', label: 'Right Max' },
    { id: 'split', label: 'Split' },
    { id: 'swing', label: 'Swing' },
  ];

  const adjustTemp = (deltaCenti: number) => {
    if (!props.hvac) return;
    const current = Math.round(Number.parseFloat(props.hvac.target_temp_c) * 100);
    const next = current + deltaCenti;
    props.onControl({ temp: next });
  };

  return (
    <div class="space-y-4">
      {/* Top Grid: Power & Mode + Temperature Setpoint */}
      <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
        {/* Power & Mode */}
        <Card bordered>
          <CardBody>
            <div class="flex items-center justify-between pb-3 border-b border-base-content/10">
              <div class="flex items-center gap-2.5">
                <div
                  class={`p-2 rounded-lg transition-colors ${
                    props.hvac?.power
                      ? 'bg-success/20 text-success'
                      : 'bg-base-300 text-base-content/40'
                  }`}
                >
                  <Power class="w-4 h-4" />
                </div>
                <div>
                  <div class="font-semibold text-sm text-base-content">Unit Power</div>
                  <div class="text-xs text-base-content/60">
                    {props.hvac?.power ? 'Active (ON)' : 'Standby (OFF)'}
                  </div>
                </div>
              </div>
              <input
                type="checkbox"
                class="toggle toggle-success toggle-md"
                checked={Boolean(props.hvac?.power)}
                onChange={(e) => props.onControl({ power: e.currentTarget.checked ? 1 : 0 })}
                aria-label="Toggle HVAC Power"
              />
            </div>

            <CardTitle class="mt-4">Mode</CardTitle>
            <div class="flex flex-wrap gap-2">
              <For each={modes}>
                {(m) => {
                  const Icon = m.icon;
                  const active = () => props.hvac?.mode === m.id;
                  return (
                    <button
                      type="button"
                      class={`btn btn-sm font-medium ${
                        active() ? `${m.colorClass} shadow-sm` : 'btn-ghost bg-base-300'
                      }`}
                      onClick={() => props.onControl({ mode: m.id })}
                    >
                      <Icon class="w-3.5 h-3.5 mr-1" />
                      {m.label}
                    </button>
                  );
                }}
              </For>
            </div>
          </CardBody>
        </Card>

        {/* Temperature Setpoint Hero */}
        <Card bordered>
          <CardBody>
            <div class="flex items-center justify-around py-2">
              {/* Target Temp */}
              <div class="text-center">
                <CardTitle>Target Setpoint</CardTitle>
                <div class="text-4xl sm:text-5xl font-black text-primary my-1">
                  {props.hvac?.target_temp_c || '--'}
                  <span class="text-xl font-normal text-base-content/60 ml-1">°C</span>
                </div>
                <div class="flex justify-center gap-2 mt-2">
                  <button
                    type="button"
                    class="btn btn-circle btn-sm btn-outline"
                    onClick={() => adjustTemp(-50)}
                    aria-label="Decrease temperature by 0.5 degrees"
                  >
                    <Minus class="w-4 h-4" />
                  </button>
                  <button
                    type="button"
                    class="btn btn-circle btn-sm btn-outline"
                    onClick={() => adjustTemp(50)}
                    aria-label="Increase temperature by 0.5 degrees"
                  >
                    <Plus class="w-4 h-4" />
                  </button>
                </div>
              </div>

              {/* Divider */}
              <div class="divider divider-horizontal mx-0" />

              {/* Room Temp & Compressor */}
              <div class="text-center">
                <CardTitle>Room Temp</CardTitle>
                <div class="text-3xl sm:text-4xl font-extrabold text-base-content/80 my-1">
                  {props.hvac?.room_temp_c || '--'}
                  <span class="text-lg font-normal text-base-content/60 ml-1">°C</span>
                </div>
                <div class="text-xs text-base-content/60 mt-3 font-mono">
                  Compressor:{' '}
                  {props.hvac?.compressor_freq_hz != null
                    ? `${props.hvac.compressor_freq_hz} Hz`
                    : '0 Hz'}
                </div>
              </div>
            </div>
          </CardBody>
        </Card>
      </div>

      {/* Middle Grid: Fan Speed & Vertical Vane */}
      <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
        {/* Fan Speed */}
        <Card bordered>
          <CardBody>
            <CardTitle>Fan Speed</CardTitle>
            <div class="flex flex-wrap gap-2">
              <For each={fans}>
                {(f) => (
                  <button
                    type="button"
                    class={`btn btn-sm ${
                      props.hvac?.fan_speed === f.id ? 'btn-primary' : 'btn-ghost bg-base-300'
                    }`}
                    onClick={() => props.onControl({ fan: f.id })}
                  >
                    {f.label}
                  </button>
                )}
              </For>
            </div>
          </CardBody>
        </Card>

        {/* Vertical Vane */}
        <Card bordered>
          <CardBody>
            <CardTitle>Vertical Vane (Up / Down)</CardTitle>
            <div class="flex flex-wrap gap-2">
              <For each={vanesV}>
                {(v) => (
                  <button
                    type="button"
                    class={`btn btn-sm ${
                      props.hvac?.vane_vertical === v.id ? 'btn-primary' : 'btn-ghost bg-base-300'
                    }`}
                    onClick={() => props.onControl({ vane_v: v.id })}
                  >
                    {v.label}
                  </button>
                )}
              </For>
            </div>
          </CardBody>
        </Card>
      </div>

      {/* Full-width Horizontal Vane (conditionally shown if hardware supports wide vane adjustment) */}
      <Show when={props.hvac?.wide_vane_adjustment}>
        <Card bordered>
          <CardBody>
            <CardTitle>Horizontal Vane (Left / Right)</CardTitle>
            <div class="flex flex-wrap gap-2">
              <For each={vanesH}>
                {(h) => (
                  <button
                    type="button"
                    class={`btn btn-sm ${
                      props.hvac?.vane_horizontal === h.id ? 'btn-primary' : 'btn-ghost bg-base-300'
                    }`}
                    onClick={() => props.onControl({ vane_h: h.id })}
                  >
                    {h.label}
                  </button>
                )}
              </For>
            </div>
          </CardBody>
        </Card>
      </Show>
    </div>
  );
};
