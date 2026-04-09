// Garden — each project is its own garden.
//
// View 1: Project picker — cards showing all projects with mini previews.
// View 2: Garden scene — rich SVG scene for one project:
//   - Trees     = sessions (height = output tokens)
//   - Pond      = input tokens (size = total input)
//   - Sun       = cache read (brightness = cache hit ratio)
//   - Mushrooms = cache creation (count scales with new cache)
//   - Rain      = live activity (drops fall when requests are active)
//
// Features:
//   - Hover tooltips explain every element
//   - Garden Advisor sidebar with contextual suggestions
//   - Garden Health Score (thriving / needs attention / overgrown)
//   - Pictogram legend bar with clickable icons
//   - Tree inspector panel on click
//   - Help mode toggle ("?") labels all elements

import { invoke, formatNumber } from './utils.js';

const SVG_NS = 'http://www.w3.org/2000/svg';

// ---- State ----
let gardenTimeRange = 'all';
let currentCwd = null;     // null = picker view, string = garden view
let projectList = [];
let gardenDetail = null;   // GardenDetail for current project
let liveTimer = null;
let rainTimers = new Map();
let helpMode = false;
let expandedSessionId = null; // which tree is currently expanded
let advisorSuggestions = [];  // current suggestions list
let advisorIndex = 0;         // which suggestion is showing

// ---- Public API ----

export function initGarden() {
  // Help mode toggle
  document.getElementById('garden-help-btn')?.addEventListener('click', () => {
    helpMode = !helpMode;
    const btn = document.getElementById('garden-help-btn');
    const svg = document.getElementById('garden-svg');
    if (btn) btn.classList.toggle('active', helpMode);
    if (svg) svg.classList.toggle('help-mode', helpMode);
  });

  // Click on SVG background to collapse expanded tree
  document.getElementById('garden-svg')?.addEventListener('click', (e) => {
    // Only collapse if clicking the background (not a tree)
    if (e.target.closest('.garden-tree')) return;
    if (expandedSessionId) {
      expandedSessionId = null;
      renderGardenScene();
    }
  });

  // Tooltip follows mouse
  const sceneWrap = document.querySelector('.garden-scene-wrap');
  if (sceneWrap) {
    sceneWrap.addEventListener('mousemove', (e) => {
      const tooltip = document.getElementById('garden-tooltip');
      if (tooltip && tooltip.style.display !== 'none') {
        const rect = sceneWrap.getBoundingClientRect();
        let x = e.clientX - rect.left + 14;
        let y = e.clientY - rect.top + 14;
        // Keep in bounds
        if (x + 260 > rect.width) x = e.clientX - rect.left - 270;
        if (y + 120 > rect.height) y = e.clientY - rect.top - 120;
        tooltip.style.left = `${x}px`;
        tooltip.style.top = `${y}px`;
      }
    });
  }

  // Quilly: click icon or "next" to cycle suggestions
  document.getElementById('quilly-icon')?.addEventListener('click', () => {
    cycleSuggestion();
  });
  document.getElementById('quilly-bubble-next')?.addEventListener('click', () => {
    cycleSuggestion();
  });

  // Quilly: health button toggles stats panel
  document.getElementById('quilly-health-btn')?.addEventListener('click', () => {
    const panel = document.getElementById('quilly-stats-panel');
    if (panel) {
      const showing = panel.style.display !== 'none';
      panel.style.display = showing ? 'none' : '';
    }
  });
}

export function loadGarden() {
  // Always fetch project list first, then show selected (or first) garden
  loadProjectList();
}

export function startGardenPolling() {
  stopGardenPolling();
  if (currentCwd) {
    liveTimer = setInterval(() => pollLive(currentCwd), 4000);
    pollLive(currentCwd);
  }
}

export function stopGardenPolling() {
  if (liveTimer) { clearInterval(liveTimer); liveTimer = null; }
  rainTimers.forEach(t => clearTimeout(t));
  rainTimers.clear();
}

// ---- Project List & Name Bar ----

function loadProjectList() {
  invoke('get_garden_stats', { timeRange: gardenTimeRange })
    .then(data => {
      projectList = data.projects || [];
      renderNameBar();
      // Auto-select first garden if none selected, or reload current
      if (projectList.length > 0) {
        if (!currentCwd || !projectList.find(p => p.cwd === currentCwd)) {
          currentCwd = projectList[0].cwd;
        }
        document.getElementById('garden-empty').style.display = 'none';
        document.getElementById('garden-scene').style.display = '';
        loadGardenDetail(currentCwd);
        startGardenPolling();
        highlightActivePill();
      } else {
        currentCwd = null;
        document.getElementById('garden-empty').style.display = '';
      }
    })
    .catch(e => console.error('[garden]', e));
}

function renderNameBar() {
  const scroll = document.getElementById('garden-names-scroll');
  if (!scroll) return;

  scroll.innerHTML = projectList.map(p =>
    `<span class="garden-name-pill" data-cwd="${esc(p.cwd)}">${esc(p.display_name)}</span>`
  ).join('');

  scroll.querySelectorAll('.garden-name-pill').forEach(pill => {
    pill.addEventListener('click', () => {
      const cwd = pill.dataset.cwd;
      if (cwd === currentCwd) return;
      currentCwd = cwd;
      expandedSessionId = null;
      highlightActivePill();
      loadGardenDetail(currentCwd);
      startGardenPolling();
    });
  });
}

function highlightActivePill() {
  const scroll = document.getElementById('garden-names-scroll');
  if (!scroll) return;
  scroll.querySelectorAll('.garden-name-pill').forEach(p => {
    p.classList.toggle('active', p.dataset.cwd === currentCwd);
  });
}

// ---- Garden Detail ----

function loadGardenDetail(cwd) {
  invoke('get_garden_detail', { cwd, timeRange: gardenTimeRange })
    .then(data => {
      gardenDetail = data;
      renderGardenScene();
      renderStatsRow();
      renderPictogramBar();
      renderHealthScore();
      renderAdvisor();
    })
    .catch(e => console.error('[garden]', e));
}

