<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { platformMembershipApi, type PlatformMembership } from '@/api/platform-memberships'
import type { PlatformRole } from '@/api/auth'

const roleOptions: Array<{ value: PlatformRole; label: string }> = [
  { value: 'platform_owner', label: 'Platform Owner' },
  { value: 'platform_admin', label: 'Platform Admin' },
  { value: 'platform_operator', label: 'Platform Operator' },
  { value: 'support_engineer', label: 'Support Engineer' },
  { value: 'business_operator', label: 'Business Operator' },
  { value: 'security_auditor', label: 'Security Auditor' },
]

const members = ref<PlatformMembership[]>([])
const loading = ref(false)
const error = ref('')
const creating = ref(false)
const form = reactive({ username: '', password: '', displayName: '', email: '', roles: ['support_engineer'] as PlatformRole[] })

async function load() {
  loading.value = true
  error.value = ''
  try { members.value = await platformMembershipApi.list() }
  catch (cause) { error.value = cause instanceof Error ? cause.message : '加载失败' }
  finally { loading.value = false }
}

async function createMember() {
  creating.value = true
  error.value = ''
  try {
    await platformMembershipApi.create({ ...form })
    Object.assign(form, { username: '', password: '', displayName: '', email: '', roles: ['support_engineer'] })
    await load()
  } catch (cause) { error.value = cause instanceof Error ? cause.message : '创建失败' }
  finally { creating.value = false }
}

async function setStatus(member: PlatformMembership, status: PlatformMembership['status']) {
  await platformMembershipApi.update(member.id, { status })
  await load()
}

async function setRoles(member: PlatformMembership, event: Event) {
  const values = Array.from((event.target as HTMLSelectElement).selectedOptions).map(option => option.value as PlatformRole)
  if (values.length === 0) return
  await platformMembershipApi.update(member.id, { roles: values })
  await load()
}

onMounted(load)
</script>

<template>
  <section class="platform-members">
    <header class="page-heading">
      <div><span>CONTROL / IDENTITY</span><h1>平台成员与角色授权</h1></div>
      <p>Identity 是认证主体；平台成员关系与多角色授权单独管理，不创建系统租户。</p>
    </header>

    <form class="create-panel" @submit.prevent="createMember">
      <div class="panel-title">NEW PLATFORM MEMBERSHIP</div>
      <label>用户名<input v-model="form.username" class="input" autocomplete="off" required /></label>
      <label>初始密码<input v-model="form.password" class="input" type="password" minlength="8" autocomplete="new-password" required /></label>
      <label>显示名称<input v-model="form.displayName" class="input" /></label>
      <label>邮箱<input v-model="form.email" class="input" type="email" /></label>
      <label class="roles-field">角色
        <select v-model="form.roles" class="input" multiple required>
          <option v-for="role in roleOptions" :key="role.value" :value="role.value">{{ role.label }}</option>
        </select>
      </label>
      <button class="btn btn-primary" type="submit" :disabled="creating">{{ creating ? '创建中…' : '创建平台成员' }}</button>
    </form>

    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <div class="member-table-wrap">
      <table class="member-table">
        <thead><tr><th>IDENTITY</th><th>STATUS</th><th>ROLE GRANTS</th><th>CONTROL</th></tr></thead>
        <tbody>
          <tr v-if="loading"><td colspan="4">正在加载平台成员…</td></tr>
          <tr v-for="member in members" :key="member.id">
            <td><strong>{{ member.displayName || member.username }}</strong><small>{{ member.username }} · {{ member.identityId.slice(0, 8) }}</small></td>
            <td><span class="status" :data-status="member.status">{{ member.status }}</span></td>
            <td>
              <select class="role-select" multiple :value="member.roles" @change="setRoles(member, $event)">
                <option v-for="role in roleOptions" :key="role.value" :value="role.value">{{ role.label }}</option>
              </select>
            </td>
            <td class="actions">
              <button v-if="member.status !== 'active'" type="button" @click="setStatus(member, 'active')">启用</button>
              <button v-if="member.status === 'active'" type="button" @click="setStatus(member, 'suspended')">暂停</button>
              <button v-if="member.status !== 'revoked'" type="button" class="danger" @click="setStatus(member, 'revoked')">撤销</button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>

<style scoped>
.platform-members { display: grid; gap: 22px; }
.page-heading { display: grid; grid-template-columns: 1fr minmax(280px, 520px); gap: 24px; align-items: end; border-bottom: 1px solid var(--border-default); padding-bottom: 20px; }
.page-heading span, .panel-title, th { font-family: var(--font-mono); font-size: var(--font-size-xs); letter-spacing: .1em; color: var(--accent); }
h1 { margin: 7px 0 0; font-family: var(--font-mono); font-size: var(--font-size-2xl); }
.page-heading p { margin: 0; color: var(--text-secondary); line-height: 1.65; }
.create-panel { display: grid; grid-template-columns: repeat(4, minmax(140px, 1fr)) minmax(180px, 1.2fr) auto; gap: 12px; align-items: end; padding: 16px; border: 1px solid var(--border-default); border-radius: var(--radius-lg); background: var(--bg-elevated); }
.panel-title { grid-column: 1 / -1; }
label { display: grid; gap: 6px; color: var(--text-secondary); font-size: var(--font-size-sm); }
select[multiple] { min-height: 76px; }
.error { margin: 0; padding: 12px 14px; border: 1px solid var(--accent); border-radius: var(--radius-sm); color: var(--accent); background: var(--accent-muted); }
.member-table-wrap { overflow: auto; border: 1px solid var(--border-default); border-radius: var(--radius-lg); background: var(--bg-elevated); }
.member-table { width: 100%; border-collapse: collapse; }
th, td { padding: 14px 16px; border-bottom: 1px solid var(--border-subtle); text-align: left; vertical-align: middle; }
td strong, td small { display: block; }
td small { margin-top: 4px; color: var(--text-tertiary); font-family: var(--font-mono); }
.status { font-family: var(--font-mono); color: var(--text-secondary); }
.status[data-status='active'] { color: var(--success); }
.status[data-status='suspended'] { color: var(--warning); }
.role-select { min-width: 210px; min-height: 84px; border: 1px solid var(--border-default); border-radius: var(--radius-sm); background: var(--bg-surface); color: var(--text-primary); }
.actions { display: flex; gap: 8px; }
.actions button { min-height: 36px; padding: 0 10px; border: 1px solid var(--border-default); border-radius: var(--radius-sm); background: var(--bg-surface); color: var(--text-primary); cursor: pointer; }
.actions .danger { color: var(--accent); }
@media (max-width: 1200px) { .create-panel { grid-template-columns: repeat(2, 1fr); } }
@media (max-width: 720px) { .page-heading, .create-panel { grid-template-columns: 1fr; } }
</style>
