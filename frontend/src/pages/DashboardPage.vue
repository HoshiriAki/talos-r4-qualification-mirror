<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, inject, defineComponent, h } from 'vue'
import { RouterLink } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useToast } from 'primevue/usetoast'
import { Icon } from '@iconify/vue'
import { fetchDashboardStats, type DashboardStats } from '@/api/dashboard'
import CadLoading from '@/components/common/CadLoading.vue'
import { HEADER_ACTIONS_KEY } from '@/constants/keys'
import { shanghaiBusinessDate } from '@/utils/businessDate'
import { useUiModeStore } from '@/stores/uiMode'
import { useTenantPreviewStore } from '@/stores/tenantPreview'
import DashboardWorkSurface from '@/components/dashboard/DashboardWorkSurface.vue'
import DashboardHudSurface from '@/components/dashboard/DashboardHudSurface.vue'

const uiMode = useUiModeStore()
const tenantPreview = useTenantPreviewStore()
const { t } = useI18n()
const toast = useToast()

const stats = ref<DashboardStats | null>(null)
const loading = ref(true)
const error = ref('')
const activeTab = ref('all')
const abort = ref<AbortController | null>(null)
const retrying = ref(false)
const svgTimer = ref<ReturnType<typeof setTimeout> | null>(null)

function classifyError(e: unknown): string {
  if (e instanceof DOMException && e.name === 'AbortError') return ''
  if (e instanceof TypeError && e.message === 'Failed to fetch') return t('common.networkError')
  const msg = (e as any)?.message || ''
  if (msg.includes('401')) return t('common.authError')
  if (msg.includes('403')) return t('common.forbidden')
  if (msg.includes('500') || msg.includes('502') || msg.includes('503')) return t('common.serverError')
  return msg || t('common.error')
}

const tabs = computed(() => [
  { key: 'all', label: t('dash.tabAll'), count: () => stats.value?.recentOrders.length ?? 0 },
  { key: 'active', label: t('dash.tabActive'), count: () => stats.value?.activeOrders ?? 0 },
  { key: 'pending', label: t('dash.tabPending'), count: () => 0 },
  { key: 'overdue', label: t('dash.tabOverdue'), count: () => stats.value?.overdueReturns ?? 0 },
  { key: 'completed', label: t('dash.tabCompleted'), count: () => 0 },
])

async function loadData() {
  // Cancel any in-flight request
  if (abort.value) { abort.value.abort(); abort.value = null }
  abort.value = new AbortController()

  loading.value = true
  error.value = ''
  try {
    stats.value = await fetchDashboardStats(abort.value?.signal)
  } catch (e: unknown) {
    error.value = classifyError(e)
  } finally {
    loading.value = false
    retrying.value = false
    abort.value = null
  }
}

async function retry() {
  if (retrying.value) return
  retrying.value = true
  await loadData()
}

// ── Header Actions — injected into DefaultLayout navbar ────────────────────
const DashboardHeaderActions = defineComponent({
  name: 'DashboardHeaderActions',
  setup() {
    const exporting = ref(false)
    const { t: _t } = useI18n()
    const _toast = useToast()
    const preview = useTenantPreviewStore()

    async function handleExport() {
      exporting.value = true
      try {
        const { exportRevenueReport } = await import('@/api/reports')
        const blob = await exportRevenueReport()
        const url = URL.createObjectURL(blob)
        const a = document.createElement('a')
        a.href = url
        a.download = `revenue-report-${shanghaiBusinessDate()}.xlsx`
        a.click()
        URL.revokeObjectURL(url)
      } catch (_) {
        _toast.add({ severity: 'error', summary: _t('common.exportError'), life: 4000 })
      } finally {
        exporting.value = false
      }
    }

    return () => preview.active
      ? null
      :
      h('div', { class: 'topbar-actions', style: 'display:flex; gap:8px;' }, [
        h('button', { class: 'header-action-btn', onClick: handleExport, disabled: exporting.value }, [
          h(Icon, { icon: 'material-symbols:download', width: 16, height: 16 }),
          h('span', _t('dash.exportReport')),
        ]),
        h(RouterLink, { class: 'header-action-btn header-action-btn--primary', to: '/price' }, [
          h(Icon, { icon: 'material-symbols:add', width: 18, height: 18 }),
          h('span', _t('dash.newOrder')),
        ]),
      ])
  },
})
// Inject the ancestor-owned ref and mount the header actions
const headerActionsRef = inject(HEADER_ACTIONS_KEY)
onMounted(() => { if (headerActionsRef) headerActionsRef.value = DashboardHeaderActions })
onUnmounted(() => { if (headerActionsRef) headerActionsRef.value = null })

