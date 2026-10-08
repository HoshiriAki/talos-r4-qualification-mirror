/**
 * 租户 API 客户端
 *
 * 提供租户管理的 API 调用方法
 */

import { requestJson } from './client'

export interface Tenant {
  id: string
  name: string
  slug: string
  status: 'active' | 'suspended' | 'inactive' | 'deleted'
  plan: string  // 'free' | 'pro' | 'enterprise'
  settings?: string // JSON string
  createdAt: string
  updatedAt: string
}

export interface CreateTenantInput {
  name: string
  slug: string
  settings?: Record<string, any>
}

export interface UpdateTenantInput {
  name?: string
  slug?: string
  settings?: Record<string, any>
}

export interface TenantListResponse {
  tenants: Tenant[]
  total: number
}

/**
 * 租户 API
 */
export const tenantApi = {
  /**
   * 列出所有租户（platform capability required）
   */
  async list(): Promise<TenantListResponse> {
    return requestJson('/api/tenants', '获取租户列表失败')
  },

  /**
   * 获取单个租户详情（platform capability required）
   */
  async get(tenantId: string): Promise<Tenant> {
    return requestJson(`/api/tenants/${tenantId}`, '获取租户详情失败')
  },

  /**
   * 创建新租户（platform capability required）
   */
  async create(input: CreateTenantInput): Promise<Tenant> {
    return requestJson('/api/tenants', {
      method: 'POST',
      body: JSON.stringify(input),
    }, '创建租户失败')
  },

  /**
   * 更新租户信息（platform capability required）
   */
  async update(tenantId: string, input: UpdateTenantInput): Promise<Tenant> {
    return requestJson(`/api/tenants/${tenantId}`, {
      method: 'PUT',
      body: JSON.stringify(input),
    }, '更新租户失败')
  },

  /**
   * 更新租户状态（platform capability required）
   */
  async updateStatus(tenantId: string, status: 'active' | 'suspended' | 'inactive'): Promise<Tenant> {
    return requestJson(`/api/tenants/${tenantId}/status`, {
      method: 'PATCH',
      body: JSON.stringify({ status }),
    }, '更新租户状态失败')
  },

  /**
   * 删除租户（platform capability required）
   */
  async delete(tenantId: string): Promise<void> {
    await requestJson(`/api/tenants/${tenantId}`, {
      method: 'DELETE',
    }, '删除租户失败')
  },
}
