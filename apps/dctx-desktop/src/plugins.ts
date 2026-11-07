// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
// DOM adapter for the desktop plugin settings (PLUGINS.md §7.3 PL-APP).
// Uses the pure plugins-model and the existing Tauri plugin management commands.
// Initialized by settings.ts; never fetches from a network or executes plugin code.
// Consent echoes the reviewed API payload only after an explicit confirmation.

import { invoke } from '@tauri-apps/api/core';
import { pluginsToViewModels } from './plugins-model';
import type { ConsentRequest, PluginRow, PluginViewModel } from './plugins-model';

interface PluginUpdate {
  id: string;
  version: string | null;
  containing_release: boolean;
}

function element<K extends keyof HTMLElementTagNameMap>(
  tag: K, text = '', className = '',
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.textContent = text;
  node.className = className;
  return node;
}

function button(label: string, action: () => void): HTMLButtonElement {
  const node = element('button', label, 'secondary');
  node.type = 'button';
  node.addEventListener('click', action);
  return node;
}

function capabilityList(lines: readonly string[]): HTMLElement {
  const list = element('ul', '', 'plugin-capabilities');
  for (const line of lines) list.append(element('li', line));
  if (lines.length === 0) list.append(element('li', 'No capabilities requested.'));
  return list;
}

function reviewConsent(
  plugin: PluginViewModel, payload: ConsentRequest | null, trigger: HTMLButtonElement,
  approve: (approval: ConsentRequest) => Promise<boolean>,
): void {
  const dialog = element('dialog', '', 'plugin-consent');
  const title = element('h3', `Grant consent to ${plugin.name}?`);
  title.id = 'plugin-consent-title';
  dialog.setAttribute('aria-labelledby', title.id);
  const approval = payload === null ? null : structuredClone(payload);
  let pending = false;
  const cancel = button('Cancel', () => dialog.close());
  const reason = element('p');
  reason.setAttribute('role', 'status');
  const confirm = button('Confirm grant', () => { void submit(); });
  confirm.disabled = approval === null;
  async function submit(): Promise<void> {
    if (pending || approval === null) return;
    pending = true;
    cancel.disabled = confirm.disabled = true;
    reason.textContent = 'Recording consent…';
    if (await approve(approval)) {
      dialog.close();
    } else {
      reason.textContent = 'Consent failed. Close this dialog and refresh plugins before reviewing again.';
      cancel.disabled = false;
    }
    pending = false;
  }
  dialog.addEventListener('cancel', event => { if (pending) event.preventDefault(); });
  const actions = element('div', '', 'plugin-actions');
  actions.append(cancel, confirm);
  dialog.append(title);
  if (approval !== null) {
    dialog.append(element('p', `${approval.id} · Version ${approval.version}`),
      element('p', `Artifact SHA-256: ${approval.digest}`),
      element('p', 'This grants the following capabilities for all projects of this user.'),
      capabilityList(approval.capability_lines), element('h4', 'Exact grants and bounds'),
      element('pre', JSON.stringify(approval.capabilities, null, 2)));
  } else {
    reason.textContent = 'Consent metadata is unavailable. Refresh plugins before granting consent.';
  }
  dialog.append(reason, actions);
  dialog.addEventListener('close', () => {
    dialog.remove();
    if (trigger.isConnected) trigger.focus();
    else document.querySelector<HTMLElement>('[data-section-id="plugins"] > button')?.focus();
  }, { once: true });
  document.body.append(dialog);
  dialog.showModal();
}

