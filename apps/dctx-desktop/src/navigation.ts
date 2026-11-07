// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
// apps/dctx-desktop/src/navigation.ts
//
// Screen navigation and state management for multi-screen desktop app

export type ScreenType = 'main' | 'projects' | 'settings';

export interface AppState {
  currentScreen: ScreenType;
  selectedProject: string | null;
  isProjectSpecificSettings: boolean;
}

// Global state
let state: AppState = {
  currentScreen: 'main',
  selectedProject: null,
  isProjectSpecificSettings: false,
};

// Screen containers
let mainScreen: HTMLElement | null = null;
let projectsScreen: HTMLElement | null = null;
let settingsScreen: HTMLElement | null = null;

// Navigation buttons
let projectsNavBtn: HTMLElement | null = null;
let settingsBtn: HTMLElement | null = null;

/**
 * Initialize navigation system
 * Call this once on DOMContentLoaded
 */
export function initNavigation() {
  // Get screen containers
  mainScreen = document.querySelector('#main-screen');
  projectsScreen = document.querySelector('#projects-screen');
  settingsScreen = document.querySelector('#settings-screen');

  // Get navigation buttons
  projectsNavBtn = document.querySelector('#projects-nav-btn');
  settingsBtn = document.querySelector('#settings-btn');

  // Attach event listeners
  if (projectsNavBtn) {
    projectsNavBtn.addEventListener('click', () => navigateToProjects());
  }

  if (settingsBtn) {
    settingsBtn.addEventListener('click', () => navigateToSettings());
  }

  // Show initial screen
  showScreen(state.currentScreen);
}

/**
 * Navigate to a specific screen with transition
 */
export function navigateToScreen(screen: ScreenType) {
  if (state.currentScreen === screen) return;

  // Update state
  const previousScreen = state.currentScreen;
  state.currentScreen = screen;

  // Perform transition
  transitionScreens(previousScreen, screen);

  // Update navigation visibility
  updateNavigationVisibility();
}

/**
 * Navigate to main screen
 */
export function navigateToMain() {
  navigateToScreen('main');
}

/**
 * Navigate to projects list screen
 */
export function navigateToProjects() {
  state.selectedProject = null;
  state.isProjectSpecificSettings = false;
  navigateToScreen('projects');

  // Initialize projects screen when navigating to it
  // Dynamic import to avoid circular dependency
  import('./recent-projects').then(({ initProjectsScreen }) => {
    initProjectsScreen().catch(err => {
      console.error('Failed to initialize projects screen:', err);
    });
  });
}

/**
 * Show projects screen (exported for use by recent-projects module)
 */
export function showProjectsScreen() {
  navigateToProjects();
}

/**
 * Navigate to settings screen
 * @param projectPath Optional project path for project-specific settings
 */
export function navigateToSettings(projectPath?: string) {
  if (projectPath) {
    state.selectedProject = projectPath;
    state.isProjectSpecificSettings = true;
  } else {
    state.selectedProject = null;
    state.isProjectSpecificSettings = false;
  }
  navigateToScreen('settings');
}

/**
 * Get current navigation state
 */
export function getState(): AppState {
  return { ...state };
}

/**
 * Transition between screens with fade animation
 */
function transitionScreens(from: ScreenType, to: ScreenType) {
  const fromElement = getScreenElement(from);
  const toElement = getScreenElement(to);

  if (!fromElement || !toElement) {
    console.error('Screen elements not found:', { from, to });
    return;
  }

  // Fade out current screen
  fromElement.classList.add('fade-out');

  setTimeout(() => {
    // Hide old screen
    fromElement.classList.remove('screen-active', 'fade-out');
    fromElement.classList.add('screen-hidden');

    // Show new screen
    toElement.classList.remove('screen-hidden');
    toElement.classList.add('screen-active', 'fade-in');

    // Remove fade-in class after animation
    setTimeout(() => {
      toElement.classList.remove('fade-in');
    }, 300);
  }, 200);
}

/**
 * Show a screen immediately (no transition)
 */
function showScreen(screen: ScreenType) {
  const screens = [mainScreen, projectsScreen, settingsScreen];
  const screenTypes: ScreenType[] = ['main', 'projects', 'settings'];

  screens.forEach((el, index) => {
    if (!el) return;

    if (screenTypes[index] === screen) {
      el.classList.remove('screen-hidden');
      el.classList.add('screen-active');
    } else {
      el.classList.remove('screen-active');
      el.classList.add('screen-hidden');
    }
  });

  updateNavigationVisibility();
}

/**
 * Update visibility of navigation buttons based on current screen
 */
function updateNavigationVisibility() {
  // Projects button: visible on main and settings screens, hidden on projects screen
  if (projectsNavBtn) {
    if (state.currentScreen === 'projects') {
      projectsNavBtn.classList.add('hidden');
    } else {
      projectsNavBtn.classList.remove('hidden');
    }
  }

  // Settings button: always visible
  if (settingsBtn) {
    settingsBtn.classList.remove('hidden');
  }
}

/**
 * Get screen element by type
 */
function getScreenElement(screen: ScreenType): HTMLElement | null {
  switch (screen) {
    case 'main':
      return mainScreen;
    case 'projects':
      return projectsScreen;
    case 'settings':
      return settingsScreen;
    default:
      return null;
  }
}
