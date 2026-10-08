<script setup lang="ts">
import { shanghaiTimestampLabel } from '@/utils/businessDate'
import type { GovernanceAuditEvent } from '@/api/tenant-governance'

defineProps<{ events: GovernanceAuditEvent[]; loading?: boolean; error?: string | null; showTenant?: boolean }>()
const emit = defineEmits<{ select: [event: GovernanceAuditEvent] }>()
const resultLabel = { attempted: '尝试', succeeded: '成功', failed: '失败', denied: '拒绝' }
</script>

<template>
  <div class="audit-frame">
    <p v-if="loading" class="state" role="status">正在查询不可变审计记录…</p>
    <p v-else-if="error" class="state error" role="alert">{{ error }}</p>
    <div v-else-if="events.length" class="table-scroll">
      <table class="table">
        <thead><tr><th class="table-th">时间（上海）</th><th v-if="showTenant" class="table-th">租户</th><th class="table-th">命令</th><th class="table-th">操作者</th><th class="table-th">结果</th><th class="table-th">关联 ID</th></tr></thead>
        <tbody>
          <tr v-for="event in events" :key="event.id" class="table-tr audit-row" tabindex="0" @click="emit('select', event)" @keydown.enter="emit('select', event)">
            <td class="table-td mono">{{ shanghaiTimestampLabel(event.occurredAt) }}</td>
            <td v-if="showTenant" class="table-td">{{ event.tenantName || event.tenantId }}</td>
            <td class="table-td mono command">{{ event.command }}</td>
            <td class="table-td">{{ event.actorLabel || event.actorId || '系统' }}</td>
            <td class="table-td"><span class="result" :data-result="event.result">{{ resultLabel[event.result] }}</span></td>
            <td class="table-td mono correlation">{{ event.correlationId }}</td>
          </tr>
        </tbody>
      </table>
    </div>
    <p v-else class="state">当前筛选条件下没有审计事件</p>
  </div>
</template>

<style scoped>
.audit-frame{border:1px solid var(--border-base);border-radius:var(--radius-xl);background:var(--bg-surface);overflow:hidden}.table-scroll{overflow:auto}.audit-row{cursor:pointer}.audit-row:focus{outline:2px solid var(--accent);outline-offset:-2px}.mono{font-family:var(--font-mono);font-size:12px}.command{color:var(--text-primary)}.correlation{max-width:190px;overflow:hidden;text-overflow:ellipsis}.result{display:inline-flex;padding:3px 8px;border:1px solid currentColor;border-radius:var(--radius-lg);font:600 12px var(--font-mono)}[data-result=attempted]{color:var(--text-tertiary)}[data-result=succeeded]{color:var(--status-success)}[data-result=failed],[data-result=denied]{color:var(--status-error)}.state{margin:0;padding:28px;text-align:center;color:var(--text-tertiary);font-size:14px}.error{color:var(--status-error)}
</style>
