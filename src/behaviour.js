// Agent Behaviour view — high-level monitoring with day-wise metric trends

import { invoke, formatNumber } from './utils.js';

let behTimeRange = '7d';
let behBackend = 'all';

// ============ Init ============

export function initBehaviour() {
  const timeSelect = document.getElementById('beh-time-select');
  if (timeSelect) {
    timeSelect.addEventListener('change', () => {
      behTimeRange = timeSelect.value;
      loadBehaviour();
    });
  }

  const backendSelect = document.getElementById('beh-backend-select');
  if (backendSelect) {
    backendSelect.addEventListener('change', () => {
      behBackend = backendSelect.value;
      loadBehaviour();
    });
  }

  const refreshBtn = document.getElementById('beh-refresh-btn');
  if (refreshBtn) {
    refreshBtn.addEventListener('click', () => loadBehaviour());
  }
}

// ============ Load ============

export async function loadBehaviour() {
  const content = document.getElementById('behaviour-content');
  if (!content) return;
  content.innerHTML = '<p class="loading">Loading...</p>';

  try {
    const [data, backends] = await Promise.all([
      invoke('get_agent_behaviour', { timeRange: behTimeRange, backend: behBackend }),
      invoke('get_backends'),
    ]);

    const sel = document.getElementById('beh-backend-select');
    if (sel && sel.options.length <= 1) {
      backends.forEach((backend) => {
        const opt = document.createElement('option');
        opt.value = backend;
        opt.textContent = backend.replace('-hooks', '');
        sel.appendChild(opt);
      });
    }

    const dashboard = buildBehaviourDashboard(data.sessions);
    content.innerHTML = renderBehaviour(data.sessions, dashboard);
    attachBehaviourHandlers();

    if (window.lucide) window.lucide.createIcons();
  } catch (err) {
    content.innerHTML = `<p class="error">Failed to load: ${err}</p>`;
  }
}

// ============ Render ============

function renderBehaviour(sessions, dashboard) {
  if (sessions.length === 0) {
    return `
      <div class="beh-shell">
        ${renderHero(dashboard)}
        <div class="card beh-empty-card">
          <div class="beh-empty-title">No sessions yet</div>
          <div class="beh-empty-copy">
            This page starts filling in once hooked agents begin reading files, editing code, and using tools.
          </div>
        </div>
      </div>
    `;
  }

  return `
    <div class="beh-shell">
      ${renderHero(dashboard)}
      <div class="beh-trend-grid">
        ${dashboard.metricCards.map(renderMetricCard).join('')}
      </div>
      <div class="beh-secondary-grid">
        <div class="card beh-side-card">
          <div class="card-header">Current Operating Mix</div>
          <div class="card-body beh-side-body">
            <div class="beh-mix-list">
              ${dashboard.mixRows.map(renderMixRow).join('')}
            </div>
          </div>
        </div>
        <div class="card beh-side-card">
          <div class="card-header">Current Profile</div>
          <div class="card-body beh-side-body">
            <div class="beh-profile-list">
              ${dashboard.profileRows.map(renderProfileRow).join('')}
            </div>
          </div>
        </div>
        <div class="card beh-side-card">
          <div class="card-header">Coverage</div>
          <div class="card-body beh-side-body">
            <div class="beh-coverage-grid">
              ${dashboard.coverageStats.map(renderCoverageStat).join('')}
            </div>
            <div class="beh-coverage-note">
              Day-wise graphs only include days with recorded sessions in the selected range.
            </div>
          </div>
        </div>
      </div>
      <div class="beh-sessions-header">
        <div>
          <h2 class="beh-section-title">Recent Sessions</h2>
          <p class="beh-section-subtitle">
            The top layer shows trend shape; expand a session only when you want the underlying turn detail.
          </p>
        </div>
        <span class="beh-session-count">${sessions.length} session${sessions.length !== 1 ? 's' : ''}</span>
      </div>
      <div class="beh-sessions">
        ${sessions.map((session, index) => renderSession(session, index)).join('')}
      </div>
    </div>
  `;
}

