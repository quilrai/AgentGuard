// Guided setup flow — Quilly walks new users through full configuration

import { invoke, navigateTo } from './utils.js';

// ============================================================================
// Constants
// ============================================================================

const GUIDE_DONE_KEY = 'quilr_guide_completed';

const AGENTS = [
  { id: 'claude', name: 'Claude Code', checkCmd: 'check_claude_hooks_installed', installCmd: 'install_claude_hooks' },
  { id: 'codex',  name: 'Codex',       checkCmd: 'check_codex_hooks_installed',  installCmd: 'install_codex_hooks' },
  { id: 'cursor', name: 'Cursor',      checkCmd: 'check_cursor_hooks_installed', installCmd: 'install_cursor_hooks' },
];

const AGENT_ICONS = {
  claude: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 1C12.5 6 18 11.5 23 12 18 12.5 12.5 18 12 23 11.5 18 6 12.5 1 12 6 11.5 11.5 6 12 1Z"/></svg>',
  codex:  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M12 2.5L21 7.5V16.5L12 21.5L3 16.5V7.5Z"/><path d="M12 2.5V12L21 7.5" opacity="0.45"/><path d="M12 12L3 7.5" opacity="0.45"/><path d="M12 12V21.5" opacity="0.45"/></svg>',
  cursor: '<svg viewBox="0 0 467 533" fill="currentColor"><path d="M457.4 125.9 244.4 3c-6.8-4-15.3-4-22.1 0L9.3 125.9C3.5 129.3 0 135.4 0 142.1v248c0 6.6 3.5 12.8 9.3 16.1l213 123c6.9 4 15.3 4 22.2 0l213-123c5.8-3.3 9.3-9.5 9.3-16.1v-248c0-6.7-3.5-12.8-9.3-16.1ZM444 152 238.4 508.2c-1.4 2.4-5 1.4-5-1.4V273.6c0-4.7-2.5-9-6.5-11.3L24.9 145.7c-2.4-1.4-1.4-5.1 1.3-5.1h411.3c5.8 0 9.5 6.3 6.6 11.4Z"/></svg>',
};

const QUILLY_SVG = `<svg viewBox="0 0 32 32" fill="none">
  <path d="M16 2C14.5 8 11 14 8 18c-2 2.5-3 4-3.5 6-.4 1.5.2 3 1.5 3.8 1.5 1 3.5.6 4.5-.5 1.5-1.8 2.5-4 3.5-6.3.8-2 1.5-4 2-6z" fill="#71D083" opacity="0.85"/>
  <path d="M16 2c.8 5 2 10 4.5 14.5 1.5 2.8 3.2 5 4.5 6.5 1 1.2.5 3-.8 3.5-1.5.6-3.2-.2-4-1.5-1.5-2.5-2.5-5.5-3.2-8.5-.5-2.5-1-5-1-6.5z" fill="#4a8a3a" opacity="0.75"/>
  <path d="M15.5 2c0 0 .3 4 .5 8s0 8-.5 12" stroke="#2d5a1e" stroke-width="1.2" stroke-linecap="round" opacity="0.6"/>
  <circle cx="16" cy="28" r="2.5" fill="#5a3a20"/>
</svg>`;

// ============================================================================
// State
// ============================================================================

let currentStep = 0;
let agentStates = {}; // { claude: { installed: false, busy: false }, ... }
let guardianDone = false;
let tokenSaverDone = false;

// ============================================================================
// Step definitions
// ============================================================================

function getSteps() {
  return [
    { id: 'welcome',  title: 'Welcome' },
    { id: 'hooks',    title: 'Add Agents' },
    { id: 'guardian', title: 'Guardian' },
    { id: 'saver',   title: 'Token Saver' },
    { id: 'done',    title: 'All Set' },
  ];
}

// ============================================================================
// Render helpers
// ============================================================================

function renderStepDots() {
  const steps = getSteps();
  return `<div class="guide-dots">
    ${steps.map((s, i) => `<span class="guide-dot${i === currentStep ? ' active' : ''}${i < currentStep ? ' done' : ''}" title="${s.title}"></span>`).join('')}
  </div>`;
}