function renderStatsRow() {
  const row = document.getElementById('garden-stats-row');
  if (!row || !gardenDetail) return;
  const d = gardenDetail;
  const cacheRatio = d.total_input + d.cache_read > 0
    ? Math.round((d.cache_read / (d.total_input + d.cache_read)) * 100)
    : 0;
  row.innerHTML = `
    <div class="garden-stat"><span class="garden-stat-label">Output</span><span class="garden-stat-value">${formatNumber(d.total_output)}</span></div>
    <div class="garden-stat"><span class="garden-stat-label">Input</span><span class="garden-stat-value">${formatNumber(d.total_input)}</span></div>
    <div class="garden-stat"><span class="garden-stat-label">Cache Read</span><span class="garden-stat-value">${formatNumber(d.cache_read)}</span></div>
    <div class="garden-stat"><span class="garden-stat-label">Cache Write</span><span class="garden-stat-value">${formatNumber(d.cache_creation)}</span></div>
    <div class="garden-stat"><span class="garden-stat-label">Cache %</span><span class="garden-stat-value">${cacheRatio}%</span></div>
    <div class="garden-stat"><span class="garden-stat-label">Sessions</span><span class="garden-stat-value">${d.sessions.length}</span></div>
    <div class="garden-stat"><span class="garden-stat-label">Requests</span><span class="garden-stat-value">${formatNumber(d.request_count)}</span></div>
  `;
}

// ============ Pictogram Legend Bar ============

function renderPictogramBar() {
  const bar = document.getElementById('garden-pictogram-bar');
  if (!bar || !gardenDetail) return;
  const d = gardenDetail;
  const cacheRatio = d.total_input + d.cache_read > 0
    ? Math.round((d.cache_read / (d.total_input + d.cache_read)) * 100)
    : 0;
  const activeSessions = rainTimers.size;

  bar.innerHTML = `
    <span class="garden-pictogram" data-target="tree" title="Trees represent sessions. Click to highlight.">
      <svg class="garden-pictogram-icon" viewBox="0 0 16 16"><rect x="7" y="6" width="2" height="10" rx="1" fill="#5a3a20"/><circle cx="8" cy="5" r="5" fill="#4a8a3a"/></svg>
      <span class="garden-pictogram-value">${d.sessions.length}</span> sessions
    </span>
    <span class="garden-pictogram" data-target="pond" title="The pond represents input tokens sent.">
      <svg class="garden-pictogram-icon" viewBox="0 0 16 16"><ellipse cx="8" cy="10" rx="7" ry="4" fill="#1a3a5a"/><ellipse cx="6" cy="9" rx="2" ry="1" fill="#4a90d9" opacity="0.5"/></svg>
      <span class="garden-pictogram-value">${formatNumber(d.total_input)}</span> input
    </span>
    <span class="garden-pictogram" data-target="sun" title="The sun shows cache read efficiency.">
      <svg class="garden-pictogram-icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="4" fill="#f5c542"/><line x1="8" y1="1" x2="8" y2="3" stroke="#f5c542" stroke-width="1.5" stroke-linecap="round"/><line x1="8" y1="13" x2="8" y2="15" stroke="#f5c542" stroke-width="1.5" stroke-linecap="round"/><line x1="1" y1="8" x2="3" y2="8" stroke="#f5c542" stroke-width="1.5" stroke-linecap="round"/><line x1="13" y1="8" x2="15" y2="8" stroke="#f5c542" stroke-width="1.5" stroke-linecap="round"/></svg>
      <span class="garden-pictogram-value">${cacheRatio}%</span> cache
    </span>
    <span class="garden-pictogram" data-target="mushroom" title="Mushrooms sprout when new cache entries are created.">
      <svg class="garden-pictogram-icon" viewBox="0 0 16 16"><rect x="7" y="10" width="2" height="5" rx="1" fill="#d4c8b0"/><ellipse cx="8" cy="10" rx="5" ry="3.5" fill="#c084fc"/><circle cx="6" cy="9" r="1" fill="#fff" opacity="0.5"/></svg>
      <span class="garden-pictogram-value">${formatNumber(d.cache_creation)}</span> cached
    </span>
    <span class="garden-pictogram" data-target="rain" title="Rain falls on active sessions.">
      <svg class="garden-pictogram-icon" viewBox="0 0 16 16"><line x1="4" y1="3" x2="4" y2="7" stroke="#5b9bd5" stroke-width="1.5" stroke-linecap="round"/><line x1="8" y1="1" x2="8" y2="5" stroke="#5b9bd5" stroke-width="1.5" stroke-linecap="round"/><line x1="12" y1="4" x2="12" y2="8" stroke="#5b9bd5" stroke-width="1.5" stroke-linecap="round"/><line x1="6" y1="8" x2="6" y2="12" stroke="#5b9bd5" stroke-width="1.5" stroke-linecap="round"/><line x1="10" y1="6" x2="10" y2="10" stroke="#5b9bd5" stroke-width="1.5" stroke-linecap="round"/></svg>
      <span class="garden-pictogram-value">${activeSessions}</span> active
    </span>
    <span class="garden-pictogram" data-target="requests" title="Total API requests across all sessions.">
      <svg class="garden-pictogram-icon" viewBox="0 0 16 16"><rect x="2" y="10" width="3" height="5" rx="1" fill="#71D083"/><rect x="6.5" y="6" width="3" height="9" rx="1" fill="#71D083" opacity="0.7"/><rect x="11" y="3" width="3" height="12" rx="1" fill="#71D083" opacity="0.5"/></svg>
      <span class="garden-pictogram-value">${formatNumber(d.request_count)}</span> reqs
    </span>
  `;

  // Click to pulse corresponding SVG elements
  bar.querySelectorAll('.garden-pictogram').forEach(p => {
    p.addEventListener('click', () => {
      const target = p.dataset.target;
      pulseElements(target);
    });
  });
}

/** Flash/pulse SVG elements by type when user clicks a pictogram */
function pulseElements(type) {
  const svg = document.getElementById('garden-svg');
  if (!svg) return;
  const classMap = {
    tree: '.garden-tree',
    pond: '.garden-pond-group',
    sun: '.garden-sun-group',
    mushroom: '.garden-mushroom',
    rain: '.garden-raindrop',
  };
  const sel = classMap[type];
  if (!sel) return;
  svg.querySelectorAll(sel).forEach(el => {
    el.style.transition = 'filter 0.3s';
    el.style.filter = 'brightness(2) drop-shadow(0 0 8px #71D083)';
    setTimeout(() => { el.style.filter = ''; }, 800);
  });
}

// ============ Garden Health Score ============

