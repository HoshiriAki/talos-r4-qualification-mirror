// 协议稳定性：一次 Inspector 修改不得产生 INIT/SNAPSHOT 消息循环。
import { expect, test, type Page } from '@playwright/test'
import { gotoLab, SANDBOX_IFRAME, selectPrimeOption } from './helpers'

interface Counters {
  hostReceived: Record<string, number>
  sandboxReceived: Record<string, number>
}

async function installCounters(page: Page): Promise<void> {
  await page.evaluate(() => {
    const host: Record<string, number> = {}
    const sandboxReceived: Record<string, number> = {}
    window.addEventListener('message', (event) => {
      const type = (event.data as { type?: string } | null)?.type
      if (type) host[type] = (host[type] ?? 0) + 1
    })
    const iframe = document.querySelector('iframe[src="/ui-lab/sandbox"]') as HTMLIFrameElement | null
    if (iframe?.contentWindow) {
      iframe.contentWindow.addEventListener('message', (event) => {
        const type = (event.data as { type?: string } | null)?.type
        if (type) sandboxReceived[type] = (sandboxReceived[type] ?? 0) + 1
      })
    }
    ;(window as unknown as Record<string, unknown>).__labCounters = { hostReceived: host, sandboxReceived: sandboxReceived }
  })
}

async function readCounters(page: Page): Promise<Counters> {
  return page.evaluate(() => (window as unknown as Record<string, unknown>).__labCounters as Counters)
}

test.describe('protocol stability', () => {
  test('one inspector change produces bounded patches and no snapshot/init loop', async ({ page }) => {
    await gotoLab(page)
    await installCounters(page)

    // 一次 Inspector 修改
    await selectPrimeOption(page, page.locator('.inspector label', { hasText: '变体' }).locator('.p-select'), '次级')

    await page.waitForTimeout(500)
    const afterChange = await readCounters(page)
    expect(afterChange.sandboxReceived.LAB_PATCH ?? 0).toBeGreaterThanOrEqual(1)
    // 不重复 INIT
    expect(afterChange.sandboxReceived.LAB_INIT ?? 0).toBe(0)
    // 不再无条件回发 SNAPSHOT
    expect(afterChange.hostReceived.SANDBOX_SNAPSHOT ?? 0).toBe(0)

    // 稳定窗口：无持续增长
    await page.waitForTimeout(1200)
    const afterWait = await readCounters(page)
    expect(afterWait.sandboxReceived.LAB_PATCH ?? 0).toBe(afterChange.sandboxReceived.LAB_PATCH ?? 0)
    expect(afterWait.hostReceived.SANDBOX_SNAPSHOT ?? 0).toBe(afterChange.hostReceived.SANDBOX_SNAPSHOT ?? 0)
    expect(afterWait.sandboxReceived.LAB_INIT ?? 0).toBe(0)
  })

  test('component switch re-initializes exactly once, then stabilizes', async ({ page }) => {
    await gotoLab(page)
    await installCounters(page)

    await page.locator('.registry-pane button', { hasText: 'TALOS Status Indicator' }).first().click()
    await page.waitForTimeout(400)
    const afterSwitch = await readCounters(page)
    expect(afterSwitch.sandboxReceived.LAB_INIT ?? 0).toBe(1)

    await page.waitForTimeout(1000)
    const afterWait = await readCounters(page)
    expect(afterWait.sandboxReceived.LAB_INIT).toBe(1)
  })
})
