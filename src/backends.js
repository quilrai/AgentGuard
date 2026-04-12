import { invoke, escapeHtml } from './utils.js';
import {
  loadDetections,
  getDetections,
  onDetectionsChanged,
  setAllDetections,
  showDetectionsModal,
} from './settings.js';

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
    const depProt = settings.dependency_protection || {};
    return {
      dlp_enabled: settings.dlp_enabled !== false,
      max_tokens_in_a_request: settings.max_tokens_in_a_request || 0,
      action_for_max_tokens_in_a_request: settings.action_for_max_tokens_in_a_request || 'block',
      token_saving: {
        shell_compression: tokenSaving.shell_compression || false,
        ctx_read: tokenSaving.ctx_read || false,
        search_compressor: tokenSaving.search_compressor || false,
        diff_compressor: tokenSaving.diff_compressor || false,
        tool_crusher: tokenSaving.tool_crusher || false,
        compression_cache: tokenSaving.compression_cache || false,
      },
      dependency_protection: {
        inform_updated_packages: depProt.inform_updated_packages || false,
        block_malicious_packages: depProt.block_malicious_packages || false,
      },
    };
  } catch {
    return { dlp_enabled: true, max_tokens_in_a_request: 0, action_for_max_tokens_in_a_request: 'block', token_saving: { shell_compression: false, ctx_read: false, search_compressor: false, diff_compressor: false, tool_crusher: false, compression_cache: false }, dependency_protection: { inform_updated_packages: false, block_malicious_packages: false } };
  }
}