const devicesOutCount = computed(() => stats.value?.devicesOut ?? 0)
const totalDevices = computed(() => stats.value?.totalDevices ?? 0)
const currentDeviceCount = computed(() => `${devicesOutCount.value} / ${totalDevices.value}`)

// ── Isometric SVG scene ──────────────────────────────────────────────────
function renderIsoScene(svgId: string) {
  const svg = document.getElementById(svgId)
  if (!svg) return
  const targetSvg = svg

  const ns = 'http://www.w3.org/2000/svg'
  const tw = 66, th = 33
  const originX = 620, originY = 260

  function project(c: number, r: number): [number, number] {
    return [originX + (c - r) * (tw / 2), originY + (c + r) * (th / 2)]
  }

  function isoCube(c: number, r: number, height: number, tones: [string, string, string]) {
    const [x0, y0] = project(c, r)
    const hw = tw / 2, hh = th / 2
    const top = [x0, y0 - hh - height] as [number, number]
    const right: [number, number] = [x0 + hw, y0 - height]
    const bottom: [number, number] = [x0, y0 + hh - height]
    const left: [number, number] = [x0 - hw, y0 - height]
    const groundBottom: [number, number] = [x0, y0 + hh]
    const groundLeft: [number, number] = [x0 - hw, y0]
    const groundRight: [number, number] = [x0 + hw, y0]
    const g = document.createElementNS(ns, 'g')
    const mk = (pts: [number, number][], fill: string) => {
      const p = document.createElementNS(ns, 'polygon')
      p.setAttribute('points', pts.map(pt => pt.join(',')).join(' '))
      p.setAttribute('fill', fill)
      p.setAttribute('stroke', '#0B0B0D')
      p.setAttribute('stroke-width', '1.2')
      g.appendChild(p)
    }
    mk([top, right, bottom, left], tones[0])
    mk([left, bottom, groundBottom, groundLeft], tones[1])
    mk([right, bottom, groundBottom, groundRight], tones[2])
    targetSvg.appendChild(g)
  }

  const TOP = '#E9E9EB', LEFT = '#B7B7BC', RIGHT = '#87878C'
  const TOP_D = '#CFCFD2', LEFT_D = '#9C9CA1', RIGHT_D = '#6F6F74'

  const layout = [
    [0, 0, 1.0, 0], [1, 0, 1.0, 0], [2, 0, 1.5, 0], [3, 0, 1.0, 0],
    [0, 1, 1.0, 0], [1, 1, 0.6, 1], [2, 1, 1.5, 0], [3, 1, 1.0, 0],
    [0, 2, 0.6, 1], [1, 2, 1.0, 0], [2, 2, 1.0, 0], [3, 2, 0.6, 1],
    [1, 3, 0.6, 1], [2, 3, 0.6, 1],
  ]
  layout.forEach(([c, r, hRatio, dark]) => {
    isoCube(c as number, r as number, (hRatio as number) * 54, dark ? [TOP_D, LEFT_D, RIGHT_D] : [TOP, LEFT, RIGHT])
  })

  const p1 = project(0, 0)
  const p2 = project(3, 0)
  const ridge = document.createElementNS(ns, 'line')
  ridge.setAttribute('x1', String(p1[0]))
  ridge.setAttribute('y1', String(p1[1] - 86))
  ridge.setAttribute('x2', String(p2[0]))
  ridge.setAttribute('y2', String(p2[1] - 86))
  ridge.setAttribute('stroke', '#0B0B0D')
  ridge.setAttribute('stroke-width', '9')
  svg.appendChild(ridge)

  const boltPositions = [[1, 0, 1.5], [2, 1, 1.8], [1, 2, 1.0], [3, 1, 1.0], [0, 1, 1.0]]
  const boltPts = boltPositions.map(([c, r, hR]) => {
    const [x, y] = project(c as number, r as number)
    return [x, y - (hR as number) * 54] as [number, number]
  })
  for (let i = 0; i < boltPts.length - 1; i++) {
    const l = document.createElementNS(ns, 'line')
    l.setAttribute('x1', String(boltPts[i][0]))
    l.setAttribute('y1', String(boltPts[i][1]))
    l.setAttribute('x2', String(boltPts[i + 1][0]))
    l.setAttribute('y2', String(boltPts[i + 1][1]))
    l.setAttribute('stroke', '#3D3D42')
    l.setAttribute('stroke-width', '1')
    l.setAttribute('stroke-dasharray', '1 4')
    svg.appendChild(l)
  }
  boltPts.forEach(([x, y], i) => {
    const isAlert = i === 2
    const c = document.createElementNS(ns, 'circle')
    c.setAttribute('cx', String(x))
    c.setAttribute('cy', String(y))
    c.setAttribute('r', isAlert ? '6' : '4.5')
    c.setAttribute('fill', isAlert ? '#FF453A' : '#0B0B0D')
    c.setAttribute('stroke', '#E9E9EB')
    c.setAttribute('stroke-width', '1.3')
    svg.appendChild(c)
  })

  for (let c = -1; c <= 1; c++) {
    for (let r = 4; r <= 6; r++) {
      const [x, y] = project(c, r)
      const d = document.createElementNS(ns, 'circle')
      d.setAttribute('cx', String(x))
      d.setAttribute('cy', String(y))
      d.setAttribute('r', '2')
      d.setAttribute('fill', '#242427')
      svg.appendChild(d)
    }
  }
}

