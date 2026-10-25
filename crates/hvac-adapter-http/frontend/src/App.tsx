import {
  Badge,
  DashboardShell,
  type EcosystemStrategy,
  MatterEcosystemPanel,
  Toast,
} from '@embedded-device-foundation/dashboard-core';
import { Activity, Layers } from 'lucide-solid';
import { createSignal, onCleanup, onMount } from 'solid-js';
import { factoryReset, fetchLogs, reboot } from './api';
import { HvacControls } from './components/HvacControls';
import { createHvacStore } from './store/hvacStore';
import type { HvacCommandParams } from './types';

export function App() {
  const store = createHvacStore();
  const [toastMsg, setToastMsg] = createSignal<string | null>(null);
  const [toastType, setToastType] = createSignal<'info' | 'success' | 'error'>('info');

  const hvacStatus = () => {
    const status = store.status();
    if (!status) {
      return { variant: 'neutral' as const, dotClass: 'bg-base-content/30 animate-pulse' };
    }
    const hvac = status.hvac;
    if (!hvac?.connected) {
      return { variant: 'error' as const, dotClass: 'bg-error' };
    }
    return { variant: 'success' as const, dotClass: 'bg-success' };
  };

  let toastTimer: ReturnType<typeof setTimeout> | null = null;

  const showToast = (msg: string, type: 'info' | 'success' | 'error' = 'info') => {
    setToastMsg(msg);
    setToastType(type);
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      setToastMsg(null);
    }, 2500);
  };

  const handleControl = async (params: HvacCommandParams) => {
    try {
      await store.applyControl(params);
      showToast('Command sent', 'success');
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : 'Failed to update';
      showToast(msg, 'error');
    }
  };

  const handleResetMatter = async () => {
    try {
      await store.resetMatter();
      showToast('Matter KVS reset initiated. Rebooting...', 'info');
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : 'Failed to reset Matter KVS';
      showToast(msg, 'error');
    }
  };

  const handleReboot = async () => {
    try {
      await reboot();
      showToast('Device reboot initiated...', 'info');
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : 'Failed to reboot device';
      showToast(msg, 'error');
    }
  };

  const handleFactoryReset = async () => {
    try {
      await factoryReset();
      showToast('Factory reset initiated. Device will reboot into setup mode...', 'info');
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : 'Failed to factory reset';
      showToast(msg, 'error');
    }
  };

  const ecosystemStrategies: EcosystemStrategy[] = [
    {
      id: 'matter',
      label: 'Matter over Wi-Fi',
      icon: Layers,
      Component: (props) => (
        <MatterEcosystemPanel
          ecosystem={props.ecosystem}
          subscriptionRows={[
            { label: 'Thermostat', propertyKey: 'thermostat_subscribed' },
            { label: 'Power', propertyKey: 'on_off_subscribed' },
            { label: 'Fan & Vane', propertyKey: 'fan_subscribed' },
          ]}
          endpointIcon={(endpoint) => {
            switch (endpoint.cluster) {
              case 'Thermostat':
                return '🌡️ ';
              case 'OnOff':
                return '⚡ ';
              case 'FanControl':
                return '💨 ';
              default:
                return '❄️ ';
            }
          }}
          onResetMatter={handleResetMatter}
          isResettingMatter={store.isResettingMatter()}
        />
      ),
    },
  ];

  onMount(() => {
    store.poll();
    const interval = setInterval(store.poll, 2500);
    onCleanup(() => clearInterval(interval));
  });

  return (
    <DashboardShell
      title="HVAC Bridge"
      brandIcon={<Activity class="w-5 h-5" />}
      badges={
        <Badge variant={hvacStatus().variant}>
          <span class={`w-2 h-2 rounded-full ${hvacStatus().dotClass}`} />
          <span class="lowercase">hvac:</span>
          <span class="normal-case">CN105</span>
        </Badge>
      }
      controlTab={<HvacControls hvac={store.hvac()} onControl={handleControl} />}
      status={store.status()}
      themeStorageKey="hvac_theme"
      ecosystemStrategies={ecosystemStrategies}
      tabOrder={['controls', 'ecosystem', 'diagnostics', 'logs']}
      loadLogs={fetchLogs}
      onReboot={handleReboot}
      onFactoryReset={handleFactoryReset}
      toast={<Toast message={toastMsg()} type={toastType()} />}
    />
  );
}

export default App;
