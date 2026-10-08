// 视觉验收矩阵：3 组件 × dark/light × zh-CN/en × desktop/mobile + 定向样本。
// 截图输出到 .playwright-mcp/（相对项目根），命名稳定。
import { expect, test, type Page } from '@playwright/test'
import * as path from 'node:path'
import { gotoLab, labDebug, sandbox, selectPrimeOption, waitForDebug } from './helpers'

// cwd 为 frontend/ → .playwright-mcp 位于仓库根
const SHOT_DIR = path.resolve(process.cwd(), '../.playwright-mcp')

interface Shot {
  name: string
  buffer: Buffer
}

async function capture(page: Page, name: string, shots: Shot[]): Promise<void> {
  await page.waitForTimeout(180)
  const buffer = await page.locator('.ui-lab-vnext').screenshot({ animations: 'disabled' })
  shots.push({ name, buffer })
}

async function setComponent(page: Page, title: string): Promise<void> {
  await page.locator('.registry-pane button', { hasText: title }).first().click()
  await waitForDebug(page, (s) => s.componentId !== '', 'component')
}

async function setTheme(page: Page, label: string): Promise<void> {
  await page.locator('.toolbar-controls .p-selectbutton').nth(0).locator('button', { hasText: label }).click()
  await page.waitForTimeout(120)
}

async function setLocale(page: Page, label: string): Promise<void> {
  await page.locator('.toolbar-controls .p-selectbutton').nth(2).locator('button', { hasText: label }).click()
  await page.waitForTimeout(120)
}

/** 切到 Diagnostics tab 使诊断条目可见（当前 locale 下 tab 文案）。 */
async function openDiagnosticsTab(page: Page, tabText: string): Promise<void> {
  await page.locator('.bottom-dock .p-tablist button', { hasText: tabText }).click()
  await expect(page.locator('.bottom-dock .diagnostics')).toBeVisible()
}