onMounted(() => {
  loadData()
  svgTimer.value = setTimeout(() => renderIsoScene('isoScene'), 100)
})
onUnmounted(() => {
  if (abort.value) { abort.value.abort(); abort.value = null }
  if (svgTimer.value) { clearTimeout(svgTimer.value); svgTimer.value = null }
})
</script>

<template>
  <!-- Loading / Error (shared) -->
  <div v-if="loading" class="pt-12 flex justify-center"><CadLoading /></div>
  <div v-else-if="error" class="panel p-4 flex items-center gap-3 max-w-md">
    <Icon icon="material-symbols:warning" width="18" height="18" class="text-status-error shrink-0" />
    <p class="text-sm flex-1">{{ error }}</p>
    <button class="btn-secondary text-xs px-3 py-1.5" :disabled="retrying" @click="retry">{{ retrying ? t('common.retrying') : t('common.retry') }}</button>
  </div>

  <!-- Work surface: existing hero + KPIs + table -->
  <div v-else-if="stats && uiMode.mode === 'work'" class="dashboard-content">
      <!-- ═══ Hero Banner ═══ -->
      <div class="hero-banner">
        <div class="hero-grid"></div>
        <svg id="isoScene" viewBox="0 0 1400 900" preserveAspectRatio="xMidYMid meet"></svg>

        <div class="hero-copy">
          <div>
            <div class="hero-eyebrow">MODULE-01 · {{ shanghaiBusinessDate() }}</div>
            <div class="hero-title">{{ t('dash.inOperation') }}<small>{{ currentDeviceCount }} {{ t('dash.deviceCount') }}</small></div>
          </div>
        </div>

        <div class="hero-chip chip-tr">
          <div><div class="lab">{{ t('dash.monthlyOut') }}</div><div class="val">{{ stats.devicesOut }} / {{ stats.totalDevices }}</div></div>
        </div>
        <div class="hero-chip chip-br">
          <div>
            <div class="lab">{{ t('dash.overdue') }}</div>
            <div class="val" :class="stats.overdueReturns > 0 ? 'text-accent' : ''">{{ stats.overdueReturns }} {{ t('dash.units') }}</div>
          </div>
          <span v-if="stats.overdueReturns > 0" class="chip-alert">!</span>
        </div>
      </div>

      <!-- ═══ KPI Strip ═══ -->
      <div class="kpi-strip">
        <div class="kpi-item">
          <div class="kpi-num up">{{ stats.activeOrders }}<span v-if="stats.todayNewOrders > 0" class="kpi-delta up">+{{ stats.todayNewOrders }}</span></div>
          <div class="kpi-lab">{{ t('dash.activeOrders') }}</div>
        </div>
        <div class="kpi-item">
          <div class="kpi-num up">{{ Math.round((stats.devicesOut / Math.max(stats.totalDevices, 1)) * 100) }}<small>%</small></div>
          <div class="kpi-lab">{{ t('dash.utilization') }}</div>
        </div>
        <div class="kpi-item">
          <div class="kpi-num up">{{ stats.availableDevices }}<small> {{ t('dash.availableUnit') }}</small></div>
          <div class="kpi-lab">{{ t('dash.available') }}</div>
        </div>
        <div class="kpi-item">
          <div class="kpi-num up">{{ stats.todayNewOrders }}<span v-if="stats.returnsDueToday > 0" class="kpi-delta down">{{ stats.returnsDueToday }} {{ t('dash.due') }}</span></div>
          <div class="kpi-lab">{{ t('dash.todayNew') }}</div>
        </div>
      </div>

      <!-- ═══ Toolbar ═══ -->
      <div class="toolbar">
        <div class="tabs">
          <button
            v-for="tab in tabs"
            :key="tab.key"
            class="tab"
            :class="{ active: activeTab === tab.key }"
            @click="activeTab = tab.key"
          >
            {{ tab.label }}
            <span class="n" :class="{ accent: tab.key === 'overdue' && stats.overdueReturns > 0 }">{{ tab.count() }}</span>
          </button>
        </div>
        <button class="btn-secondary text-xs px-3 py-1.5">
          {{ t('dash.filter') }} ▾
        </button>
      </div>

      <!-- ═══ Order Data Table ═══ -->
      <div class="table-card">
        <table>
          <thead>
            <tr>
              <th>{{ t('dash.device') }}</th>
              <th>{{ t('dash.orderNo') }}</th>
              <th>{{ t('dash.customer') }}</th>
              <th>{{ t('dash.status') }}</th>
              <th>{{ t('dash.amount') }}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr v-if="stats.recentOrders.length === 0">
              <td colspan="6" class="text-center py-8 text-text-tertiary text-sm">
                {{ t('dash.noOrders') }}
              </td>
            </tr>
            <tr v-for="order in stats.recentOrders" :key="order.id">
              <td class="device-cell">
                <div class="device-thumb" :aria-label="t('dash.deviceIcon')">
                  <Icon icon="material-symbols:videocam" width="16" height="16" />
                </div>
                <div style="min-width:0">
                  <div class="device-name truncate-cell">{{ order.orderNo }}</div>
                  <div class="device-code">{{ order.province || '-' }}</div>
                </div>
              </td>
              <td class="mono">
                <span class="truncate-cell" style="display:block;max-width:140px">{{ order.orderNo }}</span>
              </td>
              <td><span class="truncate-cell" style="display:block;max-width:120px">{{ order.province || '—' }}</span></td>
              <td>
                <span class="status-pill progress" role="status">
                  <i aria-hidden="true"></i>{{ t('dash.inUse') }}
                </span>
              </td>
              <td class="amount">¥{{ order.totalPrice?.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 }) ?? '0.00' }}</td>
              <td>
                <router-link :to="tenantPreview.routeFor('orders')" class="row-link" :aria-label="t('dash.detail')">{{ t('dash.detail') }} →</router-link>
              </td>
            </tr>
          </tbody>
        </table>
        <div class="table-foot">
          <span>{{ t('dash.showing', { from: 1, to: stats.recentOrders.length, total: stats.recentOrders.length }) }}</span>
          <div class="pager">
            <button>‹</button>
            <button class="active">1</button>
            <button>›</button>
          </div>
        </div>
      </div>
    </div>

    <!-- HUD surface: SVG scene + alerts + task rail -->
    <DashboardHudSurface
      v-else-if="stats"
      :model="stats"
      :loading="false"
      :compact="uiMode.mode === 'hud-compact'"
    />
