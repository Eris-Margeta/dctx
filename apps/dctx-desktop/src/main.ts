// apps/dctx-desktop/src/main.ts
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";

// Check if running in Tauri (vs browser dev mode)
const isTauri = '__TAURI__' in window;

let pathInputEl: HTMLInputElement | null;
let outputEl: HTMLElement | null;
let generateBtnEl: HTMLButtonElement | null;
let clearBtnEl: HTMLButtonElement | null;
let browseBtnEl: HTMLButtonElement | null;
let copyBtnEl: HTMLButtonElement | null;
let dropZoneEl: HTMLElement | null;
let btnTextEl: HTMLElement | null;
let btnLoadingEl: HTMLElement | null;
let statusEl: HTMLElement | null;
let lastGeneratedOutput: string = "";

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

function setPath(path: string) {
  if (pathInputEl) {
    pathInputEl.value = path;
    setStatus(`Selected: ${path}`);
  }
}

async function browseFolderDialog() {
  if (!isTauri) {
    setStatus("⚠️ Browse only works in desktop app (not browser dev mode)");
    return;
  }

  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Select Project Directory",
    });

    if (selected && typeof selected === "string") {
      setPath(selected);
    }
  } catch (error) {
    console.error("Failed to open dialog:", error);
    setStatus("Error opening folder dialog");
  }
}

async function generateSnapshot() {
  if (!pathInputEl || !outputEl) return;

  const path = pathInputEl.value.trim();

  if (!path) {
    outputEl.textContent = "❌ Error: Please enter a valid directory path or drag & drop a folder";
    outputEl.style.color = "#ef4444";
    setStatus("Error: No path provided");
    return;
  }

  if (!isTauri) {
    outputEl.textContent = "⚠️ Snapshot generation only works in the desktop app.\n\nRun: bun run tauri:dev";
    outputEl.style.color = "var(--text-secondary)";
    setStatus("Not running in Tauri");
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
      lastGeneratedOutput = response.output;  // Store for copy
      outputEl.textContent = `✅ Success!\n\n${response.output}`;
      outputEl.style.color = "var(--text-primary)";
      setStatus(`Snapshot generated successfully`);

      // Enable copy button
      if (copyBtnEl) {
        copyBtnEl.disabled = false;
      }
    } else {
      lastGeneratedOutput = "";  // Clear on error
      outputEl.textContent = `❌ Error:\n\n${response.error}`;
      outputEl.style.color = "#ef4444";
      setStatus("Error generating snapshot");

      // Disable copy button
      if (copyBtnEl) {
        copyBtnEl.disabled = true;
      }
    }
  } catch (error) {
    lastGeneratedOutput = "";  // Clear on error
    outputEl.textContent = `❌ Fatal Error:\n\n${error}`;
    outputEl.style.color = "#ef4444";
    setStatus("Fatal error occurred");

    // Disable copy button
    if (copyBtnEl) {
      copyBtnEl.disabled = true;
    }
  } finally {
    setLoading(false);
  }
}

async function copyToClipboard() {
  if (!isTauri || !lastGeneratedOutput) {
    setStatus("No snapshot to copy");
    return;
  }

  try {
    await invoke("copy_to_clipboard", { text: lastGeneratedOutput });

    // Visual feedback: change button to success state
    if (copyBtnEl) {
      copyBtnEl.classList.add("success");
      copyBtnEl.innerHTML = '<span>✅</span><span>Copied!</span>';

      // Reset after 2 seconds
      setTimeout(() => {
        if (copyBtnEl) {
          copyBtnEl.classList.remove("success");
          copyBtnEl.innerHTML = '<span>📋</span><span>Copy to Clipboard</span>';
        }
      }, 2000);
    }

    setStatus("Copied to clipboard");
  } catch (error) {
    console.error("Failed to copy:", error);
    setStatus("Failed to copy to clipboard");
  }
}

function clearOutput() {
  if (outputEl) {
    outputEl.textContent = "";
    outputEl.style.color = "var(--text-primary)";
  }
  if (pathInputEl) {
    pathInputEl.value = "";
  }
  if (copyBtnEl) {
    copyBtnEl.disabled = true;
  }
  lastGeneratedOutput = "";
  setStatus("Ready");
}

// Drag and drop handlers - attach to entire document for instant feedback
let dragCounter = 0; // Track nested drag events

function handleDragEnter(e: DragEvent) {
  e.preventDefault();
  e.stopPropagation();
  dragCounter++;

  if (dropZoneEl && dragCounter === 1) {
    dropZoneEl.classList.add("drag-over");
    console.log("Drag enter - counter:", dragCounter);
  }
}

function handleDragOver(e: DragEvent) {
  e.preventDefault();
  e.stopPropagation();
  // Must prevent default to allow drop
}

function handleDragLeave(e: DragEvent) {
  e.preventDefault();
  e.stopPropagation();
  dragCounter--;

  if (dropZoneEl && dragCounter === 0) {
    dropZoneEl.classList.remove("drag-over");
    console.log("Drag leave - counter:", dragCounter);
  }
}

