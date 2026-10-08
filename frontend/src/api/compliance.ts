// Compliance API — privacy consent + data deletion + 2FA
import { requestJson } from './client'

// ── Consent ────────────────────────────────────────────────────────

export async function recordConsent(params: {
  consentType: string
  version: string
  ipAddress: string
  userAgent: string
}): Promise<{ ok: boolean; id: string; createdAt: string }> {
  return requestJson('/api/compliance/consent/record', {
    method: 'POST',
    body: JSON.stringify(params),
  })
}

export async function checkConsent(params: {
  consentType: string
  version: string
}): Promise<{ ok: boolean; hasConsented: boolean }> {
  return requestJson('/api/compliance/consent/check', {
    method: 'POST',
    body: JSON.stringify(params),
  })
}

export async function revokeConsent(params: {
  consentType: string
}): Promise<{ ok: boolean; affected: number; revokedAt: string }> {
  return requestJson('/api/compliance/consent/revoke', {
    method: 'POST',
    body: JSON.stringify(params),
  })
}

export async function auditConsent(userId: string): Promise<{
  ok: boolean
  records: Array<{
    id: string; userId: string; consentType: string; version: string
    consented: boolean; ipAddress: string; userAgent: string
    revokedAt: string | null; createdAt: string
  }>
}> {
  return requestJson('/api/compliance/consent/audit', {
    method: 'POST',
    body: JSON.stringify({ userId }),
  })
}

// ── Deletion ───────────────────────────────────────────────────────

export async function requestDataDeletion(params: {
  requestType: string
  reason: string
}): Promise<{ ok: boolean; id: string; status: string; requestedAt: string }> {
  return requestJson('/api/compliance/deletion/request', {
    method: 'POST',
    body: JSON.stringify(params),
  })
}

export async function listDeletionRequests(params?: {
  status?: string
}): Promise<{
  ok: boolean
  requests: Array<{
    id: string; userId: string; requestType: string; status: string
    reason: string; requestedAt: string; completedAt: string | null
    adminNotes: string; createdAt: string
  }>
}> {
  return requestJson('/api/compliance/deletion/list', {
    method: 'POST',
    body: JSON.stringify(params || {}),
  })
}

export async function processDeletion(params: {
  requestId: string
  adminNotes: string
}): Promise<{ ok: boolean; id: string; status: string }> {
  return requestJson('/api/compliance/deletion/process', {
    method: 'POST',
    body: JSON.stringify(params),
  })
}

export async function completeDeletion(params: {
  requestId: string
  adminNotes: string
}): Promise<{ ok: boolean; id: string; status: string; completedAt: string; anonymizedUserId: string }> {
  return requestJson('/api/compliance/deletion/complete', {
    method: 'POST',
    body: JSON.stringify(params),
  })
}

export async function rejectDeletion(params: {
  requestId: string
  adminNotes: string
}): Promise<{ ok: boolean; id: string; status: string; rejectedAt: string }> {
  return requestJson('/api/compliance/deletion/reject', {
    method: 'POST',
    body: JSON.stringify(params),
  })
}

// ── 2FA ────────────────────────────────────────────────────────────

export async function generateTwoFaSecret(): Promise<{
  ok: boolean; secret: string; otpauthUrl: string; message: string
}> {
  return requestJson('/api/compliance/2fa/generate', {
    method: 'POST',
    body: JSON.stringify({}),
  })
}

export async function verifyAndEnableTwoFa(params: {
  code: string
}): Promise<{ ok: boolean; enabled: boolean; message: string }> {
  return requestJson('/api/compliance/2fa/verify_enable', {
    method: 'POST',
    body: JSON.stringify(params),
  })
}

export async function disableTwoFa(userId: string): Promise<{
  ok: boolean; enabled: boolean; message: string
}> {
  return requestJson('/api/compliance/2fa/disable', {
    method: 'POST',
    body: JSON.stringify({ userId }),
  })
}

export async function getTwoFaStatus(): Promise<{
  ok: boolean; enabled: boolean; hasSecret: boolean
}> {
  return requestJson('/api/compliance/2fa/status', {
    method: 'POST',
    body: JSON.stringify({}),
  })
}
