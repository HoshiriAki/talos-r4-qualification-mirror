<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { setLocale, type Locale } from '@/i18n'
import { registeredComponents, type UiComponentDefinition, type UiLabControlDefinition } from '@/ui'
import LabToolbar from './components/LabToolbar.vue'
import LabRegistryPane from './components/LabRegistryPane.vue'
import LabSandboxViewport from './components/LabSandboxViewport.vue'
import LabInspectorPane from './components/LabInspectorPane.vue'
import LabBottomDock from './components/LabBottomDock.vue'
import { captureExportBundle } from './export/agent-export'
import { mergeJournalChange, type LabJournalEntry } from './state/change-journal'
import {
  cloneLabSession,
  collectLabControlPaths,
  collectLabSlotPaths,
  createInitialLabSession,
  LAB_DEFAULT_VIEWPORTS,
  labPathForControl,
  readLabPath,
  sameLabValue,
  validateLabSession,
  writeLabPath,
  LAB_SCHEMA_VERSION,
  type LabSessionState,
  type UiLabPatchSource,
} from './state/lab-session'
import { validateLabPathValue } from './state/lab-value'
import { labText, type UiLabLocale } from './locale'

const sessionId = `lab-${crypto.randomUUID()}`
const selectedId = ref(registeredComponents[0]?.id ?? '')
const baselineState = ref<LabSessionState>(
  createInitialLabSession(selectedId.value, LAB_DEFAULT_VIEWPORTS['desktop-1600']),
)
const state = ref<LabSessionState>(cloneLabSession(baselineState.value))
const mobilePane = ref<'registry' | 'inspector' | null>(null)
const motionProgress = ref(0)
const query = ref('')
const changes = ref<LabJournalEntry[]>([])
const auditHistory = ref<LabJournalEntry[]>([])
const diagnostics = ref<string[]>([])
const sandboxViewport = ref<InstanceType<typeof LabSandboxViewport> | null>(null)
const sandboxReady = ref(false)
const sandboxLifecycle = ref<Array<{ ready: boolean; epoch: string | null }>>([])
const pendingCommands = ref<unknown[]>([])
const MAX_PENDING_COMMANDS = 32
let resyncScheduled = false
let resyncWindowStarted = 0
let resyncAttempts = 0
const handshakeRequested = ref(false)

const selectedComponent = computed<UiComponentDefinition>(() =>
  registeredComponents.find((item) => item.id === selectedId.value) ?? registeredComponents[0],
)
const locale = computed<UiLabLocale>(() => state.value.environment.locale)

const filteredComponents = computed(() => {
  const needle = query.value.trim().toLowerCase()
  return !needle
    ? registeredComponents
    : registeredComponents.filter((item) =>
        `${item.id} ${item.title} ${item.category}`.toLowerCase().includes(needle),
      )
})

const allowedPaths = computed(() => {
  const set = new Set<string>()
  for (const path of collectLabControlPaths(selectedComponent.value.lab?.controls ?? [])) set.add(path)
  for (const path of collectLabSlotPaths(selectedComponent.value.lab?.designSlots ?? [])) set.add(path)
  set.add('component.id')
  set.add('scene.id')
  set.add('environment.theme')
  set.add('environment.locale')
  set.add('environment.direction')
  set.add('environment.viewport')
  set.add('environment.zoom')
  set.add('environment.reducedMotion')
  set.add('motion.activeMotionId')
  set.add('motion.engine')
  set.add('motion.playbackRate')
  set.add('motion.loop')
  set.add('motion.currentTime')
  set.add('motion.status')
  return set
})

const controlValues = computed(() =>
  Object.fromEntries(
    (selectedComponent.value.lab?.controls ?? []).map((control) => [
      `${control.scope}:${control.path}`,
      readLabPath(state.value, labPathForControl(control.scope, control.path)) ?? control.default,
    ]),
  ),
)
const slotValues = computed(() =>
  Object.fromEntries(
    (selectedComponent.value.lab?.designSlots ?? []).map((slot) => [
      slot.id,
      state.value.component.slots[slot.id] ?? { enabled: slot.enabledByDefault, content: null },
    ]),
  ),
)
const exportBundle = computed(() => captureExportBundle(
  selectedComponent.value,
  baselineState.value,
  state.value,
  changes.value,
  diagnostics.value,
  auditHistory.value,
))
const snapshot = computed(() => exportBundle.value.snapshot)
const diff = computed(() => exportBundle.value.diff)
const prompt = computed(() => exportBundle.value.prompt)

