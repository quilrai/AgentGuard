// Garden view — project-level file/module breakdown.
//
// Each project is a garden. Inside:
//   - Grove        = top-level module (first path segment)
//   - Tree         = a file the agent has actually touched
//   - Tree height  = file size (est_tokens), sqrt-scaled so huge files
//                    don't dwarf everything
//   - Trunk width  = line count (code-heavy files look sturdier)
//   - Canopy blobs = touch count (how often Claude came back)
//   - Canopy glow  = work weight (tokens × touches)
//   - Bark color   = top backend touching the file
//   - Dead tree    = file no longer exists on disk (stale reference)
//   - Coin rows    = input token spend, rendered as an HTML overlay next
//                    to the Quilly icon. Two horizontal rows:
//                      gold   = full-price input (raw input + cache_creation)
//                      copper = cache_read
//                    1 gold ≈ 100K tokens; 1 copper ≈ 1M tokens.
//   - Sun          = cache hit ratio
//
// Data comes from two Tauri commands:
//   - get_garden_stats  → project list for the picker bar
//   - get_garden_detail → per-project file breakdown

import { invoke, formatNumber } from './utils.js';

const SVG_NS = 'http://www.w3.org/2000/svg';

// ---- State ----
let gardenTimeRange = 'all';
let currentCwd = null;
let projectList = [];
let gardenDetail = null;
let expandedFilePath = null;
let helpMode = false;

// ============ Public API ============

export function initGarden() {
  // Help mode toggle — labels every element on the scene.
  document.getElementById('garden-help-btn')?.addEventListener('click', () => {
    helpMode = !helpMode;
    const btn = document.getElementById('garden-help-btn');
    const svg = document.getElementById('garden-svg');
    const coinRows = document.getElementById('garden-coin-rows');
    if (btn) btn.classList.toggle('active', helpMode);
    if (svg) svg.classList.toggle('help-mode', helpMode);
    if (coinRows) coinRows.classList.toggle('help-mode', helpMode);
  });

  // Click on SVG background collapses any expanded tree.
  document.getElementById('garden-svg')?.addEventListener('click', (e) => {
    if (e.target.closest('.garden-tree')) return;
    if (expandedFilePath) {
      expandedFilePath = null;
      renderGardenScene();
    }
  });

  // Tooltip follows the mouse inside the scene wrap.
  const sceneWrap = document.querySelector('.garden-scene-wrap');
  if (sceneWrap) {
    sceneWrap.addEventListener('mousemove', (e) => {
      const tooltip = document.getElementById('garden-tooltip');
      if (tooltip && tooltip.style.display !== 'none') {
        const rect = sceneWrap.getBoundingClientRect();
        let x = e.clientX - rect.left + 14;
        let y = e.clientY - rect.top + 14;
        if (x + 260 > rect.width) x = e.clientX - rect.left - 270;
        if (y + 120 > rect.height) y = e.clientY - rect.top - 120;
        tooltip.style.left = `${x}px`;
        tooltip.style.top = `${y}px`;
      }
    });
  }

  // Quilly mascot toggles the stats panel.
  document.getElementById('quilly-icon')?.addEventListener('click', () => {
    const panel = document.getElementById('quilly-stats-panel');
    if (panel) {
      const showing = panel.style.display !== 'none';
      panel.style.display = showing ? 'none' : '';
    }
  });
}

export function loadGarden() {
  loadProjectList();
}

// ============ Project picker ============

