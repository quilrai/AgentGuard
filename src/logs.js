import {
  invoke,
  logsTimeRange,
  setLogsTimeRange,
  logsBackend,
  setLogsBackend,
  logsModel,
  setLogsModel,
  logsDlpAction,
  setLogsDlpAction,
  logsSearch,
  setLogsSearch,
  logsPage,
  setLogsPage,
  currentLogs,
  setCurrentLogs,
  formatNumber,
  formatLatency,
  formatRelativeTime,
  shortenModel,
  escapeHtml
} from './utils.js';

// Highlight search terms in text (case-insensitive)
function highlightSearchTerms(text, searchTerm) {
  if (!searchTerm || !searchTerm.trim()) {
    return escapeHtml(text);
  }

  // Escape HTML first
  const escapedText = escapeHtml(text);

  // Escape regex special characters in search term
  const escapedSearchTerm = searchTerm.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

  // Create case-insensitive regex
  const regex = new RegExp(`(${escapedSearchTerm})`, 'gi');

  // Replace matches with highlighted version
  return escapedText.replace(regex, '<mark class="search-highlight">$1</mark>');
}

// Get DLP status info
function getDlpStatus(dlpAction) {
  switch (dlpAction) {
    case 4: return { label: 'Notify-Ratelimit', class: 'notify-ratelimit' };
    case 3: return { label: 'Ratelimited', class: 'ratelimited' };
    case 2: return { label: 'Blocked', class: 'blocked' };
    case 1: return { label: 'Redacted', class: 'redacted' };
    default: return { label: 'Passed', class: 'passed' };
  }
}

// Format JSON string for display
function formatJson(jsonStr) {
  try {
    const parsed = JSON.parse(jsonStr);
    return JSON.stringify(parsed, null, 2);
  } catch {
    return jsonStr || 'null';
  }
}

// Render a single log card
function renderLogCard(log, index, cardNum, total) {
  const status = getDlpStatus(log.dlp_action);

  return `
    <div class="log-card" data-index="${index}">
      <div class="log-card-header">
        <span class="log-number">${cardNum}/${total}</span>
        <span class="log-time">${formatRelativeTime(log.timestamp)}</span>
        <span class="log-pill backend">${log.backend}</span>
        <span class="log-pill model">${shortenModel(log.model)}</span>
        <span class="log-pill status ${status.class}">${status.label}</span>
      </div>
      <div class="log-card-stats">
        <span class="stat"><strong>Latency:</strong> ${formatLatency(log.latency_ms)}</span>
        <span class="stat"><strong>In:</strong> ${formatNumber(log.input_tokens)}</span>
        <span class="stat"><strong>Out:</strong> ${formatNumber(log.output_tokens)}</span>
        ${log.tokens_saved > 0 ? `<span class="stat token-saved"><strong>Saved:</strong> ${formatNumber(log.tokens_saved)}</span>` : ''}
      </div>
      <div class="log-card-tabs">
        <button class="log-tab active" data-tab="data" data-index="${index}">Data</button>
        <button class="log-tab" data-tab="headers" data-index="${index}">Headers</button>
        <button class="log-tab" data-tab="dlp" data-index="${index}">Detections</button>
        <button class="log-tab" data-tab="tools" data-index="${index}">Tool Calls</button>
      </div>
      <div class="log-card-subtabs">
        <button class="log-subtab active" data-subtab="request" data-index="${index}">Request</button>
        <button class="log-subtab" data-subtab="response" data-index="${index}">Response</button>
        <button class="log-copy-btn" data-index="${index}" title="Copy request & response">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
            <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
          </svg>
        </button>
      </div>
      <div class="log-card-content">
        <pre class="log-json" data-index="${index}">${highlightSearchTerms(formatJson(log.request_body), logsSearch)}</pre>
      </div>
    </div>
  `;
}

// Render pagination into static container
function renderPagination(total) {
  const paginationEl = document.getElementById('logs-pagination');
  const totalPages = Math.ceil(total / 10) || 1;
  const currentPage = logsPage + 1;

  paginationEl.innerHTML = `
    <button class="pagination-btn" id="logs-prev" ${logsPage === 0 ? 'disabled' : ''}>Previous</button>
    <span class="pagination-info">Page ${currentPage} of ${totalPages}</span>
    <button class="pagination-btn" id="logs-next" ${currentPage >= totalPages ? 'disabled' : ''}>Next</button>
  `;
}

// Render message logs as cards
function renderLogsCards(logs, total) {
  setCurrentLogs(logs);

  if (logs.length === 0 && logsPage === 0) {
    return `
      <div class="empty-state">
        <h3>No logs yet</h3>
        <p>Make some API requests through the proxy to see logs here.</p>
      </div>
    `;
  }

  const startNum = logsPage * 10 + 1;

  return `
    <div class="logs-grid">
      ${logs.map((log, index) => renderLogCard(log, index, startNum + index, total)).join('')}
    </div>
  `;
}

