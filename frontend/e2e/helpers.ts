import { expect, type Locator, type Page } from '@playwright/test'

export const SANDBOX_IFRAME = 'iframe[src="/ui-lab/sandbox"]'

export function sandbox(page: Page) {
  return page.frameLocator(SANDBOX_IFRAME)
}

export async function gotoLab(page: Page): Promise<void> {
  await page.goto('/ui-lab')
  await expect(page.locator('.ui-lab-vnext')).toBeVisible({ timeout: 20_000 })
  await expect(sandbox(page).locator('.sandbox-stage')).toBeVisible({ timeout: 20_000 })
}

export interface LabDebugState {
  revision: number
  componentId: string
  sceneId: string
  theme: string
  locale: string
  direction: string
  reducedMotion: boolean
  activeMotionId: string | null
  motionStatus: string
  motionCurrentTime: number
  changes: Array<{ path: string; source: string; revision: number; before: unknown; after: unknown }>
  diffChanges: Array<{ path: string }>
  diffBaselineRevision: number
  diffCurrentRevision: number
  snapshotBaselineRevision: number
  snapshotCurrentRevision: number
  prompt: string
}

export async function labDebug(page: Page): Promise<LabDebugState> {
  return page.evaluate(() => {
    const d = (window as unknown as Record<string, unknown>).__uiLabDebug as {
      getState: () => {
        revision: number
        component: { id: string }
        scene: { id: string }
        environment: { theme: string; locale: string; direction: string; reducedMotion: boolean }
        motion: { activeMotionId: string | null; status: string; currentTime: number }
      }
      getChanges: () => Array<{ path: string; source: string; revision: number; before: unknown; after: unknown }>
      getDiff: () => { changes: Array<{ path: string }>; baselineRevision: number; currentRevision: number }
      getSnapshot: () => { baselineRevision: number; currentRevision: number; prompt?: never }
      getPrompt: () => string
    }
    const state = d.getState()
    const diff = d.getDiff()
    const snapshot = d.getSnapshot()
    return {
      revision: state.revision,
      componentId: state.component.id,
      sceneId: state.scene.id,
      theme: state.environment.theme,
      locale: state.environment.locale,
      direction: state.environment.direction,
      reducedMotion: state.environment.reducedMotion,
      activeMotionId: state.motion.activeMotionId,
      motionStatus: state.motion.status,
      motionCurrentTime: state.motion.currentTime,
      changes: d.getChanges(),
      diffChanges: diff.changes,
      diffBaselineRevision: diff.baselineRevision,
      diffCurrentRevision: diff.currentRevision,
      snapshotBaselineRevision: snapshot.baselineRevision,
      snapshotCurrentRevision: snapshot.currentRevision,
      prompt: d.getPrompt(),
    }
  })
}

export async function waitForDebug(
  page: Page,
  predicate: (state: LabDebugState) => boolean,
  label: string,
): Promise<LabDebugState> {
  await expect
    .poll(async () => predicate(await labDebug(page)), {
      timeout: 10_000,
      message: `debug state: ${label}`,
    })
    .toBe(true)
  return labDebug(page)
}

/** 点击 PrimeVue Select（trigger 已定位）后选择选项。 */
export async function selectPrimeOption(page: Page, trigger: Locator, optionText: string): Promise<void> {
  await trigger.click()
  const option = page.locator('.p-select-option', { hasText: optionText }).first()
  await expect(option).toBeVisible({ timeout: 5_000 })
  await option.click()
  await expect(option).toBeHidden()
}

export async function selectComponent(page: Page, title: string): Promise<void> {
  const button = page.locator('.registry-pane button', { hasText: title }).first()
  await expect(button).toBeVisible()
  await button.click()
  await waitForDebug(page, (s) => s.componentId === titleToId(title), `component=${title}`)
}

function titleToId(title: string): string {
  if (title.includes('Button')) return 'talos.ui.button'
  if (title.includes('Status Indicator')) return 'talos.ui.status-indicator'
  if (title.includes('Loading Overlay')) return 'talos.ui.loading-overlay'
  return ''
}

export async function waitForSandboxMarker(page: Page, selector: string): Promise<void> {
  await expect(sandbox(page).locator(selector).first()).toBeVisible({ timeout: 15_000 })
}

export async function setToolbarLocale(page: Page, label: '中文' | 'EN'): Promise<void> {
  await page.locator('.toolbar-controls .p-selectbutton').nth(2).locator('button', { hasText: label }).click()
  await waitForDebug(page, (s) => s.locale === (label === '中文' ? 'zh-CN' : 'en'), `locale=${label}`)
}

export async function setToolbarTheme(page: Page, label: '深色' | '浅色' | 'Dark' | 'Light'): Promise<void> {
  const isZh = label === '深色' || label === '浅色'
  await page
    .locator('.toolbar-controls .p-selectbutton')
    .nth(0)
    .locator('button', { hasText: label })
    .click()
  await waitForDebug(
    page,
    (s) => s.theme === (label === '深色' || label === 'Dark' ? 'dark' : 'light'),
    `theme=${label}`,
  )
  void isZh
}
