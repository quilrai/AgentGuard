import { invoke, escapeHtml } from './utils.js';

// Predefined backends loaded from the Rust side. Custom backends were
// removed when the passthrough proxy layer was deleted.
let predefinedBackends = [];

// Parse settings JSON with defaults
export function parseSettings(settingsJson) {
  try {
    const settings = JSON.parse(settingsJson || '{}');
    const tokenSaving = settings.token_saving || {};
    return {
      dlp_enabled: settings.dlp_enabled !== false, // default true
      rate_limit_requests: settings.rate_limit_requests || 0,
      rate_limit_minutes: settings.rate_limit_minutes || 1,
      max_tokens_in_a_request: settings.max_tokens_in_a_request || 0,
      action_for_max_tokens_in_a_request: settings.action_for_max_tokens_in_a_request || 'block',
      token_saving: {
        shell_compression: tokenSaving.shell_compression || false,
      }
    };
  } catch {
    return { dlp_enabled: true, rate_limit_requests: 0, rate_limit_minutes: 1, max_tokens_in_a_request: 0, action_for_max_tokens_in_a_request: 'block', token_saving: { shell_compression: false } };
  }
}

// Build settings JSON from form values
export function buildSettingsJson(dlpEnabled, rateRequests, rateMinutes, maxTokens, maxTokensAction, tokenSaving) {
  return JSON.stringify({
    dlp_enabled: dlpEnabled,
    rate_limit_requests: rateRequests,
    rate_limit_minutes: rateMinutes,
    max_tokens_in_a_request: maxTokens,
    action_for_max_tokens_in_a_request: maxTokensAction,
    token_saving: tokenSaving
  });
}

// Show status message
function showBackendsStatus(message, type) {
  // Create or find status element
  let status = document.getElementById('backends-status');
  if (!status) {
    // Backend config now lives inside the Guardian Agent page
    const cardBody = document.querySelector('#guardian-tab .card-body');
    if (cardBody) {
      status = document.createElement('div');
      status.id = 'backends-status';
      status.className = 'settings-status';
      cardBody.insertBefore(status, cardBody.firstChild);
    }
  }

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

// ============================================================================
// Predefined Backends
// ============================================================================

// Load predefined backends from backend
export async function loadPredefinedBackends() {
  try {
    predefinedBackends = await invoke('get_predefined_backends');
    renderPredefinedBackends(predefinedBackends);
  } catch (error) {
    console.error('Failed to load predefined backends:', error);
    const container = document.getElementById('predefined-backends-list');
    if (container) {
      container.innerHTML = '<p class="empty-text">Failed to load predefined backends</p>';
    }
  }
}

// Render predefined backends list
function renderPredefinedBackends(backends) {
  const container = document.getElementById('predefined-backends-list');
  if (!container) return;

  container.innerHTML = backends.map(backend => {
    const settings = parseSettings(backend.settings);
    const dlpBadge = settings.dlp_enabled
      ? '<span class="backend-setting-badge dlp-on">Protection On</span>'
      : '<span class="backend-setting-badge dlp-off">Protection Off</span>';
    const rateBadge = settings.rate_limit_requests > 0
      ? `<span class="backend-setting-badge rate-limit">${settings.rate_limit_requests}/${settings.rate_limit_minutes}min</span>`
      : '<span class="backend-setting-badge no-rate-limit">No Rate Limit</span>';
    const tokenBadge = settings.max_tokens_in_a_request > 0
      ? `<span class="backend-setting-badge token-limit">${settings.max_tokens_in_a_request} tokens (${settings.action_for_max_tokens_in_a_request})</span>`
      : '<span class="backend-setting-badge no-token-limit">No Token Limit</span>';
    const predTsFeatures = Object.entries(settings.token_saving).filter(([, v]) => v).map(([k]) => k.replace(/_/g, ' '));
    const predTokenSavingBadge = predTsFeatures.length > 0
      ? `<span class="backend-setting-badge token-saving-on">Saving: ${predTsFeatures.join(', ')}</span>`
      : '';

    return `
    <div class="backend-item predefined" data-name="${escapeHtml(backend.name)}">
      <div class="backend-info">
        <div class="backend-header">
          <span class="backend-name">${escapeHtml(backend.name)}</span>
          <span class="backend-status enabled">Pre-defined</span>
        </div>
        <div class="backend-details">
          <div class="backend-url">
            <span class="backend-label">Target:</span>
            <code>${escapeHtml(backend.base_url)}</code>
          </div>
        </div>
        <div class="backend-settings-summary">
          ${dlpBadge}
          ${rateBadge}
          ${tokenBadge}
          ${predTokenSavingBadge}
        </div>
      </div>
      <div class="backend-actions">
        <button class="dlp-pattern-edit predefined-backend-edit" data-name="${escapeHtml(backend.name)}" title="Edit settings">
          <i data-lucide="pencil"></i>
        </button>
      </div>
    </div>
  `;
  }).join('');

  // Re-initialize Lucide icons
  lucide.createIcons();

  // Add event listeners for edit buttons
  container.querySelectorAll('.predefined-backend-edit').forEach(btn => {
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      const name = btn.dataset.name;
      const backend = predefinedBackends.find(b => b.name === name);
      if (backend) {
        showPredefinedBackendModal(backend);
      }
    });
  });
}

