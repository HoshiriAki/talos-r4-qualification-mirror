<script setup lang="ts">
import { reactive, computed } from 'vue'

const props = defineProps<{
  orderNo: string
  address: string
  notes: string
  pickupMethod: string
  provinces: string[]
  receiveArea: string
  returnArea: string
  receiveProvince: string
  returnProvince: string
}>()

const emit = defineEmits<{
  'update:orderNo': [value: string]
  'update:address': [value: string]
  'update:notes': [value: string]
  'update:pickupMethod': [value: string]
  'update:receiveArea': [value: string]
  'update:returnArea': [value: string]
  'update:receiveProvince': [value: string]
  'update:returnProvince': [value: string]
}>()

const errors = reactive<Record<string, string>>({})

function validate(field: string, value: string) {
  switch (field) {
    case 'orderNo':
      if (!value.trim()) errors.orderNo = '订单号不能为空'
      else if (!/^\d+$/.test(value.trim())) errors.orderNo = '订单号必须为纯数字'
      else delete errors.orderNo
      break
    case 'address':
      if (!value.trim()) errors.address = '地址不能为空'
      else delete errors.address
      break
    default:
      break
  }
}

function onBlur(field: string, value: string) {
  validate(field, value)
}

function update(field: string, value: string) {
  // Clear error on input
  if (errors[field]) delete errors[field]
  const emitMap: Record<string, string> = {
    orderNo: 'update:orderNo',
    address: 'update:address',
    notes: 'update:notes',
  }
  emit(emitMap[field] as any, value)
}

const pickupOptions = [
  { value: '顺丰标快', label: '顺丰标快' },
  { value: '自取/跑腿', label: '自取/跑腿' },
  { value: '半日达', label: '半日达' },
]

const receiveAreas = ['区域1', '区域1.5', '区域2', '区域3']
const returnAreas = ['区域1', '区域2', '区域3']

// Province hints matching original transfer-system area tooltips
const areaProvinceHints: Record<string, Record<string, string>> = {
  receive: {
    '区域1': '上海 江苏 广东 浙江 安徽 江西',
    '区域1.5': '湖南 湖北 山东 福建 河南',
    '区域2': '成都 重庆 北京 天津 河北 海南 广西',
    '区域3': '吉林 辽宁 黑龙江 贵州 云南 山西 陕西 内蒙古',
  },
  return: {
    '区域1': '上海 江苏 广东 浙江 安徽 江西 湖南 湖北 山东 福建 河南',
    '区域2': '成都 重庆 北京 天津 河北 海南 广西',
    '区域3': '吉林 辽宁 黑龙江 贵州 云南 山西 陕西 内蒙古',
  },
}

// City-to-province aliases (original hints use city names for some entries)
const cityAliases: Record<string, string> = {
  '成都': '四川',
}

// Filter provinces by area — match short keywords against full province names
function filterProvinces(fullList: string[], area: string, mode: string) {
  if (!area) return fullList
  const hint = areaProvinceHints[mode]?.[area]
  if (!hint) return fullList
  const keywords = hint.split(/\s+/).map(k => cityAliases[k] ?? k)
  return fullList.filter(p => keywords.some(k => p.includes(k)))
}

const filteredReceiveProvinces = computed(() =>
  filterProvinces(props.provinces, props.receiveArea, 'receive')
)
const filteredReturnProvinces = computed(() =>
  filterProvinces(props.provinces, props.returnArea, 'return')
)

</script>

