import { requestJson } from './client'

export interface TenantMember {
  membershipId: string
  identityId: string
  username: string
  role: 'owner' | 'admin' | 'staff'
  isEnabled: boolean
  createdAt: string
  updatedAt: string
  lastLoginAt?: string | null
}

export async function fetchAll(): Promise<{ memberships: TenantMember[] }> {
  return requestJson('/tenant-memberships', '获取租户成员列表失败')
}

export async function createStaff(data: { username: string; password: string; role: string }): Promise<{ ok: boolean; member: TenantMember }> {
  return requestJson('/tenant-memberships', {
    method: 'POST',
    body: JSON.stringify(data),
  }, '创建员工失败')
}

export async function resetPassword(membershipId: string, newPassword: string): Promise<{ ok: boolean; revokedSessionCount: number }> {
  return requestJson(`/tenant-memberships/${encodeURIComponent(membershipId)}/reset-password`, {
    method: 'POST',
    body: JSON.stringify({ newPassword }),
  }, '重置密码失败')
}

export async function toggleEnabled(membershipId: string, isEnabled: boolean): Promise<{ ok: boolean; member: TenantMember; revokedSessionCount: number }> {
  return requestJson(`/tenant-memberships/${encodeURIComponent(membershipId)}/toggle-enabled`, {
    method: 'POST',
    body: JSON.stringify({ isEnabled }),
  }, '切换状态失败')
}

export async function deleteStaff(membershipId: string): Promise<{ ok: boolean; revokedSessionCount: number }> {
  return requestJson(`/tenant-memberships/${encodeURIComponent(membershipId)}`, {
    method: 'DELETE',
  }, '删除员工失败')
}

export async function updateUsername(membershipId: string, username: string): Promise<{ ok: boolean; member: TenantMember }> {
  return requestJson(`/tenant-memberships/${encodeURIComponent(membershipId)}/username`, {
    method: 'POST',
    body: JSON.stringify({ username }),
  }, '修改用户名失败')
}

export async function transferOwnership(membershipId: string): Promise<{ ok: boolean; ownerMembershipId: string }> {
  return requestJson(`/tenant-memberships/${encodeURIComponent(membershipId)}/transfer-ownership`, {
    method: 'POST',
  }, '转移租户所有权失败')
}
