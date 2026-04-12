import { charts, setCharts, colors, formatNumber, formatLatency, shortenModel } from './utils.js';

// Extended color palette for charts
const chartColors = [
  colors.primary,    // Indigo
  colors.secondary,  // Green
  colors.warning,    // Amber
  colors.pink,       // Pink
  colors.blue,       // Blue
  '#8b5cf6',         // Purple
  '#14b8a6',         // Teal
  '#f97316',         // Orange
  '#ef4444',         // Red
  '#84cc16',         // Lime
];

// Destroy existing charts
export function destroyCharts() {
  Object.values(charts).forEach(chart => chart.destroy());
  setCharts({});
}

// Create Models Chart (Horizontal Bar)
export function createModelsChart(container, models) {
  const ctx = document.createElement('canvas');
  container.appendChild(ctx);

  const data = models.slice(0, 5); // Top 5 models
  const labels = data.map(m => shortenModel(m.model));
  const values = data.map(m => m.count);

  const newCharts = { ...charts };
  newCharts.models = new Chart(ctx, {
    type: 'bar',
    data: {
      labels,
      datasets: [{
        data: values,
        backgroundColor: [colors.primary, colors.secondary, colors.warning, colors.pink, colors.blue],
        borderRadius: 6,
        barThickness: 24,
      }]
    },
    options: {
      indexAxis: 'y',
      responsive: true,
      maintainAspectRatio: false,
      plugins: {
        legend: { display: false }
      },
      scales: {
        x: {
          grid: { display: false },
          ticks: { font: { size: 11 } }
        },
        y: {
          grid: { display: false },
          ticks: { font: { size: 11 } }
        }
      }
    }
  });
  setCharts(newCharts);
}

// Create Token Usage Chart (Stacked Bar per request)
export function createTokenChart(container, requests) {
  const ctx = document.createElement('canvas');
  container.appendChild(ctx);

  // Reverse to show oldest first (left to right)
  const data = [...requests].reverse();
  const labels = data.map((_, i) => `#${i + 1}`);

  const newCharts = { ...charts };
  newCharts.tokens = new Chart(ctx, {
    type: 'bar',
    data: {
      labels,
      datasets: [
        {
          label: 'Input',
          data: data.map(r => r.input_tokens),
          backgroundColor: colors.primary,
          borderRadius: 4,
        },
        {
          label: 'Output',
          data: data.map(r => r.output_tokens),
          backgroundColor: colors.secondary,
          borderRadius: 4,
        },
        {
          label: 'Cache Read',
          data: data.map(r => r.cache_read_tokens),
          backgroundColor: colors.warning,
          borderRadius: 4,
        },
      ]
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      plugins: {
        legend: {
          position: 'top',
          labels: {
            boxWidth: 12,
            padding: 16,
            font: { size: 11 }
          }
        }
      },
      scales: {
        x: {
          stacked: true,
          grid: { display: false },
          ticks: { font: { size: 10 } }
        },
        y: {
          stacked: true,
          grid: { color: 'rgba(113, 208, 131, 0.08)' },
          ticks: {
            font: { size: 10 },
            color: '#888',
            callback: v => formatNumber(v)
          }
        }
      }
    }
  });
  setCharts(newCharts);
}

// Create Latency Chart (Line)
export function createLatencyChart(container, latencyPoints) {
  const ctx = document.createElement('canvas');
  container.appendChild(ctx);

  // Reverse to show oldest first
  const data = [...latencyPoints].reverse();
  const labels = data.map((_, i) => i + 1);
  const values = data.map(p => p.latency_ms);

  const newCharts = { ...charts };
  newCharts.latency = new Chart(ctx, {
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
        pointRadius: 2,
        pointHoverRadius: 4,
      }]
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      plugins: {
        legend: { display: false }
      },
      scales: {
        x: {
          display: false,
        },
        y: {
          grid: { color: 'rgba(113, 208, 131, 0.08)' },
          ticks: {
            font: { size: 10 },
            color: '#888',
            callback: v => formatLatency(v)
          }
        }
      }
    }
  });
  setCharts(newCharts);
}

// Create Tool Calls Chart (Horizontal Bar) - kept for backwards compatibility
export function createToolCallsChart(container, toolCallStats) {
  const ctx = document.createElement('canvas');
  container.appendChild(ctx);

  const data = toolCallStats.slice(0, 10); // Top 10 tools
  const labels = data.map(t => t.tool_name);
  const values = data.map(t => t.count);

  const newCharts = { ...charts };
  newCharts.toolCalls = new Chart(ctx, {
    type: 'bar',
    data: {
      labels,
      datasets: [{
        data: values,
        backgroundColor: chartColors.slice(0, data.length),
        borderRadius: 6,
        barThickness: 20,
      }]
    },
    options: {
      indexAxis: 'y',
      responsive: true,
      maintainAspectRatio: false,
      plugins: {
        legend: { display: false }
      },
      scales: {
        x: {
          grid: { display: false },
          ticks: { font: { size: 11 } }
        },
        y: {
          grid: { display: false },
          ticks: { font: { size: 11 } }
        }
      }
    }
  });
  setCharts(newCharts);
}