function renderHero(dashboard) {
  const backendLabel = behBackend === 'all' ? 'All backends' : behBackend.replace('-hooks', '');

  return `
    <div class="card beh-hero-card">
      <div class="beh-hero-copy">
        <div class="beh-hero-kicker">Behaviour Monitor</div>
        <h2 class="beh-hero-title">Track stable agent habits over time.</h2>
        <p class="beh-hero-text">
          This page stays intentionally high-level: read-first behaviour, exploration balance, shell reliance, and tool tempo, shown as day-wise trends instead of inferred before/after comparisons.
        </p>
      </div>
      <div class="beh-hero-meta">
        <span class="beh-hero-pill beh-hero-pill--accent">${dashboard.overall.sessionCount} session${dashboard.overall.sessionCount !== 1 ? 's' : ''}</span>
        <span class="beh-hero-pill">${dashboard.overall.turnCount} turns</span>
        <span class="beh-hero-pill">${dashboard.dayCount} active day${dashboard.dayCount !== 1 ? 's' : ''}</span>
        <span class="beh-hero-pill">${dashboard.overall.projectCount} project${dashboard.overall.projectCount !== 1 ? 's' : ''}</span>
        <span class="beh-hero-pill">${backendLabel}</span>
        <span class="beh-hero-pill beh-hero-pill--soft">${timeRangeLabel(behTimeRange)}</span>
      </div>
    </div>
  `;
}

function renderMetricCard(card) {
  return `
    <div class="beh-trend-card beh-trend--${card.key}">
      <div class="beh-trend-top">
        <span class="beh-trend-label">${card.label}</span>
        <span class="beh-trend-tag">${card.tag}</span>
      </div>
      <div class="beh-trend-value">${card.value}</div>
      <div class="beh-trend-copy">${card.copy}</div>
      <div class="beh-trend-spark">
        ${renderSparkline(card.sparkline, card.key, card.scale)}
      </div>
      <div class="beh-trend-footer">
        <span>${card.footerLeft}</span>
        <span>${card.footerRight}</span>
      </div>
    </div>
  `;
}

function renderMixRow(row) {
  return `
    <div class="beh-mix-row">
      <div class="beh-mix-meta">
        <span class="beh-mix-label">${row.label}</span>
        <span class="beh-mix-value">${row.percentLabel} • ${row.countLabel}</span>
      </div>
      <div class="beh-mix-track">
        <span class="beh-mix-fill beh-mix-fill--${row.key}" style="width:${row.percent.toFixed(1)}%"></span>
      </div>
    </div>
  `;
}

function renderProfileRow(row) {
  return `
    <div class="beh-profile-row">
      <div class="beh-profile-label">${row.label}</div>
      <div>
        <div class="beh-profile-value">${row.value}</div>
        <div class="beh-profile-detail">${row.detail}</div>
      </div>
    </div>
  `;
}

function renderCoverageStat(stat) {
  return `
    <div class="beh-coverage-stat">
      <div class="beh-coverage-label">${stat.label}</div>
      <div class="beh-coverage-value">${stat.value}</div>
      <div class="beh-coverage-detail">${stat.detail}</div>
    </div>
  `;
}

