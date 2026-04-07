import {
  invoke,
  currentTimeRange,
  setCurrentTimeRange,
  currentBackend,
  setCurrentBackend,
  formatNumber,
  colors,
  formatLatency,
  shortenModel
} from './utils.js';
import { destroyCharts, createModelsChart, createTokenChart, createLatencyChart, createDlpChart, createToolInsightsChart } from './charts.js';

// Store chart creation functions for fullscreen recreation
let chartData = {};

// Render dashboard HTML
function renderDashboard(data, dlpStats, toolInsights) {
  const { models, features, token_totals, recent_requests, latency_points } = data;

  const pct = (val) => features.total_requests > 0 ? Math.round((val / features.total_requests) * 100) : 0;

  return `
    <div class="charts-grid">
      <!-- Models Chart -->
      <div class="card">
        <div class="card-header">
          <span>Models Used</span>
          <div class="card-header-actions">
            <span class="badge">${models.length} models</span>
            <button class="expand-btn" data-chart="models" title="Expand"><svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 3 21 3 21 9"></polyline><polyline points="9 21 3 21 3 15"></polyline><line x1="21" y1="3" x2="14" y2="10"></line><line x1="3" y1="21" x2="10" y2="14"></line></svg></button>
          </div>
        </div>
        <div class="card-body chart-container-sm" id="models-chart"></div>
      </div>

      <!-- Token Totals -->
      <div class="card">
        <div class="card-header">Token Totals</div>
        <div class="card-body">
          <div class="token-stats">
            <div class="token-stat input">
              <div class="value">${formatNumber(token_totals.input)}</div>
              <div class="label">Input</div>
            </div>
            <div class="token-stat output">
              <div class="value">${formatNumber(token_totals.output)}</div>
              <div class="label">Output</div>
            </div>
            <div class="token-stat cache-read">
              <div class="value">${formatNumber(token_totals.cache_read)}</div>
              <div class="label">Cache Read</div>
            </div>
            <div class="token-stat cache-create">
              <div class="value">${formatNumber(token_totals.cache_creation)}</div>
              <div class="label">Cache Create</div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div class="charts-grid">
      <!-- Request Features -->
      <div class="card">
        <div class="card-header">Request Features</div>
        <div class="card-body">
          <div class="feature-bars">
            <div class="feature-bar">
              <div class="feature-label">
                <span class="feature-name">System Prompt</span>
                <span class="feature-value">${features.with_system_prompt} (${pct(features.with_system_prompt)}%)</span>
              </div>
              <div class="bar-track">
                <div class="bar-fill system" style="width: ${pct(features.with_system_prompt)}%"></div>
              </div>
            </div>
            <div class="feature-bar">
              <div class="feature-label">
                <span class="feature-name">Tools</span>
                <span class="feature-value">${features.with_tools} (${pct(features.with_tools)}%)</span>
              </div>
              <div class="bar-track">
                <div class="bar-fill tools" style="width: ${pct(features.with_tools)}%"></div>
              </div>
            </div>
            <div class="feature-bar">
              <div class="feature-label">
                <span class="feature-name">Thinking</span>
                <span class="feature-value">${features.with_thinking} (${pct(features.with_thinking)}%)</span>
              </div>
              <div class="bar-track">
                <div class="bar-fill thinking" style="width: ${pct(features.with_thinking)}%"></div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Latency Chart -->
      <div class="card">
        <div class="card-header">
          <span>Latency Trend</span>
          <div class="card-header-actions">
            <span class="badge">${latency_points.length} requests</span>
            <button class="expand-btn" data-chart="latency" title="Expand"><svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 3 21 3 21 9"></polyline><polyline points="9 21 3 21 3 15"></polyline><line x1="21" y1="3" x2="14" y2="10"></line><line x1="3" y1="21" x2="10" y2="14"></line></svg></button>
          </div>
        </div>
        <div class="card-body chart-container-sm" id="latency-chart"></div>
      </div>
    </div>

    <!-- Token Usage Per Request -->
    <div class="charts-grid">
      <div class="card full-width">
        <div class="card-header">
          <span>Token Usage Per Request</span>
          <div class="card-header-actions">
            <span class="badge">${recent_requests.length} requests</span>
            <button class="expand-btn" data-chart="tokens" title="Expand"><svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 3 21 3 21 9"></polyline><polyline points="9 21 3 21 3 15"></polyline><line x1="21" y1="3" x2="14" y2="10"></line><line x1="3" y1="21" x2="10" y2="14"></line></svg></button>
          </div>
        </div>
        <div class="card-body chart-container" id="token-chart"></div>
      </div>
    </div>

    <!-- Tool Insights -->
    <div class="charts-grid">
      <div class="card full-width">
        <div class="card-header">
          <span>Tool Insights</span>
          <div class="card-header-actions">
            <span class="badge">${toolInsights.tools.reduce((sum, t) => sum + t.count, 0)} calls / ${toolInsights.tools.length} tools</span>
            <button class="expand-btn" data-chart="toolInsights" title="Expand"><svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 3 21 3 21 9"></polyline><polyline points="9 21 3 21 3 15"></polyline><line x1="21" y1="3" x2="14" y2="10"></line><line x1="3" y1="21" x2="10" y2="14"></line></svg></button>
          </div>
        </div>
        <div class="card-body chart-container-lg" id="tool-insights-chart">
          ${toolInsights.tools.length === 0 ? '<p class="empty-text">No tool calls</p>' : ''}
        </div>
      </div>
    </div>

    <!-- Detections -->
    <div class="charts-grid">
      <div class="card full-width">
        <div class="card-header">
          <span>Detections</span>
          <div class="card-header-actions">
            <button class="expand-btn" data-chart="dlp" title="Expand"><svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 3 21 3 21 9"></polyline><polyline points="9 21 3 21 3 15"></polyline><line x1="21" y1="3" x2="14" y2="10"></line><line x1="3" y1="21" x2="10" y2="14"></line></svg></button>
          </div>
        </div>
        <div class="card-body">
          <div class="dlp-chart-container" id="dlp-chart">
            ${(!dlpStats || dlpStats.detections_by_pattern.length === 0) ? '<p class="empty-text">No detections</p>' : ''}
          </div>
        </div>
      </div>
    </div>

    <!-- Fullscreen Chart Modal -->
    <div class="chart-fullscreen-modal" id="chart-fullscreen-modal">
      <div class="chart-fullscreen-content">
        <div class="chart-fullscreen-header">
          <h3 id="chart-fullscreen-title">Chart</h3>
          <button class="chart-fullscreen-close" id="chart-fullscreen-close">&times;</button>
        </div>
        <div class="chart-fullscreen-body" id="chart-fullscreen-body"></div>
      </div>
    </div>
  `;
}

