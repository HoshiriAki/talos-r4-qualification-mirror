import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { tenantPreviewApi, type TenantPreviewSession } from '@/api/tenant-preview'
import { setPreviewTransportContext } from '@/api/client'

export type PreviewSurface = 'dashboard' | 'orders' | 'devices' | 'audit'

const surfacePaths: Record<PreviewSurface, string> = {
  dashboard: 'dashboard',
  orders: 'orders',
  devices: 'devices',
  audit: 'audit',
}

export const useTenantPreviewStore = defineStore('tenant-preview', () => {
  const session = ref<TenantPreviewSession | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)
  const invalidating = ref(false)

  const active = computed(() => session.value?.status === 'active')
  const sessionId = computed(() => session.value?.id ?? null)

  function routeFor(surface: PreviewSurface): string {
    if (!session.value) return surface === 'dashboard' ? '/app/overview' : `/app/${surface === 'orders' ? 'orders' : surface === 'devices' ? 'devices' : surface}`
    return `/embedded/preview/${encodeURIComponent(session.value.id)}/${surfacePaths[surface]}`
  }

  function clear() {
    session.value = null
    error.value = null
    setPreviewTransportContext(null)
  }

  function activate(value: TenantPreviewSession) {
    session.value = value
    setPreviewTransportContext({
      sessionId: value.id,
      onInvalid: () => {
        if (invalidating.value) return
        invalidating.value = true
        const tenantId = session.value?.tenantId
        void import('@/router').then(({ default: router }) => {
          sessionStorage.setItem('routeGuardMessage', '租户预览会话已过期或已结束')
          return router.replace(tenantId ? `/control/governance/${tenantId}` : '/control/governance')
        }).finally(() => {
          clear()
          invalidating.value = false
        })
      },
    })
  }

  async function start(tenantId: string) {
    loading.value = true
    error.value = null
    try {
      const value = await tenantPreviewApi.create(tenantId)
      activate(value)
      return value
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : '创建预览会话失败'
      throw cause
    } finally {
      loading.value = false
    }
  }

  async function hydrate(id: string) {
    if (session.value?.id === id && active.value) return session.value
    loading.value = true
    error.value = null
    try {
      const value = await tenantPreviewApi.get(id)
      if (value.status !== 'active') throw new Error('预览会话已失效')
      activate(value)
      return value
    } catch (cause) {
      clear()
      error.value = cause instanceof Error ? cause.message : '预览会话已失效'
      throw cause
    } finally {
      loading.value = false
    }
  }

  async function exit() {
    const current = session.value
    if (!current) return
    try {
      await tenantPreviewApi.exit(current.id)
    } finally {
      clear()
    }
  }

  return { session, loading, error, active, sessionId, routeFor, start, hydrate, exit, clear }
})