function quillyBubble(message, mood) {
  const moodClass = mood ? ` quilly-${mood}` : '';
  return `
    <div class="guide-quilly${moodClass}">
      <div class="guide-quilly-icon">${QUILLY_SVG}</div>
      <div class="guide-bubble">
        <div class="guide-bubble-arrow"></div>
        <div class="guide-bubble-text">${message}</div>
      </div>
    </div>`;
}

// ============================================================================
// Step renderers
// ============================================================================

function renderWelcome() {
  return `
    ${quillyBubble("Hi there! I'm <strong>Quilly</strong>, your setup guide. Let me help you get everything configured so your AI agents are protected and optimized.", 'wave')}
    <div class="guide-actions">
      <button class="guide-btn guide-btn--primary" id="guide-next">Let's get started</button>
      <button class="guide-btn guide-btn--skip" id="guide-skip">I'll set up later</button>
    </div>
  `;
}

function renderHooks() {
  const agentRows = AGENTS.map(a => {
    const st = agentStates[a.id] || {};
    const installed = st.installed;
    const busy = st.busy;
    const icon = AGENT_ICONS[a.id];
    const statusClass = installed ? 'guide-agent--installed' : '';
    const btnLabel = busy ? 'Adding...' : installed ? 'Added' : '+ Add';
    const btnClass = installed ? 'guide-agent-btn--done' : 'guide-agent-btn--add';
    return `
      <div class="guide-agent ${statusClass}" data-agent="${a.id}">
        <span class="guide-agent-icon guide-agent-icon--${a.id}">${icon}</span>
        <span class="guide-agent-name">${a.name}</span>
        <button class="guide-agent-btn ${btnClass}" data-agent="${a.id}" ${busy ? 'disabled' : ''} ${installed ? 'disabled' : ''}>${btnLabel}</button>
      </div>`;
  }).join('');

  const anyInstalled = AGENTS.some(a => agentStates[a.id]?.installed);

  return `
    ${quillyBubble("First, let's connect your AI coding agents. Click <strong>+ Add</strong> to install hooks for each agent you use.", 'point')}
    <div class="guide-agent-list">${agentRows}</div>
    <div class="guide-actions">
      <button class="guide-btn guide-btn--primary" id="guide-next" ${anyInstalled ? '' : 'disabled'}>Next</button>
      <button class="guide-btn guide-btn--skip" id="guide-skip-step">Skip for now</button>
    </div>
  `;
}

function renderGuardian() {
  const checkmark = guardianDone ? '<span class="guide-checkmark">&#10003;</span>' : '';
  return `
    ${quillyBubble("Now let's protect your code. The <strong>Guardian Agent</strong> scans for secrets, enforces token limits, and guards dependencies.", 'shield')}
    <div class="guide-feature-card">
      <div class="guide-feature-items">
        <div class="guide-feature-item"><span class="guide-feature-dot"></span> Data Protection (DLP)</div>
        <div class="guide-feature-item"><span class="guide-feature-dot"></span> Token Cap (200K limit)</div>
        <div class="guide-feature-item"><span class="guide-feature-dot"></span> Vulnerability Guard</div>
        <div class="guide-feature-item"><span class="guide-feature-dot"></span> Update Advisor</div>
      </div>
      <button class="guide-btn guide-btn--recommend" id="guide-guardian-recommend" ${guardianDone ? 'disabled' : ''}>
        ${checkmark}${guardianDone ? 'Recommended settings applied' : 'Enable recommended settings'}
      </button>
    </div>
    <div class="guide-actions">
      <button class="guide-btn guide-btn--primary" id="guide-next">Next</button>
      <button class="guide-btn guide-btn--skip" id="guide-skip-step">Skip</button>
    </div>
  `;
}

