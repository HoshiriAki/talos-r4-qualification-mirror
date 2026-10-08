import { requestJson } from './client'

export const opticalSopApi = {
  create: (body: any) => requestJson('/api/optical-sop/inspections/create', { method: 'POST', body }),
  updateStep: (body: any) => requestJson('/api/optical-sop/inspections/update-step', { method: 'POST', body }),
  complete: (body: any) => requestJson('/api/optical-sop/inspections/complete', { method: 'POST', body }),
  get: (params: Record<string, any>) => requestJson(`/api/optical-sop/inspections/get?${new URLSearchParams(params)}`),
  list: (params?: Record<string, any>) => requestJson(`/api/optical-sop/inspections/list${params ? '?' + new URLSearchParams(params) : ''}`),
  stats: () => requestJson('/api/optical-sop/inspections/stats'),
}
