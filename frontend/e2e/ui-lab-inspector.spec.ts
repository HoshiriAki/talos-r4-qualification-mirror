// Inspector 修改 → 真实 DOM / Snapshot / Journal / Diff 一致
import { expect, test, type Page } from '@playwright/test'
import {
  gotoLab,
  labDebug,
  sandbox,
  selectComponent,
  selectPrimeOption,
  waitForDebug,
  waitForSandboxMarker,
} from './helpers'

async function changeButtonVariant(page: Page, option: string, classSuffix: string): Promise<void> {
  const label = page.locator('.inspector label', { hasText: '变体' }).first()
  await selectPrimeOption(page, label.locator('.p-select'), option)
  await waitForSandboxMarker(page, `.talos-button--${classSuffix}`)
}

test.describe('Button inspector → real DOM', () => {
  test('variant/state/label drive the iframe DOM and update Snapshot, Journal and Diff', async ({ page }) => {
    await gotoLab(page)

    // variant → secondary
    await changeButtonVariant(page, '次级', 'secondary')
    let state = await labDebug(page)
    expect(state.diffChanges.map((c) => c.path)).toContain('component.props.variant')
    expect(state.changes[0].path).toBe('component.props.variant')
    expect(state.changes[0].source).toBe('inspector')
    expect(state.snapshotCurrentRevision).toBe(state.revision)
    expect(state.diffCurrentRevision).toBe(state.revision)

    // state → disabled
    await selectPrimeOption(page, page.locator('.inspector label', { hasText: '状态' }).locator('.p-select'), '禁用')
    await waitForSandboxMarker(page, '.talos-button--disabled')
    await expect(sandbox(page).locator('.talos-button')).toBeDisabled()

    // label → 新文案
    const labelInput = page.locator('.inspector label', { hasText: '标签' }).locator('.p-inputtext')
    await labelInput.fill('执行转换')
    await waitForDebug(page, (s) => s.diffChanges.some((c) => c.path === 'component.props.label'), 'label journaled')
    await expect(sandbox(page).locator('.talos-button')).toContainText('执行转换')

    state = await labDebug(page)
    expect(state.changes.length).toBeGreaterThanOrEqual(3)
    // Snapshot、Diff、Prompt 同一 revision
    expect(state.snapshotCurrentRevision).toBe(state.diffCurrentRevision)
    expect(state.prompt).toContain('component.props.variant')
    expect(state.prompt).toContain('component.props.label')
  })
})

test.describe('Status Indicator inspector', () => {
  test('status drives visual state, aria-label and locale-aware label', async ({ page }) => {
    await gotoLab(page)
    await selectComponent(page, 'TALOS Status Indicator')
    await waitForSandboxMarker(page, '.talos-status')

    await selectPrimeOption(page, page.locator('.inspector label', { hasText: '状态' }).locator('.p-select'), '已逾期')
    await waitForSandboxMarker(page, '.talos-status--overdue')
    await expect(sandbox(page).locator('.talos-status')).toHaveAttribute('aria-label', '已逾期')

    // 切换 en：aria-label 跟随 locale
    await page.locator('.toolbar-controls .p-selectbutton').nth(2).locator('button', { hasText: 'EN' }).click()
    await waitForDebug(page, (s) => s.locale === 'en', 'locale=en')
    await expect(sandbox(page).locator('.talos-status')).toHaveAttribute('aria-label', 'Overdue')
  })
})

test.describe('Loading Overlay inspector', () => {
  test('maskOpacity and message change real sandbox styles', async ({ page }) => {
    await gotoLab(page)
    await selectComponent(page, 'TALOS Loading Overlay')
    await waitForSandboxMarker(page, '.loading-overlay')

    // message
    const messageInput = page.locator('.inspector label', { hasText: '提示文本' }).locator('.p-inputtext')
    await messageInput.fill('正在同步本地样本')
    await expect(sandbox(page).locator('.loading-overlay strong')).toHaveText('正在同步本地样本')

    // maskOpacity 是 scene 控件，先切到 Scene tab
    await page.locator('.inspector .p-tablist button', { hasText: '场景' }).click()
    const slider = page.locator('.inspector label', { hasText: '遮罩透明度' }).locator('.p-slider')
    const box = await slider.boundingBox()
    if (!box) throw new Error('mask slider missing')
    // 点击最右端（range 上限 0.9）
    await slider.click({ position: { x: Math.max(box.width - 2, 1), y: box.height / 2 } })
    await expect
      .poll(async () => {
        const opacity = await sandbox(page)
          .locator('.sandbox')
          .evaluate((el) => getComputedStyle(el).getPropertyValue('--mask-opacity'))
        return Number.parseFloat(opacity)
      }, { timeout: 10_000 })
      .toBeGreaterThan(0.5)
    await expect(sandbox(page).locator('.loading-overlay')).toBeVisible()
  })
})