function renderTokenSaver() {
  const checkmark = tokenSaverDone ? '<span class="guide-checkmark">&#10003;</span>' : '';
  return `
    ${quillyBubble("Finally, let's save you some tokens! The <strong>Token Saver</strong> compresses shell output, caches file reads, and crushes bulky JSON.", 'sparkle')}
    <div class="guide-feature-card">
      <div class="guide-feature-items">
        <div class="guide-feature-item"><span class="guide-feature-dot"></span> Shell output compression</div>
        <div class="guide-feature-item"><span class="guide-feature-dot"></span> File read caching</div>
        <div class="guide-feature-item"><span class="guide-feature-dot"></span> Search compressor</div>
        <div class="guide-feature-item"><span class="guide-feature-dot"></span> Diff compressor</div>
        <div class="guide-feature-item"><span class="guide-feature-dot"></span> JSON crusher</div>
      </div>
      <button class="guide-btn guide-btn--recommend" id="guide-saver-recommend" ${tokenSaverDone ? 'disabled' : ''}>
        ${checkmark}${tokenSaverDone ? 'All token saving enabled' : 'Enable recommended settings'}
      </button>
    </div>
    <div class="guide-actions">
      <button class="guide-btn guide-btn--primary" id="guide-next">Next</button>
      <button class="guide-btn guide-btn--skip" id="guide-skip-step">Skip</button>
    </div>
  `;
}

function renderDone() {
  const lines = [];
  const installedAgents = AGENTS.filter(a => agentStates[a.id]?.installed).map(a => a.name);
  if (installedAgents.length) lines.push(`Agents connected: <strong>${installedAgents.join(', ')}</strong>`);
  if (guardianDone) lines.push('Guardian protections: <strong>enabled</strong>');
  if (tokenSaverDone) lines.push('Token saving: <strong>enabled</strong>');
  if (!lines.length) lines.push('You can configure everything from the home screen anytime.');

  return `
    ${quillyBubble("You're all set! Your workspace is configured and ready to go. I'll be in the <strong>Garden</strong> if you need me.", 'celebrate')}
    <div class="guide-summary">
      ${lines.map(l => `<div class="guide-summary-line">${l}</div>`).join('')}
    </div>
    <div class="guide-actions">
      <button class="guide-btn guide-btn--primary" id="guide-finish">Start using Vibefriend</button>
    </div>
  `;
}

// ============================================================================
// Main render
// ============================================================================

function render() {
  const container = document.getElementById('guide-overlay');
  if (!container) return;

  const steps = getSteps();
  const step = steps[currentStep];
  let body = '';

  switch (step.id) {
    case 'welcome':  body = renderWelcome(); break;
    case 'hooks':    body = renderHooks(); break;
    case 'guardian': body = renderGuardian(); break;
    case 'saver':    body = renderTokenSaver(); break;
    case 'done':     body = renderDone(); break;
  }

  container.innerHTML = `
    <div class="guide-backdrop"></div>
    <div class="guide-panel">
      <div class="guide-panel-head">
        <span class="guide-step-label">${step.title}</span>
        ${renderStepDots()}
      </div>
      <div class="guide-panel-body">${body}</div>
    </div>
  `;

  attachHandlers();
}

// ============================================================================
// Event handlers
// ============================================================================

function attachHandlers() {
  // Next / skip / finish
  const next = document.getElementById('guide-next');
  const skip = document.getElementById('guide-skip');
  const skipStep = document.getElementById('guide-skip-step');
  const finish = document.getElementById('guide-finish');

  if (next) next.addEventListener('click', () => goStep(currentStep + 1));
  if (skipStep) skipStep.addEventListener('click', () => goStep(currentStep + 1));
  if (skip) skip.addEventListener('click', closeGuide);
  if (finish) finish.addEventListener('click', closeGuide);

  // Agent install buttons
  document.querySelectorAll('.guide-agent-btn[data-agent]').forEach(btn => {
    btn.addEventListener('click', () => installAgent(btn.dataset.agent));
  });

  // Guardian recommend
  const guardianBtn = document.getElementById('guide-guardian-recommend');
  if (guardianBtn) guardianBtn.addEventListener('click', applyGuardianRecommended);

  // Token saver recommend
  const saverBtn = document.getElementById('guide-saver-recommend');
  if (saverBtn) saverBtn.addEventListener('click', applyTokenSaverRecommended);
}

