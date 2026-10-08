<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick, watch } from 'vue'
import { useToast } from 'primevue/usetoast'
import { useConfirm } from 'primevue/useconfirm'
import { Html5Qrcode, Html5QrcodeSupportedFormats } from 'html5-qrcode'
import type { Html5QrcodeResult } from 'html5-qrcode'
import * as ordersApi from '@/api/orders'
import type { Order } from '@/api/orders'
import { shanghaiBusinessDate } from '@/utils/businessDate'
import Timeline from 'primevue/timeline'

const toast = useToast()
const confirm = useConfirm()

// ── Step / view control ──
const showShipping = ref(false)

// ── Prep state ──
interface PrepOrder {
  orderNo: string
  trackingNo: string
  remark: string
  order: Order | null   // populated after API validation
  validated: boolean
}
const batchInput = ref('')
const importing = ref(false)
const prepOrders = ref<PrepOrder[]>([])
const offlineMode = ref(false)

// ── Shipping state ──
interface QueueEntry {
  order: Order
  processed: boolean
  trackingNo: string        // expected tracking number (from prep)
  remark: string
  scannedTrackingNo: string // tracked tracking number from scan
  trackingVerified: boolean  // scanned tracking matches expected
  scannedDevices: string[]
  ready: boolean
  skipped: boolean
}
const queue = ref<QueueEntry[]>([])
const currentIdx = ref<number | null>(null)
const scanning = ref(false)
const confirming = ref(false)
const cameraReady = ref(false)
const cameraStarting = ref(false)
const cameraError = ref<string | null>(null)
const manualInput = ref('')
const lastScannedAt = ref(0)
const showSummary = ref(false)
const shippingStartedAt = ref(0)
const hasRestoredSession = ref(false)

const STORAGE_KEY = 'talos-shipping-state'

function saveStateToLocalStorage() {
  const state = {
    prepOrders: prepOrders.value,
    queue: queue.value,
    currentIdx: currentIdx.value,
    shippingStartedAt: shippingStartedAt.value,
    showShipping: showShipping.value,
    showSummary: showSummary.value,
  }
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state))
  } catch { /* quota exceeded — silently ignore */ }
}

let saveTimer: ReturnType<typeof setTimeout> | null = null
function debouncedSave() {
  if (saveTimer) clearTimeout(saveTimer)
  saveTimer = setTimeout(() => saveStateToLocalStorage(), 200)
}

function loadStateFromLocalStorage(): boolean {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return false
    const state = JSON.parse(raw)
    if (!state || typeof state !== 'object') return false
    // 2-小时会话过期：超过2小时自动清除旧状态
    if (state.shippingStartedAt && Date.now() - state.shippingStartedAt > 2 * 60 * 60 * 1000) {
      clearStoredState()
      return false
    }
    prepOrders.value = Array.isArray(state.prepOrders) ? state.prepOrders : []
    queue.value = Array.isArray(state.queue) ? state.queue : []
    currentIdx.value = typeof state.currentIdx === 'number' ? state.currentIdx : null
    shippingStartedAt.value = typeof state.shippingStartedAt === 'number' ? state.shippingStartedAt : 0
    showShipping.value = !!state.showShipping
    showSummary.value = !!state.showSummary
    return true
  } catch {
    return false
  }
}

function clearStoredState() {
  try { localStorage.removeItem(STORAGE_KEY) } catch {}
}

watch(
  () => ({
    prepOrders: prepOrders.value,
    queue: queue.value.map(e => ({
      order: e.order,
      processed: e.processed,
      trackingNo: e.trackingNo,
      scannedTrackingNo: e.scannedTrackingNo,
      trackingVerified: e.trackingVerified,
      scannedDevices: e.scannedDevices,
      ready: e.ready,
      remark: e.remark,
    })),
    currentIdx: currentIdx.value,
    shippingStartedAt: shippingStartedAt.value,
    showShipping: showShipping.value,
    showSummary: showSummary.value,
  }),
  () => { debouncedSave() },
  { deep: true }
)

let scanner: Html5Qrcode | null = null
let currentCameraId: string | null = null

// ── Camera zoom state ──
const zoomSupported = ref(false)
const zoomValue = ref(1)
const zoomMin = ref(1)
const zoomMax = ref(3)
const zoomStep = ref(0.1)
let zoomCapability: any = null
const autoZoomEnabled = ref(false)
let lastBoundsSeenAt = 0
let autoZoomTimer: ReturnType<typeof setInterval> | null = null

// ── Computed ──

const prepStats = computed(() => {
  const valid = prepOrders.value.filter(p => p.validated).length
  const invalid = prepOrders.value.length - valid
  return { valid, invalid, total: prepOrders.value.length }
})

const pendingCount = computed(() => queue.value.filter(e => !e.ready && !e.skipped).length)
const processedCount = computed(() => queue.value.filter(e => e.ready).length)
const skippedCount = computed(() => queue.value.filter(e => e.skipped).length)

const currentStep = computed(() => {
  if (!showShipping.value) return 1
  if (showSummary.value) return 3
  return 2
})

const sortedQueue = computed(() => {
  const ready = queue.value.filter(e => e.ready)
  const skipped = queue.value.filter(e => e.skipped && !e.ready)
  const pending = queue.value.filter(e => !e.ready && !e.skipped)
  return [...ready, ...skipped, ...pending]
})

const currentOrder = computed(() => {
  if (currentIdx.value === null) return null
  return queue.value[currentIdx.value]?.order ?? null
})

const currentEntry = computed(() => {
  if (currentIdx.value === null) return null
  return queue.value[currentIdx.value] ?? null
})

// ── Summary stats ──

const shippedOrders = computed(() => queue.value.filter(e => e.processed))
const readyOrders = computed(() => queue.value.filter(e => e.ready && !e.processed))
const shippedWithTracking = computed(() => [...readyOrders.value, ...shippedOrders.value].filter(e => e.trackingNo).length)
const reviewCount = computed(() => shippedOrders.value.length > 0 ? shippedOrders.value.length : readyOrders.value.length)
const shippingDuration = computed(() => {
  if (!shippingStartedAt.value) return '-'
  const secs = Math.round((Date.now() - shippingStartedAt.value) / 1000)
  if (secs < 60) return `${secs}s`
  return `${Math.floor(secs / 60)}m ${secs % 60}s`
})

// ── Mobile queue collapsed view (±2 around current, rest folded) ──
const entriesBeforeCurrent = computed(() => {
  if (currentIdx.value === null) return []
  return queue.value.slice(0, currentIdx.value)
})

const entriesAfterCurrent = computed(() => {
  if (currentIdx.value === null) return queue.value
  return queue.value.slice(currentIdx.value + 1)
})

// Past: show ALL entries before current (regardless of ready), limit to last 2 visible
const mobileVisiblePast = computed(() => {
  const past = entriesBeforeCurrent.value
  return past.length <= 2 ? past : past.slice(-2)
})

// Future: show ALL entries after current (regardless of ready), limit to first 2 visible
const mobileVisibleFuture = computed(() => {
  const future = entriesAfterCurrent.value
  return future.length <= 2 ? future : future.slice(0, 2)
})

const mobileHiddenPastCount = computed(() => Math.max(0, entriesBeforeCurrent.value.length - 2))
const mobileHiddenFutureCount = computed(() => Math.max(0, entriesAfterCurrent.value.length - 2))

// Past fold badge: counts for hidden (folded) entries before current
const mobilePastCompletedInHidden = computed(() => {
  const past = entriesBeforeCurrent.value
  if (past.length <= 2) return 0
  return past.slice(0, -2).filter(e => e.ready).length
})
const mobilePastSkippedInHidden = computed(() => {
  const past = entriesBeforeCurrent.value
  if (past.length <= 2) return 0
  return past.slice(0, -2).filter(e => e.skipped).length
})

// ── Timeline ──

const timelineSteps = [
  { label: '导入订单' },
  { label: '扫描并确认' },
  { label: '发货完成' },
]

function stepClass(index: number): string {
  const s = index + 1
  if (s < currentStep.value) return 'done'
  if (s === currentStep.value) return 'active'
  return 'next'
}

// ── Prep methods ──

// ── Courier prefix detection ──
// Known courier tracking number prefixes. Case-insensitive match.
const COURIER_PREFIXES = [
  'SF',   // 顺丰
  'JD',   // 京东
  'JDE',  // 京东企业
  'YT',   // 圆通
  'YTO',  // 圆通
  'ZT',   // 中通
  'ZTO',  // 中通
  'ST',   // 申通
  'STO',  // 申通
  'HT',   // 百世汇通
  'DB',   // 德邦
  'DPK',  // 德邦快递
  'EMS',  // 邮政EMS
  'YZ',   // 邮政
  'YUNDA',// 韵达
  'YD',   // 韵达
  'JT',   // 极兔
  'JNT',  // 极兔
  'BEST', // 百世
  'DHL',  // DHL
  'UPS',  // UPS
  'FEDEX',// FedEx
  'TNT',  // TNT
]

/** Detect if a token looks like a courier tracking number (starts with known prefix + digit) */
function isCourierTrackingNo(token: string): boolean {
  const upper = token.toUpperCase()
  return COURIER_PREFIXES.some(p => upper.startsWith(p) && /\d/.test(upper.slice(p.length)))
}

/**
 * Parse batch text into PrepOrder[].
 * Each line can be:
 *   "订单号 快递单号"           (default — no courier prefix detected)
 *   "订单号 [备注...] 快递单号"  (courier prefix detected, remark in between)
 *   "快递单号 [备注...] 订单号"  (courier prefix first, remark in between)
 *   "订单号"                    (no tracking number)
 */
