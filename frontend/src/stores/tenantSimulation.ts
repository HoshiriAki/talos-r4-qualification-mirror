import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import {
  tenantSimulationApi,
  type CreateTenantSimulationInput,
  type ReferenceConfig,
  type SimulationDiffItem,
  type SimulationEffect,
  type TenantSimulation,
} from '@/api/tenantSimulation'

function idempotencyKey(): string {
  return `${crypto.randomUUID()}-${Date.now().toString(36)}`
}

export const useTenantSimulationStore = defineStore('tenant-simulation', () => {
  const session = ref<TenantSimulation | null>(null)
  const configs = ref<ReferenceConfig[]>([])
  const diffItems = ref<SimulationDiffItem[]>([])
  const effects = ref<SimulationEffect[]>([])
  const loading = ref(false)
  const mutationLoading = ref(false)
  const error = ref<string | null>(null)
  const diffEvaluationId = ref('')
  const diffNextCursor = ref<string | null>(null)
  const effectsNextCursor = ref<string | null>(null)

  const executable = computed(() => session.value?.status === 'active')
  const dirtyCount = computed(() => diffItems.value.filter((item) => item.status !== 'clean').length)
  const conflictCount = computed(() => diffItems.value.filter((item) => item.status === 'conflict').length)

  function clear() {
    session.value = null
    configs.value = []
    diffItems.value = []
    effects.value = []
    diffEvaluationId.value = ''
    diffNextCursor.value = null
    effectsNextCursor.value = null
    error.value = null
  }
  function fail(cause: unknown) {
    error.value = cause instanceof Error ? cause.message : '模拟操作失败'
  }
  async function create(input: CreateTenantSimulationInput) {
    mutationLoading.value = true; error.value = null
    try { const value = await tenantSimulationApi.create(input); session.value = value; return value }
    catch (cause) { fail(cause); throw cause } finally { mutationLoading.value = false }
  }
  async function hydrate(id: string) {
    loading.value = true; error.value = null
    try { session.value = await tenantSimulationApi.get(id); return session.value }
    catch (cause) { fail(cause); throw cause } finally { loading.value = false }
  }
  async function discard() {
    if (!session.value) return
    mutationLoading.value = true
    try { await tenantSimulationApi.discard(session.value.id); session.value = { ...session.value, status: 'discarding' } }
    catch (cause) { fail(cause); throw cause } finally { mutationLoading.value = false }
  }
  async function loadWorkspace() {
    if (!session.value) return
    loading.value = true; error.value = null
    try { await Promise.all([loadConfigs(), loadDiff(), loadEffects()]) }
    catch (cause) { fail(cause); throw cause } finally { loading.value = false }
  }
  async function loadConfigs() { if (session.value) configs.value = await tenantSimulationApi.listReferenceConfigs(session.value.id) }
  async function loadDiff(cursor?: string) {
    if (!session.value) return
    const page = cursor && diffEvaluationId.value
      ? await tenantSimulationApi.diffPage(session.value.id, diffEvaluationId.value, cursor)
      : await tenantSimulationApi.diff(session.value.id)
    diffEvaluationId.value = page.evaluationId || diffEvaluationId.value
    diffItems.value = cursor ? [...diffItems.value, ...page.items] : page.items
    diffNextCursor.value = page.nextCursor ?? null
  }
  async function loadEffects(cursor?: string) {
    if (!session.value) return
    const page = await tenantSimulationApi.effects(session.value.id, cursor)
    effects.value = cursor ? [...effects.value, ...page.items] : page.items
    effectsNextCursor.value = page.nextCursor ?? null
  }
  async function saveConfig(key: string, value: unknown, create = false) {
    if (!session.value) return
    mutationLoading.value = true
    try {
      if (create) await tenantSimulationApi.createReferenceConfig(session.value.id, key, value, idempotencyKey())
      else await tenantSimulationApi.putReferenceConfig(session.value.id, key, value, idempotencyKey())
      await loadWorkspace()
    } catch (cause) { fail(cause); throw cause } finally { mutationLoading.value = false }
  }
  async function deleteConfig(key: string) {
    if (!session.value) return
    mutationLoading.value = true
    try { await tenantSimulationApi.deleteReferenceConfig(session.value.id, key, idempotencyKey()); await loadWorkspace() }
    catch (cause) { fail(cause); throw cause } finally { mutationLoading.value = false }
  }
  async function recordEffect(key: string, value: unknown, note: string) {
    if (!session.value) return
    mutationLoading.value = true
    try { await tenantSimulationApi.recordEffect(session.value.id, key, value, note, idempotencyKey()); await loadWorkspace() }
    catch (cause) { fail(cause); throw cause } finally { mutationLoading.value = false }
  }
  async function exportDiff() {
    if (!session.value || !diffEvaluationId.value) return
    const blob = await tenantSimulationApi.exportDiff(session.value.id, diffEvaluationId.value)
    const url = URL.createObjectURL(blob); const link = document.createElement('a')
    link.href = url; link.download = `simulation-${session.value.id}-diff.json`; link.click(); URL.revokeObjectURL(url)
  }
  return { session, configs, diffItems, effects, loading, mutationLoading, error, executable, dirtyCount, conflictCount, diffEvaluationId, diffNextCursor, effectsNextCursor, clear, create, hydrate, discard, loadWorkspace, loadConfigs, loadDiff, loadEffects, saveConfig, deleteConfig, recordEffect, exportDiff }
})