function goStep(idx) {
  const steps = getSteps();
  if (idx < 0 || idx >= steps.length) return;
  currentStep = idx;
  render();
}

// ============================================================================
// Actions
// ============================================================================

async function installAgent(agentId) {
  const agent = AGENTS.find(a => a.id === agentId);
  if (!agent) return;
  agentStates[agentId] = { ...agentStates[agentId], busy: true };
  render();

  try {
    await invoke(agent.installCmd);
    agentStates[agentId] = { installed: true, busy: false };
  } catch (e) {
    console.error(`Guide: failed to install ${agentId}:`, e);
    agentStates[agentId] = { installed: false, busy: false };
  }
  render();
}

async function applyGuardianRecommended() {
  const btn = document.getElementById('guide-guardian-recommend');
  if (btn) { btn.disabled = true; btn.textContent = 'Applying...'; }

  try {
    // Apply recommended guardian for all installed agents
    const backends = ['claude', 'codex', 'cursor-hooks'];
    for (const name of backends) {
      try {
        const predefined = await invoke('get_predefined_backends');
        const backend = predefined.find(b => b.name === name);
        if (!backend) continue;
        const settings = JSON.parse(backend.settings || '{}');
        settings.dlp_enabled = true;
        settings.max_tokens_in_a_request = 200000;
        settings.action_for_max_tokens_in_a_request = 'block';
        settings.dependency_protection = {
          block_malicious_packages: true,
          inform_updated_packages: true,
        };
        await invoke('update_predefined_backend', { name, settings: JSON.stringify(settings) });
      } catch (_) {}
    }
    await invoke('restart_server');
    guardianDone = true;
  } catch (e) {
    console.error('Guide: failed to apply guardian settings:', e);
  }
  render();
}

async function applyTokenSaverRecommended() {
  const btn = document.getElementById('guide-saver-recommend');
  if (btn) { btn.disabled = true; btn.textContent = 'Applying...'; }

  try {
    // Install hooks
    try { await invoke('install_compression_hook_claude'); } catch (_) {}
    try { await invoke('install_ctx_read_hook_claude'); } catch (_) {}

    // Enable all token saving settings
    const predefined = await invoke('get_predefined_backends');
    const backend = predefined.find(b => b.name === 'claude');
    if (backend) {
      const settings = JSON.parse(backend.settings || '{}');
      settings.token_saving = {
        shell_compression: true,
        ctx_read: true,
        search_compressor: true,
        diff_compressor: true,
        tool_crusher: true,
      };
      await invoke('update_predefined_backend', { name: 'claude', settings: JSON.stringify(settings) });
      await invoke('restart_server');
    }
    tokenSaverDone = true;
  } catch (e) {
    console.error('Guide: failed to apply token saver settings:', e);
  }
  render();
}

// ============================================================================
// Open / Close
// ============================================================================

// ============================================================================
// Setup completeness check
// ============================================================================

async function refreshAgentStates() {
  const results = await Promise.all(
    AGENTS.map(a => invoke(a.checkCmd).catch(() => false)),
  );
  AGENTS.forEach((a, i) => {
    agentStates[a.id] = { installed: results[i], busy: false };
  });
}

/** Check if guardian recommended settings are already applied on any backend */
async function checkGuardianComplete() {
  try {
    const predefined = await invoke('get_predefined_backends');
    // Consider guardian done if at least one backend has all protections on
    return predefined.some(b => {
      try {
        const s = JSON.parse(b.settings || '{}');
        const dp = s.dependency_protection || {};
        return s.dlp_enabled && s.max_tokens_in_a_request > 0
          && dp.block_malicious_packages && dp.inform_updated_packages;
      } catch { return false; }
    });
  } catch { return false; }
}

/** Check if token saver is fully configured for claude */
async function checkTokenSaverComplete() {
  try {
    const predefined = await invoke('get_predefined_backends');
    const claude = predefined.find(b => b.name === 'claude');
    if (!claude) return false;
    const s = JSON.parse(claude.settings || '{}');
    const ts = s.token_saving || {};
    return ts.shell_compression && ts.ctx_read;
  } catch { return false; }
}

