import { defineStore } from 'pinia'
import { ref } from 'vue'
import { overdueApi } from '@/api/overdue'

export const useOverdueStore = defineStore('overdue', () => {
  const records = ref<any[]>([])
  const recordsTotal = ref(0)
  const stats = ref<any>(null)
  const config = ref<any>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)

  // ── Records ───────────────────────────────────────────────────────────

  async function fetchList(params?: Record<string, any>) {
    error.value = null
    loading.value = true
    try {
      const data = await overdueApi.list(params)
      records.value = data.records || data.items || data || []
      recordsTotal.value = data.total || records.value.length
      return data
    } catch (e: any) {
      error.value = e.message || '获取逾期列表失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  async function getRecord(params?: Record<string, any>) {
    error.value = null
    loading.value = true
    try {
      const data = await overdueApi.get(params)
      return data
    } catch (e: any) {
      error.value = e.message || '获取逾期详情失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  // ── Stats ─────────────────────────────────────────────────────────────

  async function fetchStats() {
    error.value = null
    try {
      const data = await overdueApi.stats()
      stats.value = data
      return data
    } catch (e: any) {
      error.value = e.message || '获取统计失败'
      throw e
    }
  }

  // ── Config ────────────────────────────────────────────────────────────

  async function fetchConfig() {
    error.value = null
    try {
      const data = await overdueApi.config()
      config.value = data
      return data
    } catch (e: any) {
      error.value = e.message || '获取配置失败'
      throw e
    }
  }

  async function upsertConfig(body: any) {
    error.value = null
    try {
      const data = await overdueApi.configUpsert(body)
      config.value = data
      return data
    } catch (e: any) {
      error.value = e.message || '保存配置失败'
      throw e
    }
  }

  // ── Actions ───────────────────────────────────────────────────────────

  async function detect(body?: any) {
    error.value = null
    try {
      const data = await overdueApi.detect(body)
      return data
    } catch (e: any) {
      error.value = e.message || '检测失败'
      throw e
    }
  }

  async function calcFee(body: any) {
    error.value = null
    try {
      const data = await overdueApi.calc(body)
      return data
    } catch (e: any) {
      error.value = e.message || '计算失败'
      throw e
    }
  }

  async function applyFee(body: any) {
    error.value = null
    try {
      const data = await overdueApi.apply(body)
      return data
    } catch (e: any) {
      error.value = e.message || '应用失败'
      throw e
    }
  }

  async function waiveFee(body: any) {
    error.value = null
    try {
      const data = await overdueApi.waive(body)
      return data
    } catch (e: any) {
      error.value = e.message || '豁免失败'
      throw e
    }
  }

  async function escalate(body?: any) {
    error.value = null
    try {
      const data = await overdueApi.escalate(body)
      return data
    } catch (e: any) {
      error.value = e.message || '升级通知失败'
      throw e
    }
  }

  async function fetchEscalationHistory(params?: Record<string, any>) {
    error.value = null
    try {
      const data = await overdueApi.escalationHistory(params)
      return data
    } catch (e: any) {
      error.value = e.message || '获取升级历史失败'
      throw e
    }
  }

  // ── Pre-order ─────────────────────────────────────────────────────────

  async function checkBeforeOrder(body: any) {
    error.value = null
    try {
      const data = await overdueApi.checkBeforeOrder(body)
      return data
    } catch (e: any) {
      error.value = e.message || '逾期检查失败'
      throw e
    }
  }

  return {
    records,
    recordsTotal,
    stats,
    config,
    loading,
    error,
    fetchList,
    getRecord,
    fetchStats,
    fetchConfig,
    upsertConfig,
    detect,
    calcFee,
    applyFee,
    waiveFee,
    escalate,
    fetchEscalationHistory,
    checkBeforeOrder,
  }
})
