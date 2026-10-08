// ── UI Lab Host ↔ Sandbox 协议 ──────────────────────────────────────────
//
// 安全边界说明：
//   UI Lab Sandbox 是**可信组件**的渲染隔离层，不是不可信代码执行环境。
//   它通过同源 iframe + sandbox 属性提供样式/脚本隔离，但不对不可信代码
//   提供安全保证。消息校验用于防御编程错误与同源注入，不代表沙箱边界。
//
// 消息职责：
//   LAB_INIT            仅用于 iframe 首次 ready / reload / 切换组件 /
//                       显式恢复完整 Snapshot。payload 为完整 LabSessionState。
//   LAB_PATCH           普通状态修改（Inspector / scene / environment /
//                       motion 配置）。payload 含 revision / path / value / source。
//   LAB_COMMAND         非持久化命令（motion.play / pause / restart / seek、
//                       snapshot.capture、interaction.reset）。
//   LAB_REQUEST_SNAPSHOT Host 显式请求 Snapshot。
//   SANDBOX_READY       Sandbox 初始化完成。
//   SANDBOX_PATCH       Sandbox 内部发生 Host 不知道的有效状态变化。
//   SANDBOX_SNAPSHOT    仅由显式请求或初始化握手确认后发送，不随每个
//                       patch 无条件回发（防止 INIT/SNAPSHOT 循环）。
//   SANDBOX_EVENT       非持久化事件（interaction、motion state、diagnostic）。
//   SANDBOX_ERROR       渲染 / 执行错误。

import type { UiComponentDefinition } from '@/ui'
import { assertSafeLabPath, validateLabSession, type LabSessionState, type UiLabPatchSource } from '../state/lab-session'
import { validateLabPathValue } from '../state/lab-value'

export const UI_LAB_PROTOCOL_VERSION = '1.0' as const

export type LabProtocolType =
  | 'LAB_INIT'
  | 'LAB_PATCH'
  | 'LAB_COMMAND'
  | 'LAB_REQUEST_SNAPSHOT'
  | 'SANDBOX_READY'
  | 'SANDBOX_PATCH'
  | 'SANDBOX_SNAPSHOT'
  | 'SANDBOX_EVENT'
  | 'SANDBOX_ERROR'

export type LabCommandName =
  | 'motion.play'
  | 'motion.pause'
  | 'motion.restart'
  | 'motion.seek'
  | 'snapshot.capture'
  | 'interaction.reset'

export interface LabProtocolEnvelope<TType extends LabProtocolType = LabProtocolType> {
  protocolVersion: typeof UI_LAB_PROTOCOL_VERSION
  sessionId: string
  sequence: number
  frameEpoch: string
  revision: number
  type: TType
  componentId: string | null
  payload: unknown
}

export interface LabInitPayload {
  session: LabSessionState
}

export interface LabPatchPayload {
  revision: number
  baseRevision: number
  path: string
  value: unknown
  source: UiLabPatchSource
}

export interface LabCommandPayload {
  command: LabCommandName
  [key: string]: unknown
}

export interface SandboxReadyPayload {
  protocol: string
}

export interface SandboxPatchPayload {
  revision: number
  baseRevision: number
  path: string
  value: unknown
}

export interface SandboxSnapshotPayload {
  revision: number
  session: LabSessionState
}

export interface SandboxEventPayload {
  type: string
  [key: string]: unknown
}

export interface SandboxErrorPayload {
  message: string
  [key: string]: unknown
}

export function createLabMessage<TType extends LabProtocolType>(
  sessionId: string,
  sequence: number,
  revision: number,
  type: TType,
  componentId: string | null,
  payload: unknown,
  frameEpoch = 'legacy',
): LabProtocolEnvelope<TType> {
  const normalizedPayload = payload && typeof payload === 'object' ? { ...(payload as Record<string, unknown>) } : payload
  let effectiveRevision = revision
  if (normalizedPayload && typeof normalizedPayload === 'object') {
    const candidate = normalizedPayload as Record<string, unknown>
    if ((type === 'LAB_PATCH' || type === 'SANDBOX_PATCH' || type === 'SANDBOX_SNAPSHOT') && typeof candidate.revision === 'number') {
      effectiveRevision = candidate.revision
      if ((type === 'LAB_PATCH' || type === 'SANDBOX_PATCH') && typeof candidate.baseRevision !== 'number') {
        candidate.baseRevision = Math.max(0, candidate.revision - 1)
      }
    }
  }
  return { protocolVersion: UI_LAB_PROTOCOL_VERSION, sessionId, sequence, frameEpoch, revision: effectiveRevision, type, componentId, payload: normalizedPayload }
}

