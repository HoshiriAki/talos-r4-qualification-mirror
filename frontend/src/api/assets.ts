// Assets API — 设备全生命周期管理 (采购/折旧/ROA/维修聚合)
import { requestJson } from './client'

// ── Types ─────────────────────────────────────────────────────────

export interface PurchaseInfo {
  id: string
  deviceSerialNo: string
  purchasePrice: number
  purchaseDate: string
  vendor: string
  invoiceNo: string
  replacementValue: number
  notes: string
  createdAt: string
}

export interface DepreciationEntry {
  id: string
  deviceSerialNo: string
  period: string
  openingValue: number
  depreciationAmount: number
  closingValue: number
  method: string
  createdAt: string
}

export interface DepreciationSummary {
  totalDepreciation: number
  months: number
}

export interface DepreciationResponse {
  ok: boolean
  deviceSerialNo: string
  depreciationLog: DepreciationEntry[]
  summary: DepreciationSummary
}

export interface NetBookValue {
  ok: boolean
  deviceSerialNo: string
  purchasePrice: number
  totalDepreciation: number
  netBookValue: number
  monthlyDepreciation: number
  usefulLifeMonths: number
  depreciationMonths: number
  remainingMonths: number
}

export interface RoaResult {
  deviceSerialNo: string
  purchasePrice: number
  totalRevenue: number
  annualRevenue: number
  yearsInService: number
  roa: number
  roaPercent: number
}

export interface RoaCalculate extends RoaResult {
  ok: boolean
  assessment: string
}

export interface RoaListResponse {
  ok: boolean
  devices: RoaResult[]
  total: number
}

export interface RepairStats {
  ok: boolean
  deviceSerialNo: string
  totalRepairs: number
  totalRepairCost: number
  replacementValue: number
  thresholdRatio: number
  exceedsReplacementThreshold: boolean
  warning: string
  recentRepairs: Array<{
    id: string
    status: string
    repairCost: number
    vendor: string
    createdAt: string
    completedAt: string | null
  }>
}

// ── Procurement ───────────────────────────────────────────────────

export async function recordPurchase(params: {
  deviceSerialNo: string
  purchasePrice: number
  purchaseDate?: string
  vendor?: string
  invoiceNo?: string
  replacementValue?: number
  notes?: string
}): Promise<any> {
  return requestJson('/api/assets/procurement/record_purchase', {
    method: 'POST',
    body: JSON.stringify(params),
  })
}

export async function setReplacementValue(deviceSerialNo: string, replacementValue: number): Promise<any> {
  return requestJson('/api/assets/procurement/set_value', {
    method: 'POST',
    body: JSON.stringify({ deviceSerialNo, replacementValue }),
  })
}

export async function getPurchase(deviceSerialNo: string): Promise<{ ok: boolean; purchase: PurchaseInfo | null }> {
  return requestJson('/api/assets/procurement/get', {
    method: 'POST',
    body: JSON.stringify({ deviceSerialNo }),
  })
}

// ── Depreciation ──────────────────────────────────────────────────

export async function calculateDepreciation(deviceSerialNo: string, usefulLifeMonths?: number): Promise<NetBookValue> {
  return requestJson('/api/assets/depreciation/calculate', {
    method: 'POST',
    body: JSON.stringify({ deviceSerialNo, usefulLifeMonths }),
  })
}

export async function runMonthlyDepreciation(usefulLifeMonths?: number): Promise<any> {
  return requestJson('/api/assets/depreciation/run_monthly', {
    method: 'POST',
    body: JSON.stringify({ usefulLifeMonths }),
  })
}

export async function getDepreciation(deviceSerialNo: string): Promise<DepreciationResponse> {
  return requestJson('/api/assets/depreciation/get', {
    method: 'POST',
    body: JSON.stringify({ deviceSerialNo }),
  })
}

// ── ROA ───────────────────────────────────────────────────────────

export async function calculateRoa(deviceSerialNo: string): Promise<RoaCalculate> {
  return requestJson('/api/assets/roa/calculate', {
    method: 'POST',
    body: JSON.stringify({ deviceSerialNo }),
  })
}

export async function listRoa(): Promise<RoaListResponse> {
  return requestJson('/api/assets/roa/list', {
    method: 'POST',
    body: JSON.stringify({}),
  })
}

// ── Repair Aggregate ──────────────────────────────────────────────

export async function getRepairStats(deviceSerialNo: string): Promise<RepairStats> {
  return requestJson('/api/assets/repair/stats', {
    method: 'POST',
    body: JSON.stringify({ deviceSerialNo }),
  })
}