function renderSession(session, index) {
  const shortId = session.session_id.length > 12 ? session.session_id.slice(0, 12) : session.session_id;
  const backendLabel = session.backend.replace('-hooks', '');
  const durationStr = formatDuration(session.duration_ms);
  const timeStr = formatTime(session.first_seen);
  const metrics = deriveSessionMetrics(session);
  const flags = buildSessionFlags(session, metrics);
  const filesTouched = session.unique_files_read + session.unique_files_written;

  return `
    <div class="beh-session" data-beh-index="${index}">
      <div class="beh-session-header">
        <div class="beh-session-left">
          <span class="beh-session-backend beh-backend--${backendLabel}">${backendLabel}</span>
          <span class="beh-session-project">${escHtml(session.display_name || 'unknown')}</span>
          <span class="beh-session-id" title="${escHtml(session.session_id)}">${escHtml(shortId)}</span>
        </div>
        <div class="beh-session-meta">
          <span class="beh-session-time">${timeStr}</span>
          <span class="beh-pill">${session.turn_count} turn${session.turn_count !== 1 ? 's' : ''}</span>
          <span class="beh-pill">${durationStr}</span>
          <span class="beh-pill">${formatNumber(session.total_input_tokens + session.total_output_tokens)} tok</span>
          ${session.dlp_blocks > 0 ? `<span class="beh-pill beh-pill--danger">${session.dlp_blocks} blocked</span>` : ''}
        </div>
        <button class="beh-expand-btn" type="button" aria-label="Expand session">
          <i data-lucide="chevron-down"></i>
        </button>
      </div>
      ${flags.length ? `<div class="beh-session-flags">${flags.map(renderSessionFlag).join('')}</div>` : ''}
      <div class="beh-session-metrics">
        <div class="beh-metric">
          <span class="beh-metric-label">Files touched</span>
          <span class="beh-metric-val">${filesTouched}</span>
        </div>
        <div class="beh-metric">
          <span class="beh-metric-label">Read-first</span>
          <span class="beh-metric-val beh-rbw--${readFirstClass(session.read_before_write_pct)}">${session.read_before_write_pct.toFixed(0)}%</span>
        </div>
        <div class="beh-metric">
          <span class="beh-metric-label">Explore / modify</span>
          <span class="beh-metric-val">${session.exploration_ratio.toFixed(1)}x</span>
        </div>
        <div class="beh-metric">
          <span class="beh-metric-label">Bash share</span>
          <span class="beh-metric-val">${metrics.bashSharePct.toFixed(0)}%</span>
        </div>
        <div class="beh-metric">
          <span class="beh-metric-label">Tools / turn</span>
          <span class="beh-metric-val">${metrics.toolsPerTurn.toFixed(1)}</span>
        </div>
      </div>
      <div class="beh-session-turns" id="beh-turns-${index}" style="display:none;">
        ${session.turns.map((turn, turnIndex) => renderTurn(turn, turnIndex)).join('')}
      </div>
    </div>
  `;
}

function renderSessionFlag(flag) {
  return `<span class="beh-flag beh-flag--${flag.tone}">${flag.label}</span>`;
}

function renderTurn(turn, turnIndex) {
  const promptPreview = turn.prompt
    ? escHtml(turn.prompt.length > 120 ? `${turn.prompt.slice(0, 120)}...` : turn.prompt)
    : '<span class="beh-no-prompt">No prompt text</span>';

  const toolTags = turn.tool_counts
    .map((tc) => {
      const cls = toolClass(tc.tool_name);
      return `<span class="beh-tool-tag ${cls}">${escHtml(tc.tool_name)} <b>${tc.count}</b></span>`;
    })
    .join('');

  const fileReadItems = turn.files_read
    .slice(0, 8)
    .map((file) => `<span class="beh-file-chip beh-file--read" title="${escHtml(file)}">${shortPath(file)}</span>`)
    .join('');

  const fileWriteItems = turn.files_written
    .slice(0, 8)
    .map((file) => `<span class="beh-file-chip beh-file--write" title="${escHtml(file)}">${shortPath(file)}</span>`)
    .join('');

  const bashItems = turn.bash_commands
    .slice(0, 3)
    .map((cmd) => `<code class="beh-bash-cmd">${escHtml(cmd.length > 80 ? `${cmd.slice(0, 80)}...` : cmd)}</code>`)
    .join('');

  return `
    <div class="beh-turn">
      <div class="beh-turn-header">
        <span class="beh-turn-num">#${turnIndex + 1}</span>
        <span class="beh-turn-prompt">${promptPreview}</span>
        <span class="beh-turn-tokens">${formatNumber(turn.input_tokens)} in / ${formatNumber(turn.output_tokens)} out</span>
        ${turn.dlp_action >= 2 ? '<span class="beh-pill beh-pill--danger">blocked</span>' : ''}
      </div>
      ${toolTags ? `<div class="beh-turn-tools">${toolTags}</div>` : ''}
      ${fileReadItems || fileWriteItems ? `
        <div class="beh-turn-files">
          ${fileReadItems ? `<div class="beh-file-group"><span class="beh-file-label">Read</span>${fileReadItems}${turn.files_read.length > 8 ? `<span class="beh-file-more">+${turn.files_read.length - 8}</span>` : ''}</div>` : ''}
          ${fileWriteItems ? `<div class="beh-file-group"><span class="beh-file-label">Write</span>${fileWriteItems}${turn.files_written.length > 8 ? `<span class="beh-file-more">+${turn.files_written.length - 8}</span>` : ''}</div>` : ''}
        </div>
      ` : ''}
      ${bashItems ? `<div class="beh-turn-bash">${bashItems}</div>` : ''}
    </div>
  `;
}

