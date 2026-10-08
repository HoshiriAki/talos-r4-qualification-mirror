import { defineStore } from 'pinia'
import { ref, reactive, computed } from 'vue'
import type { DataQuery, DataPage, PageInfo } from '@/types/data-table'

export function createBaseTableStore<T, TFilter extends object = Record<string, unknown>>(
  id: string,
  fetchFn: (query: DataQuery<TFilter>) => Promise<DataPage<T>>,
  defaultFilter: TFilter,
  defaultPageSize = 30,
) {
  return defineStore(id, () => {
    const rows = ref<T[]>([])
    const loading = ref(false)
    const error = ref<string | null>(null)
    const filter = reactive<TFilter>({ ...defaultFilter })
    const page = ref(1)
    const pageSize = ref(defaultPageSize)
    const total = ref(0)
    const totalPages = ref(0)
    const hasMore = ref(false)
    const lastFilterSignature = ref('')

    function getFilterSignature(): string {
      return JSON.stringify({ ...filter, pageSize: pageSize.value })
    }

    async function fetchPage(params?: { page?: number; pageSize?: number }) {
      const newPage = params?.page ?? page.value
      const newPageSize = params?.pageSize ?? pageSize.value

      const sig = getFilterSignature()
      const filterChanged = sig !== lastFilterSignature.value
      const effectivePage = filterChanged ? 1 : newPage

      loading.value = true
      error.value = null

      try {
        const query: DataQuery<TFilter> = {
          mode: 'page',
          page: effectivePage,
          pageSize: newPageSize,
          filter: { ...filter } as TFilter,
        }
        const result = await fetchFn(query)
        rows.value = result.rows as T[]
        total.value = result.total ?? 0
        totalPages.value = result.totalPages ?? 0
        hasMore.value = result.hasMore ?? false
        page.value = effectivePage
        pageSize.value = newPageSize
        lastFilterSignature.value = sig
      } catch (e: any) {
        error.value = e.message || '加载失败'
        throw e
      } finally {
        loading.value = false
      }
    }

    async function fetchMore() {
      loading.value = true
      error.value = null

      try {
        const query: DataQuery<TFilter> = {
          mode: 'offset',
          offset: rows.value.length,
          limit: defaultPageSize,
          filter: { ...filter } as TFilter,
        }
        const result = await fetchFn(query)
        rows.value = [...rows.value, ...result.rows as T[]] as T[]
        hasMore.value = result.hasMore ?? false
        if (result.total != null) total.value = result.total
        if (result.totalPages != null) totalPages.value = result.totalPages
      } catch (e: any) {
        error.value = e.message || '加载失败'
        throw e
      } finally {
        loading.value = false
      }
    }

    function setFilter(partial: Partial<TFilter>) {
      Object.assign(filter, partial)
    }

    function resetFilter() {
      Object.assign(filter, defaultFilter)
    }

    const pagination = computed<PageInfo>(() => ({
      page: page.value,
      pageSize: pageSize.value,
      total: total.value,
      totalPages: totalPages.value,
    }))

    return {
      rows,
      loading,
      error,
      filter,
      page,
      pageSize,
      total,
      totalPages,
      hasMore,
      pagination,
      fetchPage,
      fetchMore,
      setFilter,
      resetFilter,
      getFilterSignature,
    }
  })
}
