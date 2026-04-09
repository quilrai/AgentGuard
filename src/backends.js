import { invoke, escapeHtml } from './utils.js';

let predefinedBackends = [];

// Map agent card IDs to backend names in the database
const AGENT_TO_BACKEND = {
  claude: 'claude',
  codex: 'codex',
  cursor: 'cursor-hooks',
};

// Friendly display names per agent
const AGENT_NAMES = {
  claude: 'Claude Code',
  codex: 'Codex',
  cursor: 'Cursor',
};

// Agent hook commands
const AGENTS = [
  { id: 'claude', checkCmd: 'check_claude_hooks_installed', installCmd: 'install_claude_hooks', uninstallCmd: 'uninstall_claude_hooks' },
  { id: 'codex',  checkCmd: 'check_codex_hooks_installed',  installCmd: 'install_codex_hooks',  uninstallCmd: 'uninstall_codex_hooks' },
  { id: 'cursor', checkCmd: 'check_cursor_hooks_installed', installCmd: 'install_cursor_hooks', uninstallCmd: 'uninstall_cursor_hooks' },
];

// ============================================================================
// Settings helpers
// ============================================================================

export function parseSettings(settingsJson) {
  try {
    const settings = JSON.parse(settingsJson || '{}');
    const tokenSaving = settings.token_saving || {};
    return {
      dlp_enabled: settings.dlp_enabled !== false,
      max_tokens_in_a_request: settings.max_tokens_in_a_request || 0,
      action_for_max_tokens_in_a_request: settings.action_for_max_tokens_in_a_request || 'block',
      token_saving: { shell_compression: tokenSaving.shell_compression || false },
    };
  } catch {
    return { dlp_enabled: true, max_tokens_in_a_request: 0, action_for_max_tokens_in_a_request: 'block', token_saving: { shell_compression: false } };
  }
}

export function buildSettingsJson(dlpEnabled, maxTokens, maxTokensAction, tokenSaving) {
  return JSON.stringify({
    dlp_enabled: dlpEnabled,
    max_tokens_in_a_request: maxTokens,
    action_for_max_tokens_in_a_request: maxTokensAction,
    token_saving: tokenSaving,
  });
}

// ============================================================================
// Hook status & install/remove
// ============================================================================

function setPillState(agentId, state) {
  const pill = document.getElementById(`guardian-${agentId}-pill`);
  if (!pill) return;
  pill.classList.remove('pill-active', 'pill-inactive', 'pill-error');
  pill.classList.add(`pill-${state}`);
  const dot = pill.querySelector('.status-dot');
  if (dot) {
    dot.classList.remove('active', 'inactive', 'error');
    dot.classList.add(state);
  }
}

function setCardState(agentId, installed) {
  const card = document.querySelector(`.agent-card[data-agent="${agentId}"]`);
  if (!card) return;
  card.classList.toggle('is-installed', !!installed);
}

async function refreshAgentStatus(agent) {
  const statusEl = document.getElementById(`guardian-${agent.id}-status`);
  const btn = document.getElementById(`guardian-${agent.id}-btn`);
  const pill = document.getElementById(`guardian-${agent.id}-pill`);
  const name = AGENT_NAMES[agent.id] || agent.id;
  if (!statusEl || !btn) return;
  try {
    const installed = await invoke(agent.checkCmd);

    // Only show pill when installed — hide entirely otherwise
    if (pill) {
      if (installed) {
        pill.style.display = '';
        statusEl.textContent = 'Installed';
        setPillState(agent.id, 'active');
      } else {
        pill.style.display = 'none';
      }
    }

    setCardState(agent.id, installed);
    btn.textContent = installed ? `Remove from ${name}` : '+ Add';
    btn.className = `agent-hook-btn ${installed ? 'is-remove' : 'needs-setup'}`;
    btn.disabled = false;
  } catch (error) {
    if (pill) {
      pill.style.display = '';
      statusEl.textContent = 'Error';
      setPillState(agent.id, 'error');
    }
    setCardState(agent.id, false);
    btn.disabled = true;
    console.error(`Failed to check ${agent.id} hook status:`, error);
  }
}

function attachAgentHandler(agent) {
  const btn = document.getElementById(`guardian-${agent.id}-btn`);
  const card = document.querySelector(`.agent-card[data-agent="${agent.id}"]`);
  if (!btn) return;

  async function doToggle() {
    const isInstalled = btn.classList.contains('is-remove');
    btn.disabled = true;
    btn.textContent = isInstalled ? 'Removing…' : 'Adding…';
    try {
      await invoke(isInstalled ? agent.uninstallCmd : agent.installCmd);
    } catch (error) {
      console.error(`Failed to ${isInstalled ? 'remove' : 'add'} ${agent.id} hooks:`, error);
      alert(`Failed: ${error}`);
    } finally {
      await refreshAgentStatus(agent);
      showSetupCallout();
    }
  }

  btn.addEventListener('click', (e) => {
    e.stopPropagation();
    doToggle();
  });

  // Make the whole card clickable for install when not-installed
  if (card) {
    card.addEventListener('click', () => {
      if (!card.classList.contains('is-installed') && !btn.disabled) {
        doToggle();
      }
    });
  }
}

