// ── UI Lab 权威会话状态契约 ─────────────────────────────────────────────
// Host、Sandbox、Snapshot、Diff、Journal 共用同一状态树与同一路径语义。
// Registry 以 (scope, path) 声明控件（如 props.variant / scene.maskOpacity），
// 本模块将其归一化为完整 lab path（component.props.variant / scene.maskOpacity），
// 并唯一实现 read/write 语义。禁止 Host 与 Sandbox 各自维护路径解释规则。

// type-only 依赖 Registry 定义，不引入运行时耦合。
// eslint-disable-next-line import/no-unresolved
import type { UiComponentDefinition } from '@/ui'

export type UiLabTheme = 'dark' | 'light'
export type UiLabLocale = 'zh-CN' | 'en'
export type UiLabDirection = 'ltr' | 'rtl'
export type UiLabEngine = 'auto' | 'anime' | 'gsap' | 'native'
export type UiLabMotionStatus = 'idle' | 'playing' | 'paused' | 'finished'
export type UiLabPatchSource = 'inspector' | 'scene' | 'environment' | 'timeline' | 'sandbox' | 'system'

export interface LabViewport {
  id: string
  width: number
  height: number
}

export interface LabEnvironment {
  theme: UiLabTheme
  locale: UiLabLocale
  direction: UiLabDirection
  viewport: LabViewport
  zoom: number
  reducedMotion: boolean
}

export interface LabSlotState {
  enabled: boolean
  content: string | null
}

export interface LabMotionState {
  activeMotionId: string | null
  engine: UiLabEngine
  playbackRate: number
  loop: boolean
  currentTime: number
  status: UiLabMotionStatus
}

export interface LabSceneState {
  id: string
  [param: string]: unknown
}

export interface LabSessionState {
  revision: number
  component: {
    id: string
    props: Record<string, unknown>
    slots: Record<string, LabSlotState>
  }
  scene: LabSceneState
  environment: LabEnvironment
  motion: LabMotionState
}

export const LAB_SCHEMA_VERSION = 'talos.ui-lab.session/1.1'

export const LAB_DEFAULT_VIEWPORTS: Record<string, LabViewport> = {
  'desktop-1600': { id: 'desktop-1600', width: 1600, height: 900 },
  'laptop-1440': { id: 'laptop-1440', width: 1440, height: 900 },
  'tablet-1024': { id: 'tablet-1024', width: 1024, height: 1366 },
  'mobile-390': { id: 'mobile-390', width: 390, height: 844 },
}

export interface LabSessionValidationResult {
  ok: boolean
  reason?: string
}

export interface LabSessionValidationOptions {
  componentId?: string
  allowedPaths?: ReadonlySet<string>
  expectedRevision?: number
  /** 提供后启用 Registry value contract 严格校验（枚举/motion/scene/viewport）。 */
  component?: UiComponentDefinition
}

function isFiniteNonNegativeInteger(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && Number.isInteger(value) && value >= 0
}

function isFinitePositiveInteger(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && Number.isInteger(value) && value > 0
}

const DANGEROUS_KEYS: ReadonlySet<string> = new Set(['__proto__', 'prototype', 'constructor'])

export function isDangerousLabKey(segment: string): boolean {
  return DANGEROUS_KEYS.has(segment)
}

export function assertSafeLabPath(path: string): void {
  const segments = path.split('.').filter(Boolean)
  for (const segment of segments) {
    if (DANGEROUS_KEYS.has(segment)) {
      throw new Error(`Forbidden lab path segment: "${segment}" in "${path}"`)
    }
  }
}

/** Registry (scope, path) → 完整 lab path。 */
export function labPathForControl(scope: string, path: string): string {
  if (scope === 'component') {
    if (path.startsWith('component.')) return path
    return `component.${path}`
  }
  if (scope === 'scene') {
    if (path.startsWith('scene.')) return path
    return `scene.${path}`
  }
  if (scope === 'motion') {
    if (path.startsWith('motion.')) return path
    return `motion.${path}`
  }
  if (scope === 'environment') {
    if (path.startsWith('environment.')) return path
    return `environment.${path}`
  }
  throw new Error(`Unknown lab control scope: ${scope}`)
}

/** Registry control 声明的全部可写路径（去重）。 */
export function collectLabControlPaths(controls: Array<{ scope: string; path: string }>): ReadonlySet<string> {
  return new Set(controls.map((control) => labPathForControl(control.scope, control.path)))
}

