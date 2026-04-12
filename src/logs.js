import {
  invoke,
  logsTimeRange,
  setLogsTimeRange,
  logsBackend,
  setLogsBackend,
  logsPage,
  setLogsPage,
  logsView,
  setLogsView,
  currentLogs,
  setCurrentLogs,
  formatNumber,
  formatRelativeTime,
  escapeHtml
} from './utils.js';

// ============================================================================
// Token Saving View
// ============================================================================

function getTokenSavingMethod(meta) {
  if (!meta) return 'unknown';
  try {
    const parsed = typeof meta === 'string' ? JSON.parse(meta) : meta;
    const keys = Object.keys(parsed);
    if (keys.length > 0) return keys[0];
  } catch {}
  return 'unknown';
}

function formatMethodLabel(method) {
  switch (method) {
    case 'shell_compression': return 'Shell Compression';
    case 'ctx_read': return 'Context Read';
    case 'ctx_smart_read': return 'Smart Read';
    default: return method;
  }
}

function getMethodIcon(method) {
  switch (method) {
    case 'shell_compression': return 'terminal';
    case 'ctx_read': return 'file-text';
    case 'ctx_smart_read': return 'brain';
    default: return 'zap';
  }
}

function truncateContent(text, maxLen) {
  if (!text) return '';
  if (text.length <= maxLen) return text;
  return text.slice(0, maxLen) + '\n... (' + formatNumber(text.length - maxLen) + ' chars truncated)';
}

function renderTokenSavingRow(log, index, cardNum, total) {
  const method = getTokenSavingMethod(log.token_saving_meta);
  const inputTokens = log.input_tokens || 0;
  const outputTokens = log.output_tokens || 0;
  const saved = log.tokens_saved || 0;
  const pct = inputTokens > 0 ? ((saved / inputTokens) * 100).toFixed(1) : '0.0';
  const colSpan = 8;

  return `
    <tr class="ts-row" data-ts-index="${index}">
      <td class="ts-cell ts-num">${cardNum}</td>
      <td class="ts-cell ts-time">${formatRelativeTime(log.timestamp)}</td>
      <td class="ts-cell ts-backend">${escapeHtml(log.backend)}</td>
      <td class="ts-cell ts-method">
        <span class="ts-method-badge" data-method="${method}">
          <i data-lucide="${getMethodIcon(method)}"></i>
          ${formatMethodLabel(method)}
        </span>
      </td>
      <td class="ts-cell ts-tokens">${formatNumber(inputTokens)}</td>
      <td class="ts-cell ts-tokens">${formatNumber(outputTokens)}</td>
      <td class="ts-cell ts-saved">${formatNumber(saved)}</td>
      <td class="ts-cell ts-pct">
        <div class="ts-pct-bar-wrap">
          <div class="ts-pct-bar" style="width: ${Math.min(pct, 100)}%"></div>
          <span class="ts-pct-label">${pct}%</span>
        </div>
      </td>
    </tr>
    <tr class="ts-detail-row" id="ts-detail-${index}" style="display: none;">
      <td colspan="${colSpan}" class="ts-detail-cell">
        <div class="ts-detail-panels">
          <div class="ts-detail-panel">
            <div class="ts-detail-label">
              <i data-lucide="file-input"></i>
              Original <span class="ts-detail-tokens">${formatNumber(inputTokens)} tokens</span>
            </div>
            <pre class="ts-detail-pre">${escapeHtml(truncateContent(log.request_body, 3000))}</pre>
          </div>
          <div class="ts-detail-arrow">
            <i data-lucide="arrow-right"></i>
          </div>
          <div class="ts-detail-panel">
            <div class="ts-detail-label">
              <i data-lucide="file-output"></i>
              Compressed <span class="ts-detail-tokens ts-detail-tokens-saved">${formatNumber(outputTokens)} tokens</span>
            </div>
            <pre class="ts-detail-pre">${escapeHtml(truncateContent(log.response_body, 3000))}</pre>
          </div>
        </div>
      </td>
    </tr>
  `;
}