function handleDrop(e: DragEvent) {
  e.preventDefault();
  e.stopPropagation();
  dragCounter = 0;

  if (dropZoneEl) {
    dropZoneEl.classList.remove("drag-over");
  }

  console.log("Drop event fired on DOM");
  // Note: Tauri handles file drops via events, not through DataTransfer
  // The actual drop handling is done via the Tauri event listener below
}

window.addEventListener("DOMContentLoaded", async () => {
  pathInputEl = document.querySelector("#path-input");
  outputEl = document.querySelector("#output");
  generateBtnEl = document.querySelector("#generate-btn");
  clearBtnEl = document.querySelector("#clear-btn");
  browseBtnEl = document.querySelector("#browse-btn");
  copyBtnEl = document.querySelector("#copy-btn");
  dropZoneEl = document.querySelector("#drop-zone");
  btnTextEl = document.querySelector("#btn-text");
  btnLoadingEl = document.querySelector("#btn-loading");
  statusEl = document.querySelector("#status");

  // Click event listeners
  generateBtnEl?.addEventListener("click", () => generateSnapshot());
  clearBtnEl?.addEventListener("click", () => clearOutput());
  browseBtnEl?.addEventListener("click", (e) => {
    e.preventDefault();
    e.stopPropagation();
    browseFolderDialog();
  });
  copyBtnEl?.addEventListener("click", () => copyToClipboard());

  // Enter key on input
  pathInputEl?.addEventListener("keypress", (e) => {
    if (e.key === "Enter") {
      generateSnapshot();
    }
  });

  // Drag and drop - attach to entire document for instant visual feedback
  document.addEventListener("dragenter", handleDragEnter);
  document.addEventListener("dragover", handleDragOver);
  document.addEventListener("dragleave", handleDragLeave);
  document.addEventListener("drop", handleDrop);

  // Also allow clicking the drop zone to browse
  if (dropZoneEl) {
    dropZoneEl.addEventListener("click", (e) => {
      // Don't trigger if clicking the browse button
      if (e.target !== browseBtnEl && !browseBtnEl?.contains(e.target as Node)) {
        browseFolderDialog();
      }
    });
  }

  // Tauri file drop event listener (only in desktop app)
  let unlisten1: (() => void) | undefined;
  let unlisten2: (() => void) | undefined;
  let unlisten3: (() => void) | undefined;

  if (isTauri) {
    console.log("Setting up Tauri file drop listeners...");

    try {
      // Try the correct Tauri 2.0 event name
      unlisten1 = await listen<string[]>("tauri://file-drop", (event) => {
        console.log("✅ File drop event (tauri://file-drop):", event);
        console.log("  Payload:", event.payload);

        // In Tauri 2.0, payload is directly the array of paths
        const paths = event.payload;
        if (paths && paths.length > 0) {
          const path = paths[0];
          console.log("  Setting path:", path);
          setPath(path);
          setStatus(`Dropped: ${path}`);
        }
      });

      // Also try alternative event structure
      unlisten2 = await listen<{ paths: string[] }>("tauri://file-drop", (event) => {
        console.log("✅ File drop event (alt structure):", event);
        if (event.payload.paths && event.payload.paths.length > 0) {
          const path = event.payload.paths[0];
          console.log("  Setting path:", path);
          setPath(path);
          setStatus(`Dropped: ${path}`);
        }
      });

      // Fallback for older event name
      unlisten3 = await listen<any>("tauri://drag-drop", (event) => {
        console.log("✅ Drag-drop event (tauri://drag-drop):", event);
        const paths = Array.isArray(event.payload) ? event.payload : event.payload?.paths;
        if (paths && paths.length > 0) {
          setPath(paths[0]);
          setStatus(`Dropped: ${paths[0]}`);
        }
      });

      console.log("✅ All Tauri event listeners registered");
    } catch (error) {
      console.error("Failed to setup Tauri event listeners:", error);
    }
  } else {
    console.log("⚠️ Running in browser mode - Tauri features disabled");
    setStatus("Running in browser dev mode - use 'bun run tauri:dev' for full features");
  }

  // Set initial status
  setStatus("Ready - Drag & drop a folder to start");

  // Detect system theme
  const isDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  console.log(`System theme: ${isDark ? "dark" : "light"} mode`);

  // Listen for theme changes
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", (e) => {
    console.log(`Theme changed to: ${e.matches ? "dark" : "light"} mode`);
  });

  // Cleanup on page unload
  window.addEventListener("beforeunload", () => {
    if (unlisten1) unlisten1();
    if (unlisten2) unlisten2();
    if (unlisten3) unlisten3();
    document.removeEventListener("dragenter", handleDragEnter);
    document.removeEventListener("dragover", handleDragOver);
    document.removeEventListener("dragleave", handleDragLeave);
    document.removeEventListener("drop", handleDrop);
  });
});
