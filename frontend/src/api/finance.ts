// Finance API — 押金 + 退款
import { requestJson } from './client'

// ── Deposit ──────────────────────────────────────────────────────

export interface DepositInfo {
  id: string
  orderId: string
  amount: number
  status: 'pending' | 'paid' | 'released' | 'forfeited' | 'partially_forfeited'
  paidAt: string | null
  releasedAt: string | null
  forfeitedAt: string | null
  createdAt: string
  updatedAt: string
}

export interface LedgerEntry {
  entryType: 'collect' | 'release' | 'forfeit' | 'refund'
  amount: number
  balanceAfter: number
  description: string
  operator: string
  createdAt: string
}

export interface DepositResponse {
  ok: boolean
  deposit: DepositInfo | null
  ledger: LedgerEntry[]
}

export async function getDeposit(orderId: string): Promise<DepositResponse> {
  return requestJson('/api/finance/deposit/get', {
    method: 'POST',
    body: JSON.stringify({ orderId }),
  })
}

export async function collectDeposit(orderId: string, amount: number): Promise<any> {
  return requestJson('/api/finance/deposit/collect', {
    method: 'POST',
    body: JSON.stringify({ orderId, amount }),
  })
}

export async function releaseDeposit(orderId: string, reason?: string): Promise<any> {
  return requestJson('/api/finance/deposit/release', {
    method: 'POST',
    body: JSON.stringify({ orderId, reason: reason || '' }),
  })
}

export async function forfeitDeposit(orderId: string, amount: number, reason: string): Promise<any> {
  return requestJson('/api/finance/deposit/forfeit', {
    method: 'POST',
    body: JSON.stringify({ orderId, amount, reason }),
  })
}

// ── Refund ───────────────────────────────────────────────────────

export interface RefundInfo {
  id: string
  depositId: string
  orderId: string
  amount: number
  reason: string
  status: 'pending' | 'approved' | 'rejected' | 'executed'
  requestedBy: string
  approvedBy: string | null
  rejectedBy: string | null
  executedBy: string | null
  approvedAt: string | null
  rejectedAt: string | null
  executedAt: string | null
  createdAt: string
}

export async function requestRefund(orderId: string, amount: number, reason?: string): Promise<any> {
  return requestJson('/api/finance/refund/request', {
    method: 'POST',
    body: JSON.stringify({ orderId, amount, reason: reason || '' }),
  })
}

export async function approveRefund(refundId: string): Promise<any> {
  return requestJson('/api/finance/refund/approve', {
    method: 'POST',
    body: JSON.stringify({ refundId }),
  })
}

export async function rejectRefund(refundId: string, reason?: string): Promise<any> {
  return requestJson('/api/finance/refund/reject', {
    method: 'POST',
    body: JSON.stringify({ refundId, reason: reason || '' }),
  })
}

export async function executeRefund(refundId: string): Promise<any> {
  return requestJson('/api/finance/refund/execute', {
    method: 'POST',
    body: JSON.stringify({ refundId }),
  })
}

export async function listRefunds(params?: { orderId?: string; status?: string; page?: number; pageSize?: number }): Promise<{
  ok: boolean
  refunds: RefundInfo[]
  pagination: { page: number; pageSize: number; total: number }
}> {
  return requestJson('/api/finance/refund/list', {
    method: 'POST',
    body: JSON.stringify({
      orderId: params?.orderId,
      status: params?.status,
      page: params?.page,
      pageSize: params?.pageSize,
    }),
  })
}
