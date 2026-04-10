import { invoke } from './utils.js';
import { parseSettings, buildSettingsJson } from './backends.js';

// Each feature has its own AGENTS array keyed by toggle id suffix.
// Shell compression agents:
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

// File read caching agents:
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

const ALL_AGENTS = [...SHELL_AGENTS, ...CTX_READ_AGENTS];

async function checkAndSetToggle(agent) {
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

async function setBackendSetting(backendName, settingKey, enabled) {
  const predefined = await invoke('get_predefined_backends');
  const backend = predefined.find(b => b.name === backendName);
  if (!backend) return;

  const settings = parseSettings(backend.settings);
  settings.token_saving[settingKey] = enabled;
  const newSettings = buildSettingsJson(
    settings.dlp_enabled,
    settings.max_tokens_in_a_request,
    settings.action_for_max_tokens_in_a_request,
    settings.token_saving
  );
  await invoke('update_predefined_backend', { name: backendName, settings: newSettings });
  await invoke('restart_server');
}

function setupToggle(agent) {
  const toggle = document.getElementById(agent.toggleId);
  if (!toggle) return;

  toggle.addEventListener('change', async () => {
    const shouldInstall = toggle.checked;
    toggle.disabled = true;

    try {
      if (shouldInstall) {
        await invoke(agent.installCmd);
        await setBackendSetting(agent.backend, agent.settingKey, true);
      } else {
        await invoke(agent.uninstallCmd);
        await setBackendSetting(agent.backend, agent.settingKey, false);
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

async function refreshAllToggles() {
  await Promise.all(ALL_AGENTS.map(a => checkAndSetToggle(a)));
}

export function initTokenSaving() {
  ALL_AGENTS.forEach(a => setupToggle(a));
  refreshAllToggles();
}

export function refreshTokenSaver() {
  refreshAllToggles();
}