export function buildSettingsJson(dlpEnabled, maxTokens, maxTokensAction, tokenSaving, dependencyProtection) {
  return JSON.stringify({
    dlp_enabled: dlpEnabled,
    max_tokens_in_a_request: maxTokens,
    action_for_max_tokens_in_a_request: maxTokensAction,
    token_saving: tokenSaving,
    dependency_protection: dependencyProtection || { inform_updated_packages: false, block_malicious_packages: false },
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
// Data Protection button + popover
// ============================================================================

function updateProtectionSummary() {
  const detections = getDetections();
  const total = detections.length;
  const enabled = detections.filter(d => d.enabled).length;

  const countEl = document.getElementById('guardian-protection-count');
  if (countEl) {
    countEl.textContent = total > 0 ? `${enabled}/${total} enabled` : '—';
    countEl.classList.toggle('is-off', total > 0 && enabled === 0);
  }

  const summaryEl = document.getElementById('guardian-protection-summary');
  if (summaryEl) {
    summaryEl.textContent = `${enabled} / ${total} enabled`;
  }
}

function openProtectionPopover() {
  const popover = document.getElementById('guardian-protection-popover');
  const btn = document.getElementById('guardian-protection-btn');
  if (!popover) return;
  popover.hidden = false;
  requestAnimationFrame(() => popover.classList.add('show'));
  if (btn) btn.setAttribute('aria-expanded', 'true');
}

function closeProtectionPopover() {
  const popover = document.getElementById('guardian-protection-popover');
  const btn = document.getElementById('guardian-protection-btn');
  if (!popover) return;
  popover.classList.remove('show');
  if (btn) btn.setAttribute('aria-expanded', 'false');
  setTimeout(() => { popover.hidden = true; }, 180);
}

function toggleProtectionPopover() {
  const popover = document.getElementById('guardian-protection-popover');
  if (!popover) return;
  if (popover.hidden) openProtectionPopover();
  else closeProtectionPopover();
}

function initProtectionControls() {
  const btn = document.getElementById('guardian-protection-btn');
  const popover = document.getElementById('guardian-protection-popover');
  const closeBtn = document.getElementById('guardian-protection-popover-close');
  const enableAll = document.getElementById('guardian-protection-enable-all');
  const disableAll = document.getElementById('guardian-protection-disable-all');
  const openBtn = document.getElementById('guardian-protection-open');

  if (btn) {
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      toggleProtectionPopover();
    });
  }
  if (closeBtn) {
    closeBtn.addEventListener('click', (e) => {
      e.stopPropagation();
      closeProtectionPopover();
    });
  }
  if (popover) {
    popover.addEventListener('click', (e) => e.stopPropagation());
  }
  if (enableAll) {
    enableAll.addEventListener('click', (e) => {
      e.stopPropagation();
      setAllDetections(true);
    });
  }
  if (disableAll) {
    disableAll.addEventListener('click', (e) => {
      e.stopPropagation();
      setAllDetections(false);
    });
  }
  if (openBtn) {
    openBtn.addEventListener('click', (e) => {
      e.stopPropagation();
      closeProtectionPopover();
      showDetectionsModal();
    });
  }

  // Close popover on outside click
  document.addEventListener('click', (e) => {
    const pop = document.getElementById('guardian-protection-popover');
    if (!pop || pop.hidden) return;
    if (pop.contains(e.target)) return;
    if (e.target.closest('#guardian-protection-btn')) return;
    closeProtectionPopover();
  });

  // Close popover on Escape
  document.addEventListener('keydown', (e) => {
    if (e.key !== 'Escape') return;
    const pop = document.getElementById('guardian-protection-popover');
    if (pop && !pop.hidden) closeProtectionPopover();
  });

  // Keep counts in sync
  onDetectionsChanged(updateProtectionSummary);
  updateProtectionSummary();
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
    const blockActive = tokenAction === 'block' ? 'is-active' : '';
    const notifyActive = tokenAction === 'notify' ? 'is-active' : '';
    const tokenNudge = tokenVal === 0 ? 'needs-setup' : '';

    container.innerHTML = `
      <div class="agent-tiles">
        <div class="agent-tile tile-protection ${dlpOn ? 'tile-active' : 'tile-muted'}">
          <div class="agent-tile-head">
            <span class="agent-tile-dot"></span>
            <span class="agent-tile-label">Protection</span>
            <span class="agent-tile-val">${dlpOn ? 'Active' : 'Off'}</span>
          </div>
          <button class="agent-tile-action backend-dlp-btn" type="button">
            ${dlpOn ? 'Disable' : 'Enable'}
          </button>
        </div>

        <div class="agent-tile tile-token ${tokenNudge}">
          <div class="agent-tile-head">
            <i data-lucide="coins"></i>
            <span class="agent-tile-label">Token Cap</span>
          </div>
          <div class="agent-tile-fields">
            <label class="agent-tile-field">
              <span class="agent-tile-flabel-row">
                <span class="agent-tile-flabel">Max tokens</span>
                <button type="button" class="agent-tile-suggest backend-suggest-btn">Use suggested</button>
              </span>
              <input type="number" class="agent-tile-input backend-max-tokens" min="0" value="${tokenVal || ''}" placeholder="200000" />
            </label>
            <div class="agent-tile-field">
              <span class="agent-tile-flabel-row">
                <span class="agent-tile-flabel">When exceeded</span>
              </span>
              <div class="agent-tile-seg" role="group" aria-label="When exceeded">
                <button type="button" class="agent-tile-seg-btn backend-action-btn ${blockActive}" data-action="block">Block</button>
                <button type="button" class="agent-tile-seg-btn backend-action-btn ${notifyActive}" data-action="notify">Notify</button>
              </div>
            </div>
          </div>
        </div>

        <div class="agent-tile tile-dep tile-dep--block ${settings.dependency_protection.block_malicious_packages ? 'tile-active' : 'tile-muted'}">
          <div class="agent-tile-head">
            <span class="agent-tile-dot"></span>
            <span class="agent-tile-label">Vulnerability Guard</span>
            <span class="agent-tile-val">${settings.dependency_protection.block_malicious_packages ? 'Active' : 'Off'}</span>
          </div>
          <span class="agent-tile-desc">Checks every install command and dependency file against the OSV vulnerability database. Blocks packages with known CVEs.</span>
          <button class="agent-tile-action dep-block-btn" type="button">
            ${settings.dependency_protection.block_malicious_packages ? 'Disable' : 'Enable'}
          </button>
        </div>

        <div class="agent-tile tile-dep tile-dep--inform ${settings.dependency_protection.inform_updated_packages ? 'tile-active' : 'tile-muted'}">
          <div class="agent-tile-head">
            <span class="agent-tile-dot"></span>
            <span class="agent-tile-label">Update Advisor</span>
            <span class="agent-tile-val">${settings.dependency_protection.inform_updated_packages ? 'Active' : 'Off'}</span>
          </div>
          <span class="agent-tile-desc">When the agent installs or pins a package, checks the registry for newer versions and nudges the agent to ask the user about updating.</span>
          <button class="agent-tile-action dep-inform-btn" type="button">
            ${settings.dependency_protection.inform_updated_packages ? 'Disable' : 'Enable'}
          </button>
        </div>
      </div>
    `;

    lucide.createIcons({ nodes: [container] });

    // Protection: single click toggles enable/disable
    const dlpBtn = container.querySelector('.backend-dlp-btn');
    dlpBtn.addEventListener('click', (e) => {
      e.stopPropagation();
      toggleDlp(container, backendName);
    });

    // Token Cap: max tokens — save on blur (or Enter)
    const maxInput = container.querySelector('.backend-max-tokens');
    maxInput.addEventListener('click', (e) => e.stopPropagation());
    maxInput.addEventListener('blur', () => saveLimits(container, backendName));
    maxInput.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') { e.preventDefault(); maxInput.blur(); }
    });

    // Token Cap: action segmented buttons — click to select + save
    container.querySelectorAll('.backend-action-btn').forEach(btn => {
      btn.addEventListener('click', (e) => {
        e.preventDefault();
        e.stopPropagation();
        if (btn.classList.contains('is-active')) return;
        container.querySelectorAll('.backend-action-btn').forEach(b => b.classList.remove('is-active'));
        btn.classList.add('is-active');
        saveLimits(container, backendName);
      });
    });

    // Token Cap: "Use suggested" — autofill 200k and save
    const suggestBtn = container.querySelector('.backend-suggest-btn');
    suggestBtn.addEventListener('click', (e) => {
      e.preventDefault();
      e.stopPropagation();
      maxInput.value = '200000';
      saveLimits(container, backendName);
    });

    // Dep Protection: Vulnerability Guard toggle
    const depBlockBtn = container.querySelector('.dep-block-btn');
    if (depBlockBtn) {
      depBlockBtn.addEventListener('click', (e) => {
        e.stopPropagation();
        toggleDepProtection(backendName, 'block_malicious_packages', !settings.dependency_protection.block_malicious_packages);
      });
    }

    // Dep Protection: Update Advisor toggle
    const depInformBtn = container.querySelector('.dep-inform-btn');
    if (depInformBtn) {
      depInformBtn.addEventListener('click', (e) => {
        e.stopPropagation();
        toggleDepProtection(backendName, 'inform_updated_packages', !settings.dependency_protection.inform_updated_packages);
      });
    }
  }
}