function addDiagnostic(message: string) {
  diagnostics.value.push(message)
  if (diagnostics.value.length > 200) diagnostics.value.splice(0, diagnostics.value.length - 200)
}

// ── 权威状态写入 ─────────────────────────────────────────────────────
function commitLabPatch(path: string, value: unknown, source: UiLabPatchSource, emitToSandbox = true) {
  if (!allowedPaths.value.has(path)) {
    addDiagnostic(`Rejected unregistered path: ${path}`)
    return false
  }
  const before = readLabPath(state.value, path)
  try {
    if (sameLabValue(before, value)) return false
  } catch (error) {
    addDiagnostic(`Rejected non-serializable value at ${path}: ${error instanceof Error ? error.message : String(error)}`)
    return false
  }
  let safeValue: unknown
  try {
    safeValue = JSON.parse(JSON.stringify(value))
  } catch (error) {
    addDiagnostic(`Rejected non-serializable value at ${path}: ${error instanceof Error ? error.message : String(error)}`)
    return false
  }
  const valueReason = validateLabPathValue(selectedComponent.value, path, safeValue)
  if (valueReason) {
    addDiagnostic(`Rejected value at ${path}: ${valueReason}`)
    return false
  }
  const baseRevision = state.value.revision
  writeLabPath(state.value, path, safeValue, { allowedPaths: allowedPaths.value })
  state.value.revision = baseRevision + 1
  const entry = {
    id: crypto.randomUUID(),
    timestamp: new Date().toISOString(),
    revision: state.value.revision,
    source,
    path,
    before,
     after: safeValue,
    componentId: state.value.component.id,
  }
  changes.value = mergeJournalChange(changes.value, entry)
  auditHistory.value = [...auditHistory.value, entry].slice(-1000)
  if (emitToSandbox) {
    sandboxViewport.value?.send('LAB_PATCH', { revision: state.value.revision, baseRevision, path, value, source })
  }
  return true
}

function initializeComponent(component: UiComponentDefinition) {
  const session = createInitialLabSession(component.id, LAB_DEFAULT_VIEWPORTS['desktop-1600'])
  for (const control of component.lab?.controls ?? []) {
    writeLabPath(session, labPathForControl(control.scope, control.path), control.default)
  }
  for (const slot of component.lab?.designSlots ?? []) {
    session.component.slots[slot.id] = { enabled: slot.enabledByDefault, content: null }
  }
  session.scene.id = component.lab?.preview?.defaultScene ?? 'surface'
  baselineState.value = cloneLabSession(session)
  state.value = cloneLabSession(session)
  changes.value = []
  auditHistory.value = []
  sandboxReady.value = false
  pendingCommands.value = []
  handshakeRequested.value = false
  motionProgress.value = 0
}

function selectComponent(id: string) {
  if (id === selectedId.value) return
  if (changes.value.length) addDiagnostic(labText(locale.value, 'switchClearedChanges'))
  selectedId.value = id
}

function handleMobileComponentSelect(id: string) {
  selectComponent(id)
  mobilePane.value = null
}

function applyControl(control: UiLabControlDefinition, after: unknown) {
  const path = labPathForControl(control.scope, control.path)
  if (!allowedPaths.value.has(path)) {
    addDiagnostic(`Rejected unregistered path: ${path}`)
    return
  }
  commitLabPatch(path, after, 'inspector')
}

function applySlotChange(slotId: string, patch: { enabled?: boolean; content?: string | null }) {
  if (patch.enabled !== undefined) {
    commitLabPatch(`component.slots.${slotId}.enabled`, Boolean(patch.enabled), 'inspector')
  }
  if (patch.content !== undefined) {
    commitLabPatch(`component.slots.${slotId}.content`, patch.content, 'inspector')
  }
}

// ── Scene / Environment ─────────────────────────────────────────────
function setScene(id: string) {
  commitLabPatch('scene.id', id, 'scene')
}
function setTheme(value: 'dark' | 'light') {
  commitLabPatch('environment.theme', value, 'environment')
}
function setLocaleForLab(value: Locale) {
  commitLabPatch('environment.locale', value, 'environment')
  setLocale(value)
}
function setDirection(value: 'ltr' | 'rtl') {
  commitLabPatch('environment.direction', value, 'environment')
}
function setViewport(id: string) {
  const viewport = LAB_DEFAULT_VIEWPORTS[id]
  if (viewport) commitLabPatch('environment.viewport', viewport, 'environment')
}
function setZoom(value: number) {
  commitLabPatch('environment.zoom', value, 'environment')
}
function setReducedMotion(value: boolean) {
  commitLabPatch('environment.reducedMotion', value, 'environment')
}

