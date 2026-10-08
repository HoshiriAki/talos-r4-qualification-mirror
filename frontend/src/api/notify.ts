// Notification API — 站内信 + 通知发送
import { requestJson } from './client'

export interface NotificationMessage {
  id: string
  eventType: string
  channel: 'email' | 'sms' | 'in_app'
  subject: string
  body: string
  status: 'pending' | 'sent' | 'failed'
  readAt: string | null
  createdAt: string
}

export interface NotificationTemplate {
  id: string
  eventType: string
  channel: string
  subjectTemplate: string
  bodyTemplate: string
  isEnabled: boolean
  createdAt: string
  updatedAt: string
}

export interface NotificationListResponse {
  ok: boolean
  messages: NotificationMessage[]
  unreadCount: number
  pagination: {
    page: number
    pageSize: number
    total: number
  }
}

export async function sendNotification(params: {
  eventType: string
  channel?: string
  recipient?: string
  variables?: Record<string, unknown>
}): Promise<{ ok: boolean; messageId: string; status: string }> {
  return requestJson('/api/notify/send', {
    method: 'POST',
    body: JSON.stringify({
      eventType: params.eventType,
      channel: params.channel,
      recipient: params.recipient,
      variables: params.variables,
    }),
  })
}

export async function listNotifications(params?: {
  page?: number
  pageSize?: number
}): Promise<NotificationListResponse> {
  return requestJson('/api/notify/list', {
    method: 'POST',
    body: JSON.stringify({
      page: params?.page || 1,
      pageSize: params?.pageSize || 20,
    }),
  })
}

export async function markRead(messageId?: string): Promise<{ ok: boolean; affected: number }> {
  return requestJson('/api/notify/mark-read', {
    method: 'POST',
    body: JSON.stringify({
      messageId: messageId || null,
    }),
  })
}

export async function getTemplates(): Promise<{ ok: boolean; templates: NotificationTemplate[] }> {
  return requestJson('/api/notify/templates', {
    method: 'POST',
    body: JSON.stringify({ action: 'list' }),
  })
}

export async function upsertTemplate(params: {
  id?: string
  eventType: string
  channel: string
  subjectTemplate: string
  bodyTemplate: string
  isEnabled?: boolean
}): Promise<{ ok: boolean; id?: string; created?: boolean; updated?: boolean }> {
  return requestJson('/api/notify/templates/upsert', {
    method: 'POST',
    body: JSON.stringify({
      id: params.id,
      eventType: params.eventType,
      channel: params.channel,
      subjectTemplate: params.subjectTemplate,
      bodyTemplate: params.bodyTemplate,
      isEnabled: params.isEnabled,
    }),
  })
}

export async function triggerNotification(params: {
  eventType: string
  orderId?: string
  variables?: Record<string, unknown>
}): Promise<{ ok: boolean; results: unknown[] }> {
  return requestJson('/api/notify/trigger', {
    method: 'POST',
    body: JSON.stringify({
      eventType: params.eventType,
      orderId: params.orderId,
      variables: params.variables,
    }),
  })
}
