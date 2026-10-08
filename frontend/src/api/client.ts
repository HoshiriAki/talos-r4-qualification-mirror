// API client — fetch wrapper for Talos backend
// Cookie-based auth (talos_session), auto-redirect on 401

import type { Router } from 'vue-router'

let _router: Router | null = null

interface PreviewTransportContext {
  sessionId: string
  onInvalid: () => void
}

let _preview: PreviewTransportContext | null = null

export function setPreviewTransportContext(context: PreviewTransportContext | null) {
  _preview = context
}

const previewReadMappings: Array<[prefix: string, target: string]> = [
  ['/api/dashboard/stats', 'dashboard/summary'],
  ['/users', 'orders'],
  ['/devices', 'devices'],
  ['/audit-logs', 'audit-logs'],
]

function isPreviewControlPlane(url: string): boolean {
  return url.startsWith('/api/tenant-preview/sessions') || url === '/auth/me' || url === '/auth/logout'
}

function preparePreviewRequest(url: string, options: RequestInit): { url: string; options: RequestInit } {
  if (!_preview || isPreviewControlPlane(url)) return { url, options }

  const method = (options.method || 'GET').toUpperCase()
  if (method !== 'GET' && method !== 'HEAD') {
    throw new Error('租户预览为只读模式，已阻止写操作')
  }

  const mapping = previewReadMappings.find(([prefix]) => url === prefix || url.startsWith(`${prefix}/`) || url.startsWith(`${prefix}?`))
  if (!mapping) throw new Error('当前读取不在租户预览允许范围内')

  const [prefix, target] = mapping
  const suffix = url.slice(prefix.length)
  return {
    url: `/api/tenant-preview/sessions/${encodeURIComponent(_preview.sessionId)}/${target}${suffix}`,
    options,
  }
}

function handleInvalidPreview(res: Response, data: any, requestUrl = '') {
  const code = data?.code || data?.errorCode || (typeof data?.error === 'string' ? data.error.split(':')[0] : '')
  if (_preview && (code === 'PREVIEW_SESSION_NOT_FOUND' || code === 'PREVIEW_SESSION_EXPIRED' || code === 'PREVIEW_SESSION_INACTIVE' || code === 'PREVIEW_TENANT_INACTIVE')) {
    _preview.onInvalid()
  }
}

export function setRouter(router: Router) {
  _router = router
}

async function parseJsonSafe(res: Response): Promise<any> {
  try {
    return await res.json()
  } catch {
    return null
  }
}

function resolveErrorMessage(data: any, fallbackMessage?: string): string {
  if (data && typeof data === 'object') {
    if (typeof data.error === 'string' && data.error.trim()) return data.error
    if (typeof data.message === 'string' && data.message.trim()) return data.message
    if (Array.isArray(data.errors) && data.errors.length > 0) {
      return data.errors.map((e: any) => e.message || '').filter(Boolean).join('; ') || fallbackMessage || '请求失败'
    }
  }
  return fallbackMessage || '请求失败'
}

function authorityHeaders(url: string, source?: HeadersInit): Headers {
  const headers = new Headers(source)
  if (
    url.startsWith('/api/platform/')
    || url.startsWith('/api/tenants')
    || url.startsWith('/api/tenant-governance')
    || url.startsWith('/api/tenant-preview')
    || url.startsWith('/api/tenant-workspaces')
    || url.startsWith('/api/tenant-simulation')
  ) {
    headers.set('X-Talos-Authority', 'platform')
  }
  return headers
}

export async function requestJson(
  url: string,
  options?: RequestInit | string,
  fallbackMessage?: string
): Promise<any> {
  let requestOptions: RequestInit = {}
  let message: string | undefined

  if (typeof options === 'string') {
    message = options
  } else if (options) {
    requestOptions = options
    message = fallbackMessage
  } else if (fallbackMessage) {
    message = fallbackMessage
  }

  const headers = authorityHeaders(url, requestOptions.headers)

  // Only set Content-Type for requests with a JSON body
  if (requestOptions.body && !(requestOptions.body instanceof FormData)) {
    headers.set('Content-Type', 'application/json')
  }

  const mergedOptions: RequestInit = {
    ...requestOptions,
    credentials: 'same-origin',
    headers,
  }

  const prepared = preparePreviewRequest(url, mergedOptions)
  const res = await fetch(prepared.url, prepared.options)
  const data = await parseJsonSafe(res)
  handleInvalidPreview(res, data, prepared.url)

  if (res.status === 401) {
    if (_preview) _preview.onInvalid()
    if (!(mergedOptions as any).__retry) {
      const refreshRes = await fetch('/auth/me', { credentials: 'same-origin' })
      if (refreshRes.ok) {
        return requestJson(url, { ...mergedOptions, __retry: true } as any, message || fallbackMessage)
      }
    }
    if (_router && _router.currentRoute.value.path !== '/login') {
      _router.push(_router.currentRoute.value.path.startsWith('/control') ? '/control/login' : '/login')
    }
    throw new Error(resolveErrorMessage(data, message))
  }

  if (!res.ok) {
    const error = new Error(resolveErrorMessage(data, message))
    ;(error as any).status = res.status
    ;(error as any).code = data?.code || data?.errorCode
    throw error
  }

  return data || {}
}

export function request(url: string, options?: RequestInit): Promise<Response> {
  const prepared = preparePreviewRequest(url, {
    ...options,
    credentials: 'same-origin',
    headers: authorityHeaders(url, options?.headers),
  })
  return fetch(prepared.url, prepared.options)
}

export async function requestBlob(
  url: string,
  options?: RequestInit,
  fallbackMessage?: string
): Promise<Blob> {
  const headers = authorityHeaders(url, options?.headers)
  if (options?.body && !(options.body instanceof FormData)) {
    headers.set('Content-Type', 'application/json')
  }
  const prepared = preparePreviewRequest(url, {
    ...options,
    credentials: 'same-origin',
    headers,
  })
  const res = await fetch(prepared.url, prepared.options)
  if (res.status === 401) {
    if (_router && _router.currentRoute.value.path !== '/login') {
      _router.push(_router.currentRoute.value.path.startsWith('/control') ? '/control/login' : '/login')
    }
    throw new Error(fallbackMessage || '登录已过期')
  }
  if (!res.ok) {
    const data = await parseJsonSafe(res)
    handleInvalidPreview(res, data, prepared.url)
    throw new Error(fallbackMessage || '请求失败')
  }
  return res.blob()
}
