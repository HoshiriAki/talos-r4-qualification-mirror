import { requestJson } from './client'

export type TenantPreviewStatus = 'active' | 'ended'

export interface TenantPreviewSession {
  id: string
  tenantId: string
  tenantName: string
  tenantSlug: string
  status: TenantPreviewStatus
  expiresAt: string
  mode: 'preview' | 'diagnostics' | 'simulation'
  simulationId?: string | null
  capabilities: string[]
}

export interface CreateTenantWorkspaceInput {
  tenantId: string
  mode?: TenantPreviewSession['mode']
  simulationId?: string
  ttlMinutes?: number
}

function unwrapSession(value: any): TenantPreviewSession {
  const session = value?.session ?? value
  return {
    ...session,
    id: session.id ?? session.sessionId,
  } as TenantPreviewSession
}

export const tenantPreviewApi = {
  async create(
    tenantOrInput: string | CreateTenantWorkspaceInput,
  ): Promise<TenantPreviewSession> {
    const input = typeof tenantOrInput === 'string'
      ? { tenantId: tenantOrInput, mode: 'preview' as const }
      : tenantOrInput
    const value = await requestJson('/api/tenant-workspaces/sessions', {
      method: 'POST',
      body: JSON.stringify(input),
    }, '创建租户预览会话失败')
    return unwrapSession(value)
  },

  async get(sessionId: string): Promise<TenantPreviewSession> {
    const value = await requestJson(
      `/api/tenant-workspaces/sessions/${encodeURIComponent(sessionId)}`,
      '获取租户预览会话失败',
    )
    return unwrapSession(value)
  },

  async exit(sessionId: string): Promise<void> {
    await requestJson(`/api/tenant-workspaces/sessions/${encodeURIComponent(sessionId)}`, {
      method: 'DELETE',
    }, '退出租户预览失败')
  },
}