function renderTokenSavingView(logs, total) {
  if (logs.length === 0 && logsPage === 0) {
    return `
      <div class="empty-state">
        <i data-lucide="zap-off"></i>
        <h3>No token saving events yet</h3>
        <p>Enable shell compression or context-aware reading to start saving tokens.</p>
      </div>
    `;
  }

  const startNum = logsPage * 10 + 1;

  return `
    <div class="ts-table-wrap">
      <table class="ts-table">
        <thead>
          <tr>
            <th class="ts-th">#</th>
            <th class="ts-th">Time</th>
            <th class="ts-th">Backend</th>
            <th class="ts-th">Method</th>
            <th class="ts-th">Input</th>
            <th class="ts-th">Output</th>
            <th class="ts-th">Saved</th>
            <th class="ts-th">Reduction</th>
          </tr>
        </thead>
        <tbody>
          ${logs.map((log, i) => renderTokenSavingRow(log, i, startNum + i, total)).join('')}
        </tbody>
      </table>
    </div>
  `;
}

// ============================================================================
// Guardian Agent View
// ============================================================================

function getGuardianAction(dlpAction) {
  switch (dlpAction) {
    case 4: return { label: 'Notify + Ratelimit', class: 'notify-ratelimit', icon: 'bell' };
    case 3: return { label: 'Ratelimited', class: 'ratelimited', icon: 'clock' };
    case 2: return { label: 'Blocked', class: 'blocked', icon: 'shield-x' };
    case 1: return { label: 'Redacted', class: 'redacted', icon: 'eye-off' };
    default: return { label: 'Passed', class: 'passed', icon: 'check' };
  }
}

function extractGuardianReason(log) {
  // Try to extract the reason from request/response bodies
  try {
    const resp = JSON.parse(log.response_body || '{}');
    // Claude hook responses
    if (resp.hook_specific_output?.permission_decision_reason) {
      return resp.hook_specific_output.permission_decision_reason;
    }
    // Prompt submit responses
    if (resp.reason) return resp.reason;
    if (resp.decision) return `Decision: ${resp.decision}`;
  } catch {}
  return null;
}

function extractToolName(log) {
  try {
    const req = JSON.parse(log.request_body || '{}');
    return req.tool_name || null;
  } catch {}
  return null;
}

function renderGuardianCard(log, index, cardNum, total) {
  const action = getGuardianAction(log.dlp_action);
  const reason = extractGuardianReason(log);
  const toolName = extractToolName(log);

  return `
    <div class="guardian-card" data-index="${index}">
      <div class="guardian-card-header">
        <span class="guardian-num">${cardNum}/${total}</span>
        <span class="guardian-time">${formatRelativeTime(log.timestamp)}</span>
        <span class="guardian-pill backend">${escapeHtml(log.backend)}</span>
        ${toolName ? `<span class="guardian-pill tool">${escapeHtml(toolName)}</span>` : ''}
        <span class="guardian-pill action ${action.class}">
          <i data-lucide="${action.icon}"></i>
          ${action.label}
        </span>
      </div>
      ${reason ? `<div class="guardian-reason">${escapeHtml(reason)}</div>` : ''}
      <div class="guardian-details" id="guardian-details-${index}">
        <div class="guardian-details-loading">Loading...</div>
      </div>
    </div>
  `;
}

function renderGuardianView(logs, total) {
  if (logs.length === 0 && logsPage === 0) {
    return `
      <div class="empty-state">
        <i data-lucide="shield-check"></i>
        <h3>No guardian actions yet</h3>
        <p>The guardian agent will log actions here when it blocks, redacts, or rate-limits requests.</p>
      </div>
    `;
  }

  const startNum = logsPage * 10 + 1;

  return `
    <div class="guardian-list">
      ${logs.map((log, i) => renderGuardianCard(log, i, startNum + i, total)).join('')}
    </div>
  `;
}