function parseBatch(text: string): PrepOrder[] {
  return text
    .split('\n')
    .map(line => line.trim())
    .filter(line => line.length > 0)
    .map(line => {
      const parts = line.split(/\s+/)
      if (parts.length === 1) {
        return { orderNo: parts[0], trackingNo: '', remark: '' }
      }

      // Check each part for a courier tracking number prefix
      const courierIdx = parts.findIndex(p => isCourierTrackingNo(p))

      if (courierIdx !== -1) {
        // Courier prefix detected → that part is trackingNo
        const trackingNo = parts[courierIdx]

        if (courierIdx === 0) {
          // Format: "快递单号 [备注...] 订单号"
          const orderNo = parts[parts.length - 1]
          const remark = parts.slice(1, -1).join(' ')
          return { orderNo, trackingNo, remark }
        } else {
          // Format: "订单号 [备注...] 快递单号"
          const orderNo = parts[0]
          const remark = parts.slice(1, courierIdx).join(' ')
          return { orderNo, trackingNo, remark }
        }
      }

      // Default: first part = orderNo, rest = trackingNo, no remark
      return { orderNo: parts[0], trackingNo: parts.slice(1).join(' '), remark: '' }
    })
    .filter(item => item.orderNo.length > 0)
    .map(item => ({ ...item, order: null, validated: false }))
}

function genUUID(): string {
  if (typeof crypto !== 'undefined' && crypto.randomUUID) return crypto.randomUUID()
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, c => {
    const r = Math.random() * 16 | 0
    return (c === 'x' ? r : (r & 0x3 | 0x8)).toString(16)
  })
}

function makeOfflineOrder(orderNo: string): Order {
  return { id: `offline-${genUUID()}`, orderNo, devices: [], status: 'reserved', _offline: true } as unknown as Order
}

async function importBatch() {
  const orders = parseBatch(batchInput.value)
  if (orders.length === 0) {
    toast.add({ severity: 'warn', summary: '无数据', detail: '请粘贴至少一个订单号', life: 3000 })
    return
  }
  importing.value = true
  batchInput.value = ''

  if (offlineMode.value) {
    for (const po of orders) {
      prepOrders.value.push({
        ...po,
        order: makeOfflineOrder(po.orderNo),
        validated: true,
      })
    }
    importing.value = false
    toast.add({ severity: 'success', summary: '已导入（离线）', detail: `导入 ${orders.length} 个订单，未校验`, life: 2000 })
    return
  }

  let found = 0
  let missing = 0

  for (const po of orders) {
    try {
      const result = await ordersApi.fetchPage({ orderNo: po.orderNo }, 1, 1)
      if (result.orders && result.orders.length > 0) {
        prepOrders.value.push({ ...po, order: result.orders[0], validated: true })
        found++
      } else {
        prepOrders.value.push({ ...po, order: null, validated: false })
        missing++
      }
    } catch {
      prepOrders.value.push({ ...po, order: null, validated: false })
      missing++
    }
  }

  importing.value = false
  if (missing > 0 && found > 0) {
    toast.add({ severity: 'warn', summary: '部分导入', detail: `成功 ${found} 个，${missing} 个未找到`, life: 3000 })
  } else if (found > 0) {
    toast.add({ severity: 'success', summary: '已导入', detail: `成功导入 ${found} 个订单`, life: 2000 })
  } else {
    toast.add({ severity: 'error', summary: '导入失败', detail: '所有订单号均未找到', life: 4000 })
  }
}

function removePrepRow(index: number) {
  prepOrders.value.splice(index, 1)
}

async function switchToShipping() {
  const valid = prepOrders.value.filter(p => p.validated)
  if (valid.length === 0) {
    toast.add({ severity: 'warn', summary: '无有效订单', detail: '请先导入至少一个有效订单', life: 3000 })
    return
  }
  shippingStartedAt.value = Date.now()
  showShipping.value = true
  await nextTick()
  startCamera()
  // Pre-load validated prep orders directly into queue (already fetched from API)
  for (const po of prepOrders.value) {
    if (!po.validated || !po.order) continue
    const existing = findQueueIndex(po.orderNo)
    if (existing !== -1) {
      currentIdx.value = existing
      continue
    }
    queue.value.push({
      order: po.order,
      processed: false,
      trackingNo: po.trackingNo,
      remark: po.remark || '',
      scannedTrackingNo: '',
      trackingVerified: false,
      scannedDevices: [],
      ready: false,
      skipped: false,
    })
    if (currentIdx.value === null) currentIdx.value = queue.value.length - 1
  }
}

// ── Camera ──

function classifyCameraError(e: any): string {
  const msg = e?.message || e?.toString?.() || ''
  const name = e?.name || ''

  if (name === 'NotAllowedError' || msg.includes('NotAllowed') || msg.includes('Permission')) {
    const isHttp = typeof location !== 'undefined' && location.protocol === 'http:' && location.hostname !== 'localhost' && location.hostname !== '127.0.0.1'
    if (isHttp) return '摄像头需要 HTTPS 安全连接。当前为 HTTP，浏览器已阻止摄像头访问。请使用 HTTPS 访问，或通过手动输入订单号操作。'
    return '摄像头权限被拒绝。请在浏览器设置中允许本站访问摄像头，或使用手动输入。'
  }
  if (name === 'NotFoundError' || msg.includes('NotFound') || msg.includes('No camera')) {
    return '未检测到摄像头设备。请确认设备已连接摄像头，或使用手动输入订单号。'
  }
  if (name === 'NotReadableError' || msg.includes('NotReadable') || msg.includes('in use')) {
    return '摄像头被其他应用占用。请关闭其他使用摄像头的应用后刷新页面。'
  }
  if (name === 'OverconstrainedError' || msg.includes('Overconstrained')) {
    return '摄像头不支持要求的参数。将尝试使用前置摄像头。'
  }
  return `摄像头启动失败：${msg || '未知错误'}。可通过下方手动输入继续操作。`
}

async function startCamera() {
  if (cameraStarting.value) return
  cameraStarting.value = true
  try {
    scanner = new Html5Qrcode('camera-container', {
      formatsToSupport: [
        Html5QrcodeSupportedFormats.QR_CODE,
        Html5QrcodeSupportedFormats.CODE_128,
        Html5QrcodeSupportedFormats.EAN_13,
        Html5QrcodeSupportedFormats.CODE_39,
        Html5QrcodeSupportedFormats.CODABAR,
        Html5QrcodeSupportedFormats.DATA_MATRIX,
      ],
      useBarCodeDetectorIfSupported: true,
      verbose: false,
    })

    const scanConfig = {
      fps: 10,
      qrbox: { width: 250, height: 250 },
      aspectRatio: 1.333,
    }

    const onSuccess = (decodedText: string, result: Html5QrcodeResult) => {
      handleScanResult(decodedText, result)
    }

    // If we already know which camera was in use (e.g. user switched, or
    // visibility-change restart), try that exact device first.
    if (currentCameraId) {
      try {
        await scanner.start(
          { deviceId: { exact: currentCameraId } },
          scanConfig,
          onSuccess,
          () => {}
        )
        cameraReady.value = true
        cameraError.value = null
        initZoom()
        applyMaxResolution()
        return
      } catch {
        // device gone — fall through to constraint-based search
        try { await scanner.stop() } catch {}
        scanner = new Html5Qrcode('camera-container', {
          formatsToSupport: [
            Html5QrcodeSupportedFormats.QR_CODE,
            Html5QrcodeSupportedFormats.CODE_128,
            Html5QrcodeSupportedFormats.EAN_13,
            Html5QrcodeSupportedFormats.CODE_39,
            Html5QrcodeSupportedFormats.CODABAR,
            Html5QrcodeSupportedFormats.DATA_MATRIX,
          ],
          useBarCodeDetectorIfSupported: true,
          verbose: false,
        })
        currentCameraId = null
      }
    }

    // First launch or camera-id miss — discover via facingMode fallback
    const constraints: MediaTrackConstraints[] = [
      { facingMode: 'environment' },
      { facingMode: 'user' },
      { facingMode: { ideal: 'environment' } },
    ]

    let lastError: any = null
    for (const c of constraints) {
      try {
        await scanner.start(
          c,
          scanConfig,
          onSuccess,
          () => {}
        )
        cameraReady.value = true
        cameraError.value = null
        initZoom()
        applyMaxResolution()
        return
      } catch (err: any) {
        lastError = err
        try { await scanner.stop() } catch {}
        scanner = new Html5Qrcode('camera-container', {
          formatsToSupport: [
            Html5QrcodeSupportedFormats.QR_CODE,
            Html5QrcodeSupportedFormats.CODE_128,
            Html5QrcodeSupportedFormats.EAN_13,
            Html5QrcodeSupportedFormats.CODE_39,
            Html5QrcodeSupportedFormats.CODABAR,
            Html5QrcodeSupportedFormats.DATA_MATRIX,
          ],
          useBarCodeDetectorIfSupported: true,
          verbose: false,
        })
      }
    }

    cameraReady.value = false
    cameraError.value = classifyCameraError(lastError)
  } catch (e: any) {
    cameraReady.value = false
    cameraError.value = classifyCameraError(e)
  } finally {
    cameraStarting.value = false
  }
}

async function stopCamera() {
  stopAutoZoom()
  zoomSupported.value = false
  zoomValue.value = 1
  zoomCapability = null
  if (scanner) {
    try { await scanner.stop() } catch {}
    scanner = null
  }
  cameraReady.value = false
}

