<script setup lang="ts">
import { computed, reactive, watch } from 'vue'
import { useTenantGovernanceStore } from '@/stores/tenantGovernance'

const props = defineProps<{ visible: boolean; tenantId: string; tenantName: string }>()
const emit = defineEmits<{ close: []; recorded: [intentId: string] }>()
const store = useTenantGovernanceStore()
const form = reactive({ reason: '', intendedOutcome: '', impact: '', costMinor: undefined as number | undefined, currency: 'CNY' })
const canSubmit = computed(() => form.reason.trim() && form.intendedOutcome.trim() && (!form.costMinor || form.costMinor >= 0))

watch(() => props.visible, visible => {
  if (visible) Object.assign(form, { reason: '', intendedOutcome: '', impact: '', costMinor: undefined, currency: 'CNY' })
})

async function submit() {
  if (!canSubmit.value) return
  const intent = await store.recordIntent({
    targetTenantId: props.tenantId,
    reason: form.reason.trim(),
    intendedOutcome: form.intendedOutcome.trim(),
    impact: form.impact.trim() || undefined,
    costMinor: form.costMinor,
    currency: form.costMinor !== undefined ? form.currency.trim().toUpperCase() : undefined,
  })
  emit('recorded', intent.id)
}
</script>

<template>
  <div v-if="visible" class="overlay" role="presentation" @click.self="emit('close')">
    <section class="dialog glass" role="dialog" aria-modal="true" aria-labelledby="intent-title">
      <header><div><p>CHANGE INTENT / 仅记录意图</p><h2 id="intent-title">为 {{ tenantName }} 记录变更意图</h2></div><button class="btn-ghost" type="button" aria-label="关闭" @click="emit('close')">×</button></header>
      <div class="notice">此操作不会修改租户生产数据，也不会自动发起审批或应用配置。</div>
      <form @submit.prevent="submit">
        <label>业务原因<textarea v-model="form.reason" class="input" required rows="3" placeholder="说明原因、风险与验证标准"></textarea></label>
        <label>期望结果<textarea v-model="form.intendedOutcome" class="input" required rows="3" placeholder="说明希望观察到的业务结果"></textarea></label>
        <label>影响范围（可选）<textarea v-model="form.impact" class="input" rows="2" placeholder="受影响的流程、人员或数据范围"></textarea></label>
        <div class="cost-row"><label>预计成本（最小货币单位，可选）<input v-model.number="form.costMinor" class="input mono" type="number" min="0" step="1" placeholder="例如 12500" /></label><label>币种<input v-model="form.currency" class="input mono" maxlength="3" :disabled="form.costMinor === undefined" /></label></div>
        <p v-if="store.errors.intent" class="form-error" role="alert">{{ store.errors.intent }}</p>
        <footer><button class="btn-secondary" type="button" @click="emit('close')">取消</button><button class="btn-primary" type="submit" :disabled="!canSubmit || store.loading.intent">{{ store.loading.intent ? '记录中…' : '记录意图' }}</button></footer>
      </form>
    </section>
  </div>
</template>

<style scoped>
.overlay{position:fixed;inset:0;z-index:1200;display:grid;place-items:center;padding:20px;background:var(--overlay-bg)}.dialog{width:min(640px,100%);max-height:90vh;overflow:auto;border-radius:var(--radius-xl);box-shadow:var(--shadow-offset-accent)}header,footer{display:flex;align-items:center;justify-content:space-between;gap:18px}header{padding:20px 22px;border-bottom:var(--glass-divider)}header p{margin:0 0 6px;font:12px var(--font-mono);letter-spacing:.1em;color:var(--text-tertiary)}h2{margin:0;font:600 18px var(--font-mono);color:var(--text-primary)}.notice{margin:18px 22px 0;padding:11px 13px;border:1px solid var(--status-warning);border-radius:var(--radius-md);background:var(--bg-elevated);color:var(--text-secondary);font-size:14px}form{padding:20px 22px;display:grid;gap:16px}label{display:grid;gap:7px;color:var(--text-secondary);font-size:14px}.cost-row{display:grid;grid-template-columns:1fr 120px;gap:12px}.mono{font-family:var(--font-mono)}small,.form-error{color:var(--status-error);font-size:12px;margin:0}footer{padding-top:4px;justify-content:flex-end}@media(max-width:560px){.cost-row{grid-template-columns:1fr}}
</style>
