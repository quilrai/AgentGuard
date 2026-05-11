import { invoke } from './utils.js';
import { parseSettings, buildSettingsJson } from './backends.js';

// ---------------------------------------------------------------------------
// Hook-based agents: toggle installs/uninstalls a hook AND sets a backend flag
// ---------------------------------------------------------------------------
const SHELL_HOOK_AGENTS = [
  {
    id: 'claude',
    backend: 'claude',
    toggleId: 'ts-claude-shell',
    settingKey: 'shell_compression',
    checkCmd: 'check_compression_hook_claude',
    installCmd: 'install_compression_hook_claude',
    uninstallCmd: 'uninstall_compression_hook_claude',
  },
];

const CTX_READ_AGENTS = [
  {
    id: 'claude',
    backend: 'claude',
    toggleId: 'ts-claude-ctx-read',
    settingKey: 'ctx_read',
    checkCmd: 'check_ctx_read_hook_claude',
    installCmd: 'install_ctx_read_hook_claude',
    uninstallCmd: 'uninstall_ctx_read_hook_claude',
  },
];

const HOOK_AGENTS = [...SHELL_HOOK_AGENTS, ...CTX_READ_AGENTS];

// ---------------------------------------------------------------------------
// Settings-only agents: toggle only flips a backend flag (no hook install).
// Advanced compressors require shell_compression to be on (auto-enabled on turn-on).
// ---------------------------------------------------------------------------
const ADVANCED_KEYS = ['search_compressor', 'diff_compressor', 'tool_crusher'];
const SHELL_TOGGLE_BY_BACKEND = {
  claude: 'ts-claude-shell',
  codex: 'ts-codex-shell',
};
const SHELL_ENABLE_COMMAND_BY_BACKEND = {
  claude: 'install_compression_hook_claude',
  codex: 'install_codex_hooks',
};

const SETTINGS_AGENTS = [
  { id: 'claude', backend: 'claude', toggleId: 'ts-claude-search-compressor',  settingKey: 'search_compressor' },
  { id: 'claude', backend: 'claude', toggleId: 'ts-claude-diff-compressor',    settingKey: 'diff_compressor' },
  { id: 'claude', backend: 'claude', toggleId: 'ts-claude-tool-crusher',       settingKey: 'tool_crusher' },
  { id: 'codex',  backend: 'codex',  toggleId: 'ts-codex-shell',               settingKey: 'shell_compression', ensureCmd: 'install_codex_hooks' },
  { id: 'codex',  backend: 'codex',  toggleId: 'ts-codex-search-compressor',   settingKey: 'search_compressor' },
  { id: 'codex',  backend: 'codex',  toggleId: 'ts-codex-diff-compressor',     settingKey: 'diff_compressor' },
  { id: 'codex',  backend: 'codex',  toggleId: 'ts-codex-tool-crusher',        settingKey: 'tool_crusher' },
];

const ALL_AGENTS = [...HOOK_AGENTS, ...SETTINGS_AGENTS];

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

async function getBackendSettings(backendName) {
  const predefined = await invoke('get_predefined_backends');
  const backend = predefined.find(b => b.name === backendName);
  if (!backend) return null;
  return parseSettings(backend.settings);
}

async function saveTokenSaving(backendName, tokenSaving) {
  const predefined = await invoke('get_predefined_backends');
  const backend = predefined.find(b => b.name === backendName);
  if (!backend) return;
  const settings = parseSettings(backend.settings);
  const newSettings = buildSettingsJson(
    settings.dlp_enabled,
    settings.max_tokens_in_a_request,
    settings.action_for_max_tokens_in_a_request,
    tokenSaving,
    settings.dependency_protection,
  );
  await invoke('update_predefined_backend', { name: backendName, settings: newSettings });
  await invoke('restart_server');
}

function anyAdvancedOn(tokenSaving) {
  return ADVANCED_KEYS.some(k => tokenSaving[k]);
}

