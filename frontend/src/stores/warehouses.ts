import { defineStore } from 'pinia'
import { ref } from 'vue'
import * as warehousesApi from '@/api/warehouses'
import type { Warehouse, RegionRule } from '@/api/warehouses'

export const useWarehousesStore = defineStore('warehouses', () => {
  const warehouses = ref<Warehouse[]>([])
  const regions = ref<Record<string, RegionRule[]>>({})
  const loading = ref(false)

  async function fetchAll() {
    loading.value = true
    try {
      const data = await warehousesApi.fetchWarehouses()
      warehouses.value = data.warehouses || []
    } finally {
      loading.value = false
    }
  }

  async function fetchRegions(warehouseId: string) {
    const data = await warehousesApi.fetchWarehouse(warehouseId)
    if (data.regions) {
      regions.value = { ...regions.value, [warehouseId]: data.regions }
    }
    return data
  }

  async function create(payload: Partial<Warehouse>) {
    const data = await warehousesApi.createWarehouse(payload)
    if (data.warehouse) warehouses.value.push(data.warehouse)
    return data
  }

  async function update(id: string, payload: Partial<Warehouse>) {
    const data = await warehousesApi.updateWarehouse(id, payload)
    if (data.warehouse) {
      const idx = warehouses.value.findIndex(w => w.id === id)
      if (idx !== -1) warehouses.value[idx] = data.warehouse
    }
    return data
  }

  async function remove(id: string) {
    await warehousesApi.deleteWarehouse(id)
    warehouses.value = warehouses.value.filter(w => w.id !== id)
  }

  async function saveRegionRule(warehouseId: string, payload: Partial<RegionRule>) {
    const data = await warehousesApi.upsertRegionRule(warehouseId, payload)
    if (data.regions) {
      regions.value = { ...regions.value, [warehouseId]: data.regions }
    }
    return data
  }

  async function deleteRegionRule(warehouseId: string, province: string) {
    await warehousesApi.deleteRegionRule(warehouseId, province)
    const current = regions.value[warehouseId] || []
    regions.value = {
      ...regions.value,
      [warehouseId]: current.filter(r => r.province !== province),
    }
  }

  return { warehouses, regions, loading, fetchAll, fetchRegions, create, update, remove, saveRegionRule, deleteRegionRule }
})
