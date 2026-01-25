// apps/dctx-desktop/src/main.ts
import { invoke } from "@tauri-apps/api/core";

let pathInputEl: HTMLInputElement | null;
let outputEl: HTMLElement | null;
let generateBtnEl: HTMLButtonElement | null;

async function generateSnapshot() {
  if (pathInputEl && outputEl && generateBtnEl) {
    const path = pathInputEl.value;

    if (!path) {
      outputEl.textContent = "Please enter a path";
      return;
    }

    generateBtnEl.disabled = true;
    outputEl.textContent = "Generating snapshot...";

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
        outputEl.textContent = response.output;
      } else {
        outputEl.textContent = `Error: ${response.error}`;
      }
    } catch (error) {
      outputEl.textContent = `Error: ${error}`;
    } finally {
      generateBtnEl.disabled = false;
    }
  }
}

window.addEventListener("DOMContentLoaded", () => {
  pathInputEl = document.querySelector("#path-input");
  outputEl = document.querySelector("#output");
  generateBtnEl = document.querySelector("#generate-btn");

  generateBtnEl?.addEventListener("click", () => generateSnapshot());
});