// Update card content based on current tab/subtab state
async function updateCardContent(card, index) {
  const log = currentLogs[index];
  const activeTab = card.querySelector('.log-tab.active').dataset.tab;
  const activeSubtab = card.querySelector('.log-subtab.active').dataset.subtab;
  const jsonPre = card.querySelector('.log-json');
  const subtabsContainer = card.querySelector('.log-card-subtabs');

  // Show/hide subtabs based on tab type
  if (activeTab === 'dlp') {
    subtabsContainer.style.display = 'none';
    // Fetch and display DLP detections
    try {
      const detections = await invoke('get_dlp_detections_for_request', { requestId: log.id });
      if (detections.length === 0) {
        jsonPre.textContent = 'No detections for this request.';
      } else {
        const formatted = detections.map(d => ({
          pattern: d.pattern_name,
          type: d.pattern_type,
          original: d.original_value,
          replaced_with: d.placeholder,
          message_index: d.message_index
        }));
        jsonPre.textContent = JSON.stringify(formatted, null, 2);
      }
    } catch (err) {
      jsonPre.textContent = 'Error loading detections: ' + err;
    }
  } else if (activeTab === 'tools') {
    subtabsContainer.style.display = 'none';
    // Fetch and display tool calls
    try {
      const toolCalls = await invoke('get_tool_calls_for_request', { requestId: log.id });
      if (toolCalls.length === 0) {
        jsonPre.textContent = 'No tool calls for this request.';
      } else {
        const formatted = toolCalls.map(tc => ({
          name: tc.tool_name,
          id: tc.tool_call_id,
          input: JSON.parse(tc.tool_input || '{}')
        }));
        jsonPre.textContent = JSON.stringify(formatted, null, 2);
      }
    } catch (err) {
      jsonPre.textContent = 'Error loading tool calls: ' + err;
    }
  } else {
    subtabsContainer.style.display = '';
    let content;
    if (activeTab === 'data') {
      content = activeSubtab === 'request' ? log.request_body : log.response_body;
    } else {
      content = activeSubtab === 'request' ? log.request_headers : log.response_headers;
    }
    // Use innerHTML with highlighting for data/headers tabs
    jsonPre.innerHTML = highlightSearchTerms(formatJson(content), logsSearch);
  }
}

// Copy both request and response as tuple
async function copyLogData(index, tab) {
  const log = currentLogs[index];
  let data;

  if (tab === 'data') {
    let request, response;
    try { request = JSON.parse(log.request_body || '{}'); } catch { request = log.request_body; }
    try { response = JSON.parse(log.response_body || '{}'); } catch { response = log.response_body; }
    data = { request, response };
  } else if (tab === 'headers') {
    let request, response;
    try { request = JSON.parse(log.request_headers || '{}'); } catch { request = log.request_headers; }
    try { response = JSON.parse(log.response_headers || '{}'); } catch { response = log.response_headers; }
    data = { request, response };
  } else if (tab === 'dlp') {
    try {
      const detections = await invoke('get_dlp_detections_for_request', { requestId: log.id });
      data = detections.map(d => ({
        pattern: d.pattern_name,
        type: d.pattern_type,
        original: d.original_value,
        replaced_with: d.placeholder,
        message_index: d.message_index
      }));
    } catch {
      data = [];
    }
  } else if (tab === 'tools') {
    try {
      const toolCalls = await invoke('get_tool_calls_for_request', { requestId: log.id });
      data = toolCalls.map(tc => ({
        name: tc.tool_name,
        id: tc.tool_call_id,
        input: JSON.parse(tc.tool_input || '{}')
      }));
    } catch {
      data = [];
    }
  }

  navigator.clipboard.writeText(JSON.stringify(data, null, 2)).then(() => {
    // Brief visual feedback could be added here
  }).catch(err => {
    console.error('Failed to copy:', err);
  });
}

// Scroll to the first search highlight
function scrollToFirstHighlight(container) {
  if (!logsSearch || !logsSearch.trim()) return;

  const firstHighlight = container.querySelector('.search-highlight');
  if (firstHighlight) {
    // Scroll the card's content area to show the highlight
    const cardContent = firstHighlight.closest('.log-card-content');
    if (cardContent) {
      // Scroll within the card content
      firstHighlight.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }

    // Also scroll the card into view in the main container
    const card = firstHighlight.closest('.log-card');
    if (card) {
      card.scrollIntoView({ behavior: 'smooth', block: 'start' });
    }
  }
}

