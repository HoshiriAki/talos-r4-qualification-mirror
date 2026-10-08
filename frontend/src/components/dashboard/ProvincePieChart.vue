<script setup lang="ts">
import { ref, onMounted, watch, nextTick } from 'vue'
import { useG2 } from '@/composables/useG2'
import { useChartTheme, escapeHtml } from '@/composables/useChartTheme'
import { fetchProvinceStats } from '@/api/dashboard'

const props = withDefaults(defineProps<{ days?: number }>(), { days: 30 })
const data = ref<any[]>([])
const loading = ref(true)
const error = ref('')
const { series, palettes } = useChartTheme()

const { containerRef, renderChart } = useG2((d, theme) => ({
  type: 'interval',
  data: d,
  encode: { y: 'count', color: 'province' },
  transform: [{ type: 'stackY' }],
  coordinate: { type: 'theta', innerRadius: 0.5 },
  scale: { color: { range: series.value } },
  labels: [{ text: 'province', position: 'outside', fontSize: 10, connector: true, connectorStyle: { stroke: palettes.value.grid, lineWidth: 0.5 } }],
  legend: { color: { position: 'right', itemLabelFontSize: 10, itemLabelFill: palettes.value.gray, itemMarker: 'square' } },
  tooltip: { items: [{ channel: 'y', name: '订单数' }] },
  interaction: { tooltip: { render: (_e: any, { items }: any) => { if (!items?.length) return ''; const p = palettes.value; const rows = items.filter(Boolean).map((item: any) => `<div style="display:flex;align-items:center;gap:6px;font-family:monospace;font-size:11px;line-height:1.7"><span style="display:inline-block;width:8px;height:8px;background:${item.color || p.marker};flex-shrink:0"></span><span style="color:#A0A0A8;white-space:nowrap">${item.name || ''}</span><span style="color:${p.tooltipText};font-weight:600;margin-left:auto;white-space:nowrap">${item.value ?? ''}</span></div>`).join(''); return `<div style="padding:6px 10px;background:${p.tooltipBg};border:1px solid ${p.tooltipBorder};font-family:monospace;font-size:12px;min-width:100px">${rows}</div>` } } },
}))

async function load() {
  loading.value = true; error.value = ''
  try {
    const result = await fetchProvinceStats('pie', props.days)
    const raw = Array.isArray(result) ? result : (result?.data || [])
    if (raw.length > 10) {
      const otherCount = raw.slice(10).reduce((s: number, d: any) => s + (d.count || 0), 0)
      data.value = [...raw.slice(0, 10), { province: '其他', count: otherCount }]
    } else { data.value = raw }
    await nextTick(); renderChart(data.value)
  } catch (e: any) { error.value = e.message || '加载失败'
  } finally { loading.value = false }
}

onMounted(load)
watch(() => props.days, load)
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
