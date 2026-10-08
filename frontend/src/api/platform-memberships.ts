import { requestJson } from './client'
import type { PlatformRole } from './auth'

export interface PlatformMembership {
  id: string
  identityId: string
  username: string
  displayName: string
  email: string
  status: 'active' | 'suspended' | 'revoked'
  roles: PlatformRole[]
  createdAt: string
  updatedAt: string
}

export interface CreatePlatformMembershipInput {
  identityId?: string
  username?: string
  password?: string
  displayName?: string
  email?: string
  phone?: string
  roles: PlatformRole[]
}

export const platformMembershipApi = {
  async list(): Promise<PlatformMembership[]> {
    const data = await requestJson('/api/platform/memberships', '加载平台成员失败')
    return data.memberships ?? []
  },
  create(input: CreatePlatformMembershipInput) {
    return requestJson('/api/platform/memberships', {
      method: 'POST',
      body: JSON.stringify(input),
    }, '创建平台成员失败')
  },
  update(id: string, input: { status?: PlatformMembership['status']; roles?: PlatformRole[] }) {
    return requestJson(`/api/platform/memberships/${encodeURIComponent(id)}`, {
      method: 'PUT',
      body: JSON.stringify(input),
    }, '更新平台成员失败')
  },
  revoke(id: string) {
    return requestJson(`/api/platform/memberships/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    }, '撤销平台成员失败')
  },
}
