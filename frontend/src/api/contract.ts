import { requestJson } from './client'

export const contractApi = {
  // Templates
  templateList: (params?: Record<string, any>) =>
    requestJson(`/api/contract/templates${params ? '?' + new URLSearchParams(params) : ''}`, '获取模板列表失败'),
  templateGet: (params?: Record<string, any>) =>
    requestJson(`/api/contract/templates/get${params ? '?' + new URLSearchParams(params) : ''}`, '获取模板详情失败'),
  templateCreate: (body: any) =>
    requestJson('/api/contract/templates/create', { method: 'POST', body: JSON.stringify(body) }, '创建模板失败'),
  templateUpdate: (body: any) =>
    requestJson('/api/contract/templates/update', { method: 'PUT', body: JSON.stringify(body) }, '更新模板失败'),
  templateDelete: (body: any) =>
    requestJson('/api/contract/templates/delete', { method: 'POST', body: JSON.stringify(body) }, '删除模板失败'),
  templateRender: (body: any) =>
    requestJson('/api/contract/templates/render', { method: 'POST', body: JSON.stringify(body) }, '渲染模板失败'),

  // Contracts
  contractList: (params?: Record<string, any>) =>
    requestJson(`/api/contract/contracts${params ? '?' + new URLSearchParams(params) : ''}`, '获取合同列表失败'),
  contractGet: (params?: Record<string, any>) =>
    requestJson(`/api/contract/contracts/get${params ? '?' + new URLSearchParams(params) : ''}`, '获取合同详情失败'),
  contractGenerate: (body: any) =>
    requestJson('/api/contract/contracts/generate', { method: 'POST', body: JSON.stringify(body) }, '生成合同失败'),
  contractVoid: (body: any) =>
    requestJson('/api/contract/contracts/void', { method: 'POST', body: JSON.stringify(body) }, '作废合同失败'),

  // Signing
  signRequest: (body: any) =>
    requestJson('/api/contract/sign/request', { method: 'POST', body: JSON.stringify(body) }, '发起签署失败'),
  signVerify: (body: any) =>
    requestJson('/api/contract/sign/verify', { method: 'POST', body: JSON.stringify(body) }, '验证签名失败'),
  signStatus: (params?: Record<string, any>) =>
    requestJson(`/api/contract/sign/status${params ? '?' + new URLSearchParams(params) : ''}`, '获取签署状态失败'),

  // Pre-order auto-trigger
  checkHighValue: (body: any) =>
    requestJson('/api/contract/check-high-value', { method: 'POST', body: JSON.stringify(body) }, '高价值检查失败'),
}
