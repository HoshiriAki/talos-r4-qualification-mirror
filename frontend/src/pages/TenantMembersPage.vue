<script setup lang="ts">
import { ref, onMounted, nextTick } from 'vue'
import { formatTime } from '@/composables/useFormatTime'
import { useAuthStore } from '@/stores/auth'
import { useStaffStore } from '@/stores/staff'
import { useToast } from 'primevue/usetoast'
import { useConfirm } from 'primevue/useconfirm'
import DataTable from 'primevue/datatable'
import Column from 'primevue/column'
import type { TenantMember } from '@/api/staff'

const auth = useAuthStore()
const staff = useStaffStore()
const toast = useToast()
const confirm = useConfirm()

// ==================== Selection ====================
const selectedUsers = ref<TenantMember[]>([])

// Create modal
const createModalOpen = ref(false)
const createForm = ref({
  username: '',
  password: '',
  role: 'staff' as 'admin' | 'staff',
})
const createLoading = ref(false)
const createError = ref('')
const confirmPassword = ref('')
const passwordError = ref('')

function validatePassword(password: string, confirm: string): boolean {
  passwordError.value = ''
  if (password.length < 8) {
    passwordError.value = '密码长度至少 8 位'
    return false
  }
  if (password !== confirm) {
    passwordError.value = '两次输入的密码不一致'
    return false
  }
  return true
}

function focusNext(event: KeyboardEvent, selector: string) {
  (event.target as HTMLInputElement | null)?.blur()
  document.querySelector<HTMLInputElement>(selector)?.focus()
}

async function openCreateModal() {
  createForm.value = { username: '', password: '', role: 'staff' }
  confirmPassword.value = ''
  passwordError.value = ''
  createError.value = ''
  createModalOpen.value = true
  await nextTick()
  const input = document.querySelector<HTMLInputElement>('#create-username-input')
  input?.focus()
}

function closeCreateModal() {
  createModalOpen.value = false
}

async function handleCreate() {
  if (!createForm.value.username.trim() || !createForm.value.password) {
    createError.value = '用户名和密码不能为空'
    return
  }
  if (!validatePassword(createForm.value.password, confirmPassword.value)) {
    return
  }
  createLoading.value = true
  createError.value = ''
  try {
    await staff.create({
      username: createForm.value.username.trim(),
      password: createForm.value.password,
      role: createForm.value.role,
    })
    closeCreateModal()
    await staff.fetchAll()
  } catch (e: any) {
    createError.value = e?.message || '创建失败'
  } finally {
    createLoading.value = false
  }
}

// Reset password modal
const resetPwModalOpen = ref(false)
const resetPwUserId = ref('')
const resetPwUsername = ref('')
const resetPwNewPassword = ref('')
const resetPwConfirmPassword = ref('')
const resetPwLoading = ref(false)
const resetPwError = ref('')

function openResetPassword(user: TenantMember) {
  resetPwUserId.value = user.membershipId
  resetPwUsername.value = user.username
  resetPwNewPassword.value = ''
  resetPwConfirmPassword.value = ''
  resetPwError.value = ''
  resetPwModalOpen.value = true
}

function closeResetPassword() {
  resetPwModalOpen.value = false
}

async function handleResetPassword() {
  if (!resetPwNewPassword.value) {
    resetPwError.value = '新密码不能为空'
    return
  }
  if (resetPwNewPassword.value.length < 8) {
    resetPwError.value = '密码长度至少 8 位'
    return
  }
  if (resetPwNewPassword.value !== resetPwConfirmPassword.value) {
    resetPwError.value = '两次输入的密码不一致'
    return
  }
  resetPwLoading.value = true
  resetPwError.value = ''
  try {
    await staff.resetPassword(resetPwUserId.value, resetPwNewPassword.value)
    closeResetPassword()
    toast.add({ severity: 'success', summary: '密码已重置', life: 2000 })
  } catch (e: any) {
    resetPwError.value = e?.message || '重置失败'
  } finally {
    resetPwLoading.value = false
  }
}

// Toggle enabled
const togglingId = ref('')

function handleToggleEnabled(user: TenantMember) {
  const action = user.isEnabled ? '禁用' : '启用'
  confirm.require({
    message: `确认${action}员工 "${user.username}"？`,
    header: `${action}员工`,
    accept: async () => {
      togglingId.value = user.membershipId
      try {
        await staff.toggleEnabled(user.membershipId, !user.isEnabled)
        toast.add({ severity: 'success', summary: `已${action}员工 "${user.username}"`, life: 2000 })
      } catch (e: any) {
        toast.add({ severity: 'error', summary: `${action}失败`, detail: e.message || '请重试', life: 4000 })
      } finally {
        togglingId.value = ''
      }
    },
  })
}

// Delete
const deletingId = ref('')
const transferringId = ref('')

