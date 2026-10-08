import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { createBaseTableStore } from './baseTable'
import * as auditApi from '@/api/audit'
import type { AuditLog, AuditFilters } from '@/api/audit'
import type { DataQuery, DataPage } from '@/types/data-table'

async function fetchAdapter(query: DataQuery<AuditFilters>): Promise<DataPage<AuditLog>> {
  let offset: number
  let limit: number

  if (query.mode === 'page') {
    limit = query.pageSize ?? 50
    offset = ((query.page ?? 1) - 1) * limit
  } else {
    offset = query.offset ?? 0
    limit = query.limit ?? 50
  }

  const result = await auditApi.fetchLogs({
    ...query.filter,
    offset,
    limit,
  })
  return {
    rows: result.logs || [],
    total: result.total || 0,
    totalPages: Math.ceil((result.total || 0) / limit),
    hasMore: (result.logs?.length || 0) === limit,
  }
}

const base = createBaseTableStore<AuditLog, AuditFilters>(
  'audit-base',
  fetchAdapter,
  { actionType: '', entityType: '', actorUsername: '', keyword: '', date: '' },
  50,
)

export const useAuditStore = defineStore('audit', () => {
  const table = base()
  const initialLoading = ref(true)

  const logs = computed(() => table.rows)

  async function fetchLogs(reset = true) {
    if (reset) {
      table.rows.splice(0, table.rows.length)
      table.hasMore = true
      table.total = 0
    }
    if (!table.hasMore) return

    initialLoading.value = reset
    try {
      await table.fetchMore()
    } finally {
      initialLoading.value = false
    }
  }

  function resetFilters() {
    table.filter.actionType = ''
    table.filter.entityType = ''
    table.filter.actorUsername = ''
    table.filter.keyword = ''
    table.filter.date = ''
  }

  return {
    logs,
    filters: table.filter,
    loading: table.loading,
    hasMore: table.hasMore,
    totalCount: table.total,
    initialLoading,
    error: table.error,
    pagination: table.pagination,
    fetchLogs,
    fetchPage: table.fetchPage,
    resetFilters,
  }
})
