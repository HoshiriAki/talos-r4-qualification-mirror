<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useSimulationPricingStore } from '@/stores/simulationPricing'
import type { SimulationHolidayRule, SimulationPricingConfig } from '@/api/simulationPricing'

const route = useRoute()
const router = useRouter()
const pricing = useSimulationPricingStore()
const simulationId = computed(() => String(route.params.simulationId || ''))
const form = reactive<SimulationPricingConfig>({ baseWeekdayPrice: 8.5, baseWeekendPrice: 14, holidayRules: [], receiveShippingFees: { area1: 7, area2: 7, area3: 7, area4: 18 }, dynamicPriceMap: {} })
const startDate = ref(todayShanghai())
const endDate = ref(addDays(todayShanghai(), 1))

function todayShanghai() {
  const parts = new Intl.DateTimeFormat('en', { timeZone: 'Asia/Shanghai', year: 'numeric', month: '2-digit', day: '2-digit' }).formatToParts(new Date())
  const get = (type: string) => parts.find(part => part.type === type)?.value || ''
  return `${get('year')}-${get('month')}-${get('day')}`
}
function addDays(date: string, days: number) { const value = new Date(`${date}T12:00:00Z`); value.setUTCDate(value.getUTCDate() + days); return value.toISOString().slice(0, 10) }
function cloneConfig(source: SimulationPricingConfig) {
  form.baseWeekdayPrice = source.baseWeekdayPrice
  form.baseWeekendPrice = source.baseWeekendPrice
  form.holidayRules = source.holidayRules.map(rule => ({ ...rule }))
  form.receiveShippingFees = { ...source.receiveShippingFees }
  form.dynamicPriceMap = { ...source.dynamicPriceMap }
}
function configPayload(): SimulationPricingConfig {
  const map: Record<string, number> = {}
  for (const [date, price] of Object.entries(form.dynamicPriceMap)) if (date && Number.isFinite(Number(price)) && Number(price) >= 0) map[date] = Number(price)
  return {
    baseWeekdayPrice: Number(form.baseWeekdayPrice), baseWeekendPrice: Number(form.baseWeekendPrice),
    holidayRules: form.holidayRules.map(rule => ({ ...rule, price: Number(rule.price), includePreviousDay: Boolean(rule.includePreviousDay) })),
    receiveShippingFees: Object.fromEntries(Object.entries(form.receiveShippingFees).map(([area, fee]) => [area, Number(fee)])), dynamicPriceMap: map,
  }
}
function addHolidayRule() { form.holidayRules.push({ name: '', startDate: '', endDate: '', price: 0, includePreviousDay: false }) }
function removeHolidayRule(index: number) { form.holidayRules.splice(index, 1) }
function dynamicDates() { return Array.from({ length: 15 }, (_, index) => addDays(todayShanghai(), index)) }
function sourceLabel(source?: string) { return source ? source.replaceAll('_', ' ') : 'resolved' }
function money(value: number) { return `¥${Number(value || 0).toFixed(2)}` }
function validForm() {
  const base = Number(form.baseWeekdayPrice) >= 0 && Number(form.baseWeekendPrice) >= 0
  const rules = form.holidayRules.every(rule => rule.name.trim() && /^\d{4}-\d{2}-\d{2}$/.test(rule.startDate) && /^\d{4}-\d{2}-\d{2}$/.test(rule.endDate) && rule.startDate <= rule.endDate && Number(rule.price) >= 0)
  return base && rules && Object.values(form.receiveShippingFees).every(fee => Number(fee) >= 0)
}
async function load() { if (simulationId.value) { await pricing.load(simulationId.value); cloneConfig(pricing.config) } }
async function save() {
  if (!validForm()) { pricing.error = '请填写有效的基础价格、节假日规则与收货运费。'; return }
  await pricing.save(configPayload()); cloneConfig(pricing.config)
}
async function estimate() {
  if (!startDate.value || !endDate.value || startDate.value > endDate.value) { pricing.error = '请选择有效的起止日期。'; return }
  await pricing.estimate(startDate.value, endDate.value)
}
watch(simulationId, () => { void load() })
onMounted(() => { void load() })
onBeforeUnmount(pricing.clear)
</script>