// ── Motion ──────────────────────────────────────────────────────────
function selectMotion(id: string) {
  commitLabPatch('motion.activeMotionId', id, 'timeline')
}
function requireSandboxReady(): boolean {
  return true
}
function sendSandboxCommand(payload: unknown) {
  if (!sandboxReady.value) {
    const needsHandshake = pendingCommands.value.length === 0 && !handshakeRequested.value
    if (pendingCommands.value.length >= MAX_PENDING_COMMANDS) pendingCommands.value.shift()
    pendingCommands.value.push(payload)
    if (needsHandshake) {
      handshakeRequested.value = true
      sandboxViewport.value?.sendInit()
    }
    return
  }
  sandboxViewport.value?.send('LAB_COMMAND', payload)
}
function requestSandboxResync(reason: string) {
  if (!sandboxViewport.value || reason.includes('message delivery failed')) return
  const now = Date.now()
  if (now - resyncWindowStarted > 5_000) {
    resyncWindowStarted = now
    resyncAttempts = 0
  }
  if (resyncAttempts >= 3 || resyncScheduled) return
  resyncAttempts += 1
  resyncScheduled = true
  window.setTimeout(() => {
    resyncScheduled = false
    sandboxViewport.value?.sendInit()
  }, 0)
}
function onSandboxNotReady() {
  sandboxReady.value = false
  handshakeRequested.value = false
  sandboxLifecycle.value = [...sandboxLifecycle.value, {
    ready: false,
    epoch: sandboxViewport.value?.getFrameEpoch?.() ?? null,
  }].slice(-40)
}
function onSandboxReady() {
  sandboxReady.value = true
  handshakeRequested.value = false
  sandboxLifecycle.value = [...sandboxLifecycle.value, {
    ready: true,
    epoch: sandboxViewport.value?.getFrameEpoch?.() ?? null,
  }].slice(-40)
  resyncAttempts = 0
  const queued = pendingCommands.value.splice(0)
  for (const payload of queued) sandboxViewport.value?.send('LAB_COMMAND', payload)
}
function playMotion() {
  if (!requireSandboxReady()) return
  sendSandboxCommand({ command: 'motion.play' })
}
function pauseMotion() {
  if (!requireSandboxReady()) return
  sendSandboxCommand({ command: 'motion.pause' })
}
function restartMotion() {
  if (!requireSandboxReady()) return
  sendSandboxCommand({ command: 'motion.restart' })
}
function seekMotion(progress: number) {
  motionProgress.value = progress
  commitLabPatch('motion.currentTime', progress, 'timeline')
  sendSandboxCommand({ command: 'motion.seek', progress })
}
function setMotionSettings(value: { playbackRate: number; loop: boolean; engine: 'auto' | 'anime' | 'gsap' | 'native' }) {
  commitLabPatch('motion.playbackRate', value.playbackRate, 'timeline')
  commitLabPatch('motion.loop', value.loop, 'timeline')
  commitLabPatch('motion.engine', value.engine, 'timeline')
}

// ── Reset / baseline ────────────────────────────────────────────────
async function resetAll() {
  const before = cloneLabSession(state.value)
  const next = cloneLabSession(baselineState.value)
  const baseRevision = state.value.revision
  next.revision = baseRevision + 1
  state.value = next
  changes.value = []
   auditHistory.value = [...auditHistory.value, {
    id: crypto.randomUUID(), timestamp: new Date().toISOString(), revision: state.value.revision,
     source: 'system' as const, path: '$session.reset', before, after: cloneLabSession(next), componentId: next.component.id,
   }].slice(-1000)
  motionProgress.value = 0
  addDiagnostic(labText(locale.value, 'sessionReset'))
  // 等 props 反映新 state 后再以完整 Snapshot 重建 sandbox
  await nextTick()
  sandboxViewport.value?.sendInit()
}

function resetPage() {
  for (const control of selectedComponent.value.lab?.controls?.filter((item) => item.scope === 'component') ?? []) {
    commitLabPatch(labPathForControl(control.scope, control.path), readLabPath(baselineState.value, labPathForControl(control.scope, control.path)), 'system')
  }
  addDiagnostic(labText(locale.value, 'componentReset'))
}