function renderSparkline(points, key, scale) {
  const values = points.map((point) => point.value);
  const numericValues = values.filter((value) => Number.isFinite(value));
  if (!numericValues.length) {
    return '<div class="beh-spark-empty">No daily data</div>';
  }

  const width = 220;
  const height = 72;
  const padX = 8;
  const padY = 10;
  const baselineY = height - padY;
  const count = points.length;
  const xForIndex = (index) => (
    count === 1
      ? width / 2
      : padX + ((width - padX * 2) * index) / (count - 1)
  );

  let minValue = 0;
  let maxValue = 0;

  if (scale === 'percent') {
    minValue = 0;
    maxValue = 100;
  } else {
    minValue = Math.min(...numericValues);
    maxValue = Math.max(...numericValues);
    if (minValue > 0) minValue = 0;
    if (maxValue === minValue) maxValue = minValue + 1;
  }

  const yForValue = (value) => {
    const range = maxValue - minValue || 1;
    return baselineY - ((value - minValue) / range) * (height - padY * 2);
  };

  const definedPoints = points
    .map((point, index) => ({ ...point, index, x: xForIndex(index) }))
    .filter((point) => Number.isFinite(point.value))
    .map((point) => ({ ...point, y: yForValue(point.value) }));

  const linePath = buildLinePath(definedPoints);
  const areaPath = buildAreaPath(definedPoints, baselineY);
  const lastPoint = definedPoints[definedPoints.length - 1];
  const labels = buildSparkLabels(points);

  return `
    <svg class="beh-spark-svg" viewBox="0 0 ${width} ${height}" preserveAspectRatio="none" aria-hidden="true">
      <line x1="${padX}" y1="${baselineY}" x2="${width - padX}" y2="${baselineY}" class="beh-spark-axis"></line>
      ${areaPath ? `<path d="${areaPath}" class="beh-spark-area beh-spark-area--${key}"></path>` : ''}
      ${linePath ? `<path d="${linePath}" class="beh-spark-line beh-spark-line--${key}"></path>` : ''}
      ${lastPoint ? `<circle cx="${lastPoint.x}" cy="${lastPoint.y}" r="3.5" class="beh-spark-dot beh-spark-dot--${key}"></circle>` : ''}
    </svg>
    <div class="beh-spark-labels">
      <span>${labels.start}</span>
      <span>${labels.end}</span>
    </div>
  `;
}

function buildLinePath(points) {
  if (!points.length) return '';
  if (points.length === 1) {
    return `M ${points[0].x - 24} ${points[0].y} L ${points[0].x + 24} ${points[0].y}`;
  }
  return points.map((point, index) => `${index === 0 ? 'M' : 'L'} ${point.x} ${point.y}`).join(' ');
}

function buildAreaPath(points, baselineY) {
  if (!points.length) return '';
  if (points.length === 1) {
    const { x, y } = points[0];
    return `M ${x - 24} ${baselineY} L ${x - 24} ${y} L ${x + 24} ${y} L ${x + 24} ${baselineY} Z`;
  }
  const head = `M ${points[0].x} ${baselineY} L ${points[0].x} ${points[0].y}`;
  const body = points.slice(1).map((point) => `L ${point.x} ${point.y}`).join(' ');
  const tail = `L ${points[points.length - 1].x} ${baselineY} Z`;
  return `${head} ${body} ${tail}`;
}

function buildSparkLabels(points) {
  const withValues = points.filter((point) => Number.isFinite(point.value));
  if (!withValues.length) return { start: 'No data', end: '' };
  return {
    start: withValues[0].label,
    end: withValues.length > 1 ? withValues[withValues.length - 1].label : withValues[0].label,
  };
}

