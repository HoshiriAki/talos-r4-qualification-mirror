import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { simulationPricingApi, type SimulationPricingConfig, type SimulationPricingEstimate } from '@/api/simulationPricing'

const DEFAULT_CONFIG: SimulationPricingConfig = {
  baseWeekdayPrice: 8.5,
  baseWeekendPrice: 14,
  holidayRules: [],
  receiveShippingFees: { area1: 7, area2: 7, area3: 7, area4: 18 },
  dynamicPriceMap: {},
}

function idempotencyKey() { return `${crypto.randomUUID()}-${Date.now().toString(36)}` }

export const useSimulationPricingStore = defineStore('simulation-pricing', () => {
  const simulationId = ref('')
  const config = ref<SimulationPricingConfig>({ ...DEFAULT_CONFIG })
  const estimateResult = ref<SimulationPricingEstimate | null>(null)
  const loading = ref(false)
  const saving = ref(false)
  const estimating = ref(false)
  const error = ref<string | null>(null)
  const ready = computed(() => Boolean(simulationId.value))

  function clear() {
    simulationId.value = ''
    config.value = { ...DEFAULT_CONFIG, receiveShippingFees: { ...DEFAULT_CONFIG.receiveShippingFees }, dynamicPriceMap: {} }
    estimateResult.value = null
    error.value = null
  }

  async function load(id: string) {
    simulationId.value = id
    loading.value = true; error.value = null
    try { config.value = await simulationPricingApi.getConfig(id) }
    catch (cause) { error.value = cause instanceof Error ? cause.message : '读取隔离定价配置失败'; throw cause }
    finally { loading.value = false }
  }

  async function save(next: SimulationPricingConfig) {
    if (!simulationId.value) return
    saving.value = true; error.value = null
    try { config.value = await simulationPricingApi.saveConfig(simulationId.value, next, idempotencyKey()) }
    catch (cause) { error.value = cause instanceof Error ? cause.message : '写入隔离定价配置失败'; throw cause }
    finally { saving.value = false }
  }

  async function estimate(startDate: string, endDate: string) {
    if (!simulationId.value) return
    estimating.value = true; error.value = null
    try { estimateResult.value = await simulationPricingApi.estimate(simulationId.value, startDate, endDate) }
    catch (cause) { error.value = cause instanceof Error ? cause.message : '计算隔离价格失败'; throw cause }
    finally { estimating.value = false }
  }

  return { simulationId, config, estimateResult, loading, saving, estimating, error, ready, clear, load, save, estimate }
})
