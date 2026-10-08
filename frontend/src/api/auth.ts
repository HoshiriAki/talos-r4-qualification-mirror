import { requestJson } from './client'

export interface AuthUser {
  id: string
  username: string
  displayName: string
  email: string
  phone: string
  authority: TenantAuthority | PlatformAuthority
  capabilities: PlatformCapability[]
}

export type TenantRole = 'staff' | 'admin' | 'owner'
export type PlatformRole = 'platform_owner' | 'platform_admin' | 'platform_operator' | 'support_engineer' | 'business_operator' | 'security_auditor'
export type PlatformCapability =
  | 'platform_overview_read' | 'platform_health_read' | 'platform_operations_manage'
  | 'tenant_list' | 'tenant_read' | 'tenant_create' | 'tenant_update' | 'tenant_suspend' | 'tenant_delete'
  | 'tenant_governance_read' | 'tenant_governance_manage'
  | 'tenant_preview_create' | 'tenant_preview_read' | 'tenant_diagnostics_read'
  | 'tenant_simulation_create' | 'tenant_simulation_read' | 'tenant_simulation_discard'
  | 'business_metrics_read' | 'billing_read' | 'billing_manage' | 'contract_manage'
  | 'support_case_read' | 'support_case_manage' | 'audit_read' | 'security_events_read'
  | 'platform_identity_manage' | 'platform_role_manage'

export interface TenantAuthority {
  kind: 'tenant'
  membership_id: string
  tenant_id: string
  role: TenantRole
}

export interface PlatformAuthority {
  kind: 'platform'
  membership_id: string
  roles: PlatformRole[]
}

export interface LoginResult {
  ok: boolean
  user: AuthUser
}

export interface UpdateProfileInput {
  displayName?: string
  email?: string
  phone?: string
}

export async function login(username: string, password: string, authority: 'tenant' | 'platform' = 'tenant', totpCode?: string): Promise<LoginResult> {
  return requestJson(authority === 'platform' ? '/auth/platform/login' : '/auth/login', {
    method: 'POST',
    body: JSON.stringify({ username, password, totpCode: totpCode || undefined }),
  }, '登录失败')
}

export async function logout(): Promise<void> {
  await requestJson('/auth/logout', { method: 'POST' }, '退出登录失败')
}

export async function fetchMe(): Promise<{ user: AuthUser }> {
  return requestJson('/auth/me', '登录状态失效')
}

export async function changePassword(oldPassword: string, newPassword: string): Promise<{ ok: boolean }> {
  return requestJson('/auth/change-password', {
    method: 'POST',
    body: JSON.stringify({ oldPassword, newPassword }),
  }, '密码修改失败')
}

export async function updateProfile(input: UpdateProfileInput): Promise<{ ok: boolean; user: AuthUser }> {
  return requestJson('/auth/me', {
    method: 'PUT',
    body: JSON.stringify(input),
  }, '保存资料失败')
}
