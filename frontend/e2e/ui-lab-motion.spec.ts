// Motion：选择任意 motion、probe target、缺失 target 诊断、restart、seek、reduced-motion
import { expect, test } from '@playwright/test'
import { gotoLab, sandbox, selectPrimeOption, waitForDebug } from './helpers'

async function setEngine(page: Page, engine: string): Promise<void> {
  const engineGroup = page.locator('.motion-controls .p-selectbutton').nth(1)
  await engineGroup.locator('button', { hasText: engine }).click()
  await waitForDebug(page, (s) => s.revision >= 0, 'engine set')
}

async function selectMotion(page: Page, title: string): Promise<void> {
  const trigger = page.locator('.motion-head .p-select').first()
  await selectPrimeOption(page, trigger, title)
  await waitForDebug(page, (s) => Boolean(s.activeMotionId), `motion=${title}`)
  // 让 LAB_PATCH（activeMotionId）传播到 sandbox，避免 sandbox fallback 到 motions[0]
  await page.waitForTimeout(200)
}

async function openMotionTab(page: Page): Promise<void> {
  await page.locator('.bottom-dock .p-tablist button', { hasText: '动画' }).click()
  await expect(page.locator('.bottom-dock .motion-head')).toBeVisible()
}

/** 切到 Diagnostics tab，使诊断条目可见。 */
async function openDiagnosticsTab(page: Page): Promise<void> {
  await page.locator('.bottom-dock .p-tablist button', { hasText: '诊断' }).click()
  await expect(page.locator('.bottom-dock .diagnostics')).toBeVisible()
}

async function expectDiagnosticVisible(page: Page, text: string): Promise<void> {
  await openDiagnosticsTab(page)
  await expect(page.locator('.bottom-dock .diagnostics li', { hasText: text }).first()).toBeVisible({
    timeout: 10_000,
  })
}

test.describe('motion selection and target resolution', () => {
  test('plays a non-first motion on the probe, not the specimen root', async ({ page }) => {
    await gotoLab(page)
    await openMotionTab(page)
    await setEngine(page, 'Native')

    // 选择第二个 motion（signal-sweep → probe target）
    await selectMotion(page, 'Signal sweep')
    await page.locator('.motion-actions button', { hasText: '播放' }).click()
    // 播放命令进入诊断
    await expectDiagnosticVisible(page, 'Motion:')

    // native 引擎用 WAAPI（el.animate），不写 inline style —— 断言 computed transform。
    // probe（.talos-button__signal）的动画在播放过程中会反映为 matrix 变换。
    await expect
      .poll(
        () =>
          sandbox(page)
            .locator('.talos-button__signal')
            .evaluate((el) => getComputedStyle(el).transform),
        { timeout: 10_000 },
      )
      .not.toBe('none')
    // 根节点保持静止（specimen 根不被 probe 动画污染）
    const rootTransform = await sandbox(page)
      .locator('.talos-button')
      .evaluate((el) => getComputedStyle(el).transform)
    expect(rootTransform).toBe('none')
  })

  test('missing motion target raises a Diagnostic instead of falling back to the root', async ({ page }) => {
    await gotoLab(page)
    await openMotionTab(page)
    await setEngine(page, 'Native')

    await selectMotion(page, 'Trailing sweep')
    await page.locator('.motion-actions button', { hasText: '播放' }).click()

    await expectDiagnosticVisible(page, '动画目标缺失')
  })

  test('restart and seek update state through commands', async ({ page }) => {
    await gotoLab(page)
    await openMotionTab(page)
    await setEngine(page, 'Native')
    await selectMotion(page, 'Press response')

    await page.locator('.motion-actions button', { hasText: '播放' }).click()
    await expectDiagnosticVisible(page, 'Motion:')

    // 切回 motion tab 后操作
    await openMotionTab(page)
    await page.locator('.motion-actions button', { hasText: '重播' }).click()
    await expectDiagnosticVisible(page, 'restart')

    // seek：回到 motion tab 后点击进度条中部
    await openMotionTab(page)
    const slider = page.locator('.motion-controls .p-slider').first()
    const box = await slider.boundingBox()
    if (!box) throw new Error('seek slider missing')
    await slider.click({ position: { x: box.width * 0.5, y: box.height / 2 } })
    const state = await waitForDebug(page, (s) => s.motionCurrentTime > 0.1, 'seek applied')
    expect(state.motionCurrentTime).toBeGreaterThan(0.1)
  })

  test('reduced-motion simulation finishes playback statically without progress', async ({ page }) => {
    await gotoLab(page)
    await openMotionTab(page)
    await setEngine(page, 'Native')
    await selectMotion(page, 'Press response')

    // 开启 reduced-motion 模拟
    await page
      .locator('.motion-controls label', { hasText: '减少动画' })
      .locator('.p-toggleswitch')
      .click()
    await waitForDebug(page, (s) => s.reducedMotion === true, 'reducedMotion on')

    await page.locator('.motion-actions button', { hasText: '播放' }).click()
    await waitForDebug(page, (s) => s.motionStatus === 'finished', 'reduced playback finished')
    await expectDiagnosticVisible(page, '"reduced":true')
  })
})
