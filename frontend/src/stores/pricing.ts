import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import * as pricingApi from '@/api/pricing'
import type { PricingConfig, HolidayRule } from '@/api/pricing'

const DEFAULT_CONFIG: PricingConfig = {
  baseWeekdayPrice: 8.5,
  baseWeekendPrice: 14,
  holidayRules: [],
  receiveShippingFees: { area1: 7, area2: 7, area3: 7, area4: 18 },
  dynamicPriceMap: {},
}

export const usePricingStore = defineStore('pricing', () => {
  const config = ref<PricingConfig>({ ...DEFAULT_CONFIG })
  const loading = ref(false)
  const saving = ref(false)
  const error = ref<string | null>(null)

  const baseWeekdayPrice = computed(() => config.value.baseWeekdayPrice)
  const baseWeekendPrice = computed(() => config.value.baseWeekendPrice)
  const holidayRules = computed(() => config.value.holidayRules)
  const dynamicPriceMap = computed(() => config.value.dynamicPriceMap)

  async function fetchConfig() {
    error.value = null
    loading.value = true
    try {
      const data = await pricingApi.fetchConfig()
      if (data.config) {
        config.value = { ...DEFAULT_CONFIG, ...data.config }
      }
    } catch (e: any) {
      error.value = e.message || 'Failed to fetch pricing config'
    } finally {
      loading.value = false
    }
  }

  async function saveConfig(payload: Partial<PricingConfig>) {
    error.value = null
    saving.value = true
    try {
      const data = await pricingApi.saveConfig(payload)
      if (data.config) {
        config.value = { ...DEFAULT_CONFIG, ...data.config }
      }
      return data
    } catch (e: any) {
      error.value = e.message || 'Failed to save pricing config'
      throw e
    } finally {
      saving.value = false
    }
  }

  return {
    error, config, loading, saving,
    baseWeekdayPrice, baseWeekendPrice, holidayRules, dynamicPriceMap,
    fetchConfig, saveConfig,
  }
})