test.describe('visual acceptance matrix', () => {
  test('captures the full matrix and targeted samples', async ({ page }) => {
    await gotoLab(page)
    const shots: Shot[] = []

    const components: Array<{ title: string; slug: string; marker: string }> = [
      { title: 'TALOS Button', slug: 'button', marker: '.talos-button' },
      { title: 'TALOS Status Indicator', slug: 'status', marker: '.talos-status' },
      { title: 'TALOS Loading Overlay', slug: 'loading', marker: '.loading-overlay' },
    ]
    const themes: Array<{ labelZh: string; labelEn: string; slug: string }> = [
      { labelZh: '深色', labelEn: 'Dark', slug: 'dark' },
      { labelZh: '浅色', labelEn: 'Light', slug: 'light' },
    ]
    const locales: Array<{ label: string; slug: string }> = [
      { label: '中文', slug: 'zh' },
      { label: 'EN', slug: 'en' },
    ]

    // ── 基础矩阵（desktop）──
    await page.setViewportSize({ width: 1600, height: 1000 })
    for (const component of components) {
      await setComponent(page, component.title)
      await expect(page.frameLocator('iframe[src="/ui-lab/sandbox"]').locator(component.marker).first()).toBeVisible()
      for (const theme of themes) {
        await setLocale(page, '中文')
        await setTheme(page, theme.labelZh)
        await setLocale(page, 'EN')
        await setTheme(page, theme.labelEn)
        for (const locale of locales) {
          await setLocale(page, locale.label)
          await setTheme(page, locale.slug === 'zh' ? theme.labelZh : theme.labelEn)
          await capture(page, `${component.slug}-${theme.slug}-${locale.slug}-desktop.png`, shots)
        }
      }
    }

    // ── 基础矩阵（mobile 390px）──
    // 390px 下 registry 抽屉与 toolbar 控件隐藏；组件覆盖由 desktop 矩阵承担。
    // mobile 状态切换通过 dev-only debug 钩子的 applyPatch（同一 commitLabPatch 闭环）。
    await setComponent(page, 'TALOS Button')
    await page.evaluate(() => {
      const d = (window as unknown as Record<string, unknown>).__uiLabDebug as {
        applyPatch: (path: string, value: unknown) => void
      }
      d.applyPatch('environment.theme', 'dark')
      d.applyPatch('environment.locale', 'zh-CN')
    })
    await page.setViewportSize({ width: 390, height: 844 })
    await page.waitForTimeout(250)
    await capture(page, 'button-dark-zh-mobile.png', shots)

    await page.evaluate(() => {
      const d = (window as unknown as Record<string, unknown>).__uiLabDebug as {
        applyPatch: (path: string, value: unknown) => void
      }
      d.applyPatch('environment.theme', 'light')
      d.applyPatch('environment.locale', 'en')
    })
    await page.waitForTimeout(250)
    await capture(page, 'button-light-en-mobile.png', shots)

    // ── 定向样本 ──
    await page.setViewportSize({ width: 1600, height: 1000 })

    // RTL
    await setComponent(page, 'TALOS Status Indicator')
    await setLocale(page, 'EN')
    await page.locator('.toolbar-controls .p-selectbutton').nth(1).locator('button', { hasText: 'RTL' }).click()
    await waitForDebug(page, (s) => s.direction === 'rtl', 'rtl')
    await capture(page, 'status-rtl-en-desktop.png', shots)

    // reduced-motion（loading-overlay）
    await setComponent(page, 'TALOS Loading Overlay')
    await setLocale(page, '中文')
    await page.locator('.bottom-dock .p-tablist button', { hasText: '动画' }).click()
    await page.locator('.motion-controls label', { hasText: '减少动画' }).locator('.p-toggleswitch').click()
    await waitForDebug(page, (s) => s.reducedMotion === true, 'rm')
    await capture(page, 'loading-reduced-motion-zh-desktop.png', shots)

    // Motion active（button, native, press response）
    await setComponent(page, 'TALOS Button')
    await setLocale(page, '中文')
    await page.locator('.motion-controls .p-selectbutton').nth(1).locator('button', { hasText: 'Native' }).click()
    await page.locator('.motion-head .p-select').first().click()
    await page.locator('.p-select-option', { hasText: 'Press response' }).first().click()
    await page.locator('.motion-actions button', { hasText: '播放' }).click()
    await openDiagnosticsTab(page, '诊断')
    await expect(page.locator('.diagnostics li', { hasText: 'Motion:' }).first()).toBeVisible({ timeout: 10_000 })
    // 截图前回到 motion tab 展示播放态
    await page.locator('.bottom-dock .p-tablist button', { hasText: '动画' }).click()
    await capture(page, 'button-motion-active-zh-desktop.png', shots)

    // Design slot active（Slots tab）
    await page.locator('.inspector .p-tablist button', { hasText: '插槽' }).click()
    await expect(page.locator('.inspector .slot-row').first()).toBeVisible()
    await capture(page, 'button-slot-active-zh-desktop.png', shots)

    // Inspector modified + Journal populated + Prompt/Diff output
    await setComponent(page, 'TALOS Button')
    await setLocale(page, 'EN')
    await page.locator('.inspector .p-tablist button', { hasText: 'Component' }).click()
    await selectPrimeOption(page, page.locator('.inspector label', { hasText: 'Variant' }).locator('.p-select'), 'Secondary')
    await selectPrimeOption(page, page.locator('.inspector label', { hasText: 'State' }).locator('.p-select'), 'Disabled')
    await capture(page, 'button-inspector-modified-en-desktop.png', shots)

    await page.locator('.bottom-dock .p-tablist button', { hasText: 'Changes' }).click()
    await expect(page.locator('.bottom-dock .changes article').first()).toBeVisible()
    await capture(page, 'button-journal-populated-en-desktop.png', shots)

    await page.locator('.bottom-dock .p-tablist button', { hasText: 'Prompt' }).click()
    await expect(page.locator('.bottom-dock .export textarea')).toBeVisible()
    await capture(page, 'button-diff-output-en-desktop.png', shots)

    // Diagnostics error state（missing motion target：先选 trailing-sweep）
    await openDiagnosticsTab(page, 'Diagnostics')
    await page.locator('.bottom-dock .p-tablist button', { hasText: 'Motion' }).click()
    await page.locator('.motion-head .p-select').first().click()
    await page.locator('.p-select-option', { hasText: 'Trailing sweep' }).first().click()
    await page.waitForTimeout(200)
    await page.locator('.motion-actions button', { hasText: 'Play' }).click()
    await openDiagnosticsTab(page, 'Diagnostics')
    await expect(page.locator('.diagnostics li', { hasText: 'Motion target missing' }).first()).toBeVisible({ timeout: 10_000 })
    await capture(page, 'button-diagnostics-error-en-desktop.png', shots)

    // 写盘（先于断言，确保断言失败也保留证据）
    const { writeFileSync, mkdirSync } = await import('node:fs')
    mkdirSync(SHOT_DIR, { recursive: true })
    for (const shot of shots) {
      writeFileSync(path.join(SHOT_DIR, shot.name), shot.buffer)
    }
    await test.info().attach('visual-bundle-manifest', {
      body: JSON.stringify(shots.map((s) => s.name), null, 2),
      contentType: 'application/json',
    })
    // 导出最终 runtime export bundle（真实样本：journal populated、非空 diff、
    // prompt、missing-target Diagnostic、restored baseline 快照），供 bundle 打包。
    // 此时状态含 motion trailing-sweep 已 Play（motionSlotDisabled Diagnostic）与
    // inspector 修改（variant=Secondary / state=Disabled），diagnostics 非空。
    const finalExport = await page.evaluate(() => (window as unknown as { __uiLabDebug: { getExportBundle: () => unknown } }).__uiLabDebug.getExportBundle())
    writeFileSync(path.join(SHOT_DIR, 'ui-lab-export.json'), JSON.stringify(finalExport, null, 2))
    // 断言生成数量符合矩阵（3 组件 × dark/light × zh-CN/en × desktop = 12，
    // mobile 2（390px 下 registry 隐藏，仅 button），定向样本 8）
    expect(shots.length).toBeGreaterThanOrEqual(22)
  })
})

// 引用 sandbox 避免未使用告警
void sandbox
