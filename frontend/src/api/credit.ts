import { requestJson } from './client'

export const creditApi = {
  // Blacklist
  blacklistAdd: (body: any) => requestJson('/api/credit/blacklist/add', { method: 'POST', body: JSON.stringify(body) }, '添加黑名单失败'),
  blacklistRemove: (body: any) => requestJson('/api/credit/blacklist/remove', { method: 'POST', body: JSON.stringify(body) }, '移除黑名单失败'),
  blacklistCheck: (body: any) => requestJson('/api/credit/blacklist/check', { method: 'POST', body: JSON.stringify(body) }, '检查黑名单失败'),
  blacklistList: (params?: Record<string, any>) => requestJson(`/api/credit/blacklist/list${params ? '?' + new URLSearchParams(params) : ''}`, '获取黑名单列表失败'),

  // Violations
  violationRecord: (body: any) => requestJson('/api/credit/violations/record', { method: 'POST', body: JSON.stringify(body) }, '记录违规失败'),
  violationAppeal: (body: any) => requestJson('/api/credit/violations/appeal', { method: 'POST', body: JSON.stringify(body) }, '提交申诉失败'),
  violationReview: (body: any) => requestJson('/api/credit/violations/review', { method: 'POST', body: JSON.stringify(body) }, '审核失败'),
  violationList: (params?: Record<string, any>) => requestJson(`/api/credit/violations/list${params ? '?' + new URLSearchParams(params) : ''}`, '获取违规列表失败'),
  violationGet: (params?: Record<string, any>) => requestJson(`/api/credit/violations/get${params ? '?' + new URLSearchParams(params) : ''}`, '获取违规详情失败'),

  // Credit Score
  creditGet: (params?: Record<string, any>) => requestJson(`/api/credit/score${params ? '?' + new URLSearchParams(params) : ''}`, '获取信用评分失败'),
  creditHistory: (params?: Record<string, any>) => requestJson(`/api/credit/score/history${params ? '?' + new URLSearchParams(params) : ''}`, '获取信用历史失败'),
  creditRecalculate: (body: any) => requestJson('/api/credit/score/recalculate', { method: 'POST', body: JSON.stringify(body) }, '重新计算失败'),

  // Pre-order check
  checkBeforeOrder: (body: any) => requestJson('/api/credit/check-order', { method: 'POST', body: JSON.stringify(body) }, '信用检查失败'),
}