const KNOWN_TYPES = new Set<LabProtocolType>([
  'LAB_INIT', 'LAB_PATCH', 'LAB_COMMAND', 'LAB_REQUEST_SNAPSHOT',
  'SANDBOX_READY', 'SANDBOX_PATCH', 'SANDBOX_SNAPSHOT', 'SANDBOX_EVENT', 'SANDBOX_ERROR',
])

const KNOWN_COMMANDS = new Set<LabCommandName>([
  'motion.play', 'motion.pause', 'motion.restart', 'motion.seek',
  'snapshot.capture', 'interaction.reset',
])

export function isFiniteNonNegativeInteger(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 && Number.isInteger(value)
}

export function isFinitePositiveInteger(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 && Number.isInteger(value)
}

export function isLabProtocolEnvelope(value: unknown): value is LabProtocolEnvelope {
  if (!value || typeof value !== 'object') return false
  const candidate = value as Partial<LabProtocolEnvelope>
  return candidate.protocolVersion === UI_LAB_PROTOCOL_VERSION
    && typeof candidate.sessionId === 'string'
    && isFinitePositiveInteger(candidate.sequence)
    && isFiniteNonNegativeInteger(candidate.revision)
    && typeof candidate.frameEpoch === 'string'
    && candidate.frameEpoch.length > 0
    && typeof candidate.type === 'string'
    && KNOWN_TYPES.has(candidate.type as LabProtocolType)
    && (typeof candidate.componentId === 'string' || candidate.componentId === null)
    && 'payload' in candidate
}

export interface ValidationResult {
  ok: boolean
  reason?: string
}

export interface LabMessageValidationContext {
  sessionId?: string
  frameEpoch?: string
  componentId?: string | null
  allowedPaths?: ReadonlySet<string>
  revision?: number
  /** 提供后启用 Registry value contract 严格校验（枚举/类型/范围）。 */
  component?: UiComponentDefinition
}

