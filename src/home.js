// Home page — status cards + rotating fact strip

import { invoke, formatNumber } from './utils.js';

// ============================================================================
// Agent badge helpers
// ============================================================================

const GUARDIAN_AGENTS = [
  { name: 'Claude Code', checkCmd: 'check_claude_hooks_installed' },
  { name: 'Codex',       checkCmd: 'check_codex_hooks_installed' },
  { name: 'Cursor',      checkCmd: 'check_cursor_hooks_installed' },
];

const TOKEN_SAVER_AGENTS = [
  { name: 'Claude Code', checkCmd: 'check_claude_hooks_installed' },
];

function renderAgentBadges(container, agents) {
  container.innerHTML = agents.map(a => `
    <span class="agent-badge ${a.installed ? 'agent-badge--installed' : 'agent-badge--missing'}">
      <span class="agent-badge-status">${a.installed ? 'Added for' : 'Not added for'}</span>
      <span class="agent-badge-name">${a.name}</span>
    </span>
  `).join('');
}

// ============================================================================
// Status cards
// ============================================================================

async function loadGuardianCard() {
  const agentsEl = document.getElementById('guardian-agents');
  const stats = document.getElementById('guardian-card-stats');
  if (!agentsEl || !stats) return;

  try {
    const installed = await Promise.all(
      GUARDIAN_AGENTS.map(a => invoke(a.checkCmd).catch(() => false)),
    );

    const agents = GUARDIAN_AGENTS.map((a, i) => ({ ...a, installed: installed[i] }));
    renderAgentBadges(agentsEl, agents);

    stats.innerHTML = '';
  } catch (e) {
    console.error('Failed to load guardian card:', e);
    agentsEl.innerHTML = '';
    stats.innerHTML = '<div class="home-card-stat-line">Click to configure</div>';
  }
}

async function loadTokenSaverCard() {
  const agentsEl = document.getElementById('token-saver-agents');
  const stats = document.getElementById('token-saver-card-stats');
  if (!agentsEl || !stats) return;

  try {
    const [facts, ...installed] = await Promise.all([
      invoke('get_home_facts'),
      ...TOKEN_SAVER_AGENTS.map(a => invoke(a.checkCmd).catch(() => false)),
    ]);

    const agents = TOKEN_SAVER_AGENTS.map((a, i) => ({ ...a, installed: installed[i] }));
    renderAgentBadges(agentsEl, agents);

    const saved = facts.tokens_saved_today || 0;
    const savedLine = saved > 0
      ? `<div class="home-card-stat-line">${formatNumber(saved)} tokens saved today</div>`
      : '';

    if (agents.some(a => a.installed)) {
      stats.innerHTML = `
        ${savedLine || '<div class="home-card-stat-line">Hooks active</div>'}
      `;
    } else {
      stats.innerHTML = `
        ${savedLine || '<div class="home-card-stat-line">No hooks installed yet</div>'}
        <div class="home-card-stat-line">Click to set up</div>
      `;
    }
  } catch (e) {
    console.error('Failed to load token saver card:', e);
    agentsEl.innerHTML = '';
    stats.innerHTML = '<div class="home-card-stat-line">Click to configure</div>';
  }
}

// ============================================================================
// Rotating fact strip
// ============================================================================

const FACT_INTERVAL_MS = 6000;
const FADE_MS = 400;

let factTimer = null;
let factPool = [];
let factIndex = 0;

function relativeDays(isoTimestamp) {
  if (!isoTimestamp) return null;
  const then = new Date(isoTimestamp).getTime();
  if (Number.isNaN(then)) return null;
  const days = Math.floor((Date.now() - then) / (1000 * 60 * 60 * 24));
  return days;
}

function shortenModelName(model) {
  if (!model) return null;
  // claude-sonnet-4-5-20250930 → claude-sonnet-4-5
  const m = model.match(/^([a-z0-9-]+?)(?:-\d{8})?$/i);
  return m ? m[1] : model;
}

