/** 通用分页查询 */
export interface DataQuery<TFilter = Record<string, unknown>> {
  mode: 'page' | 'offset'
  page?: number
  pageSize?: number
  offset?: number
  limit?: number
  filter?: TFilter
  sortBy?: string
  sortOrder?: 'asc' | 'desc'
}

/** 通用分页响应 */
export interface DataPage<T> {
  rows: T[]
  total?: number
  totalPages?: number
  hasMore?: boolean
}

/** 筛选匹配模式 */
export type FilterMatch = 'exact' | 'contains' | 'gte' | 'lte' | 'between'

/** 筛选规则定义 */
export interface FilterRule {
  key: string
  column: string | string[]
  match: FilterMatch
  /** between 模式的配对字段 key */
  pairKey?: string
}

/** 列定义 */
export interface ColumnDef {
  type: 'text' | 'number' | 'date'
  table?: string
  column?: string
  sql?: string
}

/** 分页信息 */
export interface PageInfo {
  page: number
  pageSize: number
  total: number
  totalPages: number
}
