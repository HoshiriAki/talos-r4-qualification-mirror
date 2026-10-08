import { requestJson } from './client'

export type TenantLifecycleStatus = 'active' | 'suspended' | 'inactive'
export type HealthStatus = 'healthy' | 'degraded' | 'unavailable' | 'unknown'

export interface GovernedTenant {
  id: string
  name: string
  slug: string
  status: TenantLifecycleStatus
  plan: string
  createdAt: string
  updatedAt: string
}

export interface TenantHealthSummary {
  tenantId: string
  status: HealthStatus
  checkedAt: string
  lastActivityAt?: string | null
  lifecycleStatus: TenantLifecycleStatus
  auditEvents24h: number
  failedCommandCount: number
  signals: Array<{ key: string; label: string; status: HealthStatus; detail?: string | null }>
}

export interface GovernanceAuditEvent {
  id: string
  tenantId: string
  tenantName?: string | null
  actorId?: string | null
  actorLabel?: string | null
  command: string
  result: 'attempted' | 'succeeded' | 'failed' | 'denied'
  source?: string | null
  correlationId: string
  changeIntentId?: string | null
  occurredAt: string
  detail?: Record<string, unknown> | null
}

export interface TenantDirectoryQuery {
  search?: string
  status?: TenantLifecycleStatus | ''
  limit?: number
  cursor?: string
}

export interface AuditQuery {
  tenantId?: string
  actor?: string
  command?: string
  result?: GovernanceAuditEvent['result'] | ''
  from?: string
  to?: string
  limit?: number
  cursor?: string
}

export interface TenantDirectoryResult { tenants: GovernedTenant[]; nextCursor?: string | null }
export interface AuditResult { events: GovernanceAuditEvent[]; nextCursor?: string | null }

export interface RecordChangeIntentInput {
  targetTenantId: string
  reason: string
  intendedOutcome: string
  impact?: string
  costMinor?: number
  currency?: string
}

export interface ChangeIntent {
  id: string
  targetTenantId: string
  actorId: string
  source: string
  status: 'recorded'
  correlationId: string
  createdAt: string
  createdBy?: string | null
}

function queryString(values: Record<string, string | number | undefined>): string {
  const params = new URLSearchParams()
  Object.entries(values).forEach(([key, value]) => {
    if (value !== undefined && value !== '') params.set(key, String(value))
  })
  const value = params.toString()
  return value ? `?${value}` : ''
}

export const tenantGovernanceApi = {
  listTenants(query: TenantDirectoryQuery = {}): Promise<TenantDirectoryResult> {
    return requestJson(`/api/tenant-governance/tenants${queryString({ ...query })}`, '获取租户目录失败')
  },

  getTenant(tenantId: string): Promise<GovernedTenant> {
    return requestJson(`/api/tenant-governance/tenants/${encodeURIComponent(tenantId)}`, '获取租户详情失败')
  },

  getTenantHealth(tenantId: string): Promise<TenantHealthSummary> {
    return requestJson(`/api/tenant-governance/tenants/${encodeURIComponent(tenantId)}/health`, '获取租户健康状态失败')
  },

  queryAudit(query: AuditQuery = {}): Promise<AuditResult> {
    return requestJson(`/api/tenant-governance/audit-events${queryString({ ...query })}`, '查询治理审计失败')
  },

  getAuditEvent(eventId: string): Promise<GovernanceAuditEvent> {
    return requestJson(`/api/tenant-governance/audit-events/${encodeURIComponent(eventId)}`, '获取审计事件失败')
  },

  recordChangeIntent(input: RecordChangeIntentInput): Promise<ChangeIntent> {
    return requestJson('/api/tenant-governance/change-intents', {
      method: 'POST',
      body: JSON.stringify(input),
    }, '记录变更意图失败')
  },
}
