// ── UI Lab 场景目录 ──────────────────────────────────────────────────────
// 场景定义与翻译 key 分离：目录只声明 id + labelKey，显示文本由调用方
// 按当前 locale 读取。禁止在 setup 时构造固定语言场景数组。

export interface LabSceneDefinition {
  id: string
  labelKey: string
}

export const LAB_SCENE_CATALOG: readonly LabSceneDefinition[] = Object.freeze([
  { id: 'blank', labelKey: 'uiLab.scene.blank' },
  { id: 'surface', labelKey: 'uiLab.scene.surface' },
  { id: 'dashboard-shell', labelKey: 'uiLab.scene.dashboardShell' },
  { id: 'auth-shell', labelKey: 'uiLab.scene.authShell' },
  { id: 'fullscreen', labelKey: 'uiLab.scene.fullscreen' },
  { id: 'modal-context', labelKey: 'uiLab.scene.modalContext' },
  { id: 'loading-stage', labelKey: 'uiLab.scene.loadingStage' },
  { id: 'background-stage', labelKey: 'uiLab.scene.backgroundStage' },
])

export function labSceneDefinition(sceneId: string): LabSceneDefinition | undefined {
  return LAB_SCENE_CATALOG.find((scene) => scene.id === sceneId)
}
