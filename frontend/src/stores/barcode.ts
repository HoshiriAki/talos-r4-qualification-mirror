import { defineStore } from 'pinia'
import { ref } from 'vue'
import * as barcodeApi from '@/api/barcode'
import type { BarcodeLabel, ScanEvent, ScanStats } from '@/api/barcode'

export const useBarcodeStore = defineStore('barcode', () => {
  const labels = ref<BarcodeLabel[]>([])
  const scanEvents = ref<ScanEvent[]>([])
  const scanTotal = ref(0)
  const stats = ref<ScanStats | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)

  async function generate(deviceSerialNo: string) {
    loading.value = true
    error.value = null
    try {
      const label = await barcodeApi.generateBarcode(deviceSerialNo)
      const idx = labels.value.findIndex(l => l.deviceSerialNo === deviceSerialNo)
      if (idx >= 0) labels.value[idx] = label
      else labels.value.push(label)
      return label
    } catch (e: any) {
      error.value = e.message
      throw e
    } finally {
      loading.value = false
    }
  }

  async function batchGenerate() {
    loading.value = true
    error.value = null
    try {
      const result = await barcodeApi.batchGenerateBarcodes()
      return result
    } catch (e: any) {
      error.value = e.message
      throw e
    } finally {
      loading.value = false
    }
  }

  async function lookup(barcodeText: string) {
    return barcodeApi.lookupBarcode(barcodeText)
  }

  async function recordScan(data: Parameters<typeof barcodeApi.recordScanEvent>[0]) {
    return barcodeApi.recordScanEvent(data)
  }

  async function fetchScanHistory(params: Parameters<typeof barcodeApi.getScanHistory>[0]) {
    loading.value = true
    error.value = null
    try {
      const result = await barcodeApi.getScanHistory(params)
      scanEvents.value = result.events
      scanTotal.value = result.total
      return result
    } catch (e: any) {
      error.value = e.message
      throw e
    } finally {
      loading.value = false
    }
  }

  async function fetchStats() {
    loading.value = true
    error.value = null
    try {
      stats.value = await barcodeApi.getScanStats()
      return stats.value
    } catch (e: any) {
      error.value = e.message
      throw e
    } finally {
      loading.value = false
    }
  }

  return {
    labels, scanEvents, scanTotal, stats, loading, error,
    generate, batchGenerate, lookup, recordScan, fetchScanHistory, fetchStats,
  }
})
