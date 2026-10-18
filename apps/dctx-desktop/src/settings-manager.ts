// apps/dctx-desktop/src/settings-manager.ts
//
// Settings Manager - Handles loading, saving, and syncing desktop settings with backend

import { invoke } from "@tauri-apps/api/core";

/**
 * Desktop configuration structure (matches Rust DesktopConfig)
 */
export interface DesktopConfig {
  last_directory: string | null;
  window_width: number;
  window_height: number;
  file_count_limit: number;
  theme: string;
  auto_copy_to_clipboard: boolean;
  // Filter settings
  filter_extensions: string[] | null;
  include_binary: boolean;
  only_hidden: boolean;
  min_size_mb: number | null;
  max_size_mb: number | null;
  only_empty: boolean;
  only_non_empty: boolean;
  tree_only: boolean;
}

/**
 * Current settings state (cached in memory)
 */
let currentSettings: DesktopConfig | null = null;

/**
 * Starts at zero and increases for every save or reset.
 * A completion applies only when it is still the latest.
 */
let saveSequence = 0;

/** True while syncForm is writing controls, so those writes do not autosave. */
let applyingForm = false;

function publishSaveSequence(sequence: number): void {
  (globalThis as { __dctxPendingSeq?: number }).__dctxPendingSeq = sequence;
}

function closeSaveSequence(sequence: number): void {
  const scope = globalThis as { __dctxPendingSeq?: number; __dctxClosedSeq?: number };
  scope.__dctxPendingSeq = sequence;
  scope.__dctxClosedSeq = sequence;
}

/**
 * Load settings from backend
 */
export async function loadSettings(): Promise<DesktopConfig> {
  try {
    const settings = cloneConfig(await invoke<DesktopConfig>("get_desktop_settings"));
    currentSettings = settings;
    return settings;
  } catch (error) {
    console.error("Failed to load settings:", error);
    // Return defaults on error
    const defaults = getDefaultSettings();
    currentSettings = defaults;
    return defaults;
  }
}

function cloneConfig(settings: DesktopConfig): DesktopConfig {
  const extensions = settings.filter_extensions;
  return {
    ...getDefaultSettings(),
    ...settings,
    filter_extensions: Array.isArray(extensions) ? [...extensions] : null,
  };
}

/**
 * Save settings to backend.
 * A slower save that started earlier is ignored so it cannot restore pre-reset privacy settings.
 */
export async function saveSettings(settings: DesktopConfig): Promise<void> {
  const sequence = ++saveSequence;
  const snapshot = cloneConfig(settings);
  publishSaveSequence(sequence);
  try {
    await invoke("save_desktop_settings", { config: snapshot });
    if (sequence !== saveSequence) return;
    currentSettings = snapshot;
    console.log("Settings saved successfully");
  } catch (error) {
    console.error("Failed to save settings:", error);
    if (sequence !== saveSequence) return;
    throw error;
  }
}

/**
 * Reset settings to defaults
 */
export async function resetSettings(): Promise<DesktopConfig> {
  // Close every save issued before this reset so a late completion cannot restore it.
  const sequence = ++saveSequence;
  closeSaveSequence(sequence);
  try {
    await invoke("reset_desktop_settings");
    const settings = await loadSettings();
    return settings;
  } catch (error) {
    console.error("Failed to reset settings:", error);
    throw error;
  }
}

/**
 * Get default settings (fallback)
 */
function getDefaultSettings(): DesktopConfig {
  return {
    last_directory: null,
    window_width: 1200,
    window_height: 800,
    file_count_limit: 10000,
    theme: "system",
    auto_copy_to_clipboard: false,
    filter_extensions: null,
    include_binary: false,
    only_hidden: false,
    min_size_mb: null,
    max_size_mb: null,
    only_empty: false,
    only_non_empty: false,
    tree_only: false,
  };
}

/**
 * Get current cached settings
 */
export function getCurrentSettings(): DesktopConfig | null {
  return currentSettings;
}

async function saveCurrent(update: (settings: DesktopConfig) => void): Promise<void> {
  if (applyingForm) return;
  if (!currentSettings) await loadSettings();
  if (applyingForm || !currentSettings) return;
  update(currentSettings);
  await saveSettings(currentSettings);
}