function loadProjectList() {
  invoke('get_garden_stats', { timeRange: gardenTimeRange })
    .then(data => {
      projectList = data.projects || [];
      renderNameBar();
      if (projectList.length > 0) {
        if (!currentCwd || !projectList.find(p => p.cwd === currentCwd)) {
          currentCwd = projectList[0].cwd;
        }
        document.getElementById('garden-empty').style.display = 'none';
        document.getElementById('garden-scene').style.display = '';
        loadGardenDetail(currentCwd);
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
      expandedFilePath = null;
      highlightActivePill();
      loadGardenDetail(currentCwd);
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

// ============ Garden detail load ============

function loadGardenDetail(cwd) {
  expandedFilePath = null;
  invoke('get_garden_detail', { cwd, timeRange: gardenTimeRange })
    .then(data => {
      gardenDetail = data;
      renderGardenScene();
      renderStatsRow();
      renderPictogramBar();
      renderCoinRows(gardenDetail);
    })
    .catch(e => console.error('[garden]', e));
}

function renderStatsRow() {
  const row = document.getElementById('garden-stats-row');
  if (!row || !gardenDetail) return;
  const d = gardenDetail;
  const cacheRatio = d.total_input > 0
    ? Math.round((d.cache_read / d.total_input) * 100)
    : 0;
  const modules = new Set(d.files.map(f => topSegment(f.path))).size;
  row.innerHTML = `
    <div class="garden-stat"><span class="garden-stat-label">Files</span><span class="garden-stat-value">${d.files.length}</span></div>
    <div class="garden-stat"><span class="garden-stat-label">Modules</span><span class="garden-stat-value">${modules}</span></div>
    <div class="garden-stat"><span class="garden-stat-label">Input</span><span class="garden-stat-value">${formatNumber(d.total_input)}</span></div>
    <div class="garden-stat"><span class="garden-stat-label">Output</span><span class="garden-stat-value">${formatNumber(d.total_output)}</span></div>
    <div class="garden-stat"><span class="garden-stat-label">Cache %</span><span class="garden-stat-value">${cacheRatio}%</span></div>
    <div class="garden-stat"><span class="garden-stat-label">Requests</span><span class="garden-stat-value">${formatNumber(d.request_count)}</span></div>
  `;
}

function renderPictogramBar() {
  const bar = document.getElementById('garden-pictogram-bar');
  if (!bar || !gardenDetail) return;
  const d = gardenDetail;

  // Top 3 modules by combined token×touch weight — the "where is Claude
  // actually working" answer up front.
  const modAgg = new Map();
  for (const f of d.files) {
    const k = topSegment(f.path);
    const w = Math.max(f.est_tokens, 1) * Math.max(f.touch_count, 1);
    modAgg.set(k, (modAgg.get(k) || 0) + w);
  }
  const topMods = [...modAgg.entries()].sort((a, b) => b[1] - a[1]).slice(0, 3);
  const modTotal = [...modAgg.values()].reduce((a, b) => a + b, 0) || 1;

  const hottestFile = d.files[0]; // already sorted by weight from backend
  const totalTouches = d.files.reduce((a, f) => a + f.touch_count, 0);

  const modChips = topMods.map(([name, w]) => {
    const pct = Math.round((w / modTotal) * 100);
    return `<span class="garden-mod-chip" title="${esc(name)} — ${pct}% of agent work weight"><span class="garden-mod-dot" style="background:${moduleColor(name)}"></span>${esc(name)} ${pct}%</span>`;
  }).join('');

  bar.innerHTML = `
    <span class="garden-pictogram" title="Files the agent has actually touched in this project.">
      <svg class="garden-pictogram-icon" viewBox="0 0 16 16"><rect x="7" y="6" width="2" height="10" rx="1" fill="#5a3a20"/><circle cx="8" cy="5" r="5" fill="#4a8a3a"/></svg>
      <span class="garden-pictogram-value">${d.files.length}</span> files
    </span>
    <span class="garden-pictogram" title="Total tool-call touches across every file.">
      <svg class="garden-pictogram-icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5" fill="none" stroke="#71D083" stroke-width="1.5"/><circle cx="8" cy="8" r="1.5" fill="#71D083"/></svg>
      <span class="garden-pictogram-value">${formatNumber(totalTouches)}</span> touches
    </span>
    <span class="garden-pictogram" title="The file with the highest estimated work weight (size × touches).">
      <svg class="garden-pictogram-icon" viewBox="0 0 16 16"><path d="M8 1l1.8 4.5 4.7.4-3.6 3.1 1.1 4.7L8 11.3 3.9 13.7l1.1-4.7L1.5 5.9l4.7-.4z" fill="#f5c542"/></svg>
      <span class="garden-pictogram-value">${hottestFile ? esc(baseName(hottestFile.path)) : '—'}</span> hottest
    </span>
    <span class="garden-mod-chips">${modChips}</span>
  `;
}

// ============ SVG scene ============

function renderGardenScene() {
  const svg = document.getElementById('garden-svg');
  if (!svg || !gardenDetail) return;
  svg.innerHTML = '';
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
    <radialGradient id="gSunGlow" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="rgba(245,197,66,0.25)"/>
      <stop offset="100%" stop-color="transparent"/>
    </radialGradient>
    <filter id="fGlow">
      <feGaussianBlur in="SourceGraphic" stdDeviation="2.5" result="b"/>
      <feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge>
    </filter>
  `;
  svg.appendChild(defs);

  // ---- Sky + stars ----
  svg.appendChild(rect(0, 0, W, H, 'url(#gSky)'));
  for (let i = 0; i < 50; i++) {
    const s = el('circle');
    s.setAttribute('class', 'garden-star');
    setA(s, { cx: rand(0, W), cy: rand(0, GROUND_Y * 0.5), r: rand(0.4, 1.4), fill: '#c8dcff' });
    s.style.animationDelay = `${-rand(0, 3)}s`;
    svg.appendChild(s);
  }

  // ---- Sun (cache read ratio) ----
  drawSun(svg, gardenDetail);

  // ---- Ground ----
  svg.appendChild(rect(0, GROUND_Y, W, H - GROUND_Y, 'url(#gGround)'));
  const gl = el('line');
  setA(gl, { x1: 0, y1: GROUND_Y, x2: W, y2: GROUND_Y, stroke: '#3d5e2a', 'stroke-width': 2, opacity: 0.5 });
  svg.appendChild(gl);

  // (Coin rows are rendered as an HTML overlay, not inside this SVG —
  // see renderCoinRows, called from loadGardenDetail.)

  // ---- Empty state ----
  if (!gardenDetail.files || gardenDetail.files.length === 0) {
    const msg = el('text');
    setA(msg, { x: W / 2, y: GROUND_Y - 120, 'text-anchor': 'middle', fill: '#666', 'font-size': 16, 'font-family': '-apple-system, sans-serif' });
    msg.textContent = 'No files touched yet in the selected time range.';
    svg.appendChild(msg);
    return;
  }

  // ---- Groves & trees ----
  drawGroves(svg, gardenDetail.files, W, GROUND_Y);

  // ---- Fireflies (ambient) ----
  for (let i = 0; i < 5; i++) {
    const f = el('circle');
    f.setAttribute('class', 'garden-firefly');
    setA(f, { cx: rand(50, W - 50), cy: rand(GROUND_Y - 200, GROUND_Y - 30), r: 2, fill: '#b8e986', filter: 'url(#fGlow)' });
    f.style.animationDelay = `${-rand(0, 3)}s`;
    svg.appendChild(f);
  }
}

// ---- Groves: group files by top-level segment, lay out side by side ----

function drawGroves(svg, files, W, groundY) {
  // Bucket by top path segment.
  const groveMap = new Map();
  for (const f of files) {
    const k = topSegment(f.path);
    if (!groveMap.has(k)) groveMap.set(k, []);
    groveMap.get(k).push(f);
  }

  const groveWeight = (fs) => fs.reduce((a, f) =>
    a + Math.max(f.est_tokens, 1) * Math.max(f.touch_count, 1), 0);
  const groves = [...groveMap.entries()]
    .sort((a, b) => groveWeight(b[1]) - groveWeight(a[1]));

  const maxTokens = Math.max(...files.map(f => f.est_tokens), 1);
  const maxTouches = Math.max(...files.map(f => f.touch_count), 1);

  // Weight-proportional widths with a clamp so tiny groves still render
  // and huge ones don't hog the whole canvas.
  const padL = 60, padR = 60;
  const zoneW = W - padL - padR;
  const totalWeight = groves.reduce((a, [, fs]) => a + groveWeight(fs), 0) || 1;

  let cursorX = padL;
  for (const [modName, fs] of groves) {
    const w = groveWeight(fs);
    const rawWidth = (w / totalWeight) * zoneW;
    const groveWidth = Math.max(Math.min(rawWidth, zoneW * 0.6), 90);
    drawGrove(svg, modName, fs, cursorX, groveWidth, groundY, maxTokens, maxTouches);
    cursorX += groveWidth + 18;
  }
}

function drawGrove(svg, modName, files, x0, width, groundY, maxTokens, maxTouches) {
  const groveG = el('g');
  groveG.setAttribute('class', 'garden-grove');

  // Cap trees per grove so very hot modules don't choke the SVG.
  const visible = [...files]
    .sort((a, b) =>
      (b.est_tokens * Math.max(b.touch_count, 1)) -
      (a.est_tokens * Math.max(a.touch_count, 1)))
    .slice(0, 18);

  const accent = moduleColor(modName);

  // Grove floor: subtle dark patch under the trees to bind the grove.
  const floor = el('ellipse');
  setA(floor, {
    cx: x0 + width / 2,
    cy: groundY + 4,
    rx: width / 2 - 4,
    ry: 10,
    fill: '#0f1810',
    opacity: 0.5,
  });
  groveG.appendChild(floor);

  // Trees.
  const count = visible.length;
  const spacing = width / (count + 1);
  visible.forEach((file, i) => {
    const cx = x0 + spacing * (i + 1);
    drawFileTree(groveG, cx, groundY, file, maxTokens, maxTouches, accent);
  });

  // Grove label (module name) below the ground line.
  const labelY = groundY + 40;
  const lbl = el('text');
  lbl.setAttribute('class', 'garden-grove-label');
  setA(lbl, {
    x: x0 + width / 2,
    y: labelY,
    'text-anchor': 'middle',
    fill: accent,
    'font-size': 11,
    'font-weight': 600,
    'font-family': '-apple-system, sans-serif',
  });
  lbl.textContent = modName;
  groveG.appendChild(lbl);

  // File count sub-label.
  const sub = el('text');
  sub.setAttribute('class', 'garden-grove-sub');
  setA(sub, {
    x: x0 + width / 2,
    y: labelY + 13,
    'text-anchor': 'middle',
    fill: '#666',
    'font-size': 9,
    'font-family': '-apple-system, sans-serif',
  });
  const hidden = files.length - visible.length;
  sub.textContent = hidden > 0
    ? `${files.length} files (+${hidden} hidden)`
    : `${files.length} file${files.length === 1 ? '' : 's'}`;
  groveG.appendChild(sub);

  svg.appendChild(groveG);
}

// ---- One tree per file ----

function drawFileTree(parent, cx, groundY, file, maxTokens, maxTouches, accent) {
  // Sqrt scaling so a 50k-token file isn't 100× taller than a 500-token one.
  const tokenRatio = Math.sqrt(file.est_tokens / Math.max(maxTokens, 1));
  const heightRatio = Math.max(tokenRatio, 0.12);
  const treeH = 50 + heightRatio * 200;

  const lineRatio = Math.min(file.lines / 2000, 1);
  const trunkW = 4 + lineRatio * 10;

  const canopyR = 18 + heightRatio * 42;
  const touchRatio = Math.min(file.touch_count / Math.max(maxTouches, 1), 1);

  const trunkTop = groundY - treeH;

  const treeG = el('g');
  treeG.setAttribute('class', 'garden-tree garden-hover-target');
  treeG.dataset.path = file.path;

  const isExpanded = expandedFilePath === file.path;
  if (expandedFilePath && !isExpanded) {
    treeG.setAttribute('opacity', '0.28');
  }

  // Trunk (darker for dead/missing files).
  const trunkColor = file.exists ? '#5a3a20' : '#3a2815';
  const trunk = el('rect');
  trunk.setAttribute('class', 'garden-trunk');
  setA(trunk, {
    x: cx - trunkW / 2,
    y: trunkTop,
    width: trunkW,
    height: treeH,
    rx: trunkW / 3,
    fill: trunkColor,
  });
  treeG.appendChild(trunk);

  if (file.exists) {
    // Healthy canopy. Backend tint if we can identify one.
    const barkAccent = file.backend_touches && file.backend_touches.length > 0
      ? backendColor(file.backend_touches[0][0])
      : accent;

    const darkC = darken(barkAccent, 0.4);
    const blobs = 3 + Math.floor(touchRatio * 4);
    for (let i = 0; i < blobs; i++) {
      const a = (i / blobs) * Math.PI * 2 - Math.PI / 2;
      const d = canopyR * 0.3;
      const bx = cx + Math.cos(a) * d;
      const by = trunkTop + 3 + Math.sin(a) * d * 0.55;
      const br = canopyR * (0.55 + Math.random() * 0.35);
      const blob = el('circle');
      blob.setAttribute('class', 'garden-leaf');
      setA(blob, { cx: bx, cy: by, r: br, fill: i % 2 === 0 ? barkAccent : darkC, opacity: 0.78 });
      blob.style.animationDelay = `${-i * 0.6}s`;
      treeG.appendChild(blob);
    }
    // Core glow — brightness tracks work weight.
    const core = el('circle');
    const glowOpacity = 0.55 + Math.min(touchRatio, 0.5);
    setA(core, {
      cx, cy: trunkTop + 3,
      r: canopyR * 0.45,
      fill: barkAccent,
      opacity: glowOpacity,
      filter: 'url(#fGlow)',
    });
    treeG.appendChild(core);

    // Multi-backend: show a secondary-backend splash at the base.
    if (file.backend_touches && file.backend_touches.length > 1) {
      const total = file.backend_touches.reduce((a, b) => a + b[1], 0);
      let px = cx - 10;
      for (const [bk, ct] of file.backend_touches) {
        const w = Math.max(2, Math.round((ct / total) * 20));
        const bar = el('rect');
        setA(bar, { x: px, y: groundY + 2, width: w, height: 3, rx: 1, fill: backendColor(bk), opacity: 0.85 });
        treeG.appendChild(bar);
        px += w + 1;
      }
    }
  } else {
    // Dead tree: bare branches, no canopy.
    const bCount = 4;
    for (let b = 0; b < bCount; b++) {
      const by = trunkTop + treeH * (0.15 + b * 0.15);
      const dir = b % 2 === 0 ? 1 : -1;
      const bLen = 10 + b * 3;
      const br = el('line');
      setA(br, {
        x1: cx, y1: by,
        x2: cx + dir * bLen, y2: by - rand(6, 14),
        stroke: '#4a3a28', 'stroke-width': 1.5, 'stroke-linecap': 'round',
      });
      treeG.appendChild(br);
    }
    const ghost = el('text');
    setA(ghost, { x: cx, y: trunkTop - 6, 'text-anchor': 'middle', fill: '#555', 'font-size': 10 });
    ghost.textContent = '\u2205'; // empty-set symbol
    treeG.appendChild(ghost);
  }

  // Filename label under the tree.
  const nameLbl = el('text');
  nameLbl.setAttribute('class', 'garden-file-label');
  setA(nameLbl, { x: cx, y: groundY + 16, 'text-anchor': 'middle', fill: '#999', 'font-size': 9, 'font-family': '-apple-system, sans-serif' });
  nameLbl.textContent = truncate(baseName(file.path), 14);
  treeG.appendChild(nameLbl);

  // Size label.
  const sizeLbl = el('text');
  sizeLbl.setAttribute('class', 'garden-file-size');
  setA(sizeLbl, { x: cx, y: groundY + 27, 'text-anchor': 'middle', fill: '#555', 'font-size': 8, 'font-family': '-apple-system, sans-serif' });
  sizeLbl.textContent = file.exists ? `${formatNumber(file.est_tokens)} · ${file.touch_count}×` : 'missing';
  treeG.appendChild(sizeLbl);

  // Help-mode detail label above the trunk.
  const helpLbl = el('text');
  helpLbl.setAttribute('class', 'garden-help-label');
  setA(helpLbl, { x: cx, y: trunkTop - 20, 'text-anchor': 'middle' });
  helpLbl.textContent = `${file.lines} lines · ${file.touch_count} touches`;
  treeG.appendChild(helpLbl);

  // Tooltip + expand interactions.
  treeG.addEventListener('mouseenter', () => {
    if (isExpanded) return;
    const topBk = file.backend_touches && file.backend_touches[0]
      ? file.backend_touches[0][0] : 'unknown';
    const existMsg = file.exists
      ? `${file.lines} lines · ~${formatNumber(file.est_tokens)} tokens`
      : 'File no longer exists on disk (stale reference)';
    showTooltip(
      esc(file.path),
      `${existMsg} · ${file.touch_count} touch${file.touch_count === 1 ? '' : 'es'}`,
      `Top backend: ${esc(topBk)}. Last touched: ${formatTs(file.last_touched)}. Click for details.`
    );
  });
  treeG.addEventListener('mouseleave', hideTooltip);
  treeG.addEventListener('click', (e) => {
    e.stopPropagation();
    expandedFilePath = expandedFilePath === file.path ? null : file.path;
    renderGardenScene();
  });

  // Expanded: inline info card instead of tooltip.
  if (isExpanded) {
    drawFileInfoCard(treeG, cx, groundY, trunkTop, canopyR, treeH, file);
  }

  parent.appendChild(treeG);
}

function drawFileInfoCard(group, cx, groundY, trunkTop, canopyR, treeH, file) {
  const g = el('g');
  g.setAttribute('class', 'garden-tree-info');

  const pad = 10;
  const cardW = 220;
  const cardH = 130;
  const cardX = cx + canopyR + 14;
  const cardY = trunkTop;

  const bg = el('rect');
  setA(bg, {
    x: cardX, y: cardY, width: cardW, height: cardH,
    rx: 8, fill: '#1a1d1eee', stroke: '#3a3d3e', 'stroke-width': 1,
  });
  g.appendChild(bg);

  const title = el('text');
  setA(title, { x: cardX + pad, y: cardY + pad + 10, fill: '#e0e0e0', 'font-size': 11, 'font-weight': 600, 'font-family': '-apple-system, sans-serif' });
  title.textContent = truncate(file.path, 32);
  g.appendChild(title);

  const rows = [
    { label: 'Lines', value: file.exists ? String(file.lines) : '—', color: '#71D083' },
    { label: 'Tokens', value: file.exists ? formatNumber(file.est_tokens) : '—', color: '#f5c542' },
    { label: 'Touches', value: String(file.touch_count), color: '#5b9bd5' },
    { label: 'Last', value: formatTs(file.last_touched), color: '#aaa' },
  ];
  rows.forEach((r, i) => {
    const y = cardY + pad + 28 + i * 16;
    const lbl = el('text');
    setA(lbl, { x: cardX + pad, y, fill: '#777', 'font-size': 9, 'font-family': '-apple-system, sans-serif' });
    lbl.textContent = r.label;
    g.appendChild(lbl);
    const val = el('text');
    setA(val, { x: cardX + cardW - pad, y, 'text-anchor': 'end', fill: r.color, 'font-size': 10, 'font-weight': 600, 'font-family': '-apple-system, sans-serif' });
    val.textContent = r.value;
    g.appendChild(val);
  });

  // Backend breakdown bar.
  if (file.backend_touches && file.backend_touches.length > 0) {
    const barY = cardY + cardH - 18;
    const barW = cardW - pad * 2;
    const total = file.backend_touches.reduce((a, b) => a + b[1], 0);
    let bx = cardX + pad;
    file.backend_touches.forEach(([bk, ct]) => {
      const w = Math.max(2, Math.round((ct / total) * barW));
      const seg = el('rect');
      setA(seg, { x: bx, y: barY, width: w, height: 5, fill: backendColor(bk), opacity: 0.85 });
      g.appendChild(seg);
      bx += w;
    });
    const blbl = el('text');
    setA(blbl, { x: cardX + pad, y: barY - 3, fill: '#666', 'font-size': 8, 'font-family': '-apple-system, sans-serif' });
    blbl.textContent = 'backends';
    g.appendChild(blbl);
  }

  group.appendChild(g);
}

// ---- Sun & Coin pile ----

function drawSun(svg, detail) {
  const ratio = detail.total_input > 0 ? detail.cache_read / detail.total_input : 0;
  const sunR = 20 + ratio * 30;
  const opacity = 0.3 + ratio * 0.7;
  const cx = 100, cy = 100;

  const grp = el('g');
  grp.setAttribute('class', 'garden-sun-group garden-hover-target');

  const glow = el('circle');
  glow.setAttribute('class', 'garden-sun-glow');
  setA(glow, { cx, cy, r: sunR * 3, fill: 'url(#gSunGlow)', opacity });
  grp.appendChild(glow);

  if (ratio > 0.05) {
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
      grp.appendChild(ray);
    }
  }

  const disc = el('circle');
  setA(disc, { cx, cy, r: sunR, fill: '#f5c542', opacity, filter: 'url(#fGlow)' });
  grp.appendChild(disc);

  const lbl = el('text');
  lbl.setAttribute('class', 'garden-label-dim');
  setA(lbl, { x: cx, y: cy + sunR + 16, 'text-anchor': 'middle', fill: '#888', 'font-size': 10 });
  lbl.textContent = `${Math.round(ratio * 100)}% cache`;
  grp.appendChild(lbl);

  grp.addEventListener('mouseenter', () => {
    showTooltip(
      'Sun \u2014 Cache Read',
      `${Math.round(ratio * 100)}% hit rate \u00B7 ${formatNumber(detail.cache_read)} tokens from cache`,
      'Bright sun means most context is being served from the prompt cache — the cheapest path.'
    );
  });
  grp.addEventListener('mouseleave', hideTooltip);

  svg.appendChild(grp);
}

// ---- Coin rows (HTML overlay) ----
//
// Rendered as HTML siblings of the Quilly mascot inside .garden-scene-wrap
// rather than as SVG children, so they share Quilly's coordinate space.
// The SVG uses preserveAspectRatio="meet" and gets letterboxed whenever
// the wrap doesn't match the 1200×700 aspect — anything drawn in viewBox
// coords drifts away from HTML siblings under resize, which is how the
// earlier SVG version ended up overlapping Quilly.
//
// Fixed denominations:
//   1 gold   ≈ 100K full-price tokens (raw input + cache_creation)
//   1 copper ≈ 1M cached tokens       (cache_read)
// Chosen so one gold coin ≈ one copper coin in dollar value (gold 1× base,
// copper ~0.1× base), letting row length read as comparable cost.

function renderCoinRows(detail) {
  const container = document.getElementById('garden-coin-rows');
  if (!container) return;
  container.innerHTML = '';
  if (!detail) return;

  const goldTokens = Math.max(0, detail.total_input - detail.cache_read);
  const copperTokens = Math.max(0, detail.cache_read);
  if (goldTokens + copperTokens <= 0) return;

  // At least 1 coin for any non-zero bucket, capped at 20 so a huge
  // project doesn't extend the row off-screen.
  const coinsFor = (t, denom) => {
    if (t <= 0) return 0;
    return Math.max(1, Math.min(20, Math.round(t / denom)));
  };
  const goldCoins = coinsFor(goldTokens, 100_000);
  const copperCoins = coinsFor(copperTokens, 1_000_000);

  if (goldCoins > 0) {
    container.appendChild(makeCoinRow('gold', goldCoins, goldTokens,
      'Gold \u2014 Full-price input',
      `${formatNumber(goldTokens)} tokens`,
      'Raw input plus cache writes. Each gold coin represents ~100K tokens.'));
  }
  if (copperCoins > 0) {
    container.appendChild(makeCoinRow('copper', copperCoins, copperTokens,
      'Copper \u2014 Cached input',
      `${formatNumber(copperTokens)} tokens`,
      'Cache reads \u2014 the cheap bucket. Each copper coin represents ~1M tokens.'));
  }
}

function makeCoinRow(kind, count, tokens, title, metric, explain) {
  const row = document.createElement('div');
  row.className = 'garden-coin-row';
  row.dataset.help = kind === 'gold' ? 'Full-price' : 'Cache hit';

  const label = document.createElement('span');
  label.className = 'garden-coin-label';
  label.textContent = formatNumber(tokens);
  row.appendChild(label);

  const stack = document.createElement('span');
  stack.className = 'garden-coin-stack';
  for (let i = 0; i < count; i++) {
    const coin = document.createElement('span');
    coin.className = `garden-coin ${kind}`;
    stack.appendChild(coin);
  }
  row.appendChild(stack);

  row.addEventListener('mouseenter', () => showTooltip(title, metric, explain));
  row.addEventListener('mouseleave', hideTooltip);

  return row;
}

// ---- Tooltip ----

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

// ---- Helpers ----

function el(tag) { return document.createElementNS(SVG_NS, tag); }
function setA(node, attrs) { for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v); }
function rect(x, y, w, h, fill) {
  const r = el('rect');
  setA(r, { x, y, width: w, height: h, fill });
  return r;
}
function rand(lo, hi) { return lo + Math.random() * (hi - lo); }
function esc(s) { return String(s).replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;'); }
function truncate(s, n) { return s.length > n ? s.slice(0, n - 1) + '\u2026' : s; }
function baseName(p) {
  const parts = p.split('/').filter(Boolean);
  return parts[parts.length - 1] || p;
}
function topSegment(p) {
  // First meaningful path segment. Files directly at the project root
  // are bucketed into "(root)" so they still get a grove of their own.
  const parts = p.split('/').filter(Boolean);
  if (parts.length === 0) return '(root)';
  if (parts.length === 1) return '(root)';
  return parts[0];
}

function formatTs(ts) {
  if (!ts) return '—';
  try {
    const d = new Date(ts);
    const now = Date.now();
    const diff = Math.max(0, now - d.getTime());
    if (diff < 60_000) return 'just now';
    if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
    if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
    return `${Math.floor(diff / 86_400_000)}d ago`;
  } catch {
    return ts;
  }
}

// Deterministic HSL color per module name — same module always looks the same.
function moduleColor(name) {
  let h = 0;
  for (let i = 0; i < name.length; i++) {
    h = (h * 31 + name.charCodeAt(i)) | 0;
  }
  const hue = Math.abs(h) % 360;
  return `hsl(${hue}, 55%, 58%)`;
}

function backendColor(backend) {
  const b = (backend || '').toLowerCase();
  if (b.includes('claude')) return '#4a8a3a';
  if (b.includes('codex')) return '#3a6a8a';
  if (b.includes('cursor')) return '#7a5a8a';
  return '#5a7a3a';
}

function darken(input, amount) {
  if (input.startsWith('#')) {
    const r = Math.max(0, parseInt(input.slice(1, 3), 16) - Math.floor(amount * 255));
    const g = Math.max(0, parseInt(input.slice(3, 5), 16) - Math.floor(amount * 255));
    const b = Math.max(0, parseInt(input.slice(5, 7), 16) - Math.floor(amount * 255));
    return `rgb(${r},${g},${b})`;
  }
  if (input.startsWith('hsl(')) {
    return input.replace(/hsl\(\s*([\d.]+)\s*,\s*([\d.]+)%\s*,\s*([\d.]+)%\s*\)/,
      (_, h, s, l) => `hsl(${h}, ${s}%, ${Math.max(0, parseFloat(l) - amount * 100)}%)`);
  }
  return input;
}