export async function refreshGuardianHooks() {
  await Promise.all(AGENTS.map(refreshAgentStatus));
  document.querySelector('.agent-setup-grid')?.classList.add('status-checked');
  showSetupCallout();
}

// Setup callout — no longer needed; the "Add guardian to" label replaces it
function showSetupCallout() {}

// ============================================================================
// Guardian tabs
// ============================================================================

function initGuardianTabs() {
  const tabs = document.querySelectorAll('#guardian-tab .page-tab');
  const panels = document.querySelectorAll('#guardian-tab .guardian-tab-panel');
  if (!tabs.length || !panels.length) return;
  tabs.forEach(tab => {
    tab.addEventListener('click', () => {
      const target = tab.dataset.guardianTab;
      tabs.forEach(t => {
        const active = t === tab;
        t.classList.toggle('active', active);
        t.setAttribute('aria-selected', active ? 'true' : 'false');
      });
      panels.forEach(p => {
        p.classList.toggle('active', p.id === `guardian-panel-${target}`);
      });
    });
  });
}

// ============================================================================
// Backend settings — render into agent cards
// ============================================================================

export async function loadPredefinedBackends() {
  try {
    predefinedBackends = await invoke('get_predefined_backends');
    renderSettingsIntoCards();
  } catch (error) {
    console.error('Failed to load predefined backends:', error);
  }
}

function renderSettingsIntoCards() {
  for (const [agentId, backendName] of Object.entries(AGENT_TO_BACKEND)) {
    const container = document.getElementById(`agent-settings-${agentId}`);
    if (!container) continue;

    const backend = predefinedBackends.find(b => b.name === backendName);
    if (!backend) continue;

    const settings = parseSettings(backend.settings);

    const dlpOn = settings.dlp_enabled;
    const tokenVal = settings.max_tokens_in_a_request;
    const tokenAction = settings.action_for_max_tokens_in_a_request;

    const tokenDisplay = tokenVal > 0
      ? `${tokenVal.toLocaleString()} \u00b7 ${tokenAction === 'block' ? 'Block' : 'Notify'}`
      : 'Not set';
    const blockSel = tokenAction === 'block' ? 'selected' : '';
    const notifySel = tokenAction === 'notify' ? 'selected' : '';
    const dlpNudge = !dlpOn ? 'needs-setup' : '';
    const tokenNudge = tokenVal === 0 ? 'needs-setup' : '';

    container.innerHTML = `
      <div class="agent-tiles">
        <button class="agent-tile ${dlpOn ? 'tile-active' : 'tile-muted'} ${dlpNudge}" data-tile="dlp" type="button">
          <span class="agent-tile-dot"></span>
          <span class="agent-tile-body">
            <span class="agent-tile-label">Protection</span>
            <span class="agent-tile-val">${dlpOn ? 'Active' : 'Off'}</span>
          </span>
        </button>
        <button class="agent-tile ${tokenNudge}" data-tile="token" type="button">
          <i data-lucide="coins"></i>
          <span class="agent-tile-body">
            <span class="agent-tile-label">Token Cap</span>
            <span class="agent-tile-val">${tokenDisplay}</span>
          </span>
        </button>
      </div>

      <div class="agent-editor" data-editor="dlp">
        <div class="agent-editor-panel">
          <div class="agent-editor-head">
            <i data-lucide="shield-check"></i>
            <span>Data Protection</span>
          </div>
          <p class="agent-editor-hint">Catches API keys, tokens, and secrets before they leave your machine. Keeps your credentials safe while coding with AI.</p>
          <div class="agent-editor-row-toggle">
            <span class="agent-editor-status">${dlpOn ? 'Scanning active' : 'Scanning disabled'}</span>
            <label class="toggle-switch toggle-sm">
              <input type="checkbox" class="backend-dlp-toggle" ${dlpOn ? 'checked' : ''} />
              <span class="toggle-slider"></span>
            </label>
          </div>
        </div>
      </div>

      <div class="agent-editor" data-editor="token">
        <div class="agent-editor-panel">
          <div class="agent-editor-head">
            <i data-lucide="coins"></i>
            <span>Token Cap</span>
          </div>
          <p class="agent-editor-hint">Prevents runaway prompts from burning your budget. <strong>200,000</strong> is a safe default for most workflows.</p>
          <div class="agent-editor-field">
            <label class="agent-editor-flabel">Max tokens</label>
            <input type="number" class="agent-editor-input backend-max-tokens" min="0" value="${tokenVal}" placeholder="200000" />
          </div>
          <div class="agent-editor-field">
            <label class="agent-editor-flabel">When exceeded</label>
            <select class="agent-editor-select backend-max-tokens-action">
              <option value="block" ${blockSel}>Block request</option>
              <option value="notify" ${notifySel}>Notify only</option>
            </select>
          </div>
          <div class="agent-editor-foot">
            <button class="agent-editor-btn agent-editor-btn--ghost backend-reset-btn" type="button">Reset</button>
            <button class="agent-editor-btn agent-editor-btn--accent backend-save-btn" type="button">Save</button>
          </div>
        </div>
      </div>
    `;

    lucide.createIcons({ nodes: [container] });

    // Tile click handlers — toggle corresponding editor
    container.querySelectorAll('.agent-tile').forEach(tile => {
      tile.addEventListener('click', (e) => {
        e.stopPropagation();
        const setting = tile.dataset.tile;
        const editor = container.querySelector(`.agent-editor[data-editor="${setting}"]`);
        const isOpen = editor.classList.contains('open');

        // Close all editors across all cards
        document.querySelectorAll('.agent-editor.open').forEach(ed => ed.classList.remove('open'));
        document.querySelectorAll('.agent-tile.tile-expanded').forEach(t => t.classList.remove('tile-expanded'));

        if (!isOpen) {
          editor.classList.add('open');
          tile.classList.add('tile-expanded');
        }
      });
    });

    // DLP toggle — save immediately
    const dlpToggle = container.querySelector('.backend-dlp-toggle');
    dlpToggle.addEventListener('click', (e) => e.stopPropagation());
    dlpToggle.addEventListener('change', () => saveDlpToggle(container, backendName));

    // Prevent clicks inside editor panels from bubbling
    container.querySelectorAll('.agent-editor-panel').forEach(panel => {
      panel.addEventListener('click', (e) => e.stopPropagation());
    });

    // Save limits
    container.querySelector('.backend-save-btn').addEventListener('click', (e) => {
      e.stopPropagation();
      saveLimits(container, backendName);
    });

    // Reset
    container.querySelector('.backend-reset-btn').addEventListener('click', (e) => {
      e.stopPropagation();
      resetBackend(container, backendName);
    });
  }
}

