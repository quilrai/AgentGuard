import { invoke, getCurrentPort, escapeHtml } from './utils.js';

// Store backends for editing
let customBackends = [];
let predefinedBackends = [];

// Parse settings JSON with defaults
function parseSettings(settingsJson) {
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
        context_trimming: tokenSaving.context_trimming || false,
      }
    };
  } catch {
    return { dlp_enabled: true, rate_limit_requests: 0, rate_limit_minutes: 1, max_tokens_in_a_request: 0, action_for_max_tokens_in_a_request: 'block', token_saving: { context_trimming: false } };
  }
}

// Build settings JSON from form values
function buildSettingsJson(dlpEnabled, rateRequests, rateMinutes, maxTokens, maxTokensAction, tokenSaving) {
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
    const cardBody = document.querySelector('#backends-tab .card-body');
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

// Load custom backends from backend
export async function loadCustomBackends() {
  try {
    customBackends = await invoke('get_custom_backends');
    renderBackends(customBackends);
  } catch (error) {
    console.error('Failed to load custom backends:', error);
    const container = document.getElementById('backends-list');
    if (container) {
      container.innerHTML = '<p class="empty-text">Failed to load backends</p>';
    }
  }
}

// Render backends list
function renderBackends(backends) {
  const container = document.getElementById('backends-list');
  if (!container) return;

  const port = getCurrentPort();

  if (backends.length === 0) {
    container.innerHTML = `
      <div class="backends-empty">
        <p class="empty-text">No custom backends configured</p>
        <p class="empty-hint">Click "Add Backend" to add an OpenAI-compatible API endpoint.</p>
      </div>
    `;
    return;
  }

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
    const tsFeatures = Object.entries(settings.token_saving).filter(([, v]) => v).map(([k]) => k.replace(/_/g, ' '));
    const tokenSavingBadge = tsFeatures.length > 0
      ? `<span class="backend-setting-badge token-saving-on">Saving: ${tsFeatures.join(', ')}</span>`
      : '';

    return `
    <div class="backend-item ${backend.enabled ? '' : 'disabled'}" data-id="${backend.id}">
      <div class="backend-info">
        <div class="backend-header">
          <input type="checkbox" class="dlp-checkbox backend-toggle" data-id="${backend.id}" ${backend.enabled ? 'checked' : ''} />
          <span class="backend-name">${escapeHtml(backend.name)}</span>
          <span class="backend-status ${backend.enabled ? 'enabled' : 'disabled'}">${backend.enabled ? 'Active' : 'Disabled'}</span>
        </div>
        <div class="backend-details">
          <div class="backend-url">
            <span class="backend-label">Proxy URL:</span>
            <code>http://localhost:${port}/${escapeHtml(backend.name)}</code>
          </div>
          <div class="backend-url">
            <span class="backend-label">Target:</span>
            <code>${escapeHtml(backend.base_url)}</code>
          </div>
        </div>
        <div class="backend-settings-summary">
          ${dlpBadge}
          ${rateBadge}
          ${tokenBadge}
          ${tokenSavingBadge}
        </div>
      </div>
      <div class="backend-actions">
        <button class="dlp-pattern-edit backend-edit" data-id="${backend.id}" title="Edit backend">
          <i data-lucide="pencil"></i>
        </button>
        <button class="dlp-pattern-delete backend-delete" data-id="${backend.id}" title="Delete backend">
          <i data-lucide="trash-2"></i>
        </button>
      </div>
    </div>
  `;
  }).join('');

  // Re-initialize Lucide icons
  lucide.createIcons();

  // Add event listeners for toggles
  container.querySelectorAll('.backend-toggle').forEach(checkbox => {
    checkbox.addEventListener('change', async (e) => {
      e.stopPropagation();
      const id = parseInt(checkbox.dataset.id);
      try {
        await invoke('toggle_custom_backend', { id, enabled: checkbox.checked });
        await invoke('restart_proxy');
        showBackendsStatus('Backend updated and gateway restarted.', 'success');
        loadCustomBackends();
      } catch (error) {
        console.error('Failed to toggle backend:', error);
        checkbox.checked = !checkbox.checked;
        showBackendsStatus(`Failed to toggle: ${error}`, 'error');
      }
    });
  });

  // Add event listeners for edit buttons
  container.querySelectorAll('.backend-edit').forEach(btn => {
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      const id = parseInt(btn.dataset.id);
      const backend = customBackends.find(b => b.id === id);
      if (backend) {
        showBackendModal(backend);
      }
    });
  });

  // Add event listeners for delete buttons
  container.querySelectorAll('.backend-delete').forEach(btn => {
    btn.addEventListener('click', async (e) => {
      e.stopPropagation();
      const id = parseInt(btn.dataset.id);
      const backend = customBackends.find(b => b.id === id);
      if (confirm(`Delete backend "${backend?.name}"?`)) {
        try {
          await invoke('delete_custom_backend', { id });
          await invoke('restart_proxy');
          showBackendsStatus('Backend deleted and gateway restarted.', 'success');
          loadCustomBackends();
        } catch (error) {
          showBackendsStatus(`Failed to delete: ${error}`, 'error');
        }
      }
    });
  });
}