function extensionText(extensions: DesktopConfig["filter_extensions"]): string {
  return Array.isArray(extensions) ? extensions.join(", ") : "";
}

function syncForm(settings: DesktopConfig): void {
  applyingForm = true;
  try {
    const themeSelect = document.querySelector<HTMLSelectElement>("#theme-select");
    const fileCountInput = document.querySelector<HTMLInputElement>("#file-count-input");
    const autoCopyToggle = document.querySelector<HTMLInputElement>("#auto-copy-toggle");
    const filterExtensionsInput = document.querySelector<HTMLInputElement>("#filter-extensions-input");
    const includeBinaryToggle = document.querySelector<HTMLInputElement>("#include-binary-toggle");
    const onlyHiddenToggle = document.querySelector<HTMLInputElement>("#only-hidden-toggle");
    const minSizeInput = document.querySelector<HTMLInputElement>("#min-size-input");
    const maxSizeInput = document.querySelector<HTMLInputElement>("#max-size-input");
    const treeOnlyToggle = document.querySelector<HTMLInputElement>("#tree-only-toggle");
    const contentFilterRadios = document.querySelectorAll<HTMLInputElement>('input[name="content-filter"]');

    if (themeSelect) themeSelect.value = settings.theme;
    if (fileCountInput) fileCountInput.value = settings.file_count_limit.toString();
    if (autoCopyToggle) autoCopyToggle.checked = settings.auto_copy_to_clipboard;
    if (filterExtensionsInput) filterExtensionsInput.value = extensionText(settings.filter_extensions);
    if (includeBinaryToggle) includeBinaryToggle.checked = settings.include_binary;
    if (onlyHiddenToggle) onlyHiddenToggle.checked = settings.only_hidden;
    if (minSizeInput) minSizeInput.value = settings.min_size_mb?.toString() || "";
    if (maxSizeInput) maxSizeInput.value = settings.max_size_mb?.toString() || "";
    if (treeOnlyToggle) treeOnlyToggle.checked = settings.tree_only;

    let initialValue = "all";
    if (settings.only_empty) initialValue = "empty";
    else if (settings.only_non_empty) initialValue = "non-empty";
    contentFilterRadios.forEach(radio => {
      radio.checked = radio.value === initialValue;
    });
  } finally {
    applyingForm = false;
  }
}

function settingsBound(): boolean {
  const sentinel = document.querySelector<HTMLElement>("#reset-settings-btn")
    ?? document.querySelector<HTMLElement>("#clear-all-cache-btn")
    ?? document.querySelector<HTMLElement>("#theme-select");
  return sentinel?.dataset.dctxSettingsBound === "true";
}

function markSettingsBound(): void {
  for (const id of ["#theme-select", "#reset-settings-btn", "#clear-all-cache-btn"]) {
    const element = document.querySelector<HTMLElement>(id);
    if (element) element.dataset.dctxSettingsBound = "true";
  }
}

/**
 * Install each control's listener once. Reset reloads values through the same handlers.
 */