// ============ Event Handlers ============

function attachBehaviourHandlers() {
  document.querySelectorAll('.beh-session').forEach((el) => {
    const header = el.querySelector('.beh-session-header');
    const expandBtn = el.querySelector('.beh-expand-btn');
    const index = Number(el.dataset.behIndex);
    const turnsDiv = document.getElementById(`beh-turns-${index}`);

    if (!header || !turnsDiv) return;

    header.addEventListener('click', () => {
      const isOpen = turnsDiv.style.display !== 'none';
      turnsDiv.style.display = isOpen ? 'none' : 'block';
      el.classList.toggle('beh-session--open', !isOpen);
      if (expandBtn) {
        expandBtn.innerHTML = isOpen
          ? '<i data-lucide="chevron-down"></i>'
          : '<i data-lucide="chevron-up"></i>';
        if (window.lucide) window.lucide.createIcons();
      }
    });
  });
}

// ============ Data Model ============

function buildBehaviourDashboard(sessions) {
  const ordered = [...sessions].sort((a, b) => a.first_seen.localeCompare(b.first_seen));
  const overall = aggregateSessions(ordered);
  const dayBuckets = buildDayBuckets(ordered);

  return {
    overall,
    dayCount: dayBuckets.length,
    metricCards: buildMetricCards(overall, dayBuckets),
    mixRows: buildMixRows(overall.toolMix, overall.totalToolCalls),
    profileRows: buildProfileRows(overall),
    coverageStats: buildCoverageStats(overall, dayBuckets),
  };
}

function aggregateSessions(sessions) {
  const projectKeys = new Set();
  const toolMix = { explore: 0, modify: 0, bash: 0, other: 0 };

  let turnCount = 0;
  let totalToolCalls = 0;
  let totalFilesRead = 0;
  let totalFilesWritten = 0;
  let totalInputTokens = 0;
  let totalOutputTokens = 0;
  let blockedSessions = 0;
  let blockedTurns = 0;
  let weightedReadFirst = 0;
  let readFirstWeight = 0;
  let filesTouchedTotal = 0;

  sessions.forEach((session) => {
    projectKeys.add(session.cwd || session.display_name || session.session_id);
    turnCount += session.turn_count || 0;
    totalToolCalls += session.total_tool_calls || 0;
    totalFilesRead += session.total_files_read || 0;
    totalFilesWritten += session.total_files_written || 0;
    totalInputTokens += session.total_input_tokens || 0;
    totalOutputTokens += session.total_output_tokens || 0;
    blockedTurns += session.dlp_blocks || 0;
    filesTouchedTotal += (session.unique_files_read || 0) + (session.unique_files_written || 0);

    if ((session.dlp_blocks || 0) > 0) blockedSessions += 1;

    if ((session.total_files_written || 0) > 0) {
      weightedReadFirst += session.read_before_write_pct * session.total_files_written;
      readFirstWeight += session.total_files_written;
    }

    const mix = getSessionToolMix(session);
    toolMix.explore += mix.explore;
    toolMix.modify += mix.modify;
    toolMix.bash += mix.bash;
    toolMix.other += mix.other;
  });

  const readBeforeWritePct = readFirstWeight > 0 ? weightedReadFirst / readFirstWeight : null;
  const modifyBase = toolMix.modify + toolMix.bash;
  const explorationRatio = modifyBase > 0
    ? toolMix.explore / modifyBase
    : (toolMix.explore > 0 ? toolMix.explore : null);

  return {
    sessionCount: sessions.length,
    turnCount,
    projectCount: projectKeys.size,
    totalToolCalls,
    totalFilesRead,
    totalFilesWritten,
    totalInputTokens,
    totalOutputTokens,
    toolMix,
    blockedSessions,
    blockedTurns,
    readBeforeWritePct,
    explorationRatio,
    bashSharePct: totalToolCalls > 0 ? (toolMix.bash / totalToolCalls) * 100 : null,
    toolsPerTurn: turnCount > 0 ? totalToolCalls / turnCount : null,
    avgTurnsPerSession: sessions.length > 0 ? turnCount / sessions.length : 0,
    filesTouchedPerSession: sessions.length > 0 ? filesTouchedTotal / sessions.length : 0,
  };
}