<template>
  <main class="pricing-page">
    <section class="simulation-banner" role="status">
      <div><p class="eyebrow">TENANT SIMULATION · PRICING OVERLAY</p><h1>隔离定价实验</h1><p>SIMULATION {{ simulationId }} · 仅写入模拟 overlay</p></div>
      <div class="banner-actions"><button class="btn-ghost" type="button" @click="router.push(`/control/simulation/${encodeURIComponent(simulationId)}`)">返回模拟工作台</button><span class="badge-warning">NO PRODUCTION APPLY</span></div>
    </section>
    <p class="notice">此页面不读取或调用常规定价模块；不包含型号、占用或物流估价。所有保存和估价均受当前 simulation session 约束。</p>
    <p v-if="pricing.error" class="error" role="alert">{{ pricing.error }}</p>
    <p v-if="pricing.loading" class="state">正在读取隔离定价配置…</p>

    <template v-else>
      <section class="card">
        <header class="section-header"><div><p class="eyebrow">GLOBAL BASE</p><h2>基础价格与收货运费</h2></div><button class="btn-primary" type="button" :disabled="pricing.saving" @click="save">{{ pricing.saving ? '正在写入 overlay…' : '保存隔离定价' }}</button></header>
        <div class="price-grid"><label>周一至周四 <input v-model.number="form.baseWeekdayPrice" class="input-mono" type="number" min="0" step="0.01" /></label><label>周五至周日 <input v-model.number="form.baseWeekendPrice" class="input-mono" type="number" min="0" step="0.01" /></label></div>
        <div class="fees-grid"><label v-for="area in ['area1','area2','area3','area4']" :key="area">{{ area }} 收货运费<input v-model.number="form.receiveShippingFees[area]" class="input-mono" type="number" min="0" step="0.01" /></label></div>
      </section>

      <section class="card"><header class="section-header"><div><p class="eyebrow">HOLIDAY RULES</p><h2>节假日规则</h2></div><button class="btn-secondary" type="button" @click="addHolidayRule">新增规则</button></header><div class="rules"><article v-for="(rule, index) in form.holidayRules" :key="index" class="rule"><label>名称<input v-model="rule.name" class="input" maxlength="80" /></label><label>开始<input v-model="rule.startDate" class="input-mono" type="date" /></label><label>结束<input v-model="rule.endDate" class="input-mono" type="date" /></label><label>日价<input v-model.number="rule.price" class="input-mono" type="number" min="0" step="0.01" /></label><label class="check"><input v-model="rule.includePreviousDay" type="checkbox" />包含前一天</label><button class="btn-danger" type="button" @click="removeHolidayRule(index)">移除</button></article><p v-if="!form.holidayRules.length" class="empty">没有节假日 override。</p></div></section>

      <section class="card"><header class="section-header"><div><p class="eyebrow">15-DAY DYNAMIC MAP</p><h2>动态日价</h2></div><span class="counter">仅未来 15 天</span></header><div class="dynamic-grid"><label v-for="date in dynamicDates()" :key="date">{{ date }}<input v-model.number="form.dynamicPriceMap[date]" class="input-mono" type="number" min="0" step="0.01" placeholder="继承" /></label></div></section>

      <section class="card"><header class="section-header"><div><p class="eyebrow">OVERLAY ESTIMATE</p><h2>隔离价格估算</h2></div><button class="btn-primary" type="button" :disabled="pricing.estimating" @click="estimate">{{ pricing.estimating ? '计算中…' : '计算' }}</button></header><div class="estimate-controls"><label>开始日期<input v-model="startDate" class="input-mono" type="date" /></label><label>结束日期<input v-model="endDate" class="input-mono" type="date" /></label></div><div v-if="pricing.estimateResult" class="estimate-result"><strong>总租金 {{ money(pricing.estimateResult.totalPrice) }}</strong><div class="table-wrap"><table class="table"><thead><tr><th class="table-th">日期</th><th class="table-th">来源</th><th class="table-th">日价</th></tr></thead><tbody><tr v-for="item in pricing.estimateResult.breakdown" :key="item.dateKey || item.date" class="table-tr"><td class="table-td font-mono">{{ item.dateKey || item.date }}</td><td class="table-td">{{ sourceLabel(item.source) }}</td><td class="table-td font-mono">{{ money(item.price) }}</td></tr></tbody></table></div></div></section>
    </template>
  </main>
</template>

<style scoped>
.pricing-page{max-width:1280px;margin:0 auto;padding:28px;display:grid;gap:18px}.simulation-banner{position:sticky;top:0;z-index:20;display:flex;align-items:center;justify-content:space-between;gap:18px;padding:18px 20px;border:2px solid var(--status-warning);background:var(--bg-elevated);box-shadow:4px 4px 0 var(--status-warning)}h1,h2{margin:0;font-family:var(--font-mono);color:var(--text-primary)}h1{font-size:var(--font-size-3xl)}.eyebrow{margin:0 0 6px;font:var(--font-size-xs) var(--font-mono);letter-spacing:.12em;color:var(--status-warning)}.simulation-banner p:not(.eyebrow),.notice,.counter{margin:6px 0 0;color:var(--text-secondary);font:var(--font-size-xs) var(--font-mono)}.banner-actions,.section-header{display:flex;align-items:center;gap:10px}.notice{padding:12px 14px;background:var(--bg-surface);outline:1px solid var(--status-warning)}.card{display:grid;gap:16px}.section-header{justify-content:space-between}.price-grid,.fees-grid,.estimate-controls{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:14px}.fees-grid{grid-template-columns:repeat(4,minmax(0,1fr))}label{display:grid;gap:6px;font:var(--font-size-xs) var(--font-mono);color:var(--text-secondary)}.rules{display:grid;gap:10px}.rule{display:grid;grid-template-columns:1.3fr 1fr 1fr .8fr auto auto;align-items:end;gap:10px;padding:12px;border:1px solid var(--border-base);background:var(--bg-surface)}.check{display:flex;align-items:center;gap:6px;padding-bottom:10px}.dynamic-grid{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:10px}.estimate-result{display:grid;gap:12px;padding:14px;border:1px solid var(--status-warning);background:var(--bg-surface);font-family:var(--font-mono)}.table-wrap{overflow:auto}.error{margin:0;color:var(--status-error);font-family:var(--font-mono)}.state,.empty{padding:22px;color:var(--text-tertiary);font-family:var(--font-mono)}@media(max-width:860px){.pricing-page{padding:18px}.simulation-banner,.section-header{align-items:stretch;flex-direction:column}.banner-actions{justify-content:space-between}.fees-grid,.dynamic-grid,.price-grid,.estimate-controls{grid-template-columns:1fr 1fr}.rule{grid-template-columns:1fr 1fr}.check{padding-bottom:0}}@media(max-width:520px){.fees-grid,.dynamic-grid,.price-grid,.estimate-controls,.rule{grid-template-columns:1fr}}
</style>