// Close editors on outside click
document.addEventListener('click', () => {
  document.querySelectorAll('.agent-editor.open').forEach(ed => ed.classList.remove('open'));
  document.querySelectorAll('.agent-tile.tile-expanded').forEach(t => t.classList.remove('tile-expanded'));
});

// ============================================================================
// Save / Reset
// ============================================================================

async function saveDlpToggle(container, backendName) {
  const dlpEnabled = container.querySelector('.backend-dlp-toggle').checked;
  const existing = predefinedBackends.find(b => b.name === backendName);
  const s = existing ? parseSettings(existing.settings) : parseSettings('{}');

  const settings = buildSettingsJson(dlpEnabled, s.max_tokens_in_a_request, s.action_for_max_tokens_in_a_request, s.token_saving);

  try {
    await invoke('update_predefined_backend', { name: backendName, settings });
    await invoke('restart_server');
    loadPredefinedBackends();
  } catch (error) {
    alert(`Failed to save: ${error}`);
  }
}

async function saveLimits(container, backendName) {
  const maxTokens = parseInt(container.querySelector('.backend-max-tokens').value) || 0;
  const maxTokensAction = container.querySelector('.backend-max-tokens-action').value || 'block';

  const existing = predefinedBackends.find(b => b.name === backendName);
  const s = existing ? parseSettings(existing.settings) : parseSettings('{}');

  const settings = buildSettingsJson(s.dlp_enabled, maxTokens, maxTokensAction, s.token_saving);

  const saveBtn = container.querySelector('.backend-save-btn');
  saveBtn.disabled = true;
  saveBtn.textContent = 'Saving…';

  try {
    await invoke('update_predefined_backend', { name: backendName, settings });
    await invoke('restart_server');
    container.querySelectorAll('.agent-editor.open').forEach(ed => ed.classList.remove('open'));
    container.querySelectorAll('.agent-tile.tile-expanded').forEach(t => t.classList.remove('tile-expanded'));
    loadPredefinedBackends();
  } catch (error) {
    alert(`Failed to save: ${error}`);
  } finally {
    saveBtn.disabled = false;
    saveBtn.textContent = 'Save';
  }
}

async function resetBackend(container, backendName) {
  if (!confirm('Reset settings to defaults?')) return;
  try {
    await invoke('reset_predefined_backend', { name: backendName });
    await invoke('restart_server');
    container.querySelectorAll('.agent-editor.open').forEach(ed => ed.classList.remove('open'));
    container.querySelectorAll('.agent-tile.tile-expanded').forEach(t => t.classList.remove('tile-expanded'));
    loadPredefinedBackends();
  } catch (error) {
    alert(`Failed to reset: ${error}`);
  }
}

// ============================================================================
// Init (called once from main.js)
// ============================================================================

export function initBackends() {
  AGENTS.forEach(attachAgentHandler);
  initGuardianTabs();
  refreshGuardianHooks();
  loadPredefinedBackends();
}