function getSessionToolMix(session) {
  const mix = { explore: 0, modify: 0, bash: 0, other: 0 };

  (session.turns || []).forEach((turn) => {
    (turn.tool_counts || []).forEach((tool) => {
      mix[toolCategory(tool.tool_name)] += tool.count || 0;
    });
  });

  return mix;
}

function buildDayBuckets(sessions) {
  const byDay = new Map();

  sessions.forEach((session) => {
    const key = localDayKey(session.first_seen);
    if (!byDay.has(key)) byDay.set(key, []);
    byDay.get(key).push(session);
  });

  return Array.from(byDay.entries())
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([key, daySessions]) => {
      const date = parseDayKey(key);
      const aggregate = aggregateSessions(daySessions);
      return {
        key,
        label: formatDayShort(date),
        fullLabel: formatDayLong(date),
        ...aggregate,
      };
    });
}

function buildMetricCards(overall, dayBuckets) {
  return [
    {
      key: 'read',
      label: 'Read-first discipline',
      tag: 'Day-wise',
      value: overall.readBeforeWritePct == null ? 'No writes' : `${overall.readBeforeWritePct.toFixed(0)}%`,
      copy: describeReadFirst(overall.readBeforeWritePct, overall.totalFilesWritten),
      sparkline: dayBuckets.map((bucket) => ({ label: bucket.label, fullLabel: bucket.fullLabel, value: bucket.readBeforeWritePct })),
      scale: 'percent',
      footerLeft: 'Daily trend',
      footerRight: `${dayBuckets.length} active day${dayBuckets.length !== 1 ? 's' : ''}`,
    },
    {
      key: 'explore',
      label: 'Exploration balance',
      tag: 'Day-wise',
      value: overall.explorationRatio == null ? 'No edits' : `${overall.explorationRatio.toFixed(1)}x`,
      copy: describeExploration(overall.explorationRatio, overall.totalToolCalls),
      sparkline: dayBuckets.map((bucket) => ({ label: bucket.label, fullLabel: bucket.fullLabel, value: bucket.explorationRatio })),
      scale: 'ratio',
      footerLeft: 'Daily trend',
      footerRight: `${dayBuckets.length} active day${dayBuckets.length !== 1 ? 's' : ''}`,
    },
    {
      key: 'bash',
      label: 'Bash reliance',
      tag: 'Day-wise',
      value: overall.bashSharePct == null ? 'No tools' : `${overall.bashSharePct.toFixed(0)}%`,
      copy: describeBashShare(overall.bashSharePct, overall.totalToolCalls),
      sparkline: dayBuckets.map((bucket) => ({ label: bucket.label, fullLabel: bucket.fullLabel, value: bucket.bashSharePct })),
      scale: 'percent',
      footerLeft: 'Daily trend',
      footerRight: `${dayBuckets.length} active day${dayBuckets.length !== 1 ? 's' : ''}`,
    },
    {
      key: 'tempo',
      label: 'Tool tempo',
      tag: 'Day-wise',
      value: overall.toolsPerTurn == null ? 'No turns' : `${overall.toolsPerTurn.toFixed(1)} / turn`,
      copy: describeTempo(overall.toolsPerTurn, overall.turnCount),
      sparkline: dayBuckets.map((bucket) => ({ label: bucket.label, fullLabel: bucket.fullLabel, value: bucket.toolsPerTurn })),
      scale: 'rate',
      footerLeft: 'Daily trend',
      footerRight: `${dayBuckets.length} active day${dayBuckets.length !== 1 ? 's' : ''}`,
    },
  ];
}

function buildMixRows(toolMix, totalToolCalls) {
  const categories = [
    { key: 'explore', label: 'Explore', count: toolMix.explore },
    { key: 'modify', label: 'Modify', count: toolMix.modify },
    { key: 'bash', label: 'Bash', count: toolMix.bash },
    { key: 'other', label: 'Other', count: toolMix.other },
  ];

  return categories.map((row) => ({
    ...row,
    percent: totalToolCalls > 0 ? (row.count / totalToolCalls) * 100 : 0,
    percentLabel: `${totalToolCalls > 0 ? Math.round((row.count / totalToolCalls) * 100) : 0}%`,
    countLabel: formatNumber(row.count),
  }));
}