// ── Sandbox 回传 ────────────────────────────────────────────────────
function onSandboxPatch(payload: { revision: number; baseRevision: number; path: string; value: unknown }) {
  if (payload.baseRevision !== state.value.revision || payload.revision !== payload.baseRevision) {
    addDiagnostic(`Rejected stale sandbox patch at r${payload.baseRevision}; host is r${state.value.revision}`)
    requestSandboxResync('stale sandbox patch')
    return
  }
  if (!allowedPaths.value.has(payload.path)) {
    addDiagnostic(`Rejected sandbox path: ${payload.path}`)
    requestSandboxResync('invalid sandbox path')
    return
  }
  commitLabPatch(payload.path, payload.value, 'sandbox')
}

function onSandboxEvent(payload: Record<string, unknown>) {
  if (payload.type === 'motion-state') {
    onMotionState(payload)
  } else if (payload.type === 'diagnostic') {
    if (payload.code === 'motionTargetMissing') {
      addDiagnostic(`${labText(locale.value, 'motionTargetMissing')}: ${String(payload.target)} (${String(payload.motionId ?? '-')})`)
    } else if (payload.code === 'motionSlotDisabled') {
      addDiagnostic(`${labText(locale.value, 'motionTargetMissing')}: disabled slot ${String(payload.target)} (${String(payload.motionId ?? '-')})`)
    } else {
      addDiagnostic(`Sandbox diagnostic: ${JSON.stringify(payload)}`)
    }
  } else if (payload.type === 'interaction') {
    addDiagnostic(`Sandbox interaction: ${String(payload.action ?? payload.type)}`)
  }
}

function onMotionState(payload: Record<string, unknown>) {
  const action = String(payload.action ?? '')
  if (action === 'progress') {
    motionProgress.value = Number(payload.progress ?? motionProgress.value)
    return
  }
  const statusMap: Record<string, 'playing' | 'paused' | 'finished' | 'idle'> = {
    play: 'playing', pause: 'paused', finish: 'finished', restart: 'playing',
  }
  if (action in statusMap) commitLabPatch('motion.status', statusMap[action], 'sandbox')
  if ((action === 'finish' || action === 'seek' || action === 'pause') && typeof payload.progress === 'number') {
    motionProgress.value = payload.progress
    commitLabPatch('motion.currentTime', payload.progress, 'sandbox')
  }
  addDiagnostic(`Motion: ${JSON.stringify(payload)}`)
}

function onSandboxError(message: string) {
  addDiagnostic(`Sandbox error: ${message}`)
  requestSandboxResync(message)
}

function restoreSnapshot(candidate: unknown): boolean {
  if (!candidate || typeof candidate !== 'object') {
    addDiagnostic('Snapshot restore rejected: snapshot must be an object')
    return false
  }
  const snapshot = candidate as {
    schemaVersion?: unknown
    componentId?: unknown
    baselineRevision?: unknown
    currentRevision?: unknown
    baseline?: unknown
    current?: unknown
  }
  if (snapshot.schemaVersion !== LAB_SCHEMA_VERSION || snapshot.componentId !== selectedComponent.value.id
    || !Number.isInteger(snapshot.baselineRevision) || !Number.isInteger(snapshot.currentRevision)
    || !snapshot.baseline || !snapshot.current) {
    addDiagnostic('Snapshot restore rejected: schema or current session mismatch')
    return false
  }
  const validation = validateLabSession(snapshot.current, {
    componentId: selectedComponent.value.id,
    expectedRevision: snapshot.currentRevision as number,
    allowedPaths: allowedPaths.value,
    component: selectedComponent.value,
  })
  if (!validation.ok) {
    addDiagnostic(`Snapshot restore rejected: ${validation.reason ?? 'invalid session'}`)
    return false
  }
  const baselineValidation = validateLabSession(snapshot.baseline, {
    componentId: selectedComponent.value.id,
    expectedRevision: snapshot.baselineRevision as number,
    allowedPaths: allowedPaths.value,
    component: selectedComponent.value,
  })
  if (!baselineValidation.ok) {
    addDiagnostic(`Snapshot restore rejected: ${baselineValidation.reason ?? 'invalid baseline'}`)
    return false
  }
  const previous = cloneLabSession(state.value)
  const restored = cloneLabSession(snapshot.current as LabSessionState)
  const nextRevision = Math.max(previous.revision, restored.revision) + 1
  restored.revision = nextRevision
  baselineState.value = cloneLabSession(restored)
  state.value = cloneLabSession(restored)
  changes.value = []
  auditHistory.value = [...auditHistory.value, {
    id: crypto.randomUUID(), timestamp: new Date().toISOString(), revision: nextRevision,
    source: 'system' as const, path: '$session.restore', before: previous, after: cloneLabSession(restored), componentId: restored.component.id,
  }].slice(-1000)
  motionProgress.value = restored.motion.currentTime
  addDiagnostic(`Snapshot restored as new baseline at revision ${nextRevision}`)
  void nextTick().then(() => sandboxViewport.value?.sendInit())
  return true
}

