import { expect, test } from '@playwright/test'
import { gotoLab, labDebug, sandbox, waitForDebug } from './helpers'

test.describe('UI Lab runtime contract', () => {
  test('reload performs a fresh epoch handshake and restores a frozen snapshot baseline', async ({ page }) => {
    await gotoLab(page)
    await waitForDebug(page, (state) => state.revision === 0, 'initial session')

    // 1. 记录 reload 前的 epoch 与 revision
    const before = await page.evaluate(() => {
      const d = (window as unknown as Record<string, unknown>).__uiLabDebug as {
        getState: () => { revision: number }
        getSandboxContext: () => { ready: boolean; epoch: string | null; sessionId: string }
        getSandboxLifecycle: () => Array<{ ready: boolean; epoch: string | null }>
      }
      return { revision: d.getState().revision, context: d.getSandboxContext(), lifecycleLength: d.getSandboxLifecycle().length }
    })
    expect(before.context.ready).toBe(true)
    expect(before.context.epoch).toBeTruthy()

    // 2. 导出 snapshot，做一次临时修改
    const snapshot = await page.evaluate(() => (window as unknown as { __uiLabDebug: { getSnapshot: () => unknown } }).__uiLabDebug.getSnapshot())
    await page.evaluate(() => (window as unknown as { __uiLabDebug: { applyPatch: (path: string, value: unknown) => void } }).__uiLabDebug.applyPatch('component.props.label', 'Temporary'))
    await waitForDebug(page, (state) => state.changes.length === 1, 'temporary change')

    // 3. 仅 reload sandbox iframe（不 reload 整个 Host 页面）
    await page.locator('iframe[src="/ui-lab/sandbox"]').evaluate((element) => {
      const frame = element as HTMLIFrameElement
      frame.contentWindow?.location.reload()
    })

    // 5. ready 经历 false → true，epoch 更新，Host revision 保持
    await expect
      .poll(async () => {
        const ctx = await page.evaluate(() => {
          const d = (window as unknown as Record<string, unknown>).__uiLabDebug as {
            getSandboxContext: () => { ready: boolean; epoch: string | null }
          }
          return d.getSandboxContext()
        })
        return ctx.ready && ctx.epoch !== before.context.epoch
      })
      .toBe(true)
    const after = await waitForDebug(page, (state) => state.revision === 1, 'revision preserved after reload')
    expect(after.revision).toBe(before.revision + 1)
    const afterContext = await page.evaluate(() => {
      const d = (window as unknown as Record<string, unknown>).__uiLabDebug as {
        getSandboxContext: () => { epoch: string | null }
        getSandboxLifecycle: () => Array<{ ready: boolean; epoch: string | null }>
      }
      return { context: d.getSandboxContext(), lifecycle: d.getSandboxLifecycle() }
    })
    expect(afterContext.context.epoch).not.toBe(before.context.epoch)
    const reloadLifecycle = afterContext.lifecycle.slice(before.lifecycleLength)
    expect(reloadLifecycle.some((entry) => entry.ready === false)).toBe(true)
    expect(reloadLifecycle.some((entry) => entry.ready === true && entry.epoch === afterContext.context.epoch)).toBe(true)

    const rejection = await page.evaluate((oldEpoch) => {
      const d = (window as unknown as Record<string, unknown>).__uiLabDebug as {
        getState: () => { revision: number; component: { id: string } }
        getSandboxContext: () => { sessionId: string }
        getDiagnostics: () => string[]
        injectSandboxMessage: (message: unknown) => void
      }
      const beforeRevision = d.getState().revision
      d.injectSandboxMessage({
        protocolVersion: '1.0', sessionId: d.getSandboxContext().sessionId, sequence: 999_999,
        frameEpoch: oldEpoch, revision: beforeRevision, type: 'SANDBOX_PATCH', componentId: d.getState().component.id,
        payload: { revision: beforeRevision, baseRevision: Math.max(0, beforeRevision - 1), path: 'component.props.label', value: 'stale frame must not apply' },
      })
      return { beforeRevision }
    }, before.context.epoch)
    await expect.poll(async () => page.evaluate(() => {
      const d = (window as unknown as Record<string, unknown>).__uiLabDebug as { getDiagnostics: () => string[] }
      return d.getDiagnostics().some((message) => message.includes('stale frame epoch'))
    })).toBe(true)
    const afterRejection = await page.evaluate(() => {
      const d = (window as unknown as Record<string, unknown>).__uiLabDebug as { getState: () => { revision: number; component: { props: { label: string } } } }
      return d.getState()
    })
    expect(afterRejection.revision).toBe(rejection.beforeRevision)
    expect(afterRejection.component.props.label).toBe('Temporary')

    // 6. Sandbox DOM 恢复为 Host 当前状态（label = Temporary）
    await expect(sandbox(page).locator('.talos-button__content')).toContainText('Temporary', { timeout: 20_000 })

    // 7. reload 后 Inspector patch 仍生效
    await page.evaluate(() => (window as unknown as { __uiLabDebug: { applyPatch: (path: string, value: unknown) => void } }).__uiLabDebug.applyPatch('component.props.label', 'After Reload'))
    await expect(sandbox(page).locator('.talos-button__content')).toContainText('After Reload', { timeout: 20_000 })

    // 8. reload 后 Motion command 正常（press-response 命中 specimen）
    await page.evaluate(() => (window as unknown as { __uiLabDebug: { applyPatch: (path: string, value: unknown) => void } }).__uiLabDebug.applyPatch('motion.activeMotionId', 'press-response'))
    await page.locator('.bottom-dock .p-tablist button', { hasText: '动画' }).click()
    await page.locator('.motion-actions button', { hasText: '播放' }).click()
    await page.locator('.bottom-dock .p-tablist button', { hasText: '诊断' }).click()
    await expect(page.locator('.bottom-dock .diagnostics li', { hasText: 'Motion:' }).first()).toBeVisible({ timeout: 10_000 })

    // 9. Restore 冻结 snapshot baseline
    const restored = await page.evaluate((value) => (window as unknown as { __uiLabDebug: { restoreSnapshot: (snapshot: unknown) => boolean } }).__uiLabDebug.restoreSnapshot(value), snapshot)
    expect(restored).toBe(true)
    const afterRestore = await waitForDebug(page, (state) => state.changes.length === 0 && state.revision > 2, 'restored baseline')
    expect(afterRestore.snapshotBaselineRevision).toBe(afterRestore.snapshotCurrentRevision)
    expect(afterRestore.diffChanges).toHaveLength(0)
  })

  test('design slots resolve to exactly one authoritative target and drive the real component', async ({ page }) => {
    await gotoLab(page)
    // 每个声明的 slot id 在 iframe 内恰好一个权威 target：
    // 组件带真实 anchor 的 slot → 恰好一个 [data-lab-slot]；
    // 组件无内部 anchor 的 slot → 恰好一个 [data-lab-slot-adapter]，且不再出现同名 data-lab-slot。
    const realSlots = ['leading-signal', 'primary-content']
    const adapterSlots = ['trailing-meta', 'focus-frame', 'state-badge']
    for (const id of realSlots) {
      await expect(sandbox(page).locator(`[data-lab-slot="${id}"]`)).toHaveCount(1)
      await expect(sandbox(page).locator(`[data-lab-slot-adapter="${id}"]`)).toHaveCount(0)
    }
    for (const id of adapterSlots) {
      await expect(sandbox(page).locator(`[data-lab-slot-adapter="${id}"]`)).toHaveCount(1)
      await expect(sandbox(page).locator(`[data-lab-slot="${id}"]`)).toHaveCount(0)
    }

    // primary-content 修改后显示在按钮内部
    await page.locator('.inspector .p-tablist button', { hasText: '插槽' }).click()
    const contentRow = page.locator('.inspector .slot-row').nth(1)
    await expect(contentRow).toBeVisible()
    await contentRow.locator('.p-inputtext').fill('Plain text slot')
    await expect(sandbox(page).locator('[data-lab-slot="primary-content"]')).toContainText('Plain text slot', { timeout: 10_000 })

    // enabled=false 作用于真实节点
    await contentRow.locator('.p-toggleswitch').click()
    await expect(sandbox(page).locator('[data-lab-slot="primary-content"]')).toHaveAttribute('data-lab-slot-enabled', 'false')
    await expect(sandbox(page).locator('[data-lab-slot="primary-content"]')).not.toContainText('Plain text slot')

    // 重新启用并确认恢复显示
    await contentRow.locator('.p-toggleswitch').click()
    await expect(sandbox(page).locator('[data-lab-slot="primary-content"]')).toContainText('Plain text slot', { timeout: 10_000 })
  })

  test('snapshot restore rewrites host state, sandbox DOM, baseline and clears the diff', async ({ page }) => {
    await gotoLab(page)
    // 修改多个属性
    await page.evaluate(() => {
      const d = (window as unknown as Record<string, unknown>).__uiLabDebug as { applyPatch: (path: string, value: unknown) => void }
      d.applyPatch('component.props.variant', 'danger')
      d.applyPatch('component.props.label', 'Restore Target')
      d.applyPatch('environment.theme', 'light')
    })
    await waitForDebug(page, (state) => state.changes.length === 3, 'three changes')

    // 导出 snapshot
    const snapshot = await page.evaluate(() => (window as unknown as { __uiLabDebug: { getSnapshot: () => unknown } }).__uiLabDebug.getSnapshot())

    // 再修改一个不同 path（journal 按 path 合并，避免被误合）
    await page.evaluate(() => (window as unknown as { __uiLabDebug: { applyPatch: (path: string, value: unknown) => void } }).__uiLabDebug.applyPatch('motion.playbackRate', 2))
    await waitForDebug(page, (state) => state.changes.length === 4, 'fourth change')

    // Restore
    const restored = await page.evaluate((value) => (window as unknown as { __uiLabDebug: { restoreSnapshot: (snapshot: unknown) => boolean } }).__uiLabDebug.restoreSnapshot(value), snapshot)
    expect(restored).toBe(true)

    // Host state 恢复（label = Restore Target, variant = danger, theme = light），revision 递增
    const state = await waitForDebug(page, (s) => s.revision > 4, 'restored revision')
    expect(state.diffChanges).toHaveLength(0)
    expect(state.snapshotBaselineRevision).toBe(state.snapshotCurrentRevision)

    // Sandbox DOM 反映恢复状态
    await expect(sandbox(page).locator('.talos-button__content')).toContainText('Restore Target', { timeout: 20_000 })
    await expect(sandbox(page).locator('.talos-button--danger')).toBeVisible()

    // 新 baseline = restored current
    const baseline = await page.evaluate(() => (window as unknown as { __uiLabDebug: { getBaseline: () => { component: { props: Record<string, unknown> } } } }).__uiLabDebug.getBaseline())
    expect(baseline.component.props.label).toBe('Restore Target')
    expect(baseline.component.props.variant).toBe('danger')
  })

  test('sandbox-originated status patch journals as sandbox source and stays in lockstep', async ({ page }) => {
    await gotoLab(page)
    await page.locator('.registry-pane button', { hasText: 'TALOS Status Indicator' }).first().click()
    await waitForDebug(page, (s) => s.componentId === 'talos.ui.status-indicator', 'status indicator')

    const before = await labDebug(page)
    // 点击 sandbox 内的真实组件 → cycleStatus → SANDBOX_PATCH
    await sandbox(page).locator('.talos-status').first().click()

    // Host revision +1，journal source = sandbox，path = component.props.status
    const after = await waitForDebug(page, (s) => s.revision === before.revision + 1, 'host revision +1')
    const sandboxEntry = after.changes.find((c) => c.path === 'component.props.status' && c.source === 'sandbox')
    expect(sandboxEntry).toBeTruthy()

    // Snapshot / Diff / Prompt 使用同一 current revision
    expect(after.snapshotCurrentRevision).toBe(after.diffCurrentRevision)
    expect(after.snapshotCurrentRevision).toBe(after.revision)
    expect(after.prompt).toContain(`Current revision: ${after.revision}`)

    // Sandbox DOM 反映新 status（active → pending），且与 host 状态一致
    await expect(sandbox(page).locator('.talos-status--pending')).toBeVisible({ timeout: 10_000 })
    const state = await page.evaluate(() => (window as unknown as { __uiLabDebug: { getState: () => { component: { props: Record<string, unknown> } } } }).__uiLabDebug.getState())
    expect(state.component.props.status).toBe('pending')
  })
})
