import { invoke } from './utils.js';
import { parseSettings, buildSettingsJson } from './backends.js';

const AGENTS = [
  {
    id: 'claude',
    backend: 'claude',
    checkCmd: 'check_compression_hook_claude',
    installCmd: 'install_compression_hook_claude',
    uninstallCmd: 'uninstall_compression_hook_claude',
  },
];

async function checkAndSetToggle(agent) {
  const toggle = document.getElementById(`ts-${agent.id}-shell`);
  if (!toggle) return;

  try {
    const installed = await invoke(agent.checkCmd);
    toggle.checked = installed;
    toggle.disabled = false;
  } catch (error) {
    toggle.disabled = true;
    console.error(`Failed to check ${agent.id} hook status:`, error);
  }
}

async function setBackendCompression(backendName, enabled) {
  const predefined = await invoke('get_predefined_backends');
  const backend = predefined.find(b => b.name === backendName);
  if (!backend) return;

  const settings = parseSettings(backend.settings);
  settings.token_saving.shell_compression = enabled;
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
  const toggle = document.getElementById(`ts-${agent.id}-shell`);
  if (!toggle) return;

  toggle.addEventListener('change', async () => {
    const shouldInstall = toggle.checked;
    toggle.disabled = true;

    try {
      if (shouldInstall) {
        await invoke(agent.installCmd);
        await setBackendCompression(agent.backend, true);
      } else {
        await invoke(agent.uninstallCmd);
        await setBackendCompression(agent.backend, false);
      }
    } catch (error) {
      console.error(`Failed to ${shouldInstall ? 'install' : 'uninstall'} ${agent.id} hook:`, error);
      toggle.checked = !shouldInstall;
      alert(`Failed: ${error}`);
    } finally {
      toggle.disabled = false;
    }
  });
}

async function refreshAllToggles() {
  await Promise.all(AGENTS.map(a => checkAndSetToggle(a)));
}

export function initTokenSaving() {
  AGENTS.forEach(a => setupToggle(a));
  refreshAllToggles();
}

export function refreshTokenSaver() {
  refreshAllToggles();
}
