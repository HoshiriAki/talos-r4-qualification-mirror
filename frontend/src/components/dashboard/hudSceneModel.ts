import type { HudSceneNode } from '@/api/dashboard'

export interface HudSceneModule extends Omit<HudSceneNode, 'count'> {
  count: number
  position: { x: number; y: number }
  width: number
  depth: number
  height: number
}

export interface HudWorkflowStep {
  id: string
  label: string
  state: 'reference'
}

const PRIMARY_POSITIONS = [
  { x: 390, y: 210 },
  { x: 540, y: 272 },
  { x: 690, y: 210 },
  { x: 315, y: 310 },
  { x: 465, y: 372 },
  { x: 615, y: 330 },
] as const

const WORKFLOWS: Record<string, string[]> = {
  'return-risk': ['识别', '联系', '回收', '验收'],
  'device-cluster': ['分配', '出库', '使用', '归还'],
}

function positionFor(index: number): { x: number; y: number } {
  if (index < PRIMARY_POSITIONS.length) return { ...PRIMARY_POSITIONS[index] }
  const overflowIndex = index - PRIMARY_POSITIONS.length
  return {
    x: 315 + (overflowIndex % 3) * 150,
    y: 440 + Math.floor(overflowIndex / 3) * 88,
  }
}

export function buildHudScene(nodes: HudSceneNode[]): HudSceneModule[] {
  return nodes.map((node, index) => {
    const count = Math.max(0, Number.isFinite(node.count) ? node.count : 0)
    return {
      ...node,
      count,
      position: positionFor(index),
      width: 124,
      depth: 62,
      height: Math.max(24, Math.min(92, 28 + Math.sqrt(count) * 7)),
    }
  })
}

export function buildWorkflowSteps(node: HudSceneNode): HudWorkflowStep[] {
  const labels = WORKFLOWS[node.kind] ?? ['接收', '判断', '执行', '完成']
  return labels.map((label, index) => ({
    id: `${node.id}-${index}`,
    label,
    state: 'reference',
  }))
}
