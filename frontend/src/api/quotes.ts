import { requestJson } from './client'

export interface Money {
  minor: number
  currency: string
}

export interface CustomerOption {
  id: string
  displayName: string
  legalName: string
  status: string
}

export interface QuoteLine {
  id: string
  kind: 'model' | 'accessory'
  referenceId: string
  description: string
  quantity: number
  unitPrice: Money
  subtotal: Money
  priceSnapshot: Record<string, unknown>
}

export interface Quote {
  id: string
  customerId: string
  status: 'draft' | 'confirmed' | 'expired' | 'cancelled' | 'converted'
  startDate: string
  endDate: string
  region: string
  total: Money
  expiresAt: string
  confirmedAt?: string | null
  convertedOrderId?: string | null
  lines: QuoteLine[]
}

export interface OrderFromQuote {
  orderId: string
  orderNo: string
  quoteId: string
  customerId: string
  total: Money
}

export interface CreateQuoteInput {
  customerId: string
  startDate: string
  endDate: string
  region: string
  modelLines: Array<{ modelId: string; quantity: number }>
  accessoryLines: Array<{ accessoryId: string; quantity: number }>
}

export function fetchCustomers(): Promise<CustomerOption[]> {
  return requestJson('/api/v2/customers', '获取客户列表失败')
}

export function createQuote(input: CreateQuoteInput): Promise<Quote> {
  return requestJson('/api/v2/quotes', {
    method: 'POST',
    body: JSON.stringify(input),
  }, '生成服务器报价失败')
}

export function confirmQuote(id: string): Promise<Quote> {
  return requestJson(`/api/v2/quotes/${encodeURIComponent(id)}/confirm`, {
    method: 'POST',
  }, '确认报价失败')
}

export function expireQuote(id: string): Promise<Quote> {
  return requestJson(`/api/v2/quotes/${encodeURIComponent(id)}/expire`, {
    method: 'POST',
  }, '报价过期处理失败')
}

export function createOrderFromQuote(id: string, remark = ''): Promise<OrderFromQuote> {
  return requestJson(`/api/v2/quotes/${encodeURIComponent(id)}/orders`, {
    method: 'POST',
    body: JSON.stringify({ remark }),
  }, '从报价创建订单失败')
}