// Attach event handlers to log cards
function attachCardHandlers(container) {
  // Tab switching (Data/Headers)
  container.querySelectorAll('.log-tab').forEach(tab => {
    tab.addEventListener('click', () => {
      const card = tab.closest('.log-card');
      const index = parseInt(tab.dataset.index);

      card.querySelectorAll('.log-tab').forEach(t => t.classList.remove('active'));
      tab.classList.add('active');
      updateCardContent(card, index);
    });
  });

  // Subtab switching (Request/Response)
  container.querySelectorAll('.log-subtab').forEach(subtab => {
    subtab.addEventListener('click', () => {
      const card = subtab.closest('.log-card');
      const index = parseInt(subtab.dataset.index);

      card.querySelectorAll('.log-subtab').forEach(t => t.classList.remove('active'));
      subtab.classList.add('active');
      updateCardContent(card, index);
    });
  });

  // Copy button
  container.querySelectorAll('.log-copy-btn').forEach(btn => {
    btn.addEventListener('click', () => {
      const card = btn.closest('.log-card');
      const index = parseInt(btn.dataset.index);
      const activeTab = card.querySelector('.log-tab.active').dataset.tab;
      copyLogData(index, activeTab);

      // Visual feedback
      btn.classList.add('copied');
      setTimeout(() => btn.classList.remove('copied'), 1000);
    });
  });
}

// Load message logs
export async function loadMessageLogs() {
  const content = document.getElementById('logs-content');
  content.innerHTML = '<p class="loading">Loading...</p>';

  try {
    const result = await invoke('get_message_logs', {
      timeRange: logsTimeRange,
      backend: logsBackend,
      model: logsModel,
      dlpAction: logsDlpAction,
      search: logsSearch,
      page: logsPage
    });
    renderPagination(result.total);
    content.innerHTML = renderLogsCards(result.logs, result.total);
    attachCardHandlers(content);
    attachPaginationHandlers();

    // Scroll to first search match if searching
    scrollToFirstHighlight(content);
  } catch (error) {
    content.innerHTML = `
      <div class="empty-state">
        <h3>Error loading logs</h3>
        <p>${error}</p>
      </div>
    `;
  }
}

// Attach pagination handlers
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

// Load backends for logs tab
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

// Initialize logs backend filter
export function initLogsBackendFilter() {
  const select = document.getElementById('logs-backend-select');
  select.addEventListener('change', () => {
    setLogsBackend(select.value);
    setLogsPage(0);
    loadMessageLogs();
  });
}

// Initialize logs time filter
export function initLogsTimeFilter() {
  const select = document.getElementById('logs-time-select');
  select.addEventListener('change', () => {
    setLogsTimeRange(select.value);
    setLogsPage(0);
    loadMessageLogs();
  });
}

// Load models for logs tab
export async function loadLogsModels() {
  try {
    const models = await invoke('get_models');
    const select = document.getElementById('logs-model-select');
    select.innerHTML = '<option value="all">All Models</option>';
    models.forEach(model => {
      const option = document.createElement('option');
      option.value = model;
      option.textContent = shortenModel(model);
      select.appendChild(option);
    });
  } catch (error) {
    console.error('Failed to load models:', error);
  }
}

// Initialize logs model filter
export function initLogsModelFilter() {
  const select = document.getElementById('logs-model-select');
  select.addEventListener('change', () => {
    setLogsModel(select.value);
    setLogsPage(0);
    loadMessageLogs();
  });
}

// Initialize logs DLP filter
export function initLogsDlpFilter() {
  const select = document.getElementById('logs-dlp-select');
  select.addEventListener('change', () => {
    setLogsDlpAction(select.value);
    setLogsPage(0);
    loadMessageLogs();
  });
}

// Initialize logs search
export function initLogsSearch() {
  const input = document.getElementById('logs-search-input');
  const searchBtn = document.getElementById('logs-search-btn');

  const performSearch = () => {
    setLogsSearch(input.value);
    setLogsPage(0);
    loadMessageLogs();
  };

  // Search on Enter key
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      performSearch();
    }
  });

  // Search on button click
  searchBtn.addEventListener('click', performSearch);
}

// Export logs to JSONL file
export async function exportLogs() {
  const exportBtn = document.getElementById('logs-export-btn');

  try {
    // Show loading state
    exportBtn.disabled = true;
    exportBtn.classList.add('loading');

    // Fetch all logs matching current filters
    const logs = await invoke('export_message_logs', {
      timeRange: logsTimeRange,
      backend: logsBackend,
      model: logsModel,
      dlpAction: logsDlpAction,
      search: logsSearch
    });

    if (logs.length === 0) {
      alert('No logs to export with current filters.');
      return;
    }

    // Convert to JSONL format
    const jsonlContent = logs.map(log => {
      // Parse request/response bodies if they're JSON strings
      let requestBody = log.request_body;
      let responseBody = log.response_body;

      try {
        if (requestBody) requestBody = JSON.parse(requestBody);
      } catch {}
      try {
        if (responseBody) responseBody = JSON.parse(responseBody);
      } catch {}

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

    // Use Tauri dialog to save file
    const { save } = window.__TAURI__.dialog;
    const filePath = await save({
      defaultPath: `logs_export_${new Date().toISOString().slice(0, 10)}.jsonl`,
      filters: [{ name: 'JSONL', extensions: ['jsonl'] }]
    });

    if (filePath) {
      // Write file using Tauri fs
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

// Initialize export button
export function initLogsExport() {
  const exportBtn = document.getElementById('logs-export-btn');
  exportBtn.addEventListener('click', exportLogs);
}