function restoreSnapshotFromText(text: string) {
  try {
    restoreSnapshot(JSON.parse(text))
  } catch (error) {
    addDiagnostic(`Snapshot restore rejected: ${error instanceof Error ? error.message : String(error)}`)
  }
}

function copy(text: string) {
  navigator.clipboard?.writeText(text)
    .then(() => addDiagnostic(labText(locale.value, 'copiedToClipboard')))
    .catch(() => addDiagnostic('Clipboard permission was unavailable.'))
}
function download(name: string, text: string) {
  const anchor = document.createElement('a')
  anchor.href = URL.createObjectURL(new Blob([text], { type: 'application/json' }))
  anchor.download = name
  anchor.click()
  URL.revokeObjectURL(anchor.href)
  addDiagnostic(`${labText(locale.value, 'downloadStarted')} ${name}`)
}

watch(selectedId, () => initializeComponent(selectedComponent.value), { immediate: true })

// ── Dev-only test seam (tree-shaken from production builds) ─────────────
if (import.meta.env.DEV) {
  ;(window as unknown as Record<string, unknown>).__uiLabDebug = {
    getState: () => state.value,
    getBaseline: () => baselineState.value,
    getChanges: () => changes.value,
    getDiff: () => diff.value,
    getSnapshot: () => snapshot.value,
    getPrompt: () => prompt.value,
    getExportBundle: () => captureExportBundle(selectedComponent.value, baselineState.value, state.value, changes.value, diagnostics.value, auditHistory.value),
    getAuditHistory: () => auditHistory.value,
    getDiagnostics: () => diagnostics.value,
    getSandboxContext: () => ({ ready: sandboxReady.value, epoch: sandboxViewport.value?.getFrameEpoch?.() ?? null, sessionId }),
    getSandboxLifecycle: () => sandboxLifecycle.value,
    injectSandboxMessage: (message: unknown) => sandboxViewport.value?.receiveTestMessage?.(message),
    resyncSandbox: () => sandboxViewport.value?.sendInit(),
    restoreSnapshot,
    // 走与 Inspector 相同的 commitLabPatch 闭环（含 Journal + LAB_PATCH 下发），
    // 仅用于自动化测试/截图驱动。
    applyPatch: (path: string, value: unknown) => {
      commitLabPatch(path, value, 'system')
    },
  }
}
</script>

