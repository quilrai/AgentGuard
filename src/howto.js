import { invoke } from './utils.js';

// Get instructions for each tool
function getToolInstructions(tool) {
  const instructions = {
    'claude-code': {
      title: 'Claude Code CLI',
      content: `<p class="empty-text">Hook installer coming soon.</p>`
    },
    'cursor': {
      title: 'Cursor',
      content: `
        <p>Cursor uses hooks for data protection integration. Click the button below to install or remove the hooks.</p>

        <div class="cursor-hooks-section">
          <div class="cursor-hooks-status">
            <span class="status-indicator" id="cursor-status-indicator"></span>
            <span id="cursor-status-text">Checking status...</span>
          </div>
          <button id="cursor-hooks-btn" class="btn btn-primary cursor-hooks-btn" disabled>
            Install Hooks
          </button>
        </div>

        <div id="cursor-action-status" class="shell-set-status"></div>

        <div class="cursor-info" style="margin-top: 24px;">
          <h4>What this does:</h4>
          <ul>
            <li>Creates a hook script at <code>~/.cursor/quilr-cursor-hooks.sh</code></li>
            <li>Configures <code>~/.cursor/hooks.json</code> to use the hook</li>
            <li>Intercepts prompts and file reads to check for sensitive data</li>
            <li>Blocks requests containing detected patterns (API keys, custom patterns)</li>
          </ul>
        </div>

        <div class="cursor-info" style="margin-top: 16px;">
          <h4>Hooks enabled:</h4>
          <ul>
            <li><strong>beforeSubmitPrompt</strong> - Check prompts before sending</li>
            <li><strong>beforeReadFile</strong> - Check file contents before agent reads</li>
            <li><strong>beforeTabFileRead</strong> - Check files for Tab completions</li>
            <li><strong>afterAgentResponse</strong> - Log agent responses</li>
            <li><strong>afterAgentThought</strong> - Log thinking process</li>
            <li><strong>afterTabFileEdit</strong> - Log Tab edits</li>
          </ul>
        </div>
      `
    },
    'codex': {
      title: 'Codex CLI',
      content: `<p class="empty-text">Hook installer coming soon.</p>`
    },
  };

  return instructions[tool] || { title: 'Unknown', content: '<p>No instructions available.</p>' };
}

// Check Cursor hooks installation status
async function checkCursorHooksStatus() {
  const statusIndicator = document.getElementById('cursor-status-indicator');
  const statusText = document.getElementById('cursor-status-text');
  const btn = document.getElementById('cursor-hooks-btn');

  if (!statusIndicator || !statusText || !btn) return;

  try {
    const isInstalled = await invoke('check_cursor_hooks_installed');

    if (isInstalled) {
      statusIndicator.className = 'status-indicator installed';
      statusText.textContent = 'Hooks installed';
      btn.textContent = 'Remove Hooks';
      btn.dataset.action = 'remove';
      btn.classList.remove('btn-primary');
      btn.classList.add('btn-danger');
    } else {
      statusIndicator.className = 'status-indicator not-installed';
      statusText.textContent = 'Not installed';
      btn.textContent = 'Install Hooks';
      btn.dataset.action = 'install';
      btn.classList.remove('btn-danger');
      btn.classList.add('btn-primary');
    }
    btn.disabled = false;
  } catch (error) {
    statusIndicator.className = 'status-indicator error';
    statusText.textContent = 'Error checking status';
    btn.disabled = true;
    console.error('Failed to check Cursor hooks status:', error);
  }
}

// Handle Cursor hooks install/uninstall
async function handleCursorHooksAction(btn) {
  const action = btn.dataset.action;
  const statusDiv = document.getElementById('cursor-action-status');

  btn.disabled = true;
  btn.textContent = action === 'install' ? 'Installing...' : 'Removing...';

  try {
    let result;
    if (action === 'install') {
      result = await invoke('install_cursor_hooks');
    } else {
      result = await invoke('uninstall_cursor_hooks');
    }

    // Show success
    btn.textContent = 'Done!';
    btn.classList.remove('btn-primary', 'btn-danger');
    btn.classList.add('btn-success');

    if (statusDiv) {
      statusDiv.textContent = result;
      statusDiv.className = 'shell-set-status show success';
    }

    // Update status after success
    setTimeout(async () => {
      btn.classList.remove('btn-success');
      await checkCursorHooksStatus();
    }, 1500);
  } catch (error) {
    btn.textContent = 'Failed';
    btn.classList.remove('btn-primary', 'btn-danger');
    btn.classList.add('btn-error');

    if (statusDiv) {
      statusDiv.textContent = error;
      statusDiv.className = 'shell-set-status show error';
    }

    // Reset button after 3 seconds
    setTimeout(async () => {
      btn.classList.remove('btn-error');
      await checkCursorHooksStatus();
    }, 3000);
  }
}

// Show instructions for selected tool
async function showToolInstructions(tool) {
  const instructionsDiv = document.getElementById('howto-instructions');
  const buttons = document.querySelectorAll('.howto-tool-btn');

  // Update active button
  buttons.forEach(btn => {
    if (btn.dataset.tool === tool) {
      btn.classList.add('active');
    } else {
      btn.classList.remove('active');
    }
  });

  // Show instructions
  const info = getToolInstructions(tool);
  instructionsDiv.innerHTML = `
    <h3>${info.title}</h3>
    ${info.content}
  `;

  // Handle Cursor hooks
  if (tool === 'cursor') {
    await checkCursorHooksStatus();

    const cursorBtn = document.getElementById('cursor-hooks-btn');
    if (cursorBtn) {
      cursorBtn.addEventListener('click', () => handleCursorHooksAction(cursorBtn));
    }
  }
}

// Initialize How to use tab
export function initHowTo() {
  const buttons = document.querySelectorAll('.howto-tool-btn');
  buttons.forEach(btn => {
    btn.addEventListener('click', () => {
      showToolInstructions(btn.dataset.tool);
    });
  });
}
