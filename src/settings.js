import { invoke, getCurrentPort, setCurrentPort, escapeHtml } from './utils.js';

// Tauri event listener
const { listen } = window.__TAURI__.event;

// ============ Status Display ============

// Show status message in settings
function showSettingsStatus(message, type, elementId = 'settings-status') {
  const status = document.getElementById(elementId);
  if (!status) return;
  status.textContent = message;
  status.className = 'settings-status show ' + type;

  // Auto-hide after 5 seconds for success
  if (type === 'success') {
    setTimeout(() => {
      status.className = 'settings-status';
    }, 5000);
  }
}

// Update topbar port display
function updateServerStatusDisplay(port, isRestarting = false, isError = false) {
  const statusText = document.getElementById('proxy-status-text');
  const statusDot = document.getElementById('proxy-status-dot');

  if (statusText) {
    if (isError) {
      statusText.innerHTML = `Failed — <span class="proxy-status-link" id="change-port-link">change port</span>`;
      // Add click handler for the link
      const link = document.getElementById('change-port-link');
      if (link) {
        link.addEventListener('click', (e) => {
          e.stopPropagation();
          openPortPopover();
        });
      }
    } else if (isRestarting) {
      statusText.textContent = `Restarting on ${port}…`;
    } else {
      statusText.textContent = `Running at localhost:${port}`;
    }
  }

  if (statusDot) {
    statusDot.classList.remove('restarting', 'error', 'starting');
    if (isError) {
      statusDot.classList.add('error');
    } else if (isRestarting) {
      statusDot.classList.add('restarting');
    }
  }
}

// ============ Port Popover ============

function openPortPopover() {
  const popover = document.getElementById('port-popover');
  if (!popover) return;
  popover.hidden = false;
  // Defer to next frame so the transition runs
  requestAnimationFrame(() => popover.classList.add('show'));
  const input = document.getElementById('port-input');
  if (input) {
    input.value = getCurrentPort();
    setTimeout(() => {
      input.focus();
      input.select();
    }, 50);
  }
}

function closePortPopover() {
  const popover = document.getElementById('port-popover');
  if (!popover) return;
  popover.classList.remove('show');
  // Clear any inline status
  const status = document.getElementById('settings-status');
  if (status) status.className = 'settings-status';
  setTimeout(() => { popover.hidden = true; }, 180);
}

function togglePortPopover() {
  const popover = document.getElementById('port-popover');
  if (!popover) return;
  if (popover.hidden) openPortPopover();
  else closePortPopover();
}

// ============ Port Settings ============

// Load port setting from backend
export async function loadPortSetting() {
  try {
    const port = await invoke('get_port_setting');
    setCurrentPort(port);
    const portInput = document.getElementById('port-input');
    if (portInput) {
      portInput.value = port;
    }
    // Don't update status here - let loadServerStatus handle it
  } catch (error) {
    console.error('Failed to load port setting:', error);
  }
}

// Load and display the actual server status from backend
async function loadServerStatus() {
  try {
    const status = await invoke('get_server_status');
    setCurrentPort(status.port);

    if (status.status === 'running') {
      updateServerStatusDisplay(status.port, false, false);
    } else if (status.status === 'failed') {
      updateServerStatusDisplay(status.port, false, true);
    }
    // If 'starting', leave it as is (yellow dot)
  } catch (error) {
    console.error('Failed to load server status:', error);
  }
}

// Save port setting and restart server
async function savePortSetting() {
  const portInput = document.getElementById('port-input');
  const saveBtn = document.getElementById('save-port-btn');
  const port = parseInt(portInput.value, 10);
  const currentPort = getCurrentPort();

  // Validate
  if (isNaN(port) || port < 1024 || port > 65535) {
    showSettingsStatus('Port must be between 1024 and 65535', 'error');
    return;
  }

  // Skip if port hasn't changed
  if (port === currentPort) {
    showSettingsStatus('Port unchanged', 'info');
    return;
  }

  saveBtn.disabled = true;
  saveBtn.textContent = 'Saving...';
  updateServerStatusDisplay(port, true);

  try {
    // Save the port setting
    await invoke('save_port_setting', { port });
    setCurrentPort(port);

    // Restart the server
    showSettingsStatus('Restarting server...', 'info');
    await invoke('restart_server');

    // Wait for server to restart
    await new Promise(resolve => setTimeout(resolve, 1500));
    updateServerStatusDisplay(port, false);
    showSettingsStatus(`Server now running on port ${port}`, 'success');
    // Auto-close the popover shortly after success
    setTimeout(() => closePortPopover(), 900);
  } catch (error) {
    updateServerStatusDisplay(currentPort, false);
    showSettingsStatus(`Failed: ${error}`, 'error');
  } finally {
    saveBtn.disabled = false;
    saveBtn.textContent = 'Save';
  }
}

