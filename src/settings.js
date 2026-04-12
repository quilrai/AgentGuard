import { invoke, getCurrentPort, setCurrentPort, escapeHtml } from './utils.js';

// Tauri event listener
const { listen } = window.__TAURI__.event;

// ============ Status Display ============

// Show status message in settings
function showSettingsStatus(message, type, elementId = 'settings-status') {
  const status = document.getElementById(elementId);
  if (!status) return;
  status.textContent = message;
  status.className = 'settings-status show ' + type;

  // Auto-hide after 5 seconds for success
  if (type === 'success') {
    setTimeout(() => {
      status.className = 'settings-status';
    }, 5000);
  }
}

// Update topbar port display
function updateServerStatusDisplay(port, isRestarting = false, isError = false) {
  const statusText = document.getElementById('proxy-status-text');
  const statusDot = document.getElementById('proxy-status-dot');

  if (statusText) {
    if (isError) {
      statusText.innerHTML = `Failed — <span class="proxy-status-link" id="change-port-link">change port</span>`;
      // Add click handler for the link
      const link = document.getElementById('change-port-link');
      if (link) {
        link.addEventListener('click', (e) => {
          e.stopPropagation();
          openPortPopover();
        });
      }
    } else if (isRestarting) {
      statusText.textContent = `Restarting on ${port}…`;
    } else {
      statusText.textContent = `Running at localhost:${port}`;
    }
  }

  if (statusDot) {
    statusDot.classList.remove('restarting', 'error', 'starting');
    if (isError) {
      statusDot.classList.add('error');
    } else if (isRestarting) {
      statusDot.classList.add('restarting');
    }
  }
}

// ============ Port Popover ============

function openPortPopover() {
  const popover = document.getElementById('port-popover');
  if (!popover) return;
  popover.hidden = false;
  // Defer to next frame so the transition runs
  requestAnimationFrame(() => popover.classList.add('show'));
  const input = document.getElementById('port-input');
  if (input) {
    input.value = getCurrentPort();
    setTimeout(() => {
      input.focus();
      input.select();
    }, 50);
  }
}

function closePortPopover() {
  const popover = document.getElementById('port-popover');
  if (!popover) return;
  popover.classList.remove('show');
  // Clear any inline status
  const status = document.getElementById('settings-status');
  if (status) status.className = 'settings-status';
  setTimeout(() => { popover.hidden = true; }, 180);
}

function togglePortPopover() {
  const popover = document.getElementById('port-popover');
  if (!popover) return;
  if (popover.hidden) openPortPopover();
  else closePortPopover();
}

// ============ Port Settings ============

// Load port setting from backend
export async function loadPortSetting() {
  try {
    const port = await invoke('get_port_setting');
    setCurrentPort(port);
    const portInput = document.getElementById('port-input');
    if (portInput) {
      portInput.value = port;
    }
    // Don't update status here - let loadServerStatus handle it
  } catch (error) {
    console.error('Failed to load port setting:', error);
  }
}

// Load and display the actual server status from backend
async function loadServerStatus() {
  try {
    const status = await invoke('get_server_status');
    setCurrentPort(status.port);

    if (status.status === 'running') {
      updateServerStatusDisplay(status.port, false, false);
    } else if (status.status === 'failed') {
      updateServerStatusDisplay(status.port, false, true);
    }
    // If 'starting', leave it as is (yellow dot)
  } catch (error) {
    console.error('Failed to load server status:', error);
  }
}

// Save port setting and restart server
async function savePortSetting() {
  const portInput = document.getElementById('port-input');
  const saveBtn = document.getElementById('save-port-btn');
  const port = parseInt(portInput.value, 10);
  const currentPort = getCurrentPort();

  // Validate
  if (isNaN(port) || port < 1024 || port > 65535) {
    showSettingsStatus('Port must be between 1024 and 65535', 'error');
    return;
  }

  // Skip if port hasn't changed
  if (port === currentPort) {
    showSettingsStatus('Port unchanged', 'info');
    return;
  }

  saveBtn.disabled = true;
  saveBtn.textContent = 'Saving...';
  updateServerStatusDisplay(port, true);

  try {
    // Save the port setting
    await invoke('save_port_setting', { port });
    setCurrentPort(port);

    // Restart the server
    showSettingsStatus('Restarting server...', 'info');
    await invoke('restart_server');

    // Wait for server to restart
    await new Promise(resolve => setTimeout(resolve, 1500));
    updateServerStatusDisplay(port, false);
    showSettingsStatus(`Server now running on port ${port}`, 'success');
    // Auto-close the popover shortly after success
    setTimeout(() => closePortPopover(), 900);
  } catch (error) {
    updateServerStatusDisplay(currentPort, false);
    showSettingsStatus(`Failed: ${error}`, 'error');
  } finally {
    saveBtn.disabled = false;
    saveBtn.textContent = 'Save';
  }
}