<template>
  <main class="ui-lab-vnext" :class="`theme-${state.environment.theme}`">
    <LabToolbar
      :component="selectedComponent"
      :scene-id="state.scene.id"
      :viewport-id="state.environment.viewport.id"
      :zoom="state.environment.zoom"
      :theme="state.environment.theme"
      :locale="state.environment.locale"
      :direction="state.environment.direction"
      :reduced-motion="state.environment.reducedMotion"
      :dirty="changes.length > 0"
      @update:scene-id="setScene"
      @update:viewport-id="setViewport"
      @update:zoom="setZoom"
      @update:theme="setTheme"
      @update:locale="setLocaleForLab"
      @update:direction="setDirection"
      @toggle-registry="mobilePane = mobilePane === 'registry' ? null : 'registry'"
      @toggle-inspector="mobilePane = mobilePane === 'inspector' ? null : 'inspector'"
      @reset="resetAll"
    />
    <section class="workspace">
      <LabRegistryPane
        :components="filteredComponents"
        :selected-id="selectedId"
        :query="query"
        :locale="state.environment.locale"
        @update:query="query = $event"
        @select="selectComponent"
      />
      <LabSandboxViewport
        ref="sandboxViewport"
        :session-id="sessionId"
        :session="state"
        @ready="onSandboxReady"
        @not-ready="onSandboxNotReady"
        @patch="onSandboxPatch"
        @event="onSandboxEvent"
        @error="onSandboxError"
      />
      <LabInspectorPane
        :component="selectedComponent"
        :values="controlValues"
        :slots="selectedComponent.lab?.designSlots ?? []"
        :slot-values="slotValues"
        :scene-id="state.scene.id"
        :locale="state.environment.locale"
        @change="applyControl"
        @slot-change="applySlotChange"
        @reset-page="resetPage"
        @reset-all="resetAll"
      />
    </section>
      <LabBottomDock
      :component="selectedComponent"
      :changes="changes"
      :prompt="prompt"
      :snapshot="JSON.stringify(snapshot, null, 2)"
      :diff="JSON.stringify(diff, null, 2)"
      :diagnostics="diagnostics"
      :locale="state.environment.locale"
      :progress="motionProgress"
      :active-motion-id="state.motion.activeMotionId"
      :playback-rate="state.motion.playbackRate"
      :loop="state.motion.loop"
      :engine="state.motion.engine"
      :reduced-motion="state.environment.reducedMotion"
      @copy="copy"
      @download="download"
        @restore="restoreSnapshotFromText"
        @restore-error="(message) => addDiagnostic(`Snapshot restore file rejected: ${message}`)"
      @select-motion="selectMotion"
      @play="playMotion"
      @pause="pauseMotion"
      @restart="restartMotion"
      @seek="seekMotion"
      @settings="setMotionSettings"
      @toggle-reduced-motion="setReducedMotion"
    />
    <div v-if="mobilePane" class="mobile-pane-backdrop" @click="mobilePane = null" />
    <section v-if="mobilePane" class="mobile-pane" :aria-label="mobilePane === 'registry' ? labText(locale, 'registry') : labText(locale, 'inspector')">
      <button class="mobile-pane-close" type="button" :aria-label="labText(locale, 'collapse')" @click="mobilePane = null">×</button>
      <LabRegistryPane
        v-if="mobilePane === 'registry'"
        :components="filteredComponents"
        :selected-id="selectedId"
        :query="query"
        :locale="state.environment.locale"
        @update:query="query = $event"
        @select="handleMobileComponentSelect"
      />
      <LabInspectorPane
        v-else
        :component="selectedComponent"
        :values="controlValues"
        :slots="selectedComponent.lab?.designSlots ?? []"
        :slot-values="slotValues"
        :scene-id="state.scene.id"
        :locale="state.environment.locale"
        @change="applyControl"
        @slot-change="applySlotChange"
        @reset-page="resetPage"
        @reset-all="resetAll"
      />
    </section>
  </main>
</template>

<style scoped>
.ui-lab-vnext { --lab-edge: 12px; display: grid; grid-template-rows: 38px minmax(430px, 1fr) 240px; gap: var(--lab-edge); height: 100vh; min-height: 700px; box-sizing: border-box; padding: var(--lab-edge); background: var(--bg-canvas); color: var(--text-primary); }
.ui-lab-vnext.theme-light { background: #ebebed; }
.workspace { display: grid; grid-template-columns: minmax(220px, 250px) minmax(0, 1fr) minmax(300px, 360px); min-height: 0; border: 1px solid var(--border-strong); background: var(--bg-surface); }
.mobile-pane, .mobile-pane-backdrop { display: none; }
.mobile-pane-close { position: absolute; top: 6px; right: 6px; z-index: 2; width: 26px; height: 26px; border: 1px solid var(--border-strong); background: var(--bg-field); color: var(--text-secondary); font-family: var(--font-mono); cursor: pointer; }
.mobile-pane-close:hover { color: var(--text-primary); }
@media (max-width: 1199px) {
  .ui-lab-vnext { grid-template-rows: 38px minmax(430px, 1fr) 240px; }
  .workspace { grid-template-columns: minmax(180px, 220px) minmax(0, 1fr); }
  .workspace > :last-child { display: none; }
  .mobile-pane-backdrop { display: block; position: fixed; inset: 0; background: rgb(0 0 0 / 0.4); z-index: 20; }
  .mobile-pane { display: block; position: fixed; z-index: 21; top: 56px; right: 12px; bottom: 12px; width: min(360px, calc(100vw - 24px)); border: 1px solid var(--border-strong); background: var(--bg-surface); box-shadow: 4px 4px 0 var(--bg-canvas); }
}
@media (max-width: 760px) {
  .ui-lab-vnext { --lab-edge: 7px; grid-template-rows: 38px minmax(370px, 1fr) 240px; min-height: 660px; }
  .workspace { grid-template-columns: 1fr; }
  .workspace > :first-child { display: none; }
  .mobile-pane { top: 50px; right: 7px; bottom: 7px; width: calc(100vw - 14px); }
}
</style>
