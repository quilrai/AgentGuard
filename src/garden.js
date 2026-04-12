// Garden view — project-level file/module breakdown.
//
// Each project is a garden. Inside:
//   - Grove        = top-level module (first path segment)
//   - Clearing     = sub-folder cluster within a grove
//   - Hero tree    = a heavily-touched file rendered as a full conical tree
//   - Bush         = a lightly-touched file rendered as small undergrowth
//   - Tree height  = file size (est_tokens), sqrt-scaled
//   - Trunk width  = line count (code-heavy files look sturdier)
//   - Canopy tiers = stacked triangles; more tiers = more touches
//   - Canopy color = top backend touching the file
//   - Dead tree    = file no longer exists on disk (bare branches)
//   - Depth        = hero trees in foreground (lower, bigger), bushes in
//                    background (higher, smaller, fainter)
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
// Symbols cache: { [filePath]: { symbols: [...], loading: bool } }
let symbolsCache = {};
let importGraph = null;

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
      hideSymbolPanel();
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
  symbolsCache = {};
  importGraph = null;
  hideSymbolPanel();
  invoke('get_garden_detail', { cwd, timeRange: gardenTimeRange })
    .then(data => {
      gardenDetail = data;
      renderGardenScene();
      renderStatsRow();
      renderPictogramBar();
      renderCoinRows(gardenDetail);
      // Load import graph in the background.
      invoke('get_import_graph', { cwd })
        .then(graph => {
          importGraph = graph;
          renderImportEdges();
        })
        .catch(() => {}); // non-critical
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
    <filter id="fTreeShadow">
      <feDropShadow dx="0" dy="2" stdDeviation="3" flood-color="#000" flood-opacity="0.3"/>
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

  // ---- Empty state ----
  if (!gardenDetail.files || gardenDetail.files.length === 0) {
    const msg = el('text');
    setA(msg, { x: W / 2, y: GROUND_Y - 120, 'text-anchor': 'middle', fill: '#666', 'font-size': 16, 'font-family': '-apple-system, sans-serif' });
    msg.textContent = 'No files touched yet in the selected time range.';
    svg.appendChild(msg);
    return;
  }

  // ---- Groves with clearings ----
  drawGroves(svg, gardenDetail.files, W, GROUND_Y);

  // ---- Fireflies (ambient) ----
  for (let i = 0; i < 8; i++) {
    const f = el('circle');
    f.setAttribute('class', 'garden-firefly');
    setA(f, { cx: rand(50, W - 50), cy: rand(GROUND_Y - 280, GROUND_Y - 30), r: 2, fill: '#b8e986', filter: 'url(#fGlow)' });
    f.style.animationDelay = `${-rand(0, 3)}s`;
    svg.appendChild(f);
  }
}

// ---- Work weight helper ----
function fileWeight(f) {
  return Math.max(f.est_tokens, 1) * Math.max(f.touch_count, 1);
}

// ---- Groves: group files by top-level segment, lay out side by side ----

function drawGroves(svg, files, W, groundY) {
  const groveMap = new Map();
  for (const f of files) {
    const k = topSegment(f.path);
    if (!groveMap.has(k)) groveMap.set(k, []);
    groveMap.get(k).push(f);
  }

  const groveWeight = (fs) => fs.reduce((a, f) => a + fileWeight(f), 0);
  const groves = [...groveMap.entries()]
    .sort((a, b) => groveWeight(b[1]) - groveWeight(a[1]));

  const maxTokens = Math.max(...files.map(f => f.est_tokens), 1);
  const maxTouches = Math.max(...files.map(f => f.touch_count), 1);

  const padL = 50, padR = 50;
  const zoneW = W - padL - padR;
  const totalWeight = groves.reduce((a, [, fs]) => a + groveWeight(fs), 0) || 1;

  let cursorX = padL;
  for (const [modName, fs] of groves) {
    const w = groveWeight(fs);
    const rawWidth = (w / totalWeight) * zoneW;
    const groveWidth = Math.max(Math.min(rawWidth, zoneW * 0.55), 100);
    drawGrove(svg, modName, fs, cursorX, groveWidth, groundY, maxTokens, maxTouches);
    cursorX += groveWidth + 24;
  }
}

function drawGrove(svg, modName, files, x0, width, groundY, maxTokens, maxTouches) {
  const groveG = el('g');
  groveG.setAttribute('class', 'garden-grove');

  const accent = moduleColor(modName);

  // ---- Sub-folder clearings ----
  // Group files by their second path segment (sub-folder within the module).
  const clearingMap = new Map();
  for (const f of files) {
    const k = subSegment(f.path);
    if (!clearingMap.has(k)) clearingMap.set(k, []);
    clearingMap.get(k).push(f);
  }

  const clearingWeight = (fs) => fs.reduce((a, f) => a + fileWeight(f), 0);
  const clearings = [...clearingMap.entries()]
    .sort((a, b) => clearingWeight(b[1]) - clearingWeight(a[1]));

  // Distribute clearings across the grove width.
  const totalCW = clearings.reduce((a, [, fs]) => a + clearingWeight(fs), 0) || 1;
  let cx = x0;

  for (const [subName, cFiles] of clearings) {
    const cw = clearingWeight(cFiles);
    const rawW = (cw / totalCW) * width;
    const clearingW = Math.max(Math.min(rawW, width * 0.7), 60);

    drawClearing(groveG, subName, cFiles, cx, clearingW, groundY, maxTokens, maxTouches, accent);
    cx += clearingW + 12;
  }

  // Grove label below ground.
  const labelY = groundY + 52;
  const lbl = el('text');
  lbl.setAttribute('class', 'garden-grove-label');
  setA(lbl, {
    x: x0 + width / 2, y: labelY,
    'text-anchor': 'middle', fill: accent,
    'font-size': 12, 'font-weight': 700,
    'font-family': '-apple-system, sans-serif',
  });
  lbl.textContent = modName;
  groveG.appendChild(lbl);

  // File count sub-label.
  const sub = el('text');
  sub.setAttribute('class', 'garden-grove-sub');
  setA(sub, {
    x: x0 + width / 2, y: labelY + 14,
    'text-anchor': 'middle', fill: '#555',
    'font-size': 9, 'font-family': '-apple-system, sans-serif',
  });
  sub.textContent = `${files.length} file${files.length === 1 ? '' : 's'}`;
  groveG.appendChild(sub);

  svg.appendChild(groveG);
}

// ---- Clearing: sub-folder cluster with hero trees + undergrowth ----

function drawClearing(parent, subName, files, x0, width, groundY, maxTokens, maxTouches, groveAccent) {
  const clearingG = el('g');
  clearingG.setAttribute('class', 'garden-clearing');

  // Sort by weight descending.
  const sorted = [...files].sort((a, b) => fileWeight(b) - fileWeight(a));

  // Hero trees: top 3 (or fewer). Rest become bushes.
  const heroCount = Math.min(3, sorted.length);
  const heroes = sorted.slice(0, heroCount);
  const bushes = sorted.slice(heroCount);

  // Clearing floor ellipse.
  const floor = el('ellipse');
  setA(floor, {
    cx: x0 + width / 2, cy: groundY + 4,
    rx: width / 2 + 4, ry: 12,
    fill: '#0f1a0e', opacity: 0.45,
  });
  clearingG.appendChild(floor);

  // Draw hero trees with depth: heaviest in front (lower, bigger),
  // lighter ones behind (higher, smaller).
  const heroSpacing = width / (heroCount + 1);
  heroes.forEach((file, i) => {
    const treeCx = x0 + heroSpacing * (i + 1);
    // Depth: index 0 (heaviest) is foreground, last is background.
    const depthFactor = heroCount > 1 ? i / (heroCount - 1) : 0;
    const depthScale = 1.0 - depthFactor * 0.25; // foreground 1.0, background 0.75
    const depthY = groundY - depthFactor * 20;    // background trees sit higher
    const depthOpacity = 1.0 - depthFactor * 0.15;

    drawConicalTree(clearingG, treeCx, depthY, file, maxTokens, maxTouches, groveAccent, depthScale, depthOpacity, true);
  });

  // Draw bushes (undergrowth) as small clustered shapes behind the heroes.
  if (bushes.length > 0) {
    drawUndergrowth(clearingG, bushes, x0, width, groundY, maxTokens, maxTouches, groveAccent);
  }

  // Clearing sub-label (sub-folder name) — only if it's not "(files)" root.
  if (subName !== '(files)') {
    const slbl = el('text');
    slbl.setAttribute('class', 'garden-clearing-label');
    setA(slbl, {
      x: x0 + width / 2, y: groundY + 22,
      'text-anchor': 'middle', fill: '#555',
      'font-size': 8, 'font-family': '-apple-system, sans-serif',
      'font-style': 'italic',
    });
    slbl.textContent = subName;
    clearingG.appendChild(slbl);
  }

  parent.appendChild(clearingG);
}

// ---- Conical tree (hero) ----

function drawConicalTree(parent, cx, groundY, file, maxTokens, maxTouches, accent, scale, opacity, isHero) {
  const tokenRatio = Math.sqrt(file.est_tokens / Math.max(maxTokens, 1));
  const heightRatio = Math.max(tokenRatio, 0.15);
  const baseH = isHero ? (70 + heightRatio * 180) : (30 + heightRatio * 60);
  const treeH = baseH * scale;

  const lineRatio = Math.min(file.lines / 2000, 1);
  const trunkW = (isHero ? (5 + lineRatio * 8) : (3 + lineRatio * 4)) * scale;
  const trunkH = treeH * 0.3;

  const touchRatio = Math.min(file.touch_count / Math.max(maxTouches, 1), 1);
  // Number of canopy tiers: 2-4 based on touches.
  const tiers = Math.max(2, Math.min(4, Math.floor(2 + touchRatio * 2.5)));

  const trunkTop = groundY - trunkH;
  const treeTop = groundY - treeH;

  const treeG = el('g');
  treeG.setAttribute('class', 'garden-tree garden-hover-target');
  treeG.dataset.path = file.path;
  if (opacity < 1) treeG.setAttribute('opacity', String(opacity));

  const isExpanded = expandedFilePath === file.path;
  if (expandedFilePath && !isExpanded) {
    treeG.setAttribute('opacity', '0.25');
  }

  // Backend color for canopy.
  const canopyColor = file.backend_touches && file.backend_touches.length > 0
    ? backendColor(file.backend_touches[0][0])
    : accent;

  if (file.exists) {
    // ---- Trunk ----
    const trunk = el('rect');
    trunk.setAttribute('class', 'garden-trunk');
    setA(trunk, {
      x: cx - trunkW / 2, y: trunkTop,
      width: trunkW, height: trunkH + 2,
      rx: trunkW / 3, fill: '#5a3a20',
    });
    treeG.appendChild(trunk);

    // ---- Conical canopy tiers ----
    const canopyH = treeH - trunkH;
    const baseW = (isHero ? (28 + heightRatio * 35) : (16 + heightRatio * 18)) * scale;
    const darkC = darken(canopyColor, 0.3);

    for (let t = 0; t < tiers; t++) {
      const tierFrac = t / tiers;
      const tierBottom = trunkTop - canopyH * tierFrac + 6;
      const tierTop = trunkTop - canopyH * ((t + 1) / tiers);
      const tierW = baseW * (1.0 - tierFrac * 0.4); // wider at bottom, narrower at top
      const tierH = tierBottom - tierTop;

      // Main triangle.
      const tri = el('polygon');
      tri.setAttribute('class', 'garden-canopy-tier');
      const points = `${cx},${tierTop} ${cx - tierW / 2},${tierBottom} ${cx + tierW / 2},${tierBottom}`;
      setA(tri, {
        points,
        fill: t % 2 === 0 ? canopyColor : darkC,
        opacity: 0.85 - t * 0.05,
      });
      tri.style.animationDelay = `${-t * 0.5}s`;
      treeG.appendChild(tri);
    }

    // Glow at canopy center — brighter for heavily worked files.
    const glowOpacity = 0.2 + Math.min(touchRatio * 0.5, 0.45);
    const glow = el('ellipse');
    setA(glow, {
      cx, cy: treeTop + canopyH * 0.4,
      rx: baseW * 0.25, ry: canopyH * 0.25,
      fill: canopyColor, opacity: glowOpacity,
      filter: 'url(#fGlow)',
    });
    treeG.appendChild(glow);

    // Multi-backend indicator at trunk base.
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
    // ---- Dead tree: bare trunk + branches, no canopy ----
    const trunk = el('rect');
    trunk.setAttribute('class', 'garden-trunk');
    setA(trunk, {
      x: cx - trunkW / 2, y: treeTop + treeH * 0.3,
      width: trunkW, height: treeH * 0.7,
      rx: trunkW / 3, fill: '#3a2815',
    });
    treeG.appendChild(trunk);

    const bCount = 4;
    for (let b = 0; b < bCount; b++) {
      const by = treeTop + treeH * (0.3 + b * 0.12);
      const dir = b % 2 === 0 ? 1 : -1;
      const bLen = (8 + b * 3) * scale;
      const br = el('line');
      setA(br, {
        x1: cx, y1: by,
        x2: cx + dir * bLen, y2: by - rand(5, 12),
        stroke: '#4a3a28', 'stroke-width': 1.5, 'stroke-linecap': 'round',
      });
      treeG.appendChild(br);
    }
    const ghost = el('text');
    setA(ghost, { x: cx, y: treeTop + treeH * 0.2, 'text-anchor': 'middle', fill: '#555', 'font-size': 10 });
    ghost.textContent = '\u2205';
    treeG.appendChild(ghost);
  }

  // ---- Labels (only for hero trees) ----
  if (isHero) {
    const nameLbl = el('text');
    nameLbl.setAttribute('class', 'garden-file-label');
    setA(nameLbl, {
      x: cx, y: groundY + 16,
      'text-anchor': 'middle', fill: '#999',
      'font-size': 9, 'font-family': '-apple-system, sans-serif',
    });
    nameLbl.textContent = truncate(baseName(file.path), 14);
    treeG.appendChild(nameLbl);

    const sizeLbl = el('text');
    sizeLbl.setAttribute('class', 'garden-file-size');
    setA(sizeLbl, {
      x: cx, y: groundY + 27,
      'text-anchor': 'middle', fill: '#555',
      'font-size': 8, 'font-family': '-apple-system, sans-serif',
    });
    sizeLbl.textContent = file.exists ? `${formatNumber(file.est_tokens)} · ${file.touch_count}×` : 'missing';
    treeG.appendChild(sizeLbl);
  }

  // Help-mode label.
  const helpLbl = el('text');
  helpLbl.setAttribute('class', 'garden-help-label');
  setA(helpLbl, { x: cx, y: treeTop - 12, 'text-anchor': 'middle' });
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

  if (isExpanded) {
    const canopyW = (isHero ? (28 + heightRatio * 35) : 20) * scale;
    drawFileInfoCard(treeG, cx, groundY, treeTop, canopyW, treeH, file);
  }

  parent.appendChild(treeG);
}

// ---- Undergrowth: small bushes for less-important files ----

function drawUndergrowth(parent, files, x0, width, groundY, maxTokens, maxTouches, accent) {
  const undergrowthG = el('g');
  undergrowthG.setAttribute('class', 'garden-undergrowth');

  // Show up to 6 bushes, hide the rest behind a "+N" label.
  const maxBushes = 6;
  const visible = files.slice(0, maxBushes);
  const hidden = files.length - visible.length;

  // Scatter bushes in a band just above the ground, slightly behind heroes.
  const bandTop = groundY - 35;
  const bandBottom = groundY - 8;

  visible.forEach((file, i) => {
    // Stagger horizontally across the clearing width.
    const bx = x0 + 10 + ((i / Math.max(visible.length, 1)) * (width - 20));
    // Slight vertical scatter.
    const by = bandBottom - (i % 2) * (bandBottom - bandTop) * 0.6 - rand(0, 8);
    const bushScale = 0.4 + Math.sqrt(file.est_tokens / Math.max(maxTokens, 1)) * 0.3;

    drawBush(undergrowthG, bx, by, file, bushScale, accent);
  });

  // "+N more" label if bushes were hidden.
  if (hidden > 0) {
    const moreLabel = el('text');
    moreLabel.setAttribute('class', 'garden-undergrowth-more');
    setA(moreLabel, {
      x: x0 + width - 6, y: groundY - 4,
      'text-anchor': 'end', fill: '#555',
      'font-size': 8, 'font-family': '-apple-system, sans-serif',
    });
    moreLabel.textContent = `+${hidden} more`;

    // Tooltip showing hidden file names.
    const hiddenNames = files.slice(maxBushes).map(f => baseName(f.path)).join(', ');
    moreLabel.addEventListener('mouseenter', () => {
      showTooltip(
        `${hidden} more file${hidden === 1 ? '' : 's'}`,
        hiddenNames,
        'Smaller files not shown as individual bushes.'
      );
    });
    moreLabel.addEventListener('mouseleave', hideTooltip);
    undergrowthG.appendChild(moreLabel);
  }

  parent.appendChild(undergrowthG);
}

function drawBush(parent, cx, cy, file, scale, accent) {
  const bushG = el('g');
  bushG.setAttribute('class', 'garden-tree garden-hover-target garden-bush');
  bushG.dataset.path = file.path;

  const isExpanded = expandedFilePath === file.path;
  if (expandedFilePath && !isExpanded) {
    bushG.setAttribute('opacity', '0.25');
  }

  const bushColor = file.backend_touches && file.backend_touches.length > 0
    ? backendColor(file.backend_touches[0][0])
    : accent;

  if (file.exists) {
    // Small trunk stub.
    const trunkH = 6 * scale;
    const trunkW = 2 * scale;
    const trunk = el('rect');
    setA(trunk, {
      x: cx - trunkW / 2, y: cy - trunkH,
      width: trunkW, height: trunkH,
      rx: 1, fill: '#5a3a20',
    });
    bushG.appendChild(trunk);

    // Bush: 2-3 overlapping small ellipses.
    const blobCount = 2 + Math.floor(scale * 2);
    const darkC = darken(bushColor, 0.25);
    for (let i = 0; i < blobCount; i++) {
      const bx = cx + (i - blobCount / 2) * 4 * scale;
      const by = cy - trunkH - 3 * scale;
      const blob = el('ellipse');
      blob.setAttribute('class', 'garden-bush-leaf');
      setA(blob, {
        cx: bx, cy: by,
        rx: (6 + i * 1.5) * scale,
        ry: (5 + i) * scale,
        fill: i % 2 === 0 ? bushColor : darkC,
        opacity: 0.7,
      });
      bushG.appendChild(blob);
    }
  } else {
    // Dead bush: tiny bare sticks.
    const stickH = 8 * scale;
    for (let s = -1; s <= 1; s++) {
      const stick = el('line');
      setA(stick, {
        x1: cx + s * 3 * scale, y1: cy,
        x2: cx + s * 4 * scale, y2: cy - stickH,
        stroke: '#4a3a28', 'stroke-width': 1, 'stroke-linecap': 'round',
      });
      bushG.appendChild(stick);
    }
  }

  // Tooltip on hover.
  bushG.addEventListener('mouseenter', () => {
    if (isExpanded) return;
    const topBk = file.backend_touches && file.backend_touches[0]
      ? file.backend_touches[0][0] : 'unknown';
    const existMsg = file.exists
      ? `${file.lines} lines · ~${formatNumber(file.est_tokens)} tokens`
      : 'File no longer exists on disk';
    showTooltip(
      esc(file.path),
      `${existMsg} · ${file.touch_count} touch${file.touch_count === 1 ? '' : 'es'}`,
      `Top backend: ${esc(topBk)}. Last touched: ${formatTs(file.last_touched)}. Click for details.`
    );
  });
  bushG.addEventListener('mouseleave', hideTooltip);
  bushG.addEventListener('click', (e) => {
    e.stopPropagation();
    expandedFilePath = expandedFilePath === file.path ? null : file.path;
    renderGardenScene();
  });

  if (isExpanded) {
    drawFileInfoCard(bushG, cx, cy, cy - 20, 15, 20, file);
  }

  parent.appendChild(bushG);
}

// ---- Info card (expanded tree/bush) ----
// Rendered as an HTML overlay so we can scroll long symbol lists.

function drawFileInfoCard(group, cx, groundY, treeTop, canopyW, treeH, file) {
  // The SVG info card is now minimal — just a connector line.
  // The real card is an HTML overlay rendered by showSymbolPanel().
  const g = el('g');
  g.setAttribute('class', 'garden-tree-info');

  // Thin connector line from tree to where the panel will appear.
  const lineEndX = cx + canopyW + 10;
  const line = el('line');
  setA(line, {
    x1: cx, y1: treeTop + 20,
    x2: lineEndX, y2: treeTop + 20,
    stroke: '#3a3d3e', 'stroke-width': 1, 'stroke-dasharray': '3,3',
  });
  g.appendChild(line);

  group.appendChild(g);

  // Show the HTML symbol panel.
  loadAndShowSymbolPanel(file);
}

function loadAndShowSymbolPanel(file) {
  const panel = document.getElementById('garden-symbol-panel');
  if (!panel) return;

  // Position the panel on the right side of the scene.
  panel.style.display = '';

  const cached = symbolsCache[file.path];
  if (cached && !cached.loading) {
    renderSymbolPanel(panel, file, cached.symbols);
    return;
  }

  // Show loading state.
  panel.innerHTML = `
    <div class="garden-symbol-header">${esc(truncate(file.path, 40))}</div>
    <div class="garden-symbol-loading">Loading symbols\u2026</div>
  `;

  if (cached && cached.loading) return; // already in flight

  symbolsCache[file.path] = { symbols: [], loading: true };

  invoke('get_file_symbols', { cwd: currentCwd, filePath: file.path })
    .then(data => {
      symbolsCache[file.path] = { symbols: data.symbols || [], loading: false };
      // Only render if this file is still expanded.
      if (expandedFilePath === file.path) {
        renderSymbolPanel(panel, file, data.symbols || []);
      }
    })
    .catch(() => {
      symbolsCache[file.path] = { symbols: [], loading: false };
      if (expandedFilePath === file.path) {
        renderSymbolPanel(panel, file, []);
      }
    });
}

function renderSymbolPanel(panel, file, symbols) {
  const imports = symbols.filter(s => s.kind === 'import');
  const defs = symbols.filter(s => s.kind !== 'import');

  // Group definitions by kind.
  const defGroups = new Map();
  for (const d of defs) {
    if (!defGroups.has(d.kind)) defGroups.set(d.kind, []);
    defGroups.get(d.kind).push(d);
  }

  const topBk = file.backend_touches && file.backend_touches[0]
    ? file.backend_touches[0][0] : '';

  let html = `
    <div class="garden-symbol-header">${esc(truncate(file.path, 40))}</div>
    <div class="garden-symbol-stats">
      <span>${file.exists ? `${file.lines} lines` : 'missing'}</span>
      <span>${formatNumber(file.est_tokens)} tokens</span>
      <span>${file.touch_count} touch${file.touch_count === 1 ? '' : 'es'}</span>
      ${topBk ? `<span class="garden-symbol-backend">${esc(topBk)}</span>` : ''}
    </div>
  `;

  if (file.backend_touches && file.backend_touches.length > 0) {
    const total = file.backend_touches.reduce((a, b) => a + b[1], 0);
    html += `<div class="garden-symbol-backend-bar">`;
    for (const [bk, ct] of file.backend_touches) {
      const pct = Math.round((ct / total) * 100);
      html += `<span class="garden-symbol-bar-seg" style="width:${pct}%;background:${backendColor(bk)}" title="${esc(bk)}: ${ct}"></span>`;
    }
    html += `</div>`;
  }

  if (imports.length > 0) {
    html += `<div class="garden-symbol-section">
      <div class="garden-symbol-section-title">Imports (${imports.length})</div>
      <div class="garden-symbol-list">`;
    for (const imp of imports.slice(0, 20)) {
      const src = imp.source || imp.name;
      const name = imp.name && imp.name !== src ? imp.name : '';
      html += `<div class="garden-symbol-item import">
        <span class="garden-symbol-kind">imp</span>
        <span class="garden-symbol-name">${esc(src)}${name ? ` <span class="garden-symbol-dim">${esc(name)}</span>` : ''}</span>
        <span class="garden-symbol-line">:${imp.line}</span>
      </div>`;
    }
    if (imports.length > 20) {
      html += `<div class="garden-symbol-more">+${imports.length - 20} more</div>`;
    }
    html += `</div></div>`;
  }

  for (const [kind, items] of defGroups) {
    const kindLabel = kind.charAt(0).toUpperCase() + kind.slice(1) + 's';
    html += `<div class="garden-symbol-section">
      <div class="garden-symbol-section-title">${esc(kindLabel)} (${items.length})</div>
      <div class="garden-symbol-list">`;
    for (const item of items.slice(0, 25)) {
      html += `<div class="garden-symbol-item def">
        <span class="garden-symbol-kind">${esc(kind.slice(0, 3))}</span>
        <span class="garden-symbol-name">${esc(item.name)}</span>
        <span class="garden-symbol-line">:${item.line}</span>
      </div>`;
    }
    if (items.length > 25) {
      html += `<div class="garden-symbol-more">+${items.length - 25} more</div>`;
    }
    html += `</div></div>`;
  }

  if (symbols.length === 0) {
    html += `<div class="garden-symbol-empty">No symbols extracted yet. Symbols are captured when the agent reads or edits this file.</div>`;
  }

  panel.innerHTML = html;
}

function hideSymbolPanel() {
  const panel = document.getElementById('garden-symbol-panel');
  if (panel) {
    panel.style.display = 'none';
    panel.innerHTML = '';
  }
}

// ---- Import edges ----
// Draws curved lines between trees that import each other.

function renderImportEdges() {
  const svg = document.getElementById('garden-svg');
  if (!svg || !importGraph || !importGraph.edges) return;

  // Remove old edges.
  svg.querySelectorAll('.garden-import-edge').forEach(e => e.remove());

  // Build a map of file_path → tree element center coordinates.
  const treeCenters = new Map();
  svg.querySelectorAll('.garden-tree').forEach(treeEl => {
    const path = treeEl.dataset.path;
    if (!path) return;
    const bbox = treeEl.getBBox();
    treeCenters.set(path, {
      x: bbox.x + bbox.width / 2,
      y: bbox.y + bbox.height * 0.3, // canopy area
    });
  });

  // Draw edges only between files that both have trees visible.
  const drawn = new Set();
  for (const edge of importGraph.edges) {
    if (!edge.to_file) continue;
    const from = treeCenters.get(edge.from_file);
    const to = treeCenters.get(edge.to_file);
    if (!from || !to) continue;

    const key = `${edge.from_file}->${edge.to_file}`;
    if (drawn.has(key)) continue;
    drawn.add(key);

    // Curved path between the two tree canopies.
    const midY = Math.min(from.y, to.y) - 30;
    const path = el('path');
    path.setAttribute('class', 'garden-import-edge');
    setA(path, {
      d: `M${from.x},${from.y} Q${(from.x + to.x) / 2},${midY} ${to.x},${to.y}`,
      fill: 'none',
      stroke: '#71D08330',
      'stroke-width': 1.5,
      'stroke-dasharray': '4,3',
    });

    // Insert edges behind trees (after ground, before tree groups).
    const firstGrove = svg.querySelector('.garden-grove');
    if (firstGrove) {
      svg.insertBefore(path, firstGrove);
    } else {
      svg.appendChild(path);
    }
  }
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

function subSegment(p) {
  // Second path segment — the sub-folder within a module. Files that sit
  // directly in the module root (only 2 segments: module/file.ext) are
  // bucketed into "(files)".
  const parts = p.split('/').filter(Boolean);
  if (parts.length <= 2) return '(files)';
  return parts[1];
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