</template>

<style scoped>
/* ═══ Content Container ═══ */
.dashboard-content {
  display: flex;
  flex-direction: column;
  gap: 28px;
  padding-bottom: 60px;
}

/* ═══ Hero Banner ═══ */
.hero-banner {
  position: relative;
  background: var(--bg-surface);
  border: 1px solid var(--border-base);
  border-radius: 14px;
  overflow: hidden;
  height: 280px;
  display: flex;
  align-items: stretch;
}

.hero-banner::after {
  content: '';
  position: absolute;
  inset: 0;
  pointer-events: none;
  background: radial-gradient(90% 120% at 68% 40%, transparent 55%, rgba(0, 0, 0, 0.45) 100%);
}

.hero-grid {
  position: absolute;
  inset: 0;
  background-image:
    linear-gradient(rgba(255, 255, 255, 0.025) 1px, transparent 1px),
    linear-gradient(90deg, rgba(255, 255, 255, 0.025) 1px, transparent 1px);
  background-size: 28px 28px;
}

#isoScene {
  position: absolute;
  right: -40px;
  top: -20px;
  width: 640px;
  height: 340px;
  z-index: 1;
}

.hero-copy {
  position: relative;
  z-index: 3;
  padding: 28px 32px;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  max-width: 280px;
}

