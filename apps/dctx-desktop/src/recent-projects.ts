// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
// apps/dctx-desktop/src/recent-projects.ts
//
// Recent Projects Management Module
// Handles loading, saving, and displaying recently accessed projects

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { navigateToMain } from "./navigation";

// Check if running in Tauri (vs browser dev mode)
const isTauri = '__TAURI__' in window;

/**
 * RecentProject represents a single project in the recent list
 */
export interface RecentProject {
  path: string;
  name: string;
  last_accessed: string; // ISO timestamp
  access_count: number;
}

/**
 * Load recent projects from backend storage
 */
export async function loadRecentProjects(): Promise<RecentProject[]> {
  if (!isTauri) {
    console.warn("Recent projects only work in Tauri mode");
    return [];
  }

  try {
    const projects = await invoke<RecentProject[]>("get_recent_projects");
    return projects;
  } catch (error) {
    console.error("Failed to load recent projects:", error);
    return [];
  }
}

/**
 * Add a project to recent projects list
 */
export async function addRecentProject(path: string): Promise<void> {
  if (!isTauri) {
    console.warn("Recent projects only work in Tauri mode");
    return;
  }

  try {
    await invoke("add_recent_project", { path });
    console.log(`Added to recent projects: ${path}`);
  } catch (error) {
    console.error("Failed to add recent project:", error);
    throw error;
  }
}

/**
 * Remove a project from recent projects list
 */
export async function removeRecentProject(path: string): Promise<void> {
  if (!isTauri) {
    console.warn("Recent projects only work in Tauri mode");
    return;
  }

  try {
    await invoke("remove_recent_project", { path });
    console.log(`Removed from recent projects: ${path}`);
  } catch (error) {
    console.error("Failed to remove recent project:", error);
    throw error;
  }
}

/**
 * Format timestamp as human-readable relative time
 */
function formatRelativeTime(isoTimestamp: string): string {
  const date = new Date(isoTimestamp);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffSec = Math.floor(diffMs / 1000);
  const diffMin = Math.floor(diffSec / 60);
  const diffHour = Math.floor(diffMin / 60);
  const diffDay = Math.floor(diffHour / 24);

  if (diffSec < 60) {
    return "just now";
  } else if (diffMin < 60) {
    return `${diffMin} minute${diffMin !== 1 ? 's' : ''} ago`;
  } else if (diffHour < 24) {
    return `${diffHour} hour${diffHour !== 1 ? 's' : ''} ago`;
  } else if (diffDay === 1) {
    return "yesterday";
  } else if (diffDay < 7) {
    return `${diffDay} days ago`;
  } else if (diffDay < 30) {
    const weeks = Math.floor(diffDay / 7);
    return `${weeks} week${weeks !== 1 ? 's' : ''} ago`;
  } else if (diffDay < 365) {
    const months = Math.floor(diffDay / 30);
    return `${months} month${months !== 1 ? 's' : ''} ago`;
  } else {
    const years = Math.floor(diffDay / 365);
    return `${years} year${years !== 1 ? 's' : ''} ago`;
  }
}

/**
 * Render projects list into the DOM
 */
function renderProjectsList(projects: RecentProject[]) {
  const listEl = document.querySelector("#projects-list");
  if (!listEl) {
    console.error("Projects list element not found");
    return;
  }

  // Clear existing content
  listEl.innerHTML = "";

  if (projects.length === 0) {
    // Show empty state
    listEl.innerHTML = `
      <div class="empty-state">
        <div class="empty-icon">📂</div>
        <div class="empty-text">No recent projects</div>
        <div class="empty-hint">Generate a snapshot to add a project here</div>
      </div>
    `;
    return;
  }

  // Render project cards
  projects.forEach((project) => {
    const card = document.createElement("div");
    card.className = "project-card";
    card.innerHTML = `
      <div class="project-info">
        <div class="project-name">${escapeHtml(project.name)}</div>
        <div class="project-path">${escapeHtml(project.path)}</div>
        <div class="project-meta">
          <span class="project-time">${formatRelativeTime(project.last_accessed)}</span>
          <span class="project-count">•</span>
          <span class="project-count">${project.access_count} time${project.access_count !== 1 ? 's' : ''}</span>
        </div>
      </div>
      <div class="project-actions">
        <button class="btn-open" data-path="${escapeHtml(project.path)}">Open</button>
        <button class="btn-remove" data-path="${escapeHtml(project.path)}">Remove</button>
      </div>
    `;

    listEl.appendChild(card);
  });

  // Wire up event handlers
  listEl.querySelectorAll(".btn-open").forEach((btn) => {
    btn.addEventListener("click", handleOpenProject);
  });

  listEl.querySelectorAll(".btn-remove").forEach((btn) => {
    btn.addEventListener("click", handleRemoveProject);
  });
}

/**
 * Handle "Open" button click - pre-fill path and navigate to main screen
 */
async function handleOpenProject(e: Event) {
  const btn = e.target as HTMLButtonElement;
  const path = btn.dataset.path;
  if (!path) return;

  // Pre-fill the path input on main screen
  const pathInput = document.querySelector("#path-input") as HTMLInputElement;
  if (pathInput) {
    pathInput.value = path;
  }

  // Navigate to main screen
  navigateToMain();

  // Set status
  const statusEl = document.querySelector("#status");
  if (statusEl) {
    statusEl.textContent = `Opened: ${path}`;
  }
}

/**
 * Handle "Remove" button click - confirm and remove from list
 */
async function handleRemoveProject(e: Event) {
  const btn = e.target as HTMLButtonElement;
  const path = btn.dataset.path;
  if (!path) return;

  // Confirm removal
  const confirmed = confirm(`Remove "${path}" from recent projects?`);
  if (!confirmed) return;

  try {
    await removeRecentProject(path);
    // Reload projects list
    await initProjectsScreen();
  } catch (error) {
    console.error("Failed to remove project:", error);
    alert("Failed to remove project. See console for details.");
  }
}

/**
 * Handle "Add New Project" button - open folder picker
 */
async function handleAddNewProject() {
  if (!isTauri) {
    alert("Folder picker only works in desktop app");
    return;
  }

  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Select Project Directory",
    });

    if (selected && typeof selected === "string") {
      await addRecentProject(selected);
      // Reload projects list
      await initProjectsScreen();
    }
  } catch (error) {
    console.error("Failed to add project:", error);
    alert("Failed to add project. See console for details.");
  }
}

/**
 * Initialize projects screen - load and render projects
 */
export async function initProjectsScreen(): Promise<void> {
  const projects = await loadRecentProjects();
  renderProjectsList(projects);

  // Wire up "Add New Project" button if not already wired
  const addBtn = document.querySelector("#add-project-btn") as HTMLElement;
  if (addBtn && !addBtn.dataset.wired) {
    addBtn.addEventListener("click", handleAddNewProject);
    addBtn.dataset.wired = "true";
  }
}

/**
 * Escape HTML to prevent XSS
 */
function escapeHtml(unsafe: string): string {
  return unsafe
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

/**
 * Call this after successful snapshot generation
 */
export async function recordProjectAccess(path: string): Promise<void> {
  try {
    await addRecentProject(path);
  } catch (error) {
    // Non-fatal: log but don't disrupt user flow
    console.error("Failed to record project access:", error);
  }
}