// Show backend modal (add or edit)
function showBackendModal(backend = null) {
  const modal = document.getElementById('backend-modal');
  const title = document.getElementById('backend-modal-title');
  const nameInput = document.getElementById('backend-name');
  const urlInput = document.getElementById('backend-url');
  const dlpEnabledInput = document.getElementById('backend-dlp-enabled');
  const rateRequestsInput = document.getElementById('backend-rate-requests');
  const rateMinutesInput = document.getElementById('backend-rate-minutes');
  const maxTokensInput = document.getElementById('backend-max-tokens');
  const maxTokensActionInput = document.getElementById('backend-max-tokens-action');

  // Set title
  title.textContent = backend ? 'Edit Backend' : 'Add Backend';

  // Parse existing settings or use defaults
  const settings = backend ? parseSettings(backend.settings) : { dlp_enabled: true, rate_limit_requests: 0, rate_limit_minutes: 1, max_tokens_in_a_request: 0, action_for_max_tokens_in_a_request: 'block', token_saving: { context_trimming: false } };

  // Reset/populate form
  document.getElementById('backend-id').value = backend ? backend.id : '';
  nameInput.value = backend ? backend.name : '';
  urlInput.value = backend ? backend.base_url : '';
  dlpEnabledInput.checked = settings.dlp_enabled;
  rateRequestsInput.value = settings.rate_limit_requests;
  rateMinutesInput.value = settings.rate_limit_minutes;
  maxTokensInput.value = settings.max_tokens_in_a_request;
  maxTokensActionInput.value = settings.action_for_max_tokens_in_a_request;

  // Token saving settings
  document.getElementById('backend-ts-context-trimming').checked = settings.token_saving.context_trimming;

  // If editing, disable name field (changing name not allowed)
  nameInput.disabled = !!backend;

  modal.classList.add('show');

  // Focus appropriate input
  setTimeout(() => {
    if (backend) {
      urlInput.focus();
    } else {
      nameInput.focus();
    }
  }, 100);
}

// Hide backend modal
function hideBackendModal() {
  const modal = document.getElementById('backend-modal');
  modal.classList.remove('show');
  document.getElementById('backend-name').disabled = false;
}

