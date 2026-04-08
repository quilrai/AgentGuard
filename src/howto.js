import { invoke } from './utils.js';

// Guardian Agent setup hooks: Claude Code, Codex CLI, Cursor.
// Each row mirrors the token-saver sc-hook-row pattern.

const AGENTS = [
  {
    id: 'claude',
    checkCmd: 'check_claude_hooks_installed',
    installCmd: 'install_claude_hooks',
    uninstallCmd: 'uninstall_claude_hooks',
  },
  {
    id: 'codex',
    checkCmd: 'check_codex_hooks_installed',
    installCmd: 'install_codex_hooks',
    uninstallCmd: 'uninstall_codex_hooks',
  },
  {
    id: 'cursor',
    checkCmd: 'check_cursor_hooks_installed',
    installCmd: 'install_cursor_hooks',
    uninstallCmd: 'uninstall_cursor_hooks',
  },
];

async function refreshAgentStatus(agent) {
  const statusEl = document.getElementById(`guardian-${agent.id}-status`);
  const btn = document.getElementById(`guardian-${agent.id}-btn`);
  if (!statusEl || !btn) return;

  try {
    const installed = await invoke(agent.checkCmd);
    statusEl.textContent = installed ? 'Installed' : 'Not installed';
    statusEl.className = `sc-hook-status ${installed ? 'installed' : 'not-installed'}`;
    btn.textContent = installed ? 'Remove' : 'Install';
    btn.className = `btn btn-sm ${installed ? 'btn-danger' : 'btn-primary'}`;
    btn.disabled = false;
  } catch (error) {
    statusEl.textContent = 'Error';
    statusEl.className = 'sc-hook-status error';
    btn.disabled = true;
    console.error(`Failed to check ${agent.id} hook status:`, error);
  }
}

function attachAgentHandler(agent) {
  const btn = document.getElementById(`guardian-${agent.id}-btn`);
  if (!btn) return;

  btn.addEventListener('click', async () => {
    const isInstalled = btn.textContent === 'Remove';
    btn.disabled = true;
    btn.textContent = isInstalled ? 'Removing...' : 'Installing...';

    try {
      await invoke(isInstalled ? agent.uninstallCmd : agent.installCmd);
    } catch (error) {
      console.error(`Failed to ${isInstalled ? 'uninstall' : 'install'} ${agent.id} hooks:`, error);
      alert(`Failed: ${error}`);
    } finally {
      await refreshAgentStatus(agent);
    }
  });
}

export async function refreshGuardianHooks() {
  await Promise.all(AGENTS.map(refreshAgentStatus));
}

export function initHowTo() {
  AGENTS.forEach(attachAgentHandler);
  refreshGuardianHooks();
}
