// Garden — each project is its own garden.
//
// View 1: Project picker — cards showing all projects with mini previews.
// View 2: Garden scene — rich SVG scene for one project:
//   - Trees     = sessions (height = output tokens)
//   - Pond      = input tokens (size = total input)
//   - Sun       = cache read (brightness = cache hit ratio)
//   - Mushrooms = cache creation (count scales with new cache)
//   - Rain      = live activity (drops fall when requests are active)

import { invoke, formatNumber } from './utils.js';

const SVG_NS = 'http://www.w3.org/2000/svg';

// ---- State ----
let gardenTimeRange = '1d';
let currentCwd = null;     // null = picker view, string = garden view
let projectList = [];
let gardenDetail = null;   // GardenDetail for current project
let liveTimer = null;
let rainTimers = new Map();

// ---- Public API ----

export function initGarden() {
  document.getElementById('garden-time-select')?.addEventListener('change', (e) => {
    gardenTimeRange = e.target.value;
    loadGarden();
  });

  document.getElementById('garden-back-btn')?.addEventListener('click', () => {
    currentCwd = null;
    showPicker();
    loadGarden();
  });
}

export function loadGarden() {
  if (currentCwd) {
    loadGardenDetail(currentCwd);
  } else {
    loadProjectList();
  }
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

// ---- Views ----

function showPicker() {
  document.getElementById('garden-picker').style.display = '';
  document.getElementById('garden-scene').style.display = 'none';
  stopGardenPolling();
}

function showScene() {
  document.getElementById('garden-picker').style.display = 'none';
  document.getElementById('garden-scene').style.display = '';
}

// ---- Project List ----

function loadProjectList() {
  invoke('get_garden_stats', { timeRange: gardenTimeRange })
    .then(data => {
      projectList = data.projects || [];
      renderPickerGrid();
    })
    .catch(e => console.error('[garden]', e));
}

function renderPickerGrid() {
  const grid = document.getElementById('garden-picker-grid');
  const empty = document.getElementById('garden-picker-empty');
  if (!grid) return;

  if (projectList.length === 0) {
    grid.innerHTML = '';
    if (empty) { empty.style.display = ''; grid.appendChild(empty); }
    return;
  }
  if (empty) empty.style.display = 'none';

  grid.innerHTML = projectList.map(p => `
    <div class="garden-project-card" data-cwd="${esc(p.cwd)}">
      <span class="garden-project-card-accent"></span>
      <div class="garden-project-card-name">${esc(p.display_name)}</div>
      <div class="garden-project-card-path">${esc(p.cwd)}</div>
      <div class="garden-project-card-stats">
        <div class="garden-project-card-stat">
          <span class="garden-project-card-stat-label">Output</span>
          <span class="garden-project-card-stat-value">${formatNumber(p.output_tokens)}</span>
        </div>
        <div class="garden-project-card-stat">
          <span class="garden-project-card-stat-label">Input</span>
          <span class="garden-project-card-stat-value">${formatNumber(p.input_tokens)}</span>
        </div>
        <div class="garden-project-card-stat">
          <span class="garden-project-card-stat-label">Requests</span>
          <span class="garden-project-card-stat-value">${formatNumber(p.request_count)}</span>
        </div>
      </div>
      <div class="garden-project-card-backends">
        ${p.backends.map(b => `<span class="garden-backend-pill">${esc(b)}</span>`).join('')}
      </div>
      ${miniTreeSvg(p)}
    </div>
  `).join('');

  grid.querySelectorAll('.garden-project-card').forEach(card => {
    card.addEventListener('click', () => {
      currentCwd = card.dataset.cwd;
      showScene();
      loadGardenDetail(currentCwd);
      startGardenPolling();
    });
  });
}

/** Tiny inline SVG tree preview for the picker card. */
function miniTreeSvg(project) {
  const maxT = Math.max(project.output_tokens, 1);
  const h = 20 + Math.min(maxT / 50000, 1) * 30;
  return `<svg class="garden-card-preview" width="40" height="60" viewBox="0 0 40 60">
    <rect x="18" y="${60 - h}" width="4" height="${h}" rx="2" fill="#5a3a20"/>
    <circle cx="20" cy="${60 - h - 8}" r="14" fill="#3a7a2a" opacity="0.7"/>
    <circle cx="14" cy="${60 - h - 4}" r="10" fill="#2d5a1e" opacity="0.6"/>
    <circle cx="26" cy="${60 - h - 4}" r="10" fill="#2d5a1e" opacity="0.6"/>
  </svg>`;
}

// ---- Garden Detail ----

function loadGardenDetail(cwd) {
  invoke('get_garden_detail', { cwd, timeRange: gardenTimeRange })
    .then(data => {
      gardenDetail = data;
      document.getElementById('garden-scene-title').textContent = data.display_name;
      document.getElementById('garden-scene-path').textContent = data.cwd;
      renderGardenScene();
      renderStatsRow();
      // Re-init lucide icons for the back button
      if (window.lucide) lucide.createIcons();
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

// ============ SVG Garden Scene ============

function renderGardenScene() {
  const svg = document.getElementById('garden-svg');
  if (!svg || !gardenDetail) return;
  svg.innerHTML = '';

  const W = 1200, H = 700;
  const GROUND_Y = 520;
  const WATER_Y = GROUND_Y + 8;

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
  const sessions = gardenDetail.sessions;
  const maxOut = Math.max(...sessions.map(s => s.output_tokens), 1);

  // Layout: spread trees across left 75% of width (pond is on right)
  const treeZoneW = W * 0.7;
  const treeZoneStart = 60;
  const count = sessions.length;
  const spacing = count > 1 ? treeZoneW / count : treeZoneW / 2;

  sessions.forEach((sess, i) => {
    const cx = treeZoneStart + spacing * (i + 0.5);
    const g = el('g');
    g.setAttribute('class', 'garden-tree');
    g.dataset.session = sess.session_id;
    drawTree(g, cx, GROUND_Y, sess, maxOut);
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
  const ratio = total > 0 ? detail.cache_read / total : 0; // 0..1
  const sunR = 20 + ratio * 30;  // 20..50
  const opacity = 0.3 + ratio * 0.7;
  const cx = 100, cy = 100;

  // Glow
  const glow = el('circle');
  glow.setAttribute('class', 'garden-sun-glow');
  setA(glow, { cx, cy, r: sunR * 3, fill: 'url(#gSunGlow)', opacity });
  svg.appendChild(glow);

  // Rays
  if (ratio > 0.05) {
    const raysG = el('g');
    raysG.setAttribute('class', 'garden-sun-rays');
    // Store the center so CSS rotation works
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
    svg.appendChild(raysG);
  }

  // Sun disc
  const disc = el('circle');
  setA(disc, { cx, cy, r: sunR, fill: '#f5c542', opacity, filter: 'url(#fGlow)' });
  svg.appendChild(disc);

  // Label
  const lbl = el('text');
  lbl.setAttribute('class', 'garden-label-dim');
  setA(lbl, { x: cx, y: cy + sunR + 16 });
  lbl.textContent = `${Math.round(ratio * 100)}% cache`;
  svg.appendChild(lbl);
}

// ---- Pond: input tokens ----

function drawPond(svg, detail, W, groundY) {
  const inputRatio = Math.min(detail.total_input / 500000, 1);
  const pondW = 80 + inputRatio * 160;  // 80..240
  const pondH = 20 + inputRatio * 30;   // 20..50
  const cx = W - pondW / 2 - 80;
  const cy = groundY + 25;

  // Pond body
  const pond = el('ellipse');
  setA(pond, { cx, cy, rx: pondW / 2, ry: pondH / 2, fill: 'url(#gPond)', opacity: 0.8 });
  svg.appendChild(pond);

  // Shimmer highlight
  const shimmer = el('ellipse');
  shimmer.setAttribute('class', 'garden-pond-shimmer');
  setA(shimmer, { cx: cx - pondW * 0.15, cy: cy - pondH * 0.15, rx: pondW * 0.25, ry: pondH * 0.2, fill: '#4a90d9', opacity: 0.3 });
  svg.appendChild(shimmer);

  // Ripples
  for (let i = 0; i < 2; i++) {
    const ripple = el('circle');
    ripple.setAttribute('class', 'garden-pond-ripple');
    setA(ripple, { cx: cx + rand(-pondW * 0.2, pondW * 0.2), cy, r: 4, fill: 'none', stroke: '#4a90d9', 'stroke-width': 0.8 });
    ripple.style.animationDelay = `${i * 1}s`;
    svg.appendChild(ripple);
  }

  // Label
  const lbl = el('text');
  lbl.setAttribute('class', 'garden-label-dim');
  setA(lbl, { x: cx, y: cy + pondH / 2 + 14 });
  lbl.textContent = `${formatNumber(detail.total_input)} input`;
  svg.appendChild(lbl);
}

// ---- Tree per session ----

function drawTree(group, cx, groundY, session, maxOutput) {
  const ratio = Math.max(session.output_tokens / maxOutput, 0.12);
  const treeH = 60 + ratio * 220;
  const trunkW = 5 + ratio * 14;
  const canopyR = 22 + ratio * 55;
  const trunkTop = groundY - treeH;

  // Backend color accent
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

    // Small leaf cluster at branch end
    if (ratio > 0.3) {
      const lf = el('circle');
      lf.setAttribute('class', 'garden-leaf');
      setA(lf, { cx: cx + dir * bLen, cy: by - rand(10, 20), r: rand(8, 14), fill: accent, opacity: 0.6 });
      lf.style.animationDelay = `${-b * 0.5}s`;
      group.appendChild(lf);
    }
  }

  // Canopy — organic blobs
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

  // Central bright blob
  const cc = el('circle');
  setA(cc, { cx, cy: trunkTop + 3, r: canopyR * 0.45, fill: accent, opacity: 0.85, filter: 'url(#fGlow)' });
  canopyG.appendChild(cc);

  group.appendChild(canopyG);

  // Session label
  const lbl = el('text');
  lbl.setAttribute('class', 'garden-label');
  setA(lbl, { x: cx, y: groundY + 18 });
  // Short session id
  const shortId = session.session_id.length > 12
    ? session.session_id.slice(0, 8) + '...'
    : session.session_id;
  lbl.textContent = shortId;
  group.appendChild(lbl);

  // Output tokens label
  const sub = el('text');
  sub.setAttribute('class', 'garden-label-dim');
  setA(sub, { x: cx, y: groundY + 30 });
  sub.textContent = formatNumber(session.output_tokens);
  group.appendChild(sub);
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
  // Scatter across ground area
  for (let i = 0; i < count; i++) {
    const mx = rand(40, W - 40);
    const my = groundY + rand(8, 60);
    const g = el('g');
    g.setAttribute('class', 'garden-mushroom');
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
    })
    .catch(() => {});
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

      // Splash
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

function darken(hex, amount) {
  // Simple darken for rgb(...) or hex strings
  if (hex.startsWith('#')) {
    const r = Math.max(0, parseInt(hex.slice(1, 3), 16) - Math.floor(amount * 255));
    const g = Math.max(0, parseInt(hex.slice(3, 5), 16) - Math.floor(amount * 255));
    const b = Math.max(0, parseInt(hex.slice(5, 7), 16) - Math.floor(amount * 255));
    return `rgb(${r},${g},${b})`;
  }
  return hex;
}
