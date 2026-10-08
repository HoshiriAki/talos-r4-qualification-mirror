import { ref } from 'vue'
import { defineStore } from 'pinia'
import { opticalSopApi } from '@/api/optical-sop'

export const useOpticalSopStore = defineStore('opticalSop', () => {
  // ── State ──────────────────────────────────────────────────────────

  const loading = ref(false)
  const inspections = ref<any[]>([])
  const totalRecords = ref(0)
  const currentInspection = ref<any>(null)
  const stats = ref<any>(null)

  // ── Actions ─────────────────────────────────────────────────────────

  async function fetchList(params?: Record<string, any>) {
    loading.value = true
    try {
      const data = await opticalSopApi.list(params)
      inspections.value = data.items || data.inspections || data.data || data || []
      totalRecords.value = data.total || data.totalRecords || data.total_records || 0
    } finally {
      loading.value = false
    }
  }

  async function fetchOne(id: string) {
    loading.value = true
    try {
      const data = await opticalSopApi.get({ id })
      currentInspection.value = data.record || data.inspection || data.data || data
    } finally {
      loading.value = false
    }
  }

  async function createInspection(body: { order_id: string; device_serial_no: string }) {
    loading.value = true
    try {
      const data = await opticalSopApi.create(body)
      if (data?.id != null) {
        await fetchOne(String(data.id))
      } else {
        currentInspection.value = data.record || data.inspection || data.data || data
      }
      return data
    } finally {
      loading.value = false
    }
  }

  async function updateStep(body: { inspection_id: string; step: string; passed: boolean; note?: string }) {
    loading.value = true
    try {
      const normalized = {
        ...body,
        step: body.step.endsWith('_ok') ? body.step.slice(0, -3) : body.step,
      }
      const data = await opticalSopApi.updateStep(normalized)
      if (body.inspection_id) {
        await fetchOne(String(body.inspection_id))
      } else {
        currentInspection.value = data.record || data.inspection || data.data || data
      }
      return data
    } finally {
      loading.value = false
    }
  }

  async function completeInspection(id: string) {
    loading.value = true
    try {
      const data = await opticalSopApi.complete({ inspection_id: id })
      await fetchOne(String(id))
      return data
    } finally {
      loading.value = false
    }
  }

  async function fetchStats() {
    try {
      const data = await opticalSopApi.stats()
      stats.value = data.stats || data.data || data
    } catch {
      // stats are non-critical
    }
  }

  return {
    loading,
    inspections,
    totalRecords,
    currentInspection,
    stats,
    fetchList,
    fetchOne,
    createInspection,
    updateStep,
    completeInspection,
    fetchStats,
  }
})