.hero-eyebrow {
  font-size: 11px;
  letter-spacing: 0.14em;
  color: var(--text-tertiary);
  text-transform: uppercase;
  font-family: var(--font-mono);
}

.hero-title {
  font-family: var(--font-sans);
  font-size: 40px;
  font-weight: 800;
  letter-spacing: -0.02em;
  margin-top: 10px;
  line-height: 1.05;
}

.hero-title small {
  display: block;
  font-family: var(--font-mono);
  font-weight: 700;
  font-size: 13px;
  color: var(--text-secondary);
  margin-top: 10px;
  letter-spacing: 0.02em;
}

.hero-chip {
  position: absolute;
  z-index: 3;
  background: var(--bg-elevated);
  border: 1px solid var(--border-base);
  border-radius: 9px;
  padding: 9px 13px;
  display: flex;
  align-items: center;
  gap: 8px;
}

.chip-tr { top: 24px; right: 32px; }
.chip-br { bottom: 24px; right: 32px; }

.hero-chip .lab {
  font-size: 10px;
  color: var(--text-tertiary);
  letter-spacing: 0.06em;
  text-transform: uppercase;
  font-family: var(--font-mono);
}

.hero-chip .val {
  font-family: var(--font-mono);
  font-weight: 700;
  font-size: 14px;
  margin-top: 1px;
}

.chip-alert {
  min-width: 15px;
  height: 15px;
  padding: 0 4px;
  border-radius: 8px;
  background: var(--accent);
  color: #fff;
  font-size: 9px;
  font-family: var(--font-mono);
  display: flex;
  align-items: center;
  justify-content: center;
  margin-left: 6px;
}

/* ═══ KPI Strip ═══ */
.kpi-strip {
  display: flex;
  gap: 40px;
  padding: 4px;
}

.kpi-item {
  border-left: 1px solid var(--border-base);
  padding-left: 16px;
}

.kpi-num {
  font-family: var(--font-mono);
  font-weight: 700;
  font-size: 22px;
}

.kpi-num.up { color: var(--text-primary); }

.kpi-num small {
  font-size: 12px;
  color: var(--text-tertiary);
  font-weight: 400;
  margin-left: 3px;
}

.kpi-lab {
  font-size: 12px;
  color: var(--text-secondary);
  margin-top: 3px;
}

.kpi-delta {
  font-size: 11px;
  margin-left: 6px;
  font-family: var(--font-mono);
}

.kpi-delta.up { color: var(--status-success); }
.kpi-delta.down { color: var(--accent); }

