<script setup lang="ts">
import { ref, onMounted, nextTick } from 'vue'
import { useG2 } from '@/composables/useG2'
import { useChartTheme, escapeHtml } from '@/composables/useChartTheme'
import { fetchDeviceStatusDistribution } from '@/api/dashboard'

const data = ref<any[]>([])
const loading = ref(true)
const error = ref('')
const { statusColors, palettes } = useChartTheme()

const STATUS_LABELS: Record<string, string> = {
  '已入库': '已入库', '租赁中': '租赁中', '返厂维修': '返厂维修', '确认丢失': '确认丢失', '已报废': '已报废',
}

const { containerRef, renderChart } = useG2((d, theme) => ({
  type: 'interval',
  data: d,
  encode: { y: 'count', color: 'status' },
  transform: [{ type: 'stackY' }],
  coordinate: { type: 'theta', innerRadius: 0.55 },
  scale: { color: { range: Object.values(statusColors.value) } },
  labels: [
    { text: 'status', position: 'outside', fontSize: 10, connector: true, connectorStyle: { stroke: palettes.value.grid, lineWidth: 0.5 } },
  ],
  legend: { color: { position: 'bottom', itemLabelFontSize: 10, itemLabelFill: palettes.value.gray, itemMarker: 'square' } },
  tooltip: { items: [{ channel: 'y', name: '数量' }] },
  interaction: { tooltip: { render: (_e: any, { items }: any) => { if (!items?.length) return ''; const p = palettes.value; const rows = items.filter(Boolean).map((item: any) => `<div style="display:flex;align-items:center;gap:6px;font-family:monospace;font-size:11px;line-height:1.7"><span style="display:inline-block;width:8px;height:8px;background:${item.color || p.marker};flex-shrink:0"></span><span style="color:#A0A0A8;white-space:nowrap">${item.name || ''}</span><span style="color:${p.tooltipText};font-weight:600;margin-left:auto;white-space:nowrap">${item.value ?? ''}</span></div>`).join(''); return `<div style="padding:6px 10px;background:${p.tooltipBg};border:1px solid ${p.tooltipBorder};font-family:monospace;font-size:12px;min-width:100px">${rows}</div>` } } },
}))

async function load() {
  loading.value = true; error.value = ''
  try {
    const result = await fetchDeviceStatusDistribution()
    data.value = Array.isArray(result) ? result : (result?.data || [])
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
@keyframes pulse{
  0%, 100% { opacity: 0.6; }
  50% { opacity: 1; }
}
</style>