function renderHealthScore() {
  const btn = document.getElementById('quilly-health-btn');
  if (!btn || !gardenDetail) return;
  const d = gardenDetail;
  const cacheRatio = d.total_input + d.cache_read > 0
    ? d.cache_read / (d.total_input + d.cache_read)
    : 0;

  let issues = 0;

  const maxLastInput = Math.max(...d.sessions.map(s => s.last_input_tokens || 0), 0);
  if (maxLastInput > 150000) issues += 2;
  else if (maxLastInput > 80000) issues += 1;

  if (d.cache_read > 0 || d.total_input > 10000) {
    if (cacheRatio < 0.2) issues += 2;
    else if (cacheRatio < 0.5) issues += 1;
  }

  if (d.total_input > 0 && d.total_output > 0) {
    const ioRatio = d.total_input / d.total_output;
    if (ioRatio > 10) issues += 1;
  }

  let level, label;
  if (issues === 0) { level = 'thriving'; label = 'Stats'; }
  else if (issues <= 2) { level = 'attention'; label = 'Stats'; }
  else { level = 'overgrown'; label = 'Stats'; }

  btn.dataset.level = level;
  const lbl = document.getElementById('quilly-health-label');
  if (lbl) lbl.textContent = label;

  // Also update the topbar health indicator
  const topHealth = document.getElementById('garden-health');
  if (topHealth) {
    topHealth.dataset.level = level;
    const topLbl = topHealth.querySelector('.garden-health-label');
    if (topLbl) topLbl.textContent = label;
  }
}

// ============ Garden Advisor ============

function renderAdvisor() {
  if (!gardenDetail) return;
  const d = gardenDetail;
  const suggestions = [];

  const cacheRatio = d.total_input + d.cache_read > 0
    ? d.cache_read / (d.total_input + d.cache_read)
    : 0;
  const maxLastInput = Math.max(...d.sessions.map(s => s.last_input_tokens || 0), 0);
  const maxSession = d.sessions.find(s => (s.last_input_tokens || 0) === maxLastInput);

  // --- Oversized context ---
  if (maxLastInput > 150000 && maxSession) {
    suggestions.push({
      severity: 'bad',
      icon: '\u2702\uFE0F',
      text: `Time to <strong>prune the tree</strong> \u2014 session <code>${shortId(maxSession.session_id)}</code> is carrying ${formatNumber(maxLastInput)} context tokens.`,
      hint: 'The conversation has grown very large. Consider starting a fresh session or trimming unnecessary context.',
    });
  } else if (maxLastInput > 80000 && maxSession) {
    suggestions.push({
      severity: 'warn',
      icon: '\u2702\uFE0F',
      text: `A tall tree is growing \u2014 session <code>${shortId(maxSession.session_id)}</code> has ${formatNumber(maxLastInput)} context tokens.`,
      hint: 'Keep an eye on it. As context grows, responses may slow down and costs increase.',
    });
  }

  // --- Many small sessions ---
  if (d.sessions.length > 8 && maxLastInput < 20000) {
    suggestions.push({
      severity: 'warn',
      icon: '\uD83C\uDF3F',
      text: 'Your garden is <strong>bushy</strong> \u2014 lots of small sessions.',
      hint: 'You might get better results with fewer, longer conversations.',
    });
  }

  // --- Input-heavy (pond flooding) ---
  if (d.total_input > 0 && d.total_output > 0) {
    const ioRatio = d.total_input / d.total_output;
    if (ioRatio > 10) {
      suggestions.push({
        severity: 'warn',
        icon: '\uD83C\uDF0A',
        text: 'Your <strong>pond is flooding</strong> \u2014 sending a lot of input but getting little output.',
        hint: 'Check if you\'re sending unnecessary context in your prompts.',
      });
    }
  }

  // --- Cache performance ---
  if (d.cache_read > 0 || d.total_input > 10000) {
    if (cacheRatio >= 0.7) {
      suggestions.push({
        severity: 'good',
        icon: '\u2600\uFE0F',
        text: '<strong>Beautiful sunny day!</strong> Great cache reuse at ' + Math.round(cacheRatio * 100) + '%.',
        hint: 'You\'re being cost-efficient. Keep reusing conversation threads.',
      });
    } else if (cacheRatio < 0.2) {
      suggestions.push({
        severity: 'bad',
        icon: '\u2601\uFE0F',
        text: 'The <strong>sun is hiding</strong> \u2014 cache hit rate is only ' + Math.round(cacheRatio * 100) + '%.',
        hint: 'Try reusing conversation threads instead of starting fresh to save costs.',
      });
    } else if (cacheRatio < 0.5) {
      suggestions.push({
        severity: 'warn',
        icon: '\u26C5',
        text: 'Partly cloudy \u2014 cache hit rate is ' + Math.round(cacheRatio * 100) + '%.',
        hint: 'There\'s room to improve. Longer sessions tend to reuse cache better.',
      });
    }
  }

  // --- Mushroom overload ---
  if (d.cache_creation > 100000) {
    suggestions.push({
      severity: 'warn',
      icon: '\uD83C\uDF44',
      text: '<strong>Mushroom overload</strong> \u2014 ' + formatNumber(d.cache_creation) + ' tokens of new cache created.',
      hint: 'Lots of fresh contexts being cached. This means many new conversations are being started.',
    });
  }

  // --- Single tree dominates ---
  if (d.sessions.length > 1 && maxSession) {
    const totalLastInput = d.sessions.reduce((s, x) => s + (x.last_input_tokens || 0), 0);
    if (totalLastInput > 0 && maxLastInput / totalLastInput > 0.7) {
      suggestions.push({
        severity: 'info',
        icon: '\uD83C\uDF33',
        text: `One tree is <strong>overshadowing</strong> the others \u2014 <code>${shortId(maxSession.session_id)}</code> carries ${Math.round(maxLastInput / totalLastInput * 100)}% of total context.`,
        hint: 'Most of the context weight is concentrated in a single session.',
      });
    }
  }

  // --- No activity ---
  if (d.sessions.length === 0) {
    suggestions.push({
      severity: 'info',
      icon: '\uD83C\uDF19',
      text: 'The garden is <strong>quiet</strong>. No sessions yet.',
      hint: 'Start a new coding session to grow your garden.',
    });
  }

  // --- All healthy ---
  if (suggestions.length === 0) {
    suggestions.push({
      severity: 'good',
      icon: '\uD83C\uDF31',
      text: 'Your garden is <strong>healthy</strong>!',
      hint: 'Everything looks balanced. Keep up the good work.',
    });
  }

  advisorSuggestions = suggestions;
  advisorIndex = 0;
  showSuggestion();
}

