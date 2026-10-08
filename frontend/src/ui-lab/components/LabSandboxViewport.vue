<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { getComponentDefinition } from '@/ui'
import { createLabMessage, isLabProtocolEnvelope, validateLabMessage, type LabProtocolType } from '../sandbox/sandbox-protocol'
import type { LabSessionState } from '../state/lab-session'

const props = defineProps<{
  sessionId: string
  session: LabSessionState
}>()

const emit = defineEmits<{
  ready: []
  notReady: []
  snapshot: [payload: { revision: number; session: LabSessionState }]
  patch: [payload: { revision: number; baseRevision: number; path: string; value: unknown }]
  measure: [payload: { width: number; height: number }]
  event: [payload: Record<string, unknown>]
  error: [payload: string]
}>()

const frame = ref<HTMLIFrameElement | null>(null)
const canvas = ref<HTMLElement | null>(null)
const sequence = ref(0)
const incomingSequence = ref(0)
const isReady = ref(false)
const frameEpoch = ref(`frame-${crypto.randomUUID()}`)
const canvasSize = ref({ width: 0, height: 0 })
const environment = computed(() => props.session.environment)
const viewport = computed(() => environment.value.viewport)
const effectiveZoom = computed(() =>
  Math.min(environment.value.zoom, canvasSize.value.width / viewport.value.width, canvasSize.value.height / viewport.value.height),
)
const viewportStyle = computed(() => ({
  width: `${viewport.value.width * effectiveZoom.value}px`,
  height: `${viewport.value.height * effectiveZoom.value}px`,
  '--sandbox-scale': effectiveZoom.value,
}))
let canvasObserver: ResizeObserver | undefined

function send(type: LabProtocolType, payload: unknown) {
  const target = frame.value?.contentWindow
  if (!target) {
    console.warn('[UI Lab] send skipped: iframe window unavailable', type)
    return
  }
  try {
    const serializablePayload = JSON.parse(JSON.stringify(payload)) as unknown
    target.postMessage(
      createLabMessage(props.sessionId, ++sequence.value, props.session.revision, type, props.session.component.id, serializablePayload, frameEpoch.value),
      location.origin,
    )
  } catch (error) {
    emit('error', `Sandbox message delivery failed: ${error instanceof Error ? error.message : String(error)}`)
  }
}

function sendInit() {
  send('LAB_INIT', { session: props.session })
}

function beginFrameEpoch() {
  frameEpoch.value = `frame-${crypto.randomUUID()}`
  sequence.value = 0
  incomingSequence.value = 0
  isReady.value = false
  emit('notReady')
}

function onLoad() {
  beginFrameEpoch()
  sendInit()
}

function onMessage(event: MessageEvent<unknown>) {
  if (event.origin !== location.origin || event.source !== frame.value?.contentWindow || !isLabProtocolEnvelope(event.data)) return
  const message = event.data
  if (message.sessionId !== props.sessionId && message.sessionId !== '') {
    emit('error', 'Protocol message rejected: session context mismatch; resync required')
    return
  }
  const bootstrapReady = message.type === 'SANDBOX_READY' && message.sessionId === '' && message.frameEpoch === 'legacy'
  if (bootstrapReady) incomingSequence.value = 0
  if (!bootstrapReady && message.frameEpoch !== frameEpoch.value) {
    emit('error', 'Protocol message rejected: stale frame epoch; resync required')
    return
  }
  if (message.sequence <= incomingSequence.value) {
    emit('error', 'Protocol message rejected: stale sequence; resync required')
    return
  }
  const validated = validateLabMessage(message, {
    sessionId: bootstrapReady ? undefined : props.sessionId,
    frameEpoch: bootstrapReady ? undefined : frameEpoch.value,
    componentId: bootstrapReady ? undefined : props.session.component.id,
    component: getComponentDefinition(props.session.component.id),
  })
  if (!validated.ok) {
    emit('error', `Protocol message rejected: ${validated.reason ?? 'unknown'}`)
    return
  }
  if (!bootstrapReady) incomingSequence.value = message.sequence
  if (message.type === 'SANDBOX_READY') {
    if (message.sessionId === props.sessionId) {
      if (!isReady.value) {
        isReady.value = true
        emit('ready')
      }
    } else if (message.sessionId === '') {
      // Sandbox 主动握手（挂载后、可能尚未收到 INIT）→ 重发 INIT
      sendInit()
    }
  } else if (message.type === 'SANDBOX_SNAPSHOT') {
    emit('snapshot', message.payload as { revision: number; session: LabSessionState })
  } else if (message.type === 'SANDBOX_PATCH') {
    emit('patch', message.payload as { revision: number; baseRevision: number; path: string; value: unknown })
  } else if (message.type === 'SANDBOX_EVENT') {
    // Progress and interaction events are ephemeral. A delayed event from an
    // older host revision is discarded; patch/snapshot divergence is the only
    // case that requests an INIT resync.
    if (message.revision !== props.session.revision) return
    const payload = message.payload as Record<string, unknown>
    if (payload.type === 'measure') {
      emit('measure', { width: Number(payload.width ?? 0), height: Number(payload.height ?? 0) })
    } else {
      emit('event', payload)
    }
  } else if (message.type === 'SANDBOX_ERROR') {
    emit('error', String((message.payload as { message?: string }).message ?? 'Sandbox error'))
  }
}

