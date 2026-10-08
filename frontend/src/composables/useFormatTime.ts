/**
 * 将 ISO 8601 时间字符串格式化为本地显示格式
 * @param iso ISO 时间字符串
 * @param withSeconds 是否包含秒（默认 false）
 * @returns 格式化后的时间字符串，如 "2025-01-15 09:30" 或 "2025-01-15 09:30:45"
 */
export function formatTime(iso: string, withSeconds = false): string {
  if (!iso) return '-'
  const d = new Date(iso)
  if (isNaN(d.getTime())) return '-'
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  const h = String(d.getHours()).padStart(2, '0')
  const mi = String(d.getMinutes()).padStart(2, '0')
  if (!withSeconds) return `${y}-${m}-${day} ${h}:${mi}`
  const s = String(d.getSeconds()).padStart(2, '0')
  return `${y}-${m}-${day} ${h}:${mi}:${s}`
}