function buildProfileRows(overall) {
  return [
    {
      label: 'Investigation',
      value: overall.readBeforeWritePct == null
        ? 'Read-only'
        : overall.readBeforeWritePct >= 85
          ? 'Read-first'
          : overall.readBeforeWritePct >= 70
            ? 'Mostly read-first'
            : 'Edit-led',
      detail: overall.readBeforeWritePct == null
        ? 'Recent activity is mostly inspection, with no file writes recorded.'
        : `${overall.readBeforeWritePct.toFixed(0)}% of writes follow a prior read.`,
    },
    {
      label: 'Balance',
      value: overall.explorationRatio == null
        ? 'No edits'
        : overall.explorationRatio >= 2.5
          ? 'Investigative'
          : overall.explorationRatio >= 1
            ? 'Balanced'
            : 'Edit-led',
      detail: overall.explorationRatio == null
        ? 'Not enough modify activity to compute an explore/modify balance.'
        : `Explore/modify ratio is ${overall.explorationRatio.toFixed(1)}x.`,
    },
    {
      label: 'Shell',
      value: overall.bashSharePct == null
        ? 'No tools'
        : overall.bashSharePct >= 30
          ? 'High'
          : overall.bashSharePct >= 15
            ? 'Moderate'
            : 'Low',
      detail: overall.bashSharePct == null
        ? 'No tool activity was recorded in the selected range.'
        : `${overall.bashSharePct.toFixed(0)}% of tool calls are bash commands.`,
    },
    {
      label: 'Tempo',
      value: overall.toolsPerTurn == null
        ? 'No turns'
        : overall.toolsPerTurn >= 4.5
          ? 'Busy'
          : overall.toolsPerTurn >= 2.5
            ? 'Steady'
            : 'Light',
      detail: overall.toolsPerTurn == null
        ? 'No turns were recorded in the selected range.'
        : `${overall.toolsPerTurn.toFixed(1)} tool calls per turn.`,
    },
  ];
}

function buildCoverageStats(overall, dayBuckets) {
  return [
    {
      label: 'Active days',
      value: `${dayBuckets.length}`,
      detail: `${overall.sessionCount} session${overall.sessionCount !== 1 ? 's' : ''} recorded.`,
    },
    {
      label: 'Avg turns / session',
      value: overall.avgTurnsPerSession.toFixed(1),
      detail: `${overall.turnCount} total turn${overall.turnCount !== 1 ? 's' : ''}.`,
    },
    {
      label: 'Files touched / session',
      value: overall.filesTouchedPerSession.toFixed(1),
      detail: `${overall.projectCount} project${overall.projectCount !== 1 ? 's' : ''} observed.`,
    },
    {
      label: 'Guardrails',
      value: overall.blockedSessions > 0 ? `${overall.blockedSessions} session${overall.blockedSessions !== 1 ? 's' : ''}` : 'Quiet',
      detail: overall.blockedTurns > 0
        ? `${overall.blockedTurns} blocked turn${overall.blockedTurns !== 1 ? 's' : ''}.`
        : 'No blocked prompts or actions recorded.',
    },
  ];
}

function deriveSessionMetrics(session) {
  const mix = getSessionToolMix(session);
  const totalToolCalls = session.total_tool_calls || 0;

  return {
    toolsPerTurn: session.turn_count > 0 ? totalToolCalls / session.turn_count : 0,
    bashSharePct: totalToolCalls > 0 ? (mix.bash / totalToolCalls) * 100 : 0,
  };
}