/** Returns { anyAgents, allAgents, guardianDone, tokenSaverDone, fullyDone } */
async function checkSetupStatus() {
  await refreshAgentStates();
  const anyAgents = AGENTS.some(a => agentStates[a.id]?.installed);
  const allAgents = AGENTS.every(a => agentStates[a.id]?.installed);
  const gDone = await checkGuardianComplete();
  const tDone = await checkTokenSaverComplete();
  return {
    anyAgents,
    allAgents,
    guardianDone: gDone,
    tokenSaverDone: tDone,
    fullyDone: anyAgents && gDone && tDone,
  };
}

// ============================================================================
// Open / Close
// ============================================================================

function openGuide(startAt) {
  const overlay = document.getElementById('guide-overlay');
  if (!overlay) return;
  currentStep = startAt || 0;
  overlay.hidden = false;
  requestAnimationFrame(() => overlay.classList.add('show'));
  render();
}

function closeGuide() {
  const overlay = document.getElementById('guide-overlay');
  if (!overlay) return;
  overlay.classList.remove('show');
  localStorage.setItem(GUIDE_DONE_KEY, '1');
  setTimeout(() => { overlay.hidden = true; }, 350);
}

// ============================================================================
// Public API
// ============================================================================

export function isGuideCompleted() {
  return localStorage.getItem(GUIDE_DONE_KEY) === '1';
}

export function resetGuide() {
  localStorage.removeItem(GUIDE_DONE_KEY);
}

/**
 * Uninstall all hooks, reset all backend settings, and clear guide state.
 * Returns true on success.
 */
export async function resetAllSettings() {
  // Uninstall all guardian hooks
  const uninstallCmds = [
    'uninstall_claude_hooks',
    'uninstall_codex_hooks',
    'uninstall_cursor_hooks',
  ];
  // Uninstall all token-saver hooks
  const tsUninstallCmds = [
    'uninstall_compression_hook_claude',
    'uninstall_ctx_read_hook_claude',
  ];

  const allCmds = [...uninstallCmds, ...tsUninstallCmds];
  await Promise.all(allCmds.map(cmd => invoke(cmd).catch(() => {})));

  // Reset all backend settings to defaults
  const backends = ['claude', 'codex', 'cursor-hooks'];
  const defaultSettings = JSON.stringify({
    dlp_enabled: false,
    max_tokens_in_a_request: 0,
    action_for_max_tokens_in_a_request: 'block',
    token_saving: {
      shell_compression: false,
      ctx_read: false,
      search_compressor: false,
      diff_compressor: false,
      tool_crusher: false,
      compression_cache: false,
    },
    dependency_protection: {
      block_malicious_packages: false,
      inform_updated_packages: false,
    },
  });

  for (const name of backends) {
    try {
      await invoke('update_predefined_backend', { name, settings: defaultSettings });
    } catch (_) {}
  }

  try { await invoke('restart_server'); } catch (_) {}

  // Reset guide state so it auto-shows again
  localStorage.removeItem(GUIDE_DONE_KEY);

  return true;
}

/** Manually open the guide (from "Setup guide" link). Always opens. */
export async function startGuide() {
  const status = await checkSetupStatus();
  guardianDone = status.guardianDone;
  tokenSaverDone = status.tokenSaverDone;
  openGuide(0);
}

/**
 * Auto-show on app launch when setup is incomplete.
 * Skips if localStorage says guide was dismissed, OR if everything is configured.
 */
export async function initGuide() {
  if (isGuideCompleted()) return;

  const status = await checkSetupStatus();

  // Everything configured — mark done silently, no guide needed
  if (status.fullyDone) {
    localStorage.setItem(GUIDE_DONE_KEY, '1');
    return;
  }

  // Pre-populate state so already-done steps show checkmarks
  guardianDone = status.guardianDone;
  tokenSaverDone = status.tokenSaverDone;

  // Open at the first incomplete step
  let startAt = 0; // welcome
  if (status.anyAgents && !status.guardianDone) startAt = 2; // guardian
  else if (status.anyAgents && status.guardianDone && !status.tokenSaverDone) startAt = 3; // saver

  openGuide(startAt);
}