// ============ DLP Settings ============

// Store patterns for editing
let dlpPatterns = [];

// Load DLP settings
async function loadDlpSettings() {
  try {
    const settings = await invoke('get_dlp_settings');
    dlpPatterns = settings.patterns || [];
    renderPatterns(dlpPatterns);
  } catch (error) {
    console.error('Failed to load DLP settings:', error);
    const container = document.getElementById('dlp-patterns');
    if (container) {
      container.innerHTML = '<p class="empty-text">Failed to load patterns</p>';
    }
  }
}

// Render all patterns (builtin + custom)
function renderPatterns(patterns) {
  const container = document.getElementById('dlp-patterns');
  if (!container) return;

  if (patterns.length === 0) {
    container.innerHTML = '<p class="empty-text">No patterns configured</p>';
    return;
  }

  container.innerHTML = patterns.map(pattern => `
    <div class="dlp-pattern-item" data-id="${pattern.id}">
      <input type="checkbox" class="dlp-checkbox dlp-pattern-toggle" data-id="${pattern.id}" ${pattern.enabled ? 'checked' : ''} />
      <span class="dlp-pattern-name">${escapeHtml(pattern.name)}</span>
      <span class="dlp-pattern-badge ${pattern.is_builtin ? 'builtin' : pattern.pattern_type}">${pattern.is_builtin ? 'Built-in' : pattern.pattern_type}</span>
      ${pattern.min_unique_chars > 0 ? `<span class="dlp-pattern-meta">Unique chars >= ${pattern.min_unique_chars}</span>` : ''}
      <span class="dlp-pattern-meta">Occurrence >= ${pattern.min_occurrences}</span>
      <div class="dlp-pattern-actions">
        <button class="dlp-pattern-edit" data-id="${pattern.id}" title="Edit pattern">
          <i data-lucide="pencil"></i>
        </button>
        ${!pattern.is_builtin ? `
          <button class="dlp-pattern-delete" data-id="${pattern.id}" title="Delete pattern">
            <i data-lucide="trash-2"></i>
          </button>
        ` : ''}
      </div>
    </div>
  `).join('');

  // Re-initialize Lucide icons for new elements
  lucide.createIcons();

  // Add event listeners for toggles
  container.querySelectorAll('.dlp-pattern-toggle').forEach(checkbox => {
    checkbox.addEventListener('change', async (e) => {
      e.stopPropagation();
      const id = parseInt(checkbox.dataset.id);
      try {
        await invoke('toggle_dlp_pattern', { id, enabled: checkbox.checked });
      } catch (error) {
        console.error('Failed to toggle pattern:', error);
        checkbox.checked = !checkbox.checked;
      }
    });
  });

  // Add event listeners for edit buttons
  container.querySelectorAll('.dlp-pattern-edit').forEach(btn => {
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      const id = parseInt(btn.dataset.id);
      const pattern = dlpPatterns.find(p => p.id === id);
      if (pattern) {
        showPatternModal(pattern);
      }
    });
  });

  // Add event listeners for delete buttons
  container.querySelectorAll('.dlp-pattern-delete').forEach(btn => {
    btn.addEventListener('click', async (e) => {
      e.stopPropagation();
      const id = parseInt(btn.dataset.id);
      try {
        await invoke('delete_dlp_pattern', { id });
        await loadDlpSettings();
      } catch (error) {
        console.error('Failed to delete pattern:', error);
      }
    });
  });
}

