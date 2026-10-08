// Baseline / Snapshot / Diff / Prompt revision 一致 + Reset 恢复
import { expect, test } from '@playwright/test'
import { gotoLab, labDebug, selectPrimeOption, waitForDebug } from './helpers'

test.describe('baseline and output consistency', () => {
  test('Snapshot, Diff and Prompt share the same revision; Reset restores baseline', async ({ page }) => {
    await gotoLab(page)

    const initial = await labDebug(page)
    expect(initial.revision).toBe(0)
    expect(initial.diffBaselineRevision).toBe(0)
    expect(initial.diffCurrentRevision).toBe(0)
    expect(initial.diffChanges).toEqual([])

    // 修改两个属性
    await selectPrimeOption(page, page.locator('.inspector label', { hasText: '变体' }).locator('.p-select'), '次级')
    await selectPrimeOption(page, page.locator('.inspector label', { hasText: '状态' }).locator('.p-select'), '禁用')

    const modified = await waitForDebug(page, (s) => s.changes.length >= 2, 'two changes journaled')
    expect(modified.revision).toBe(2)
    // 三份输出 revision 一致
    expect(modified.snapshotCurrentRevision).toBe(modified.revision)
    expect(modified.diffCurrentRevision).toBe(modified.revision)
    expect(modified.diffChanges.map((c) => c.path).sort()).toEqual([
      'component.props.state',
      'component.props.variant',
    ])
    // prompt 包含同一变更
    expect(modified.prompt).toContain('component.props.variant')
    expect(modified.prompt).toContain('component.props.state')

    // Reset → 恢复 baseline，Diff 清空
    await page.locator('.revision button', { hasText: '重置' }).click()
    const reset = await waitForDebug(page, (s) => s.changes.length === 0 && s.revision > 0, 'reset to baseline')
    expect(reset.diffChanges).toEqual([])
    expect(reset.diffCurrentRevision).toBe(reset.revision)
    expect(reset.snapshotCurrentRevision).toBe(reset.revision)
    // DOM 回到默认（Reset 通过 INIT 重发，等待渲染）
    await expect(
      page.frameLocator('iframe[src="/ui-lab/sandbox"]').locator('.talos-button--execution-primary'),
    ).toBeVisible({ timeout: 10_000 })
  })

  test('switching components re-establishes a fresh baseline per component', async ({ page }) => {
    await gotoLab(page)
    await selectPrimeOption(page, page.locator('.inspector label', { hasText: '变体' }).locator('.p-select'), '危险')

    await page.locator('.registry-pane button', { hasText: 'TALOS Status Indicator' }).first().click()
    const after = await waitForDebug(page, (s) => s.componentId === 'talos.ui.status-indicator', 'switched')
    expect(after.revision).toBe(0)
    expect(after.changes).toEqual([])
    expect(after.diffChanges).toEqual([])
  })
})
