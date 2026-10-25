import { expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const dashboard = readFileSync(new URL('../../assets/dashboard.html', import.meta.url), 'utf8');

test('embedded dashboard keeps HVAC controls, branding, callbacks, and diagnostics', () => {
  for (const text of [
    'HVAC Bridge',
    'CN105',
    'hvac_theme',
    '/api/action/hvac?',
    '/api/action/reset/matter',
    '/api/logs',
    'Fan Control',
    'Thermostat',
    'On / Off',
    'Home Assistant',
    'Native direct integration for Home Assistant',
    'Live System Logs',
    'Network & Hardware',
  ]) {
    expect(dashboard.includes(text), `missing dashboard content: ${text}`).toBe(true);
  }
});

test('embedded dashboard includes its scripts and themed shared component styles inline', () => {
  expect(dashboard).not.toMatch(/<script[^>]+src=/);
  expect(dashboard).not.toMatch(/<link[^>]+(?:stylesheet|modulepreload)/);
  expect(dashboard).toContain('data-theme=night');
  expect(dashboard).toContain('data-theme=light');
  expect(dashboard).toContain('max-height:500px');
});