/** designSlots 声明的可写路径。 */
export function collectLabSlotPaths(slots: Array<{ id: string }>): ReadonlySet<string> {
  const paths = new Set<string>()
  for (const slot of slots) {
    paths.add(`component.slots.${slot.id}.enabled`)
    paths.add(`component.slots.${slot.id}.content`)
  }
  return paths
}

export function createInitialLabSession(componentId: string, viewport: LabViewport): LabSessionState {
  return {
    revision: 0,
    component: { id: componentId, props: {}, slots: {} },
    scene: { id: '' },
    environment: {
      theme: 'dark',
      locale: 'zh-CN',
      direction: 'ltr',
      viewport,
      zoom: 1,
      reducedMotion: false,
    },
    motion: {
      activeMotionId: null,
      engine: 'auto',
      playbackRate: 1,
      loop: false,
      currentTime: 0,
      status: 'idle',
    },
  }
}

export function cloneLabSession(state: LabSessionState): LabSessionState {
  return JSON.parse(JSON.stringify(state)) as LabSessionState
}

export function sameLabValue(left: unknown, right: unknown): boolean {
  return JSON.stringify(left) === JSON.stringify(right)
}

export function readLabPath(state: LabSessionState, path: string): unknown {
  assertSafeLabPath(path)
  const segments = path.split('.').filter(Boolean)
  if (segments.length === 0) return undefined
  let cursor: unknown = state
  for (const segment of segments) {
    if (cursor && typeof cursor === 'object') {
      cursor = (cursor as Record<string, unknown>)[segment]
    } else {
      return undefined
    }
  }
  return cursor
}

export interface WriteLabPathOptions {
  /** 若提供，仅允许写这些路径（Registry 声明范围内）。 */
  allowedPaths?: ReadonlySet<string>
}

export function writeLabPath(
  state: LabSessionState,
  path: string,
  value: unknown,
  options: WriteLabPathOptions = {},
): void {
  assertSafeLabPath(path)
  if (options.allowedPaths && !options.allowedPaths.has(path)) {
    throw new Error(`Path is not declared in the Registry: "${path}"`)
  }
  const segments = path.split('.').filter(Boolean)
  if (segments.length === 0) throw new Error('Empty lab path')

  const root = state as unknown as Record<string, unknown>
  let cursor = root
  for (const segment of segments.slice(0, -1)) {
    const next = cursor[segment]
    if (next && typeof next === 'object') {
      cursor = next as Record<string, unknown>
    } else if (next === undefined || next === null) {
      const created: Record<string, unknown> = {}
      cursor[segment] = created
      cursor = created
    } else {
      throw new Error(`Cannot write through a non-object at "${path}"`)
    }
  }
  cursor[segments[segments.length - 1]] = value
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value)
}

function pathIsDeclared(path: string, allowedPaths?: ReadonlySet<string>): boolean {
  if (!allowedPaths) return true
  for (const declared of allowedPaths) {
    if (path === declared || path.startsWith(`${declared}.`) || declared.startsWith(`${path}.`)) return true
  }
  return false
}

function validateValue(value: unknown, path: string, allowedPaths: ReadonlySet<string> | undefined, seen: WeakSet<object>): string | undefined {
  if (typeof value === 'number' && !Number.isFinite(value)) return `${path} must be finite`
  if (typeof value === 'function' || typeof value === 'symbol' || typeof value === 'bigint') return `${path} has an unsupported value`
  if (value === null || typeof value !== 'object') return undefined
  if (seen.has(value)) return `${path} contains a cycle`
  seen.add(value)
  if (Array.isArray(value)) {
    for (let index = 0; index < value.length; index += 1) {
      const reason = validateValue(value[index], `${path}.${index}`, allowedPaths, seen)
      if (reason) return reason
    }
    return undefined
  }
  for (const [key, child] of Object.entries(value)) {
    if (isDangerousLabKey(key)) return `${path}.${key} is forbidden`
    const childPath = `${path}.${key}`
    const labPath = childPath.startsWith('session.') ? childPath.slice('session.'.length) : childPath
    const structuralPaths = new Set([
      'revision', 'component', 'component.id', 'component.props', 'component.slots', 'scene', 'scene.id',
      'environment', 'environment.theme', 'environment.locale', 'environment.direction', 'environment.viewport',
      'environment.zoom', 'environment.reducedMotion', 'motion', 'motion.activeMotionId', 'motion.engine',
      'motion.playbackRate', 'motion.loop', 'motion.currentTime', 'motion.status',
    ])
    const structural = structuralPaths.has(labPath)
    const declared = pathIsDeclared(labPath, allowedPaths)
    if (!structural && !declared) return `${labPath} is not declared in the Registry`
    if ((labPath.startsWith('component.props.') || labPath.startsWith('component.slots.') || labPath.startsWith('scene.')) && !declared) {
      return `${labPath} is not declared in the Registry`
    }
    const reason = validateValue(child, childPath, allowedPaths, seen)
    if (reason) return reason
  }
  return undefined
}

