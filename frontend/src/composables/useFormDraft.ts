import { ref, watch, onMounted, onUnmounted, type Ref } from 'vue'
import { onBeforeRouteLeave } from 'vue-router'

const STORAGE_PREFIX = 'draft:'
const DEFAULT_EXPIRY_MS = 24 * 60 * 60 * 1000
const DEFAULT_AUTO_SAVE_MS = 2000

interface DraftWrapper<T> {
  savedAt: number
  data: T
}

export interface UseFormDraftOptions {
  key: string
  expiryMs?: number
  autoSaveIntervalMs?: number
}

export function useFormDraft<T extends Record<string, any>>(
  state: Ref<T>,
  options: UseFormDraftOptions,
) {
  const storageKey = `${STORAGE_PREFIX}${options.key}`
  const expiryMs = options.expiryMs ?? DEFAULT_EXPIRY_MS
  const autoSaveIntervalMs = options.autoSaveIntervalMs ?? DEFAULT_AUTO_SAVE_MS

  const hasDraft = ref(false)
  const lastSavedAt = ref<number | null>(null)

  let autoSaveTimer: ReturnType<typeof setTimeout> | null = null
  let dirty = false

  function saveDraft() {
    try {
      const wrapper: DraftWrapper<T> = {
        savedAt: Date.now(),
        data: state.value,
      }
      localStorage.setItem(storageKey, JSON.stringify(wrapper))
      lastSavedAt.value = wrapper.savedAt
      hasDraft.value = true
    } catch {
      // localStorage full or unavailable — silently ignore
    }
  }

  function loadDraft(): T | null {
    try {
      const raw = localStorage.getItem(storageKey)
      if (!raw) return null
      const wrapper: DraftWrapper<T> = JSON.parse(raw)
      if (Date.now() - wrapper.savedAt > expiryMs) {
        localStorage.removeItem(storageKey)
        hasDraft.value = false
        return null
      }
      lastSavedAt.value = wrapper.savedAt
      hasDraft.value = true
      return wrapper.data
    } catch {
      return null
    }
  }

  function clearDraft() {
    try {
      localStorage.removeItem(storageKey)
    } catch {
      // ignore
    }
    hasDraft.value = false
    lastSavedAt.value = null
  }

  // Deep watch with debounced auto-save
  const stopWatch = watch(
    state,
    () => {
      dirty = true
      if (autoSaveTimer) clearTimeout(autoSaveTimer)
      autoSaveTimer = setTimeout(() => {
        autoSaveTimer = null
        saveDraft()
      }, autoSaveIntervalMs)
    },
    { deep: true },
  )

  // Flush pending save on route leave — only if form was modified
  onBeforeRouteLeave(() => {
    if (autoSaveTimer) {
      clearTimeout(autoSaveTimer)
      autoSaveTimer = null
    }
    if (dirty) saveDraft()
  })

  // Save on tab close — only if form was modified
  function onBeforeUnload() {
    if (dirty) saveDraft()
  }

  onMounted(() => {
    window.addEventListener('beforeunload', onBeforeUnload)
    // Check if draft exists on mount
    const existing = loadDraft()
    if (!existing) {
      hasDraft.value = false
    }
  })

  onUnmounted(() => {
    window.removeEventListener('beforeunload', onBeforeUnload)
    if (autoSaveTimer) clearTimeout(autoSaveTimer)
    stopWatch()
  })

  return {
    saveDraft,
    loadDraft,
    clearDraft,
    hasDraft,
    lastSavedAt,
  }
}
