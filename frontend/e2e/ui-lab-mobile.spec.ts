// 移动端（390×844）抽屉操作闭环
import { expect, test } from '@playwright/test'
import { gotoLab, sandbox, waitForDebug, waitForSandboxMarker } from './helpers'

test.use({ viewport: { width: 390, height: 844 } })

test.describe('mobile workbench at 390px', () => {
  test('opens the Registry drawer, switches three samples, closes, then edits in the Inspector drawer', async ({
    page,
  }) => {
    await gotoLab(page)

    // 打开 Registry 抽屉
    await page.locator('.mobile-actions button', { hasText: '组件注册表' }).click()
    const drawer = page.locator('.mobile-pane')
    await expect(drawer).toBeVisible()
    await expect(drawer).toHaveAttribute('aria-label', '组件注册表')

    const samples: Array<{ title: string; marker: string }> = [
      { title: 'TALOS Button', marker: '.talos-button' },
      { title: 'TALOS Status Indicator', marker: '.talos-status' },
      { title: 'TALOS Loading Overlay', marker: '.loading-overlay' },
    ]
    for (const sample of samples) {
      await drawer.locator('button', { hasText: sample.title }).first().click()
      // 抽屉正确关闭
      await expect(page.locator('.mobile-pane')).toBeHidden()
      await waitForSandboxMarker(page, sample.marker)
      // 当前组件真实变化
      await waitForDebug(
        page,
        (s) => s.componentId === (sample.title.includes('Button') ? 'talos.ui.button'
          : sample.title.includes('Status') ? 'talos.ui.status-indicator'
            : 'talos.ui.loading-overlay'),
        `switch=${sample.title}`,
      )
      // 重新打开抽屉进入下一轮
      await page.locator('.mobile-actions button', { hasText: '组件注册表' }).click()
      await expect(page.locator('.mobile-pane')).toBeVisible()
    }
    // 关闭抽屉
    await page.locator('.mobile-pane .mobile-pane-close').click()
    await expect(page.locator('.mobile-pane')).toBeHidden()

    // 打开 Inspector 抽屉并修改属性（当前组件为 Loading Overlay → 改 message）
    await page.locator('.mobile-actions button', { hasText: '属性检查器' }).click()
    await expect(page.locator('.mobile-pane')).toBeVisible()
    await expect(page.locator('.mobile-pane')).toHaveAttribute('aria-label', '属性检查器')

    const messageInput = page.locator('.mobile-pane label', { hasText: '提示文本' }).locator('.p-inputtext')
    await messageInput.fill('正在同步本地样本')
    await expect(sandbox(page).locator('.loading-overlay strong')).toHaveText('正在同步本地样本')
  })
})