export function initPlugins(): void {
  const section = document.querySelector<HTMLElement>('[data-section-id="plugins"]');
  if (!section || section.dataset.initialized === 'true') return;
  section.dataset.initialized = 'true';

  const header = element('div', '', 'section-header');
  header.append(element('h2', 'Plugins', 'section-title'), element('p',
    'Manage bundled and installed plugins and review their requested capabilities.',
    'section-description'));
  const status = element('p', '', 'section-description');
  status.setAttribute('role', 'status');
  status.setAttribute('aria-live', 'polite');
  const list = element('div', '', 'section-content');
  const refresh = button('Refresh plugins', () => { void load(); });
  section.append(header, refresh, status, list);

  let rows: PluginRow[] = [];
  let busy = false;

  function setBusy(value: boolean): void {
    busy = value;
    section!.setAttribute('aria-busy', String(value));
    refresh.disabled = value;
    list.querySelectorAll('button').forEach(node => { node.disabled = value; });
  }

  function render(): void {
    const models = pluginsToViewModels(rows);
    list.replaceChildren();
    if (models.length === 0) list.append(element('p', 'No plugins found.'));
    for (const plugin of models) {
      const card = element('article', '', 'plugin-card');
      const heading = element('div', '', 'plugin-heading');
      heading.append(element('h3', plugin.name),
        element('span', plugin.sourceLabel, 'plugin-badge'),
        element('span', plugin.stateLabel, plugin.badgeClass));
      card.append(heading, element('p', `${plugin.id} · ${plugin.versionLabel}`, 'label-hint'),
        element('h4', 'Requested capabilities'), capabilityList(plugin.capabilityLines));
      if (plugin.lastFailure !== null) {
        card.append(element('p', `Last failure: ${plugin.lastFailure}`, 'plugin-failure'));
      }
      const actions = element('div', '', 'plugin-actions');
      if (plugin.canEnable || plugin.canDisable) {
        actions.append(button(plugin.toggleLabel, () => {
          void run(async () => {
            const row = await invoke<PluginRow>(
              plugin.canDisable ? 'disable_plugin' : 'enable_plugin', { id: plugin.id });
            rows = await invoke<PluginRow[]>('list_plugins');
            rows = rows.map(existing => existing.id === row.id
              ? { ...row, consent: existing.consent } : existing);
            return `${row.name}: ${pluginsToViewModels([row])[0].stateLabel}.`;
          });
        }));
      }
      if (plugin.canGrantConsent) {
        const payload = rows.find(row => row.id === plugin.id)?.consent ?? null;
        const grant = button('Grant consent', () => reviewConsent(plugin, payload, grant,
          approval => run(async () => {
            await invoke<PluginRow>('consent_plugin', { id: approval.id, approval });
            rows = await invoke<PluginRow[]>('list_plugins');
            return `${plugin.name}: consent granted and plugin enabled.`;
          })));
        actions.append(grant);
      }
      if (plugin.canCheckUpdate) {
        actions.append(button('Check for update', () => {
          void run(async () => {
            const update = await invoke<PluginUpdate>('check_plugin_update', { id: plugin.id });
            if (update.containing_release) {
              return `${plugin.name}: bundled updates come with a desktop release. `
                + 'Update discovery is offline; no network adapter is available.';
            }
            return update.version === null
              ? `${plugin.name}: no cached update. Update discovery is offline; no network adapter is available.`
              : `${plugin.name}: cached update ${update.version}. Online discovery is unavailable.`;
          }, true);
        }));
      }
      card.append(actions);
      list.append(card);
    }
  }

  async function run(operation: () => Promise<string>, updateCheck = false): Promise<boolean> {
    if (busy) return false;
    setBusy(true);
    status.textContent = 'Working…';
    try {
      status.textContent = await operation();
      render();
      return true;
    } catch (error) {
      status.textContent = updateCheck
        ? `Update discovery is offline; no network adapter is available. Check failed: ${String(error)}`
        : `Plugin operation failed: ${String(error)}`;
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function load(): Promise<void> {
    await run(async () => {
      rows = await invoke<PluginRow[]>('list_plugins');
      return rows.length === 0 ? 'No plugins found.' : `${rows.length} plugin(s) loaded.`;
    });
  }

  void load();
}
