import { createMemo, createSignal } from 'solid-js';
import { createStore, reconcile } from 'solid-js/store';
import { fetchStatus, resetMatterKvs, sendHvacCommand } from '../api';
import type {
  FanSpeed,
  HvacCommandParams,
  HvacMode,
  HvacTelemetry,
  SystemDiagnostics,
  VaneHorizontal,
  VaneVertical,
} from '../types';

const INTENT_GRACE_PERIOD_MS = 4000; // 4s grace period to allow asynchronous CN105 UART round-trip

interface PendingIntent<T> {
  value: T;
  expiresAt: number;
}

interface IntentMap {
  power?: PendingIntent<boolean>;
  mode?: PendingIntent<HvacMode>;
  target_temp_c?: PendingIntent<string>;
  fan_speed?: PendingIntent<FanSpeed>;
  vane_vertical?: PendingIntent<VaneVertical>;
  vane_horizontal?: PendingIntent<VaneHorizontal>;
}

export function createHvacStore() {
  const [store, setStore] = createStore<{ status: SystemDiagnostics | null }>({
    status: null,
  });
  const [intents, setIntents] = createSignal<IntentMap>({});
  const [isResettingMatter, setIsResettingMatter] = createSignal(false);

  // Debounce timer for rapid temperature stepper clicks
  let tempDebounceTimer: ReturnType<typeof setTimeout> | null = null;
  let pendingTempParam: number | null = null;

  const clearIntent = (key: keyof IntentMap) => {
    setIntents((prev) => {
      if (!prev[key]) return prev;
      const next = { ...prev };
      delete next[key];
      return next;
    });
  };

  // Reconciled HVAC state merging physical telemetry with active optimistic intents
  const hvac = createMemo<HvacTelemetry | undefined>(() => {
    const raw = store.status?.hvac;
    if (!raw) return undefined;

    const currentIntents = intents();
    const now = Date.now();
    const reconciled: HvacTelemetry = { ...raw };

    // Reconcile Power
    if (currentIntents.power) {
      if (raw.power === currentIntents.power.value || now > currentIntents.power.expiresAt) {
        clearIntent('power');
      } else {
        reconciled.power = currentIntents.power.value;
      }
    }

    // Reconcile Mode
    if (currentIntents.mode) {
      if (raw.mode === currentIntents.mode.value || now > currentIntents.mode.expiresAt) {
        clearIntent('mode');
      } else {
        reconciled.mode = currentIntents.mode.value;
      }
    }

    // Reconcile Target Temp
    if (currentIntents.target_temp_c) {
      const rawTarget = Number.parseFloat(raw.target_temp_c);
      const intentTarget = Number.parseFloat(currentIntents.target_temp_c.value);
      if (
        Math.abs(rawTarget - intentTarget) < 0.05 ||
        now > currentIntents.target_temp_c.expiresAt
      ) {
        clearIntent('target_temp_c');
      } else {
        reconciled.target_temp_c = currentIntents.target_temp_c.value;
      }
    }

    // Reconcile Fan Speed
    if (currentIntents.fan_speed) {
      if (
        raw.fan_speed === currentIntents.fan_speed.value ||
        now > currentIntents.fan_speed.expiresAt
      ) {
        clearIntent('fan_speed');
      } else {
        reconciled.fan_speed = currentIntents.fan_speed.value;
      }
    }

    // Reconcile Vertical Vane
    if (currentIntents.vane_vertical) {
      if (
        raw.vane_vertical === currentIntents.vane_vertical.value ||
        now > currentIntents.vane_vertical.expiresAt
      ) {
        clearIntent('vane_vertical');
      } else {
        reconciled.vane_vertical = currentIntents.vane_vertical.value;
      }
    }

    // Reconcile Horizontal Vane
    if (currentIntents.vane_horizontal) {
      if (
        raw.vane_horizontal === currentIntents.vane_horizontal.value ||
        now > currentIntents.vane_horizontal.expiresAt
      ) {
        clearIntent('vane_horizontal');
      } else {
        reconciled.vane_horizontal = currentIntents.vane_horizontal.value;
      }
    }

    return reconciled;
  });

  const poll = async () => {
    if (typeof document !== 'undefined' && document.hidden) return;
    try {
      const data = await fetchStatus();
      if (!store.status) {
        setStore('status', data);
      } else {
        setStore('status', reconcile(data));
      }
    } catch (_e) {
      // Transient network error ignored
    }
  };

  const applyControl = async (params: HvacCommandParams) => {
    const expiresAt = Date.now() + INTENT_GRACE_PERIOD_MS;
    const newIntents: Partial<IntentMap> = {};

    if (params.power !== undefined) {
      newIntents.power = { value: Boolean(params.power), expiresAt };
    }
    if (params.mode !== undefined) {
      newIntents.mode = { value: params.mode, expiresAt };
    }
    if (params.temp !== undefined) {
      const tempC = (params.temp / 100).toFixed(1);
      newIntents.target_temp_c = { value: tempC, expiresAt };
    }
    if (params.fan !== undefined) {
      newIntents.fan_speed = { value: params.fan, expiresAt };
    }
    if (params.vane_v !== undefined) {
      newIntents.vane_vertical = { value: params.vane_v, expiresAt };
    }
    if (params.vane_h !== undefined) {
      newIntents.vane_horizontal = { value: params.vane_h, expiresAt };
    }

    // Optimistically update local UI immediately
    setIntents((prev) => ({ ...prev, ...newIntents }));

    // Debounce rapid temperature stepper taps
    if (params.temp !== undefined && Object.keys(params).length === 1) {
      pendingTempParam = params.temp;
      if (tempDebounceTimer) clearTimeout(tempDebounceTimer);
      return new Promise<void>((resolve, reject) => {
        tempDebounceTimer = setTimeout(async () => {
          try {
            if (pendingTempParam !== null) {
              await sendHvacCommand({ temp: pendingTempParam });
              pendingTempParam = null;
            }
            resolve();
          } catch (err) {
            clearIntent('target_temp_c');
            reject(err);
          }
        }, 250);
      });
    }

    // For discrete actions (power toggle, mode, fan, vane)
    try {
      await sendHvacCommand(params);
    } catch (err) {
      // Revert intents on error
      setIntents((prev) => {
        const next = { ...prev };
        for (const k of Object.keys(newIntents) as (keyof IntentMap)[]) {
          delete next[k];
        }
        return next;
      });
      throw err;
    }
  };

  const resetMatter = async () => {
    setIsResettingMatter(true);
    try {
      await resetMatterKvs();
    } catch (e) {
      setIsResettingMatter(false);
      throw e;
    }
  };

  return {
    status: () => store.status,
    hvac,
    isResettingMatter,
    poll,
    applyControl,
    resetMatter,
  };
}