// Show predefined backend modal for editing
function showPredefinedBackendModal(backend) {
  const modal = document.getElementById('predefined-backend-modal');
  const nameInput = document.getElementById('predefined-backend-name');
  const nameDisplay = document.getElementById('predefined-backend-name-display');
  const urlDisplay = document.getElementById('predefined-backend-url-display');
  const dlpEnabledInput = document.getElementById('predefined-backend-dlp-enabled');
  const rateRequestsInput = document.getElementById('predefined-backend-rate-requests');
  const rateMinutesInput = document.getElementById('predefined-backend-rate-minutes');
  const maxTokensInput = document.getElementById('predefined-backend-max-tokens');
  const maxTokensActionInput = document.getElementById('predefined-backend-max-tokens-action');

  const settings = parseSettings(backend.settings);

  nameInput.value = backend.name;
  nameDisplay.value = backend.name;
  urlDisplay.value = backend.base_url;
  dlpEnabledInput.checked = settings.dlp_enabled;
  rateRequestsInput.value = settings.rate_limit_requests;
  rateMinutesInput.value = settings.rate_limit_minutes;
  maxTokensInput.value = settings.max_tokens_in_a_request;
  maxTokensActionInput.value = settings.action_for_max_tokens_in_a_request;

  modal.classList.add('show');
}

// Hide predefined backend modal
function hidePredefinedBackendModal() {
  const modal = document.getElementById('predefined-backend-modal');
  modal.classList.remove('show');
}

// Save predefined backend settings
async function savePredefinedBackend() {
  const name = document.getElementById('predefined-backend-name').value;
  const dlpEnabled = document.getElementById('predefined-backend-dlp-enabled').checked;
  const rateRequests = parseInt(document.getElementById('predefined-backend-rate-requests').value) || 0;
  const rateMinutes = parseInt(document.getElementById('predefined-backend-rate-minutes').value) || 1;
  const maxTokens = parseInt(document.getElementById('predefined-backend-max-tokens').value) || 0;
  const maxTokensAction = document.getElementById('predefined-backend-max-tokens-action').value || 'block';

  // Preserve existing token saving settings
  const existingBackend = predefinedBackends.find(b => b.name === name);
  const existingSettings = existingBackend ? parseSettings(existingBackend.settings) : { token_saving: { shell_compression: false } };
  const tokenSaving = existingSettings.token_saving;

  const settings = buildSettingsJson(dlpEnabled, rateRequests, Math.max(1, rateMinutes), maxTokens, maxTokensAction, tokenSaving);

  const saveBtn = document.getElementById('save-predefined-backend-btn');
  saveBtn.disabled = true;
  saveBtn.textContent = 'Saving...';

  try {
    await invoke('update_predefined_backend', { name, settings });
    await invoke('restart_server');
    showBackendsStatus('Settings updated.', 'success');
    hidePredefinedBackendModal();
    loadPredefinedBackends();
  } catch (error) {
    alert(`Failed to save: ${error}`);
  } finally {
    saveBtn.disabled = false;
    saveBtn.textContent = 'Save';
  }
}

// Reset predefined backend to defaults
async function resetPredefinedBackend() {
  const name = document.getElementById('predefined-backend-name').value;

  if (!confirm(`Reset ${name} settings to defaults?`)) {
    return;
  }

  const resetBtn = document.getElementById('reset-predefined-backend-btn');
  resetBtn.disabled = true;
  resetBtn.textContent = 'Resetting...';

  try {
    await invoke('reset_predefined_backend', { name });
    await invoke('restart_server');
    showBackendsStatus('Settings reset.', 'success');
    hidePredefinedBackendModal();
    loadPredefinedBackends();
  } catch (error) {
    alert(`Failed to reset: ${error}`);
  } finally {
    resetBtn.disabled = false;
    resetBtn.textContent = 'Reset to defaults';
  }
}

// ============================================================================
// Init
// ============================================================================

export function initBackends() {
  // Predefined backend modal event handlers
  const predefinedModal = document.getElementById('predefined-backend-modal');
  const closePredefinedModalBtn = document.getElementById('close-predefined-backend-modal');
  const cancelPredefinedBtn = document.getElementById('cancel-predefined-backend-btn');
  const savePredefinedBtn = document.getElementById('save-predefined-backend-btn');
  const resetPredefinedBtn = document.getElementById('reset-predefined-backend-btn');

  if (closePredefinedModalBtn) closePredefinedModalBtn.addEventListener('click', hidePredefinedBackendModal);
  if (cancelPredefinedBtn) cancelPredefinedBtn.addEventListener('click', hidePredefinedBackendModal);
  if (savePredefinedBtn) savePredefinedBtn.addEventListener('click', savePredefinedBackend);
  if (resetPredefinedBtn) resetPredefinedBtn.addEventListener('click', resetPredefinedBackend);

  if (predefinedModal) {
    predefinedModal.addEventListener('click', (e) => {
      if (e.target === predefinedModal) hidePredefinedBackendModal();
    });
  }

  // Close modal on Escape key
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && predefinedModal?.classList.contains('show')) {
      hidePredefinedBackendModal();
    }
  });

  // Initial load
  loadPredefinedBackends();
}