// Show pattern modal (add or edit)
function showPatternModal(pattern = null) {
  const modal = document.getElementById('pattern-modal');
  const title = document.getElementById('pattern-modal-title');
  const nameInput = document.getElementById('pattern-name');

  // Set title
  title.textContent = pattern ? 'Edit Pattern' : 'Add Pattern';

  // Reset/populate form
  document.getElementById('pattern-id').value = pattern ? pattern.id : '';
  nameInput.value = pattern ? pattern.name : '';
  nameInput.disabled = pattern?.is_builtin || false;

  // Pattern type
  const patternType = pattern?.pattern_type || 'keyword';
  document.querySelector(`input[name="pattern-type"][value="${patternType}"]`).checked = true;

  // Patterns
  document.getElementById('pattern-values').value = pattern?.patterns?.join('\n') || '';

  // Validation
  document.getElementById('min-unique-chars').value = pattern?.min_unique_chars || 0;
  document.getElementById('min-occurrences').value = pattern?.min_occurrences || 1;

  // Negative patterns
  const negType = pattern?.negative_pattern_type || '';
  document.querySelector(`input[name="negative-pattern-type"][value="${negType}"]`).checked = true;
  document.getElementById('negative-pattern-values').value = pattern?.negative_patterns?.join('\n') || '';

  modal.classList.add('show');

  // Focus name input (if not disabled)
  if (!nameInput.disabled) {
    setTimeout(() => nameInput.focus(), 100);
  }
}

// Hide pattern modal
function hidePatternModal() {
  const modal = document.getElementById('pattern-modal');
  modal.classList.remove('show');
  document.getElementById('pattern-name').disabled = false;
  // Clear test results
  const testResults = document.getElementById('test-results');
  if (testResults) {
    testResults.style.display = 'none';
    testResults.innerHTML = '';
  }
  document.getElementById('test-text').value = '';
}

// Parse text lines into array
function parseLines(text) {
  return text
    .split('\n')
    .map(p => p.trim())
    .filter(p => p.length > 0);
}

// Test pattern against sample text
async function testPattern() {
  const testText = document.getElementById('test-text').value;
  const testResults = document.getElementById('test-results');
  const testBtn = document.getElementById('test-pattern-btn');

  if (!testText.trim()) {
    testResults.innerHTML = '<span class="test-error">Enter sample text to test</span>';
    testResults.style.display = 'block';
    return;
  }

  const patternType = document.querySelector('input[name="pattern-type"]:checked').value;
  const patterns = parseLines(document.getElementById('pattern-values').value);
  const minUniqueChars = parseInt(document.getElementById('min-unique-chars').value) || 0;
  const minOccurrences = parseInt(document.getElementById('min-occurrences').value) || 1;
  const negativePatternType = document.querySelector('input[name="negative-pattern-type"]:checked').value || null;
  const negativePatterns = parseLines(document.getElementById('negative-pattern-values').value);

  if (patterns.length === 0) {
    testResults.innerHTML = '<span class="test-error">Add at least one pattern first</span>';
    testResults.style.display = 'block';
    return;
  }

  testBtn.disabled = true;
  testBtn.textContent = 'Testing...';

  try {
    const result = await invoke('test_dlp_pattern', {
      patternType,
      patterns,
      negativePatternType,
      negativePatterns: negativePatterns.length > 0 ? negativePatterns : null,
      minOccurrences,
      minUniqueChars,
      testText
    });

    if (result.excluded) {
      testResults.innerHTML = '<span class="test-excluded">Excluded by negative pattern</span>';
    } else if (result.matches.length === 0) {
      testResults.innerHTML = '<span class="test-none">No matches found</span>';
    } else {
      testResults.innerHTML = `<span class="test-success">Matches (${result.matches.length}):</span> ` +
        result.matches.map(m => `<code>${escapeHtml(m)}</code>`).join(', ');
    }
    testResults.style.display = 'block';
  } catch (error) {
    testResults.innerHTML = `<span class="test-error">Error: ${escapeHtml(error)}</span>`;
    testResults.style.display = 'block';
  } finally {
    testBtn.disabled = false;
    testBtn.textContent = 'Test';
  }
}