async function loadGuardianDetections(index) {
  const log = currentLogs[index];
  const container = document.getElementById(`guardian-details-${index}`);
  if (!container) return;

  try {
    const detections = await invoke('get_dlp_detections_for_request', { requestId: log.id });
    if (detections.length === 0) {
      container.innerHTML = '<div class="guardian-no-detections">No pattern detections recorded for this action.</div>';
    } else {
      container.innerHTML = `
        <table class="guardian-detections-table">
          <thead>
            <tr>
              <th>Pattern</th>
              <th>Type</th>
              <th>Matched Text</th>
            </tr>
          </thead>
          <tbody>
            ${detections.map(d => `
              <tr>
                <td class="gd-pattern">${escapeHtml(d.pattern_name)}</td>
                <td class="gd-type"><span class="gd-type-badge">${escapeHtml(d.pattern_type)}</span></td>
                <td class="gd-value"><code>${escapeHtml(d.original_value)}</code></td>
              </tr>
            `).join('')}
          </tbody>
        </table>
      `;
    }
  } catch (err) {
    container.innerHTML = `<div class="guardian-error">Error loading detections: ${escapeHtml(String(err))}</div>`;
  }
}

// ============================================================================
// Shared rendering & event handlers
// ============================================================================

function renderPagination(total) {
  const paginationEl = document.getElementById('logs-pagination');
  const totalPages = Math.ceil(total / 10) || 1;
  const currentPage = logsPage + 1;

  paginationEl.innerHTML = `
    <button class="pagination-btn" id="logs-prev" ${logsPage === 0 ? 'disabled' : ''}>Previous</button>
    <span class="pagination-info">Page ${currentPage} of ${totalPages} (${total} entries)</span>
    <button class="pagination-btn" id="logs-next" ${currentPage >= totalPages ? 'disabled' : ''}>Next</button>
  `;
}

function attachTokenSavingHandlers(container) {
  container.querySelectorAll('.ts-row').forEach(row => {
    row.addEventListener('click', () => {
      const index = row.dataset.tsIndex;
      const detail = document.getElementById(`ts-detail-${index}`);
      if (!detail) return;

      const isVisible = detail.style.display !== 'none';
      // Collapse all other open details
      container.querySelectorAll('.ts-detail-row').forEach(r => {
        r.style.display = 'none';
      });
      container.querySelectorAll('.ts-row').forEach(r => r.classList.remove('ts-row-expanded'));

      if (!isVisible) {
        detail.style.display = 'table-row';
        row.classList.add('ts-row-expanded');
        // Re-render lucide icons inside the newly shown detail
        if (window.lucide) window.lucide.createIcons();
      }
    });
  });
}

function attachGuardianHandlers(container) {
  // Auto-load detections for all visible cards
  container.querySelectorAll('.guardian-card').forEach(card => {
    const index = parseInt(card.dataset.index);
    loadGuardianDetections(index);
  });
}

function attachPaginationHandlers() {
  const paginationEl = document.getElementById('logs-pagination');
  const prevBtn = paginationEl.querySelector('#logs-prev');
  const nextBtn = paginationEl.querySelector('#logs-next');

  if (prevBtn) {
    prevBtn.addEventListener('click', () => {
      if (logsPage > 0) {
        setLogsPage(logsPage - 1);
        loadMessageLogs();
      }
    });
  }

  if (nextBtn) {
    nextBtn.addEventListener('click', () => {
      setLogsPage(logsPage + 1);
      loadMessageLogs();
    });
  }
}

// ============================================================================
// Main loader
// ============================================================================

export async function loadMessageLogs() {
  const content = document.getElementById('logs-content');
  content.innerHTML = '<p class="loading">Loading...</p>';

  try {
    const result = await invoke('get_message_logs', {
      timeRange: logsTimeRange,
      backend: logsBackend,
      model: 'all',
      dlpAction: 'all',
      search: '',
      page: logsPage,
      view: logsView,
    });

    setCurrentLogs(result.logs);
    renderPagination(result.total);

    if (logsView === 'token_saving') {
      content.innerHTML = renderTokenSavingView(result.logs, result.total);
      attachTokenSavingHandlers(content);
    } else {
      content.innerHTML = renderGuardianView(result.logs, result.total);
      attachGuardianHandlers(content);
    }

    attachPaginationHandlers();

    // Re-render lucide icons for dynamically added content
    if (window.lucide) window.lucide.createIcons();
  } catch (error) {
    content.innerHTML = `
      <div class="empty-state">
        <h3>Error loading logs</h3>
        <p>${error}</p>
      </div>
    `;
  }
}