export function validateLabSession(
  value: unknown,
  options: LabSessionValidationOptions = {},
): LabSessionValidationResult {
  if (!isRecord(value)) return { ok: false, reason: 'session must be an object' }
  const session = value as Partial<LabSessionState>
  if (!isFiniteNonNegativeInteger(session.revision)) return { ok: false, reason: 'session.revision invalid' }
  if (options.expectedRevision !== undefined && session.revision !== options.expectedRevision) {
    return { ok: false, reason: `session.revision must equal ${options.expectedRevision}` }
  }
  if (options.componentId !== undefined && session.component?.id !== options.componentId) {
    return { ok: false, reason: 'session.component.id does not match the active component' }
  }
  if (!isRecord(session.component) || typeof session.component.id !== 'string' || !isRecord(session.component.props) || !isRecord(session.component.slots)) {
    return { ok: false, reason: 'session.component shape invalid' }
  }
  if (!isRecord(session.scene) || typeof session.scene.id !== 'string') return { ok: false, reason: 'session.scene shape invalid' }
  if (options.component && session.scene.id !== '') {
    const supportedScenes = options.component.lab?.preview?.supportedScenes ?? []
    if (!supportedScenes.includes(session.scene.id)) {
      return { ok: false, reason: `session.scene.id "${session.scene.id}" is not supported` }
    }
  }
  if (!isRecord(session.environment) || !isRecord(session.environment.viewport)) return { ok: false, reason: 'session.environment shape invalid' }
  if (!isRecord(session.motion)) return { ok: false, reason: 'session.motion shape invalid' }
  const environment = session.environment as LabEnvironment
  const viewport = environment.viewport
  if (typeof viewport.id !== 'string' || !Object.prototype.hasOwnProperty.call(LAB_DEFAULT_VIEWPORTS, viewport.id)
    || !isFinitePositiveInteger(viewport.width) || !isFinitePositiveInteger(viewport.height)
    || viewport.width > 4096 || viewport.height > 4096) return { ok: false, reason: 'session.environment.viewport invalid' }
  const standardViewport = LAB_DEFAULT_VIEWPORTS[viewport.id]
  if (standardViewport && (viewport.width !== standardViewport.width || viewport.height !== standardViewport.height)) {
    return { ok: false, reason: `session.environment.viewport ${viewport.id} must be ${standardViewport.width}×${standardViewport.height}` }
  }
  if (environment.theme !== 'dark' && environment.theme !== 'light') return { ok: false, reason: 'session.environment.theme invalid' }
  if (environment.locale !== 'zh-CN' && environment.locale !== 'en') return { ok: false, reason: 'session.environment.locale invalid' }
  if (environment.direction !== 'ltr' && environment.direction !== 'rtl') return { ok: false, reason: 'session.environment.direction invalid' }
  if (typeof environment.zoom !== 'number' || !Number.isFinite(environment.zoom) || environment.zoom < 0.25 || environment.zoom > 4) return { ok: false, reason: 'session.environment.zoom invalid' }
  if (typeof environment.reducedMotion !== 'boolean') return { ok: false, reason: 'session.environment.reducedMotion invalid' }
  const motion = session.motion as LabMotionState
  if (motion.activeMotionId !== null) {
    if (typeof motion.activeMotionId !== 'string') return { ok: false, reason: 'session.motion.activeMotionId invalid' }
    if (options.component && !(options.component.lab?.motions?.some((item) => item.id === motion.activeMotionId))) {
      return { ok: false, reason: `session.motion.activeMotionId "${motion.activeMotionId}" is not declared` }
    }
  }
  if (!['auto', 'anime', 'gsap', 'native'].includes(motion.engine)) return { ok: false, reason: 'session.motion.engine invalid' }
  if (typeof motion.playbackRate !== 'number' || !Number.isFinite(motion.playbackRate) || motion.playbackRate < 0.1 || motion.playbackRate > 4) return { ok: false, reason: 'session.motion.playbackRate invalid' }
  if (typeof motion.loop !== 'boolean' || typeof motion.currentTime !== 'number' || !Number.isFinite(motion.currentTime) || motion.currentTime < 0 || motion.currentTime > 1) return { ok: false, reason: 'session.motion values invalid' }
  if (!['idle', 'playing', 'paused', 'finished'].includes(motion.status)) return { ok: false, reason: 'session.motion.status invalid' }
  if (options.component) {
    for (const [propKey, propValue] of Object.entries(session.component.props)) {
      const propReason = validateLabPathValue(options.component, `component.props.${propKey}`, propValue)
      if (propReason) return { ok: false, reason: propReason }
    }
  }
  for (const [slotId, slot] of Object.entries(session.component.slots)) {
    const slotDeclared = options.component
      ? options.component.lab?.designSlots?.some((item) => item.id === slotId)
      : undefined
    if (slotDeclared === false) return { ok: false, reason: `session.component.slots.${slotId} is not declared` }
    if (!pathIsDeclared(`component.slots.${slotId}`, options.allowedPaths) || !isRecord(slot)
      || typeof slot.enabled !== 'boolean' || (slot.content !== null && typeof slot.content !== 'string')) {
      return { ok: false, reason: `session.component.slots.${slotId} invalid` }
    }
  }
  const rootReason = validateValue(value, 'session', options.allowedPaths, new WeakSet<object>())
  return rootReason ? { ok: false, reason: rootReason } : { ok: true }
}

