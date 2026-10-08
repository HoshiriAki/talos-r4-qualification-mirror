import { requestBlob } from './client'

export interface RevenueReportParams {
  startDate?: string
  endDate?: string
}

export async function exportRevenueReport(params: RevenueReportParams = {}): Promise<Blob> {
  return requestBlob('/api/reports/revenue', {
    method: 'POST',
    body: JSON.stringify({
      start_date: params.startDate || '',
      end_date: params.endDate || '',
    }),
  })
}
