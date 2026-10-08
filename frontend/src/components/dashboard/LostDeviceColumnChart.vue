<script setup lang="ts">
import { ref, onMounted, nextTick } from 'vue'
import { useG2 } from '@/composables/useG2'
import { useChartTheme } from '@/composables/useChartTheme'

const loading = ref(true)
const error = ref('')
const { palettes } = useChartTheme()

function generateData() {
  const now = new Date()
  return Array.from({ length: 30 }, (_, i) => {
    const d = new Date(now); d.setDate(d.getDate() - 29 + i)
    return { date: `${d.getMonth() + 1}/${d.getDate()}`, count: Math.floor(Math.random() * 3) }
  })
}

const data = ref<any[]>([])

const { containerRef, renderChart } = useG2((d, theme) => ({
  type: 'interval',
  data: d,
  encode: { x: 'date', y: 'count' },
  style: { fill: palettes.value.red, fillOpacity: 0.85, radius: 4 },
  axis: {
    x: { labelAutoRotate: true, title: false, line: false, tick: false, labelFill: palettes.value.gray, labelFontSize: 10 },
    y: { title: false, tickCount: 5, grid: { stroke: palettes.value.grid, lineWidth: 0.5 }, line: false, tick: false, labelFill: palettes.value.gray, labelFontSize: 10 },
  },
  labels: [{ text: (d: any) => d.count > 0 ? String(d.count) : '', position: 'top', fontSize: 10, fill: palettes.value.red }],
  tooltip: { items: [{ channel: 'y', name: '丢失数' }] },
  interaction: { tooltip: { render: (_e: any, { items }: any) => { if (!items?.length) return ''; const p = palettes.value; const rows = items.filter(Boolean).map((item: any) => `<div style="display:flex;align-items:center;gap:6px;font-family:monospace;font-size:11px;line-height:1.7"><span style="display:inline-block;width:8px;height:8px;background:${item.color || p.red};flex-shrink:0"></span><span style="color:#A0A0A8;white-space:nowrap">丢失数</span><span style="color:${p.tooltipText};font-weight:600;margin-left:auto;white-space:nowrap">${item.value ?? ''}</span></div>`).join(''); return `<div style="padding:6px 10px;background:${p.tooltipBg};border:1px solid ${p.tooltipBorder};font-family:monospace;font-size:12px;min-width:100px">${rows}</div>` } } },
}))

async function load() {
  loading.value = true; error.value = ''
  try { data.value = generateData(); await nextTick(); renderChart(data.value) }
  catch (e: any) { error.value = e.message || '加载失败' }
  finally { loading.value = false }
}

onMounted(load)
</script>

<template>
  <div class="chart-widget">
    <div ref="containerRef" class="chart-container" />
    <div v-if="loading" class="chart-overlay chart-loading">加载中...</div>
    <div v-else-if="error" class="chart-overlay chart-error-msg">{{ error }}</div>
    <div v-else-if="!data.length" class="chart-overlay chart-no-data">暂无数据</div>
  </div>
</template>

<style scoped>
.chart-widget {
  position: relative;
  width: 100%;
  min-height: 220px;
}
.chart-container {
  width: 100%;
  min-height: 220px;
}
.chart-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-surface);
  z-index: 10;
  font-family: var(--font-mono);
  font-size: var(--font-size-xs);
}
.chart-loading {
  color: var(--text-muted);
  animation: pulse 1.5s ease-in-out infinite;
}
.chart-error-msg {
  color: var(--status-error);
  padding: 16px;
  text-align: center;
}
.chart-no-data {
  color: var(--text-muted);
}
@keyframes pulse {
  0%, 100% { opacity: 0.6; }
  50% { opacity: 1; }
}
</style>
