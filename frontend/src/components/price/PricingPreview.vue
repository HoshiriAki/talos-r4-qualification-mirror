<script setup lang="ts">
import { ref, watch } from 'vue'
import { useToast } from 'primevue/usetoast'
import type { PriceDetail, WarehouseRoute } from '@/api/pricing-estimate'
import { OCCUPANCY_COEFFICIENTS } from '@/composables/usePricingCalc'

const props = defineProps<{
  startDate: string
  endDate: string
  deliveryDate: string
  breakdown: PriceDetail[]
  totalPrice: number
  route: WarehouseRoute | null
  generateQuote: (price: number) => string
}>()

const toast = useToast()
const copied = ref(false)
const showBreakdown = ref(false)
const priceFlash = ref<'up' | 'down' | null>(null)
const prevPrice = ref(0)

watch(() => props.totalPrice, (newVal, oldVal) => {
  if (oldVal !== undefined && newVal !== oldVal && oldVal > 0) {
    priceFlash.value = newVal > oldVal ? 'up' : 'down'
    setTimeout(() => priceFlash.value = null, 600)
  }
  prevPrice.value = newVal
})

const occupyLabels: Record<string, string> = {
  shipping: '发货在途',
  normal: '正常占用',
  return: '归还返程',
}

const occupyColors: Record<string, string> = {
  shipping: 'text-blue-400',
  normal: 'text-text-primary',
  return: 'text-orange-400',
}

async function copyQuote() {
  try {
    await navigator.clipboard.writeText(props.generateQuote(props.totalPrice))
    copied.value = true
    toast.add({ severity: 'success', summary: '已复制', detail: '报价话术已复制到剪贴板', life: 2000 })
    setTimeout(() => copied.value = false, 2000)
  } catch {
    toast.add({ severity: 'error', summary: '复制失败', detail: '剪贴板不可用', life: 4000 })
  }
}
</script>

<template>
  <div class="space-y-2 border-t border-border pt-3">
    <div class="flex justify-between text-xs font-mono">
      <span class="text-text-muted">开始日期</span>
      <span class="text-text-primary">{{ startDate }}</span>
    </div>
    <div class="flex justify-between text-xs font-mono">
      <span class="text-text-muted">结束日期</span>
      <span class="text-text-primary">{{ endDate || '-' }}</span>
    </div>
    <div class="flex justify-between text-xs font-mono">
      <span class="text-text-muted">发货日期</span>
      <span class="text-text-primary">{{ deliveryDate || '-' }}</span>
    </div>

    <!-- Warehouse info -->
    <template v-if="route">
      <div class="border-t border-border pt-2" />
      <div class="flex justify-between text-xs font-mono">
        <span class="text-text-muted">发货仓库</span>
        <span class="text-text-primary">{{ route.sendWarehouseName }} ({{ route.shippingDays }}天)</span>
      </div>
      <div class="flex justify-between text-xs font-mono">
        <span class="text-text-muted">归还仓库</span>
        <span class="text-text-primary">{{ route.returnWarehouseName }} ({{ route.returnDays }}天)</span>
      </div>
    </template>

    <!-- Occupancy coefficients -->
    <div class="border-t border-border pt-2" />
    <div class="flex justify-between text-xs font-mono">
      <span class="text-text-muted">占用系数</span>
      <span class="text-text-primary">
        发货{{ OCCUPANCY_COEFFICIENTS.shipping }}× / 正常{{ OCCUPANCY_COEFFICIENTS.normal }}× / 归还{{ OCCUPANCY_COEFFICIENTS.return }}×
      </span>
    </div>

    <!-- Daily breakdown toggle -->
    <div v-if="breakdown.length > 0" class="pt-1">
      <Button
        variant="text"
        severity="secondary"
        class="w-full"
        @click="showBreakdown = !showBreakdown"
      >
        <span class="flex-1 text-left">逐日明细 ({{ breakdown.length }} 天)</span>
        <span class="font-mono">{{ showBreakdown ? '▾' : '▸' }}</span>
      </Button>
      <div v-if="showBreakdown" class="relative mt-2 max-h-48 overflow-y-auto" :class="{ 'scroll-fade-bottom': breakdown.length > 5 }">
        <div class="space-y-1">
          <div
            v-for="d in breakdown"
            :key="d.dateKey"
            class="flex justify-between text-xs font-mono py-0.5"
          >
            <span class="text-text-muted w-24">{{ d.dateKey }}</span>
            <span :class="occupyColors[d.occupyType] || 'text-text-muted'" class="w-16 text-right">
              {{ occupyLabels[d.occupyType] || d.occupyType }}
            </span>
            <span class="text-text-muted w-10 text-right">{{ d.occupyFactor }}×</span>
            <span class="text-text-secondary w-18 text-right">¥{{ d.finalDailyPrice.toFixed(2) }}</span>
            <span class="text-text-primary w-18 text-right">¥{{ d.amount.toFixed(2) }}</span>
          </div>
        </div>
      </div>
    </div>

    <div class="flex justify-between items-center border-t border-border pt-2">
      <span class="font-mono font-bold text-md text-text-primary">报价</span>
      <span
        class="font-mono font-bold text-xl price-value"
        :class="{
          'price-flash-up': priceFlash === 'up',
          'price-flash-down': priceFlash === 'down',
        }"
      >&yen;{{ totalPrice.toFixed(2) }}</span>
    </div>
    <Button severity="secondary" class="w-full" :label="copied ? '已复制' : '复制报价话术'" @click="copyQuote" />
  </div>
</template>

<style scoped>
.price-value {
  transition: color 300ms ease;
}
.price-flash-up {
  color: var(--red) !important;
}
.price-flash-down {
  color: var(--green) !important;
}
.scroll-fade-bottom {
  mask-image: linear-gradient(to bottom, black 60%, transparent 100%);
  -webkit-mask-image: linear-gradient(to bottom, black 60%, transparent 100%);
}
</style>