// ============ Detections (DLP) ============

// Cached detections for popover + modal
let dlpDetections = [];
// Notify subscribers when detection state changes
const detectionListeners = new Set();

export function onDetectionsChanged(fn) {
  detectionListeners.add(fn);
  return () => detectionListeners.delete(fn);
}

function notifyDetectionsChanged() {
  detectionListeners.forEach(fn => {
    try { fn(dlpDetections); } catch (e) { console.error(e); }
  });
}

export function getDetections() {
  return dlpDetections;
}

// Load detections from backend
export async function loadDetections() {
  try {
    const settings = await invoke('get_dlp_settings');
    dlpDetections = settings.patterns || [];
    notifyDetectionsChanged();
    return dlpDetections;
  } catch (error) {
    console.error('Failed to load detections:', error);
    dlpDetections = [];
    notifyDetectionsChanged();
    return dlpDetections;
  }
}

// Toggle a single detection (optimistic — revert on failure)
export async function toggleDetection(id, enabled) {
  const prev = dlpDetections.find(d => d.id === id);
  if (!prev) return false;
  const wasEnabled = prev.enabled;
  prev.enabled = enabled;
  notifyDetectionsChanged();
  try {
    await invoke('toggle_dlp_pattern', { id, enabled });
    return true;
  } catch (error) {
    console.error('Failed to toggle detection:', error);
    prev.enabled = wasEnabled;
    notifyDetectionsChanged();
    return false;
  }
}

// Set all detections to a specific enabled state
export async function setAllDetections(enabled) {
  const targets = dlpDetections.filter(d => d.enabled !== enabled);
  if (targets.length === 0) return;
  // Optimistic update
  const prevStates = targets.map(d => ({ id: d.id, prev: d.enabled }));
  targets.forEach(d => { d.enabled = enabled; });
  notifyDetectionsChanged();
  try {
    await Promise.all(targets.map(d =>
      invoke('toggle_dlp_pattern', { id: d.id, enabled })
    ));
  } catch (error) {
    console.error('Failed to update detections:', error);
    prevStates.forEach(({ id, prev }) => {
      const d = dlpDetections.find(x => x.id === id);
      if (d) d.enabled = prev;
    });
    notifyDetectionsChanged();
  }
}

// ============ Detections Modal ============

// Categorize patterns into Auth/Secrets vs Sensitive Info (PII)
// Any pattern whose name contains one of these keywords is auth/secrets.
const AUTH_SECRETS_KEYWORDS = [
  'api', 'key', 'token', 'secret', 'credential', 'password',
  'aws', 'database', 'webhook', 'jwt', 'private key', 'slack',
  'stripe', 'sendgrid', 'twilio', 'github', 'discord',
];

function categorizeDetection(name) {
  const lower = name.toLowerCase();
  if (AUTH_SECRETS_KEYWORDS.some(kw => lower.includes(kw))) return 'secrets';
  return 'pii';
}

