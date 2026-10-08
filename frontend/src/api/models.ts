import { requestJson } from './client'

export interface DeviceModel {
  id: string
  name: string
  category: string
  prefix: string
  enabled: boolean
  weekdayPrice: number | null
  weekendPrice: number | null
  createdAt: string
  updatedAt: string
}

export async function fetchModels(): Promise<{ ok: boolean; models: DeviceModel[] }> {
  return requestJson('/api/device-models', '获取型号列表失败')
}

export async function createModel(payload: Partial<DeviceModel>): Promise<{ ok: boolean; model: DeviceModel }> {
  return requestJson('/api/device-models', {
    method: 'POST',
    body: JSON.stringify(payload),
  }, '创建型号失败')
}

export async function updateModel(id: string, payload: Partial<DeviceModel>): Promise<{ ok: boolean; model: DeviceModel }> {
  return requestJson(`/api/device-models/${id}`, {
    method: 'PATCH',
    body: JSON.stringify(payload),
  }, '更新型号失败')
}

export async function updateModelFull(id: string, payload: Partial<DeviceModel> & { weekdayPrice?: number; weekendPrice?: number }): Promise<{ ok: boolean; model: DeviceModel }> {
  return requestJson(`/api/device-models/${id}`, {
    method: 'PUT',
    body: JSON.stringify(payload),
  }, '更新型号失败')
}

export async function updateModelPricing(id: string, payload: { weekdayPrice: number; weekendPrice: number }): Promise<{ ok: boolean; model: DeviceModel }> {
  return requestJson(`/api/device-models/${id}/pricing`, {
    method: 'PUT',
    body: JSON.stringify(payload),
  }, '更新型号定价失败')
}

export async function deleteModel(id: string): Promise<{ ok: boolean }> {
  return requestJson(`/api/device-models/${id}`, {
    method: 'DELETE',
  }, '删除型号失败')
}
