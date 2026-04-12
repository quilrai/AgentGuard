import { invoke } from './utils.js';
import { parseSettings, buildSettingsJson } from './backends.js';

// ---------------------------------------------------------------------------
// Hook-based agents: toggle installs/uninstalls a hook AND sets a backend flag
// ---------------------------------------------------------------------------
const SHELL_AGENTS = [
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

const HOOK_AGENTS = [...SHELL_AGENTS, ...CTX_READ_AGENTS];

// ---------------------------------------------------------------------------
// Settings-only agents: toggle only flips a backend flag (no hook install).
// These all require shell_compression to be on (auto-enabled on turn-on).
// ---------------------------------------------------------------------------
const ADVANCED_KEYS = ['search_compressor', 'diff_compressor', 'tool_crusher'];

const SETTINGS_AGENTS = [
  { id: 'claude', backend: 'claude', toggleId: 'ts-claude-search-compressor',  settingKey: 'search_compressor' },
  { id: 'claude', backend: 'claude', toggleId: 'ts-claude-diff-compressor',    settingKey: 'diff_compressor' },
  { id: 'claude', backend: 'claude', toggleId: 'ts-claude-tool-crusher',       settingKey: 'tool_crusher' },
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
          for (const sa of SETTINGS_AGENTS) {
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

      // Auto-enable shell_compression when turning on an advanced feature
      if (enabled && !settings.token_saving.shell_compression) {
        // Install the shell compression hook first
        try {
          await invoke('install_compression_hook_claude');
        } catch (_) {
          // Hook may already be installed
        }
        settings.token_saving.shell_compression = true;
        // Update the shell toggle in the UI
        const shellToggle = document.getElementById('ts-claude-shell');
        if (shellToggle) shellToggle.checked = true;
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
  refreshAllToggles();
}

export function refreshTokenSaver() {
  refreshAllToggles();
}
