import { requestJson } from './client'

export interface AuditFilters {
  actionType?: string
  entityType?: string
  actorUsername?: string
  keyword?: string
  date?: string
  limit?: number
  offset?: number
  sortBy?: string | null
  sortOrder?: string
}

export interface AuditLog {
  id: string
  actionType: string
  entityType: string
  entityId: string
  entityLabel: string
  detail: any
  detailJson?: string
  actorIdentityId?: string
  actorUsername?: string
  ip?: string
  userAgent?: string
  createdAt: string
}

export async function fetchLogs(filters: AuditFilters = {}): Promise<{ logs: AuditLog[]; limit: number; total: number }> {
  const params = new URLSearchParams()
  const limit = filters.limit || 50

  Object.entries(filters).forEach(([key, value]) => {
    if (value !== undefined && value !== '' && value !== null && key !== 'limit' && key !== 'total') {
      params.set(key, String(value))
    }
  })
  params.set('limit', String(limit))

  return requestJson(`/audit-logs?${params.toString()}`, '获取操作日志失败')
}
