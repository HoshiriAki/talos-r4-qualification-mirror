/**
 * 租户管理 Store
 *
 * 管理租户列表、当前租户、CRUD 操作
 */

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { tenantApi, type Tenant, type CreateTenantInput, type UpdateTenantInput } from '@/api/tenant'

export const useTenantStore = defineStore('tenant', () => {
  // ── State ──
  const tenants = ref<Tenant[]>([])
  const currentTenant = ref<Tenant | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)

  // ── Computed ──
  const activeTenants = computed(() =>
    tenants.value.filter(t => t.status === 'active')
  )

  const suspendedTenants = computed(() =>
    tenants.value.filter(t => t.status === 'suspended')
  )

  const inactiveTenants = computed(() =>
    tenants.value.filter(t => t.status === 'inactive')
  )

  const totalCount = computed(() => tenants.value.length)

  // ── Actions ──

  /**
   * 加载所有租户列表
   */
  async function fetchTenants() {
    loading.value = true
    error.value = null

    try {
      const response = await tenantApi.list()
      tenants.value = response.tenants
    } catch (e: any) {
      error.value = e.response?.data?.error || e.message || '加载租户列表失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  /**
   * 获取单个租户详情
   */
  async function fetchTenant(tenantId: string) {
    loading.value = true
    error.value = null

    try {
      const tenant = await tenantApi.get(tenantId)
      currentTenant.value = tenant

      // 更新列表中的租户信息
      const index = tenants.value.findIndex(t => t.id === tenantId)
      if (index !== -1) {
        tenants.value[index] = tenant
      }

      return tenant
    } catch (e: any) {
      error.value = e.response?.data?.error || e.message || '加载租户详情失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  /**
   * 创建新租户
   */
  async function createTenant(input: CreateTenantInput) {
    loading.value = true
    error.value = null

    try {
      const tenant = await tenantApi.create(input)
      tenants.value.unshift(tenant) // 添加到列表开头
      return tenant
    } catch (e: any) {
      error.value = e.response?.data?.error || e.message || '创建租户失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  /**
   * 更新租户信息
   */
  async function updateTenant(tenantId: string, input: UpdateTenantInput) {
    loading.value = true
    error.value = null

    try {
      const tenant = await tenantApi.update(tenantId, input)

      // 更新列表中的租户
      const index = tenants.value.findIndex(t => t.id === tenantId)
      if (index !== -1) {
        tenants.value[index] = tenant
      }

      // 如果是当前租户，也更新
      if (currentTenant.value?.id === tenantId) {
        currentTenant.value = tenant
      }

      return tenant
    } catch (e: any) {
      error.value = e.response?.data?.error || e.message || '更新租户失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  /**
   * 更新租户状态
   */
  async function updateTenantStatus(tenantId: string, status: 'active' | 'suspended' | 'inactive') {
    loading.value = true
    error.value = null

    try {
      const tenant = await tenantApi.updateStatus(tenantId, status)

      // 更新列表中的租户
      const index = tenants.value.findIndex(t => t.id === tenantId)
      if (index !== -1) {
        tenants.value[index] = tenant
      }

      // 如果是当前租户，也更新
      if (currentTenant.value?.id === tenantId) {
        currentTenant.value = tenant
      }

      return tenant
    } catch (e: any) {
      error.value = e.response?.data?.error || e.message || '更新租户状态失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  /**
   * 根据 slug 查找租户
   */
  function findBySlug(slug: string): Tenant | undefined {
    return tenants.value.find(t => t.slug === slug)
  }

  /**
   * 根据 ID 查找租户
   */
  function findById(id: string): Tenant | undefined {
    return tenants.value.find(t => t.id === id)
  }

  /**
   * 清空错误信息
   */
  function clearError() {
    error.value = null
  }

  /**
   * 重置 Store
   */
  function $reset() {
    tenants.value = []
    currentTenant.value = null
    loading.value = false
    error.value = null
  }

  return {
    // State
    tenants,
    currentTenant,
    loading,
    error,

    // Computed
    activeTenants,
    suspendedTenants,
    inactiveTenants,
    totalCount,

    // Actions
    fetchTenants,
    fetchTenant,
    createTenant,
    updateTenant,
    updateTenantStatus,
    findBySlug,
    findById,
    clearError,
    $reset,
  }
})