// Load dashboard
export async function loadDashboard() {
  const content = document.getElementById('dashboard-content');
  content.innerHTML = '<p class="loading">Loading...</p>';

  destroyCharts();

  try {
    // Load dashboard stats, DLP stats, and tool insights in parallel
    const [data, dlpStats, toolInsights] = await Promise.all([
      invoke('get_dashboard_stats', { timeRange: currentTimeRange, backend: currentBackend }),
      invoke('get_dlp_detection_stats', { timeRange: currentTimeRange, backend: currentBackend }),
      invoke('get_tool_call_insights', { timeRange: currentTimeRange, backend: currentBackend })
    ]);

    if (data.total_requests === 0 && dlpStats.total_detections === 0 && toolInsights.tools.length === 0) {
      content.innerHTML = `
        <div class="empty-state">
          <h3>No data yet</h3>
          <p>Make some API requests through the proxy to see stats here.</p>
          <p style="margin-top: 12px; font-family: monospace; font-size: 0.85rem; color: #666;">
            Proxy: http://localhost:8008
          </p>
        </div>
      `;
      return;
    }

    content.innerHTML = renderDashboard(data, dlpStats, toolInsights);

    // Store chart data for fullscreen recreation
    chartData = {
      models: data.models,
      requests: data.recent_requests,
      latencyPoints: data.latency_points,
      toolInsights: toolInsights,
      dlpStats: dlpStats
    };

    // Create charts after DOM is updated
    setTimeout(() => {
      if (data.models.length > 0) {
        createModelsChart(document.getElementById('models-chart'), data.models);
      }
      if (data.recent_requests.length > 0) {
        createTokenChart(document.getElementById('token-chart'), data.recent_requests);
      }
      if (data.latency_points.length > 0) {
        createLatencyChart(document.getElementById('latency-chart'), data.latency_points);
      }
      if (toolInsights.tools.length > 0) {
        createToolInsightsChart(document.getElementById('tool-insights-chart'), toolInsights);
      }
      if (dlpStats && dlpStats.detections_by_pattern.length > 0) {
        createDlpChart(document.getElementById('dlp-chart'), dlpStats.detections_by_pattern);
      }

      // Setup expand button handlers
      setupExpandButtons();
    }, 0);

  } catch (error) {
    content.innerHTML = `
      <div class="empty-state">
        <h3>Error loading stats</h3>
        <p>${error}</p>
      </div>
    `;
  }
}

