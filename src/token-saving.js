import { invoke, escapeHtml } from './utils.js';
import { parseSettings, buildSettingsJson } from './backends.js';

let allBackends = []; // { backend }

// ============================================================================
// Shell Compression Hooks
// ============================================================================

async function checkAllHookStatuses() {
  await Promise.all([
    checkHookStatus('claude', 'check_compression_hook_claude'),
    checkHookStatus('codex', 'check_compression_hook_codex'),
  ]);
}

async function checkHookStatus(agent, checkCommand) {
  const statusEl = document.getElementById(`sc-${agent}-status`);
  const btn = document.getElementById(`sc-${agent}-btn`);
  if (!statusEl || !btn) return;

  try {
    const installed = await invoke(checkCommand);
    statusEl.textContent = installed ? 'Installed' : 'Not installed';
    statusEl.className = `sc-hook-status ${installed ? 'installed' : 'not-installed'}`;
    btn.textContent = installed ? 'Remove' : 'Install';
    btn.className = `btn btn-sm ${installed ? 'btn-danger' : 'btn-primary'}`;
    btn.disabled = false;
  } catch (error) {
    statusEl.textContent = 'Error';
    statusEl.className = 'sc-hook-status error';
    btn.disabled = true;
    console.error(`Failed to check ${agent} hook status:`, error);
  }
}

function setupHookButtons() {
  setupHookButton('claude', 'install_compression_hook_claude', 'uninstall_compression_hook_claude', 'check_compression_hook_claude');
  setupHookButton('codex', 'install_compression_hook_codex', 'uninstall_compression_hook_codex', 'check_compression_hook_codex');
}

function setupHookButton(agent, installCmd, uninstallCmd, checkCmd) {
  const btn = document.getElementById(`sc-${agent}-btn`);
  if (!btn) return;

  btn.addEventListener('click', async () => {
    const isInstalled = btn.textContent === 'Remove';
    btn.disabled = true;
    btn.textContent = isInstalled ? 'Removing...' : 'Installing...';

    try {
      if (isInstalled) {
        await invoke(uninstallCmd);
      } else {
        await invoke(installCmd);
      }
      await checkHookStatus(agent, checkCmd);
    } catch (error) {
      console.error(`Failed to ${isInstalled ? 'uninstall' : 'install'} ${agent} hook:`, error);
      alert(`Failed: ${error}`);
      await checkHookStatus(agent, checkCmd);
    }
  });
}

// ============================================================================
// Shell Compression (per-backend)
// ============================================================================

async function loadAllBackends() {
  try {
    const predefined = await invoke('get_predefined_backends');
    allBackends = predefined
      .filter(b => b.name !== 'cursor-hooks')
      .map(b => ({ backend: b }));
    renderTokenSavingList();
  } catch (error) {
    console.error('Failed to load backends for token saving:', error);
    const container = document.getElementById('token-saving-backends-list');
    if (container) {
      container.innerHTML = '<p class="empty-text">Failed to load backends</p>';
    }
  }
}

function renderTokenSavingList() {
  const container = document.getElementById('token-saving-backends-list');
  if (!container) return;

  if (allBackends.length === 0) {
    container.innerHTML = '<p class="empty-text">No backends configured</p>';
    return;
  }

  container.innerHTML = allBackends.map(({ backend }) => {
    const settings = parseSettings(backend.settings);
    const isEnabled = settings.token_saving.shell_compression;
    const typeBadge = '<span class="backend-status enabled">Pre-defined</span>';

    return `
    <div class="token-saving-item">
      <div class="token-saving-info">
        <span class="backend-name">${escapeHtml(backend.name)}</span>
        ${typeBadge}
      </div>
      <div class="setting-toggle-row" style="padding: 0;">
        <label class="toggle-switch">
          <input type="checkbox" class="ts-toggle" data-name="${escapeHtml(backend.name)}" ${isEnabled ? 'checked' : ''} />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>`;
  }).join('');

  // Add toggle event listeners
  container.querySelectorAll('.ts-toggle').forEach(toggle => {
    toggle.addEventListener('change', async () => {
      const name = toggle.dataset.name;
      const enabled = toggle.checked;
      toggle.disabled = true;

      try {
        const entry = allBackends.find(e => e.backend.name === name);
        if (!entry) return;

        const settings = parseSettings(entry.backend.settings);
        settings.token_saving.shell_compression = enabled;
        const newSettingsJson = buildSettingsJson(
          settings.dlp_enabled,
          settings.max_tokens_in_a_request,
          settings.action_for_max_tokens_in_a_request,
          settings.token_saving
        );

        await invoke('update_predefined_backend', { name, settings: newSettingsJson });
        await invoke('restart_server');
        // Update local state
        entry.backend.settings = newSettingsJson;
      } catch (error) {
        console.error('Failed to update token saving:', error);
        toggle.checked = !enabled; // revert
        alert(`Failed to update: ${error}`);
      } finally {
        toggle.disabled = false;
      }
    });
  });
}

// ============================================================================
// Init
// ============================================================================

export function initTokenSaving() {
  // Setup hook buttons
  setupHookButtons();

  checkAllHookStatuses();
  loadAllBackends();
}

// Refresh helper called by the router on route entry
export function refreshTokenSaver() {
  checkAllHookStatuses();
  loadAllBackends();
}
