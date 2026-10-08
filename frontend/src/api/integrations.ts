import { requestJson } from './client'

export type ProviderReadiness = 'stub' | 'fixture' | 'sandbox' | 'production'
export type ProviderLifecycle = 'draft' | 'active' | 'suspended' | 'retired'
export type ProviderHealth = 'unknown' | 'ready' | 'degraded' | 'unhealthy'

export interface ProviderInstanceInspection {
  providerInstanceId: string
  providerId: string
  manifestVersion: string
  configRevision: string
  configKeys: string[]
  configuredSecretNames: string[]
  lifecycle: ProviderLifecycle
  readiness: ProviderReadiness
  health: ProviderHealth
}

export interface ProviderBindingSummary {
  providerBindingId: string
  providerInstanceId: string
  capability: string
  bindingRevision: string
  enabled: boolean
}

export interface BindingInspection extends ProviderBindingSummary {
  providerId: string
  manifestVersion: string
  readiness: ProviderReadiness
  health: ProviderHealth
  lifecycle: ProviderLifecycle
}

export interface WebhookEndpointInspection {
  webhookEndpointId: string
  providerBindingId: string
  providerId: string
  enabled: boolean
  createdAt: string
}

export interface WebhookDeadLetterInspection {
  inboxId: string
  webhookEndpointId: string
  providerEventId: string
  reason: string
  replayCount: number
  createdAt: string
  replayedAt: string | null
}

export interface UpsertProviderInstanceInput {
  id: string
  providerId: string
  manifestVersion: string
  configRevision: string
  config: Record<string, string>
  secretRefs: Record<string, string>
  lifecycle: ProviderLifecycle
  health: ProviderHealth
  readiness: ProviderReadiness
}

export interface UpsertProviderBindingInput {
  id: string
  providerInstanceId: string
  capability: string
  configRevision: string
  enabled: boolean
}

export const integrationApi = {
  listInstances(): Promise<{ instances: ProviderInstanceInspection[] }> {
    return requestJson('/api/integrations/instances', '获取 Provider 实例失败')
  },

  listBindings(): Promise<{ bindings: ProviderBindingSummary[] }> {
    return requestJson('/api/integrations/bindings', '获取 Provider 绑定失败')
  },

  upsertInstance(input: UpsertProviderInstanceInput): Promise<{ registered: boolean }> {
    return requestJson('/api/integrations/instances', {
      method: 'POST',
      body: JSON.stringify(input),
    }, '保存 Provider 实例失败')
  },

  upsertBinding(input: UpsertProviderBindingInput): Promise<{ enabled: boolean }> {
    return requestJson('/api/integrations/bindings', {
      method: 'POST',
      body: JSON.stringify(input),
    }, '保存 Provider 绑定失败')
  },

  inspectBinding(capability: string): Promise<BindingInspection> {
    return requestJson(
      `/api/integrations/bindings/${encodeURIComponent(capability)}`,
      '解析 Provider 绑定失败',
    )
  },

  listWebhookEndpoints(): Promise<{ webhookEndpoints: WebhookEndpointInspection[] }> {
    return requestJson('/api/integrations/webhook-endpoints', '获取 Webhook 端点失败')
  },

  createWebhookEndpoint(bindingId: string): Promise<{ webhookEndpointId: string; endpointToken: string }> {
    return requestJson('/api/integrations/webhook-endpoints', {
      method: 'POST',
      body: JSON.stringify({ bindingId }),
    }, '创建 Webhook 端点失败')
  },

  setWebhookEndpointEnabled(endpointId: string, enabled: boolean): Promise<{ enabled: boolean }> {
    return requestJson(`/api/integrations/webhook-endpoints/${encodeURIComponent(endpointId)}/enabled`, {
      method: 'POST',
      body: JSON.stringify({ enabled }),
    }, '更新 Webhook 端点失败')
  },

  listWebhookDeadLetters(): Promise<{ webhookDeadLetters: WebhookDeadLetterInspection[] }> {
    return requestJson('/api/integrations/webhook-dead-letters', '获取 Webhook 死信失败')
  },

  replayWebhook(inboxId: string, reason: string): Promise<{ replayed: boolean }> {
    return requestJson(`/api/integrations/webhook-dead-letters/${encodeURIComponent(inboxId)}/replay`, {
      method: 'POST',
      body: JSON.stringify({ reason }),
    }, '回放 Webhook 死信失败')
  },
}