// ============================================================================
// Save / Reset
// ============================================================================

async function toggleDlp(container, backendName) {
  const existing = predefinedBackends.find(b => b.name === backendName);
  const s = existing ? parseSettings(existing.settings) : parseSettings('{}');
  const dlpEnabled = !s.dlp_enabled;

  const settings = buildSettingsJson(dlpEnabled, s.max_tokens_in_a_request, s.action_for_max_tokens_in_a_request, s.token_saving, s.dependency_protection);

  const btn = container.querySelector('.backend-dlp-btn');
  if (btn) btn.disabled = true;

  try {
    await invoke('update_predefined_backend', { name: backendName, settings });
    await invoke('restart_server');
    loadPredefinedBackends();
  } catch (error) {
    alert(`Failed to save: ${error}`);
    if (btn) btn.disabled = false;
  }
}

async function saveLimits(container, backendName) {
  const maxInput = container.querySelector('.backend-max-tokens');
  const activeActionBtn = container.querySelector('.backend-action-btn.is-active');
  const maxTokens = parseInt(maxInput.value) || 0;
  const maxTokensAction = activeActionBtn?.dataset.action || 'block';

  const existing = predefinedBackends.find(b => b.name === backendName);
  const s = existing ? parseSettings(existing.settings) : parseSettings('{}');

  // Skip if nothing actually changed — avoids an unnecessary server restart
  if (maxTokens === s.max_tokens_in_a_request && maxTokensAction === s.action_for_max_tokens_in_a_request) {
    return;
  }

  const settings = buildSettingsJson(s.dlp_enabled, maxTokens, maxTokensAction, s.token_saving, s.dependency_protection);

  try {
    await invoke('update_predefined_backend', { name: backendName, settings });
    await invoke('restart_server');
    loadPredefinedBackends();
  } catch (error) {
    alert(`Failed to save: ${error}`);
  }
}

async function toggleDepProtection(backendName, key, enabled) {
  const existing = predefinedBackends.find(b => b.name === backendName);
  const s = existing ? parseSettings(existing.settings) : parseSettings('{}');

  const depProt = { ...s.dependency_protection, [key]: enabled };
  const settings = buildSettingsJson(s.dlp_enabled, s.max_tokens_in_a_request, s.action_for_max_tokens_in_a_request, s.token_saving, depProt);

  try {
    await invoke('update_predefined_backend', { name: backendName, settings });
    await invoke('restart_server');
    loadPredefinedBackends();
  } catch (error) {
    alert(`Failed to save: ${error}`);
  }
}

// ============================================================================
// Init (called once from main.js)
// ============================================================================

export function initBackends() {
  AGENTS.forEach(attachAgentHandler);
  initProtectionControls();
  refreshGuardianHooks();
  loadPredefinedBackends();
}
