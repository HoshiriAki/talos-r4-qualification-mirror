import { requestJson } from './client'

export const overdueApi = {
  detect: (body?: any) => requestJson('/api/overdue/detect', { method: 'POST', body: body ? JSON.stringify(body) : undefined }, '逾期检测失败'),
  calc: (body: any) => requestJson('/api/overdue/calc', { method: 'POST', body: JSON.stringify(body) }, '费用计算失败'),
  apply: (body: any) => requestJson('/api/overdue/apply', { method: 'POST', body: JSON.stringify(body) }, '费用应用失败'),
  waive: (body: any) => requestJson('/api/overdue/waive', { method: 'POST', body: JSON.stringify(body) }, '豁免失败'),
  list: (params?: Record<string, any>) => requestJson(`/api/overdue/list${params ? '?' + new URLSearchParams(params) : ''}`, '获取逾期列表失败'),
  get: (params?: Record<string, any>) => requestJson(`/api/overdue/get${params ? '?' + new URLSearchParams(params) : ''}`, '获取逾期详情失败'),
  config: () => requestJson('/api/overdue/config', '获取配置失败'),
  configUpsert: (body: any) => requestJson('/api/overdue/config', { method: 'PUT', body: JSON.stringify(body) }, '保存配置失败'),
  escalate: (body?: any) => requestJson('/api/overdue/escalate', { method: 'POST', body: body ? JSON.stringify(body) : undefined }, '升级通知失败'),
  escalationHistory: (params?: Record<string, any>) => requestJson(`/api/overdue/escalation-history${params ? '?' + new URLSearchParams(params) : ''}`, '获取升级历史失败'),
  stats: () => requestJson('/api/overdue/stats', '获取统计失败'),
  checkBeforeOrder: (body: any) => requestJson('/api/overdue/check-order', { method: 'POST', body: JSON.stringify(body) }, '逾期检查失败'),
}