function handleTransferOwnership(user: TenantMember) {
  confirm.require({
    message: `确认将租户所有权转移给 "${user.username}"？完成后你的角色将变为 admin。`,
    header: '转移租户所有权',
    accept: async () => {
      transferringId.value = user.membershipId
      try {
        await staff.transferOwnership(user.membershipId)
        await auth.init('tenant', true)
        toast.add({ severity: 'success', summary: `所有权已转移给 "${user.username}"`, life: 3000 })
      } catch (e: any) {
        toast.add({ severity: 'error', summary: '所有权转移失败', detail: e.message || '请重试', life: 4000 })
      } finally {
        transferringId.value = ''
      }
    },
  })
}

function handleDelete(user: TenantMember) {
  confirm.require({
    message: `确定要删除员工 "${user.username}"？此操作不可撤销。`,
    header: '删除员工',
    accept: async () => {
      deletingId.value = user.membershipId
      try {
        await staff.remove(user.membershipId)
        toast.add({ severity: 'success', summary: `已删除员工 "${user.username}"`, life: 2000 })
      } catch (e: any) {
        toast.add({ severity: 'error', summary: '删除失败', detail: e.message || '请重试', life: 4000 })
      } finally {
        deletingId.value = ''
      }
    },
  })
}

// Inline username editing
const editingUsernameId = ref('')
const editingUsernameValue = ref('')
const editingUsernameLoading = ref(false)

function startRename(user: TenantMember) {
  editingUsernameId.value = user.membershipId
  editingUsernameValue.value = user.username
}

function cancelRename() {
  editingUsernameId.value = ''
  editingUsernameValue.value = ''
}

async function commitRename(userId: string) {
  const newName = editingUsernameValue.value.trim()
  if (!newName || newName.length < 2) {
    toast.add({ severity: 'warn', summary: '用户名至少 2 个字符', life: 2000 })
    return
  }
  editingUsernameLoading.value = true
  try {
    await staff.rename(userId, newName)
    toast.add({ severity: 'success', summary: '用户名已更新', life: 2000 })
    cancelRename()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '修改失败', detail: e.message || '请重试', life: 4000 })
  } finally {
    editingUsernameLoading.value = false
  }
}


onMounted(() => {
  staff.fetchAll().catch(e => toast.add({ severity: 'error', summary: '加载失败', detail: e.message, life: 4000 }))
})
</script>

