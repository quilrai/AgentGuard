// Main entry point - imports and initializes all modules

import { initRouter, onRouteChange, getCurrentRoute } from './utils.js';
import { loadDashboard, loadBackends, initBackendFilter, initTimeFilter } from './dashboard.js';
import {
  loadMessageLogs,
  loadLogsBackends,
  loadLogsModels,
  initLogsBackendFilter,
  initLogsModelFilter,
  initLogsDlpFilter,
  initLogsTimeFilter,
  initLogsSearch,
  initLogsExport
} from './logs.js';
import { initSettings, loadDetections } from './settings.js';
import { initBackends, loadPredefinedBackends, refreshGuardianHooks } from './backends.js';
import { initTokenSaving, refreshTokenSaver } from './token-saving.js';
import { initHome, loadHome, suspendHome, resumeHome } from './home.js';
import { initGarden, loadGarden } from './garden.js';

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
  initLogsModelFilter();
  initLogsDlpFilter();
  initLogsSearch();
  initLogsExport();
  loadLogsBackends();
  loadLogsModels();

  // Initialize Guardian Agent sub-modules
  initSettings();
  initBackends();

  // Initialize Token Saver
  initTokenSaving();

  // Initialize Garden
  initGarden();

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
      loadLogsModels();
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

    if (route === 'analytics') {
      loadBackends();
      loadDashboard();
    }
  });

  // Footer links
  const star = document.getElementById('starGithub');
  if (star) star.addEventListener('click', () => openUrl('https://github.com/quilrai/LLMWatcher'));
  const report = document.getElementById('reportIssue');
  if (report) report.addEventListener('click', () => openUrl('https://github.com/quilrai/LLMWatcher/issues'));

  // Re-run lucide once after dynamic icons are added
  setTimeout(() => lucide.createIcons(), 100);
});

// Expose navigateTo for any legacy global callers
import { navigateTo } from './utils.js';
window.__navigateTo = navigateTo;
