// Main entry point - imports and initializes all modules

import { initTabs } from './utils.js';
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
import { initSettings } from './settings.js';
import { initBackends } from './backends.js';
import { initTokenSaving } from './token-saving.js';
import { initHowTo } from './howto.js';

const { openUrl } = window.__TAURI__.opener;

// Initialize app
window.addEventListener('DOMContentLoaded', () => {
  // Initialize Lucide icons
  lucide.createIcons();

  // Initialize navigation
  initTabs();

  // Initialize dashboard
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

  // Initialize settings
  initSettings();

  // Initialize custom backends
  initBackends();

  // Initialize token saving
  initTokenSaving();

  // Initialize how-to
  initHowTo();

  // Refresh buttons - also refresh backends list
  document.getElementById('refresh-btn').addEventListener('click', () => {
    loadBackends();
    loadDashboard();
  });
  document.getElementById('logs-refresh-btn').addEventListener('click', () => {
    loadLogsBackends();
    loadMessageLogs();
  });

  // Load logs when tab is clicked
  document.querySelector('[data-tab="logs"]').addEventListener('click', () => {
    loadMessageLogs();
    loadLogsModels();
  });

  // GitHub links
  document.getElementById('starGithub').addEventListener('click', () => {
    openUrl('https://github.com/quilrai/LLMWatcher');
  });
  document.getElementById('reportIssue').addEventListener('click', () => {
    openUrl('https://github.com/quilrai/LLMWatcher/issues');
  });
});
