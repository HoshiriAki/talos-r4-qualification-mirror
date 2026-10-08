<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { getComponentDefinition, TalosButton, TalosLoadingOverlayFixture, TalosStatusIndicator, type TalosButtonVariant, type TalosStatus } from '@/ui'
import {
  createLabMessage,
  isLabProtocolEnvelope,
  validateLabMessage,
  type LabCommandPayload,
  type LabInitPayload,
  type LabPatchPayload,
} from './sandbox-protocol'
import { LAB_SANDBOX_SAFE_SELECTORS, probeSelectorFor, realSlotIdsFor, supportsSandboxComponent } from './sandbox-renderer'
import { resolveMotionTarget } from './motion-target'
import { createMotionPlayback, type UiLabMotionPlayback } from '../motion-engine'
import {
  assertSafeLabPath,
  cloneLabSession,
  collectLabControlPaths,
  collectLabSlotPaths,
  createInitialLabSession,
  LAB_DEFAULT_VIEWPORTS,
  readLabPath,
  writeLabPath,
  type LabSessionState,
} from '../state/lab-session'

const sessionId = ref('')
const frameEpoch = ref('')
const state = ref<LabSessionState>(createInitialLabSession('', LAB_DEFAULT_VIEWPORTS['desktop-1600']))
const incomingSequence = ref(0)
const outgoingSequence = ref(0)
const appliedRevision = ref(0)
const specimenRoot = ref<HTMLElement | null>(null)
let activePlayback: UiLabMotionPlayback | null = null
let lastReportedProgress = -1
let playbackGeneration = 0

const componentId = computed(() => state.value.component.id)
const sceneId = computed(() => state.value.scene.id)
const environment = computed(() => state.value.environment)
const component = computed(() => (componentId.value ? getComponentDefinition(componentId.value) : undefined))
const allowedPaths = computed(() => {
  const paths = new Set<string>(['component.id', 'scene.id', 'environment.theme', 'environment.locale', 'environment.direction', 'environment.viewport', 'environment.zoom', 'environment.reducedMotion', 'motion.activeMotionId', 'motion.engine', 'motion.playbackRate', 'motion.loop', 'motion.currentTime', 'motion.status'])
  for (const path of collectLabControlPaths(component.value?.lab?.controls ?? [])) paths.add(path)
  for (const path of collectLabSlotPaths(component.value?.lab?.designSlots ?? [])) paths.add(path)
  return paths
})
const isButton = computed(() => componentId.value === 'talos.ui.button')
const isStatusIndicator = computed(() => componentId.value === 'talos.ui.status-indicator')
const isLoadingOverlay = computed(() => componentId.value === 'talos.ui.loading-overlay')

const STATUS_CYCLE = ['completed', 'active', 'pending', 'overdue', 'cancelled'] as const
const STATUS_LABELS: Record<string, { 'zh-CN': string; en: string }> = {
  completed: { 'zh-CN': '已完成', en: 'Completed' },
  active: { 'zh-CN': '进行中', en: 'Active' },
  pending: { 'zh-CN': '待处理', en: 'Pending' },
  overdue: { 'zh-CN': '已逾期', en: 'Overdue' },
  cancelled: { 'zh-CN': '已取消', en: 'Cancelled' },
}

function readProp(key: string): unknown {
  return readLabPath(state.value, `component.props.${key}`)
}

const buttonProps = computed(() => ({
  variant: (String(readProp('variant') ?? 'execution-primary')) as TalosButtonVariant,
  state: (String(readProp('state') ?? 'default')) as 'default' | 'executing' | 'disabled',
  size: (String(readProp('size') ?? 'default')) as 'small' | 'default' | 'large',
}))
const buttonLabel = computed(() =>
  String(readProp('label') ?? (environment.value.locale === 'zh-CN' ? '执行转换' : 'Run transition')),
)
const statusProps = computed(() => {
  const status = String(readProp('status') ?? 'active') as TalosStatus
  return { status, label: STATUS_LABELS[status]?.[environment.value.locale] ?? status }
})
const loadingMessage = computed(() =>
  String(readProp('message') ?? (environment.value.locale === 'zh-CN' ? '正在同步本地样本' : 'Synchronizing local fixture')),
)
const slotStates = computed(() => component.value?.lab?.designSlots?.map((slot) => ({
  ...slot,
  state: state.value.component.slots[slot.id] ?? { enabled: slot.enabledByDefault, content: null },
})) ?? [])
// 只有组件源码无 [data-lab-slot] anchor 的 slot 才渲染显式 adapter
// （data-lab-slot-adapter），与真实 data-lab-slot 互不共享选择器。
const slotAdapters = computed(() => {
  const real = realSlotIdsFor(componentId.value)
  return slotStates.value.filter((slot) => !real.has(slot.id))
})
function slotContent(slotId: string): string | null {
  const slotState = state.value.component.slots[slotId]
  if (!slotState || !slotState.enabled || slotState.content === null) return null
  return slotState.content
}
const buttonLabelText = computed(() => slotContent('primary-content') ?? buttonLabel.value)
const statusLabelText = computed(() => slotContent('status-label') ?? statusProps.value.label)
const loadingMessageText = computed(() => slotContent('loading-message') ?? loadingMessage.value)
const maskOpacity = computed(() => Number(readLabPath(state.value, 'scene.maskOpacity') ?? 0.45))
const unsupportedLabel = computed(() =>
  environment.value.locale === 'zh-CN' ? '不支持的沙箱组件。' : 'Unsupported sandbox component.',
)

