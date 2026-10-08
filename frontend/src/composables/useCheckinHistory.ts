import { ref, onMounted } from 'vue'
import { shanghaiBusinessDate } from '@/utils/businessDate'

export interface CheckinRecord {
  serialNo: string
  ok: boolean
  action: string
  message: string
  time: string
  date?: string  // YYYY-MM-DD in Asia/Shanghai, for daily grouping
  beforeStatus?: string
  afterStatus?: string
}

const DB_NAME = 'talos-checkin'
const STORE_NAME = 'history'
const DB_VERSION = 1
const MAX_RECORDS = 200

function openDB(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, DB_VERSION)
    req.onupgradeneeded = () => {
      const db = req.result
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        const store = db.createObjectStore(STORE_NAME, { keyPath: 'id', autoIncrement: true })
        store.createIndex('time', 'time', { unique: false })
      }
    }
    req.onsuccess = () => resolve(req.result)
    req.onerror = () => reject(req.error)
  })
}

async function loadAll(): Promise<CheckinRecord[]> {
  try {
    const db = await openDB()
    return new Promise((resolve, reject) => {
      const tx = db.transaction(STORE_NAME, 'readonly')
      const store = tx.objectStore(STORE_NAME)
      const req = store.getAll()
      req.onsuccess = () => {
        const records = (req.result as CheckinRecord[]).reverse()
        resolve(records.slice(0, MAX_RECORDS))
      }
      req.onerror = () => reject(req.error)
      tx.oncomplete = () => db.close()
    })
  } catch {
    return []
  }
}

async function saveBatch(records: CheckinRecord[]): Promise<void> {
  try {
    const db = await openDB()
    return new Promise((resolve, reject) => {
      const tx = db.transaction(STORE_NAME, 'readwrite')
      const store = tx.objectStore(STORE_NAME)
      for (const r of records) {
        store.add(r)
      }
      tx.oncomplete = () => { db.close(); resolve() }
      tx.onerror = () => reject(tx.error)
    })
  } catch {
    // IndexedDB unavailable — silently degrade
  }
}

async function clearAll(): Promise<void> {
  try {
    const db = await openDB()
    return new Promise((resolve, reject) => {
      const tx = db.transaction(STORE_NAME, 'readwrite')
      const store = tx.objectStore(STORE_NAME)
      store.clear()
      tx.oncomplete = () => { db.close(); resolve() }
      tx.onerror = () => reject(tx.error)
    })
  } catch {
    // silently degrade
  }
}

function exportCSV(records: CheckinRecord[]): void {
  const header = '时间,序列号,操作,结果,状态前,状态后'
  const rows = records.map(r =>
    `${r.time},${r.serialNo},${r.action},${r.ok ? '成功' : '失败'},${r.beforeStatus || ''},${r.afterStatus || ''}`
  )
  const csv = '﻿' + [header, ...rows].join('\n')
  const blob = new Blob([csv], { type: 'text/csv;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `checkin-${shanghaiBusinessDate()}.csv`
  a.click()
  URL.revokeObjectURL(url)
}

export function useCheckinHistory() {
  const records = ref<CheckinRecord[]>([])

  onMounted(async () => {
    records.value = await loadAll()
  })

  async function addRecord(r: CheckinRecord) {
    records.value.unshift(r)
    if (records.value.length > MAX_RECORDS) {
      records.value = records.value.slice(0, MAX_RECORDS)
    }
    await saveBatch([r])
  }

  async function clearRecords() {
    records.value = []
    await clearAll()
  }

  function downloadCSV() {
    exportCSV(records.value)
  }

  return { records, addRecord, clearRecords, downloadCSV }
}
