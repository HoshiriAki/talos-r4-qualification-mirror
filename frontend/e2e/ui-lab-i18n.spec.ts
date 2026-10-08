// 国际化与方向
import { expect, test } from '@playwright/test'
import { gotoLab, sandbox, waitForDebug } from './helpers'

test.describe('i18n and direction', () => {
  test('workbench copy, scene label and status aria-label switch with locale', async ({ page }) => {
    await gotoLab(page)

    // 默认 zh-CN：场景 select 显示“表面”
    await expect(page.locator('.toolbar-controls .p-select').first()).toContainText('表面')
    await expect(page.locator('.registry-pane header')).toContainText('组件注册表')

    // 切换 EN
    await page.locator('.toolbar-controls .p-selectbutton').nth(2).locator('button', { hasText: 'EN' }).click()
    await waitForDebug(page, (s) => s.locale === 'en', 'locale=en')
    await expect(page.locator('.toolbar-controls .p-select').first()).toContainText('Surface')
    await expect(page.locator('.registry-pane header')).toContainText('REGISTRY')
    await expect(page.locator('.inspector header')).toContainText('INSPECTOR')
  })

  test('Status Indicator aria-label follows locale in the sandbox', async ({ page }) => {
    await gotoLab(page)
    await page.locator('.registry-pane button', { hasText: 'TALOS Status Indicator' }).first().click()
    await waitForDebug(page, (s) => s.componentId === 'talos.ui.status-indicator', 'switched to status')

    await expect(sandbox(page).locator('.talos-status')).toHaveAttribute('aria-label', '进行中')
    await page.locator('.toolbar-controls .p-selectbutton').nth(2).locator('button', { hasText: 'EN' }).click()
    await waitForDebug(page, (s) => s.locale === 'en', 'locale=en')
    await expect(sandbox(page).locator('.talos-status')).toHaveAttribute('aria-label', 'Active')
  })

  test('RTL applies to the sandbox scene', async ({ page }) => {
    await gotoLab(page)
    await page.locator('.toolbar-controls .p-selectbutton').nth(1).locator('button', { hasText: 'RTL' }).click()
    await waitForDebug(page, (s) => s.direction === 'rtl', 'direction=rtl')
    await expect(sandbox(page).locator('.sandbox')).toHaveAttribute('dir', 'rtl')
  })
})