function showSuggestion() {
  const bubble = document.getElementById('quilly-bubble');
  const textEl = document.getElementById('quilly-bubble-text');
  const dotsEl = document.getElementById('quilly-bubble-dots');
  if (!bubble || !textEl || advisorSuggestions.length === 0) return;

  const s = advisorSuggestions[advisorIndex];
  bubble.dataset.severity = s.severity;
  textEl.innerHTML = `<span>${s.icon}</span> ${s.text}<br><span style="color:#666;font-size:10px;">${s.hint}</span>`;

  // Render dots
  if (dotsEl) {
    dotsEl.innerHTML = advisorSuggestions.map((_, i) =>
      `<span class="quilly-bubble-dot${i === advisorIndex ? ' active' : ''}"></span>`
    ).join('');
  }

  // Hide "next" if only one suggestion
  const nextBtn = document.getElementById('quilly-bubble-next');
  if (nextBtn) nextBtn.style.display = advisorSuggestions.length <= 1 ? 'none' : '';
}

function cycleSuggestion() {
  if (advisorSuggestions.length <= 1) return;
  advisorIndex = (advisorIndex + 1) % advisorSuggestions.length;
  showSuggestion();
}

// ============ Expand Tree In-Place ============

function toggleTreeExpand(session) {
  if (expandedSessionId === session.session_id) {
    expandedSessionId = null;
  } else {
    expandedSessionId = session.session_id;
  }
  renderGardenScene();
}

/** Draw info annotations around an expanded tree (signposts hanging from branches, sign at base). */
function drawTreeInfo(group, cx, groundY, session, treeH, trunkTop, canopyR) {
  const accent = backendColor(session.backend);
  const lastInput = session.last_input_tokens || 0;
  const infoG = el('g');
  infoG.setAttribute('class', 'garden-tree-info');

  // --- Wooden sign at the base with session name ---
  const signY = groundY + 16;
  // Sign post
  const post = el('rect');
  setA(post, { x: cx - 1.5, y: signY, width: 3, height: 28, fill: '#5a3a20' });
  infoG.appendChild(post);
  // Sign board
  const boardW = 120, boardH = 22;
  const board = el('rect');
  setA(board, { x: cx - boardW / 2, y: signY + 26, width: boardW, height: boardH, rx: 3, fill: '#3a2815', stroke: '#5a3a20', 'stroke-width': 1 });
  infoG.appendChild(board);
  const signText = el('text');
  setA(signText, { x: cx, y: signY + 41, 'text-anchor': 'middle', fill: '#c8b890', 'font-size': 9, 'font-family': '-apple-system, sans-serif' });
  signText.textContent = shortId(session.session_id);
  infoG.appendChild(signText);

  // --- Tags hanging from branches (left side) ---
  const tags = [
    { label: 'Context', value: formatNumber(lastInput), color: '#71D083' },
    { label: 'Output', value: formatNumber(session.output_tokens), color: '#f5c542' },
    { label: 'Requests', value: String(session.request_count), color: '#5b9bd5' },
    { label: 'Backend', value: session.backend, color: accent },
  ];

  const tagX = cx - canopyR - 30;
  const tagStartY = trunkTop + treeH * 0.15;

  tags.forEach((tag, i) => {
    const ty = tagStartY + i * 34;

    // Connecting line from trunk to tag
    const line = el('line');
    setA(line, { x1: cx - 4, y1: ty + 6, x2: tagX + 70, y2: ty + 6, stroke: '#3d3020', 'stroke-width': 0.8, opacity: 0.5, 'stroke-dasharray': '3,2' });
    infoG.appendChild(line);

    // Tag background (leaf-shaped card)
    const tagBg = el('rect');
    setA(tagBg, { x: tagX, y: ty - 4, width: 70, height: 24, rx: 5, fill: '#1a1d1ecc', stroke: tag.color, 'stroke-width': 1 });
    infoG.appendChild(tagBg);

    // Label
    const lbl = el('text');
    setA(lbl, { x: tagX + 6, y: ty + 5, fill: '#777', 'font-size': 7, 'font-family': '-apple-system, sans-serif' });
    lbl.textContent = tag.label;
    infoG.appendChild(lbl);

    // Value
    const val = el('text');
    setA(val, { x: tagX + 6, y: ty + 15, fill: tag.color, 'font-size': 10, 'font-weight': 600, 'font-family': '-apple-system, sans-serif' });
    val.textContent = tag.value;
    infoG.appendChild(val);
  });

  // --- Context meter bar on the right side of trunk ---
  const meterX = cx + canopyR + 16;
  const meterH = Math.min(treeH * 0.7, 140);
  const meterY = trunkTop + treeH * 0.15;
  const fillRatio = Math.min(lastInput / 200000, 1);
  const fillH = meterH * fillRatio;

  // Meter track
  const track = el('rect');
  setA(track, { x: meterX, y: meterY, width: 8, height: meterH, rx: 4, fill: '#2a2d2e' });
  infoG.appendChild(track);

  // Meter fill (from bottom)
  const meterFill = el('rect');
  const fillColor = fillRatio > 0.75 ? '#f87171' : fillRatio > 0.4 ? '#facc15' : '#4ade80';
  setA(meterFill, { x: meterX, y: meterY + meterH - fillH, width: 8, height: fillH, rx: 4, fill: fillColor });
  infoG.appendChild(meterFill);

  // Meter label
  const meterLbl = el('text');
  setA(meterLbl, { x: meterX + 4, y: meterY - 6, 'text-anchor': 'middle', fill: '#777', 'font-size': 8, 'font-family': '-apple-system, sans-serif' });
  meterLbl.textContent = 'CTX';
  infoG.appendChild(meterLbl);

  // Meter value at bottom
  const meterVal = el('text');
  setA(meterVal, { x: meterX + 4, y: meterY + meterH + 14, 'text-anchor': 'middle', fill: fillColor, 'font-size': 9, 'font-weight': 600, 'font-family': '-apple-system, sans-serif' });
  meterVal.textContent = formatNumber(lastInput);
  infoG.appendChild(meterVal);

  // --- Suggestion sign (if applicable) ---
  let suggestion = null;
  if (lastInput > 150000) {
    suggestion = { icon: '\u2702\uFE0F', text: 'Time to prune!', color: '#f87171' };
  } else if (lastInput > 80000) {
    suggestion = { icon: '\uD83C\uDF3F', text: 'Growing fast...', color: '#facc15' };
  } else if (session.request_count > 30) {
    suggestion = { icon: '\uD83C\uDF3F', text: 'Very branchy', color: '#facc15' };
  }

  if (suggestion) {
    const sY = groundY + 56;
    const sW = 110, sH = 20;
    // Wooden sign
    const sPost = el('rect');
    setA(sPost, { x: cx + 20, y: sY - 14, width: 2, height: 14, fill: '#5a3a20' });
    infoG.appendChild(sPost);
    const sBg = el('rect');
    setA(sBg, { x: cx - sW / 2 + 20, y: sY, width: sW, height: sH, rx: 3, fill: '#1a1d1ecc', stroke: suggestion.color, 'stroke-width': 1 });
    infoG.appendChild(sBg);
    const sText = el('text');
    setA(sText, { x: cx + 20, y: sY + 14, 'text-anchor': 'middle', fill: suggestion.color, 'font-size': 10, 'font-weight': 600, 'font-family': '-apple-system, sans-serif' });
    sText.textContent = `${suggestion.icon} ${suggestion.text}`;
    infoG.appendChild(sText);
  }

  group.appendChild(infoG);
}