// Load available backends
export async function loadBackends() {
  try {
    const backends = await invoke('get_backends');
    const select = document.getElementById('backend-select');
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

// Initialize backend filter
export function initBackendFilter() {
  const select = document.getElementById('backend-select');
  select.addEventListener('change', () => {
    setCurrentBackend(select.value);
    loadDashboard();
  });
}

// Initialize time filter
export function initTimeFilter() {
  const select = document.getElementById('time-select');
  select.addEventListener('change', () => {
    setCurrentTimeRange(select.value);
    loadDashboard();
  });
}

// Chart title mapping
const chartTitles = {
  models: 'Models Used',
  tokens: 'Token Usage Per Request',
  latency: 'Latency Trend',
  toolInsights: 'Tool Insights',
  dlp: 'Detections'
};

// Fullscreen chart instance
let fullscreenChart = null;

// Setup expand button handlers
function setupExpandButtons() {
  const expandButtons = document.querySelectorAll('.expand-btn');
  const modal = document.getElementById('chart-fullscreen-modal');
  const closeBtn = document.getElementById('chart-fullscreen-close');
  const body = document.getElementById('chart-fullscreen-body');
  const title = document.getElementById('chart-fullscreen-title');

  expandButtons.forEach(btn => {
    btn.addEventListener('click', () => {
      const chartType = btn.dataset.chart;
      openFullscreenChart(chartType, modal, body, title);
    });
  });

  // Close modal handlers
  closeBtn.addEventListener('click', () => closeFullscreenModal(modal, body));
  modal.addEventListener('click', (e) => {
    if (e.target === modal) closeFullscreenModal(modal, body);
  });

  // ESC key to close
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && modal.classList.contains('show')) {
      closeFullscreenModal(modal, body);
    }
  });
}

// Open fullscreen chart
function openFullscreenChart(chartType, modal, body, titleEl) {
  titleEl.textContent = chartTitles[chartType] || 'Chart';
  body.innerHTML = '';

  const canvas = document.createElement('canvas');
  body.appendChild(canvas);

  modal.classList.add('show');

  // Create chart after modal is visible
  setTimeout(() => {
    switch (chartType) {
      case 'models':
        if (chartData.models?.length > 0) {
          fullscreenChart = createFullscreenModelsChart(canvas, chartData.models);
        }
        break;
      case 'tokens':
        if (chartData.requests?.length > 0) {
          fullscreenChart = createFullscreenTokenChart(canvas, chartData.requests);
        }
        break;
      case 'latency':
        if (chartData.latencyPoints?.length > 0) {
          fullscreenChart = createFullscreenLatencyChart(canvas, chartData.latencyPoints);
        }
        break;
      case 'toolInsights':
        if (chartData.toolInsights?.tools?.length > 0) {
          fullscreenChart = createFullscreenToolInsightsChart(canvas, chartData.toolInsights);
        }
        break;
      case 'dlp':
        if (chartData.dlpStats?.detections_by_pattern?.length > 0) {
          fullscreenChart = createFullscreenDlpChart(canvas, chartData.dlpStats.detections_by_pattern);
        }
        break;
    }
  }, 50);
}

// Close fullscreen modal
function closeFullscreenModal(modal, body) {
  modal.classList.remove('show');
  if (fullscreenChart) {
    fullscreenChart.destroy();
    fullscreenChart = null;
  }
  body.innerHTML = '';
}

// Fullscreen chart creation functions (duplicated with larger dimensions)
const dlpColors = [
  colors.primary, colors.secondary, colors.warning, colors.pink, colors.blue,
  '#8b5cf6', '#14b8a6', '#f97316', '#ef4444', '#84cc16'
];