// Save backend (add or update)
async function saveBackend() {
  const id = document.getElementById('backend-id').value;
  const name = document.getElementById('backend-name').value.trim();
  const baseUrl = document.getElementById('backend-url').value.trim();
  const dlpEnabled = document.getElementById('backend-dlp-enabled').checked;
  const rateRequests = parseInt(document.getElementById('backend-rate-requests').value) || 0;
  const rateMinutes = parseInt(document.getElementById('backend-rate-minutes').value) || 1;
  const maxTokens = parseInt(document.getElementById('backend-max-tokens').value) || 0;
  const maxTokensAction = document.getElementById('backend-max-tokens-action').value || 'block';
  const tokenSaving = {
    context_trimming: document.getElementById('backend-ts-context-trimming').checked,
  };

  // Build settings JSON
  const settings = buildSettingsJson(dlpEnabled, rateRequests, Math.max(1, rateMinutes), maxTokens, maxTokensAction, tokenSaving);

  // Validation
  if (!name) {
    alert('Please enter a backend name');
    return;
  }

  if (!baseUrl) {
    alert('Please enter a base URL');
    return;
  }

  const saveBtn = document.getElementById('save-backend-btn');
  saveBtn.disabled = true;
  saveBtn.textContent = 'Saving...';

  try {
    if (id) {
      // Update existing backend
      await invoke('update_custom_backend', {
        id: parseInt(id),
        name,
        baseUrl,
        settings
      });
    } else {
      // Add new backend
      await invoke('add_custom_backend', {
        name,
        baseUrl,
        settings
      });
    }
    // Restart proxy to apply changes
    await invoke('restart_proxy');
    showBackendsStatus(id ? 'Backend updated and gateway restarted.' : 'Backend added and gateway restarted.', 'success');
    hideBackendModal();
    loadCustomBackends();
  } catch (error) {
    alert(`Failed to save: ${error}`);
  } finally {
    saveBtn.disabled = false;
    saveBtn.textContent = 'Save and Restart Gateway';
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

  const port = getCurrentPort();

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

    // cursor-hooks doesn't have a proxy URL
    const proxyUrlHtml = backend.name === 'cursor-hooks'
      ? ''
      : `<div class="backend-url">
          <span class="backend-label">Proxy URL:</span>
          <code>http://localhost:${port}/${escapeHtml(backend.name)}</code>
        </div>`;

    return `
    <div class="backend-item predefined" data-name="${escapeHtml(backend.name)}">
      <div class="backend-info">
        <div class="backend-header">
          <span class="backend-name">${escapeHtml(backend.name)}</span>
          <span class="backend-status enabled">Pre-defined</span>
        </div>
        <div class="backend-details">
          ${proxyUrlHtml}
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

  // Token saving settings - hide for cursor-hooks since it doesn't use them
  const predefinedTsGroup = document.getElementById('predefined-backend-ts-group');
  if (backend.name === 'cursor-hooks') {
    predefinedTsGroup.style.display = 'none';
  } else {
    predefinedTsGroup.style.display = '';
    document.getElementById('predefined-backend-ts-context-trimming').checked = settings.token_saving.context_trimming;
  }

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
  const tokenSaving = {
    context_trimming: document.getElementById('predefined-backend-ts-context-trimming').checked,
  };

  const settings = buildSettingsJson(dlpEnabled, rateRequests, Math.max(1, rateMinutes), maxTokens, maxTokensAction, tokenSaving);

  const saveBtn = document.getElementById('save-predefined-backend-btn');
  saveBtn.disabled = true;
  saveBtn.textContent = 'Saving...';

  try {
    await invoke('update_predefined_backend', { name, settings });
    // Restart proxy to apply changes
    await invoke('restart_proxy');
    showBackendsStatus('Settings updated and gateway restarted.', 'success');
    hidePredefinedBackendModal();
    loadPredefinedBackends();
  } catch (error) {
    alert(`Failed to save: ${error}`);
  } finally {
    saveBtn.disabled = false;
    saveBtn.textContent = 'Save and Restart Gateway';
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
    // Restart proxy to apply changes
    await invoke('restart_proxy');
    showBackendsStatus('Settings reset and gateway restarted.', 'success');
    hidePredefinedBackendModal();
    loadPredefinedBackends();
  } catch (error) {
    alert(`Failed to reset: ${error}`);
  } finally {
    resetBtn.disabled = false;
    resetBtn.textContent = 'Reset and Restart Gateway';
  }
}

// ============================================================================
// Custom Backends (existing code)
// ============================================================================

// Initialize backends tab
export function initBackends() {
  // Add backend button
  const addBackendBtn = document.getElementById('add-backend-btn');
  if (addBackendBtn) {
    addBackendBtn.addEventListener('click', () => showBackendModal());
  }

  // Modal close buttons
  const closeModalBtn = document.getElementById('close-backend-modal');
  const cancelBtn = document.getElementById('cancel-backend-btn');
  if (closeModalBtn) closeModalBtn.addEventListener('click', hideBackendModal);
  if (cancelBtn) cancelBtn.addEventListener('click', hideBackendModal);

  // Modal save button
  const saveBackendBtn = document.getElementById('save-backend-btn');
  if (saveBackendBtn) {
    saveBackendBtn.addEventListener('click', saveBackend);
  }

  // Close modal on backdrop click
  const modal = document.getElementById('backend-modal');
  if (modal) {
    modal.addEventListener('click', (e) => {
      if (e.target === modal) hideBackendModal();
    });
  }

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

  // Close modals on Escape key
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      if (modal?.classList.contains('show')) {
        hideBackendModal();
      }
      if (predefinedModal?.classList.contains('show')) {
        hidePredefinedBackendModal();
      }
    }
  });

  // Load backends when tab is clicked
  document.querySelector('[data-tab="backends"]')?.addEventListener('click', () => {
    loadPredefinedBackends();
    loadCustomBackends();
  });

  // Initial load
  loadPredefinedBackends();
  loadCustomBackends();
}
