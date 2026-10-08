import { defineStore } from 'pinia'
import { ref } from 'vue'

const COLLAPSE_KEY = 'talos-sidebar-collapsed'

function loadCollapsed(): Set<string> {
  try {
    const raw = localStorage.getItem(COLLAPSE_KEY)
    if (raw) {
      const arr: string[] = JSON.parse(raw)
      if (Array.isArray(arr)) return new Set(arr)
    }
  } catch { /* corrupt — ignore */ }
  return new Set()
}

function saveCollapsed(set: Set<string>) {
  localStorage.setItem(COLLAPSE_KEY, JSON.stringify([...set]))
}

export const useSidebarStore = defineStore('sidebar', () => {
  const mobileOpen = ref(false)
  const collapsedGroups = ref<Set<string>>(loadCollapsed())
  const activeGroup = ref<string>('operations')

  function toggleMobile() {
    mobileOpen.value = !mobileOpen.value
  }

  function closeMobile() {
    mobileOpen.value = false
  }

  function isGroupCollapsed(groupId: string): boolean {
    return collapsedGroups.value.has(groupId)
  }

  function toggleGroup(groupId: string) {
    const next = new Set(collapsedGroups.value)
    if (next.has(groupId)) {
      next.delete(groupId)
    } else {
      next.add(groupId)
    }
    collapsedGroups.value = next
    saveCollapsed(next)
  }

  function setActiveGroup(groupId: string) {
    activeGroup.value = groupId
  }

  return {
    mobileOpen,
    collapsedGroups,
    activeGroup,
    toggleMobile,
    closeMobile,
    isGroupCollapsed,
    toggleGroup,
    setActiveGroup,
  }
})