// ============================================================================
// Initialization
// ============================================================================

export async function loadLogsBackends() {
  try {
    const backends = await invoke('get_backends');
    const select = document.getElementById('logs-backend-select');
    select.innerHTML = '<option value="all">All Backends</option>';
    backends.forEach(backend => {
      const option = document.createElement('option');
      option.value = backend;
      option.textContent = backend.charAt(0).toUpperCase() + backend.slice(1);
      select.appendChild(option);
    });
  } catch (error) {
    console.error('Failed to load backends:', error);
  }
}

export function initLogsBackendFilter() {
  const select = document.getElementById('logs-backend-select');
  select.addEventListener('change', () => {
    setLogsBackend(select.value);
    setLogsPage(0);
    loadMessageLogs();
  });
}

export function initLogsTimeFilter() {
  const select = document.getElementById('logs-time-select');
  select.addEventListener('change', () => {
    setLogsTimeRange(select.value);
    setLogsPage(0);
    loadMessageLogs();
  });
}

export function initLogsViewTabs() {
  document.querySelectorAll('.logs-view-tab').forEach(tab => {
    tab.addEventListener('click', () => {
      document.querySelectorAll('.logs-view-tab').forEach(t => t.classList.remove('active'));
      tab.classList.add('active');
      setLogsView(tab.dataset.view);
      setLogsPage(0);
      loadMessageLogs();
    });
  });
}

// Export logs to JSONL file
export async function exportLogs() {
  const exportBtn = document.getElementById('logs-export-btn');

  try {
    exportBtn.disabled = true;
    exportBtn.classList.add('loading');

    const logs = await invoke('export_message_logs', {
      timeRange: logsTimeRange,
      backend: logsBackend,
      model: 'all',
      dlpAction: 'all',
      search: ''
    });

    if (logs.length === 0) {
      alert('No logs to export with current filters.');
      return;
    }

    const jsonlContent = logs.map(log => {
      let requestBody = log.request_body;
      let responseBody = log.response_body;
      try { if (requestBody) requestBody = JSON.parse(requestBody); } catch {}
      try { if (responseBody) responseBody = JSON.parse(responseBody); } catch {}

      let tokenSavingMeta = null;
      if (log.token_saving_meta) {
        try { tokenSavingMeta = JSON.parse(log.token_saving_meta); } catch { tokenSavingMeta = log.token_saving_meta; }
      }
      return JSON.stringify({
        id: log.id,
        timestamp: log.timestamp,
        backend: log.backend,
        model: log.model,
        input_tokens: log.input_tokens,
        output_tokens: log.output_tokens,
        latency_ms: log.latency_ms,
        dlp_action: log.dlp_action,
        tokens_saved: log.tokens_saved || 0,
        token_saving_meta: tokenSavingMeta,
        request: requestBody,
        response: responseBody
      });
    }).join('\n');

    const { save } = window.__TAURI__.dialog;
    const filePath = await save({
      defaultPath: `logs_export_${new Date().toISOString().slice(0, 10)}.jsonl`,
      filters: [{ name: 'JSONL', extensions: ['jsonl'] }]
    });

    if (filePath) {
      const { writeTextFile } = window.__TAURI__.fs;
      await writeTextFile(filePath, jsonlContent);
      alert(`Exported ${logs.length} logs to ${filePath}`);
    }
  } catch (error) {
    console.error('Export failed:', error);
    alert('Export failed: ' + error);
  } finally {
    exportBtn.disabled = false;
    exportBtn.classList.remove('loading');
  }
}

export function initLogsExport() {
  const exportBtn = document.getElementById('logs-export-btn');
  exportBtn.addEventListener('click', exportLogs);
}