function post(type: Parameters<typeof createLabMessage>[3], revision: number, payload: unknown) {
  try {
    const serializable = JSON.parse(JSON.stringify(payload)) as unknown
    parent.postMessage(
      createLabMessage(sessionId.value, ++outgoingSequence.value, revision, type, componentId.value, serializable, frameEpoch.value || 'legacy'),
      location.origin,
    )
  } catch (error) {
    console.error('[UI Lab] sandbox message delivery failed', error)
  }
}

function emitSnapshot() {
  post('SANDBOX_SNAPSHOT', state.value.revision, { revision: state.value.revision, session: state.value })
}

function emitMeasure() {
  const rect = specimenRoot.value?.getBoundingClientRect()
  if (rect) {
    post('SANDBOX_EVENT', state.value.revision, {
      type: 'measure', width: Math.round(rect.width), height: Math.round(rect.height), viewport: environment.value.viewport,
    })
  }
}

function applyHostPatch(patch: LabPatchPayload) {
  try {
    if (patch.baseRevision !== appliedRevision.value || patch.revision <= patch.baseRevision) throw new Error(`stale base revision ${patch.baseRevision}; sandbox is ${appliedRevision.value}`)
    assertSafeLabPath(patch.path)
    writeLabPath(state.value, patch.path, patch.value, { allowedPaths: allowedPaths.value })
    appliedRevision.value = patch.revision
    state.value.revision = patch.revision
  } catch (error) {
    post('SANDBOX_ERROR', state.value.revision, {
      message: `Patch rejected: ${error instanceof Error ? error.message : String(error)}`,
    })
  }
}

function applyStaticMotionFrame(target: HTMLElement, motion: { keyframes: Array<{ style: Record<string, string | number> }> }) {
  const first = motion.keyframes[0]
  if (first) {
    for (const [property, value] of Object.entries(first.style)) {
      (target.style as unknown as Record<string, string>)[property] = String(value)
    }
  }
}

