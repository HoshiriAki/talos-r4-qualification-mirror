<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick } from 'vue'
import { useToast } from 'primevue/usetoast'
import { useDevicesStore } from '@/stores/devices'
import { useCheckinHistory, type CheckinRecord } from '@/composables/useCheckinHistory'
import * as devicesApi from '@/api/devices'
import type { Device } from '@/api/devices'
import { Html5Qrcode, Html5QrcodeSupportedFormats } from 'html5-qrcode'
import type { Html5QrcodeResult } from 'html5-qrcode'

const toast = useToast()
const store = useDevicesStore()
const { records, addRecord, clearRecords, downloadCSV } = useCheckinHistory()

const serialNo = ref('')
const scanning = ref(false)
const undoing = ref(false)

// ── Continuous scan mode ──
const continuousScanEnabled = ref(false)
const continuousScanCount = ref(0)
const continuousScanRecent = ref<Array<{ serialNo: string; time: string }>>([])

function addContinuousRecent(serialNo: string) {
  continuousScanRecent.value.unshift({ serialNo, time: formatTime() })
  if (continuousScanRecent.value.length > 10) {
    continuousScanRecent.value = continuousScanRecent.value.slice(0, 10)
  }
}

// ── Day helpers (Asia/Shanghai, local time assumed) ──
const DAY_NAMES = ['周日', '周一', '周二', '周三', '周四', '周五', '周六']

function getDateKey(): string {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

function getYesterdayKey(): string {
  const d = new Date()
  d.setDate(d.getDate() - 1)
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

function getDayOfWeekFromKey(dateKey: string): string {
  const [y, m, d] = dateKey.split('-').map(Number)
  const date = new Date(y, m - 1, d)
  return DAY_NAMES[date.getDay()]
}

// ── Camera state ──
const showCamera = ref(false)
const cameraReady = ref(false)
const cameraStarting = ref(false)
const cameraError = ref<string | null>(null)
const scannedValue = ref<string | null>(null)
const cameraPaused = ref(false)
let scanner: Html5Qrcode | null = null
let lastScanAt = 0

const cameraStatusText = computed(() => {
  if (cameraError.value) return cameraError.value
  if (!cameraReady.value) return '正在启动摄像头...'
  if (cameraPaused.value) return '扫描已暂停 — 请确认或忽略'
  return '正在扫描... 请对准条形码/二维码'
})

// ── Undo: 30-second window with countdown ──
const lastCheckin = ref<{ record: CheckinRecord; expiresAt: number } | null>(null)
const undoCountdown = ref(0)
let undoTimer: ReturnType<typeof setTimeout> | null = null
let countdownInterval: ReturnType<typeof setInterval> | null = null

function clearUndoTimers() {
  if (undoTimer) { clearTimeout(undoTimer); undoTimer = null }
  if (countdownInterval) { clearInterval(countdownInterval); countdownInterval = null }
}

function scheduleUndoExpiry() {
  clearUndoTimers()
  undoCountdown.value = 30

  undoTimer = setTimeout(() => {
    lastCheckin.value = null
    clearUndoTimers()
  }, 30000)

  countdownInterval = setInterval(() => {
    if (!lastCheckin.value) {
      clearUndoTimers()
      return
    }
    undoCountdown.value = Math.max(0, Math.ceil((lastCheckin.value.expiresAt - Date.now()) / 1000))
    if (undoCountdown.value <= 0) {
      clearUndoTimers()
      lastCheckin.value = null
    }
  }, 1000)
}

// ── Audio / vibration feedback ──
function playFeedback(ok: boolean) {
  try {
    const ctx = new AudioContext()
    const osc = ctx.createOscillator()
    const gain = ctx.createGain()
    osc.connect(gain)
    gain.connect(ctx.destination)
    osc.type = ok ? 'sine' : 'square'
    osc.frequency.value = ok ? 880 : 220
    gain.gain.value = 0.08
    osc.start()
    osc.stop(ctx.currentTime + 0.12)
  } catch { /* audio not available */ }
  try { if (ok) navigator.vibrate?.(80) } catch { /* vibration not available */ }
}

function formatTime(): string {
  const d = new Date()
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}:${String(d.getSeconds()).padStart(2, '0')}`
}

// ── Core checkin ──
async function doCheckin(overrideSerial?: string) {
  const sn = (overrideSerial ?? serialNo.value).trim()
  if (!sn || scanning.value) return

  scanning.value = true
  try {
    const result = await store.checkinScan(sn)
    const record: CheckinRecord = {
      serialNo: sn,
      ok: result.ok || false,
      action: result.action || '',
      message: result.message || '',
      time: formatTime(),
      date: getDateKey(),
      beforeStatus: result.beforeStatus,
      afterStatus: result.afterStatus,
    }
    await addRecord(record)

    // Track for undo if the checkin actually changed device status
    if (result.ok && result.action === 'updated' && result.beforeStatus) {
      lastCheckin.value = { record, expiresAt: Date.now() + 30000 }
      scheduleUndoExpiry()
    }

    if (result.ok || result.action === 'already_in_stock') {
      playFeedback(true)
      if (!overrideSerial) {
        // Continuous scan mode: count + recent list
        if (continuousScanEnabled.value) {
          continuousScanCount.value++
          addContinuousRecent(sn)
        }
        serialNo.value = ''
      }

      // Show toast for auto-completed orders
      if (result.autoCompletedOrders && result.autoCompletedOrders.length > 0) {
        result.autoCompletedOrders.forEach((oc) => {
          toast.add({
            severity: 'info',
            summary: '订单自动完成',
            detail: `订单 #ORD-${oc.orderId} 所有设备已还清，已自动完成`,
            life: 5000,
          })
        })
      }

      // Continuous scan: auto-focus input for next scan
      if (continuousScanEnabled.value && !overrideSerial) {
        nextTick(() => {
          const input = document.querySelector<HTMLInputElement>('#checkin-serial-input')
          input?.focus()
        })
      }
    } else {
      playFeedback(false)
    }
  } catch (e: any) {
    playFeedback(false)
    const record: CheckinRecord = {
      serialNo: sn,
      ok: false,
      action: 'error',
      message: e.message || '入库失败',
      time: formatTime(),
      date: getDateKey(),
    }
    await addRecord(record)
    toast.add({ severity: 'error', summary: '扫码入库失败', detail: e.message || '未知错误', life: 4000 })
  } finally {
    scanning.value = false
  }
}