// Development tests enter through the same origin/session/epoch/sequence guard
// as real browser events; production code never calls this exposed method.
function receiveTestMessage(data: unknown) {
  onMessage({ origin: location.origin, source: frame.value?.contentWindow ?? null, data } as MessageEvent<unknown>)
}

// 仅在 iframe 加载完成 / 切换组件时发送 INIT。普通状态修改一律走 LAB_PATCH，
// 由 UiLabPage 显式下发 —— 不 deep-watch 整个 session，避免 INIT/SNAPSHOT 循环。
watch(() => props.session.component.id, () => {
  beginFrameEpoch()
  sendInit()
})

onMounted(() => {
  window.addEventListener('message', onMessage)
  canvasObserver = new ResizeObserver(([entry]) => {
    canvasSize.value = { width: entry.contentRect.width, height: entry.contentRect.height }
  })
  if (canvas.value) canvasObserver.observe(canvas.value)
})

onBeforeUnmount(() => {
  window.removeEventListener('message', onMessage)
  canvasObserver?.disconnect()
})

defineExpose({
  send,
  sendInit,
  getFrameEpoch: () => frameEpoch.value,
  isReady: () => isReady.value,
  receiveTestMessage,
  requestSnapshot: () => send('LAB_REQUEST_SNAPSHOT', {}),
})
</script>

<template>
  <section class="sandbox-viewport" aria-label="Component sandbox viewport">
    <div class="viewport-ruler">
      <span>{{ viewport.width }} × {{ viewport.height }}</span>
      <span>{{ Math.round(environment.zoom * 100) }}%</span>
    </div>
    <div ref="canvas" class="viewport-canvas">
      <div class="viewport-frame" :style="viewportStyle">
        <iframe ref="frame" sandbox="allow-scripts allow-same-origin" src="/ui-lab/sandbox" title="TALOS UI Lab component sandbox" @load="onLoad" />
      </div>
    </div>
  </section>
</template>

<style scoped>
.sandbox-viewport { display: grid; grid-template-rows: 32px minmax(0, 1fr); min-width: 0; min-height: 0; background: var(--bg-canvas); border: 1px solid var(--border-strong); }
.viewport-ruler { display: flex; align-items: center; justify-content: space-between; padding: 0 10px; border-bottom: 1px solid var(--border-base); color: var(--text-tertiary); font-family: var(--font-mono); font-size: var(--font-size-caption); }
.viewport-canvas { position: relative; display: grid; place-items: center; min-height: 0; overflow: hidden; background-image: linear-gradient(var(--border-base) 1px, transparent 1px), linear-gradient(90deg, var(--border-base) 1px, transparent 1px); background-size: 24px 24px; }
.viewport-frame { flex: 0 0 auto; overflow: hidden; box-shadow: 0 0 0 1px var(--border-strong); }
iframe { display: block; width: calc(100% / var(--sandbox-scale)); height: calc(100% / var(--sandbox-scale)); border: 0; background: var(--bg-surface); transform: scale(var(--sandbox-scale)); transform-origin: top left; }
</style>
