// Tauri API
export const { invoke } = window.__TAURI__.core;

// ============ Shared State ============

// Store chart instances for cleanup
export let charts = {};

export function setCharts(newCharts) {
  charts = newCharts;
}

// Current time range filter
export let currentTimeRange = '1h';

export function setCurrentTimeRange(value) {
  currentTimeRange = value;
}

// Current backend filter
export let currentBackend = 'all';

export function setCurrentBackend(value) {
  currentBackend = value;
}

// Logs tab filters
export let logsTimeRange = '1h';

export function setLogsTimeRange(value) {
  logsTimeRange = value;
}

export let logsBackend = 'all';

export function setLogsBackend(value) {
  logsBackend = value;
}

export let logsModel = 'all';

export function setLogsModel(value) {
  logsModel = value;
}

export let logsDlpAction = 'all';

export function setLogsDlpAction(value) {
  logsDlpAction = value;
}

// Logs search
export let logsSearch = '';

export function setLogsSearch(value) {
  logsSearch = value;
}

// Logs pagination
export let logsPage = 0;

export function setLogsPage(value) {
  logsPage = value;
}

// Store logs data for modal access
export let currentLogs = [];

export function setCurrentLogs(logs) {
  currentLogs = logs;
}

// Current proxy port
let currentPort = 8008;

export function getCurrentPort() {
  return currentPort;
}

export function setCurrentPort(port) {
  currentPort = port;
}

// ============ Color Palette ============

export const colors = {
  primary: '#71D083',
  secondary: '#1FD8A4',
  warning: '#f59e0b',
  pink: '#ec4899',
  blue: '#7DD38D',
  purple: '#71D083',
};

// ============ Utility Functions ============

// Format number with K/M suffix
export function formatNumber(num) {
  if (num >= 1000000) return (num / 1000000).toFixed(1) + 'M';
  if (num >= 1000) return (num / 1000).toFixed(1) + 'K';
  return num.toLocaleString();
}

// Format latency
export function formatLatency(ms) {
  if (ms >= 1000) return (ms / 1000).toFixed(2) + 's';
  return Math.round(ms) + 'ms';
}

// Shorten model name
export function shortenModel(model) {
  const match = model.match(/claude-(\w+)-(\d+-\d+)/);
  return match ? `${match[1]}-${match[2]}` : model;
}

// Escape HTML for safe display
export function escapeHtml(text) {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

// Format timestamp for display
export function formatTimestamp(ts) {
  const date = new Date(ts);
  return date.toLocaleString();
}

// Format timestamp as relative time (e.g., "5 seconds ago")
export function formatRelativeTime(ts) {
  const now = new Date();
  const date = new Date(ts);
  const diffMs = now - date;
  const diffSecs = Math.floor(diffMs / 1000);
  const diffMins = Math.floor(diffSecs / 60);
  const diffHours = Math.floor(diffMins / 60);
  const diffDays = Math.floor(diffHours / 24);

  if (diffSecs < 60) return `${diffSecs}s ago`;
  if (diffMins < 60) return `${diffMins}m ago`;
  if (diffHours < 24) return `${diffHours}h ago`;
  return `${diffDays}d ago`;
}

// ============ Tab Switching ============

export function initTabs() {
  const navItems = document.querySelectorAll('.nav-item');
  navItems.forEach(item => {
    item.addEventListener('click', (e) => {
      // Don't switch tabs if clicking on info icon
      if (e.target.closest('.nav-info')) return;

      const tabId = item.dataset.tab;
      navItems.forEach(nav => nav.classList.remove('active'));
      item.classList.add('active');
      document.querySelectorAll('.tab-content').forEach(tab => tab.classList.remove('active'));
      document.getElementById(`${tabId}-tab`).classList.add('active');
    });
  });

  // Initialize info tooltips
  initNavInfoTooltips();
}

// ============ Nav Info Tooltips ============

function initNavInfoTooltips() {
  const infoIcons = document.querySelectorAll('.nav-info');
  let activeTooltip = null;

  infoIcons.forEach(icon => {
    icon.addEventListener('click', (e) => {
      e.stopPropagation();

      // Remove any existing tooltip
      if (activeTooltip) {
        activeTooltip.remove();
        activeTooltip = null;
      }

      // Create and show tooltip
      const tooltipText = icon.dataset.tooltip;
      const tooltip = document.createElement('div');
      tooltip.className = 'nav-info-tooltip';
      tooltip.textContent = tooltipText;
      icon.appendChild(tooltip);

      // Trigger reflow and show
      requestAnimationFrame(() => {
        tooltip.classList.add('show');
      });

      activeTooltip = tooltip;
    });
  });

  // Close tooltip when clicking elsewhere
  document.addEventListener('click', () => {
    if (activeTooltip) {
      activeTooltip.remove();
      activeTooltip = null;
    }
  });
}
