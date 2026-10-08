import { defineStore } from 'pinia'
import { ref } from 'vue'
import { contractApi } from '@/api/contract'

export const useContractStore = defineStore('contract', () => {
  // ── Templates ──────────────────────────────────────────────────────────
  const templates = ref<any[]>([])
  const templatesTotal = ref(0)
  const currentTemplate = ref<any>(null)

  // ── Contracts ──────────────────────────────────────────────────────────
  const contracts = ref<any[]>([])
  const contractsTotal = ref(0)
  const currentContract = ref<any>(null)

  // ── Signing ────────────────────────────────────────────────────────────
  const signRecords = ref<any[]>([])
  const signRecordsTotal = ref(0)
  const currentSignRecord = ref<any>(null)
  const renderedHtml = ref<string | null>(null)

  // ── General ────────────────────────────────────────────────────────────
  const loading = ref(false)
  const error = ref<string | null>(null)

  // ── Templates ──────────────────────────────────────────────────────────

  async function fetchTemplates(params?: Record<string, any>) {
    error.value = null
    loading.value = true
    try {
      const data = await contractApi.templateList(params)
      templates.value = data.records || data.items || data || []
      templatesTotal.value = data.total || templates.value.length
      return data
    } catch (e: any) {
      error.value = e.message || '获取模板列表失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  async function fetchTemplate(params?: Record<string, any>) {
    error.value = null
    loading.value = true
    try {
      const data = await contractApi.templateGet(params)
      currentTemplate.value = data
      return data
    } catch (e: any) {
      error.value = e.message || '获取模板详情失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  async function createTemplate(body: any) {
    error.value = null
    try {
      const data = await contractApi.templateCreate(body)
      return data
    } catch (e: any) {
      error.value = e.message || '创建模板失败'
      throw e
    }
  }

  async function updateTemplate(body: any) {
    error.value = null
    try {
      const data = await contractApi.templateUpdate(body)
      return data
    } catch (e: any) {
      error.value = e.message || '更新模板失败'
      throw e
    }
  }

  async function deleteTemplate(body: any) {
    error.value = null
    try {
      const data = await contractApi.templateDelete(body)
      return data
    } catch (e: any) {
      error.value = e.message || '删除模板失败'
      throw e
    }
  }

  async function renderTemplate(body: any) {
    error.value = null
    loading.value = true
    try {
      const data = await contractApi.templateRender(body)
      renderedHtml.value = data.html || data.rendered || data.content || null
      return data
    } catch (e: any) {
      error.value = e.message || '渲染模板失败'
      renderedHtml.value = null
      throw e
    } finally {
      loading.value = false
    }
  }

  // ── Contracts ──────────────────────────────────────────────────────────

  async function fetchContracts(params?: Record<string, any>) {
    error.value = null
    loading.value = true
    try {
      const data = await contractApi.contractList(params)
      contracts.value = data.records || data.items || data || []
      contractsTotal.value = data.total || contracts.value.length
      return data
    } catch (e: any) {
      error.value = e.message || '获取合同列表失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  async function fetchContract(params?: Record<string, any>) {
    error.value = null
    loading.value = true
    try {
      const data = await contractApi.contractGet(params)
      currentContract.value = data
      return data
    } catch (e: any) {
      error.value = e.message || '获取合同详情失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  async function generateContract(body: any) {
    error.value = null
    try {
      const data = await contractApi.contractGenerate(body)
      return data
    } catch (e: any) {
      error.value = e.message || '生成合同失败'
      throw e
    }
  }

  async function voidContract(body: any) {
    error.value = null
    try {
      const data = await contractApi.contractVoid(body)
      return data
    } catch (e: any) {
      error.value = e.message || '作废合同失败'
      throw e
    }
  }

  // ── Signing ────────────────────────────────────────────────────────────

  async function requestSign(body: any) {
    error.value = null
    try {
      const data = await contractApi.signRequest(body)
      return data
    } catch (e: any) {
      error.value = e.message || '发起签署失败'
      throw e
    }
  }

  async function verifySign(body: any) {
    error.value = null
    try {
      const data = await contractApi.signVerify(body)
      return data
    } catch (e: any) {
      error.value = e.message || '验证签名失败'
      throw e
    }
  }

  async function fetchSignStatus(params?: Record<string, any>) {
    error.value = null
    try {
      const data = await contractApi.signStatus(params)
      currentSignRecord.value = data
      return data
    } catch (e: any) {
      error.value = e.message || '获取签署状态失败'
      throw e
    }
  }

  async function fetchSignRecords(params?: Record<string, any>) {
    error.value = null
    loading.value = true
    try {
      const data = await contractApi.signStatus(params)
      signRecords.value = data.records || data.items || (data ? [data] : [])
      signRecordsTotal.value = data.total || signRecords.value.length
      return data
    } catch (e: any) {
      error.value = e.message || '获取签署记录失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  return {
    templates,
    templatesTotal,
    currentTemplate,
    contracts,
    contractsTotal,
    currentContract,
    signRecords,
    signRecordsTotal,
    currentSignRecord,
    renderedHtml,
    loading,
    error,
    fetchTemplates,
    fetchTemplate,
    createTemplate,
    updateTemplate,
    deleteTemplate,
    renderTemplate,
    fetchContracts,
    fetchContract,
    generateContract,
    voidContract,
    requestSign,
    verifySign,
    fetchSignStatus,
    fetchSignRecords,
  }
})