// Create Tool Insights Chart (Nested Doughnut / Sunburst)
// Inner ring: tools, Outer ring: targets aligned with each tool's arc
export function createToolInsightsChart(container, insights) {
  const ctx = document.createElement('canvas');
  container.appendChild(ctx);

  const { tools } = insights;

  // Inner ring: tools
  const innerLabels = tools.map(t => t.tool_name);
  const innerValues = tools.map(t => t.count);
  const innerColors = tools.map((_, i) => chartColors[i % chartColors.length]);

  // Outer ring: targets aligned with parent tool's arc
  // Each tool's targets must sum to that tool's count to align properly
  const outerLabels = [];
  const outerValues = [];
  const outerColors = [];
  const outerMeta = []; // Store tool name for tooltip

  tools.forEach((tool, toolIndex) => {
    const baseColor = chartColors[toolIndex % chartColors.length];

    if (tool.targets.length === 0) {
      // No targets extracted - show single segment labeled "other"
      outerLabels.push('other');
      outerValues.push(tool.count);
      outerColors.push(baseColor + 'AA');
      outerMeta.push({ tool: tool.tool_name, target: 'other' });
    } else {
      // Add each target
      let targetSum = 0;
      tool.targets.forEach((target, targetIndex) => {
        outerLabels.push(target.target);
        outerValues.push(target.count);
        // Vary opacity slightly for each target
        const opacity = Math.max(60, 99 - targetIndex * 15).toString(16).padStart(2, '0');
        outerColors.push(baseColor + opacity);
        outerMeta.push({ tool: tool.tool_name, target: target.target });
        targetSum += target.count;
      });

      // Add "other" segment for remaining count
      const remaining = tool.count - targetSum;
      if (remaining > 0) {
        outerLabels.push('other');
        outerValues.push(remaining);
        outerColors.push(baseColor + '40');
        outerMeta.push({ tool: tool.tool_name, target: 'other' });
      }
    }
  });

  // Custom plugin to render labels on chart segments
  const labelPlugin = {
    id: 'segmentLabels',
    afterDraw: (chart) => {
      const ctx = chart.ctx;
      ctx.save();

      chart.data.datasets.forEach((dataset, datasetIndex) => {
        const meta = chart.getDatasetMeta(datasetIndex);

        meta.data.forEach((arc, index) => {
          // Get arc properties
          const { x, y, startAngle, endAngle, innerRadius, outerRadius } = arc.getProps(['x', 'y', 'startAngle', 'endAngle', 'innerRadius', 'outerRadius']);

          // Calculate the angle span and mid angle
          const angleSpan = endAngle - startAngle;
          const midAngle = startAngle + angleSpan / 2;

          // Skip very small segments (less than ~15 degrees)
          if (angleSpan < 0.26) return;

          // Calculate label position (middle of the arc)
          const midRadius = (innerRadius + outerRadius) / 2;
          const labelX = x + Math.cos(midAngle) * midRadius;
          const labelY = y + Math.sin(midAngle) * midRadius;

          // Get the label text
          let label;
          if (datasetIndex === 1) {
            // Inner ring - tools
            label = innerLabels[index];
          } else {
            // Outer ring - targets
            label = outerLabels[index];
          }

          // Calculate available arc length for text
          const arcLength = angleSpan * midRadius;
          const fontSize = datasetIndex === 1 ? 11 : 9;

          // Truncate label if needed
          ctx.font = `${fontSize}px -apple-system, BlinkMacSystemFont, sans-serif`;
          const maxChars = Math.floor(arcLength / (fontSize * 0.6));
          if (label.length > maxChars && maxChars > 3) {
            label = label.substring(0, maxChars - 2) + '..';
          } else if (maxChars <= 3) {
            return; // Skip if too small
          }

          // Draw label
          ctx.textAlign = 'center';
          ctx.textBaseline = 'middle';
          ctx.fillStyle = '#fff';
          ctx.shadowColor = 'rgba(0, 0, 0, 0.5)';
          ctx.shadowBlur = 3;
          ctx.fillText(label, labelX, labelY);
        });
      });

      ctx.restore();
    }
  };

  const newCharts = { ...charts };
  newCharts.toolInsights = new Chart(ctx, {
    type: 'doughnut',
    data: {
      datasets: [
        {
          // Outer ring - targets (larger circle)
          label: 'Targets',
          data: outerValues,
          backgroundColor: outerColors,
          borderWidth: 1,
          borderColor: '#1D2021',
        },
        {
          // Inner ring - tools (smaller circle)
          label: 'Tools',
          data: innerValues,
          backgroundColor: innerColors,
          borderWidth: 2,
          borderColor: '#1D2021',
        }
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
            boxWidth: 12,
            padding: 8,
            font: { size: 11 },
            color: '#aaa',
            generateLabels: () => {
              // Only show tools in legend
              return tools.map((tool, i) => ({
                text: `${tool.tool_name} (${tool.count})`,
                fillStyle: chartColors[i % chartColors.length],
                strokeStyle: '#1D2021',
                fontColor: '#aaa',
                lineWidth: 1,
                index: i
              }));
            }
          }
        },
        tooltip: {
          callbacks: {
            label: (context) => {
              const datasetIndex = context.datasetIndex;
              const index = context.dataIndex;
              if (datasetIndex === 1) {
                // Inner ring - tool
                return `${tools[index].tool_name}: ${tools[index].count} calls`;
              } else {
                // Outer ring - target
                const meta = outerMeta[index];
                return `${meta.tool} → ${meta.target}: ${outerValues[index]}`;
              }
            }
          }
        }
      }
    },
    plugins: [labelPlugin]
  });
  setCharts(newCharts);
}

