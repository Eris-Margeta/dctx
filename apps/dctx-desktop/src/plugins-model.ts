// Pure presentation model for the desktop plugin panel (PLUGINS.md §7.3 PL-APP).
// Mirrors the public PluginRow transport from dctx-desktop-api/plugins_view.rs.
// Depends on no DOM or Tauri APIs; consumed by plugins.ts and its Bun tests.

export interface ConsentRequest {
  id: string;
  version: string;
  digest: string;
  capabilities: Record<'required' | 'optional' | 'outputs' | 'network', Record<string, unknown>[]>;
  capability_lines: string[];
  scope: { kind: 'user' };
}

export interface PluginRow {
  id: string;
  name: string;
  version: string | null;
  source: 'bundled' | 'installed';
  state: 'enabled' | 'disabled' | 'needs-consent' | 'failed';
  enabled: boolean;
  needs_consent: boolean;
  capabilities: string[];
  last_failure: string | null;
  consent: ConsentRequest | null;
}

export interface PluginViewModel {
  id: string;
  name: string;
  versionLabel: string;
  sourceLabel: string;
  stateLabel: string;
  badgeClass: string;
  capabilityLines: string[];
  lastFailure: string | null;
  toggleLabel: 'Enable' | 'Disable';
  canEnable: boolean;
  canDisable: boolean;
  canGrantConsent: boolean;
  canCheckUpdate: boolean;
}

const stateLabels: Record<PluginRow['state'], string> = {
  enabled: 'Enabled',
  disabled: 'Disabled',
  'needs-consent': 'Needs consent',
  failed: 'Failed',
};

function compareText(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

export function pluginsToViewModels(rows: readonly PluginRow[]): PluginViewModel[] {
  return [...rows]
    .sort((left, right) =>
      Number(right.source === 'bundled') - Number(left.source === 'bundled')
      || compareText(left.name.toLowerCase(), right.name.toLowerCase())
      || compareText(left.id, right.id))
    .map(row => {
      if (!Object.prototype.hasOwnProperty.call(stateLabels, row.state)
          || (row.source !== 'bundled' && row.source !== 'installed')) {
        throw new Error(`Unsupported plugin state or source: ${row.id}`);
      }
      return {
        id: row.id,
        name: row.name,
        versionLabel: row.version === null ? 'No active version' : `Version ${row.version}`,
        sourceLabel: row.source === 'bundled' ? 'Bundled' : 'Installed',
        stateLabel: stateLabels[row.state],
        badgeClass: `plugin-badge plugin-state-${row.state}`,
        capabilityLines: [...row.capabilities],
        lastFailure: row.last_failure,
        toggleLabel: row.enabled ? 'Disable' : 'Enable',
        canEnable: !row.enabled,
        canDisable: row.enabled,
        canGrantConsent: row.state === 'needs-consent' && row.needs_consent,
        canCheckUpdate: true,
      };
    });
}
