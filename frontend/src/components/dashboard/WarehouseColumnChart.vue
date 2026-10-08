<script setup lang="ts">
import { ref, onMounted, nextTick } from 'vue'
import { useG2 } from '@/composables/useG2'
import { useChartTheme, escapeHtml } from '@/composables/useChartTheme'
import { fetchWarehouseStats } from '@/api/dashboard'

const data = ref<any[]>([])
const loading = ref(true)
const error = ref('')
const { palettes } = useChartTheme()

const { containerRef, renderChart } = useG2((d, theme) => ({
  type: 'interval',
  data: d,
  encode: { x: 'warehouseName', y: 'count', color: 'type' },
  transform: [{ type: 'dodgeX' }],
  style: { radius: 4 },
  scale: { color: { range: [palettes.value.blue, palettes.value.green] } },
  axis: {
    x: { labelAutoRotate: true, title: false, line: false, tick: false, labelFill: palettes.value.gray, labelFontSize: 10 },
    y: { title: false, grid: { stroke: palettes.value.grid, lineWidth: 0.5 }, line: false, tick: false, labelFill: palettes.value.gray, labelFontSize: 10 },
  },
  labels: [{ text: 'count', position: 'top', fontSize: 10, fill: palettes.value.marker }],
  legend: { color: { position: 'top', itemLabelFontSize: 10, itemLabelFill: palettes.value.gray, itemMarker: 'square' } },
  tooltip: { items: [{ channel: 'y', name: '数量' }, { channel: 'color', name: '类型' }] },
  interaction: { tooltip: { render: (_e: any, { items }: any) => { if (!items?.length) return ''; const p = palettes.value; const rows = items.filter(Boolean).map((item: any) => `<div style="display:flex;align-items:center;gap:6px;font-family:monospace;font-size:11px;line-height:1.7"><span style="display:inline-block;width:8px;height:8px;background:${item.color || p.marker};flex-shrink:0"></span><span style="color:#A0A0A8;white-space:nowrap">${item.name || ''}</span><span style="color:${p.tooltipText};font-weight:600;margin-left:auto;white-space:nowrap">${item.value ?? ''}</span></div>`).join(''); return `<div style="padding:6px 10px;background:${p.tooltipBg};border:1px solid ${p.tooltipBorder};font-family:monospace;font-size:12px;min-width:110px">${rows}</div>` } } },
}))

function transformData(raw: { warehouseName: string; outgoing: number; stock: number }[]) {
  const result: { warehouseName: string; type: string; count: number }[] = []
  for (const item of raw) {
    result.push({ warehouseName: item.warehouseName, type: '出库', count: item.outgoing })
    result.push({ warehouseName: item.warehouseName, type: '库存', count: item.stock })
  }
  return result
}

async function load() {
  loading.value = true; error.value = ''
  try {
    const result = await fetchWarehouseStats()
    const raw = Array.isArray(result) ? result : (result?.data || [])
    data.value = transformData(raw)
    await nextTick(); renderChart(data.value)
  } catch (e: any) { error.value = e.message || '加载失败'
  } finally { loading.value = false }
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