// Pretty method names for token saving methods
const methodLabels = {
  shell_compression: 'Shell Compression',
  ctx_read: 'File Read Cache',
  ctx_smart_read: 'Smart Read',
  unknown: 'Other',
};

function prettyMethod(method) {
  return methodLabels[method] || method;
}

// Create Token Savings Chart (Doughnut by method)
export function createTokenSavingsChart(container, savingsData) {
  const ctx = document.createElement('canvas');
  container.appendChild(ctx);

  const labels = savingsData.by_method.map(m => prettyMethod(m.method));
  const values = savingsData.by_method.map(m => m.tokens_saved);
  const backgroundColors = savingsData.by_method.map((_, i) => chartColors[i % chartColors.length]);

  // Segment label plugin (same style as Tool Insights)
  const labelPlugin = {
    id: 'savingsSegmentLabels',
    afterDraw: (chart) => {
      const ctx = chart.ctx;
      ctx.save();
      const meta = chart.getDatasetMeta(0);
      meta.data.forEach((arc, index) => {
        const { x, y, startAngle, endAngle, innerRadius, outerRadius } = arc.getProps(['x', 'y', 'startAngle', 'endAngle', 'innerRadius', 'outerRadius']);
        const angleSpan = endAngle - startAngle;
        if (angleSpan < 0.26) return;
        const midAngle = startAngle + angleSpan / 2;
        const midRadius = (innerRadius + outerRadius) / 2;
        const labelX = x + Math.cos(midAngle) * midRadius;
        const labelY = y + Math.sin(midAngle) * midRadius;
        let label = labels[index];
        const arcLength = angleSpan * midRadius;
        const fontSize = 11;
        ctx.font = `${fontSize}px -apple-system, BlinkMacSystemFont, sans-serif`;
        const maxChars = Math.floor(arcLength / (fontSize * 0.6));
        if (label.length > maxChars && maxChars > 3) {
          label = label.substring(0, maxChars - 2) + '..';
        } else if (maxChars <= 3) { ctx.restore(); return; }
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillStyle = '#fff';
        ctx.shadowColor = 'rgba(0, 0, 0, 0.5)';
        ctx.shadowBlur = 3;
        ctx.fillText(label, labelX, labelY);
      });
      ctx.restore();
    }
  };

  const newCharts = { ...charts };
  newCharts.tokenSavings = new Chart(ctx, {
    type: 'doughnut',
    data: {
      labels,
      datasets: [{
        data: values,
        backgroundColor: backgroundColors,
        borderWidth: 2,
        borderColor: '#1D2021',
      }]
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
            boxWidth: 12,
            padding: 8,
            font: { size: 11 },
            color: '#aaa',
            generateLabels: () => {
              return savingsData.by_method.map((m, i) => ({
                text: `${prettyMethod(m.method)} (${formatNumber(m.tokens_saved)})`,
                fillStyle: backgroundColors[i],
                strokeStyle: '#1D2021',
                fontColor: '#aaa',
                lineWidth: 1,
                index: i
              }));
            }
          }
        },
        tooltip: {
          callbacks: {
            label: (context) => {
              const total = context.dataset.data.reduce((a, b) => a + b, 0);
              const pct = Math.round((context.parsed / total) * 100);
              return `${context.label}: ${formatNumber(context.parsed)} tokens (${pct}%)`;
            }
          }
        }
      }
    },
    plugins: [labelPlugin]
  });
  setCharts(newCharts);
}