// ============ Tooltip Helpers ============

function showTooltip(title, metric, explain) {
  const tooltip = document.getElementById('garden-tooltip');
  if (!tooltip) return;
  tooltip.innerHTML = `
    <div class="garden-tooltip-title">${title}</div>
    <div class="garden-tooltip-metric">${metric}</div>
    <div class="garden-tooltip-explain">${explain}</div>
  `;
  tooltip.style.display = '';
}

function hideTooltip() {
  const tooltip = document.getElementById('garden-tooltip');
  if (tooltip) tooltip.style.display = 'none';
}

// ============ SVG Garden Scene ============

function renderGardenScene() {
  const svg = document.getElementById('garden-svg');
  if (!svg || !gardenDetail) return;
  svg.innerHTML = '';
  // Maintain help mode class
  if (helpMode) svg.classList.add('help-mode');

  const W = 1200, H = 700;
  const GROUND_Y = 520;

  // ---- Defs ----
  const defs = el('defs');
  defs.innerHTML = `
    <linearGradient id="gSky" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#080c14"/>
      <stop offset="35%" stop-color="#0f1620"/>
      <stop offset="65%" stop-color="#162018"/>
      <stop offset="100%" stop-color="#1c2c14"/>
    </linearGradient>
    <linearGradient id="gGround" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#2a3d1a"/>
      <stop offset="100%" stop-color="#182510"/>
    </linearGradient>
    <linearGradient id="gPond" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#1a3a5a"/>
      <stop offset="100%" stop-color="#0d2040"/>
    </linearGradient>
    <radialGradient id="gSunGlow" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="rgba(245,197,66,0.25)"/>
      <stop offset="100%" stop-color="transparent"/>
    </radialGradient>
    <radialGradient id="gMoonGlow" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="rgba(200,220,255,0.12)"/>
      <stop offset="100%" stop-color="transparent"/>
    </radialGradient>
    <filter id="fGlow">
      <feGaussianBlur in="SourceGraphic" stdDeviation="2.5" result="b"/>
      <feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge>
    </filter>
    <filter id="fSoftGlow">
      <feGaussianBlur in="SourceGraphic" stdDeviation="6"/>
    </filter>
  `;
  svg.appendChild(defs);

  // ---- Sky ----
  svg.appendChild(rect(0, 0, W, H, 'url(#gSky)'));

  // ---- Stars ----
  for (let i = 0; i < 50; i++) {
    const s = el('circle');
    s.setAttribute('class', 'garden-star');
    setA(s, { cx: rand(0, W), cy: rand(0, GROUND_Y * 0.5), r: rand(0.4, 1.4), fill: '#c8dcff' });
    s.style.animationDelay = `${-rand(0, 3)}s`;
    s.style.animationDuration = `${rand(2, 5)}s`;
    svg.appendChild(s);
  }

  // ---- Moon (top-right) ----
  svg.appendChild(circle(W - 100, 70, 50, 'url(#gMoonGlow)'));
  svg.appendChild(circle(W - 100, 70, 14, '#d4dce8', 0.6));

  // ---- Clouds ----
  for (let i = 0; i < 2; i++) {
    const g = el('g');
    g.setAttribute('class', 'garden-cloud');
    g.style.animationDelay = `${-i * 35}s`;
    g.style.animationDuration = `${70 + i * 25}s`;
    const y = 50 + i * 70;
    [0, 25, 50].forEach(dx => {
      const e = el('ellipse');
      setA(e, { cx: dx, cy: y, rx: 35 + dx * 0.3, ry: 14, fill: '#fff' });
      g.appendChild(e);
    });
    svg.appendChild(g);
  }

  // ---- Sun (cache read) ----
  drawSun(svg, gardenDetail);

  // ---- Ground ----
  svg.appendChild(rect(0, GROUND_Y, W, H - GROUND_Y, 'url(#gGround)'));
  const gl = el('line');
  setA(gl, { x1: 0, y1: GROUND_Y, x2: W, y2: GROUND_Y, stroke: '#3d5e2a', 'stroke-width': 2, opacity: 0.5 });
  svg.appendChild(gl);

  // ---- Grass ----
  for (let x = 0; x < W; x += rand(6, 12)) {
    const b = el('line');
    b.setAttribute('class', 'garden-grass-blade');
    const gh = rand(3, 10);
    setA(b, {
      x1: x, y1: GROUND_Y,
      x2: x + rand(-3, 3), y2: GROUND_Y - gh,
      stroke: '#3a6a2a', 'stroke-width': 1.3, 'stroke-linecap': 'round', opacity: rand(0.3, 0.7),
    });
    b.style.animationDelay = `${-rand(0, 3)}s`;
    svg.appendChild(b);
  }

  // ---- Pond (input tokens) ----
  drawPond(svg, gardenDetail, W, GROUND_Y);

  // ---- Trees (sessions) ----
  // Tree height = last message's input tokens (how much context the session carries)
  const sessions = gardenDetail.sessions;
  const maxLastInput = Math.max(...sessions.map(s => s.last_input_tokens || 0), 1);

  const treeZoneW = W * 0.7;
  const treeZoneStart = 60;
  const count = sessions.length;
  const isExpanded = expandedSessionId !== null;
  const expandedIdx = isExpanded ? sessions.findIndex(s => s.session_id === expandedSessionId) : -1;

  // When a tree is expanded, give it center stage; others shrink to the sides
  sessions.forEach((sess, i) => {
    const isThisExpanded = sess.session_id === expandedSessionId;
    let cx;
    if (!isExpanded) {
      // Normal layout
      const spacing = count > 1 ? treeZoneW / count : treeZoneW / 2;
      cx = treeZoneStart + spacing * (i + 0.5);
    } else if (isThisExpanded) {
      // Expanded tree gets center
      cx = W * 0.42;
    } else {
      // Others squeeze to the edges
      const otherCount = count - 1;
      const idx = i < expandedIdx ? i : i - 1;
      const sideW = W * 0.15;
      const leftStart = 30;
      const rightStart = W * 0.72;
      if (idx < otherCount / 2) {
        cx = leftStart + (sideW / Math.ceil(otherCount / 2)) * (idx + 0.5);
      } else {
        const rIdx = idx - Math.ceil(otherCount / 2);
        cx = rightStart + (sideW / Math.max(Math.floor(otherCount / 2), 1)) * (rIdx + 0.5);
      }
    }

    const g = el('g');
    g.setAttribute('class', 'garden-tree garden-hover-target');
    g.dataset.session = sess.session_id;

    if (isExpanded && !isThisExpanded) {
      // Faded, smaller non-expanded trees
      g.setAttribute('opacity', '0.3');
      g.style.transition = 'opacity 0.3s';
    }

    const scale = (isExpanded && isThisExpanded) ? 1.3 : (isExpanded ? 0.6 : 1.0);
    const treeResult = drawTree(g, cx, GROUND_Y, sess, maxLastInput, scale);

    // Draw info annotations on expanded tree
    if (isThisExpanded && treeResult) {
      drawTreeInfo(g, cx, GROUND_Y, sess, treeResult.treeH, treeResult.trunkTop, treeResult.canopyR);
    }

    // Help-mode label (only on non-expanded)
    if (!isThisExpanded) {
      const helpLbl = el('text');
      helpLbl.setAttribute('class', 'garden-help-label');
      setA(helpLbl, { x: cx, y: GROUND_Y - 10 });
      helpLbl.textContent = `Context: ${formatNumber(sess.last_input_tokens || 0)} tokens`;
      g.appendChild(helpLbl);
    }

    // Hover tooltip
    g.addEventListener('mouseenter', () => {
      if (isThisExpanded) return; // expanded tree shows info directly
      const bCount = Math.min(Math.floor(sess.request_count / 5) + 1, 6);
      const lastInput = sess.last_input_tokens || 0;
      showTooltip(
        `Session: ${shortId(sess.session_id)}`,
        `${formatNumber(lastInput)} context tokens \u00B7 ${sess.request_count} requests`,
        `Tree height = last message\u2019s input tokens (${formatNumber(lastInput)}), showing how much context this session carries. ${bCount} branch${bCount !== 1 ? 'es' : ''} from ${sess.request_count} requests. Backend: ${sess.backend}.`
      );
    });
    g.addEventListener('mouseleave', hideTooltip);

    // Click to expand/collapse
    g.addEventListener('click', (e) => {
      e.stopPropagation();
      toggleTreeExpand(sess);
    });

    svg.appendChild(g);
  });

  // ---- Mushrooms (cache creation) ----
  drawMushrooms(svg, gardenDetail, W, GROUND_Y);

  // ---- Fireflies (ambient) ----
  for (let i = 0; i < 5; i++) {
    const f = el('circle');
    f.setAttribute('class', 'garden-firefly');
    setA(f, { cx: rand(50, W - 50), cy: rand(GROUND_Y - 200, GROUND_Y - 30), r: 2, fill: '#b8e986', filter: 'url(#fGlow)' });
    f.style.animationDelay = `${-rand(0, 3)}s`;
    f.style.animationDuration = `${rand(2.5, 4)}s`;
    svg.appendChild(f);
  }
}