<template>
  <div class="space-y-4">
    <!-- MODULE-12 bar -->
    <div class="module-bar mb-4">
      <span class="module-number-label">MODULE-12</span>
      <div class="structure-line" />
      <span class="module-page-label">员工管理</span>
    </div>
    <div class="panel">
      <div class="flex items-center justify-between mb-3">
        <h2 class="panel-title mb-0">员工管理</h2>
        <Button severity="primary" label="添加员工" @click="openCreateModal" />
      </div>

      <DataTable
        v-model:selection="selectedUsers"
        :value="staff.users"
        :loading="staff.loading"
        :paginator="true"
        :rows="10"
        :rowsPerPageOptions="[10, 20, 30, 50]"
        paginatorTemplate="FirstPageLink PrevPageLink PageLinks NextPageLink LastPageLink RowsPerPageDropdown CurrentPageReport"
        currentPageReportTemplate="第 {first}-{last} 条，共 {totalRecords} 条"
        :resizableColumns="true"
        columnResizeMode="expand"
        removableSort
      >
        <template #empty>
          <div class="py-6 text-center">
            <span class="font-mono text-xs text-text-muted">暂无员工</span>
          </div>
        </template>
        <Column selectionMode="multiple" headerStyle="width: 3rem" />
        <Column field="username" header="用户名" sortable>
          <template #body="{ data }: { data: TenantMember }">
            <div v-if="editingUsernameId === data.membershipId" class="flex items-center gap-1">
              <InputText
                v-model="editingUsernameValue"
                class="font-mono w-32"
                size="small"
                :disabled="editingUsernameLoading"
                @keydown.enter="commitRename(data.membershipId)"
                @keydown.escape="cancelRename"
              />
              <Button
                severity="primary"
                size="small"
                :disabled="editingUsernameLoading"
                @click="commitRename(data.membershipId)"
              >
                <template #icon><span class="font-mono text-xs">&#10003;</span></template>
              </Button>
              <Button
                severity="secondary"
                size="small"
                :disabled="editingUsernameLoading"
                @click="cancelRename"
              >
                <template #icon><span class="font-mono text-xs">&#10007;</span></template>
              </Button>
            </div>
            <div v-else class="inline-flex items-center gap-1 cursor-pointer hover:opacity-80" @dblclick="startRename(data)">
              <span class="font-mono">{{ data.username }}</span>
              <Tag v-if="data.identityId === auth.currentUser?.id" severity="info" value="当前账号" class="ml-2" />
            </div>
          </template>
        </Column>
        <Column field="role" header="角色" sortable>
          <template #body="{ data }: { data: TenantMember }">
            <Tag
              :severity="data.role === 'owner' ? 'danger' : data.role === 'admin' ? 'warn' : 'info'"
              :value="data.role === 'owner' ? '所有者' : data.role === 'admin' ? '管理员' : '员工'"
            />
          </template>
        </Column>
        <Column field="isEnabled" header="状态" sortable>
          <template #body="{ data }: { data: TenantMember }">
            <Tag :severity="data.isEnabled ? 'success' : 'danger'" :value="data.isEnabled ? '启用' : '禁用'" />
          </template>
        </Column>
        <Column field="lastLoginAt" header="最后登录" sortable>
          <template #body="{ data }: { data: TenantMember }">
            <span class="font-mono text-xs text-text-secondary">{{ data.lastLoginAt ? formatTime(data.lastLoginAt) : '-' }}</span>
          </template>
        </Column>
        <Column field="createdAt" header="创建时间" sortable>
          <template #body="{ data }: { data: TenantMember }">
            <span class="font-mono text-xs text-text-secondary">{{ formatTime(data.createdAt) }}</span>
          </template>
        </Column>
        <Column header="操作" headerStyle="width: 20rem">
          <template #body="{ data }: { data: TenantMember }">
            <div class="flex items-center gap-2">
              <Button
                v-if="auth.isTenantOwner && data.role !== 'owner' && data.isEnabled"
                severity="warn"
                :label="transferringId === data.membershipId ? '转移中...' : '转移所有权'"
                :disabled="transferringId === data.membershipId"
                @click="handleTransferOwnership(data)"
              />
              <Button severity="secondary" label="重置密码" @click="openResetPassword(data)" />
              <Button
                severity="danger"
                :label="togglingId === data.membershipId ? '处理中...' : (data.isEnabled ? '禁用' : '启用')"
                :disabled="togglingId === data.membershipId"
                @click="handleToggleEnabled(data)"
              />
              <Button
                severity="danger"
                :label="deletingId === data.membershipId ? '删除中...' : '删除'"
                :disabled="deletingId === data.membershipId || data.identityId === auth.currentUser?.id"
                @click="handleDelete(data)"
              />
            </div>
          </template>
        </Column>
      </DataTable>
    </div>

    <!-- Create Staff Modal -->
    <Dialog
      v-model:visible="createModalOpen"
      modal
      header="添加员工"
      :style="{ width: '26rem' }"
    >
      <div class="space-y-3">
        <div>
          <label class="mono-label block mb-1">用户名</label>
          <InputText
            id="create-username-input"
            v-model="createForm.username"
            class="w-full"
            placeholder="请输入用户名"
            @keydown.enter="focusNext($event, '#create-pw-input')"
          />
        </div>
        <div>
          <label class="mono-label block mb-1">密码</label>
          <InputText
            id="create-pw-input"
            v-model="createForm.password"
            type="password"
            class="w-full"
            placeholder="请输入密码"
            @keydown.enter="focusNext($event, '#create-confirm-pw-input')"
          />
        </div>
        <div>
          <label class="mono-label block mb-1">确认密码</label>
          <InputText
            id="create-confirm-pw-input"
            v-model="confirmPassword"
            type="password"
            class="w-full"
            placeholder="确认密码"
            @keydown.enter="handleCreate"
          />
        </div>
        <div>
          <label class="mono-label block mb-1">角色</label>
          <Select
            v-model="createForm.role"
            :options="[{ label: '员工 (staff)', value: 'staff' }, { label: '管理员 (admin)', value: 'admin' }]"
            option-label="label"
            option-value="value"
            class="w-full"
          />
        </div>

        <small v-if="passwordError" class="text-status-error text-xs">{{ passwordError }}</small>

        <p v-if="createError" class="text-xs font-mono text-btn-danger-text">{{ createError }}</p>
      </div>

      <template #footer>
        <Button severity="secondary" label="取消" :disabled="createLoading" @click="closeCreateModal" />
        <Button severity="primary" :label="createLoading ? '创建中...' : '创建'" :disabled="createLoading" @click="handleCreate" />
      </template>
    </Dialog>

    <!-- Reset Password Modal -->
    <Dialog
      v-model:visible="resetPwModalOpen"
      modal
      header="重置密码"
      :style="{ width: '24rem' }"
    >
      <div class="space-y-3">
        <p class="font-mono text-sm text-text-secondary">
          用户：<span class="text-text-primary">{{ resetPwUsername }}</span>
        </p>

        <div>
          <label class="mono-label block mb-1">新密码</label>
          <InputText
            id="reset-pw-input"
            v-model="resetPwNewPassword"
            type="password"
            class="w-full"
            placeholder="请输入新密码"
            @keydown.enter="focusNext($event, '#reset-pw-confirm-input')"
          />
        </div>
        <div>
          <label class="mono-label block mb-1">确认密码</label>
          <InputText
            id="reset-pw-confirm-input"
            v-model="resetPwConfirmPassword"
            type="password"
            class="w-full"
            placeholder="确认新密码"
            @keydown.enter="handleResetPassword"
          />
        </div>

        <p v-if="resetPwError" class="text-xs font-mono text-btn-danger-text">{{ resetPwError }}</p>
      </div>

      <template #footer>
        <Button severity="secondary" label="取消" :disabled="resetPwLoading" @click="closeResetPassword" />
        <Button severity="primary" :label="resetPwLoading ? '重置中...' : '确认重置'" :disabled="resetPwLoading" @click="handleResetPassword" />
      </template>
    </Dialog>
    <div class="version-footer">2026 — V1.05 — REV.N</div>
  </div>
</template>
