// apps/dctx-desktop/src/main.ts
import { invoke } from "@tauri-apps/api/core";

let pathInputEl: HTMLInputElement | null;
let outputEl: HTMLElement | null;
let generateBtnEl: HTMLButtonElement | null;
let clearBtnEl: HTMLButtonElement | null;
let btnTextEl: HTMLElement | null;
let btnLoadingEl: HTMLElement | null;
let statusEl: HTMLElement | null;

function setStatus(message: string) {
  if (statusEl) {
    statusEl.textContent = message;
  }
}

function setLoading(loading: boolean) {
  if (generateBtnEl && btnTextEl && btnLoadingEl) {
    generateBtnEl.disabled = loading;
    btnTextEl.style.display = loading ? "none" : "inline";
    btnLoadingEl.style.display = loading ? "inline-block" : "none";
  }
}

async function generateSnapshot() {
  if (!pathInputEl || !outputEl) return;

  const path = pathInputEl.value.trim();

  if (!path) {
    outputEl.textContent = "❌ Error: Please enter a valid directory path";
    outputEl.style.color = "#ef4444";
    setStatus("Error: No path provided");
    return;
  }

  setLoading(true);
  setStatus("Generating snapshot...");
  outputEl.textContent = "⏳ Scanning directory structure...\n\nThis may take a moment for large projects.";
  outputEl.style.color = "var(--text-secondary)";

  try {
    const response: any = await invoke("generate_snapshot", {
      request: {
        path,
        filter_extensions: [],
        include_binary: false,
        only_empty: false,
        only_non_empty: false,
        min_size_mb: null,
        max_size_mb: null,
        only_hidden: false,
        tree_only: false,
      },
    });

    if (response.success) {
      outputEl.textContent = `✅ Success!\n\n${response.output}`;
      outputEl.style.color = "var(--text-primary)";
      setStatus(`Snapshot generated successfully (${path})`);
    } else {
      outputEl.textContent = `❌ Error:\n\n${response.error}`;
      outputEl.style.color = "#ef4444";
      setStatus("Error generating snapshot");
    }
  } catch (error) {
    outputEl.textContent = `❌ Fatal Error:\n\n${error}`;
    outputEl.style.color = "#ef4444";
    setStatus("Fatal error occurred");
  } finally {
    setLoading(false);
  }
}

function clearOutput() {
  if (outputEl) {
    outputEl.textContent = "";
    outputEl.style.color = "var(--text-primary)";
  }
  if (pathInputEl) {
    pathInputEl.value = "";
    pathInputEl.focus();
  }
  setStatus("Ready");
}

window.addEventListener("DOMContentLoaded", () => {
  pathInputEl = document.querySelector("#path-input");
  outputEl = document.querySelector("#output");
  generateBtnEl = document.querySelector("#generate-btn");
  clearBtnEl = document.querySelector("#clear-btn");
  btnTextEl = document.querySelector("#btn-text");
  btnLoadingEl = document.querySelector("#btn-loading");
  statusEl = document.querySelector("#status");

  // Event listeners
  generateBtnEl?.addEventListener("click", () => generateSnapshot());
  clearBtnEl?.addEventListener("click", () => clearOutput());

  // Enter key on input
  pathInputEl?.addEventListener("keypress", (e) => {
    if (e.key === "Enter") {
      generateSnapshot();
    }
  });

  // Set initial status
  setStatus("Ready");

  // Auto-focus input
  pathInputEl?.focus();

  // Detect system theme
  const isDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  console.log(`System theme: ${isDark ? "dark" : "light"} mode`);

  // Listen for theme changes
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", (e) => {
    console.log(`Theme changed to: ${e.matches ? "dark" : "light"} mode`);
  });
});
