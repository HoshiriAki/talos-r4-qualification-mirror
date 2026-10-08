import { defineStore } from 'pinia'
import { computed } from 'vue'
import { createBaseTableStore } from './baseTable'
import * as devicesApi from '@/api/devices'
import type { Device, DeviceFilters } from '@/api/devices'
import type { DataQuery, DataPage } from '@/types/data-table'
import { shanghaiBusinessDate } from '@/utils/businessDate'

async function fetchPageAdapter(query: DataQuery<DeviceFilters>): Promise<DataPage<Device>> {
  const result = await devicesApi.fetchPage(
    query.filter || {},
    query.page || 1,
    query.pageSize || 30,
  )
  return {
    rows: result.devices || [],
    total: result.pagination?.total || 0,
    totalPages: result.pagination?.totalPages || 0,
    hasMore: false,
  }
}

const base = createBaseTableStore<Device, DeviceFilters>(
  'devices-base',
  fetchPageAdapter,
  { keyword: '', rentalStatus: '', notes: '', warningStatus: '' },
)

export const useDevicesStore = defineStore('devices', () => {
  const table = base()

  async function create(data: Parameters<typeof devicesApi.createDevice>[0]) {
    const device = await devicesApi.createDevice(data)
    return device
  }

  async function update(serialNo: string, data: Partial<Device>) {
    const device = await devicesApi.updateDevice(serialNo, data)
    const idx = table.rows.findIndex((d: Device) => d.serialNo === serialNo)
    if (idx >= 0) table.rows[idx] = device
    return device
  }

  async function remove(serialNo: string) {
    await devicesApi.deleteDevice(serialNo)
    table.rows = table.rows.filter((d: Device) => d.serialNo !== serialNo)
  }

  async function bulkDelete(serialNos: string[]) {
    const result = await devicesApi.bulkDeleteDevices(serialNos)
    await table.fetchPage()
    return result
  }

  async function checkinScan(serialNo: string) {
    return devicesApi.checkinScan(serialNo)
  }

  async function exportDevices(exportFilters?: DeviceFilters, serialNos?: string[]) {
    const blob = await devicesApi.exportDevices(exportFilters || table.filter, serialNos)
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `devices-${shanghaiBusinessDate()}.xlsx`
    a.click()
    URL.revokeObjectURL(url)
  }

  async function bulkUpdate(serialNos: string[], updates: Partial<Device>) {
    const result = await devicesApi.bulkUpdateDevices(serialNos, updates)
    await table.fetchPage()
    return result
  }

  async function importExcel(file: File) {
    return devicesApi.importExcel(file)
  }

  return {
    rows: table.rows,
    devices: computed(() => table.rows),
    loading: table.loading,
    error: table.error,
    total: table.total,
    page: table.page,
    pageSize: table.pageSize,
    totalPages: table.totalPages,
    pagination: table.pagination,
    filter: table.filter,
    filters: table.filter,
    fetchPage: table.fetchPage,
    fetchMore: table.fetchMore,
    setFilter: table.setFilter,
    resetFilter: table.resetFilter,
    resetFilters: table.resetFilter,
    create,
    update,
    remove,
    bulkDelete,
    bulkUpdate,
    checkinScan,
    exportDevices,
    importExcel,
  }
})