async function ensureShellCompressionSupport(backendName) {
  const command = SHELL_ENABLE_COMMAND_BY_BACKEND[backendName];
  if (command) {
    await invoke(command);
  }
}

function setShellToggleChecked(backendName, checked) {
  const toggle = document.getElementById(SHELL_TOGGLE_BY_BACKEND[backendName]);
  if (toggle) toggle.checked = checked;
}

function advancedAgentsForBackend(backendName) {
  return SETTINGS_AGENTS.filter(a => a.backend === backendName && ADVANCED_KEYS.includes(a.settingKey));
}

// ---------------------------------------------------------------------------
// Hook-based toggle: check / install / uninstall
// ---------------------------------------------------------------------------

async function checkAndSetHookToggle(agent) {
  const toggle = document.getElementById(agent.toggleId);
  if (!toggle) return;
  try {
    const installed = await invoke(agent.checkCmd);
    toggle.checked = installed;
    toggle.disabled = false;
  } catch (error) {
    toggle.disabled = true;
    console.error(`Failed to check ${agent.id} ${agent.settingKey} hook status:`, error);
  }
}

function setupHookToggle(agent) {
  const toggle = document.getElementById(agent.toggleId);
  if (!toggle) return;

  toggle.addEventListener('change', async () => {
    const shouldInstall = toggle.checked;
    toggle.disabled = true;

    try {
      // Turning off shell_compression cascades: disable all advanced features
      if (agent.settingKey === 'shell_compression' && !shouldInstall) {
        const settings = await getBackendSettings(agent.backend);
        if (settings && anyAdvancedOn(settings.token_saving)) {
          for (const key of ADVANCED_KEYS) {
            settings.token_saving[key] = false;
          }
          await saveTokenSaving(agent.backend, settings.token_saving);
          // Update advanced toggles in the UI
          for (const sa of advancedAgentsForBackend(agent.backend)) {
            const t = document.getElementById(sa.toggleId);
            if (t) t.checked = false;
          }
        }
      }

      if (shouldInstall) {
        await invoke(agent.installCmd);
      } else {
        await invoke(agent.uninstallCmd);
      }

      const settings = await getBackendSettings(agent.backend);
      if (settings) {
        settings.token_saving[agent.settingKey] = shouldInstall;
        await saveTokenSaving(agent.backend, settings.token_saving);
      }
    } catch (error) {
      console.error(`Failed to ${shouldInstall ? 'install' : 'uninstall'} ${agent.id} ${agent.settingKey} hook:`, error);
      toggle.checked = !shouldInstall;
      alert(`Failed: ${error}`);
    } finally {
      toggle.disabled = false;
    }
  });
}

// ---------------------------------------------------------------------------
// Settings-only toggle: just flip a backend flag (+ auto-enable shell)
// ---------------------------------------------------------------------------

async function checkAndSetSettingsToggle(agent) {
  const toggle = document.getElementById(agent.toggleId);
  if (!toggle) return;
  try {
    const settings = await getBackendSettings(agent.backend);
    if (settings) {
      toggle.checked = !!settings.token_saving[agent.settingKey];
    }
    toggle.disabled = false;
  } catch (error) {
    toggle.disabled = true;
    console.error(`Failed to check ${agent.id} ${agent.settingKey} setting:`, error);
  }
}

