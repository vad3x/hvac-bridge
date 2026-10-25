import type { HvacCommandParams, SystemDiagnostics } from './types';

const MOCK_STATUS: SystemDiagnostics = {
  device_type: 'hvac_bridge',
  state: 'Running',
  uptime_sec: 3642,
  wifi_ssid: 'Home_IoT_5G',
  wifi_rssi_dbm: -58,
  ip: '192.168.1.150',
  mac: '98:A3:16:A6:78:44',
  active_slot: 'ota_1',
  slot_valid: true,
  free_heap_bytes: 184320,
  total_heap_bytes: 327680,
  available_ecosystems: ['matter'],
  active_ecosystem: {
    type: 'matter',
    paired: true,
    properties: {
      thermostat_subscribed: true,
      on_off_subscribed: true,
      fan_subscribed: true,
    },
    fabrics: [
      {
        fab_idx: 1,
        vendor_id: 0x1384,
        fabric_id: '0x1B8246E1309A4D12',
        node_id: '0x0000000000000002',
        label: 'Living Room HomePod',
      },
    ],
    endpoints: [
      { id: 1, name: 'Thermostat', cluster: 'Thermostat', ready: true },
      { id: 1, name: 'Power', cluster: 'OnOff', ready: true },
      { id: 1, name: 'Fan & Vane', cluster: 'FanControl', ready: true },
    ],
  },
  hvac: {
    connected: true,
    power: true,
    mode: 'cool',
    target_temp_c: '22.0',
    room_temp_c: '23.5',
    fan_speed: 'auto',
    vane_vertical: 'auto',
    vane_horizontal: 'center',
    operating: false,
    compressor_freq_hz: 0,
  },
  version: '0.1.0',
  git_commit: 'a1b2c3d',
  build_date: '2026-10-17',
};

export async function fetchStatus(): Promise<SystemDiagnostics> {
  try {
    const res = await fetch('/api/status');
    if (!res.ok) {
      throw new Error(`Failed to fetch status: HTTP ${res.status}`);
    }
    return await res.json();
  } catch (err) {
    if (import.meta.env.DEV) {
      return MOCK_STATUS;
    }
    throw err;
  }
}

export async function sendHvacCommand(params: HvacCommandParams): Promise<void> {
  const searchParams = new URLSearchParams();
  for (const [key, val] of Object.entries(params)) {
    if (val !== undefined && val !== null) {
      if (typeof val === 'boolean') {
        searchParams.append(key, val ? '1' : '0');
      } else {
        searchParams.append(key, String(val));
      }
    }
  }

  try {
    const res = await fetch(`/api/action/hvac?${searchParams.toString()}`, {
      method: 'POST',
    });

    if (!res.ok) {
      throw new Error(`Failed to send HVAC command: HTTP ${res.status}`);
    }
  } catch (err) {
    if (import.meta.env.DEV) {
      // In dev mode simulate state update
      if (params.power !== undefined) {
        MOCK_STATUS.hvac.power = Boolean(params.power);
      }
      if (params.mode !== undefined) {
        MOCK_STATUS.hvac.mode = params.mode;
      }
      if (params.temp !== undefined) {
        MOCK_STATUS.hvac.target_temp_c = (params.temp / 100).toFixed(1);
      }
      if (params.fan !== undefined) {
        MOCK_STATUS.hvac.fan_speed = params.fan;
      }
      if (params.vane_v !== undefined) {
        MOCK_STATUS.hvac.vane_vertical = params.vane_v;
      }
      if (params.vane_h !== undefined) {
        MOCK_STATUS.hvac.vane_horizontal = params.vane_h;
      }
      return;
    }
    throw err;
  }
}

export async function fetchLogs(): Promise<string> {
  try {
    const res = await fetch('/api/logs');
    if (!res.ok) {
      throw new Error(`Failed to fetch logs: HTTP ${res.status}`);
    }
    return await res.text();
  } catch (err) {
    if (import.meta.env.DEV) {
      return '12:00:01.234 [INFO] HVAC Bridge bootloader running in slot ota_1\n12:00:02.100 [INFO] Wi-Fi connected to Home_IoT_5G (RSSI: -58 dBm)\n12:00:02.340 [INFO] Matter stack initialized: Fabric 0x1 paired\n12:00:03.010 [INFO] CN105 UART connected at 2400 baud 8E1\n12:00:03.500 [INFO] Polling telemetry: room=23.5C target=22.0C mode=cool';
    }
    throw err;
  }
}

export async function resetMatterKvs(): Promise<void> {
  const res = await fetch('/api/action/reset/matter', { method: 'POST' });
  if (!res.ok) {
    throw new Error(`Matter KVS reset failed: HTTP ${res.status}`);
  }
}

export async function reboot(): Promise<void> {
  const res = await fetch('/api/action/reboot', { method: 'POST' });
  if (!res.ok) {
    throw new Error(`Reboot failed: HTTP ${res.status}`);
  }
}

export async function factoryReset(): Promise<void> {
  const res = await fetch('/api/action/reset/factory', { method: 'POST' });
  if (!res.ok) {
    throw new Error(`Factory reset failed: HTTP ${res.status}`);
  }
}
