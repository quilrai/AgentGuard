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

// Logs view mode: 'token_saving' or 'guardian'
export let logsView = 'token_saving';

export function setLogsView(value) {
  logsView = value;
}

// Store logs data for modal access
export let currentLogs = [];

export function setCurrentLogs(logs) {
  currentLogs = logs;
}

// Current server port
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

// ============ Router ============
//
// Routes map 1:1 to <div id="{route}-tab"> elements in index.html.
// Top-bar nav items expose `home`, `analytics`, `logs`. The drill-down pages
// `guardian` and `token-saver` are reached from home cards (no top-bar entry).

const ROUTES = ['home', 'guardian', 'token-saver', 'analytics', 'logs', 'garden'];
const TOPBAR_ROUTES = new Set(['home', 'analytics', 'logs', 'garden']);

let currentRoute = 'home';
const routeListeners = new Set();

export function getCurrentRoute() {
  return currentRoute;
}

export function onRouteChange(fn) {
  routeListeners.add(fn);
  return () => routeListeners.delete(fn);
}

export function navigateTo(route) {
  if (!ROUTES.includes(route)) return;
  if (route === currentRoute) return;

  const prev = currentRoute;
  currentRoute = route;

  // Toggle tab-content visibility
  document.querySelectorAll('.tab-content').forEach(tab => tab.classList.remove('active'));
  const target = document.getElementById(`${route}-tab`);
  if (target) target.classList.add('active');

  // Topbar nav active state — only for routes that live in the topbar
  document.querySelectorAll('.topbar-nav-item').forEach(btn => {
    const r = btn.dataset.route;
    btn.classList.toggle('active', TOPBAR_ROUTES.has(route) && r === route);
  });

  // Scroll content area to top so the new page lands at the top
  const content = document.querySelector('.content');
  if (content) content.scrollTop = 0;

  // Notify listeners (so modules can lazy-load on entry)
  routeListeners.forEach(fn => {
    try { fn(route, prev); } catch (e) { console.error(e); }
  });
}

export function initRouter() {
  // Topbar nav
  document.querySelectorAll('.topbar-nav-item').forEach(btn => {
    btn.addEventListener('click', () => navigateTo(btn.dataset.route));
  });

  // Any element with [data-route] (home cards, back links, etc.)
  document.addEventListener('click', (e) => {
    const el = e.target.closest('[data-route]');
    if (!el) return;
    if (el.classList.contains('topbar-nav-item')) return; // already handled
    e.preventDefault();
    navigateTo(el.dataset.route);
  });
}