async function undoLastCheckin() {
  const entry = lastCheckin.value
  if (!entry || undoing.value) return

  undoing.value = true
  try {
    await devicesApi.undoCheckin(entry.record.serialNo, entry.record.beforeStatus!)
    lastCheckin.value = null
    clearUndoTimers()
    toast.add({ severity: 'success', summary: '已撤销', detail: `${entry.record.serialNo} 状态已还原`, life: 2000 })
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '撤销失败', detail: e.message || '未知错误', life: 4000 })
  } finally {
    undoing.value = false
  }
}

function resultClass(r: CheckinRecord): string {
  if (r.action === 'already_in_stock') return 'border-status-warning text-status-warning'
  if (r.ok) return 'border-status-success text-status-success'
  return 'border-status-error text-status-error'
}

// ── Camera ──
function classifyCameraError(e: any): string {
  const msg = e?.message || e?.toString?.() || ''
  const name = e?.name || ''

  if (name === 'NotAllowedError' || msg.includes('NotAllowed') || msg.includes('Permission')) {
    const isHttp = typeof location !== 'undefined' && location.protocol === 'http:' && location.hostname !== 'localhost' && location.hostname !== '127.0.0.1'
    if (isHttp) return '摄像头需要 HTTPS 安全连接。当前为 HTTP，浏览器已阻止摄像头访问。请使用手动输入。'
    return '摄像头权限被拒绝。请在浏览器设置中允许本站访问摄像头，或使用手动输入。'
  }
  if (name === 'NotFoundError' || msg.includes('NotFound') || msg.includes('No camera')) {
    return '未检测到摄像头设备。请使用手动输入序列号。'
  }
  if (name === 'NotReadableError' || msg.includes('NotReadable') || msg.includes('in use')) {
    return '摄像头被其他应用占用。请关闭其他使用摄像头的应用后重试。'
  }
  return `摄像头启动失败：${msg || '未知错误'}。可通过下方手动输入继续操作。`
}

