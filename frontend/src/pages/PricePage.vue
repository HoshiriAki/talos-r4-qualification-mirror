<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useToast } from 'primevue/usetoast'

import CalendarPanel from '@/components/price/CalendarPanel.vue'
import DeviceAccessoryPanel from '@/components/price/DeviceAccessoryPanel.vue'
import { useModelsStore } from '@/stores/models'
import {
  confirmQuote,
  createOrderFromQuote,
  createQuote,
  fetchCustomers,
  type CustomerOption,
  type OrderFromQuote,
  type Quote,
} from '@/api/quotes'

const router = useRouter()
const toast = useToast()
const modelsStore = useModelsStore()

const customers = ref<CustomerOption[]>([])
const customerId = ref('')
const selectedStart = ref('')
const selectedEnd = ref('')
const region = ref('')
const notes = ref('')
const selectedDevices = ref<Record<string, number>>({})
const selectedAccessories = ref<string[]>([])
const offeredQuote = ref<Quote | null>(null)
const createdOrder = ref<OrderFromQuote | null>(null)
const working = ref(false)

const modelLines = computed(() => {
  return Object.entries(selectedDevices.value)
    .filter(([, quantity]) => quantity > 0)
    .map(([modelName, quantity]) => {
      const model = modelsStore.models.find(item => item.name === modelName)
      return model ? { modelId: model.id, quantity } : null
    })
    .filter((line): line is { modelId: string; quantity: number } => line !== null)
})

const unresolvedModels = computed(() => {
  return Object.entries(selectedDevices.value)
    .filter(([, quantity]) => quantity > 0)
    .map(([modelName]) => modelName)
    .filter(modelName => !modelsStore.models.some(item => item.name === modelName))
})

const accessoryLines = computed(() => {
  const counts = new Map<string, number>()
  for (const id of selectedAccessories.value) counts.set(id, (counts.get(id) || 0) + 1)
  return [...counts.entries()].map(([accessoryId, quantity]) => ({ accessoryId, quantity }))
})

const selectionFingerprint = computed(() => JSON.stringify({
  customerId: customerId.value,
  startDate: selectedStart.value,
  endDate: selectedEnd.value,
  region: region.value,
  modelLines: modelLines.value,
  accessoryLines: accessoryLines.value,
}))

watch(selectionFingerprint, () => {
  offeredQuote.value = null
  createdOrder.value = null
})

function money(minor: number, currency: string) {
  const amount = (minor / 100).toFixed(2)
  return currency === 'CNY' ? `¥ ${amount}` : `${currency} ${amount}`
}

function clearForm() {
  customerId.value = ''
  selectedStart.value = ''
  selectedEnd.value = ''
  region.value = ''
  notes.value = ''
  selectedDevices.value = {}
  selectedAccessories.value = []
  offeredQuote.value = null
  createdOrder.value = null
}

function validateSelection(): string | null {
  if (!customerId.value) return '请选择 Customer'
  if (!selectedStart.value || !selectedEnd.value) return '请选择完整租赁日期'
  if (selectedEnd.value < selectedStart.value) return '结束日期不能早于开始日期'
  if (!region.value.trim()) return '请填写业务地区'
  if (unresolvedModels.value.length > 0) return `以下型号尚未映射到服务器 ModelId：${unresolvedModels.value.join('、')}`
  if (modelLines.value.length === 0 && accessoryLines.value.length === 0) return '至少选择一个型号或配件'
  return null
}

async function generateQuote() {
  const validation = validateSelection()
  if (validation) {
    toast.add({ severity: 'warn', summary: '报价条件不完整', detail: validation, life: 3500 })
    return
  }

  working.value = true
  try {
    offeredQuote.value = await createQuote({
      customerId: customerId.value,
      startDate: selectedStart.value,
      endDate: selectedEnd.value,
      region: region.value.trim(),
      modelLines: modelLines.value,
      accessoryLines: accessoryLines.value,
    })
    createdOrder.value = null
    toast.add({ severity: 'success', summary: '服务器报价已生成', detail: '价格已固化为 Quote 快照', life: 2500 })
  } catch (error: any) {
    toast.add({ severity: 'error', summary: '报价失败', detail: error?.message || '服务器报价失败', life: 4500 })
  } finally {
    working.value = false
  }
}