function playMotion() {
  const motion = component.value?.lab?.motions.find((item) => item.id === state.value.motion.activeMotionId)
    ?? component.value?.lab?.motions[0]
  if (!motion || !specimenRoot.value) {
    post('SANDBOX_EVENT', state.value.revision, { type: 'motion-state', action: 'play', supported: false })
    return
  }
  const target = resolveMotionTarget(specimenRoot.value, motion.target, {
    safeSelectors: LAB_SANDBOX_SAFE_SELECTORS,
    probeSelector: probeSelectorFor(componentId.value),
  })
  if (!target) {
    post('SANDBOX_EVENT', state.value.revision, {
      type: 'diagnostic', code: 'motionTargetMissing', motionId: motion.id, target: motion.target,
    })
    return
  }
  if (motion.target.startsWith('slot:')) {
    const slotId = motion.target.slice('slot:'.length)
    const slotDef = component.value?.lab?.designSlots?.find((slot) => slot.id === slotId)
    const slotState = state.value.component.slots[slotId]
    if (slotDef && (!slotState || !slotState.enabled)) {
      post('SANDBOX_EVENT', state.value.revision, {
        type: 'diagnostic', code: 'motionSlotDisabled', motionId: motion.id, target: motion.target,
      })
      return
    }
  }
  playbackGeneration += 1
  activePlayback?.destroy()
  activePlayback = null
  const generation = playbackGeneration
  const reducedMotion = environment.value.reducedMotion
    || window.matchMedia('(prefers-reduced-motion: reduce)').matches
  if (reducedMotion) {
    applyStaticMotionFrame(target, motion)
    state.value.motion.status = 'finished'
    state.value.motion.currentTime = 0
    post('SANDBOX_EVENT', state.value.revision, {
      type: 'motion-state', action: 'finish', motionId: motion.id, reduced: true, progress: 0,
    })
    return
  }
  void (async () => {
    let createdPlayback: UiLabMotionPlayback | null = null
    try {
      lastReportedProgress = -1
      const playback = await createMotionPlayback({
        selection: state.value.motion.engine,
        target,
        motion,
        reducedMotion: false,
        loop: state.value.motion.loop,
        playbackRate: state.value.motion.playbackRate,
         onUpdate: (progress) => {
           if (generation !== playbackGeneration) return
          if (Math.abs(progress - lastReportedProgress) >= 0.02 || progress === 1) {
            lastReportedProgress = progress
            state.value.motion.currentTime = progress
            post('SANDBOX_EVENT', state.value.revision, {
              type: 'motion-state', action: 'progress', motionId: motion.id, engine: activePlayback?.engine, progress,
            })
          }
        },
         onComplete: () => {
           if (generation !== playbackGeneration) return
          state.value.motion.status = 'finished'
          post('SANDBOX_EVENT', state.value.revision, {
            type: 'motion-state', action: 'finish', motionId: motion.id, engine: activePlayback?.engine, progress: 1,
          })
        },
      })
      if (generation !== playbackGeneration) {
        playback.destroy()
        return
      }
      createdPlayback = playback
      activePlayback = playback
      activePlayback.play()
      state.value.motion.status = 'playing'
      state.value.motion.currentTime = 0
      post('SANDBOX_EVENT', state.value.revision, {
        type: 'motion-state', action: 'play', motionId: motion.id, engine: activePlayback.engine, progress: 0,
      })
    } catch (error) {
      createdPlayback?.destroy()
      if (activePlayback === createdPlayback) activePlayback = null
      post('SANDBOX_ERROR', state.value.revision, {
        message: `Motion playback failed: ${error instanceof Error ? error.message : String(error)}`,
      })
    }
  })()
}

function pauseMotion() {
  activePlayback?.pause()
  state.value.motion.status = 'paused'
  post('SANDBOX_EVENT', state.value.revision, {
    type: 'motion-state', action: 'pause', engine: activePlayback?.engine, progress: activePlayback?.getProgress() ?? 0,
  })
}

function restartMotion() {
  activePlayback?.restart()
  state.value.motion.status = 'playing'
  post('SANDBOX_EVENT', state.value.revision, { type: 'motion-state', action: 'restart', progress: 0 })
}

function seekMotion(progress: number) {
  activePlayback?.seek(progress)
  state.value.motion.currentTime = progress
  post('SANDBOX_EVENT', state.value.revision, { type: 'motion-state', action: 'seek', progress })
}

function resetInteraction() {
  playbackGeneration += 1
  activePlayback?.destroy()
  activePlayback = null
  state.value.motion.status = 'idle'
  state.value.motion.currentTime = 0
  post('SANDBOX_EVENT', state.value.revision, { type: 'interaction', action: 'reset' })
}

async function handleCommand(command: LabCommandPayload) {
  switch (command.command) {
    case 'motion.play': playMotion(); break
    case 'motion.pause': pauseMotion(); break
    case 'motion.restart': restartMotion(); break
    case 'motion.seek': seekMotion(Number(command.progress ?? 0)); break
    case 'snapshot.capture': emitSnapshot(); break
    case 'interaction.reset': resetInteraction(); break
  }
}

function handleCommandSafely(command: LabCommandPayload) {
  void handleCommand(command).catch((error) => {
    post('SANDBOX_ERROR', state.value.revision, {
      message: `Command failed: ${error instanceof Error ? error.message : String(error)}`,
    })
  })
}

function cycleStatus() {
  const current = String(readProp('status') ?? 'active')
  const nextIndex = (STATUS_CYCLE.indexOf(current as (typeof STATUS_CYCLE)[number]) + 1) % STATUS_CYCLE.length
  const next = STATUS_CYCLE[nextIndex]
  try {
    writeLabPath(state.value, 'component.props.status', next, { allowedPaths: allowedPaths.value })
  } catch { return }
  post('SANDBOX_PATCH', appliedRevision.value, { revision: appliedRevision.value, baseRevision: appliedRevision.value, path: 'component.props.status', value: next })
  post('SANDBOX_EVENT', state.value.revision, { type: 'interaction', action: 'cycle-status', componentId: componentId.value })
}