function initZoom() {
  try {
    const caps = scanner?.getRunningTrackCameraCapabilities()
    if (!caps) return
    const zf = caps.zoomFeature()
    if (!zf.isSupported()) return
    zoomCapability = zf
    zoomMin.value = zf.min()
    zoomMax.value = zf.max()
    zoomStep.value = zf.step()
    const current = zf.value()
    if (current !== null) zoomValue.value = current
    zoomSupported.value = true
  } catch {
    zoomSupported.value = false
    zoomCapability = null
  }
}

async function onZoomChange(val: number) {
  if (!zoomCapability) return
  autoZoomEnabled.value = false
  try {
    await zoomCapability.apply(val)
    zoomValue.value = val
  } catch {
    // zoom apply failed — may be temporarily unavailable
  }
}

async function applyMaxResolution() {
  if (!scanner) return
  try {
    const caps = scanner.getRunningTrackCapabilities()
    const maxW = (caps as any).width?.max
    const maxH = (caps as any).height?.max
    if (!maxW || !maxH) return
    const settings = scanner.getRunningTrackSettings()
    const curW = (settings as any).width ?? 0
    const curH = (settings as any).height ?? 0
    // Only re-apply if camera can deliver significantly more than current
    if (maxW > curW * 1.15 || maxH > curH * 1.15) {
      await scanner.applyVideoConstraints({
        width: { ideal: maxW },
        height: { ideal: maxH },
      })
    }
  } catch {
    // non-critical — keep whatever resolution the browser chose
  }
}

async function switchCamera() {
  try {
    const cameras = await Html5Qrcode.getCameras()
    if (cameras.length < 2) {
      toast.add({ severity: 'info', summary: '仅有一个摄像头', detail: '没有其他可用的摄像头', life: 2000 })
      return
    }
    await stopCamera()
    const curIdx = cameras.findIndex(c => c.id === currentCameraId)
    const nextIdx = (curIdx + 1) % cameras.length
    currentCameraId = cameras[nextIdx].id

    scanner = new Html5Qrcode('camera-container', {
      formatsToSupport: [
        Html5QrcodeSupportedFormats.QR_CODE,
        Html5QrcodeSupportedFormats.CODE_128,
        Html5QrcodeSupportedFormats.EAN_13,
        Html5QrcodeSupportedFormats.CODE_39,
        Html5QrcodeSupportedFormats.CODABAR,
        Html5QrcodeSupportedFormats.DATA_MATRIX,
      ],
      useBarCodeDetectorIfSupported: true,
      verbose: false,
    })
    const onSuccess = (decodedText: string, result: Html5QrcodeResult) => {
      handleScanResult(decodedText, result)
    }
    await scanner.start(
      { deviceId: { exact: currentCameraId } },
      { fps: 10, qrbox: { width: 250, height: 250 }, aspectRatio: 1.333 },
      onSuccess,
      () => {}
    )
    cameraReady.value = true
    cameraError.value = null
    initZoom()
  } catch (e: any) {
    cameraReady.value = false
    cameraError.value = e.message || '切换摄像头失败'
    toast.add({ severity: 'error', summary: '切换失败', detail: e.message, life: 4000 })
  }
}

// ── SF Express QR code parser ──
// Format: MMM={'k1':'...','k2':'...','k3':'...','k4':'...','k5':'SF1234567890','k6':'...','k7':'...'}
// Padded to 128 chars total. k5 = tracking number.
function parseSfMmmQr(text: string): string | null {
  // Extract the MMM={...} block
  const m = /MMM\s*=\s*\{([^}]+)\}/.exec(text)
  if (!m) return null

  const body = m[1]
  // Parse single-quoted key-value pairs
  const k5m = /'k5'\s*:\s*'([^']*)'/.exec(body)
  if (k5m && k5m[1].trim()) return k5m[1].trim()

  return null
}

/**
 * Scan filter — classify scanned text into tracking / serial / invalid.
 */
function classifyScan(text: string): { type: 'tracking' | 'serial' | 'invalid'; value: string } {
  const v = text.trim()
  if (!v) return { type: 'invalid', value: '' }

  // 1. SF Express MMM QR code (k5 = tracking number)
  const sfTracking = parseSfMmmQr(v)
  if (sfTracking) return { type: 'tracking', value: sfTracking }

  // 2. Known courier prefix (SF/JD/YT etc. + digit)
  if (isCourierTrackingNo(v)) return { type: 'tracking', value: v }

  // 3. Device serial — alphanumeric 8+ chars, no CJK
  if (/^[0-9A-Za-z]{8,}$/.test(v)) return { type: 'serial', value: v }

  // 4. Invalid — too short or contains Chinese
  if (v.length < 4 || /[一-鿿]/.test(v)) return { type: 'invalid', value: v }

  // Fallback: treat as serial
  return { type: 'serial', value: v }
}

function handleScanResult(raw: string, result?: Html5QrcodeResult) {
  const now = Date.now()
  if (now - lastScannedAt.value < 1500) return
  lastScannedAt.value = now
  // Feed bounds to auto-zoom if available
  if (result?.result?.bounds && zoomCapability && !autoZoomEnabled.value) {
    processAutoZoomBounds(result.result.bounds)
  }
  doScan(raw)
}

// ── Auto-zoom ──

function processAutoZoomBounds(bounds: { x: number; y: number; width: number; height: number }) {
  lastBoundsSeenAt = Date.now()
  if (autoZoomTimer) return // already running
  autoZoomEnabled.value = true
  autoZoomTimer = setInterval(() => autoZoomTick(bounds), 300)
}

function autoZoomTick(latestBounds: { x: number; y: number; width: number; height: number }) {
  if (!zoomCapability) { stopAutoZoom(); return }

  // If no bounds seen for 3 seconds, reset zoom to 1× and stop
  if (Date.now() - lastBoundsSeenAt > 3000) {
    zoomCapability.apply(zoomMin.value).catch(() => {})
    zoomValue.value = zoomMin.value
    stopAutoZoom()
    return
  }

  // Compute target zoom: QR area should fill ~25% of the scan box area (250×250)
  const scanBoxArea = 250 * 250
  const qrArea = latestBounds.width * latestBounds.height
  if (qrArea <= 0) return

  const ratio = qrArea / scanBoxArea
  const TARGET = 0.25
  const currentZ = zoomValue.value
  const ideal = currentZ * Math.sqrt(TARGET / Math.max(ratio, 0.01))
  const target = Math.max(zoomMin.value, Math.min(zoomMax.value, ideal))

  // Smooth lerp — move 30% toward target each tick
  const newZ = currentZ + (target - currentZ) * 0.3
  const rounded = Math.round(newZ * 100) / 100

  if (Math.abs(rounded - zoomValue.value) > 0.01) {
    zoomCapability.apply(rounded).catch(() => {})
    zoomValue.value = rounded
  }
}

function stopAutoZoom() {
  if (autoZoomTimer) { clearInterval(autoZoomTimer); autoZoomTimer = null }
  autoZoomEnabled.value = false
}

// ── Core ──

function findQueueIndex(orderNo: string): number {
  return queue.value.findIndex(e => e.order.orderNo === orderNo)
}

async function doScan(raw: string) {
  if (!raw || scanning.value) return

  if (currentIdx.value === null) {
    toast.add({ severity: 'warn', summary: '未选择订单', detail: '请先在队列中选择一个订单', life: 3000 })
    return
  }

  const entry = queue.value[currentIdx.value]
  if (entry.ready) return

  const result = classifyScan(raw)
  scanning.value = true
  manualInput.value = ''

  if (result.type === 'invalid') {
    toast.add({ severity: 'warn', summary: '无效数据', detail: `无法识别的扫描内容: ${result.value}`, life: 2000 })
    scanning.value = false
    return
  }

  if (result.type === 'tracking') {
    const scannedNo = result.value

    // Priority 1: matches current entry's trackingNo → verify
    if (entry.trackingNo && scannedNo === entry.trackingNo) {
      entry.scannedTrackingNo = scannedNo
      entry.trackingVerified = true
      toast.add({ severity: 'success', summary: '快递单号已验证', detail: `${scannedNo} 与订单匹配 ✓`, life: 1500 })
      scanning.value = false
      return
    }

    // Priority 2: search entire queue for matching trackingNo → jump
    const matchIdx = queue.value.findIndex(e => e.trackingNo && e.trackingNo === scannedNo)
    if (matchIdx !== -1) {
      currentIdx.value = matchIdx
      toast.add({ severity: 'info', summary: '已跳转', detail: `快递单号 ${scannedNo} → 订单 ${queue.value[matchIdx].order.orderNo}`, life: 2000 })
      scanning.value = false
      return
    }

    // Priority 3: no match anywhere
    toast.add({ severity: 'warn', summary: '未找到订单', detail: `快递单号 ${scannedNo} 在队列中未找到对应订单`, life: 3000 })
    scanning.value = false
    return
  }

  // Serial device
  const serialNo = result.value
  if (entry.scannedDevices.includes(serialNo)) {
    toast.add({ severity: 'warn', summary: '已扫描', detail: `设备 ${serialNo} 已在列表中`, life: 2000 })
    scanning.value = false
    return
  }

  entry.scannedDevices.push(serialNo)
  toast.add({ severity: 'success', summary: '已记录', detail: `设备 ${serialNo} → 订单 ${entry.order.orderNo}`, life: 1000 })
  scanning.value = false
}

