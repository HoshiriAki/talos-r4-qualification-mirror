export { default as TalosButton } from './components/TalosButton.vue'
export { default as TalosStatusIndicator } from './components/TalosStatusIndicator.vue'
export { default as TalosLoadingOverlayFixture } from './components/TalosLoadingOverlayFixture.vue'
export {
  getComponentDefinition,
  getComponentsByCategory,
  registeredComponents,
  uiComponentRegistry,
} from './registry'
export type {
  UiComponentCategory,
  UiComponentDefinition,
  UiComponentStatus,
  UiConformanceLevel,
  UiLabContract,
  UiLabControlDefinition,
  UiLabControlScope,
  UiLabControlType,
  UiLabDesignSlotDefinition,
  UiLabAgentExportDefinition,
  UiLabPreviewDefinition,
  UiLabDirection,
  UiLabFutureMotionEngine,
  UiLabLocale,
  UiLabMotionDefinition,
  UiLabMotionEasingDefinition,
  UiLabMotionEngine,
  UiLabMotionKeyframeDefinition,
  UiLabMotionTarget,
  UiLabMotionTrigger,
  UiLabSlotKind,
  UiLabSlotPosition,
} from './registry'
export type { TalosButtonSize, TalosButtonState, TalosButtonVariant, TalosStatus } from './component-types'
