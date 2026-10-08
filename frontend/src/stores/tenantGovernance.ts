import { computed, reactive, ref } from 'vue'
import { defineStore } from 'pinia'
import {
  tenantGovernanceApi,
  type AuditQuery,
  type ChangeIntent,
  type GovernedTenant,
  type GovernanceAuditEvent,
  type RecordChangeIntentInput,
  type TenantDirectoryQuery,
  type TenantHealthSummary,
} from '@/api/tenant-governance'

export const useTenantGovernanceStore = defineStore('tenant-governance', () => {
  const tenants = ref<GovernedTenant[]>([])
  const directoryNextCursor = ref<string | null>(null)
  const selectedTenant = ref<GovernedTenant | null>(null)
  const health = ref<TenantHealthSummary | null>(null)
  const auditEvents = ref<GovernanceAuditEvent[]>([])
  const auditNextCursor = ref<string | null>(null)
  const lastIntent = ref<ChangeIntent | null>(null)
  const directoryQuery = reactive<TenantDirectoryQuery>({ search: '', status: '', limit: 25 })
  const auditQuery = reactive<AuditQuery>({ tenantId: '', actor: '', command: '', result: '', limit: 25 })
  const loading = reactive({ directory: false, detail: false, health: false, audit: false, intent: false })
  const errors = reactive<Record<keyof typeof loading, string | null>>({
    directory: null, detail: null, health: null, audit: null, intent: null,
  })

  const activeCount = computed(() => tenants.value.filter(tenant => tenant.status === 'active').length)

  function message(error: unknown): string {
    return error instanceof Error ? error.message : '请求失败'
  }

  async function fetchDirectory() {
    loading.directory = true
    errors.directory = null
    try {
      const page = await tenantGovernanceApi.listTenants(directoryQuery)
      tenants.value = page.tenants
      directoryNextCursor.value = page.nextCursor || null
    } catch (error) {
      errors.directory = message(error)
      throw error
    } finally {
      loading.directory = false
    }
  }

  async function fetchTenant(tenantId: string) {
    loading.detail = true
    errors.detail = null
    try {
      selectedTenant.value = await tenantGovernanceApi.getTenant(tenantId)
      return selectedTenant.value
    } catch (error) {
      errors.detail = message(error)
      throw error
    } finally {
      loading.detail = false
    }
  }

  async function fetchHealth(tenantId: string) {
    loading.health = true
    errors.health = null
    try {
      health.value = await tenantGovernanceApi.getTenantHealth(tenantId)
      return health.value
    } catch (error) {
      errors.health = message(error)
      throw error
    } finally {
      loading.health = false
    }
  }

  async function fetchAudit(overrides: Partial<AuditQuery> = {}) {
    Object.assign(auditQuery, overrides)
    loading.audit = true
    errors.audit = null
    try {
      const page = await tenantGovernanceApi.queryAudit(auditQuery)
      auditEvents.value = page.events
      auditNextCursor.value = page.nextCursor || null
    } catch (error) {
      errors.audit = message(error)
      throw error
    } finally {
      loading.audit = false
    }
  }

  async function recordIntent(input: RecordChangeIntentInput) {
    loading.intent = true
    errors.intent = null
    try {
      lastIntent.value = await tenantGovernanceApi.recordChangeIntent(input)
      return lastIntent.value
    } catch (error) {
      errors.intent = message(error)
      throw error
    } finally {
      loading.intent = false
    }
  }

  function resetSelection() {
    selectedTenant.value = null
    health.value = null
    auditEvents.value = []
    auditNextCursor.value = null
    lastIntent.value = null
  }

  return {
    tenants, directoryNextCursor, selectedTenant, health, auditEvents, auditNextCursor, lastIntent,
    directoryQuery, auditQuery, loading, errors, activeCount,
    fetchDirectory, fetchTenant, fetchHealth, fetchAudit, recordIntent, resetSelection,
  }
})
