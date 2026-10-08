import { requestJson } from './client'

export interface BarcodeLabel {
  id: number
  deviceSerialNo: string
  barcodeText: string
  barcodeType: string
  labelFormat: string
  generatedAt: string
}

export interface ScanEvent {
  id: number
  deviceSerialNo: string
  barcodeText: string | null
  scanType: string
  scannedBy: string
  warehouseId: string | null
  notes: string | null
  createdAt: string
}

export interface ScanStats {
  todayScans: number
  thisWeekScans: number
  byType: {
    checkout: number
    checkin: number
    inventory: number
    transfer: number
  }
}

export async function generateBarcode(deviceSerialNo: string): Promise<BarcodeLabel> {
  return requestJson('/api/barcode/generate', {
    method: 'POST',
    body: JSON.stringify({ deviceSerialNo }),
  }, '生成条码失败')
}

export async function batchGenerateBarcodes(): Promise<{ generated: number }> {
  return requestJson('/api/barcode/batch-generate', {
    method: 'POST',
    body: JSON.stringify({}),
  }, '批量生成条码失败')
}

export async function lookupBarcode(barcodeText: string): Promise<any> {
  return requestJson('/api/barcode/lookup', {
    method: 'POST',
    body: JSON.stringify({ barcodeText }),
  }, '条码查询失败')
}

export async function recordScanEvent(data: {
  deviceSerialNo: string
  barcodeText?: string
  scanType: string
  warehouseId?: string
  notes?: string
}): Promise<ScanEvent> {
  return requestJson('/api/barcode/scan', {
    method: 'POST',
    body: JSON.stringify(data),
  }, '记录扫码事件失败')
}

export async function getScanHistory(params: {
  deviceSerialNo?: string
  scanType?: string
  startDate?: string
  endDate?: string
  page?: number
  pageSize?: number
}): Promise<{ events: ScanEvent[]; total: number }> {
  const qs = new URLSearchParams()
  if (params.deviceSerialNo) qs.set('deviceSerialNo', params.deviceSerialNo)
  if (params.scanType) qs.set('scanType', params.scanType)
  if (params.startDate) qs.set('startDate', params.startDate)
  if (params.endDate) qs.set('endDate', params.endDate)
  if (params.page) qs.set('page', String(params.page))
  if (params.pageSize) qs.set('pageSize', String(params.pageSize))
  return requestJson(`/api/barcode/scan-history?${qs.toString()}`, '获取扫码记录失败')
}

export async function getScanStats(): Promise<ScanStats> {
  return requestJson('/api/barcode/scan-stats', '获取扫码统计失败')
}
