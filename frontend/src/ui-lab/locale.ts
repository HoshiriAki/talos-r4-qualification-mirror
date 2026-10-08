export type UiLabLocale = 'zh-CN' | 'en'

const copy = {
  'zh-CN': {
    registry: '组件注册表', assets: '项资产', filter: '按 ID 或名称筛选', inspector: '属性检查器', component: '组件', scene: '场景', motion: '动画', validation: '验证', changes: '变更', prompt: '提示词', diagnostics: '诊断', reset: '重置', resetPage: '重置页面', resetAll: '全部重置', clean: '干净', dirty: '已修改', play: '播放', pause: '暂停', restart: '重播', seek: '进度', copyPrompt: '复制提示词', copyDiff: '复制 Diff JSON', copySnapshot: '复制 Snapshot JSON', download: '下载 JSON', noChanges: '与会话基线没有差异。', dark: '深色', light: '浅色', blank: '空白', surface: '表面', fullscreen: '全屏', loadingStage: '加载场景', message: '提示文本', maskOpacity: '遮罩透明度', variant: '变体', state: '状态', size: '尺寸', label: '标签', implementation: '实现路径', reducedMotion: '减少动画', accessibility: '无障碍', declared: '已声明', missing: '缺失', localFixture: '本地沙箱样本 · 无业务请求',
    dashboardShell: '仪表盘外壳', authShell: '认证外壳', modalContext: '模态上下文', backgroundStage: '背景舞台', timelineZoom: '时间轴缩放', loop: '循环', playbackRate: '播放速率', reducedMotionSimulation: '减少动画模拟', expand: '展开', collapse: '折叠', snapshot: '快照', restoreSnapshot: '恢复 Snapshot', diff: '差异', motionTargetMissing: '动画目标缺失', sandboxNotReady: '沙箱未就绪', unsupportedComponent: '不支持的沙箱组件', sessionReset: '会话已重置到基线。', componentReset: '组件属性已重置到基线。', copiedToClipboard: '已复制到剪贴板。', downloadStarted: '已下载', switchClearedChanges: '切换组件时已清除未导出的变更。', slotEnabled: '插槽已启用', slotDisabled: '插槽已禁用', slot: '插槽', sandboxBoundary: '可信组件渲染隔离层（非不可信代码沙箱）', sandboxIsolation: '沙箱：同源 iframe / 仅脚本', protocolGuard: '协议：1.0 / origin、source、session、sequence、revision 守卫', rendererMap: '渲染器：显式组件映射', enginePreference: '引擎偏好',
  },
  en: {
    registry: 'REGISTRY', assets: 'assets', filter: 'Filter ID or name', inspector: 'INSPECTOR', component: 'Component', scene: 'Scene', motion: 'Motion', validation: 'Validation', changes: 'Changes', prompt: 'Prompt', diagnostics: 'Diagnostics', reset: 'Reset', resetPage: 'Reset Page', resetAll: 'Reset All', clean: 'CLEAN', dirty: 'DIRTY', play: 'Play', pause: 'Pause', restart: 'Restart', seek: 'Seek', copyPrompt: 'Copy Prompt', copyDiff: 'Copy Diff JSON', copySnapshot: 'Copy Snapshot JSON', download: 'Download JSON', noChanges: 'No differences from the session baseline.', dark: 'Dark', light: 'Light', blank: 'Blank', surface: 'Surface', fullscreen: 'Fullscreen', loadingStage: 'Loading stage', message: 'Message', maskOpacity: 'Mask opacity', variant: 'Variant', state: 'State', size: 'Size', label: 'Label', implementation: 'Implementation', reducedMotion: 'Reduced motion', accessibility: 'Accessibility', declared: 'Declared', missing: 'Missing', localFixture: 'Local sandbox fixture · no business requests',
    dashboardShell: 'Dashboard shell', authShell: 'Auth shell', modalContext: 'Modal context', backgroundStage: 'Background stage', timelineZoom: 'Timeline zoom', loop: 'Loop', playbackRate: 'Playback rate', reducedMotionSimulation: 'Reduced-motion simulation', expand: 'Expand', collapse: 'Collapse', snapshot: 'Snapshot', restoreSnapshot: 'Restore Snapshot', diff: 'Diff', motionTargetMissing: 'Motion target missing', sandboxNotReady: 'Sandbox not ready', unsupportedComponent: 'Unsupported sandbox component', sessionReset: 'Session reset to baseline.', componentReset: 'Component props reset to baseline.', copiedToClipboard: 'Copied to clipboard.', downloadStarted: 'Downloaded', switchClearedChanges: 'Unexported changes were cleared when switching the component.', slotEnabled: 'Slot enabled', slotDisabled: 'Slot disabled', slot: 'Slots', sandboxBoundary: 'Trusted-component render isolation layer (not an untrusted-code sandbox)', sandboxIsolation: 'Sandbox: same-origin iframe / scripts only', protocolGuard: 'Protocol: 1.0 / origin, source, session, sequence, revision guarded', rendererMap: 'Renderer: explicit component map', enginePreference: 'Engine preference',
  },
} as const

export type UiLabCopyKey = keyof typeof copy.en

function hasCopyKey(key: string): key is UiLabCopyKey {
  return key in copy.en
}

export function labText(locale: UiLabLocale | string, key: UiLabCopyKey) {
  return (copy[locale as UiLabLocale] ?? copy['zh-CN'])[key]
}

export function controlLabel(locale: UiLabLocale, key: string) {
  const id = key.replace('uiLab.control.', '') as UiLabCopyKey
  return hasCopyKey(id) ? labText(locale, id) : id
}

export function sceneLabel(locale: UiLabLocale, key: string) {
  const id = key.replace('uiLab.scene.', '') as UiLabCopyKey
  return hasCopyKey(id) ? labText(locale, id) : id
}

export function labErrorLabel(locale: UiLabLocale, code: string): string {
  const key = code as UiLabCopyKey
  return hasCopyKey(key) ? labText(locale, key) : code
}