// ---- Sun: cache read ratio ----

function drawSun(svg, detail) {
  const total = detail.total_input + detail.cache_read;
  const ratio = total > 0 ? detail.cache_read / total : 0;
  const sunR = 20 + ratio * 30;
  const opacity = 0.3 + ratio * 0.7;
  const cx = 100, cy = 100;

  // Wrap in group for tooltip + help label
  const sunGroup = el('g');
  sunGroup.setAttribute('class', 'garden-sun-group garden-hover-target');

  // Glow
  const glow = el('circle');
  glow.setAttribute('class', 'garden-sun-glow');
  setA(glow, { cx, cy, r: sunR * 3, fill: 'url(#gSunGlow)', opacity });
  sunGroup.appendChild(glow);

  // Rays
  if (ratio > 0.05) {
    const raysG = el('g');
    raysG.setAttribute('class', 'garden-sun-rays');
    raysG.style.transformOrigin = `${cx}px ${cy}px`;
    const rayCount = 8 + Math.floor(ratio * 8);
    for (let i = 0; i < rayCount; i++) {
      const angle = (i / rayCount) * Math.PI * 2;
      const inner = sunR + 4;
      const outer = sunR + 10 + ratio * 20;
      const ray = el('line');
      setA(ray, {
        x1: cx + Math.cos(angle) * inner,
        y1: cy + Math.sin(angle) * inner,
        x2: cx + Math.cos(angle) * outer,
        y2: cy + Math.sin(angle) * outer,
        stroke: '#f5c542',
        'stroke-width': 1.5,
        'stroke-linecap': 'round',
        opacity: opacity * 0.6,
      });
      raysG.appendChild(ray);
    }
    sunGroup.appendChild(raysG);
  }

  // Sun disc
  const disc = el('circle');
  setA(disc, { cx, cy, r: sunR, fill: '#f5c542', opacity, filter: 'url(#fGlow)' });
  sunGroup.appendChild(disc);

  // Label
  const lbl = el('text');
  lbl.setAttribute('class', 'garden-label-dim');
  setA(lbl, { x: cx, y: cy + sunR + 16 });
  lbl.textContent = `${Math.round(ratio * 100)}% cache`;
  sunGroup.appendChild(lbl);

  // Help-mode label
  const helpLbl = el('text');
  helpLbl.setAttribute('class', 'garden-help-label');
  setA(helpLbl, { x: cx, y: cy + sunR + 30 });
  helpLbl.textContent = 'Sun = cache read efficiency';
  sunGroup.appendChild(helpLbl);

  // Tooltip
  sunGroup.addEventListener('mouseenter', () => {
    showTooltip(
      'Sun \u2014 Cache Read',
      `${Math.round(ratio * 100)}% hit rate \u00B7 ${formatNumber(detail.cache_read)} tokens`,
      ratio >= 0.7
        ? 'Bright sun! Your prompts are reusing cached context efficiently, saving cost.'
        : ratio >= 0.3
          ? 'Moderate cache reuse. Longer sessions and reusing threads will make the sun brighter.'
          : 'Low cache reuse. Most prompts are starting fresh. Try reusing conversation threads to save costs.'
    );
  });
  sunGroup.addEventListener('mouseleave', hideTooltip);

  svg.appendChild(sunGroup);
}

