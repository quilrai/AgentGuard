// Main entry point - imports and initializes all modules

import { initRouter, onRouteChange, getCurrentRoute } from './utils.js';
import { loadDashboard, loadBackends, initBackendFilter, initTimeFilter } from './dashboard.js';
import {
  loadMessageLogs,
  loadLogsBackends,
  initLogsBackendFilter,
  initLogsTimeFilter,
  initLogsViewTabs,
  initLogsExport
} from './logs.js';
import { initSettings, loadDetections } from './settings.js';
import { initBackends, loadPredefinedBackends, refreshGuardianHooks } from './backends.js';
import { initTokenSaving, refreshTokenSaver } from './token-saving.js';
import { initHome, loadHome, suspendHome, resumeHome } from './home.js';
import { initGarden, loadGarden } from './garden.js';
import { initBehaviour, loadBehaviour } from './behaviour.js';
import { initGuide, startGuide, resetAllSettings } from './guide.js';

const { openUrl } = window.__TAURI__.opener;

// Initialize app
window.addEventListener('DOMContentLoaded', () => {
  // Initialize Lucide icons
  lucide.createIcons();

  // Initialize router
  initRouter();

  // Initialize home (status cards + rotating fact)
  initHome();
  loadHome();

  // Initialize analytics (formerly Dashboard)
  initTimeFilter();
  initBackendFilter();
  loadBackends();
  loadDashboard();

  // Initialize logs
  initLogsTimeFilter();
  initLogsBackendFilter();
  initLogsViewTabs();
  initLogsExport();
  loadLogsBackends();

  // Initialize Guardian Agent sub-modules
  initSettings();
  initBackends();

  // Initialize Token Saver
  initTokenSaving();

  // Initialize Garden
  initGarden();

  // Initialize Behaviour
  initBehaviour();

  // Initialize guided setup (auto-shows for new users)
  initGuide();

  // Refresh buttons
  const refreshBtn = document.getElementById('refresh-btn');
  if (refreshBtn) {
    refreshBtn.addEventListener('click', () => {
      loadBackends();
      loadDashboard();
    });
  }
  const logsRefreshBtn = document.getElementById('logs-refresh-btn');
  if (logsRefreshBtn) {
    logsRefreshBtn.addEventListener('click', () => {
      loadLogsBackends();
      loadMessageLogs();
    });
  }

  // Refresh data on route entry where it makes sense
  onRouteChange((route) => {
    if (route === 'home') {
      loadHome();
      resumeHome();
    } else {
      suspendHome();
    }

    if (route === 'logs') {
      loadMessageLogs();
    }

    if (route === 'guardian') {
      loadPredefinedBackends();
      refreshGuardianHooks();
      loadDetections();
    }

    if (route === 'token-saver') {
      refreshTokenSaver();
    }

    if (route === 'garden') {
      loadGarden();
    }

    if (route === 'behaviour') {
      loadBehaviour();
    }

    if (route === 'analytics') {
      loadBackends();
      loadDashboard();
    }
  });

  // Footer links
  const star = document.getElementById('starGithub');
  if (star) star.addEventListener('click', () => openUrl('https://github.com/quilrai/AgentGuard'));
  const report = document.getElementById('reportIssue');
  if (report) report.addEventListener('click', () => openUrl('https://github.com/quilrai/AgentGuard/issues'));
  // Setup guide & reset (now in home meta bar)
  const menuGuide = document.getElementById('menu-setup-guide');
  if (menuGuide) menuGuide.addEventListener('click', () => startGuide());
  const menuReset = document.getElementById('menu-reset-all');
  if (menuReset) menuReset.addEventListener('click', async () => {
    const ok = confirm('This will remove all hooks and clear all settings to defaults. Continue?');
    if (!ok) return;
    menuReset.style.pointerEvents = 'none';
    await resetAllSettings();
    loadHome();
    loadPredefinedBackends();
    refreshGuardianHooks();
    refreshTokenSaver();
    menuReset.style.pointerEvents = '';
  });

  // Re-run lucide once after dynamic icons are added
  setTimeout(() => lucide.createIcons(), 100);
});

// Expose navigateTo for any legacy global callers
import { navigateTo } from './utils.js';
window.__navigateTo = navigateTo;