/* ═══ Toolbar ═══ */
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: 14px;
}

.tabs {
  display: flex;
  gap: 6px;
  background: var(--bg-surface);
  border: 1px solid var(--border-base);
  border-radius: 10px;
  padding: 4px;
}

.tab {
  font-size: 12px;
  padding: 7px 14px;
  border-radius: 7px;
  color: var(--text-tertiary);
  display: flex;
  align-items: center;
  gap: 6px;
  border: none;
  background: transparent;
  cursor: pointer;
  transition: background 140ms ease, color 140ms ease;
  font-family: var(--font-sans);
}

.tab:hover {
  color: var(--text-secondary);
}

.tab.active {
  background: var(--bg-elevated);
  color: var(--text-primary);
}

.tab .n {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--text-tertiary);
}

.tab .n.accent { color: var(--accent); }
.tab.active .n { color: var(--text-secondary); }

/* ═══ Data Table ═══ */
.table-card {
  background: var(--bg-surface);
  border: 1px solid var(--border-base);
  border-radius: 14px;
  overflow: hidden;
}

table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}

thead th {
  font-size: 11px;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-tertiary);
  text-align: left;
  padding: 14px 20px;
  border-bottom: 1px solid var(--border-base);
  font-weight: 500;
  font-family: var(--font-mono);
}

thead th:last-child,
tbody td:last-child {
  text-align: right;
}

tbody td {
  padding: 14px 20px;
  border-bottom: 1px solid var(--border-base);
  color: var(--text-secondary);
}

tbody tr:last-child td {
  border-bottom: none;
}

tbody tr:hover {
  background: var(--bg-elevated);
}

.device-cell {
  display: flex;
  align-items: center;
  gap: 10px;
}

.device-thumb {
  width: 32px;
  height: 32px;
  border-radius: 7px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-base);
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  color: var(--text-tertiary);
}

.device-name {
  font-family: var(--font-mono);
  font-size: 12.5px;
  color: var(--text-primary);
}

.device-code {
  font-size: 11px;
  color: var(--text-tertiary);
  margin-top: 1px;
}

.amount {
  font-family: var(--font-mono);
  color: var(--text-primary);
}

.status-pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  padding: 4px 10px;
  border-radius: 7px;
}

.status-pill.progress {
  color: var(--text-primary);
  background: var(--bg-elevated);
}

.status-pill.done {
  color: var(--text-secondary);
  background: var(--bg-elevated);
}

.status-pill.overdue {
  color: var(--accent);
  background: var(--accent-muted);
}

.status-pill i {
  width: 6px;
  height: 6px;
  border-radius: var(--radius-sm);
  background: currentColor;
}

.truncate-cell {
  max-width: 180px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row-link {
  font-size: 12px;
  color: var(--text-tertiary);
  text-decoration: none;
}

.row-link:hover {
  color: var(--text-primary);
}

.mono {
  font-family: var(--font-mono);
}

.table-foot {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 14px 20px;
  font-size: 12px;
  color: var(--text-tertiary);
}

.pager {
  display: flex;
  gap: 6px;
}

.pager button {
  width: 28px;
  height: 28px;
  border-radius: 7px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-base);
  color: var(--text-secondary);
  font-size: 12px;
  cursor: pointer;
  font-family: var(--font-mono);
  transition: border-color 140ms ease, color 140ms ease;
}

.pager button:hover {
  color: var(--text-primary);
  border-color: var(--text-secondary);
}

.pager button.active {
  color: var(--text-primary);
  border-color: var(--text-secondary);
}

/* ═══ Responsive ═══ */
@media (max-width: 1100px) {
  .hero-banner {
    height: auto;
    flex-direction: column;
  }

  #isoScene {
    position: relative;
    width: 100%;
    height: 220px;
    right: 0;
    top: 0;
  }

  .hero-copy {
    max-width: none;
  }

  .kpi-strip {
    flex-wrap: wrap;
    gap: 24px;
  }
}

@media (prefers-reduced-motion: reduce) {
  * { transition: none !important; }
}
</style>