async function startCamera() {
  if (cameraStarting.value) return
  cameraStarting.value = true
  showCamera.value = true
  cameraError.value = null

  await nextTick()

  try {
    scanner = new Html5Qrcode('checkin-camera-container', {
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

    const onSuccess = (decodedText: string, _result: Html5QrcodeResult) => {
      onCameraScan(decodedText)
    }

    // Try environment-facing first, fall back to user-facing
    const constraints: MediaTrackConstraints[] = [
      { facingMode: 'environment' },
      { facingMode: 'user' },
      { facingMode: { ideal: 'environment' } },
    ]

    let lastError: any = null
    for (const c of constraints) {
      try {
        await scanner.start(c, scanConfig, onSuccess, () => {})
        cameraReady.value = true
        cameraError.value = null
        return
      } catch (err: any) {
        lastError = err
        try { await scanner.stop() } catch {}
        scanner = new Html5Qrcode('checkin-camera-container', {
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

function onCameraScan(text: string) {
  const now = Date.now()
  // Debounce: ignore scans within 1500ms
  if (now - lastScanAt < 1500) return
  lastScanAt = now

  const v = text.trim()
  if (!v || cameraPaused.value) return

  cameraPaused.value = true
  scannedValue.value = v
  if (scanner) {
    try { scanner.pause(true) } catch {}
  }
}

async function confirmScan() {
  const sn = scannedValue.value
  if (!sn || scanning.value) return

  scannedValue.value = null
  cameraPaused.value = false

  await doCheckin(sn)

  // Resume scanning for next device
  lastScanAt = Date.now() // debounce after confirm
  if (scanner && cameraReady.value && showCamera.value) {
    try { await scanner.resume() } catch {}
  }
}

async function ignoreScan() {
  scannedValue.value = null
  cameraPaused.value = false
  lastScanAt = Date.now() // debounce after ignore
  if (scanner && cameraReady.value && showCamera.value) {
    try { await scanner.resume() } catch {}
  }
}

async function closeCamera() {
  scannedValue.value = null
  cameraPaused.value = false
  if (scanner) {
    try { await scanner.stop() } catch {}
    scanner = null
  }
  cameraReady.value = false
  cameraError.value = null
  showCamera.value = false
}

// ── History grouping ──
const groupExpanded = ref<Record<string, boolean>>({})

interface RecordGroup {
  dateKey: string
  label: string
  sublabel: string
  records: CheckinRecord[]
}

const groupedRecords = computed<RecordGroup[]>(() => {
  const groups: Record<string, CheckinRecord[]> = {}
  const today = getDateKey()

  for (const r of records.value) {
    const key = r.date || today
    if (!groups[key]) groups[key] = []
    groups[key].push(r)
  }

  const yesterday = getYesterdayKey()

  return Object.entries(groups)
    .map(([dateKey, recs]) => {
      const isToday = dateKey === today
      const isYesterday = dateKey === yesterday
      return {
        dateKey,
        label: isToday ? '今日' : isYesterday ? '昨日' : getDayOfWeekFromKey(dateKey),
        sublabel: dateKey,
        records: recs,
      }
    })
    .sort((a, b) => b.dateKey.localeCompare(a.dateKey))
})

function isGroupExpanded(dateKey: string): boolean {
  if (groupExpanded.value[dateKey] !== undefined) {
    return groupExpanded.value[dateKey]
  }
  return dateKey === getDateKey()
}

function toggleGroup(dateKey: string) {
  groupExpanded.value[dateKey] = !isGroupExpanded(dateKey)
}

// ── Lifecycle ──
onUnmounted(() => {
  clearUndoTimers()
  if (scanner) {
    try { scanner.stop() } catch {}
    scanner = null
  }
})
</script>

<template>
  <div class="space-y-4">
    <!-- MODULE-05 bar -->
    <div class="module-bar mb-4">
      <span class="module-number-label">MODULE-05</span>
      <div class="structure-line" />
      <span class="module-page-label">扫码入库</span>
    </div>

    <!-- ══════════ Continuous Scan Mode Toggle ══════════ -->
    <div class="panel">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div class="flex items-center gap-3">
          <label class="font-mono text-sm text-text-primary" for="continuous-scan-toggle">连续扫描模式</label>
          <InputSwitch
            id="continuous-scan-toggle"
            v-model="continuousScanEnabled"
          />
        </div>
        <div v-if="continuousScanEnabled" class="flex items-center gap-3">
          <span class="badge font-mono text-xs">
            已扫描 {{ continuousScanCount }} 件
          </span>
          <Button
            severity="secondary"
            size="small"
            label="重置计数"
            @click="continuousScanCount = 0; continuousScanRecent = []"
          />
        </div>
      </div>
      <!-- Recent scan list (continuous mode) -->
      <div v-if="continuousScanEnabled && continuousScanRecent.length > 0" class="mt-3 space-y-1">
        <div class="font-mono text-xs text-text-muted mb-1">最近扫描 ({{ continuousScanRecent.length }})</div>
        <div
          v-for="(item, idx) in continuousScanRecent"
          :key="`${item.serialNo}-${idx}`"
          class="flex items-center gap-2 font-mono text-xs py-0.5"
        >
          <span class="text-status-success font-bold">&#10003;</span>
          <span class="tracking-wider text-text-primary">{{ item.serialNo }}</span>
          <span class="text-text-muted ml-auto">{{ item.time }}</span>
        </div>
      </div>
    </div>

    <!-- ══════════ Scan Input Panel ══════════ -->
    <div class="panel">
      <h2 class="panel-title">扫码入库</h2>
      <p class="font-mono text-xs text-text-muted mb-4">扫描设备二维码/条形码，或手动输入序列号进行入库操作</p>

      <!-- Camera trigger (shown when camera is off) -->
      <Button
        v-if="!showCamera"
        severity="secondary"
        :disabled="cameraStarting"
        :label="cameraStarting ? '启动中...' : '📷 启动摄像头扫描'"
        class="mb-4"
        @click="startCamera"
      />

      <!-- Camera area (shown when camera is on) -->
      <div v-if="showCamera" class="space-y-3 mb-4">
        <!-- Camera viewport -->
        <div class="relative w-full bg-input border border-border overflow-hidden" style="aspect-ratio: 4/3; max-width: 480px;">
          <div id="checkin-camera-container" class="w-full h-full" />
          <div
            v-if="!cameraReady"
            class="absolute inset-0 flex flex-col items-center justify-center text-text-muted text-sm gap-2 bg-input z-10"
          >
            <span class="text-4xl opacity-40">📷</span>
            <span v-if="cameraError" class="px-4 text-center leading-relaxed text-xs">{{ cameraError }}</span>
            <span v-else class="text-xs">摄像头预览区域</span>
          </div>
          <!-- Corner brackets -->
          <div class="absolute top-3 left-3 w-6 h-6 border-t-2 border-l-2 border-border-active z-10 pointer-events-none" />
          <div class="absolute top-3 right-3 w-6 h-6 border-t-2 border-r-2 border-border-active z-10 pointer-events-none" />
          <div class="absolute bottom-3 left-3 w-6 h-6 border-b-2 border-l-2 border-border-active z-10 pointer-events-none" />
          <div class="absolute bottom-3 right-3 w-6 h-6 border-b-2 border-r-2 border-border-active z-10 pointer-events-none" />
        </div>

        <!-- Scan result action buttons -->
        <div v-if="scannedValue" class="flex items-center gap-3 p-3 border border-border-active bg-surface-hover">
          <span class="font-mono text-sm text-text-primary tracking-wider flex-1">{{ scannedValue }}</span>
          <Button
            severity="primary"
            size="small"
            :disabled="scanning"
            :label="scanning ? '处理中...' : '确认入库'"
            @click="confirmScan"
          />
          <Button
            severity="secondary"
            size="small"
            :disabled="scanning"
            label="忽略"
            @click="ignoreScan"
          />
        </div>

        <!-- Camera status + close button -->
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2 text-xs">
            <div
              class="w-2 h-2 rounded-[var(--radius-sm)]"
              :class="cameraReady ? 'bg-status-success' : cameraError ? 'bg-status-warning' : 'bg-text-muted'"
            />
            <span
              class="font-mono"
              :class="cameraReady ? 'text-status-success' : cameraError ? 'text-status-warning' : 'text-text-muted'"
            >{{ cameraStatusText }}</span>
          </div>
          <Button
            severity="secondary"
            size="small"
            label="关闭摄像头"
            @click="closeCamera"
          />
        </div>
      </div>

      <!-- Manual input (always visible) -->
      <div class="flex items-end gap-3">
        <div class="flex-1">
          <label class="mono-label block mb-1">设备序列号</label>
          <InputText
            id="checkin-serial-input"
            v-model="serialNo"
            class="w-full"
            placeholder="扫描或输入序列号"
            :disabled="scanning"
            @keydown.enter="doCheckin()"
          />
        </div>
        <Button
          severity="primary"
          :disabled="scanning || !serialNo.trim()"
          :label="scanning ? '处理中...' : '入库'"
          @click="doCheckin()"
        />
      </div>

      <!-- Undo button with countdown -->
      <div v-if="lastCheckin" class="mt-3 flex items-center gap-2">
        <span class="font-mono text-xs text-text-muted">
          已入库 {{ lastCheckin.record.serialNo }}
        </span>
        <Button
          severity="secondary"
          size="small"
          :disabled="undoing"
          :label="undoing ? '撤销中...' : `↩ 撤销 (${undoCountdown} s)`"
          @click="undoLastCheckin"
        />
      </div>
    </div>

    <!-- ══════════ Results Panel ══════════ -->
    <div class="panel">
      <div class="flex items-center justify-between mb-3">
        <h2 class="panel-title mb-0">入库记录</h2>
        <div class="flex gap-2">
          <Button
            v-if="records.length > 0"
            severity="secondary"
            size="small"
            label="导出CSV"
            @click="downloadCSV"
          />
          <Button
            v-if="records.length > 0"
            severity="secondary"
            size="small"
            label="清空记录"
            @click="clearRecords"
          />
        </div>
      </div>

      <div v-if="records.length === 0" class="py-6 text-center">
        <span class="font-mono text-xs text-text-muted">暂无入库记录</span>
      </div>

      <!-- Grouped history -->
      <div v-else>
        <div
          v-for="group in groupedRecords"
          :key="group.dateKey"
          class="mb-3"
        >
          <!-- Group header -->
          <div
            class="flex items-center gap-2 cursor-pointer select-none py-1.5 px-1 hover:bg-surface-hover transition-colors duration-100"
            @click="toggleGroup(group.dateKey)"
          >
            <span class="font-mono text-xs text-text-muted w-4 text-center">
              {{ isGroupExpanded(group.dateKey) ? '▼' : '▶' }}
            </span>
            <span class="font-mono text-sm text-text-primary font-bold">{{ group.sublabel }}</span>
            <span class="font-mono text-sm text-text-secondary">{{ group.label }}</span>
            <span class="font-mono text-xs text-text-muted">共 {{ group.records.length }} 件</span>
          </div>

          <!-- Group records -->
          <div v-if="isGroupExpanded(group.dateKey)" class="space-y-2 mt-1">
            <div
              v-for="(r, idx) in group.records"
              :key="`${group.dateKey}-${idx}`"
              class="border px-3 py-2 font-mono text-xs"
              :class="resultClass(r)"
            >
              <div class="flex items-center justify-between">
                <span class="tracking-wider">{{ r.serialNo }}</span>
                <span class="text-text-muted text-xs">{{ r.time }}</span>
              </div>
              <div class="mt-1">
                <Tag v-if="r.ok" severity="success" :value="r.message" />
                <Tag v-else severity="danger" :value="r.message || r.action" />
              </div>
              <div
                v-if="r.beforeStatus || r.afterStatus"
                class="mt-1 text-text-muted text-xs"
              >
                状态: {{ r.beforeStatus || '-' }} → {{ r.afterStatus || '-' }}
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
    <div class="version-footer">2026 — V1.05 — REV.N</div>
  </div>
</template>
