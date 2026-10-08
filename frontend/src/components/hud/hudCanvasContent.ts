import type { DashboardStats } from '@/api/dashboard'

export interface HudCanvasContent {
  utilizationPercent: number
  heroLabel: '同步' | '告警' | '归还' | '调度' | '待命'
  heroSubLabel: string
  progressTitle: string
  progressValue: number
  progressTotal: number
  progressCaption: string
  syncLabel: string
  riskLabel: string
}

function clampPercent(value: number) {
  return Math.max(0, Math.min(100, Math.round(value)))
}

function formatAsOf(asOf: string) {
  if (!asOf) return '等待数据'
  return `${asOf.replace('T', ' ').slice(0, 16)} · UTC+8`
}

export function buildHudCanvasContent(model: DashboardStats | null): HudCanvasContent {
  if (!model) {
    return {
      utilizationPercent: 0,
      heroLabel: '同步',
      heroSubLabel: '正在获取作业状态',
      progressTitle: '当前在外设备',
      progressValue: 0,
      progressTotal: 0,
      progressCaption: '正在同步设备与订单状态',
      syncLabel: '等待数据',
      riskLabel: '风险状态尚未载入',
    }
  }

  const utilizationPercent = model.totalDevices > 0
    ? clampPercent((model.devicesOut / model.totalDevices) * 100)
    : 0

  let heroLabel: HudCanvasContent['heroLabel'] = '待命'
  let heroSubLabel = '当前没有紧急作业'

  if (model.overdueReturns > 0) {
    heroLabel = '告警'
    heroSubLabel = `${model.overdueReturns} 项逾期任务待处理`
  } else if (model.returnsDueToday > 0) {
    heroLabel = '归还'
    heroSubLabel = `今日 ${model.returnsDueToday} 台设备待归还`
  } else if (model.activeOrders > 0) {
    heroLabel = '调度'
    heroSubLabel = `${model.activeOrders} 个活跃订单执行中`
  }

  return {
    utilizationPercent,
    heroLabel,
    heroSubLabel,
    progressTitle: '当前在外设备',
    progressValue: model.devicesOut,
    progressTotal: model.totalDevices,
    progressCaption: `可用 ${model.availableDevices} 台 · 今日待还 ${model.returnsDueToday} 台`,
    syncLabel: formatAsOf(model.asOf),
    riskLabel: `逾期 ${model.overdueReturns} · 今日待还 ${model.returnsDueToday}`,
  }
}