<template>
  <div class="space-y-3">
    <!-- Order No -->
    <div>
      <label class="mono-label block mb-1">订单号</label>
      <InputText
        :model-value="orderNo"
        class="w-full"
        :class="{ '!border-[var(--color-btn-danger)] bg-[var(--color-btn-danger-bg)]': errors.orderNo }"
        placeholder="请输入订单号（纯数字）"
        @update:model-value="(v: string | undefined) => update('orderNo', v ?? '')"
        @blur="() => onBlur('orderNo', orderNo)"
      />
      <p v-if="errors.orderNo" class="text-xs font-mono text-[var(--color-btn-danger)] mt-1">{{ errors.orderNo }}</p>
    </div>

    <!-- Area selection: two columns -->
    <div class="flex gap-3">
      <!-- Receive area -->
      <div class="flex-1 border-2 border-border p-2">
        <label class="mono-label block mb-1.5 font-bold">收货区域</label>
        <div class="flex gap-1">
          <div class="flex flex-wrap gap-1 flex-1">
            <button
              v-for="a in receiveAreas"
              :key="a"
              type="button"
              :title="areaProvinceHints.receive[a]"
              class="font-mono text-xs font-bold px-3 py-1.5 border-2 transition-colors select-none cursor-pointer"
              :class="receiveArea === a
                ? 'btn-primary border-[var(--text-accent)]'
                : 'border-border text-text-secondary hover:border-[var(--text-accent)]'"
              @click="emit('update:receiveArea', receiveArea === a ? '' : a)"
            >
              {{ a }}
            </button>
          </div>
          <Select
            :model-value="receiveProvince"
            :options="filteredReceiveProvinces.map(p => ({ label: p, value: p }))"
            option-label="label"
            option-value="value"
            :placeholder="receiveArea ? '省份' : '全部省份'"
            class="w-1/3 shrink-0"
            @update:model-value="emit('update:receiveProvince', $event)"
          />
        </div>
      </div>

      <!-- Return area -->
      <div class="flex-1 border-2 border-border p-2">
        <label class="mono-label block mb-1.5 font-bold">寄回区域</label>
        <div class="flex gap-1">
          <div class="flex flex-wrap gap-1 flex-1">
            <button
              v-for="a in returnAreas"
              :key="a"
              type="button"
              :title="areaProvinceHints.return[a]"
              class="font-mono text-xs font-bold px-3 py-1.5 border-2 transition-colors select-none cursor-pointer"
              :class="returnArea === a
                ? 'btn-primary border-[var(--text-accent)]'
                : 'border-border text-text-secondary hover:border-[var(--text-accent)]'"
              @click="emit('update:returnArea', returnArea === a ? '' : a)"
            >
              {{ a }}
            </button>
          </div>
          <Select
            :model-value="returnProvince"
            :options="filteredReturnProvinces.map(p => ({ label: p, value: p }))"
            option-label="label"
            option-value="value"
            :placeholder="returnArea ? '省份' : '全部省份'"
            class="w-1/3 shrink-0"
            @update:model-value="emit('update:returnProvince', $event)"
          />
        </div>
      </div>
    </div>

    <!-- Pickup Method -->
    <div>
      <label class="mono-label block mb-1">取货方式</label>
      <div class="flex gap-1">
        <Button
          v-for="opt in pickupOptions"
          :key="opt.value"
          :severity="pickupMethod === opt.value ? 'primary' : 'secondary'"
          :label="opt.label"
          class="flex-1"
          @click="emit('update:pickupMethod', pickupMethod === opt.value ? '' : opt.value)"
        />
      </div>
    </div>

    <!-- Address -->
    <div>
      <label class="mono-label block mb-1">地址</label>
      <InputText
        :model-value="address"
        class="w-full"
        :class="{ '!border-[var(--color-btn-danger)] bg-[var(--color-btn-danger-bg)]': errors.address }"
        placeholder="请输入地址"
        @update:model-value="(v: string | undefined) => update('address', v ?? '')"
        @blur="() => onBlur('address', address)"
      />
      <p v-if="errors.address" class="text-xs font-mono text-[var(--color-btn-danger)] mt-1">{{ errors.address }}</p>
    </div>

    <!-- Notes -->
    <div>
      <label class="mono-label block mb-1">备注</label>
      <Textarea
        :model-value="notes"
        class="w-full"
        placeholder="可选"
        rows="2"
        auto-resize
        @update:model-value="(v: string | undefined) => update('notes', v ?? '')"
      />
    </div>
  </div>
</template>
