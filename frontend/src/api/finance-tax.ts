// Finance Tax API — 发票 + 结算 + 税务
import { requestJson } from './client'

// ── Invoice ───────────────────────────────────────────────────────

export interface InvoiceInfo {
  id: string
  orderId: string
  invoiceNo: string
  type: '普通发票' | '专用发票'
  amount: number
  taxRate: number
  taxAmount: number
  status: 'issued' | 'voided' | 'red_flushed'
  issuedAt: string
  voidedAt: string | null
  createdAt: string
}

export async function issueInvoice(orderId: string, amount: number, opts?: {
  invoiceType?: string
  taxRate?: number
  tenantId?: string
}): Promise<{
  ok: boolean
  invoiceId: string
  invoiceNo: string
  orderId: string
  amount: number
  taxRate: number
  taxAmount: number
  status: string
}> {
  return requestJson('/api/finance/invoice/issue', {
    method: 'POST',
    body: JSON.stringify({
      orderId,
      amount,
      invoiceType: opts?.invoiceType || '普通发票',
      taxRate: opts?.taxRate,
      tenantId: opts?.tenantId || '',
    }),
  })
}

export async function voidInvoice(invoiceId: string): Promise<{
  ok: boolean
  invoiceId: string
  status: string
}> {
  return requestJson('/api/finance/invoice/void', {
    method: 'POST',
    body: JSON.stringify({ invoiceId }),
  })
}

export async function redFlushInvoice(invoiceId: string, reason?: string): Promise<{
  ok: boolean
  originalInvoiceId: string
  redInvoiceId: string
  redInvoiceNo: string
  redAmount: number
  status: string
}> {
  return requestJson('/api/finance/invoice/red_flush', {
    method: 'POST',
    body: JSON.stringify({ invoiceId, reason: reason || '' }),
  })
}

export async function getInvoice(params?: { invoiceId?: string; orderId?: string }): Promise<{
  ok: boolean
  invoice: InvoiceInfo | null
}> {
  return requestJson('/api/finance/invoice/get', {
    method: 'POST',
    body: JSON.stringify({
      invoiceId: params?.invoiceId,
      orderId: params?.orderId,
    }),
  })
}

export async function listInvoices(params?: {
  status?: string
  dateFrom?: string
  dateTo?: string
  page?: number
  pageSize?: number
}): Promise<{
  ok: boolean
  invoices: InvoiceInfo[]
  pagination: { page: number; pageSize: number; total: number }
}> {
  return requestJson('/api/finance/invoice/list', {
    method: 'POST',
    body: JSON.stringify({
      status: params?.status,
      dateFrom: params?.dateFrom,
      dateTo: params?.dateTo,
      page: params?.page,
      pageSize: params?.pageSize,
    }),
  })
}

// ── Settlement ────────────────────────────────────────────────────

export interface SettlementInfo {
  id: string
  periodType: 'daily' | 'weekly' | 'monthly'
  periodKey: string
  totalRevenue: number
  totalDeposits: number
  totalRefunds: number
  confirmed: boolean
  confirmedAt: string | null
  tenantId: string
  createdAt: string
}

export async function generateSettlement(periodType: string, opts?: {
  periodKey?: string
  tenantId?: string
}): Promise<{
  ok: boolean
  settlementId: string
  periodType: string
  periodKey: string
  totalRevenue: number
  totalDeposits: number
  totalRefunds: number
  confirmed: boolean
}> {
  return requestJson('/api/finance/settlement/generate', {
    method: 'POST',
    body: JSON.stringify({
      periodType,
      periodKey: opts?.periodKey || '',
      tenantId: opts?.tenantId || '',
    }),
  })
}

export async function confirmSettlement(settlementId: string): Promise<{
  ok: boolean
  settlementId: string
  confirmed: boolean
}> {
  return requestJson('/api/finance/settlement/confirm', {
    method: 'POST',
    body: JSON.stringify({ settlementId }),
  })
}

export async function getSettlement(periodType: string, periodKey: string): Promise<{
  ok: boolean
  settlement: SettlementInfo | null
}> {
  return requestJson('/api/finance/settlement/get', {
    method: 'POST',
    body: JSON.stringify({ periodType, periodKey }),
  })
}

export async function listSettlements(params?: {
  periodType?: string
  confirmed?: boolean
  page?: number
  pageSize?: number
}): Promise<{
  ok: boolean
  settlements: SettlementInfo[]
  pagination: { page: number; pageSize: number; total: number }
}> {
  return requestJson('/api/finance/settlement/list', {
    method: 'POST',
    body: JSON.stringify({
      periodType: params?.periodType,
      confirmed: params?.confirmed,
      page: params?.page,
      pageSize: params?.pageSize,
    }),
  })
}

export async function exportSettlementCsv(settlementId: string): Promise<{
  ok: boolean
  csv: string
}> {
  return requestJson('/api/finance/settlement/export', {
    method: 'POST',
    body: JSON.stringify({ settlementId }),
  })
}

// ── Tax ───────────────────────────────────────────────────────────

export interface TaxConfigInfo {
  id: string
  taxType: string
  rate: number
  effectiveFrom: string
  isActive: boolean
  createdAt: string
}

export async function getTaxConfig(taxType?: string): Promise<{
  ok: boolean
  configs: TaxConfigInfo[]
}> {
  return requestJson('/api/finance/tax/config', {
    method: 'POST',
    body: JSON.stringify({ taxType: taxType || 'vat' }),
  })
}

export async function upsertTaxConfig(rate: number, opts?: {
  taxType?: string
  effectiveFrom?: string
}): Promise<{
  ok: boolean
  configId: string
  taxType: string
  rate: number
  effectiveFrom: string
  isActive: boolean
}> {
  return requestJson('/api/finance/tax/upsert_config', {
    method: 'POST',
    body: JSON.stringify({
      taxType: opts?.taxType || 'vat',
      rate,
      effectiveFrom: opts?.effectiveFrom || '',
    }),
  })
}

export async function calculateTax(amount: number, taxRate?: number): Promise<{
  ok: boolean
  totalAmount: number
  preTaxAmount: number
  taxAmount: number
  taxRate: number
}> {
  return requestJson('/api/finance/tax/calculate', {
    method: 'POST',
    body: JSON.stringify({ amount, taxRate }),
  })
}

export async function exportTaxCsv(periodKey?: string): Promise<{
  ok: boolean
  csv: string
  period: string
  invoiceCount: number
  totalTax: number
  totalAmount: number
}> {
  return requestJson('/api/finance/tax/export', {
    method: 'POST',
    body: JSON.stringify({ periodKey: periodKey || '' }),
  })
}