// ---- Pond: input tokens ----

function drawPond(svg, detail, W, groundY) {
  const inputRatio = Math.min(detail.total_input / 500000, 1);
  const pondW = 80 + inputRatio * 160;
  const pondH = 20 + inputRatio * 30;
  const cx = W - pondW / 2 - 80;
  const cy = groundY + 25;

  // Wrap in group
  const pondGroup = el('g');
  pondGroup.setAttribute('class', 'garden-pond-group garden-hover-target');

  // Pond body
  const pond = el('ellipse');
  setA(pond, { cx, cy, rx: pondW / 2, ry: pondH / 2, fill: 'url(#gPond)', opacity: 0.8 });
  pondGroup.appendChild(pond);

  // Shimmer highlight
  const shimmer = el('ellipse');
  shimmer.setAttribute('class', 'garden-pond-shimmer');
  setA(shimmer, { cx: cx - pondW * 0.15, cy: cy - pondH * 0.15, rx: pondW * 0.25, ry: pondH * 0.2, fill: '#4a90d9', opacity: 0.3 });
  pondGroup.appendChild(shimmer);

  // Ripples
  for (let i = 0; i < 2; i++) {
    const ripple = el('circle');
    ripple.setAttribute('class', 'garden-pond-ripple');
    setA(ripple, { cx: cx + rand(-pondW * 0.2, pondW * 0.2), cy, r: 4, fill: 'none', stroke: '#4a90d9', 'stroke-width': 0.8 });
    ripple.style.animationDelay = `${i * 1}s`;
    pondGroup.appendChild(ripple);
  }

  // Label
  const lbl = el('text');
  lbl.setAttribute('class', 'garden-label-dim');
  setA(lbl, { x: cx, y: cy + pondH / 2 + 14 });
  lbl.textContent = `${formatNumber(detail.total_input)} input`;
  pondGroup.appendChild(lbl);

  // Help-mode label
  const helpLbl = el('text');
  helpLbl.setAttribute('class', 'garden-help-label');
  setA(helpLbl, { x: cx, y: cy + pondH / 2 + 28 });
  helpLbl.textContent = 'Pond = input tokens sent';
  pondGroup.appendChild(helpLbl);

  // Tooltip
  const outputRatio = detail.total_output > 0
    ? (detail.total_input / detail.total_output).toFixed(1)
    : 'N/A';
  pondGroup.addEventListener('mouseenter', () => {
    showTooltip(
      'Pond \u2014 Input Tokens',
      `${formatNumber(detail.total_input)} tokens \u00B7 ${outputRatio}x input/output ratio`,
      `The pond represents all input tokens sent to the model. A larger pond means more context is being sent. ${inputRatio > 0.6 ? 'This is a big pond \u2014 consider if all that context is necessary.' : 'Pond size looks reasonable.'}`
    );
  });
  pondGroup.addEventListener('mouseleave', hideTooltip);

  svg.appendChild(pondGroup);
}

// ---- Tree per session ----

function drawTree(group, cx, groundY, session, maxLastInput, scale = 1.0) {
  // Tree height based on last message's input tokens (context size)
  const lastInput = session.last_input_tokens || 0;
  const ratio = Math.max(lastInput / maxLastInput, 0.12);
  const treeH = (60 + ratio * 220) * scale;
  const trunkW = (5 + ratio * 14) * scale;
  const canopyR = (22 + ratio * 55) * scale;
  const trunkTop = groundY - treeH;

  const accent = backendColor(session.backend);

  // Trunk
  const trunk = el('rect');
  trunk.setAttribute('class', 'garden-trunk');
  setA(trunk, {
    x: cx - trunkW / 2, y: trunkTop,
    width: trunkW, height: treeH,
    rx: trunkW / 3, fill: '#5a3a20',
  });
  group.appendChild(trunk);

  // Branches
  const bCount = Math.min(Math.floor(session.request_count / 5) + 1, 6);
  for (let b = 0; b < bCount; b++) {
    const by = trunkTop + treeH * (0.25 + b * 0.1);
    const dir = b % 2 === 0 ? 1 : -1;
    const bLen = 12 + ratio * 28;
    const br = el('line');
    setA(br, {
      x1: cx, y1: by,
      x2: cx + dir * bLen, y2: by - rand(8, 18),
      stroke: '#5a3a20', 'stroke-width': Math.max(trunkW * 0.3, 1.5), 'stroke-linecap': 'round',
    });
    group.appendChild(br);

    if (ratio > 0.3) {
      const lf = el('circle');
      lf.setAttribute('class', 'garden-leaf');
      setA(lf, { cx: cx + dir * bLen, cy: by - rand(10, 20), r: rand(8, 14), fill: accent, opacity: 0.6 });
      lf.style.animationDelay = `${-b * 0.5}s`;
      group.appendChild(lf);
    }
  }

  // Canopy
  const canopyG = el('g');
  canopyG.setAttribute('class', 'garden-canopy');

  const darkC = darken(accent, 0.4);
  const blobs = 3 + Math.floor(ratio * 3);
  for (let i = 0; i < blobs; i++) {
    const a = (i / blobs) * Math.PI * 2 - Math.PI / 2;
    const d = canopyR * 0.3;
    const bx = cx + Math.cos(a) * d;
    const by = trunkTop + 3 + Math.sin(a) * d * 0.55;
    const br = canopyR * (0.55 + Math.random() * 0.35);

    const blob = el('circle');
    blob.setAttribute('class', 'garden-leaf');
    setA(blob, { cx: bx, cy: by, r: br, fill: i % 2 === 0 ? accent : darkC, opacity: 0.75 });
    blob.style.animationDelay = `${-i * 0.6}s`;
    canopyG.appendChild(blob);
  }

  const cc = el('circle');
  setA(cc, { cx, cy: trunkTop + 3, r: canopyR * 0.45, fill: accent, opacity: 0.85, filter: 'url(#fGlow)' });
  canopyG.appendChild(cc);

  group.appendChild(canopyG);

  // Session label
  const lbl = el('text');
  lbl.setAttribute('class', 'garden-label');
  setA(lbl, { x: cx, y: groundY + 18 });
  lbl.textContent = shortId(session.session_id);
  group.appendChild(lbl);

  // Context size label (last input tokens)
  const sub = el('text');
  sub.setAttribute('class', 'garden-label-dim');
  setA(sub, { x: cx, y: groundY + 30 });
  sub.textContent = `${formatNumber(lastInput)} ctx`;
  group.appendChild(sub);

  return { treeH, trunkTop, canopyR };
}

