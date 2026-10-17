// apps/dctx-desktop/src/settings.ts
//
// Settings screen scroll-sync navigation and section management
// Implements settings navigation, including PLUGINS.md §7.3 PL-APP.
// Uses the Plugins DOM adapter; initialized by the desktop entry point.

import { initPlugins } from './plugins';

interface NavigationState {
  activeSection: string;
  sectionsInView: Set<string>;
}

let state: NavigationState = {
  activeSection: 'general',
  sectionsInView: new Set(),
};

let settingsContent: HTMLElement | null = null;
let navItems: NodeListOf<HTMLElement> | null = null;
let sections: NodeListOf<HTMLElement> | null = null;

/**
 * Initialize settings screen scroll-sync system
 */
export function initSettings() {
  settingsContent = document.querySelector('.settings-content');
  navItems = document.querySelectorAll('.settings-nav .nav-item');
  sections = document.querySelectorAll('.settings-section');

  if (!settingsContent || !navItems || !sections) {
    console.warn('Settings elements not found, skipping initialization');
    return;
  }

  // Attach scroll listener
  settingsContent.addEventListener('scroll', handleScroll);

  // Attach click listeners to navigation items
  navItems.forEach(item => {
    item.addEventListener('click', () => {
      const sectionId = item.getAttribute('data-section');
      if (sectionId) {
        scrollToSection(sectionId);
      }
    });
  });

  // Initial highlight update
  initPlugins();
  updateNavigationHighlight();
}

/**
 * Scroll to a specific settings section
 */
export function scrollToSection(sectionId: string) {
  const section = document.querySelector(`[data-section-id="${sectionId}"]`);

  if (!section || !settingsContent) {
    console.warn('Section not found:', sectionId);
    return;
  }

  // Scroll section into view
  section.scrollIntoView({
    behavior: 'smooth',
    block: 'start',
  });
}

/**
 * Handle scroll events and update navigation highlighting
 */
function handleScroll() {
  updateNavigationHighlight();
}

/**
 * Update navigation highlighting based on scroll position
 */
function updateNavigationHighlight() {
  if (!sections || !navItems || !settingsContent) return;

  const viewportHeight = settingsContent.clientHeight;
  const scrollTop = settingsContent.scrollTop;
  const viewportCenter = scrollTop + viewportHeight / 2;

  let activeSection: string | null = null;
  const sectionsInView = new Set<string>();

  // Calculate which sections are in viewport
  sections.forEach(section => {
    const sectionId = section.getAttribute('data-section-id');
    if (!sectionId) return;

    // Convert to content-relative coordinates
    const sectionTop = section.offsetTop;
    const sectionBottom = sectionTop + section.offsetHeight;
    const viewportTop = scrollTop;
    const viewportBottom = scrollTop + viewportHeight;

    // Check if section is in viewport
    const isInView = sectionBottom > viewportTop && sectionTop < viewportBottom;

    if (isInView) {
      sectionsInView.add(sectionId);
    }

    // Check if section is centered (active)
    const sectionCenter = sectionTop + section.offsetHeight / 2;
    const isCentered = Math.abs(sectionCenter - viewportCenter) < section.offsetHeight / 2;

    if (isCentered) {
      activeSection = sectionId;
    }
  });

  // If no section is centered, use the first visible section
  if (!activeSection && sectionsInView.size > 0) {
    activeSection = Array.from(sectionsInView)[0];
  }

  // Update state
  state.activeSection = activeSection || 'general';
  state.sectionsInView = sectionsInView;

  // Update navigation UI
  updateNavigationUI();
}

/**
 * Update navigation item visual states
 */
function updateNavigationUI() {
  if (!navItems) return;

  navItems.forEach(item => {
    const sectionId = item.getAttribute('data-section');
    if (!sectionId) return;

    const isActive = sectionId === state.activeSection;
    const isPartiallyVisible = state.sectionsInView.has(sectionId);

    // Remove all state classes
    item.classList.remove('active', 'partially-visible');

    // Add appropriate class
    if (isActive) {
      item.classList.add('active');
    } else if (isPartiallyVisible) {
      item.classList.add('partially-visible');
    }
  });
}

/**
 * Get current settings state
 */
export function getSettingsState(): NavigationState {
  return { ...state, sectionsInView: new Set(state.sectionsInView) };
}