function onStageClick() {
  post('SANDBOX_EVENT', state.value.revision, { type: 'interaction', action: 'stage-click', componentId: componentId.value })
}

function onComponentClick() {
  if (isStatusIndicator.value) {
    cycleStatus()
    return
  }
  post('SANDBOX_EVENT', state.value.revision, { type: 'interaction', action: 'component-click', componentId: componentId.value })
}

function onMessage(event: MessageEvent<unknown>) {
  if (event.origin !== location.origin || event.source !== window.parent || !isLabProtocolEnvelope(event.data)) return
  const message = event.data
  if (sessionId.value && message.sessionId !== sessionId.value) {
    post('SANDBOX_ERROR', state.value.revision, { message: 'Protocol message rejected: session context mismatch; resync required' })
    return
  }
  if (message.type === 'LAB_INIT' && message.frameEpoch !== frameEpoch.value) {
    frameEpoch.value = message.frameEpoch
    incomingSequence.value = 0
    outgoingSequence.value = 0
  } else if (frameEpoch.value && message.frameEpoch !== frameEpoch.value) {
    post('SANDBOX_ERROR', state.value.revision, { message: 'Protocol message rejected: stale frame epoch; resync required' })
    return
  }
  if (message.sequence <= incomingSequence.value) {
    post('SANDBOX_ERROR', state.value.revision, { message: 'Protocol message rejected: stale sequence; resync required' })
    return
  }
  const componentDefinition = getComponentDefinition(message.componentId ?? componentId.value)
  const messageAllowedPaths = new Set<string>(allowedPaths.value)
  for (const path of collectLabControlPaths(componentDefinition?.lab?.controls ?? [])) messageAllowedPaths.add(path)
  for (const path of collectLabSlotPaths(componentDefinition?.lab?.designSlots ?? [])) messageAllowedPaths.add(path)
  const validated = validateLabMessage(message, {
    sessionId: sessionId.value || undefined,
    frameEpoch: frameEpoch.value || undefined,
    componentId: message.type === 'LAB_INIT' ? undefined : componentId.value,
    allowedPaths: messageAllowedPaths,
    component: componentDefinition,
  })
  if (!validated.ok) {
    post('SANDBOX_ERROR', state.value.revision, { message: `Invalid message: ${validated.reason ?? 'unknown'}` })
    return
  }
  incomingSequence.value = message.sequence
  switch (message.type) {
    case 'LAB_INIT': {
      const init = message.payload as LabInitPayload
      playbackGeneration += 1
      activePlayback?.destroy()
      activePlayback = null
      sessionId.value = message.sessionId
      state.value = cloneLabSession(init.session)
      frameEpoch.value = message.frameEpoch
      appliedRevision.value = init.session.revision
      incomingSequence.value = message.sequence
      post('SANDBOX_READY', state.value.revision, { protocol: '1.0' })
      post('SANDBOX_SNAPSHOT', state.value.revision, { revision: state.value.revision, session: state.value })
      requestAnimationFrame(() => emitMeasure())
      break
    }
    case 'LAB_PATCH': applyHostPatch(message.payload as LabPatchPayload); break
    case 'LAB_COMMAND': handleCommandSafely(message.payload as LabCommandPayload); break
    case 'LAB_REQUEST_SNAPSHOT': emitSnapshot(); break
    default: break
  }
}

function markProbe() {
  const root = specimenRoot.value
  if (!root) return
  const selector = probeSelectorFor(componentId.value)
  if (selector) {
    root.querySelector<HTMLElement>(selector)?.setAttribute('data-lab-probe', '')
  }
}

// 将 Registry slot 的 enabled 状态映射到真实组件节点（或 adapter），
// 使 enabled/content 都作用于该唯一权威节点。不修改 Vue 管理的组件状态，
// 仅设置观测属性/class。
function applySlotAttributes() {
  const root = specimenRoot.value
  if (!root) return
  for (const slot of slotStates.value) {
    const real = root.querySelector<HTMLElement>(`[data-lab-slot="${slot.id}"]`)
    const node = real ?? root.querySelector<HTMLElement>(`[data-lab-slot-adapter="${slot.id}"]`)
    if (!node) continue
    node.setAttribute('data-lab-slot-enabled', slot.state.enabled ? 'true' : 'false')
    node.classList.toggle('lab-slot-disabled', !slot.state.enabled)
  }
}