function buildFactPool(facts) {
  const pool = [];
  const num = (v) => `<span class="fact-num">${formatNumber(v)}</span>`;

  // 1. requests last hour
  if (facts.requests_last_hour > 0) {
    pool.push(`${num(facts.requests_last_hour)} request${facts.requests_last_hour === 1 ? '' : 's'} in the last hour`);
  }

  // 2. requests last day
  if (facts.requests_last_day > 0) {
    pool.push(`${num(facts.requests_last_day)} request${facts.requests_last_day === 1 ? '' : 's'} in the last 24 hours`);
  }

  // 3. days since last claude (or any backend)
  if (facts.last_request_by_backend && facts.last_request_by_backend.length > 0) {
    // Prefer claude if present, else first backend
    const claude = facts.last_request_by_backend.find(e => /claude/i.test(e.backend));
    const entry = claude || facts.last_request_by_backend[0];
    const days = relativeDays(entry.timestamp);
    const label = entry.backend.charAt(0).toUpperCase() + entry.backend.slice(1);
    if (days !== null) {
      if (days === 0) {
        pool.push(`Last ${label} request was today`);
      } else if (days === 1) {
        pool.push(`Last ${label} request was yesterday`);
      } else {
        pool.push(`Been <span class="fact-num">${days}</span> day${days === 1 ? '' : 's'} since you last used ${label}`);
      }
    }
  }

  // 4. top model this week
  if (facts.top_model_week) {
    const short = shortenModelName(facts.top_model_week);
    pool.push(`Your top model this week — <span class="fact-num">${short}</span>`);
  }

  // 5. detections this week
  if (facts.detections_week > 0) {
    pool.push(`Guardian protected ${num(facts.detections_week)} secret${facts.detections_week === 1 ? '' : 's'} this week`);
  }

  // 6. tokens saved today
  if (facts.tokens_saved_today > 0) {
    pool.push(`Saved ${num(facts.tokens_saved_today)} tokens with shell compression today`);
  }

  // 7. top tool this week
  if (facts.top_tool_week && facts.top_tool_week.tool_name) {
    pool.push(`Your most-called tool this week — <span class="fact-num">${facts.top_tool_week.tool_name}</span> (${num(facts.top_tool_week.count)} uses)`);
  }

  // 8. avg latency
  if (facts.avg_latency_ms_day > 0) {
    const ms = Math.round(facts.avg_latency_ms_day);
    const display = ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`;
    pool.push(`Average response latency — <span class="fact-num">${display}</span>`);
  }

  // 9. cache hit %
  if (facts.cache_hit_pct_day > 0) {
    const pct = Math.round(facts.cache_hit_pct_day);
    pool.push(`<span class="fact-num">${pct}%</span> of your requests hit cache — nice efficiency`);
  }

  // 10. total requests
  if (facts.total_requests > 0) {
    pool.push(`${num(facts.total_requests)} total requests since you started`);
  }

  return pool;
}

function renderFact() {
  const el = document.getElementById('home-fact');
  if (!el || factPool.length === 0) return;
  el.classList.add('fading');
  setTimeout(() => {
    el.innerHTML = factPool[factIndex];
    el.classList.remove('fading');
    renderDots();
  }, FADE_MS);
}

function renderDots() {
  const dots = document.getElementById('home-fact-dots');
  if (!dots) return;
  if (factPool.length <= 1) {
    dots.innerHTML = '';
    return;
  }
  // Only render dots once; subsequent calls just update active state
  if (dots.children.length !== factPool.length) {
    dots.innerHTML = '';
    factPool.forEach((_, i) => {
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'home-fact-dot';
      btn.addEventListener('click', () => {
        factIndex = i;
        renderFact();
        restartFactTimer();
      });
      dots.appendChild(btn);
    });
  }
  Array.from(dots.children).forEach((c, i) => {
    c.classList.toggle('active', i === factIndex);
  });
}

function advanceFact() {
  if (factPool.length === 0) return;
  factIndex = (factIndex + 1) % factPool.length;
  renderFact();
}

function startFactTimer() {
  stopFactTimer();
  if (factPool.length > 1) {
    factTimer = setInterval(advanceFact, FACT_INTERVAL_MS);
  }
}

function stopFactTimer() {
  if (factTimer) {
    clearInterval(factTimer);
    factTimer = null;
  }
}

function restartFactTimer() {
  startFactTimer();
}

async function loadFacts() {
  const el = document.getElementById('home-fact');
  if (!el) return;
  try {
    const facts = await invoke('get_home_facts');
    factPool = buildFactPool(facts);
    factIndex = 0;
    if (factPool.length === 0) {
      el.innerHTML = 'Make a request through one of your agents to see your stats here.';
      const dots = document.getElementById('home-fact-dots');
      if (dots) dots.innerHTML = '';
      return;
    }
    el.innerHTML = factPool[factIndex];
    renderDots();
    startFactTimer();
  } catch (e) {
    console.error('Failed to load home facts:', e);
    el.innerHTML = 'Make a request through one of your agents to see your stats here.';
  }
}

// ============================================================================
// Quilly refactor suggestions (files > 2000 lines)
// ============================================================================

function esc(s) {
  const d = document.createElement('div');
  d.textContent = s;
  return d.innerHTML;
}

function baseName(p) {
  return p.split('/').pop() || p;
}

const IGNORE_REFACTOR_PATTERNS = [
  /\.lock$/i,
  /lock\.json$/i,
  /package-lock\.json$/i,
  /yarn\.lock$/i,
  /pnpm-lock\.yaml$/i,
  /Cargo\.lock$/i,
  /Gemfile\.lock$/i,
  /composer\.lock$/i,
  /poetry\.lock$/i,
  /Pipfile\.lock$/i,
  /\.sum$/i,
  /\.min\.js$/i,
  /\.min\.css$/i,
  /\.bundle\.js$/i,
  /\.map$/i,
  /\.generated\./i,
  /\.pb\.go$/i,
  /\.g\.dart$/i,
  /\/dist\//i,
  /\/build\//i,
  /\/node_modules\//i,
  /\/vendor\//i,
];

function isGeneratedOrLockFile(path) {
  return IGNORE_REFACTOR_PATTERNS.some(re => re.test(path));
}

async function loadHomeRefactorSuggestions() {
  const icon = document.getElementById('home-quilly-icon');
  const badge = document.getElementById('home-quilly-badge');
  const bubble = document.getElementById('home-quilly-bubble');
  const area = document.getElementById('home-quilly-area');
  if (!icon || !badge || !bubble || !area) return;

  try {
    const stats = await invoke('get_garden_stats', { timeRange: 'all' });
    const projects = stats.projects || [];
    if (projects.length === 0) {
      area.style.display = 'none';
      return;
    }

    // Fetch detail for all projects and collect big files.
    const allBigFiles = [];
    for (const p of projects) {
      try {
        const detail = await invoke('get_garden_detail', { cwd: p.cwd, timeRange: 'all' });
        for (const f of (detail.files || [])) {
          if (f.exists && f.lines > 2000 && !isGeneratedOrLockFile(f.path)) {
            const fullPath = f.path.startsWith('/') ? f.path : `${p.cwd}/${f.path}`;
            allBigFiles.push({ ...f, project: p.display_name, fullPath });
          }
        }
      } catch (_) { /* skip project */ }
    }

    // Deduplicate by full path (same file may appear across projects).
    const seen = new Set();
    const uniqueBigFiles = allBigFiles.filter(f => {
      if (seen.has(f.fullPath)) return false;
      seen.add(f.fullPath);
      return true;
    });

    if (uniqueBigFiles.length === 0) {
      area.style.display = 'none';
      return;
    }

    area.style.display = '';
    badge.style.display = '';
    badge.textContent = uniqueBigFiles.length;

    const sorted = uniqueBigFiles.sort((a, b) => b.lines - a.lines).slice(0, 5);
    const fileList = sorted.map(f => {
      const dir = f.path.includes('/') ? f.path.slice(0, f.path.lastIndexOf('/')) : '';
      return `<div class="quilly-refactor-file quilly-refactor-file--clickable" data-filepath="${esc(f.fullPath)}" title="${esc(f.fullPath)}">
        <div class="quilly-refactor-file-info">
          <span class="quilly-refactor-file-name">${esc(baseName(f.path))}</span>
          <span class="quilly-refactor-file-dir">${esc(f.project)}${dir ? ' / ' + esc(dir) : ''}</span>
        </div>
        <span class="quilly-refactor-file-lines">${formatNumber(f.lines)} lines</span>
      </div>`;
    }).join('');
    const extra = uniqueBigFiles.length > 5 ? `<div class="quilly-refactor-more">+${uniqueBigFiles.length - 5} more</div>` : '';

    bubble.innerHTML = `
      <div class="quilly-refactor-header">
        <span>Consider refactoring</span>
        <button class="quilly-refactor-dismiss" id="home-quilly-dismiss" type="button" title="Dismiss">&times;</button>
      </div>
      <div class="quilly-refactor-body">
        <div class="quilly-refactor-hint">${uniqueBigFiles.length} file${uniqueBigFiles.length === 1 ? '' : 's'} over 2,000 lines:</div>
        ${fileList}
        ${extra}
      </div>
    `;

    // Toggle bubble on icon click.
    icon.onclick = () => {
      bubble.style.display = bubble.style.display === 'none' ? '' : 'none';
    };

    // Dismiss button.
    document.getElementById('home-quilly-dismiss')?.addEventListener('click', (e) => {
      e.stopPropagation();
      bubble.style.display = 'none';
    });

    // Reveal file in Finder/Explorer on click.
    bubble.querySelectorAll('.quilly-refactor-file--clickable').forEach(row => {
      row.addEventListener('click', () => {
        const fp = row.dataset.filepath;
        if (fp && window.__TAURI__?.opener?.revealItemInDir) {
          window.__TAURI__.opener.revealItemInDir(fp);
        }
      });
    });
  } catch (e) {
    console.error('Failed to load refactor suggestions:', e);
    area.style.display = 'none';
  }
}

// ============================================================================
// Public API
// ============================================================================

export function initHome() {
  // Click-to-advance on the fact text itself
  const factEl = document.getElementById('home-fact');
  if (factEl) {
    factEl.addEventListener('click', () => {
      advanceFact();
      restartFactTimer();
    });
  }
}

export function loadHome() {
  loadGuardianCard();
  loadTokenSaverCard();
  loadFacts();
  loadHomeRefactorSuggestions();
}

export function suspendHome() {
  stopFactTimer();
}

export function resumeHome() {
  if (factPool.length > 1) startFactTimer();
}