function createFullscreenModelsChart(canvas, models) {
  const data = models.slice(0, 10);
  const labels = data.map(m => shortenModel(m.model));
  const values = data.map(m => m.count);

  return new Chart(canvas, {
    type: 'bar',
    data: {
      labels,
      datasets: [{
        data: values,
        backgroundColor: dlpColors.slice(0, data.length),
        borderRadius: 6,
        barThickness: 32,
      }]
    },
    options: {
      indexAxis: 'y',
      responsive: true,
      maintainAspectRatio: false,
      plugins: { legend: { display: false } },
      scales: {
        x: { grid: { display: false }, ticks: { font: { size: 14 } } },
        y: { grid: { display: false }, ticks: { font: { size: 14 } } }
      }
    }
  });
}

function createFullscreenTokenChart(canvas, requests) {
  const data = [...requests].reverse();
  const labels = data.map((_, i) => `#${i + 1}`);

  return new Chart(canvas, {
    type: 'bar',
    data: {
      labels,
      datasets: [
        { label: 'Input', data: data.map(r => r.input_tokens), backgroundColor: colors.primary, borderRadius: 4 },
        { label: 'Output', data: data.map(r => r.output_tokens), backgroundColor: colors.secondary, borderRadius: 4 },
        { label: 'Cache Read', data: data.map(r => r.cache_read_tokens), backgroundColor: colors.warning, borderRadius: 4 }
      ]
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      plugins: {
        legend: { position: 'top', labels: { boxWidth: 14, padding: 20, font: { size: 14 } } }
      },
      scales: {
        x: { stacked: true, grid: { display: false }, ticks: { font: { size: 12 } } },
        y: { stacked: true, grid: { color: 'rgba(113, 208, 131, 0.08)' }, ticks: { font: { size: 12 }, callback: v => formatNumber(v) } }
      }
    }
  });
}

function createFullscreenLatencyChart(canvas, latencyPoints) {
  const data = [...latencyPoints].reverse();
  const labels = data.map((_, i) => i + 1);
  const values = data.map(p => p.latency_ms);

  return new Chart(canvas, {
    type: 'line',
    data: {
      labels,
      datasets: [{
        label: 'Latency (ms)',
        data: values,
        borderColor: colors.primary,
        backgroundColor: 'rgba(113, 208, 131, 0.1)',
        fill: true,
        tension: 0.3,
        pointRadius: 3,
        pointHoverRadius: 6,
      }]
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      plugins: { legend: { display: false } },
      scales: {
        x: { grid: { display: false }, ticks: { font: { size: 12 } } },
        y: { grid: { color: 'rgba(113, 208, 131, 0.08)' }, ticks: { font: { size: 12 }, callback: v => formatLatency(v) } }
      }
    }
  });
}