// Save pattern (add or update)
async function savePattern() {
  const id = document.getElementById('pattern-id').value;
  const name = document.getElementById('pattern-name').value.trim();
  const patternType = document.querySelector('input[name="pattern-type"]:checked').value;
  const patterns = parseLines(document.getElementById('pattern-values').value);
  const minUniqueChars = parseInt(document.getElementById('min-unique-chars').value) || 0;
  const minOccurrences = parseInt(document.getElementById('min-occurrences').value) || 1;

  const negativePatternType = document.querySelector('input[name="negative-pattern-type"]:checked').value || null;
  const negativePatterns = parseLines(document.getElementById('negative-pattern-values').value);

  // Validation
  if (!name) {
    alert('Please enter a name');
    return;
  }

  if (patterns.length === 0) {
    alert('Please enter at least one pattern');
    return;
  }

  const saveBtn = document.getElementById('save-pattern-btn');
  saveBtn.disabled = true;
  saveBtn.textContent = 'Saving...';

  try {
    if (id) {
      // Update existing pattern
      await invoke('update_dlp_pattern', {
        id: parseInt(id),
        name,
        patternType,
        patterns,
        negativePatternType: negativePatternType || '',
        negativePatterns: negativePatterns.length > 0 ? negativePatterns : [],
        minOccurrences,
        minUniqueChars
      });
    } else {
      // Add new pattern
      await invoke('add_dlp_pattern', {
        name,
        patternType,
        patterns,
        negativePatternType,
        negativePatterns: negativePatterns.length > 0 ? negativePatterns : null,
        minOccurrences,
        minUniqueChars
      });
    }
    hidePatternModal();
    loadDlpSettings();
  } catch (error) {
    alert(`Failed to save: ${error}`);
  } finally {
    saveBtn.disabled = false;
    saveBtn.textContent = 'Save';
  }
}

// Initialize DLP settings
function initDlpSettings() {
  // Add pattern button
  const addPatternBtn = document.getElementById('add-pattern-btn');
  if (addPatternBtn) {
    addPatternBtn.addEventListener('click', () => showPatternModal());
  }

  // Modal close buttons
  const closeModalBtn = document.getElementById('close-pattern-modal');
  const cancelBtn = document.getElementById('cancel-pattern-btn');
  if (closeModalBtn) closeModalBtn.addEventListener('click', hidePatternModal);
  if (cancelBtn) cancelBtn.addEventListener('click', hidePatternModal);

  // Modal save button
  const savePatternBtn = document.getElementById('save-pattern-btn');
  if (savePatternBtn) {
    savePatternBtn.addEventListener('click', savePattern);
  }

  // Test pattern button
  const testPatternBtn = document.getElementById('test-pattern-btn');
  if (testPatternBtn) {
    testPatternBtn.addEventListener('click', testPattern);
  }

  // Close modal on backdrop click
  const modal = document.getElementById('pattern-modal');
  if (modal) {
    modal.addEventListener('click', (e) => {
      if (e.target === modal) hidePatternModal();
    });
  }

  // Close modal on Escape key
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && modal?.classList.contains('show')) {
      hidePatternModal();
    }
  });

  // Load DLP settings
  loadDlpSettings();
}

// ============ Initialize Settings ============

export function initSettings() {
  // Server port settings
  const saveBtn = document.getElementById('save-port-btn');
  const portInput = document.getElementById('port-input');

  if (saveBtn) {
    saveBtn.addEventListener('click', savePortSetting);
  }

  if (portInput) {
    portInput.addEventListener('keypress', (e) => {
      if (e.key === 'Enter') {
        savePortSetting();
      }
    });
  }

  // Port popover open/close
  const cog = document.getElementById('proxy-status-cog');
  if (cog) {
    cog.addEventListener('click', (e) => {
      e.stopPropagation();
      togglePortPopover();
    });
  }
  const popoverClose = document.getElementById('port-popover-close');
  if (popoverClose) {
    popoverClose.addEventListener('click', closePortPopover);
  }
  // Click outside to close
  document.addEventListener('click', (e) => {
    const popover = document.getElementById('port-popover');
    if (!popover || popover.hidden) return;
    if (popover.contains(e.target)) return;
    if (e.target.closest('#proxy-status-cog')) return;
    closePortPopover();
  });
  // Escape to close
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      const popover = document.getElementById('port-popover');
      if (popover && !popover.hidden) closePortPopover();
    }
  });

  // Load settings
  loadPortSetting();

  // Listen for server events (for real-time updates after initial load)
  listen('server-started', (event) => {
    const { port } = event.payload;
    setCurrentPort(port);
    updateServerStatusDisplay(port, false, false);
  });

  listen('server-failed', (event) => {
    const { port } = event.payload;
    updateServerStatusDisplay(port, false, true);
  });

  // Query actual server status after a short delay
  // This handles the race condition where the event fired before we registered listeners
  setTimeout(() => {
    loadServerStatus();
  }, 500);

  // Initialize DLP settings
  initDlpSettings();
}