function wireSettingsListeners(): void {
  const themeSelect = document.querySelector<HTMLSelectElement>("#theme-select");
  const fileCountInput = document.querySelector<HTMLInputElement>("#file-count-input");
  const autoCopyToggle = document.querySelector<HTMLInputElement>("#auto-copy-toggle");
  const filterExtensionsInput = document.querySelector<HTMLInputElement>("#filter-extensions-input");
  const includeBinaryToggle = document.querySelector<HTMLInputElement>("#include-binary-toggle");
  const onlyHiddenToggle = document.querySelector<HTMLInputElement>("#only-hidden-toggle");
  const minSizeInput = document.querySelector<HTMLInputElement>("#min-size-input");
  const maxSizeInput = document.querySelector<HTMLInputElement>("#max-size-input");
  const treeOnlyToggle = document.querySelector<HTMLInputElement>("#tree-only-toggle");
  const contentFilterRadios = document.querySelectorAll<HTMLInputElement>('input[name="content-filter"]');
  const clearButton = document.querySelector<HTMLButtonElement>("#clear-all-cache-btn");
  const resetButton = document.querySelector<HTMLButtonElement>("#reset-settings-btn");

  if (themeSelect) {
    themeSelect.addEventListener("change", async () => {
      await saveCurrent(settings => { settings.theme = themeSelect.value; });
    });
  }

  if (fileCountInput) {
    fileCountInput.addEventListener("change", async () => {
      const value = parseInt(fileCountInput.value, 10);
      if (!isNaN(value) && value >= 100) {
        await saveCurrent(settings => { settings.file_count_limit = value; });
      }
    });
  }

  if (autoCopyToggle) {
    autoCopyToggle.addEventListener("change", async () => {
      await saveCurrent(settings => { settings.auto_copy_to_clipboard = autoCopyToggle.checked; });
    });
  }

  if (filterExtensionsInput) {
    filterExtensionsInput.addEventListener("blur", async () => {
      await saveCurrent(settings => {
        const value = filterExtensionsInput.value.trim();
        settings.filter_extensions = value
          ? value.split(",").map(part => part.trim()).filter(part => part.length > 0)
          : null;
      });
    });
  }

  if (includeBinaryToggle) {
    includeBinaryToggle.addEventListener("change", async () => {
      await saveCurrent(settings => { settings.include_binary = includeBinaryToggle.checked; });
    });
  }

  if (onlyHiddenToggle) {
    onlyHiddenToggle.addEventListener("change", async () => {
      await saveCurrent(settings => { settings.only_hidden = onlyHiddenToggle.checked; });
    });
  }

  if (minSizeInput) {
    minSizeInput.addEventListener("blur", async () => {
      await saveCurrent(settings => {
        const value = parseFloat(minSizeInput.value);
        settings.min_size_mb = !isNaN(value) && value > 0 ? value : null;
      });
    });
  }

  if (maxSizeInput) {
    maxSizeInput.addEventListener("blur", async () => {
      await saveCurrent(settings => {
        const value = parseFloat(maxSizeInput.value);
        settings.max_size_mb = !isNaN(value) && value > 0 ? value : null;
      });
    });
  }

  contentFilterRadios.forEach(radio => {
    radio.addEventListener("change", async () => {
      await saveCurrent(settings => {
        settings.only_empty = radio.value === "empty";
        settings.only_non_empty = radio.value === "non-empty";
      });
    });
  });

  if (treeOnlyToggle) {
    treeOnlyToggle.addEventListener("change", async () => {
      await saveCurrent(settings => { settings.tree_only = treeOnlyToggle.checked; });
    });
  }

  if (clearButton) {
    clearButton.addEventListener("click", async () => {
      const confirmed = confirm(
        "Are you sure you want to clear all cache? This cannot be undone."
      );
      if (!confirmed) return;
      try {
        await invoke("clear_all_cache");
        alert("Cache cleared successfully");
      } catch (error) {
        console.error("Failed to clear cache:", error);
        alert("Failed to clear cache");
      }
    });
  }

  if (resetButton) {
    resetButton.addEventListener("click", async () => {
      const confirmed = confirm(
        "Are you sure you want to reset all settings to defaults? This cannot be undone."
      );

      if (confirmed) {
        try {
          await resetSettings();
          await initSettingsUI();
          alert("Settings reset to defaults successfully");
        } catch {
          alert("Failed to reset settings");
        }
      }
    });
  }

  markSettingsBound();
}

/**
 * Initialize settings UI by loading values and wiring up form controls
 */
export async function initSettingsUI(): Promise<void> {
  const settings = await loadSettings();
  syncForm(settings);
  // One listener set reads the current settings object. Rebinding on reset would stack stale closures.
  if (!settingsBound()) wireSettingsListeners();

  console.log("Settings UI initialized with:", settings);
}

/**
 * Update a specific setting and save
 */
export async function updateSetting<K extends keyof DesktopConfig>(
  key: K,
  value: DesktopConfig[K]
): Promise<void> {
  if (!currentSettings) {
    await loadSettings();
  }

  if (currentSettings) {
    currentSettings[key] = value;
    await saveSettings(currentSettings);
  }
}
