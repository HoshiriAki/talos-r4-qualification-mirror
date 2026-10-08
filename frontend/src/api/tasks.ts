import { requestJson } from './client'

export type TaskRisk = 'low' | 'medium' | 'high'
export type TaskStatus = 'queued' | 'in_progress' | 'completed' | 'cancelled'
export type TaskCapability = 'acknowledge' | 'confirm' | 'scan' | 'defer' | 'open_work' | 'send_to_pc'

export interface WorkTask {
  id: string
  kind: string
  title: string
  summary: string
  risk: TaskRisk
  status: TaskStatus
  sourceType: 'order' | 'device' | 'warehouse' | 'system'
  sourceId: string
  reason: string
  dueAt: string | null
  capabilities: TaskCapability[]
  workRoute: { name: string; query: Record<string, string> }
  version: number
}

export interface TaskQuery { status?: string; limit?: number }

export async function fetchTasks(query: TaskQuery = {}): Promise<WorkTask[]> {
  const params = new URLSearchParams()
  if (query.status) params.set('status', query.status)
  if (query.limit) params.set('limit', String(query.limit))
  const result: any = await requestJson(`/api/tasks?${params}`)
  return result.tasks ?? []
}

export interface TaskActionResult {
  status: string
  newStatus: string
  task: { id: string; version: number }
}

export async function taskAction(taskId: string, action: string, expectedVersion: number): Promise<TaskActionResult> {
  return requestJson(`/api/tasks/${taskId}/actions`, {
    method: 'POST',
    body: JSON.stringify({ action, expectedVersion }),
  }) as Promise<TaskActionResult>
}

export async function sendTaskToPc(taskId: string, expectedVersion: number): Promise<TaskActionResult> {
  return requestJson(`/api/tasks/${taskId}/send-to-pc`, {
    method: 'POST',
    body: JSON.stringify({ expectedVersion }),
  }) as Promise<TaskActionResult>
}