export function validateLabMessage(message: LabProtocolEnvelope, context: LabMessageValidationContext = {}): ValidationResult {
  if (!isLabProtocolEnvelope(message)) return { ok: false, reason: 'not a lab protocol envelope' }
  if (context.sessionId !== undefined && message.sessionId !== context.sessionId) return { ok: false, reason: 'session context mismatch' }
  if (context.frameEpoch !== undefined && message.frameEpoch !== context.frameEpoch) return { ok: false, reason: 'frame epoch mismatch' }
  if (context.componentId !== undefined && message.componentId !== context.componentId) return { ok: false, reason: 'component context mismatch' }
  if (context.revision !== undefined && message.revision < context.revision) return { ok: false, reason: 'stale revision' }
  const payload = message.payload as unknown
  switch (message.type) {
    case 'LAB_INIT': {
      const session = (payload as LabInitPayload | undefined)?.session
      if (!session || typeof session !== 'object') return { ok: false, reason: 'LAB_INIT.payload.session missing' }
      const result = validateLabSession(session, { componentId: message.componentId ?? undefined, allowedPaths: context.allowedPaths })
      return result.ok ? { ok: true } : { ok: false, reason: `LAB_INIT ${result.reason}` }
    }
    case 'LAB_PATCH': {
      const patch = payload as LabPatchPayload | undefined
      if (!patch || !isFiniteNonNegativeInteger(patch.revision) || !isFiniteNonNegativeInteger(patch.baseRevision)) return { ok: false, reason: 'LAB_PATCH revision context invalid' }
      if (patch.revision !== message.revision) return { ok: false, reason: 'LAB_PATCH payload/envelope revision mismatch' }
      if (typeof patch.path !== 'string' || !patch.path) return { ok: false, reason: 'LAB_PATCH.path missing' }
      try { assertSafeLabPath(patch.path) } catch (error) { return { ok: false, reason: error instanceof Error ? error.message : String(error) } }
      if (context.allowedPaths && !context.allowedPaths.has(patch.path)) return { ok: false, reason: `LAB_PATCH path not declared: ${patch.path}` }
      if (patch.source !== 'inspector' && patch.source !== 'scene' && patch.source !== 'environment'
        && patch.source !== 'timeline' && patch.source !== 'sandbox' && patch.source !== 'system') {
        return { ok: false, reason: `LAB_PATCH.source invalid: ${String(patch.source)}` }
      }
      if (context.component) {
        const valueReason = validateLabPathValue(context.component, patch.path, patch.value)
        if (valueReason) return { ok: false, reason: `LAB_PATCH ${valueReason}` }
      }
      return { ok: true }
    }
    case 'LAB_COMMAND': {
      const command = (payload as LabCommandPayload | undefined)?.command
      if (typeof command !== 'string' || !KNOWN_COMMANDS.has(command as LabCommandName)) {
        return { ok: false, reason: `LAB_COMMAND unknown: ${String(command)}` }
      }
      if (command === 'motion.seek') {
        const progress = (payload as Record<string, unknown>).progress
        if (typeof progress !== 'number' || !Number.isFinite(progress) || progress < 0 || progress > 1) {
          return { ok: false, reason: 'LAB_COMMAND motion.seek.progress invalid' }
        }
      }
      return { ok: true }
    }
    case 'LAB_REQUEST_SNAPSHOT':
      return { ok: true }
    case 'SANDBOX_READY':
      return { ok: true }
    case 'SANDBOX_PATCH': {
      const patch = payload as SandboxPatchPayload | undefined
      if (!patch || !isFiniteNonNegativeInteger(patch.revision) || !isFiniteNonNegativeInteger(patch.baseRevision)) return { ok: false, reason: 'SANDBOX_PATCH revision context invalid' }
      if (patch.revision !== message.revision) return { ok: false, reason: 'SANDBOX_PATCH payload/envelope revision mismatch' }
      if (typeof patch.path !== 'string' || !patch.path) return { ok: false, reason: 'SANDBOX_PATCH.path missing' }
      try { assertSafeLabPath(patch.path) } catch (error) { return { ok: false, reason: error instanceof Error ? error.message : String(error) } }
      if (context.allowedPaths && !context.allowedPaths.has(patch.path)) return { ok: false, reason: `SANDBOX_PATCH path not declared: ${patch.path}` }
      if (context.component) {
        const valueReason = validateLabPathValue(context.component, patch.path, patch.value)
        if (valueReason) return { ok: false, reason: `SANDBOX_PATCH ${valueReason}` }
      }
      return { ok: true }
    }
    case 'SANDBOX_SNAPSHOT': {
      const snapshot = payload as SandboxSnapshotPayload | undefined
      if (!snapshot || !isFiniteNonNegativeInteger(snapshot.revision)) return { ok: false, reason: 'SANDBOX_SNAPSHOT.revision invalid' }
      if (snapshot.revision !== message.revision) return { ok: false, reason: 'SANDBOX_SNAPSHOT payload/envelope revision mismatch' }
      if (!snapshot.session || typeof snapshot.session !== 'object') return { ok: false, reason: 'SANDBOX_SNAPSHOT.session missing' }
      const result = validateLabSession(snapshot.session, {
        componentId: message.componentId ?? undefined,
        allowedPaths: context.allowedPaths,
        expectedRevision: message.frameEpoch === 'legacy' ? undefined : snapshot.revision,
      })
      return result.ok ? { ok: true } : { ok: false, reason: `SANDBOX_SNAPSHOT ${result.reason}` }
    }
    case 'SANDBOX_EVENT': {
      if (typeof (payload as SandboxEventPayload | undefined)?.type !== 'string') {
        return { ok: false, reason: 'SANDBOX_EVENT.type missing' }
      }
      return { ok: true }
    }
    case 'SANDBOX_ERROR': {
      if (typeof (payload as SandboxErrorPayload | undefined)?.message !== 'string') {
        return { ok: false, reason: 'SANDBOX_ERROR.message missing' }
      }
      return { ok: true }
    }
    default:
      return { ok: false, reason: `unknown type: ${message.type}` }
  }
}