function backendColor(backend) {
  if (backend.includes('claude')) return '#4a8a3a';
  if (backend.includes('codex')) return '#3a6a8a';
  if (backend.includes('cursor')) return '#7a5a8a';
  return '#5a7a3a';
}

// ---- Mushrooms: cache creation ----

function drawMushrooms(svg, detail, W, groundY) {
  const count = Math.min(Math.floor(detail.cache_creation / 10000) + 1, 12);
  for (let i = 0; i < count; i++) {
    const mx = rand(40, W - 40);
    const my = groundY + rand(8, 60);
    const g = el('g');
    g.setAttribute('class', 'garden-mushroom garden-hover-target');
    g.style.animationDelay = `${i * 0.08}s`;

    // Stem
    const stem = el('rect');
    setA(stem, { x: mx - 2, y: my - 8, width: 4, height: 8, rx: 2, fill: '#d4c8b0' });
    g.appendChild(stem);

    // Cap
    const cap = el('ellipse');
    const capColor = ['#c084fc', '#a855f7', '#d8b4fe', '#9333ea'][i % 4];
    setA(cap, { cx: mx, cy: my - 8, rx: 7 + rand(0, 4), ry: 5, fill: capColor, opacity: 0.85 });
    g.appendChild(cap);

    // Dots on cap
    const dot = el('circle');
    setA(dot, { cx: mx + rand(-3, 3), cy: my - 9, r: 1.2, fill: '#fff', opacity: 0.5 });
    g.appendChild(dot);

    // Help-mode label (only on first mushroom)
    if (i === 0) {
      const helpLbl = el('text');
      helpLbl.setAttribute('class', 'garden-help-label');
      setA(helpLbl, { x: mx, y: my + 10 });
      helpLbl.textContent = 'Mushroom = new cache entry';
      g.appendChild(helpLbl);
    }

    // Tooltip
    g.addEventListener('mouseenter', () => {
      showTooltip(
        'Mushroom \u2014 Cache Creation',
        `${formatNumber(detail.cache_creation)} tokens cached`,
        `Mushrooms sprout when new cache entries are created. ${count} mushroom${count !== 1 ? 's' : ''} = ~${formatNumber(detail.cache_creation)} tokens of fresh context being cached. More mushrooms means more new conversations.`
      );
    });
    g.addEventListener('mouseleave', hideTooltip);

    svg.appendChild(g);
  }

  // Label in bottom corner
  if (detail.cache_creation > 0) {
    const lbl = el('text');
    lbl.setAttribute('class', 'garden-label-dim');
    setA(lbl, { x: W - 60, y: groundY + 80, 'text-anchor': 'end' });
    lbl.textContent = `${formatNumber(detail.cache_creation)} cache write`;
    svg.appendChild(lbl);
  }
}

// ============ Live Rain ============

function pollLive(cwd) {
  invoke('get_garden_live', { cwd })
    .then(data => {
      const active = data.recent || [];
      const activeSessions = new Set(active.map(a => a.session_id));
      activeSessions.forEach(sid => {
        if (!rainTimers.has(sid)) {
          triggerRain(sid);
        }
      });
      // Update the active count in pictogram bar
      updateActivePictogram(activeSessions.size);
    })
    .catch(() => {});
}

function updateActivePictogram(count) {
  const bar = document.getElementById('garden-pictogram-bar');
  if (!bar) return;
  const rainPictogram = bar.querySelector('[data-target="rain"] .garden-pictogram-value');
  if (rainPictogram) rainPictogram.textContent = count;
}

function triggerRain(sessionId) {
  const svg = document.getElementById('garden-svg');
  if (!svg) return;

  const treeG = svg.querySelector(`g[data-session="${CSS.escape(sessionId)}"]`);
  if (!treeG) return;

  treeG.classList.add('garden-tree-active');

  const bbox = treeG.getBBox();
  const cx = bbox.x + bbox.width / 2;
  const topY = bbox.y - 25;

  for (let d = 0; d < 10; d++) {
    setTimeout(() => {
      const dx = cx + (Math.random() - 0.5) * bbox.width * 0.9;
      const drop = el('line');
      drop.setAttribute('class', 'garden-raindrop');
      setA(drop, {
        x1: dx, y1: topY, x2: dx, y2: topY + 7,
        stroke: '#5b9bd5', 'stroke-width': 1.5, 'stroke-linecap': 'round', opacity: 0.6,
      });
      svg.appendChild(drop);

      setTimeout(() => {
        const sp = el('circle');
        sp.setAttribute('class', 'garden-splash');
        setA(sp, {
          cx: dx, cy: bbox.y + bbox.height - 25, r: 3,
          fill: 'none', stroke: '#5b9bd5', 'stroke-width': 0.8, opacity: 0.4,
        });
        svg.appendChild(sp);
        setTimeout(() => sp.remove(), 600);
      }, 850);

      setTimeout(() => drop.remove(), 1100);
    }, d * 120);
  }

  const timer = setTimeout(() => {
    treeG.classList.remove('garden-tree-active');
    rainTimers.delete(sessionId);
  }, 3500);
  rainTimers.set(sessionId, timer);
}

// ============ SVG Helpers ============

function el(tag) { return document.createElementNS(SVG_NS, tag); }

function setA(node, attrs) {
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v);
}

function rect(x, y, w, h, fill) {
  const r = el('rect');
  setA(r, { x, y, width: w, height: h, fill });
  return r;
}

function circle(cx, cy, r, fill, opacity) {
  const c = el('circle');
  setA(c, { cx, cy, r, fill });
  if (opacity !== undefined) c.setAttribute('opacity', opacity);
  return c;
}

function rand(lo, hi) { return lo + Math.random() * (hi - lo); }

function esc(s) { return s.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;'); }

function shortId(id) {
  return id.length > 12 ? id.slice(0, 8) + '...' : id;
}

function darken(hex, amount) {
  if (hex.startsWith('#')) {
    const r = Math.max(0, parseInt(hex.slice(1, 3), 16) - Math.floor(amount * 255));
    const g = Math.max(0, parseInt(hex.slice(3, 5), 16) - Math.floor(amount * 255));
    const b = Math.max(0, parseInt(hex.slice(5, 7), 16) - Math.floor(amount * 255));
    return `rgb(${r},${g},${b})`;
  }
  return hex;
}