/** Whether the current entry is ready to confirm (tracking verified + at least one device scanned) */
function isCurrentEntryConfirmable(): boolean {
  if (currentIdx.value === null) return false
  const e = queue.value[currentIdx.value]
  return !e.ready && e.scannedDevices.length > 0 && e.trackingVerified
}

async function confirmShip() {
  if (confirming.value || currentIdx.value === null) return
  const entry = queue.value[currentIdx.value]

  // Validation: tracking must be verified and at least one device scanned
  if (entry.scannedDevices.length === 0) {
    toast.add({ severity: 'warn', summary: '未扫描设备', detail: '请先扫描或输入设备序列号', life: 2000 })
    return
  }
  if (!entry.trackingVerified) {
    toast.add({ severity: 'warn', summary: '快递单号未验证', detail: '请先扫描快递单号并校验通过', life: 2000 })
    return
  }
  if (entry.ready) return

  confirming.value = true
  entry.ready = true
  toast.add({ severity: 'success', summary: '已就绪', detail: `订单 ${entry.order.orderNo} 待最终提交`, life: 1500 })

  const nextIdx = queue.value.findIndex(e => !e.ready && !e.skipped)
  if (nextIdx !== -1) {
    currentIdx.value = nextIdx
  } else {
    currentIdx.value = null
    stopCamera()
    showSummary.value = true
  }
  confirming.value = false
}

function selectQueueEntry(idx: number) {
  currentIdx.value = idx
}

function skipOrder() {
  if (currentIdx.value === null) return
  const entry = queue.value[currentIdx.value]
  if (entry.ready || entry.skipped) return
  entry.skipped = true
  toast.add({ severity: 'info', summary: '已跳过', detail: `订单 ${entry.order.orderNo} 已标记为跳过`, life: 2000 })

  const nextIdx = queue.value.findIndex(e => !e.ready && !e.skipped)
  if (nextIdx !== -1) {
    currentIdx.value = nextIdx
  } else {
    currentIdx.value = null
    stopCamera()
    showSummary.value = true
  }
}

function unskipCurrentOrder() {
  if (currentIdx.value === null) return
  const entry = queue.value[currentIdx.value]
  if (!entry.skipped) return
  entry.skipped = false
  toast.add({ severity: 'info', summary: '已恢复', detail: `订单 ${entry.order.orderNo} 已取消跳过`, life: 2000 })
}

function removeScannedDevice(serialNo: string) {
  if (currentIdx.value === null) return
  const entry = queue.value[currentIdx.value]
  entry.scannedDevices = entry.scannedDevices.filter(s => s !== serialNo)
}

function onManualSubmit() {
  doScan(manualInput.value.trim())
}

// ── Submit all ──

const submitting = ref(false)

async function submitAllShipping() {
  if (submitting.value || readyOrders.value.length === 0) return
  submitting.value = true

  // Resolve offline stub IDs → real UUIDs from backend
  const offlineEntries = readyOrders.value.filter(e => (e.order as any)._offline)
  if (offlineEntries.length > 0) {
    try {
      const idMap = await ordersApi.batchOrderNos(offlineEntries.map(e => e.order.orderNo))
      for (const e of offlineEntries) {
        const realId = idMap[e.order.orderNo]
        if (realId) {
          e.order.id = realId
          ;(e.order as any)._offline = false
        }
      }
    } catch {
      // continue with offline IDs — backend will reject them, user sees errors per-order
    }
  }

  const payload = readyOrders.value.map(e => ({
    id: e.order.id,
    trackingNo: e.trackingNo,
    deviceSerialNos: e.scannedDevices,
  }))

  let success = 0
  let failed = 0

  try {
    const result = await ordersApi.submitBatchShip(payload)
    success = result.successCount
    failed = result.failCount

    // Mark successfully submitted orders as processed
    for (const r of result.results) {
      if (r.ok) {
        const entry = readyOrders.value.find(e => e.order.id === r.orderId)
        if (entry) entry.processed = true
      } else {
        toast.add({ severity: 'error', summary: `提交失败: ${r.orderId}`, detail: r.error || '未知错误', life: 5000 })
      }
    }
  } catch (e: any) {
    failed = readyOrders.value.length
    toast.add({ severity: 'error', summary: '批量提交失败', detail: e.message || '未知错误', life: 5000 })
  }

  submitting.value = false

  if (failed === 0) {
    toast.add({ severity: 'success', summary: '全部发货完成', detail: `成功提交 ${success} 个订单`, life: 3000 })
    clearStoredState()
  } else if (success > 0) {
    toast.add({ severity: 'warn', summary: '部分完成', detail: `成功 ${success} 个，失败 ${failed} 个`, life: 5000 })
  }
}

// ── Summary actions ──