function renderDetectionsList() {
  const secretsList = document.getElementById('detections-list-secrets');
  const piiList = document.getElementById('detections-list-pii');
  const summary = document.getElementById('detections-modal-summary');
  const secretsCount = document.getElementById('detections-secrets-count');
  const piiCount = document.getElementById('detections-pii-count');

  const total = dlpDetections.length;
  const enabled = dlpDetections.filter(d => d.enabled).length;
  if (summary) summary.textContent = `${enabled} / ${total} enabled`;

  const secrets = dlpDetections.filter(d => categorizeDetection(d.name) === 'secrets');
  const pii = dlpDetections.filter(d => categorizeDetection(d.name) === 'pii');

  if (secretsCount) secretsCount.textContent = `${secrets.filter(d => d.enabled).length}/${secrets.length}`;
  if (piiCount) piiCount.textContent = `${pii.filter(d => d.enabled).length}/${pii.length}`;

  const renderGroup = (items) => items.length === 0
    ? '<p class="detections-empty">None</p>'
    : items.map(d => `
      <label class="detections-row" data-id="${d.id}">
        <input type="checkbox" class="detections-row-check" data-id="${d.id}" ${d.enabled ? 'checked' : ''} />
        <span class="detections-row-name">${escapeHtml(d.name)}</span>
      </label>
    `).join('');

  if (secretsList) secretsList.innerHTML = renderGroup(secrets);
  if (piiList) piiList.innerHTML = renderGroup(pii);

  // Attach handlers to both lists
  document.querySelectorAll('.detections-list .detections-row-check').forEach(cb => {
    cb.addEventListener('change', async (e) => {
      e.stopPropagation();
      const id = parseInt(cb.dataset.id);
      const ok = await toggleDetection(id, cb.checked);
      if (!ok) cb.checked = !cb.checked;
    });
  });
}

function openDetectionsModal() {
  const modal = document.getElementById('detections-modal');
  if (!modal) return;
  renderDetectionsList();
  modal.classList.add('show');
}

function closeDetectionsModal() {
  const modal = document.getElementById('detections-modal');
  if (!modal) return;
  modal.classList.remove('show');
}

export function showDetectionsModal() {
  openDetectionsModal();
}

// Initialize detections modal wiring
function initDetectionsModal() {
  const modal = document.getElementById('detections-modal');
  if (!modal) return;

  const closeBtn = document.getElementById('close-detections-modal');
  if (closeBtn) closeBtn.addEventListener('click', closeDetectionsModal);

  modal.addEventListener('click', (e) => {
    if (e.target === modal) closeDetectionsModal();
  });

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && modal.classList.contains('show')) {
      closeDetectionsModal();
    }
  });

  const enableAll = document.getElementById('detections-enable-all');
  const disableAll = document.getElementById('detections-disable-all');
  if (enableAll) enableAll.addEventListener('click', () => setAllDetections(true));
  if (disableAll) disableAll.addEventListener('click', () => setAllDetections(false));

  // Re-render whenever detections change
  onDetectionsChanged(() => {
    if (modal.classList.contains('show')) renderDetectionsList();
  });

  // Initial load
  loadDetections();
}

// ============ Initialize Settings ============

export function initSettings() {
  // Server port settings
  const saveBtn = document.getElementById('save-port-btn');
  const portInput = document.getElementById('port-input');

  if (saveBtn) {
    saveBtn.addEventListener('click', savePortSetting);
  }

  if (portInput) {
    portInput.addEventListener('keypress', (e) => {
      if (e.key === 'Enter') {
        savePortSetting();
      }
    });
  }

  // Port popover open/close
  const cog = document.getElementById('proxy-status-cog');
  if (cog) {
    cog.addEventListener('click', (e) => {
      e.stopPropagation();
      togglePortPopover();
    });
  }
  const popoverClose = document.getElementById('port-popover-close');
  if (popoverClose) {
    popoverClose.addEventListener('click', closePortPopover);
  }
  // Click outside to close
  document.addEventListener('click', (e) => {
    const popover = document.getElementById('port-popover');
    if (!popover || popover.hidden) return;
    if (popover.contains(e.target)) return;
    if (e.target.closest('#proxy-status-cog')) return;
    closePortPopover();
  });
  // Escape to close
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      const popover = document.getElementById('port-popover');
      if (popover && !popover.hidden) closePortPopover();
    }
  });

  // Load settings
  loadPortSetting();

  // Listen for server events (for real-time updates after initial load)
  listen('server-started', (event) => {
    const { port } = event.payload;
    setCurrentPort(port);
    updateServerStatusDisplay(port, false, false);
  });

  listen('server-failed', (event) => {
    const { port } = event.payload;
    updateServerStatusDisplay(port, false, true);
  });

  // Query actual server status after a short delay
  // This handles the race condition where the event fired before we registered listeners
  setTimeout(() => {
    loadServerStatus();
  }, 500);

  // Initialize detections modal
  initDetectionsModal();
}