watch(componentId, () => { requestAnimationFrame(markProbe) }, { flush: 'post' })
// slotStates 的依赖是 slot state 对象引用；writeLabPath 只改内部属性时
// 对象引用不变、watch 不触发。改用属性级快照驱动 applySlotAttributes。
watch(
  () => slotStates.value.map((slot) => `${slot.id}:${slot.state.enabled}`).join('|'),
  () => { requestAnimationFrame(applySlotAttributes) },
  { flush: 'post' },
)

onMounted(() => {
  window.addEventListener('message', onMessage)
  window.addEventListener('resize', emitMeasure)
  requestAnimationFrame(markProbe)
  requestAnimationFrame(applySlotAttributes)
  // 主动握手：Sandbox 挂载完成后通知 Host。若此时尚未收到 INIT
  // （例如 iframe 内应用挂载慢于 Host 的 onLoad），Host 会据此重发 INIT。
  post('SANDBOX_READY', state.value.revision, { protocol: '1.0' })
})

onBeforeUnmount(() => {
  window.removeEventListener('message', onMessage)
  window.removeEventListener('resize', emitMeasure)
  playbackGeneration += 1
  activePlayback?.destroy()
  activePlayback = null
})
</script>

<template>
  <main class="sandbox" :class="[`theme-${environment.theme}`, `scene-${sceneId}`]" :dir="environment.direction" :style="{ '--mask-opacity': maskOpacity }">
    <section ref="specimenRoot" class="sandbox-stage" data-lab-specimen @click="onStageClick">
      <TalosLoadingOverlayFixture v-if="isLoadingOverlay" :message="loadingMessageText" :mask-opacity="maskOpacity" />
      <TalosButton v-else-if="isButton" v-bind="buttonProps" @click="onComponentClick">{{ buttonLabelText }}</TalosButton>
      <TalosStatusIndicator v-else-if="isStatusIndicator" v-bind="statusProps" :label="statusLabelText" @click="onComponentClick" />
      <p v-else class="unsupported">{{ unsupportedLabel }}</p>
      <div class="lab-slot-adapters" aria-label="Design slot adapters">
        <span
          v-for="slot in slotAdapters"
          :key="slot.id"
          class="lab-slot-adapter"
          :data-lab-slot-adapter="slot.id"
          :data-lab-slot-enabled="slot.state.enabled ? 'true' : 'false'"
        >{{ slot.state.enabled ? (slot.state.content ?? '') : '' }}</span>
      </div>
    </section>
  </main>
</template>

<style scoped>
.sandbox { box-sizing: border-box; min-height: 100vh; padding: 24px; background: #0d0d0f; color: #f4f4f5; font-family: var(--font-sans); }
.sandbox.theme-light { background: #f4f4f5; color: #141416; }
.sandbox-stage { position: relative; display: grid; place-items: center; min-height: calc(100vh - 48px); overflow: hidden; border: 1px solid #36363a; background: #151518; }
.lab-slot-adapters { position: absolute; inset: 12px; display: flex; align-items: flex-start; justify-content: space-between; gap: 10px; pointer-events: none; z-index: 2; }
.lab-slot-adapter { min-width: 24px; min-height: 20px; padding: 3px 6px; border: 1px dashed currentColor; color: var(--accent, #ff453a); font: 11px/1.2 var(--font-mono); white-space: pre-wrap; }
.lab-slot-adapter.lab-slot-disabled { opacity: 0.32; }
.sandbox-stage :deep([data-lab-slot].lab-slot-disabled) { opacity: 0.32; }
.theme-light .sandbox-stage { background: #fff; border-color: #d4d4d8; }
.scene-blank .sandbox-stage { background: transparent; }
.scene-dashboard-shell .sandbox-stage { background: linear-gradient(90deg, #18181b 0 16%, #121214 16%); }
.scene-auth-shell .sandbox-stage { background: #101012; }
.scene-modal-context .sandbox-stage::before { content: ''; position: absolute; inset: 10%; border: 1px solid #36363a; background: #1b1b1e; box-shadow: 0 0 0 9999px rgb(0 0 0 / 0.55); }
.scene-background-stage .sandbox-stage { background: radial-gradient(circle at 20% 20%, #35353a 0 1px, transparent 1px) 0 0 / 22px 22px, #161619; }
.scene-fullscreen .sandbox-stage, .scene-loading-stage .sandbox-stage, .scene-background-stage .sandbox-stage { min-height: calc(100vh - 48px); }
.unsupported { color: #a1a1aa; }
</style>