function exportShippingCsv() {
  const rows = [...readyOrders.value, ...shippedOrders.value]
  const header = '订单号,快递单号,设备,发货时间'
  const lines = rows.map(e => {
    const time = new Date().toISOString()
    const allDevices = [...(e.order.devices || []), ...(e.scannedDevices || [])]
    const uniqueDevices = [...new Set(allDevices)]
    return `${e.order.orderNo},${e.trackingNo || ''},${uniqueDevices.join('/') || '-'},${time}`
  })
  const csv = '﻿' + [header, ...lines].join('\n')
  const blob = new Blob([csv], { type: 'text/csv;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `发货记录_${shanghaiBusinessDate()}.csv`
  a.click()
  URL.revokeObjectURL(url)
  toast.add({ severity: 'success', summary: '已导出', detail: `导出 ${rows.length} 条发货记录`, life: 2000 })
}

async function copyShippingData() {
  const rows = [...readyOrders.value, ...shippedOrders.value]
  const lines: string[] = []
  for (const e of rows) {
    const tracking = e.trackingNo || ''
    if (e.scannedDevices.length > 0) {
      for (const sn of e.scannedDevices) {
        lines.push(`${sn}\t${tracking}`)
      }
    } else {
      lines.push(`\t${tracking}`)
    }
  }
  if (lines.length === 0) {
    toast.add({ severity: 'warn', summary: '无数据', detail: '没有可复制的发货数据', life: 2000 })
    return
  }
  try {
    await navigator.clipboard.writeText(lines.join('\n'))
    toast.add({ severity: 'success', summary: '已复制', detail: `复制 ${lines.length} 条记录到剪贴板`, life: 2000 })
  } catch {
    toast.add({ severity: 'error', summary: '复制失败', detail: '剪贴板访问被拒绝', life: 3000 })
  }
}

// ── End task early ──

const endingTask = ref(false)

async function endTaskEarly() {
  if (endingTask.value || (pendingCount.value === 0 && skippedCount.value === 0)) return
  endingTask.value = true

  const unreadyTotal = pendingCount.value + skippedCount.value
  const skippedNote = skippedCount.value > 0 ? `（含 ${skippedCount.value} 个已跳过）` : ''

  // ── Dialog 1: initial confirmation ──
  confirm.require({
    message: `确定要结束任务吗？还有 ${unreadyTotal} 个订单未完成${skippedNote}。`,
    header: '结束发货任务',
    acceptLabel: '继续',
    rejectLabel: '取消',
    accept: () => {
      nextTick(() => {
        // ── Dialog 2: choose end mode ──
        confirm.require({
          message: `已完成 ${processedCount.value} 个订单${skippedNote ? '，跳过 ' + skippedCount.value + ' 个' : ''}。请选择结束方式：`,
          header: '选择结束方式',
          acceptLabel: '提前结束（保留已就绪）',
          rejectLabel: '清空所有订单',
          accept: () => {
            nextTick(() => {
              // ── Dialog 3a: confirm early end ──
              confirm.require({
                message: `已完成 ${processedCount.value} 个订单将进入汇总页，${unreadyTotal} 个未完成订单${skippedNote}将永久丢弃。确认？`,
                header: '提前结束 — 最终确认',
                acceptLabel: '确认结束',
                rejectLabel: '取消',
                accept: () => {
                  // Execute early end
                  queue.value = queue.value.filter(e => e.ready)
                  currentIdx.value = null
                  stopCamera()
                  showSummary.value = true
                  clearStoredState()
                  endingTask.value = false
                },
                reject: () => { endingTask.value = false },
              })
            })
          },
          reject: () => {
            nextTick(() => {
              // ── Dialog 3b: confirm clear all ──
              confirm.require({
                message: '所有订单和扫码进度将被清空，数据不可恢复。确认？',
                header: '清空任务 — 最终确认',
                acceptLabel: '确认清空',
                rejectLabel: '取消',
                accept: () => {
                  resetShipping()
                  endingTask.value = false
                },
                reject: () => { endingTask.value = false },
              })
            })
          },
        })
      })
    },
    reject: () => { endingTask.value = false },
  })
}

function resetShipping() {
  stopCamera()
  showShipping.value = false
  showSummary.value = false
  prepOrders.value = []
  queue.value = []
  currentIdx.value = null
  batchInput.value = ''
  manualInput.value = ''
  cameraError.value = null
  shippingStartedAt.value = 0
  clearStoredState()
}

// ── Lifecycle ──

onMounted(() => {
  const restored = loadStateFromLocalStorage()
  hasRestoredSession.value = restored
  if (restored && showShipping.value && !showSummary.value) {
    nextTick(() => {
      startCamera()
    })
  }
})

// Page visibility — re-activate camera when tab becomes visible again.
// Mobile browsers may silently kill the camera stream while backgrounded.
// Restart with the saved currentCameraId so we don't switch to a different lens.
function onVisibilityChange() {
  if (document.visibilityState === 'visible' && showShipping.value && !showSummary.value && !cameraStarting.value) {
    // Only restart if the scanner instance is gone or cameraReady flipped —
    // don't touch a working stream.
    if (scanner && cameraReady.value) return
    if (scanner) {
      try { scanner.pause(true) } catch {}
    }
    // Preserve currentCameraId (startCamera tries it first) and restart
    startCamera()
  }
}
document.addEventListener('visibilitychange', onVisibilityChange)

// Mobile Safari bfcache: pageshow fires when page is restored from cache
// (visibilitychange may not fire in this case)
function onPageShow(e: PageTransitionEvent) {
  if (e.persisted && showShipping.value && !showSummary.value && !cameraStarting.value) {
    if (scanner) {
      try { scanner.pause(true) } catch {}
    }
    startCamera()
  }
}
window.addEventListener('pageshow', onPageShow)

onUnmounted(() => {
  stopCamera()
  document.removeEventListener('visibilitychange', onVisibilityChange)
  window.removeEventListener('pageshow', onPageShow)
})
</script>

<template>
  <div class="flex flex-col gap-4 overflow-y-auto md:overflow-hidden" style="min-height: calc(100vh - 88px)">
    <!-- MODULE-06 bar -->
    <div class="module-bar mb-4">
      <span class="module-number-label">MODULE-06</span>
      <div class="structure-line" />
      <span class="module-page-label">发货管理</span>
    </div>

    <h1 class="font-mono font-bold text-2xl text-text-primary tracking-wider flex-shrink-0">发货管理</h1>

    <!-- Step Progress (PrimeVue Timeline horizontal) -->
    <div class="panel flex justify-center flex-shrink-0">
      <Timeline
        :value="timelineSteps"
        layout="horizontal"
        align="top"
        class="shipping-timeline"
      >
        <template #marker="{ index }">
          <div
            class="w-9 h-9 border-2 flex items-center justify-center text-sm font-bold transition-colors duration-micro mx-auto"
            :class="{
              'border-status-success text-status-success': stepClass(index) === 'done',
              'border-border-active text-text-primary bg-surface-hover': stepClass(index) === 'active',
              'border-border text-text-muted': stepClass(index) === 'next',
            }"
          >
            <span v-if="stepClass(index) === 'done'">&#10003;</span>
            <span v-else>{{ index + 1 }}</span>
          </div>
        </template>
        <template #content="{ item, index }">
          <span
            class="text-2xs whitespace-nowrap"
            :class="{
              'text-status-success': stepClass(index) === 'done',
              'text-text-primary font-bold': stepClass(index) === 'active',
              'text-text-muted': stepClass(index) === 'next',
            }"
          >{{ item.label }}</span>
        </template>
        <template #connector="{ index }">
          <span
            class="inline-block h-0.5"
            :class="index + 1 < currentStep ? 'bg-status-success' : 'bg-border'"
          />
        </template>
      </Timeline>
    </div>

    <!-- ══════════ STEP 1: PREP ══════════ -->
    <div v-if="!showShipping" class="panel flex flex-col md:flex-1 md:min-h-0 min-h-0">
      <div class="panel-title flex items-center justify-between mb-3 flex-shrink-0">
        <span>批量导入订单</span>
        <span class="font-normal text-text-muted text-xs">准备阶段 — 批量录入本次需发货的订单</span>
      </div>

      <!-- 恢复会话提示：页面刷新后自动恢复上次进度 -->
      <div v-if="hasRestoredSession" class="restored-session-banner" style="display:flex;align-items:center;gap:12px;padding:8px 12px;border:1px solid var(--border-default);margin-bottom:12px;">
        <span style="color:var(--text-secondary);font-size:13px;">已恢复上次的发货进度</span>
        <button class="btn btn-sm" @click="clearStoredState(); hasRestoredSession = false; resetShipping()">放弃上次进度</button>
      </div>

      <!-- Scrollable content area -->
      <div class="flex-1 min-h-0 overflow-y-auto">
        <!-- Batch textarea -->
        <div class="mb-4">
          <Textarea
            v-model="batchInput"
            rows="5"
            class="w-full"
            placeholder="批量粘贴：每行一条&#10;支持格式：订单号 [备注] 快递单号  或  快递单号 [备注] 订单号&#10;根据顺丰SF/京东JD/圆通YT等快递前缀自动识别&#10;例如：&#10;123401 加急 SF1234567890&#10;SF1234567891 张三 123402"
            auto-resize
          />
          <div class="flex justify-end mt-2">
            <div class="flex items-center gap-2">
              <Button severity="primary" :label="importing ? '检索中...' : '批量导入'" :disabled="importing" @click="importBatch" />
              <div class="flex items-center gap-1.5 ml-2 cursor-pointer select-none" @click="offlineMode = !offlineMode">
                <div
                  class="w-8 h-4 border transition-colors duration-micro flex items-center px-px"
                  :class="offlineMode ? 'bg-status-warning border-status-warning' : 'bg-border border-border'"
                >
                  <div
                    class="w-3 h-3 bg-surface transition-transform duration-micro"
                    :class="offlineMode ? 'translate-x-4' : 'translate-x-0'"
                  />
                </div>
                <span class="font-mono text-xs" :class="offlineMode ? 'text-status-warning' : 'text-text-muted'">
                  {{ offlineMode ? '离线模式' : '在线模式' }}
                </span>
              </div>
            </div>
          </div>
        </div>

        <!-- Imported orders table -->
        <div v-if="prepOrders.length > 0" class="overflow-x-auto">
          <table class="w-full border-collapse font-mono text-sm min-w-[500px]">
            <thead>
              <tr>
                <th class="text-left px-3 py-2.5 text-text-secondary font-bold text-xs tracking-wider uppercase border-b border-border bg-surface-raised w-10 text-center">#</th>
                <th class="text-left px-3 py-2.5 text-text-secondary font-bold text-xs tracking-wider uppercase border-b border-border bg-surface-raised">订单号</th>
                <th class="text-left px-3 py-2.5 text-text-secondary font-bold text-xs tracking-wider uppercase border-b border-border bg-surface-raised">快递单号</th>
                <th class="text-left px-3 py-2.5 text-text-secondary font-bold text-xs tracking-wider uppercase border-b border-border bg-surface-raised">备注</th>
                <th class="text-left px-3 py-2.5 text-text-secondary font-bold text-xs tracking-wider uppercase border-b border-border bg-surface-raised w-20 text-center">校验</th>
                <th class="text-left px-3 py-2.5 text-text-secondary font-bold text-xs tracking-wider uppercase border-b border-border bg-surface-raised w-20 text-center">操作</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="(row, idx) in prepOrders" :key="idx" :class="row.validated ? 'hover:bg-surface-hover' : 'opacity-50'">
                <td class="px-3 py-2 border-b border-border text-text-muted text-center text-xs">{{ idx + 1 }}</td>
                <td class="px-3 py-2 border-b border-border font-bold" :class="row.validated ? 'text-text-primary' : 'text-status-error line-through'">{{ row.orderNo }}</td>
                <td class="px-3 py-2 border-b border-border text-text-primary tracking-wider">{{ row.trackingNo || '-' }}</td>
                <td class="px-3 py-2 border-b border-border text-text-primary">{{ row.remark || '-' }}</td>
                <td class="px-3 py-2 border-b border-border text-center">
                  <Tag v-if="row.validated && (row.order as any)?._offline" severity="warn" value="离线" />
                  <Tag v-else-if="row.validated" severity="success" value="有效" />
                  <Tag v-else severity="danger" value="未找到" />
                </td>
                <td class="px-3 py-2 border-b border-border text-center">
                  <Button severity="danger" size="small" label="删除" variant="outlined" @click="removePrepRow(idx)" />
                </td>
              </tr>
            </tbody>
          </table>
        </div>

        <div v-else class="text-center py-12 text-text-muted text-xs">
          {{ importing ? '正在检索订单...' : '粘贴订单号并点击批量导入' }}
        </div>
      </div>

      <!-- Footer — pinned to bottom -->
      <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3 mt-4 pt-3 border-t border-border flex-shrink-0">
        <div class="flex items-center gap-3 text-xs">
          <span class="text-text-muted">
            共 <strong class="text-text-primary">{{ prepStats.total }}</strong> 个
          </span>
          <span v-if="prepStats.valid > 0" class="text-status-success">
            <strong>{{ prepStats.valid }}</strong> 有效
          </span>
          <span v-if="prepStats.invalid > 0" class="text-status-error">
            <strong>{{ prepStats.invalid }}</strong> 未找到
          </span>
        </div>
        <Button
          severity="primary"
          label="确认完成，进入扫码发货"
          :disabled="prepStats.valid === 0"
          @click="switchToShipping"
        />
      </div>
    </div>

    <!-- ══════════ STEP 2+3: SHIPPING ══════════ -->
    <div v-if="showShipping && !showSummary" class="md:flex-1 md:min-h-0">
      <div class="shipping-grid gap-4 md:h-full">

        <!-- COL 1: Order Queue -->
        <div class="shipping-col shipping-col-queue flex flex-col min-w-0 min-h-0">
          <div class="panel flex-1 flex flex-col">
            <div class="panel-title flex items-center justify-between mb-3">
              <span>&#9776; 订单队列</span>
              <span class="font-normal text-text-muted text-xs">
                已完成 {{ processedCount }}<template v-if="skippedCount > 0"> / 跳过 {{ skippedCount }}</template> / 剩余 {{ pendingCount }}
              </span>
            </div>

            <div class="flex-1 overflow-y-auto flex flex-col gap-1 min-h-0">
              <!-- Desktop: existing layout unchanged -->
              <div class="hidden sm:flex flex-col gap-1 flex-1">
                <template v-if="processedCount > 0">
                  <div class="text-2xs font-bold text-text-muted uppercase tracking-wider mt-3 mb-1.5 pl-1">已就绪</div>
                  <div
                    v-for="entry in sortedQueue.filter(e => e.ready)"
                    :key="entry.order.id"
                    class="border border-border px-3 py-2 font-mono text-sm text-center cursor-pointer transition-all duration-150 text-text-muted hover:border-hover"
                    @click="selectQueueEntry(queue.indexOf(entry))"
                  >
                    {{ entry.order.orderNo }}<span class="text-status-success ml-1.5 font-bold">&#10003;</span>
                    <div v-if="entry.trackingNo" class="text-2xs text-text-muted truncate mt-0.5">{{ entry.trackingNo }}</div>
                  </div>
                </template>

                <template v-if="skippedCount > 0">
                  <div class="text-2xs font-bold text-text-muted uppercase tracking-wider mt-3 mb-1.5 pl-1">已跳过</div>
                  <div
                    v-for="entry in sortedQueue.filter(e => e.skipped && !e.ready)"
                    :key="entry.order.id"
                    class="border border-dashed border-status-warning px-3 py-2 font-mono text-sm text-center cursor-pointer transition-all duration-150 hover:border-hover"
                    :class="entry === currentEntry ? 'bg-surface-hover font-bold border-2' : 'text-text-muted'"
                    @click="selectQueueEntry(queue.indexOf(entry))"
                  >
                    {{ entry.order.orderNo }}<span class="text-status-warning ml-1.5 font-bold">&#8629;</span>
                    <div v-if="entry.trackingNo" class="text-2xs text-text-muted truncate mt-0.5">{{ entry.trackingNo }}</div>
                  </div>
                </template>

                <div
                  v-if="(processedCount > 0 || skippedCount > 0) && pendingCount > 0"
                  class="border-t border-border my-1 relative text-center"
                >
                  <span class="text-2xs text-text-muted bg-surface px-2 relative -top-1.5">当前处理</span>
                </div>

                <div
                  v-if="currentEntry && !currentEntry.ready && !currentEntry.skipped"
                  class="border-2 border-border-active bg-surface-hover px-3 py-2 font-mono text-sm text-center font-bold"
                >
                  &#9656; {{ currentEntry.order.orderNo }}
                  <div v-if="currentEntry.trackingNo" class="text-2xs text-text-muted truncate mt-0.5">{{ currentEntry.trackingNo }}</div>
                </div>

                <template v-if="pendingCount > 0">
                  <div class="text-2xs font-bold text-text-muted uppercase tracking-wider mt-3 mb-1.5 pl-1">待处理</div>
                  <div
                    v-for="entry in sortedQueue.filter(e => !e.ready && !e.skipped)"
                    :key="entry.order.id"
                    class="border border-border px-3 py-2 font-mono text-sm text-center cursor-pointer transition-all duration-150 hover:border-hover"
                    :class="{ 'border-2 border-border-active bg-surface-hover font-bold': entry === currentEntry }"
                    @click="selectQueueEntry(queue.indexOf(entry))"
                  >
                    {{ entry.order.orderNo }}
                    <div v-if="entry.trackingNo" class="text-2xs text-text-muted truncate mt-0.5">{{ entry.trackingNo }}</div>
                  </div>
                </template>

                <div
                  v-if="queue.length === 0"
                  class="flex-1 flex items-center justify-center text-text-muted text-xs text-center py-12"
                >
                  扫描设备序列号以绑定到当前订单
                </div>
              </div>

              <!-- Mobile (≤640px): collapsed queue — current ±2 visible, rest folded to count -->
              <div class="sm:hidden flex flex-col gap-1 flex-1">
                <!-- Collapsed past badge -->
                <div
                  v-if="mobileHiddenPastCount > 0"
                  class="border border-border px-3 py-1.5 font-mono text-xs text-center text-text-muted bg-surface-raised cursor-default"
                >
                  已完成 {{ mobilePastCompletedInHidden }}<template v-if="mobilePastSkippedInHidden > 0"> / 跳过 {{ mobilePastSkippedInHidden }}</template> / 已折叠 {{ mobileHiddenPastCount }} 单
                </div>

                <!-- Visible past (last 2 entries before current) -->
                <div
                  v-for="entry in mobileVisiblePast"
                  :key="entry.order.id"
                  class="border px-3 py-2 font-mono text-sm text-center cursor-pointer transition-all duration-150 hover:border-hover"
                  :class="entry.skipped ? 'border-dashed border-status-warning text-text-muted' : entry.ready ? 'border-border text-text-muted' : 'border-border text-text-primary'"
                  @click="selectQueueEntry(queue.indexOf(entry))"
                >
                  {{ entry.order.orderNo }}<span v-if="entry.ready" class="text-status-success ml-1.5 font-bold">&#10003;</span><span v-if="entry.skipped" class="text-status-warning ml-1.5 font-bold">&#8629;</span>
                  <div v-if="entry.trackingNo" class="text-2xs text-text-muted truncate mt-0.5">{{ entry.trackingNo }}</div>
                </div>

                <!-- Divider: past → current -->
                <div
                  v-if="(mobileVisiblePast.length > 0 || mobileHiddenPastCount > 0) && currentEntry && !currentEntry.ready && !currentEntry.skipped"
                  class="border-t border-border my-1 relative text-center"
                >
                  <span class="text-2xs text-text-muted bg-surface px-2 relative -top-1.5">当前处理</span>
                </div>
                <div
                  v-else-if="(mobileVisiblePast.length > 0 || mobileHiddenPastCount > 0) && currentEntry && currentEntry.skipped"
                  class="border-t border-border my-1 relative text-center"
                >
                  <span class="text-2xs text-status-warning bg-surface px-2 relative -top-1.5">已跳过</span>
                </div>

                <!-- Current order (always visible if not ready) -->
                <div
                  v-if="currentEntry && !currentEntry.ready && !currentEntry.skipped"
                  class="border-2 border-border-active bg-surface-hover px-3 py-2 font-mono text-sm text-center font-bold"
                >
                  &#9656; {{ currentEntry.order.orderNo }}
                  <div v-if="currentEntry.trackingNo" class="text-2xs text-text-muted truncate mt-0.5">{{ currentEntry.trackingNo }}</div>
                </div>
                <div
                  v-else-if="currentEntry && currentEntry.skipped"
                  class="border-2 border-dashed border-status-warning bg-surface-hover px-3 py-2 font-mono text-sm text-center font-bold"
                >
                  &#8629; {{ currentEntry.order.orderNo }}
                  <div v-if="currentEntry.trackingNo" class="text-2xs text-text-muted truncate mt-0.5">{{ currentEntry.trackingNo }}</div>
                </div>

                <!-- Divider: current → future -->
                <div
                  v-if="(mobileVisibleFuture.length > 0 || mobileHiddenFutureCount > 0) && currentEntry && !currentEntry.ready && !currentEntry.skipped"
                  class="border-t border-border my-1 relative text-center"
                >
                  <span class="text-2xs text-text-muted bg-surface px-2 relative -top-1.5">待处理</span>
                </div>

                <!-- Visible future (first 2 entries after current) -->
                <div
                  v-for="entry in mobileVisibleFuture"
                  :key="entry.order.id"
                  class="border px-3 py-2 font-mono text-sm text-center cursor-pointer transition-all duration-150 hover:border-hover"
                  :class="entry.skipped ? 'border-dashed border-status-warning text-text-muted' : entry === currentEntry ? 'border-2 border-border-active bg-surface-hover font-bold border-border' : 'border border-border'"
                  @click="selectQueueEntry(queue.indexOf(entry))"
                >
                  {{ entry.order.orderNo }}<span v-if="entry.skipped" class="text-status-warning ml-1.5 font-bold">&#8629;</span>
                  <div v-if="entry.trackingNo" class="text-2xs text-text-muted truncate mt-0.5">{{ entry.trackingNo }}</div>
                </div>

                <!-- Collapsed future badge -->
                <div
                  v-if="mobileHiddenFutureCount > 0"
                  class="border border-border px-3 py-1.5 font-mono text-xs text-center text-text-muted bg-surface-raised cursor-default"
                >
                  剩余 {{ entriesAfterCurrent.length }} 单<span class="text-text-muted/60">（已折叠 {{ mobileHiddenFutureCount }} 单）</span>
                </div>

                <div
                  v-if="queue.length === 0"
                  class="flex-1 flex items-center justify-center text-text-muted text-xs text-center py-12"
                >
                  扫描设备序列号以绑定到当前订单
                </div>
              </div>
              <!-- end mobile -->
            </div>

            <div class="flex gap-4 flex-wrap mt-4 pt-3 border-t border-border text-2xs text-text-muted">
              <div class="flex items-center gap-1.5">
                <div class="w-4 h-4 border-2 border-border-active bg-surface-hover" />当前
              </div>
              <div class="flex items-center gap-1.5">
                <div class="w-4 h-4 border border-border" />待处理
              </div>
              <div class="flex items-center gap-1.5">
                <div class="w-4 h-4 border border-border opacity-40" />已完成
              </div>
            </div>
          </div>
        </div>

        <!-- COL 2: Camera Scanner -->
        <div class="shipping-col shipping-col-camera flex flex-col min-w-0 min-h-0">
          <div class="panel flex-1 flex flex-col">
            <div class="panel-title mb-3">&#128247; 扫码窗口</div>

            <div class="relative w-full bg-input border border-border overflow-hidden mb-2" style="aspect-ratio: 4/3;">
              <div id="camera-container" class="w-full h-full" />
              <div
                v-if="!cameraReady"
                class="absolute inset-0 flex flex-col items-center justify-center text-text-muted text-sm gap-2 bg-input z-10"
              >
                <span class="text-4xl opacity-40">&#128247;</span>
                <span v-if="cameraError" class="px-4 text-center leading-relaxed">{{ cameraError }}</span>
                <span v-else>摄像头预览区域</span>
              </div>
              <div class="absolute top-3 left-3 w-6 h-6 border-t-2 border-l-2 border-border-active z-10 pointer-events-none" />
              <div class="absolute top-3 right-3 w-6 h-6 border-t-2 border-r-2 border-border-active z-10 pointer-events-none" />
              <div class="absolute bottom-3 left-3 w-6 h-6 border-b-2 border-l-2 border-border-active z-10 pointer-events-none" />
              <div class="absolute bottom-3 right-3 w-6 h-6 border-b-2 border-r-2 border-border-active z-10 pointer-events-none" />
            </div>

            <div
              class="flex items-center gap-2 text-2xs justify-center mb-2"
              :class="cameraReady ? 'text-status-success' : cameraError ? 'text-status-warning' : 'text-text-muted'"
            >
              <div
                class="w-2 h-2 rounded-[var(--radius-sm)]"
                :class="cameraReady ? 'bg-status-success' : cameraError ? 'bg-status-warning' : 'bg-text-muted'"
              />
              <span>
                <template v-if="cameraReady && zoomSupported && autoZoomEnabled">自动对焦中 {{ zoomValue.toFixed(1) }}×</template>
                <template v-else-if="cameraReady && zoomSupported">扫描设备序列号... {{ zoomValue.toFixed(1) }}×</template>
                <template v-else-if="cameraReady">扫描设备序列号...</template>
                <template v-else-if="cameraError">摄像头不可用 — 使用手动输入</template>
                <template v-else>正在启动摄像头...</template>
              </span>
            </div>

            <!-- Zoom slider -->
            <div v-if="zoomSupported" class="flex items-center gap-2 mb-2">
              <span class="font-mono text-2xs text-text-muted w-6 text-right">{{ zoomMin }}×</span>
              <input
                type="range"
                class="zoom-slider flex-1"
                :min="zoomMin"
                :max="zoomMax"
                :step="zoomStep"
                :value="zoomValue"
                @input="onZoomChange(($event.target as HTMLInputElement).valueAsNumber)"
                @pointerdown="autoZoomEnabled = false"
              />
              <span class="font-mono text-2xs text-text-muted w-6">{{ zoomMax }}×</span>
              <span class="font-mono text-2xs text-text-muted w-10 text-right">{{ zoomValue.toFixed(1) }}×</span>
            </div>

            <div class="flex gap-2">
              <Button severity="secondary" size="small" label="切换摄像头" :disabled="!cameraReady" @click="switchCamera" />
              <span class="flex-1" />
            </div>

            <div class="flex gap-2 mt-3">
              <InputText
                v-model="manualInput"
                class="flex-1"
                placeholder="手动输入设备序列号后按 Enter..."
                :disabled="scanning"
                @keydown.enter="onManualSubmit"
              />
              <Button severity="secondary" :disabled="scanning || !manualInput.trim()" label="查找" @click="onManualSubmit" />
            </div>
          </div>
        </div>

        <!-- COL 3: Order Detail -->
        <div class="shipping-col shipping-col-detail flex flex-col min-w-0 min-h-0">
          <div class="panel flex-1 flex flex-col min-h-0 min-w-0">
            <div class="panel-title flex items-center justify-between mb-3 flex-shrink-0">
              <span>&#128196; 当前订单详情</span>
              <Tag
                v-if="currentEntry?.skipped"
                severity="warn"
                value="已跳过"
              />
              <Tag
                v-else-if="currentEntry"
                :severity="currentEntry.ready ? 'success' : 'info'"
                :value="currentEntry.ready ? '已就绪' : '待确认'"
              />
            </div>

            <template v-if="currentOrder">
              <div class="flex-1 min-h-0 overflow-y-auto">
                <div class="grid gap-x-3 gap-y-2.5 text-sm" style="grid-template-columns: minmax(0, max-content) minmax(0, 1fr);">
                <div class="text-text-secondary whitespace-nowrap">订单号</div>
                <div class="text-text-primary font-bold text-md break-all">
                  {{ currentOrder.orderNo }}
                  <span v-if="currentEntry?.trackingVerified" class="text-status-success ml-1.5 text-sm">&#10003;</span>
                </div>
                <div class="text-text-secondary whitespace-nowrap">发货日期</div>
                <div class="text-text-primary">{{ currentOrder.deliveryDate || '-' }}</div>
                <div class="text-text-secondary whitespace-nowrap">取货方式</div>
                <div class="text-text-primary">{{ (currentOrder.pickupMethods || []).join(' · ') || '-' }}</div>
                <div class="text-text-secondary whitespace-nowrap">地址</div>
                <div class="text-text-primary break-all">{{ currentOrder.address || '-' }}</div>
                <template v-if="currentEntry && currentEntry.trackingNo">
                  <div class="text-text-secondary whitespace-nowrap">快递单号</div>
                  <div class="flex items-center gap-2">
                    <span class="text-text-primary font-mono break-all">{{ currentEntry.trackingNo }}</span>
                    <span
                      v-if="currentEntry.scannedTrackingNo"
                      class="text-xs px-1.5 py-0.5 border font-mono"
                      :class="currentEntry.trackingVerified ? 'text-status-success border-status-success' : 'text-status-error border-status-error'"
                    >
                      {{ currentEntry.trackingVerified ? '✓ 已验证' : '✗ 不匹配: ' + currentEntry.scannedTrackingNo }}
                    </span>
                  </div>
                </template>
                <div class="text-text-secondary whitespace-nowrap">设备</div>
                <div class="text-text-primary tracking-wider font-mono break-all">{{ (currentOrder.devices || []).join(', ') || '-' }}</div>
                <template v-if="currentEntry && currentEntry.scannedDevices.length > 0">
                  <div class="text-text-secondary whitespace-nowrap text-status-success">本次绑定</div>
                  <div class="flex flex-wrap gap-1.5">
                    <span
                      v-for="sn in currentEntry.scannedDevices"
                      :key="sn"
                      class="inline-flex items-center gap-1 px-2 py-0.5 border border-status-success text-status-success font-mono text-xs"
                    >
                      {{ sn }}
                      <button
                        class="text-text-muted hover:text-status-error transition-colors leading-none cursor-pointer bg-transparent border-none p-0 text-xs"
                        title="移除绑定"
                        @click="removeScannedDevice(sn)"
                      >&#10005;</button>
                    </span>
                  </div>
                </template>
              </div>

              <div
                v-if="currentOrder.notes"
                class="bg-surface-raised border-l-2 border-border-active px-3 py-2 mt-3"
              >
                <div class="text-status-warning text-xs font-bold mb-1">&#9888; 备注</div>
                <div class="text-text-primary text-sm break-all">{{ currentOrder.notes }}</div>
              </div>

              <div
                v-if="currentEntry?.remark"
                class="bg-surface-raised border-l-2 border-border-active px-3 py-2 mt-3"
              >
                <div class="text-text-primary text-xs font-bold mb-1">&#128221; 导入备注</div>
                <div class="text-text-primary text-sm break-all">{{ currentEntry.remark }}</div>
              </div>
              </div>

              <div class="flex justify-end gap-2 mt-auto pt-3 border-t border-border flex-shrink-0">
                <Button
                  v-if="currentEntry?.skipped"
                  severity="secondary"
                  label="取消跳过"
                  @click="unskipCurrentOrder"
                />
                <Button
                  v-if="!currentEntry?.ready && !currentEntry?.skipped"
                  severity="secondary"
                  variant="outlined"
                  label="跳过此单"
                  @click="skipOrder"
                />
                <Button
                  severity="primary"
                  :disabled="confirming || !isCurrentEntryConfirmable()"
                  :label="confirming ? '处理中...' : '标记就绪'"
                  @click="confirmShip"
                />
              </div>
            </template>

            <div v-else class="flex-1 flex items-center justify-center text-text-muted text-xs text-center py-12">
              {{ currentStep === 3 ? '全部订单已确认完毕' : '选择一个订单，扫描设备序列号' }}
            </div>

            <div v-if="pendingCount > 0 || skippedCount > 0" class="flex justify-end mt-2">
              <Button
                severity="danger"
                variant="outlined"
                :disabled="endingTask"
                :label="endingTask ? '处理中...' : '结束任务'"
                @click="endTaskEarly"
              />
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- ══════════ STEP 3: SHIPPING COMPLETE ══════════ -->
    <div v-if="showSummary" class="panel flex flex-col md:flex-1 md:min-h-0 min-h-0">
      <!-- Show result header after submit -->
      <div v-if="shippedOrders.length > 0" class="flex items-center gap-4 mb-6 flex-shrink-0">
        <div class="w-12 h-12 rounded-[var(--radius-full)] border-2 border-status-success flex items-center justify-center text-status-success text-2xl">&#10003;</div>
        <div>
          <h2 class="font-mono font-bold text-xl text-text-primary">全部发货完成</h2>
          <p class="text-text-muted text-sm">已提交 {{ shippedOrders.length }} 个订单</p>
        </div>
      </div>

      <!-- Pre-submit: review header -->
      <div v-else class="flex items-center gap-4 mb-6 flex-shrink-0">
        <div class="w-12 h-12 rounded-[var(--radius-full)] border-2 border-border-active flex items-center justify-center text-border-active text-2xl font-bold">{{ reviewCount }}</div>
        <div>
          <h2 class="font-mono font-bold text-xl text-text-primary">待确认发货</h2>
          <p class="text-text-muted text-sm">检查设备绑定后点击「确认提交」</p>
        </div>
      </div>

      <!-- Stats cards -->
      <div class="grid grid-cols-3 gap-4 mb-6 flex-shrink-0">
        <div class="panel text-center">
          <div class="text-3xl font-bold text-text-primary">{{ reviewCount }}</div>
          <div class="text-text-muted text-2xs mt-1">{{ shippedOrders.length > 0 ? '已提交' : '待提交' }}</div>
        </div>
        <div class="panel text-center">
          <div class="text-3xl font-bold text-text-primary">{{ shippedWithTracking }}</div>
          <div class="text-text-muted text-2xs mt-1">快递单号</div>
        </div>
        <div class="panel text-center">
          <div class="text-3xl font-bold text-text-primary">{{ shippingDuration }}</div>
          <div class="text-text-muted text-2xs mt-1">处理时长</div>
        </div>
      </div>

      <!-- Review table: ready orders with device tags + delete -->
      <div class="flex-1 min-h-0 overflow-y-auto overflow-x-auto">
        <table class="w-full border-collapse font-mono text-sm min-w-[600px] md:min-w-0">
          <thead class="sticky top-0 z-10">
            <tr>
              <th class="text-left px-3 py-2.5 text-text-secondary font-bold text-xs tracking-wider uppercase border-b border-border bg-surface w-10 text-center">#</th>
              <th class="text-left px-3 py-2.5 text-text-secondary font-bold text-xs tracking-wider uppercase border-b border-border bg-surface">订单号</th>
              <th class="text-left px-3 py-2.5 text-text-secondary font-bold text-xs tracking-wider uppercase border-b border-border bg-surface">快递单号</th>
              <th class="text-left px-3 py-2.5 text-text-secondary font-bold text-xs tracking-wider uppercase border-b border-border bg-surface">已有设备</th>
              <th class="text-left px-3 py-2.5 text-text-secondary font-bold text-xs tracking-wider uppercase border-b border-border bg-surface">本次绑定</th>
              <th class="text-left px-3 py-2.5 text-text-secondary font-bold text-xs tracking-wider uppercase border-b border-border bg-surface w-16 text-center">状态</th>
            </tr>
          </thead>
          <tbody>
            <!-- Ready but not yet submitted -->
            <tr v-for="(entry, idx) in readyOrders" :key="entry.order.id">
              <td class="px-3 py-2 border-b border-border text-text-muted text-center text-xs">{{ idx + 1 }}</td>
              <td class="px-3 py-2 border-b border-border text-text-primary font-bold">{{ entry.order.orderNo }}</td>
              <td class="px-3 py-2 border-b border-border text-text-primary tracking-wider">{{ entry.trackingNo || '-' }}</td>
              <td class="px-3 py-2 border-b border-border text-text-muted font-mono text-xs break-all">{{ (entry.order.devices || []).join(', ') || '-' }}</td>
              <td class="px-3 py-2 border-b border-border">
                <div class="flex flex-wrap gap-1">
                  <span
                    v-for="sn in entry.scannedDevices"
                    :key="sn"
                    class="inline-flex items-center gap-1 px-2 py-0.5 border border-border-active text-text-primary font-mono text-xs"
                  >
                    {{ sn }}
                    <button
                      class="text-text-muted hover:text-status-error transition-colors leading-none cursor-pointer bg-transparent border-none p-0 text-xs"
                      :disabled="submitting"
                      @click="entry.scannedDevices = entry.scannedDevices.filter(s => s !== sn)"
                    >&#10005;</button>
                  </span>
                  <span v-if="entry.scannedDevices.length === 0" class="text-text-muted">-</span>
                </div>
              </td>
              <td class="px-3 py-2 border-b border-border text-center">
                <span class="text-status-warning text-xs">待提交</span>
              </td>
            </tr>
            <!-- Already submitted -->
            <tr v-for="(entry, idx) in shippedOrders" :key="entry.order.id" class="opacity-50">
              <td class="px-3 py-2 border-b border-border text-text-muted text-center text-xs">{{ readyOrders.length + idx + 1 }}</td>
              <td class="px-3 py-2 border-b border-border text-text-primary font-bold">{{ entry.order.orderNo }}</td>
              <td class="px-3 py-2 border-b border-border text-text-primary tracking-wider">{{ entry.trackingNo || '-' }}</td>
              <td class="px-3 py-2 border-b border-border text-text-muted font-mono text-xs break-all">{{ (entry.order.devices || []).join(', ') || '-' }}</td>
              <td class="px-3 py-2 border-b border-border text-text-primary tracking-wider font-mono">{{ entry.scannedDevices.join(', ') || '-' }}</td>
              <td class="px-3 py-2 border-b border-border text-center">
                <span class="text-status-success font-bold">&#10003;</span>
              </td>
            </tr>
            <!-- No orders to review -->
            <tr v-if="readyOrders.length === 0 && shippedOrders.length === 0">
              <td colspan="6" class="px-3 py-12 text-center text-text-muted text-sm">暂无待提交订单</td>
            </tr>
          </tbody>
        </table>
      </div>

      <!-- Actions -->
      <div class="flex flex-col sm:flex-row sm:justify-end gap-3 mt-4 pt-3 border-t border-border flex-shrink-0">
        <Button severity="secondary" label="导出 CSV" :disabled="readyOrders.length === 0 && shippedOrders.length === 0" @click="exportShippingCsv" />
        <Button severity="secondary" label="复制数据" :disabled="readyOrders.length === 0 && shippedOrders.length === 0" @click="copyShippingData" />
        <Button
          v-if="readyOrders.length > 0"
          severity="primary"
          :disabled="submitting"
          :label="submitting ? '提交中...' : '确认提交'"
          @click="submitAllShipping"
        />
        <Button severity="primary" label="开始新批次" @click="resetShipping" />
      </div>
    </div>
    <div class="version-footer">2026 — V1.05 — REV.N</div>
  </div>
