import { requestBlob, requestJson } from './client'

export type SimulationStatus = 'provisioning' | 'active' | 'discarding' | 'discarded' | 'expired' | 'failed'
export type SimulationDiffStatus = 'clean' | 'stale' | 'conflict'

export interface TenantSimulation {
  id: string
  tenantId: string
  tenantName: string
  tenantSlug: string
  scenarioName: string
  changeIntent: string
  status: SimulationStatus
  baseRevision: number
  generation: number
  createdAt: string
  expiresAt: string
  failureCode?: string
}

export interface CreateTenantSimulationInput {
  tenantId: string
  scenarioName: string
  changeIntent: string
  ttlMinutes: number
  plannedAbsentKeys: string[]
  idempotencyKey: string
}

export interface ReferenceConfig {
  key: string
  value: unknown
  deleted?: boolean
  updatedAt?: string
}

export interface SimulationDiffItem {
  resourceKind: string
  resourceKey: string
  status: SimulationDiffStatus
  base?: unknown
  simulation?: unknown
  production?: unknown
}

export interface SimulationDiffPage {
  evaluationId: string
  items: SimulationDiffItem[]
  nextCursor?: string | null
}

export interface SimulationEffect {
  id: string
  command: string
  kind: string
  note?: string
  createdAt: string
}

export interface SimulationEffectsPage {
  items: SimulationEffect[]
  nextCursor?: string | null
}

function sessionFrom(value: any): TenantSimulation {
  const session = value?.session ?? value
  return {
    id: session.id ?? session.sessionId,
    tenantId: session.tenantId ?? session.tenant_id,
    tenantName: session.tenantName ?? session.tenant_name ?? '',
    tenantSlug: session.tenantSlug ?? session.tenant_slug ?? '',
    scenarioName: session.scenarioName ?? session.scenario_name ?? '',
    changeIntent: session.changeIntent ?? session.change_intent ?? '',
    status: session.status,
    baseRevision: Number(session.baseRevision ?? session.base_revision ?? 0),
    generation: Number(session.generation ?? 0),
    createdAt: session.createdAt ?? session.created_at ?? '',
    expiresAt: session.expiresAt ?? session.expires_at ?? '',
    failureCode: session.failureCode ?? session.failure_code,
  }
}

function pageFrom<T>(value: any, field = 'items'): { items: T[]; nextCursor?: string | null } {
  return { items: Array.isArray(value?.[field]) ? value[field] : [], nextCursor: value?.nextCursor ?? value?.next_cursor ?? null }
}

function diffPageFrom(value: any, fallbackEvaluationId = ''): SimulationDiffPage {
  const page = pageFrom<any>(value)
  return {
    evaluationId: value?.evaluationId ?? value?.evaluation_id ?? fallbackEvaluationId,
    items: page.items.map(item => ({
      ...item,
      status: item.status ?? item.conflictStatus ?? item.conflict_status,
      simulation: item.simulation ?? item.after,
      production: item.production ?? item.current,
      base: item.base ?? item.before,
    })) as SimulationDiffItem[],
    nextCursor: page.nextCursor,
  }
}

function mutationOptions(body: unknown, idempotencyKey: string): RequestInit {
  return { method: 'POST', headers: { 'Idempotency-Key': idempotencyKey }, body: JSON.stringify(body) }
}

const basePath = '/api/tenant-simulations'

export const tenantSimulationApi = {
  async create(input: CreateTenantSimulationInput): Promise<TenantSimulation> {
    return sessionFrom(await requestJson(basePath, mutationOptions(input, input.idempotencyKey), '创建模拟会话失败'))
  },
  async get(id: string): Promise<TenantSimulation> {
    return sessionFrom(await requestJson(`${basePath}/${encodeURIComponent(id)}`, '读取模拟会话失败'))
  },
  async discard(id: string): Promise<void> {
    await requestJson(`${basePath}/${encodeURIComponent(id)}`, { method: 'DELETE' }, '结束模拟会话失败')
  },
  async listReferenceConfigs(id: string): Promise<ReferenceConfig[]> {
    const value = await requestJson(`${basePath}/${encodeURIComponent(id)}/reference-configs`, '读取模拟配置失败')
    return Array.isArray(value?.items) ? value.items : (Array.isArray(value?.configs) ? value.configs : [])
  },
  async putReferenceConfig(id: string, key: string, value: unknown, idempotencyKey: string): Promise<ReferenceConfig> {
    const result = await requestJson(`${basePath}/${encodeURIComponent(id)}/reference-configs/${encodeURIComponent(key)}`, {
      method: 'PUT', headers: { 'Idempotency-Key': idempotencyKey }, body: JSON.stringify({ value }),
    }, '写入模拟配置失败')
    return result?.item ?? result
  },
  async createReferenceConfig(id: string, key: string, value: unknown, idempotencyKey: string): Promise<ReferenceConfig> {
    const result = await requestJson(`${basePath}/${encodeURIComponent(id)}/reference-configs`, mutationOptions({ key, value }, idempotencyKey), '创建模拟配置失败')
    return result?.item ?? result
  },
  async deleteReferenceConfig(id: string, key: string, idempotencyKey: string): Promise<void> {
    await requestJson(`${basePath}/${encodeURIComponent(id)}/reference-configs/${encodeURIComponent(key)}`, {
      method: 'DELETE', headers: { 'Idempotency-Key': idempotencyKey },
    }, '删除模拟配置失败')
  },
  async recordEffect(id: string, key: string, value: unknown, note: string, idempotencyKey: string): Promise<void> {
    await requestJson(`${basePath}/${encodeURIComponent(id)}/commands/record-effect`, mutationOptions({ key, value, effect: { kind: 'reference', note } }, idempotencyKey), '记录模拟效果失败')
  },
  async diff(id: string, cursor?: string): Promise<SimulationDiffPage> {
    const suffix = cursor ? `?cursor=${encodeURIComponent(cursor)}` : ''
    const value = await requestJson(`${basePath}/${encodeURIComponent(id)}/diff${suffix}`, '读取模拟差异失败')
    return diffPageFrom(value)
  },
  async diffPage(id: string, evaluationId: string, cursor: string): Promise<SimulationDiffPage> {
    const value = await requestJson(
      `${basePath}/${encodeURIComponent(id)}/diff/${encodeURIComponent(evaluationId)}?cursor=${encodeURIComponent(cursor)}`,
      '读取冻结模拟差异失败',
    )
    return diffPageFrom(value, evaluationId)
  },
  async effects(id: string, cursor?: string): Promise<SimulationEffectsPage> {
    const suffix = cursor ? `?cursor=${encodeURIComponent(cursor)}` : ''
    return pageFrom<SimulationEffect>(await requestJson(`${basePath}/${encodeURIComponent(id)}/effects${suffix}`, '读取模拟效果失败'))
  },
  async exportDiff(id: string, evaluationId: string): Promise<Blob> {
    return requestBlob(`${basePath}/${encodeURIComponent(id)}/diff/${encodeURIComponent(evaluationId)}/export`, undefined, '导出差异失败')
  },
}
