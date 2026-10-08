import registryData from './component-registry.json'

export type UiComponentCategory =
  | 'primitive'
  | 'control'
  | 'input'
  | 'navigation'
  | 'feedback'
  | 'data-display'
  | 'layout'
  | 'pattern'
  | 'shell'
  | 'surface-adapter'

export type UiComponentStatus = 'draft' | 'experimental' | 'active' | 'deprecated' | 'removed'
export type UiConformanceLevel = 'L0' | 'L1' | 'L2' | 'L3'
export type UiLabLocale = 'zh-CN' | 'en'
export type UiLabDirection = 'ltr' | 'rtl'
export type UiLabSlotKind = 'content' | 'signal' | 'icon' | 'meta' | 'decoration' | 'focus'
export type UiLabSlotPosition = 'before' | 'after' | 'overlay' | 'underlay'
// Registry 可验证的 motion target：specimen（组件根）、probe（data-lab-probe
// 节点）、slot:<id>（data-lab-slot）、selector:<安全选择器白名单>。不允许任意
// 未经验证的选择器注入。
export type UiLabMotionTarget =
  | 'specimen'
  | 'probe'
  | `slot:${string}`
  | `selector:${string}`
export type UiLabMotionTrigger = 'mount' | 'hover' | 'press' | 'state-change' | 'manual'
export type UiLabMotionEngine = 'anime' | 'gsap' | 'native'
export type UiLabFutureMotionEngine = 'three'
export type UiLabControlType = 'select' | 'toggle' | 'number' | 'range' | 'text' | 'textarea' | 'color' | 'duration' | 'easing' | 'vector2'
export type UiLabControlScope = 'component' | 'scene' | 'motion' | 'environment'

export interface UiLabControlDefinition {
  id: string
  scope: UiLabControlScope
  path: string
  type: UiLabControlType
  labelKey: string
  default: unknown
  semanticEffect: string
  options?: Array<{ label: string; value: string | number | boolean }>
  min?: number
  max?: number
  step?: number
}

export interface UiLabPreviewDefinition {
  mode: 'sandbox-scene'
  defaultScene: string
  supportedScenes: string[]
  fillsViewport?: boolean
}

export interface UiLabAgentExportDefinition {
  implementationTargets: string[]
  preserve: string[]
}

export interface UiLabDesignSlotDefinition {
  id: string
  title: string
  kind: UiLabSlotKind
  position: UiLabSlotPosition
  required: boolean
  enabledByDefault: boolean
}

export interface UiLabMotionKeyframeDefinition {
  offset: number
  label: string
  style: Record<string, string | number>
}

export interface UiLabMotionEasingDefinition {
  anime: string
  gsap: string
  native: string
}

export interface UiLabMotionDefinition {
  id: string
  title: string
  available: boolean
  target: UiLabMotionTarget
  trigger: UiLabMotionTrigger
  durationMs: number
  delayMs: number
  easing: UiLabMotionEasingDefinition
  iterations: number
  tracks: string[]
  keyframes: UiLabMotionKeyframeDefinition[]
}

export interface UiLabContract {
  locales: UiLabLocale[]
  directions: UiLabDirection[]
  motionEngines: UiLabMotionEngine[]
  futureMotionEngines?: UiLabFutureMotionEngine[]
  designSlots: UiLabDesignSlotDefinition[]
  motions: UiLabMotionDefinition[]
  preview?: UiLabPreviewDefinition
  controls?: UiLabControlDefinition[]
  agentExport?: UiLabAgentExportDefinition
}

export interface UiComponentDefinition {
  id: string
  name: string
  title: string
  description: string
  category: UiComponentCategory
  status: UiComponentStatus
  owner: string
  version: string
  canonical: string
  replacement?: string
  implementation: {
    kind: 'talos-native' | 'primevue-adapter' | 'composition'
    path: string
  }
  variants: string[]
  states: string[]
  sizes?: string[]
  capabilities?: string[]
  lab?: UiLabContract
  conformance: {
    level: UiConformanceLevel
    themes: Array<'dark' | 'light'>
    modes: Array<'work' | 'hud' | 'hud-compact'>
    reducedMotion: boolean
    accessibility: boolean
    visualRegression: boolean
  }
}

interface UiComponentRegistry {
  schemaVersion: '1.1.0'
  generatedAt: string
  components: UiComponentDefinition[]
}

export const uiComponentRegistry = Object.freeze(
  registryData as unknown as UiComponentRegistry,
)

export const registeredComponents = Object.freeze(uiComponentRegistry.components)

export function getComponentDefinition(id: string): UiComponentDefinition | undefined {
  return registeredComponents.find((component) => component.id === id)
}

export function getComponentsByCategory(category: UiComponentCategory): UiComponentDefinition[] {
  return registeredComponents.filter((component) => component.category === category)
}