function setupSettingsToggle(agent) {
  const toggle = document.getElementById(agent.toggleId);
  if (!toggle) return;

  toggle.addEventListener('change', async () => {
    const enabled = toggle.checked;
    toggle.disabled = true;

    try {
      const settings = await getBackendSettings(agent.backend);
      if (!settings) throw new Error('Backend not found');

      settings.token_saving[agent.settingKey] = enabled;

      // Turning off shell_compression cascades for this backend.
      if (agent.settingKey === 'shell_compression' && !enabled) {
        for (const key of ADVANCED_KEYS) {
          settings.token_saving[key] = false;
        }
        for (const sa of advancedAgentsForBackend(agent.backend)) {
          const t = document.getElementById(sa.toggleId);
          if (t) t.checked = false;
        }
      }

      if (enabled && agent.ensureCmd) {
        await invoke(agent.ensureCmd);
      }

      // Auto-enable shell_compression when turning on an advanced feature
      if (enabled && ADVANCED_KEYS.includes(agent.settingKey) && !settings.token_saving.shell_compression) {
        await ensureShellCompressionSupport(agent.backend);
        settings.token_saving.shell_compression = true;
        setShellToggleChecked(agent.backend, true);
      }

      await saveTokenSaving(agent.backend, settings.token_saving);
    } catch (error) {
      console.error(`Failed to update ${agent.id} ${agent.settingKey}:`, error);
      toggle.checked = !enabled;
      alert(`Failed: ${error}`);
    } finally {
      toggle.disabled = false;
    }
  });
}

// ---------------------------------------------------------------------------
// Recommended settings: enable everything
// ---------------------------------------------------------------------------

async function applyRecommendedTokenSaving() {
  const btn = document.getElementById('ts-recommend-btn');
  if (btn) btn.disabled = true;

  try {
    // Install hooks first
    try { await invoke('install_compression_hook_claude'); } catch (_) {}
    try { await invoke('install_ctx_read_hook_claude'); } catch (_) {}
    try { await invoke('install_codex_hooks'); } catch (_) {}

    const claudeSettings = await getBackendSettings('claude');
    if (!claudeSettings) throw new Error('Claude backend not found');

    claudeSettings.token_saving.shell_compression = true;
    claudeSettings.token_saving.ctx_read = true;
    claudeSettings.token_saving.search_compressor = true;
    claudeSettings.token_saving.diff_compressor = true;
    claudeSettings.token_saving.tool_crusher = true;

    await saveTokenSaving('claude', claudeSettings.token_saving);

    const codexSettings = await getBackendSettings('codex');
    if (codexSettings) {
      codexSettings.token_saving.shell_compression = true;
      codexSettings.token_saving.search_compressor = true;
      codexSettings.token_saving.diff_compressor = true;
      codexSettings.token_saving.tool_crusher = true;
      await saveTokenSaving('codex', codexSettings.token_saving);
    }

    await refreshAllToggles();
    updateRecommendHint();
  } catch (error) {
    alert(`Failed to apply recommended settings: ${error}`);
  } finally {
    if (btn) btn.disabled = false;
  }
}

function updateRecommendHint() {
  const toggleStates = ALL_AGENTS.map(a => {
    const t = document.getElementById(a.toggleId);
    return t && t.checked;
  });
  const allOn = toggleStates.every(Boolean);
  const noneOn = toggleStates.every(v => !v);
  const hint = document.getElementById('ts-recommend-hint');
  const bar = document.getElementById('ts-recommend-bar');
  if (hint) hint.textContent = allOn ? 'All token saving features active' : 'Enable all token saving features with one click';
  if (bar) {
    bar.classList.toggle('is-all-on', allOn);
    bar.classList.toggle('needs-attention', noneOn);
  }
}

// ---------------------------------------------------------------------------
// Refresh / Init
// ---------------------------------------------------------------------------

async function refreshAllToggles() {
  await Promise.all([
    ...HOOK_AGENTS.map(a => checkAndSetHookToggle(a)),
    ...SETTINGS_AGENTS.map(a => checkAndSetSettingsToggle(a)),
  ]);
}

export function initTokenSaving() {
  HOOK_AGENTS.forEach(a => setupHookToggle(a));
  SETTINGS_AGENTS.forEach(a => setupSettingsToggle(a));

  const recommendBtn = document.getElementById('ts-recommend-btn');
  if (recommendBtn) {
    recommendBtn.addEventListener('click', () => applyRecommendedTokenSaving());
  }

  refreshAllToggles().then(() => updateRecommendHint());
}

export function refreshTokenSaver() {
  refreshAllToggles().then(() => updateRecommendHint());
}