</template>

<style scoped>
/* Fix PrimeVue Timeline for Terminal Brutalist theme */
.shipping-timeline {
  width: 100%;
  max-width: 420px;
}

:deep(.shipping-timeline.p-timeline-horizontal) {
  display: flex;
  flex-direction: row;
  justify-content: center;
  gap: 44px;
}

:deep(.shipping-timeline.p-timeline-horizontal .p-timeline-event) {
  display: flex;
  flex-direction: column;
  align-items: center;
  width: 64px;
  flex: 0 0 auto;
}

/* Separator: relative so connector can be absolute */
:deep(.shipping-timeline.p-timeline-horizontal .p-timeline-event-separator) {
  display: flex;
  flex-direction: row;
  align-items: center;
  position: relative;
}

/* Hide default marker */
:deep(.shipping-timeline .p-timeline-event-marker) {
  display: none;
}

/* Connector: absolute, centered in the gap. Event=64px, marker=32px, gap=44px → gap center at 86px from event left, separator right at 48px → offset=38px */
:deep(.shipping-timeline.p-timeline-horizontal .p-timeline-event-separator) > span {
  position: absolute;
  left: calc(100% + 38px);
  top: 50%;
  transform: translate(-50%, -50%);
  width: 20px;
  height: 2px;
}