function createFullscreenToolInsightsChart(canvas, insights) {
  const { tools } = insights;
  const innerLabels = tools.map(t => t.tool_name);
  const innerValues = tools.map(t => t.count);
  const innerColors = tools.map((_, i) => dlpColors[i % dlpColors.length]);

  const outerLabels = [];
  const outerValues = [];
  const outerColors = [];
  const outerMeta = [];

  tools.forEach((tool, toolIndex) => {
    const baseColor = dlpColors[toolIndex % dlpColors.length];
    if (tool.targets.length === 0) {
      outerLabels.push('other');
      outerValues.push(tool.count);
      outerColors.push(baseColor + 'AA');
      outerMeta.push({ tool: tool.tool_name, target: 'other' });
    } else {
      let targetSum = 0;
      tool.targets.forEach((target, targetIndex) => {
        outerLabels.push(target.target);
        outerValues.push(target.count);
        const opacity = Math.max(60, 99 - targetIndex * 15).toString(16).padStart(2, '0');
        outerColors.push(baseColor + opacity);
        outerMeta.push({ tool: tool.tool_name, target: target.target });
        targetSum += target.count;
      });
      const remaining = tool.count - targetSum;
      if (remaining > 0) {
        outerLabels.push('other');
        outerValues.push(remaining);
        outerColors.push(baseColor + '40');
        outerMeta.push({ tool: tool.tool_name, target: 'other' });
      }
    }
  });

  const labelPlugin = {
    id: 'segmentLabels',
    afterDraw: (chart) => {
      const ctx = chart.ctx;
      ctx.save();
      chart.data.datasets.forEach((dataset, datasetIndex) => {
        const meta = chart.getDatasetMeta(datasetIndex);
        meta.data.forEach((arc, index) => {
          const { x, y, startAngle, endAngle, innerRadius, outerRadius } = arc.getProps(['x', 'y', 'startAngle', 'endAngle', 'innerRadius', 'outerRadius']);
          const angleSpan = endAngle - startAngle;
          const midAngle = startAngle + angleSpan / 2;
          if (angleSpan < 0.26) return;
          const midRadius = (innerRadius + outerRadius) / 2;
          const labelX = x + Math.cos(midAngle) * midRadius;
          const labelY = y + Math.sin(midAngle) * midRadius;
          let label = datasetIndex === 1 ? innerLabels[index] : outerLabels[index];
          const arcLength = angleSpan * midRadius;
          const fontSize = datasetIndex === 1 ? 13 : 11;
          ctx.font = `${fontSize}px -apple-system, BlinkMacSystemFont, sans-serif`;
          const maxChars = Math.floor(arcLength / (fontSize * 0.6));
          if (label.length > maxChars && maxChars > 3) {
            label = label.substring(0, maxChars - 2) + '..';
          } else if (maxChars <= 3) return;
          ctx.textAlign = 'center';
          ctx.textBaseline = 'middle';
          ctx.fillStyle = '#e8e8e8';
          ctx.shadowColor = 'rgba(0, 0, 0, 0.5)';
          ctx.shadowBlur = 3;
          ctx.fillText(label, labelX, labelY);
        });
      });
      ctx.restore();
    }
  };

  return new Chart(canvas, {
    type: 'doughnut',
    data: {
      datasets: [
        { label: 'Targets', data: outerValues, backgroundColor: outerColors, borderWidth: 1, borderColor: '#1D2021' },
        { label: 'Tools', data: innerValues, backgroundColor: innerColors, borderWidth: 2, borderColor: '#1D2021' }
      ]
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      cutout: '30%',
      plugins: {
        legend: {
          display: true,
          position: 'right',
          labels: {
            boxWidth: 14,
            padding: 12,
            font: { size: 13 },
            color: '#aaa',
            generateLabels: () => tools.map((tool, i) => ({
              text: `${tool.tool_name} (${tool.count})`,
              fillStyle: dlpColors[i % dlpColors.length],
              strokeStyle: '#1D2021',
              fontColor: '#aaa',
              lineWidth: 1,
              index: i
            }))
          }
        },
        tooltip: {
          callbacks: {
            label: (context) => {
              if (context.datasetIndex === 1) return `${tools[context.dataIndex].tool_name}: ${tools[context.dataIndex].count} calls`;
              const meta = outerMeta[context.dataIndex];
              return `${meta.tool} → ${meta.target}: ${outerValues[context.dataIndex]}`;
            }
          }
        }
      }
    },
    plugins: [labelPlugin]
  });
}

function createFullscreenDlpChart(canvas, detectionsByPattern) {
  const labels = detectionsByPattern.map(p => p.pattern_name);
  const values = detectionsByPattern.map(p => p.count);
  const backgroundColors = detectionsByPattern.map((_, i) => dlpColors[i % dlpColors.length]);

  return new Chart(canvas, {
    type: 'doughnut',
    data: {
      labels,
      datasets: [{ data: values, backgroundColor: backgroundColors, borderWidth: 2, borderColor: '#1D2021' }]
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      plugins: {
        legend: {
          position: 'right',
          labels: {
            boxWidth: 16,
            padding: 16,
            font: { size: 14 },
            generateLabels: (chart) => chart.data.labels.map((label, i) => ({
              text: `${label} (${chart.data.datasets[0].data[i]})`,
              fillStyle: chart.data.datasets[0].backgroundColor[i],
              strokeStyle: '#1D2021',
              lineWidth: 2,
              index: i
            }))
          }
        },
        tooltip: {
          callbacks: {
            label: (context) => {
              const total = context.dataset.data.reduce((a, b) => a + b, 0);
              const pct = Math.round((context.parsed / total) * 100);
              return `${context.label}: ${context.parsed} (${pct}%)`;
            }
          }
        }
      }
    }
  });
}
