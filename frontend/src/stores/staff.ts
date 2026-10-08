import { defineStore } from 'pinia'
import { ref } from 'vue'
import * as staffApi from '@/api/staff'
import type { TenantMember } from '@/api/staff'

export const useStaffStore = defineStore('staff', () => {
  const users = ref<TenantMember[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)

  async function fetchAll() {
    error.value = null
    loading.value = true
    try {
      const data = await staffApi.fetchAll()
      users.value = data.memberships
    } catch (e: any) {
      error.value = e.message || 'Failed to fetch staff'
      throw e
    } finally {
      loading.value = false
    }
  }

  async function create(data: { username: string; password: string; role: string }) {
    error.value = null
    try {
      const result = await staffApi.createStaff(data)
      return result
    } catch (e: any) {
      error.value = e.message || 'Failed to create staff'
      throw e
    }
  }

  async function resetPassword(id: string, newPassword: string) {
    error.value = null
    try {
      return await staffApi.resetPassword(id, newPassword)
    } catch (e: any) {
      error.value = e.message || 'Failed to reset password'
      throw e
    }
  }

  async function toggleEnabled(id: string, isEnabled: boolean) {
    error.value = null
    try {
      const result = await staffApi.toggleEnabled(id, isEnabled)
      const idx = users.value.findIndex(u => u.membershipId === id || u.membershipId === result.member?.membershipId)
      if (idx !== -1 && result.member) {
        users.value[idx] = result.member
      }
      return result
    } catch (e: any) {
      error.value = e.message || 'Failed to toggle staff'
      throw e
    }
  }

  async function remove(id: string) {
    error.value = null
    try {
      await staffApi.deleteStaff(id)
      users.value = users.value.filter(u => u.membershipId !== id)
    } catch (e: any) {
      error.value = e.message || 'Failed to delete staff'
      throw e
    }
  }

  async function rename(id: string, newUsername: string) {
    error.value = null
    try {
      const result = await staffApi.updateUsername(id, newUsername)
      const idx = users.value.findIndex(u => u.membershipId === id)
      if (idx !== -1 && result.member) {
        users.value[idx] = result.member
      }
      return result
    } catch (e: any) {
      error.value = e.message || 'Failed to rename staff'
      throw e
    }
  }

  async function transferOwnership(id: string) {
    error.value = null
    try {
      const result = await staffApi.transferOwnership(id)
      await fetchAll()
      return result
    } catch (e: any) {
      error.value = e.message || 'Failed to transfer tenant ownership'
      throw e
    }
  }

  return { users, loading, error, fetchAll, create, resetPassword, toggleEnabled, remove, rename, transferOwnership }
})
