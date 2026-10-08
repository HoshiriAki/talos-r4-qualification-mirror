import { defineStore } from 'pinia'
import { ref } from 'vue'
import { creditApi } from '@/api/credit'

export const useCreditStore = defineStore('credit', () => {
  const currentScore = ref<any>(null)
  const scoreHistory = ref<any[]>([])
  const blacklist = ref<any[]>([])
  const blacklistTotal = ref(0)
  const violations = ref<any[]>([])
  const violationsTotal = ref(0)
  const loading = ref(false)
  const error = ref<string | null>(null)

  // ── Credit Score ───────────────────────────────────────────────────

  async function fetchCreditScore(phone: string) {
    error.value = null
    loading.value = true
    try {
      const data = await creditApi.creditGet({ phone })
      currentScore.value = data
      return data
    } catch (e: any) {
      error.value = e.message || '获取信用评分失败'
      currentScore.value = null
      throw e
    } finally {
      loading.value = false
    }
  }

  async function fetchCreditHistory(phone: string) {
    error.value = null
    loading.value = true
    try {
      const data = await creditApi.creditHistory({ phone })
      scoreHistory.value = data.history || data || []
      return data
    } catch (e: any) {
      error.value = e.message || '获取信用历史失败'
      scoreHistory.value = []
      throw e
    } finally {
      loading.value = false
    }
  }

  async function recalculateCredit(phone: string) {
    error.value = null
    loading.value = true
    try {
      const data = await creditApi.creditRecalculate({ phone })
      currentScore.value = data
      return data
    } catch (e: any) {
      error.value = e.message || '重新计算失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  // ── Blacklist ──────────────────────────────────────────────────────

  async function fetchBlacklist(params?: Record<string, any>) {
    error.value = null
    loading.value = true
    try {
      const data = await creditApi.blacklistList(params)
      blacklist.value = data.records || data.items || data || []
      blacklistTotal.value = data.total || blacklist.value.length
      return data
    } catch (e: any) {
      error.value = e.message || '获取黑名单失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  async function addToBlacklist(body: any) {
    error.value = null
    try {
      const data = await creditApi.blacklistAdd(body)
      return data
    } catch (e: any) {
      error.value = e.message || '添加黑名单失败'
      throw e
    }
  }

  async function removeFromBlacklist(body: any) {
    error.value = null
    try {
      const data = await creditApi.blacklistRemove(body)
      return data
    } catch (e: any) {
      error.value = e.message || '移除黑名单失败'
      throw e
    }
  }

  // ── Violations ─────────────────────────────────────────────────────

  async function fetchViolations(params?: Record<string, any>) {
    error.value = null
    loading.value = true
    try {
      const data = await creditApi.violationList(params)
      violations.value = data.records || data.items || data || []
      violationsTotal.value = data.total || violations.value.length
      return data
    } catch (e: any) {
      error.value = e.message || '获取违规列表失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  async function recordViolation(body: any) {
    error.value = null
    try {
      const data = await creditApi.violationRecord(body)
      return data
    } catch (e: any) {
      error.value = e.message || '记录违规失败'
      throw e
    }
  }

  async function appealViolation(body: any) {
    error.value = null
    try {
      const data = await creditApi.violationAppeal(body)
      return data
    } catch (e: any) {
      error.value = e.message || '申诉失败'
      throw e
    }
  }

  async function reviewViolation(body: any) {
    error.value = null
    try {
      const data = await creditApi.violationReview(body)
      return data
    } catch (e: any) {
      error.value = e.message || '审核失败'
      throw e
    }
  }

  return {
    currentScore,
    scoreHistory,
    blacklist,
    blacklistTotal,
    violations,
    violationsTotal,
    loading,
    error,
    fetchCreditScore,
    fetchCreditHistory,
    recalculateCredit,
    fetchBlacklist,
    addToBlacklist,
    removeFromBlacklist,
    fetchViolations,
    recordViolation,
    appealViolation,
    reviewViolation,
  }
})