async function acceptAndCreateOrder() {
  if (!offeredQuote.value || offeredQuote.value.status !== 'draft') return
  working.value = true
  try {
    const confirmed = await confirmQuote(offeredQuote.value.id)
    offeredQuote.value = confirmed
    createdOrder.value = await createOrderFromQuote(confirmed.id, notes.value.trim())
    toast.add({
      severity: 'success',
      summary: '订单已创建',
      detail: `服务端订单号：${createdOrder.value.orderNo}`,
      life: 4500,
    })
  } catch (error: any) {
    toast.add({ severity: 'error', summary: '订单创建失败', detail: error?.message || '请检查报价状态', life: 4500 })
  } finally {
    working.value = false
  }
}

function goToOrders() {
  router.push('/app/orders')
}

onMounted(async () => {
  try {
    const [customerRows] = await Promise.all([
      fetchCustomers(),
      modelsStore.fetchAll(),
    ])
    customers.value = customerRows.filter(customer => customer.status === 'active')
  } catch (error: any) {
    toast.add({ severity: 'error', summary: '初始化失败', detail: error?.message || '无法加载 Customer/Model', life: 4500 })
  }
})
</script>

<template>
  <div class="page-root">
    <div class="module-bar mb-4">
      <span class="module-number-label">MODULE-03</span>
      <div class="structure-line" />
      <span class="module-page-label">QUOTE / ORDER ENTRY</span>
    </div>

    <div class="grid grid-cols-1 xl:grid-cols-[1.2fr_0.8fr] gap-4">
      <div class="space-y-4 min-w-0">
        <section class="card">
          <div class="flex items-center justify-between mb-3">
            <h2 class="panel-title mb-0">A-01 · 客户与租期</h2>
            <button class="btn-ghost text-xs" @click="clearForm">清空</button>
          </div>

          <div class="grid grid-cols-1 md:grid-cols-2 gap-3 mb-4">
            <label class="space-y-1">
              <span class="font-mono text-2xs text-text-muted">CUSTOMER</span>
              <select v-model="customerId" class="w-full border border-border bg-transparent px-3 py-2 font-mono text-sm text-text-primary">
                <option value="">选择 Customer</option>
                <option v-for="customer in customers" :key="customer.id" :value="customer.id">
                  {{ customer.displayName }} · {{ customer.id.slice(0, 8) }}
                </option>
              </select>
            </label>

            <label class="space-y-1">
              <span class="font-mono text-2xs text-text-muted">REGION</span>
              <input
                v-model="region"
                class="w-full border border-border bg-transparent px-3 py-2 font-mono text-sm text-text-primary"
                placeholder="业务地区，例如 Shanghai"
              />
            </label>
          </div>

          <CalendarPanel
            v-model:selected-start="selectedStart"
            v-model:selected-end="selectedEnd"
          />
        </section>

        <section class="card">
          <h2 class="panel-title">A-02 · 型号与配件</h2>
          <DeviceAccessoryPanel
            v-model:model-counts="selectedDevices"
            v-model:selected-accessories="selectedAccessories"
          />
          <p v-if="unresolvedModels.length" class="mt-3 font-mono text-xs text-[var(--color-danger)]">
            MODEL MAP ERROR · {{ unresolvedModels.join(' / ') }}
          </p>
        </section>

        <section class="card">
          <h2 class="panel-title">A-03 · 订单备注</h2>
          <textarea
            v-model="notes"
            rows="4"
            class="w-full border border-border bg-transparent px-3 py-2 font-mono text-sm text-text-primary resize-y"
            placeholder="备注只在 CreateOrderFromQuote 时写入；订单号由服务器生成"
          />
        </section>
      </div>

      <div class="space-y-4 min-w-0">
        <section class="card">
          <div class="flex items-center justify-between gap-3 mb-3">
            <h2 class="panel-title mb-0">B-01 · 服务器报价</h2>
            <span class="font-mono text-2xs text-text-muted">CLIENT PRICE AUTHORITY: NONE</span>
          </div>

          <div v-if="!offeredQuote" class="py-8 text-center border border-dashed border-border">
            <p class="font-mono text-sm text-text-muted">尚未生成 Quote</p>
            <p class="font-mono text-2xs text-text-muted mt-2">型号价格与配件价格全部由服务器解析</p>
          </div>

          <template v-else>
            <div class="grid grid-cols-2 gap-2 font-mono text-xs mb-4">
              <span class="text-text-muted">QUOTE</span><span class="text-right break-all">{{ offeredQuote.id }}</span>
              <span class="text-text-muted">STATUS</span><span class="text-right uppercase">{{ offeredQuote.status }}</span>
              <span class="text-text-muted">EXPIRES</span><span class="text-right">{{ offeredQuote.expiresAt }}</span>
            </div>

            <div class="space-y-2">
              <div
                v-for="line in offeredQuote.lines"
                :key="line.id"
                class="border border-border p-3 font-mono text-xs"
              >
                <div class="flex justify-between gap-3">
                  <span>{{ line.kind.toUpperCase() }} · {{ line.description }}</span>
                  <strong>{{ money(line.subtotal.minor, line.subtotal.currency) }}</strong>
                </div>
                <div class="flex justify-between gap-3 mt-1 text-text-muted">
                  <span>QTY {{ line.quantity }}</span>
                  <span>UNIT {{ money(line.unitPrice.minor, line.unitPrice.currency) }}</span>
                </div>
              </div>
            </div>

            <div class="flex items-end justify-between mt-4 pt-4 border-t border-border">
              <span class="font-mono text-xs text-text-muted">SERVER TOTAL</span>
              <strong class="font-mono text-2xl">{{ money(offeredQuote.total.minor, offeredQuote.total.currency) }}</strong>
            </div>
          </template>

          <button
            class="btn-primary w-full mt-4"
            :disabled="working"
            @click="generateQuote"
          >
            {{ working ? '处理中...' : offeredQuote ? '重新生成报价' : '生成服务器报价' }}
          </button>
        </section>

        <section class="card">
          <h2 class="panel-title">B-02 · Structured Confirmation</h2>
          <div class="font-mono text-xs space-y-2">
            <div class="flex justify-between"><span class="text-text-muted">CUSTOMER</span><span>{{ customerId || '-' }}</span></div>
            <div class="flex justify-between"><span class="text-text-muted">DATE</span><span>{{ selectedStart || '-' }} → {{ selectedEnd || '-' }}</span></div>
            <div class="flex justify-between"><span class="text-text-muted">REGION</span><span>{{ region || '-' }}</span></div>
            <div class="flex justify-between"><span class="text-text-muted">MODELS</span><span>{{ modelLines.length }}</span></div>
            <div class="flex justify-between"><span class="text-text-muted">ACCESSORIES</span><span>{{ accessoryLines.reduce((sum, line) => sum + line.quantity, 0) }}</span></div>
            <div class="flex justify-between"><span class="text-text-muted">DEVICE SERIALS</span><span>NOT ALLOCATED</span></div>
            <div class="flex justify-between"><span class="text-text-muted">ORDER NO</span><span>SERVER GENERATED</span></div>
          </div>

          <button
            class="btn-primary w-full mt-4"
            :disabled="working || !offeredQuote || offeredQuote.status !== 'draft'"
            @click="acceptAndCreateOrder"
          >
            {{ working ? '处理中...' : '接受报价并创建订单' }}
          </button>
        </section>

        <section v-if="createdOrder" class="card">
          <h2 class="panel-title">B-03 · ORDER CREATED</h2>
          <div class="font-mono text-sm space-y-2">
            <div class="flex justify-between gap-3"><span class="text-text-muted">ORDER NO</span><strong>{{ createdOrder.orderNo }}</strong></div>
            <div class="flex justify-between gap-3"><span class="text-text-muted">ORDER ID</span><span class="break-all text-right">{{ createdOrder.orderId }}</span></div>
            <div class="flex justify-between gap-3"><span class="text-text-muted">TOTAL</span><strong>{{ money(createdOrder.total.minor, createdOrder.total.currency) }}</strong></div>
          </div>
          <button class="btn-ghost w-full mt-4" @click="goToOrders">进入订单工作台</button>
        </section>
      </div>
    </div>
  </div>
</template>
