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
 * Load settings from backend
 */
export async function loadSettings(): Promise<DesktopConfig> {
  try {
    const settings = await invoke<DesktopConfig>("get_desktop_settings");
    currentSettings = settings;
    return settings;
  } catch (error) {
    console.error("Failed to load settings:", error);
    // Return defaults on error
    return getDefaultSettings();
  }
}

/**
 * Save settings to backend
 */
export async function saveSettings(settings: DesktopConfig): Promise<void> {
  try {
    await invoke("save_desktop_settings", { config: settings });
    currentSettings = settings;
    console.log("Settings saved successfully");
  } catch (error) {
    console.error("Failed to save settings:", error);
    throw error;
  }
}

/**
 * Reset settings to defaults
 */
export async function resetSettings(): Promise<DesktopConfig> {
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

/**
 * Initialize settings UI by loading values and wiring up form controls
 */
export async function initSettingsUI(): Promise<void> {
  // Load settings from backend
  const settings = await loadSettings();

  // Wire up General settings
  const themeSelect = document.querySelector<HTMLSelectElement>("#theme-select");
  const fileCountInput = document.querySelector<HTMLInputElement>("#file-count-input");
  const autoCopyToggle = document.querySelector<HTMLInputElement>("#auto-copy-toggle");

  if (themeSelect) {
    themeSelect.value = settings.theme;
    themeSelect.addEventListener("change", async () => {
      settings.theme = themeSelect.value;
      await saveSettings(settings);
    });
  }

  if (fileCountInput) {
    fileCountInput.value = settings.file_count_limit.toString();
    fileCountInput.addEventListener("change", async () => {
      const value = parseInt(fileCountInput.value, 10);
      if (!isNaN(value) && value >= 100) {
        settings.file_count_limit = value;
        await saveSettings(settings);
      }
    });
  }

  if (autoCopyToggle) {
    autoCopyToggle.checked = settings.auto_copy_to_clipboard;
    autoCopyToggle.addEventListener("change", async () => {
      settings.auto_copy_to_clipboard = autoCopyToggle.checked;
      await saveSettings(settings);
    });
  }

  // Wire up Filter settings
  const filterExtensionsInput = document.querySelector<HTMLInputElement>("#filter-extensions-input");
  const includeBinaryToggle = document.querySelector<HTMLInputElement>("#include-binary-toggle");
  const onlyHiddenToggle = document.querySelector<HTMLInputElement>("#only-hidden-toggle");
  const minSizeInput = document.querySelector<HTMLInputElement>("#min-size-input");
  const maxSizeInput = document.querySelector<HTMLInputElement>("#max-size-input");
  const contentFilterRadios = document.querySelectorAll<HTMLInputElement>('input[name="content-filter"]');
  const treeOnlyToggle = document.querySelector<HTMLInputElement>("#tree-only-toggle");

  if (filterExtensionsInput) {
    filterExtensionsInput.value = settings.filter_extensions?.join(", ") || "";
    filterExtensionsInput.addEventListener("blur", async () => {
      const value = filterExtensionsInput.value.trim();
      if (value) {
        settings.filter_extensions = value.split(",").map(s => s.trim()).filter(s => s.length > 0);
      } else {
        settings.filter_extensions = null;
      }
      await saveSettings(settings);
    });
  }

  if (includeBinaryToggle) {
    includeBinaryToggle.checked = settings.include_binary;
    includeBinaryToggle.addEventListener("change", async () => {
      settings.include_binary = includeBinaryToggle.checked;
      await saveSettings(settings);
    });
  }

  if (onlyHiddenToggle) {
    onlyHiddenToggle.checked = settings.only_hidden;
    onlyHiddenToggle.addEventListener("change", async () => {
      settings.only_hidden = onlyHiddenToggle.checked;
      await saveSettings(settings);
    });
  }

  if (minSizeInput) {
    minSizeInput.value = settings.min_size_mb?.toString() || "";
    minSizeInput.addEventListener("blur", async () => {
      const value = parseFloat(minSizeInput.value);
      settings.min_size_mb = !isNaN(value) && value > 0 ? value : null;
      await saveSettings(settings);
    });
  }

  if (maxSizeInput) {
    maxSizeInput.value = settings.max_size_mb?.toString() || "";
    maxSizeInput.addEventListener("blur", async () => {
      const value = parseFloat(maxSizeInput.value);
      settings.max_size_mb = !isNaN(value) && value > 0 ? value : null;
      await saveSettings(settings);
    });
  }

  if (contentFilterRadios.length > 0) {
    // Set initial state
    let initialValue = "all";
    if (settings.only_empty) initialValue = "empty";
    else if (settings.only_non_empty) initialValue = "non-empty";

    contentFilterRadios.forEach(radio => {
      if (radio.value === initialValue) {
        radio.checked = true;
      }
      radio.addEventListener("change", async () => {
        settings.only_empty = radio.value === "empty";
        settings.only_non_empty = radio.value === "non-empty";
        await saveSettings(settings);
      });
    });
  }

  if (treeOnlyToggle) {
    treeOnlyToggle.checked = settings.tree_only;
    treeOnlyToggle.addEventListener("change", async () => {
      settings.tree_only = treeOnlyToggle.checked;
      await saveSettings(settings);
    });
  }

  // Wire up Reset to Defaults button
  const resetButton = document.querySelector<HTMLButtonElement>(
    '.settings-section[data-section-id="advanced"] button.danger:last-of-type'
  );

  if (resetButton) {
    resetButton.addEventListener("click", async () => {
      const confirmed = confirm(
        "Are you sure you want to reset all settings to defaults? This cannot be undone."
      );

      if (confirmed) {
        try {
          await resetSettings();
          // Reload UI with new values
          await initSettingsUI();
          alert("Settings reset to defaults successfully");
        } catch (error) {
          alert("Failed to reset settings");
        }
      }
    });
  }

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
