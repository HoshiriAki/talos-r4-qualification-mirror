import { defineStore } from 'pinia'
import { ref } from 'vue'
import * as modelsApi from '@/api/models'
import type { DeviceModel } from '@/api/models'

export const useModelsStore = defineStore('models', () => {
  const models = ref<DeviceModel[]>([])
  const loading = ref(false)

  async function fetchAll() {
    loading.value = true
    try {
      const data = await modelsApi.fetchModels()
      models.value = data.models || []
    } finally {
      loading.value = false
    }
  }

  async function create(payload: Partial<DeviceModel>) {
    const data = await modelsApi.createModel(payload)
    if (data.model) models.value.push(data.model)
    return data
  }

  async function update(id: string, payload: Partial<DeviceModel>) {
    const data = await modelsApi.updateModel(id, payload)
    if (data.model) {
      const idx = models.value.findIndex(m => m.id === id)
      if (idx !== -1) models.value[idx] = data.model
    }
    return data
  }

  async function updatePricing(id: string, payload: { weekdayPrice: number; weekendPrice: number }) {
    const data = await modelsApi.updateModelPricing(id, payload)
    if (data.model) {
      const idx = models.value.findIndex(m => m.id === id)
      if (idx !== -1) models.value[idx] = data.model
    }
    return data
  }

  async function updateFull(id: string, payload: Partial<DeviceModel> & { weekdayPrice?: number; weekendPrice?: number }) {
    const data = await modelsApi.updateModelFull(id, payload)
    if (data.model) {
      const idx = models.value.findIndex(m => m.id === id)
      if (idx !== -1) models.value[idx] = data.model
    }
    return data
  }

  async function remove(id: string) {
    await modelsApi.deleteModel(id)
    models.value = models.value.filter(m => m.id !== id)
  }

  return { models, loading, fetchAll, create, update, updatePricing, updateFull, remove }
})