// ── 统一 Registry value contract ──────────────────────────────────────────
// Host 写入、Sandbox 应用、协议校验、Snapshot Restore 全部复用
// validateLabPathValue —— 任何已注册 path 的 value 必须满足控件声明或内建
// 路径的枚举/类型/范围约束。禁止在不同文件复制多套不一致的 value 判断。

function unsupportedLabValueType(path: string, value: unknown): string | undefined {
  if (typeof value === 'function' || typeof value === 'symbol' || typeof value === 'bigint') {
    return `${path}: value has an unsupported type (function/symbol/bigint)`
  }
  if (typeof value === 'number' && !Number.isFinite(value)) {
    return `${path}: value must be finite`
  }
  if (value && typeof value === 'object') {
    const seen = new WeakSet<object>()
    try {
      JSON.stringify(value, (_key, child) => {
        if (child && typeof child === 'object') {
          if (seen.has(child)) throw new Error('cycle')
          seen.add(child)
        }
        return child
      })
    } catch {
      return `${path}: value is not serializable (cycle)`
    }
  }
  return undefined
}

function validateSelectValue(path: string, value: unknown, options: Array<{ label: string; value: string | number | boolean }>): string | undefined {
  if (!options.some((option) => option.value === value)) {
    return `${path}: value ${JSON.stringify(value)} is not in the declared options`
  }
  return undefined
}

function validateNumberValue(path: string, value: unknown, bounds: { min?: number; max?: number; step?: number }): string | undefined {
  if (typeof value !== 'number' || !Number.isFinite(value)) return `${path}: must be a finite number`
  if (bounds.min !== undefined && value < bounds.min) return `${path}: below min ${bounds.min}`
  if (bounds.max !== undefined && value > bounds.max) return `${path}: above max ${bounds.max}`
  if (bounds.step !== undefined) {
    if (!Number.isFinite(bounds.step) || bounds.step <= 0) return `${path}: declared step must be a positive finite number`
    const origin = bounds.min ?? 0
    const units = (value - origin) / bounds.step
    if (Math.abs(units - Math.round(units)) > 1e-9) return `${path}: must align to step ${bounds.step}`
  }
  return undefined
}

function validateViewportValue(path: string, value: unknown): string | undefined {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return `${path}: must be an object`
  const viewport = value as Record<string, unknown>
  if (typeof viewport.id !== 'string') return `${path}: viewport.id must be a string`
  const standard = LAB_DEFAULT_VIEWPORTS[viewport.id]
  if (!standard) return `${path}: viewport id "${String(viewport.id)}" is unknown`
  if (viewport.width !== standard.width || viewport.height !== standard.height) {
    return `${path}: viewport ${String(viewport.id)} must be ${standard.width}×${standard.height}, got ${String(viewport.width)}×${String(viewport.height)}`
  }
  return undefined
}