function buildSessionFlags(session, metrics) {
  const flags = [];

  if (session.dlp_blocks > 0) {
    flags.push({ tone: 'risk', label: `${session.dlp_blocks} blocked` });
  }

  if (session.total_files_written > 0) {
    if (session.read_before_write_pct >= 85) {
      flags.push({ tone: 'good', label: 'read-first' });
    } else if (session.read_before_write_pct < 60) {
      flags.push({ tone: 'watch', label: 'edit-first' });
    }
  }

  if (metrics.bashSharePct >= 30) {
    flags.push({ tone: 'watch', label: 'bash-heavy' });
  }

  if (session.exploration_ratio >= 2.5) {
    flags.push({ tone: 'good', label: 'deep-scan' });
  } else if (session.total_files_written > 0 && session.exploration_ratio < 0.8) {
    flags.push({ tone: 'watch', label: 'low-explore' });
  }

  if (metrics.toolsPerTurn >= 4.5) {
    flags.push({ tone: 'note', label: 'high-tempo' });
  }

  return flags.slice(0, 4);
}

// ============ Copy Helpers ============

function describeReadFirst(value, totalWrites) {
  if (totalWrites === 0 || value == null) return 'No writes in the selected range yet.';
  if (value >= 85) return 'Writes usually follow prior reads.';
  if (value >= 70) return 'Mostly read-first, with some direct editing.';
  if (value >= 50) return 'Agents are starting edits earlier than ideal.';
  return 'Edits often happen without enough prior reading.';
}

function describeExploration(value, totalToolCalls) {
  if (totalToolCalls === 0 || value == null) return 'Not enough tool activity to compute a balance.';
  if (value >= 2.5) return 'Investigation clearly outweighs modification.';
  if (value >= 1) return 'Exploration and modification are reasonably balanced.';
  if (value > 0) return 'Modification is outpacing exploration.';
  return 'Almost no exploration activity is being captured.';
}

function describeBashShare(value, totalToolCalls) {
  if (totalToolCalls === 0 || value == null) return 'No tool activity recorded.';
  if (value >= 30) return 'Shell commands are a major part of execution.';
  if (value >= 15) return 'Moderate shell usage alongside structured tools.';
  return 'Most work is flowing through structured tools instead of bash.';
}

function describeTempo(value, turnCount) {
  if (turnCount === 0 || value == null) return 'No turns recorded yet.';
  if (value >= 4.5) return 'Turns are action-heavy and multi-step.';
  if (value >= 2.5) return 'Tool cadence is steady and moderate.';
  return 'Turns are relatively compact.';
}

// ============ Generic Helpers ============

function escHtml(text) {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

function shortPath(path) {
  if (!path) return '';
  const parts = path.split('/');
  if (parts.length <= 2) return path;
  return `.../${parts.slice(-2).join('/')}`;
}

function formatDuration(ms) {
  if (ms < 1000) return '<1s';
  const secs = Math.floor(ms / 1000);
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ${secs % 60}s`;
  const hrs = Math.floor(mins / 60);
  return `${hrs}h ${mins % 60}m`;
}

function formatTime(ts) {
  try {
    const d = new Date(ts);
    return d.toLocaleString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  } catch {
    return ts;
  }
}

function localDayKey(ts) {
  const d = new Date(ts);
  const yyyy = d.getFullYear();
  const mm = String(d.getMonth() + 1).padStart(2, '0');
  const dd = String(d.getDate()).padStart(2, '0');
  return `${yyyy}-${mm}-${dd}`;
}

function parseDayKey(key) {
  const [yyyy, mm, dd] = key.split('-').map(Number);
  return new Date(yyyy, mm - 1, dd);
}

function formatDayShort(date) {
  return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

function formatDayLong(date) {
  return date.toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' });
}

function timeRangeLabel(value) {
  switch (value) {
    case '1h': return 'Last 1 hour';
    case '6h': return 'Last 6 hours';
    case '1d': return 'Last 1 day';
    case '7d': return 'Last 7 days';
    case 'all': return 'All time';
    default: return value;
  }
}

function readFirstClass(value) {
  if (value >= 80) return 'good';
  if (value >= 60) return 'ok';
  return 'warn';
}

function toolCategory(name) {
  const n = (name || '').toLowerCase();
  if (n === 'read' || n === 'grep' || n === 'glob') return 'explore';
  if (n === 'write' || n === 'edit' || n === 'notebookedit') return 'modify';
  if (n === 'bash') return 'bash';
  return 'other';
}

function toolClass(name) {
  return `beh-tool--${toolCategory(name)}`;
}
