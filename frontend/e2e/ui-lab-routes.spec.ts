// 路由边界 + 无业务请求
import { expect, test } from '@playwright/test'
import { gotoLab, SANDBOX_IFRAME } from './helpers'

const BUSINESS_PREFIXES = ['/auth', '/users', '/devices', '/audit-logs', '/tenant-memberships', '/api']

test.describe('UI Lab route boundary (dev)', () => {
  test('/ui-lab is reachable without login', async ({ page }) => {
    const businessCalls: string[] = []
    page.on('request', (request) => {
      const url = new URL(request.url())
      if (BUSINESS_PREFIXES.some((prefix) => url.pathname.startsWith(prefix))) {
        businessCalls.push(request.url())
      }
    })
    await gotoLab(page)
    expect(businessCalls).toEqual([])
  })

  test('/ui-lab/sandbox is loaded inside the Host iframe', async ({ page }) => {
    await gotoLab(page)
    const frame = page.locator(SANDBOX_IFRAME)
    await expect(frame).toHaveCount(1)
    await expect(frame).toHaveAttribute('title', 'TALOS UI Lab component sandbox')
  })

  test('sandbox renders a trusted component fixture, not a login wall', async ({ page }) => {
    await gotoLab(page)
    await expect(page.frameLocator(SANDBOX_IFRAME).locator('.sandbox-stage .talos-button')).toBeVisible()
  })
})
