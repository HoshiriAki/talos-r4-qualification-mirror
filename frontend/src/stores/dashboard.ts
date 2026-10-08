import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { useAuthStore } from './auth'

export interface WidgetConfig {
  id: string
  col: number
  row: number
  w: number
  h: number
}

const STAT_CARD_IDS = [
  'active-orders',
  'devices-out',
  'available-devices',
  'today-orders',
  'returns-due',
  'overdue',
  'total-devices',
  'utilization-rate',
]

const CHART_IDS = [
  'order-trend',
  'revenue-trend',
  'cancel-trend',
  'device-status',
  'lost-devices',
  'province-pie',
  'province-trend',
  'utilization',
  'model-ranking',
  'warehouse-stats',
]

const RECENT_ORDERS_ID = 'recent-orders'

export const ALL_WIDGET_IDS = [...STAT_CARD_IDS, ...CHART_IDS, RECENT_ORDERS_ID]

const rolePresets: Record<string, WidgetConfig[]> = {
  warehouse: [
    // Row 1: Hero chart (8 columns) + stat cards (4 columns)
    { id: 'device-status', col: 1, row: 1, w: 6, h: 2 },
    { id: 'utilization', col: 7, row: 1, w: 6, h: 2 },
    // Row 2: Secondary stats
    { id: 'lost-devices', col: 1, row: 3, w: 6, h: 2 },
    { id: 'warehouse-stats', col: 7, row: 3, w: 6, h: 2 },
    { id: 'recent-orders', col: 1, row: 5, w: 12, h: 1 },
  ],
  sales: [
    // Row 1: KPI stat cards (4 small + Hero chart)
    { id: 'active-orders', col: 1, row: 1, w: 3, h: 1 },
    { id: 'devices-out', col: 4, row: 1, w: 3, h: 1 },
    { id: 'available-devices', col: 7, row: 1, w: 3, h: 1 },
    { id: 'today-orders', col: 10, row: 1, w: 3, h: 1 },
    // Row 2: Hero trend (8 col) + province (4 col)
    { id: 'order-trend', col: 1, row: 2, w: 8, h: 2 },
    { id: 'province-pie', col: 9, row: 2, w: 4, h: 2 },
    // Row 3: Revenue (8 col) + returns-due (4 col)
    { id: 'revenue-trend', col: 1, row: 4, w: 8, h: 2 },
    { id: 'utilization', col: 9, row: 4, w: 4, h: 2 },
    { id: 'recent-orders', col: 1, row: 6, w: 12, h: 1 },
  ],
  admin: [
    // Row 1: 4 KPI cards (3-col each)
    { id: 'active-orders', col: 1, row: 1, w: 3, h: 1 },
    { id: 'devices-out', col: 4, row: 1, w: 3, h: 1 },
    { id: 'available-devices', col: 7, row: 1, w: 3, h: 1 },
    { id: 'today-orders', col: 10, row: 1, w: 3, h: 1 },
    // Row 2: Orders trend (7 col) + device status (5 col)
    { id: 'order-trend', col: 1, row: 2, w: 7, h: 2 },
    { id: 'device-status', col: 8, row: 2, w: 5, h: 2 },
    // Row 3: Revenue (7 col) + cancel (5 col)
    { id: 'revenue-trend', col: 1, row: 4, w: 7, h: 2 },
    { id: 'cancel-trend', col: 8, row: 4, w: 5, h: 2 },
    // Row 4: Stats row (3+3+3+3)
    { id: 'lost-devices', col: 1, row: 6, w: 3, h: 1 },
    { id: 'province-pie', col: 4, row: 6, w: 3, h: 1 },
    { id: 'model-ranking', col: 7, row: 6, w: 3, h: 1 },
    { id: 'warehouse-stats', col: 10, row: 6, w: 3, h: 1 },
    { id: 'recent-orders', col: 1, row: 7, w: 12, h: 1 },
  ],
  finance: [
    { id: 'revenue-trend', col: 1, row: 1, w: 8, h: 3 },
    { id: 'cancel-trend', col: 9, row: 1, w: 4, h: 3 },
    { id: 'model-ranking', col: 1, row: 4, w: 6, h: 2 },
    { id: 'province-pie', col: 7, row: 4, w: 6, h: 2 },
  ],
  compact: [
    { id: 'active-orders', col: 1, row: 1, w: 4, h: 1 },
    { id: 'devices-out', col: 5, row: 1, w: 4, h: 1 },
    { id: 'available-devices', col: 9, row: 1, w: 4, h: 1 },
    { id: 'order-trend', col: 1, row: 2, w: 8, h: 2 },
    { id: 'revenue-trend', col: 9, row: 2, w: 4, h: 2 },
    { id: 'recent-orders', col: 1, row: 4, w: 12, h: 1 },
  ],
}

export const useDashboardStore = defineStore('dashboard', () => {
  const auth = useAuthStore()
  const isEditing = ref(false)
  const layout = ref<WidgetConfig[]>([])

  function loadLayout() {
    try {
      const saved = localStorage.getItem('talos-dashboard-layout')
      if (saved) {
        const parsed = JSON.parse(saved)
        if (Array.isArray(parsed) && parsed.length > 0) {
          layout.value = parsed as WidgetConfig[]
          return
        }
      }
    } catch {
      // Fall through to role preset
    }
    const role = auth.tenantRole === 'admin' || auth.tenantRole === 'owner' ? 'admin' : 'sales'
    layout.value = rolePresets[role] || rolePresets.sales
  }

  function saveLayout() {
    try {
      localStorage.setItem('talos-dashboard-layout', JSON.stringify(layout.value))
    } catch {
      // Silently fail on quota exceeded
    }
  }

  function resetLayout() {
    const role = auth.tenantRole === 'admin' || auth.tenantRole === 'owner' ? 'admin' : 'sales'
    layout.value = rolePresets[role] || rolePresets.sales
    localStorage.removeItem('talos-dashboard-layout')
  }

  function addWidget(widgetId: string) {
    if (layout.value.some(w => w.id === widgetId)) return
    // Place at the bottom of the grid
    const maxRow = layout.value.reduce((max, w) => Math.max(max, w.row + w.h), 1)
    layout.value.push({ id: widgetId, col: 1, row: maxRow, w: 2, h: 1 })
    saveLayout()
  }

  function removeWidget(widgetId: string) {
    layout.value = layout.value.filter(w => w.id !== widgetId)
    saveLayout()
  }

  function moveWidget(widgetId: string, col: number, row: number) {
    const w = layout.value.find(w => w.id === widgetId)
    if (w) {
      w.col = Math.max(1, col)
      w.row = Math.max(1, row)
      saveLayout()
    }
  }

  function resizeWidget(widgetId: string, w: number, h: number) {
    const widget = layout.value.find(w => w.id === widgetId)
    if (widget) {
      widget.w = Math.max(1, Math.min(4, w))
      widget.h = Math.max(1, Math.min(4, h))
      saveLayout()
    }
  }

  const unusedWidgets = computed(() => {
    const used = new Set(layout.value.map(w => w.id))
    return ALL_WIDGET_IDS.filter(id => !used.has(id))
  })

  return {
    isEditing,
    layout,
    loadLayout,
    saveLayout,
    resetLayout,
    addWidget,
    removeWidget,
    moveWidget,
    resizeWidget,
    unusedWidgets,
    rolePresets,
  }
})