const LAB_THEMES = new Set(['dark', 'light'])
const LAB_LOCALES = new Set(['zh-CN', 'en'])
const LAB_DIRECTIONS = new Set(['ltr', 'rtl'])
const LAB_ENGINES = new Set(['auto', 'anime', 'gsap', 'native'])
const LAB_MOTION_STATUSES = new Set(['idle', 'playing', 'paused', 'finished'])

/**
 * 校验单个 lab path 的写入值。返回 reason 字符串；通过返回 undefined。
 */
export function validateLabPathValue(
  component: UiComponentDefinition,
  path: string,
  value: unknown,
): string | undefined {
  try {
    assertSafeLabPath(path)
  } catch (error) {
    return error instanceof Error ? error.message : String(error)
  }
  if (path.split('.').some(isDangerousLabKey)) return `${path}: forbidden path segment`

  const typeReason = unsupportedLabValueType(path, value)
  if (typeReason) return typeReason

  // 1) Registry 声明的控件路径
  const control = component.lab?.controls?.find((item) => labPathForControl(item.scope, item.path) === path)
  if (control) {
    switch (control.type) {
      case 'select':
        return validateSelectValue(path, value, control.options ?? [])
      case 'toggle':
        return typeof value === 'boolean' ? undefined : `${path}: must be boolean`
      case 'number':
      case 'range':
      case 'duration':
        return validateNumberValue(path, value, { min: control.min, max: control.max, step: control.step })
      case 'text':
      case 'textarea':
      case 'color':
      case 'easing':
        return typeof value === 'string' ? undefined : `${path}: must be a string`
      default:
        return undefined
    }
  }

  // 2) 内建环境 / motion / scene / slot 路径
  switch (path) {
    case 'component.id':
      return typeof value === 'string' && value.length > 0 ? undefined : `${path}: must be a non-empty string`
    case 'environment.theme':
      return LAB_THEMES.has(value as string) ? undefined : `${path}: must be dark or light`
    case 'environment.locale':
      return LAB_LOCALES.has(value as string) ? undefined : `${path}: must be zh-CN or en`
    case 'environment.direction':
      return LAB_DIRECTIONS.has(value as string) ? undefined : `${path}: must be ltr or rtl`
    case 'environment.viewport':
      return validateViewportValue(path, value)
    case 'environment.zoom':
      return validateNumberValue(path, value, { min: 0.25, max: 4 })
    case 'environment.reducedMotion':
      return typeof value === 'boolean' ? undefined : `${path}: must be boolean`
    case 'motion.activeMotionId': {
      if (value === null) return undefined
      if (typeof value !== 'string') return `${path}: must be a string or null`
      const declared = component.lab?.motions?.some((motion) => motion.id === value)
      return declared ? undefined : `${path}: motion "${value}" is not declared for this component`
    }
    case 'motion.engine':
      return LAB_ENGINES.has(value as string) ? undefined : `${path}: engine must be auto, anime, gsap or native`
    case 'motion.playbackRate':
      return validateNumberValue(path, value, { min: 0.1, max: 4 })
    case 'motion.loop':
      return typeof value === 'boolean' ? undefined : `${path}: must be boolean`
    case 'motion.currentTime':
      return validateNumberValue(path, value, { min: 0, max: 1 })
    case 'motion.status':
      return LAB_MOTION_STATUSES.has(value as string) ? undefined : `${path}: status must be idle, playing, paused or finished`
    case 'scene.id': {
      if (value === '') return undefined
      if (typeof value !== 'string') return `${path}: must be a string`
      const supported = component.lab?.preview?.supportedScenes ?? []
      return supported.includes(value) ? undefined : `${path}: scene "${value}" is not supported`
    }
    default: {
      const slotMatch = /^component\.slots\.([^.]+)\.(enabled|content)$/.exec(path)
      if (slotMatch) {
        const slotId = slotMatch[1]
        const field = slotMatch[2]
        const declared = component.lab?.designSlots?.some((slot) => slot.id === slotId)
        if (!declared) return `${path}: slot "${slotId}" is not declared`
        if (field === 'enabled') return typeof value === 'boolean' ? undefined : `${path}: enabled must be boolean`
        return value === null || typeof value === 'string' ? undefined : `${path}: content must be a string or null`
      }
      if (path.startsWith('component.props.')) return `${path}: not declared in the Registry`
      return undefined
    }
  }
}