:deep(.shipping-timeline .p-timeline-event-content) {
  padding: 4px 0 0;
  text-align: center;
}

:deep(.shipping-timeline .p-timeline-event-opposite) {
  display: none;
}

/* ── Responsive shipping grid ── */
.shipping-grid {
  display: grid;
  grid-template-columns: minmax(0, 22%) minmax(0, 50%) minmax(0, 28%);
}

/* Tablet: when viewport < 960px, stack camera + detail side-by-side, queue above */
@media (max-width: 959px) {
  .shipping-grid {
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    grid-template-rows: auto 1fr;
  }
  .shipping-col-queue {
    grid-column: 1 / -1;
  }
}

/* Narrow: when viewport < 640px, all three stack vertically */
@media (max-width: 639px) {
  .shipping-grid {
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto;
    height: auto;
  }
  .shipping-col-queue {
    grid-column: auto;
  }
  .shipping-col {
    min-height: 0;
  }
  .shipping-col-camera {
    min-height: 360px;
  }
  .shipping-col-detail {
    min-height: 200px;
  }
  .shipping-col .panel {
    min-height: 0;
  }
}

/* ── Zoom slider ── */
.zoom-slider {
  -webkit-appearance: none;
  appearance: none;
  height: 4px;
  background: var(--border);
  border: none;
  outline: none;
  cursor: pointer;
}

.zoom-slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 14px;
  height: 14px;
  background: var(--text-primary);
  border: 1px solid var(--border-active);
  cursor: pointer;
}

.zoom-slider::-moz-range-thumb {
  width: 14px;
  height: 14px;
  background: var(--text-primary);
  border: 1px solid var(--border-active);
  border-radius: 0;
  cursor: pointer;
}
</style>
