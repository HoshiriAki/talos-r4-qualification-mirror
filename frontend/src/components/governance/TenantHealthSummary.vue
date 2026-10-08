<script setup lang="ts">
import { shanghaiTimestampLabel } from '@/utils/businessDate'
import type { TenantHealthSummary } from '@/api/tenant-governance'

defineProps<{ health: TenantHealthSummary | null; loading?: boolean; error?: string | null }>()

const labels = { healthy: '正常', degraded: '降级', unavailable: '不可用', unknown: '未知' }
</script>

<template>
  <section class="card" aria-labelledby="tenant-health-title">
    <div class="section-head">
      <div>
        <p class="eyebrow">HEALTH / 运行观测</p>
        <h2 id="tenant-health-title">租户健康摘要</h2>
      </div>
      <span v-if="health" class="health-badge" :data-status="health.status">{{ labels[health.status] }}</span>
    </div>
    <p v-if="loading" class="state-copy" role="status">正在读取健康信号…</p>
    <p v-else-if="error" class="state-copy error" role="alert">{{ error }}</p>
    <template v-else-if="health">
      <div class="metrics">
        <div><span>24 小时审计事件</span><strong>{{ health.auditEvents24h }}</strong></div>
        <div><span>失败命令</span><strong>{{ health.failedCommandCount }}</strong></div>
        <div><span>最后活动</span><strong class="timestamp">{{ health.lastActivityAt ? shanghaiTimestampLabel(health.lastActivityAt) : '—' }}</strong></div>
      </div>
      <ul class="signals">
        <li v-for="signal in health.signals" :key="signal.key">
          <span class="signal-dot" :data-status="signal.status"></span>
          <span><strong>{{ signal.label }}</strong><small>{{ signal.detail || labels[signal.status] }}</small></span>
        </li>
      </ul>
      <p class="checked">检查时间（Asia/Shanghai）：{{ shanghaiTimestampLabel(health.checkedAt) }}</p>
    </template>
    <p v-else class="state-copy">暂无健康数据</p>
  </section>
</template>

<style scoped>
.section-head,.metrics,.signals li{display:flex;align-items:center}.section-head{justify-content:space-between;gap:16px}.eyebrow,.checked{font-family:var(--font-mono);color:var(--text-tertiary);font-size:12px;letter-spacing:.1em}.eyebrow{margin:0 0 5px}h2{margin:0;font:600 18px var(--font-mono);color:var(--text-primary)}.health-badge{padding:5px 10px;border:1px solid var(--border-base);border-radius:var(--radius-lg);font:600 12px var(--font-mono)}[data-status=healthy]{color:var(--status-success)}[data-status=degraded]{color:var(--status-warning)}[data-status=unavailable]{color:var(--status-error)}[data-status=unknown]{color:var(--text-tertiary)}.metrics{display:grid;grid-template-columns:repeat(3,1fr);gap:1px;margin:18px 0;background:var(--border-base);border:1px solid var(--border-base)}.metrics div{background:var(--bg-surface);padding:14px;display:grid;gap:7px}.metrics span{font-size:12px;color:var(--text-tertiary)}.metrics strong{font:700 22px var(--font-mono);color:var(--text-primary)}.metrics .timestamp{font-size:12px}.signals{list-style:none;padding:0;margin:0;display:grid;gap:8px}.signals li{gap:10px;padding:9px 0;border-bottom:1px solid var(--border-base)}.signals li>span:last-child{display:grid;gap:2px}.signals strong{font-size:13px;color:var(--text-primary)}.signals small{color:var(--text-tertiary)}.signal-dot{width:7px;height:7px;border-radius:var(--radius-sm);background:currentColor}.state-copy{padding:22px 0;color:var(--text-tertiary)}.error{color:var(--status-error)}.checked{margin:14px 0 0}@media(max-width:720px){.metrics{grid-template-columns:1fr}}
</style>
