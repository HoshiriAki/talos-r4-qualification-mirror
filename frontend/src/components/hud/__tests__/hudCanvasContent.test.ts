// @ts-nocheck -- Executed directly by Node 22's type-strip test runner.
import test from 'node:test'
import assert from 'node:assert/strict'
import { buildHudCanvasContent } from '../hudCanvasContent.ts'

const stats = {
  version: 1,
  asOf: '2026-07-15T09:30:00+08:00',
  activeOrders: 48,
  devicesOut: 90,
  availableDevices: 30,
  totalDevices: 120,
  returnsDueToday: 5,
  overdueReturns: 3,
  todayNewOrders: 7,
  recentOrders: [],
  overdueDetails: [],
  orderBuckets: [],
  sceneNodes: [],
}

test('uses live dashboard metrics without inventing monthly targets', () => {
  const content = buildHudCanvasContent(stats)

  assert.equal(content.utilizationPercent, 75)
  assert.equal(content.progressTitle, '当前在外设备')
  assert.equal(content.progressValue, 90)
  assert.equal(content.progressTotal, 120)
  assert.equal(content.progressCaption, '可用 30 台 · 今日待还 5 台')
  assert.equal('monthlyTarget' in content, false)
})

test('promotes overdue work to the hero state', () => {
  const content = buildHudCanvasContent(stats)

  assert.equal(content.heroLabel, '告警')
  assert.equal(content.heroSubLabel, '3 项逾期任务待处理')
})

test('falls back through due returns, dispatch and standby states', () => {
  assert.equal(buildHudCanvasContent({ ...stats, overdueReturns: 0 }).heroLabel, '归还')
  assert.equal(buildHudCanvasContent({ ...stats, overdueReturns: 0, returnsDueToday: 0 }).heroLabel, '调度')
  assert.equal(buildHudCanvasContent({
    ...stats,
    overdueReturns: 0,
    returnsDueToday: 0,
    activeOrders: 0,
  }).heroLabel, '待命')
})

test('returns a stable zero state while dashboard data loads', () => {
  const content = buildHudCanvasContent(null)

  assert.equal(content.utilizationPercent, 0)
  assert.equal(content.heroLabel, '同步')
  assert.equal(content.progressCaption, '正在同步设备与订单状态')
})
