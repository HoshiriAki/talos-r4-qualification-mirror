<script setup lang="ts">
import { computed } from 'vue'
import { useRoute } from 'vue-router'

const route = useRoute()

const sections = {
  overview: {
    code: 'CONTROL / 00',
    title: 'Talos Control',
    description: '平台控制平面只承载租户生命周期、运行治理、支持与经营能力，不进入租户生产身份。',
    links: [
      ['/control/tenants', '租户目录'],
      ['/control/governance', '租户治理'],
      ['/control/platform-members', '平台成员'],
    ],
  },
  operations: {
    code: 'CONTROL / OPS',
    title: '运行控制',
    description: '服务健康、任务、迁移与事故处理的控制面入口。当前业务模块将在能力就绪后接入此分组。',
    links: [['/control/governance', '查看租户健康与治理证据']],
  },
  business: {
    code: 'CONTROL / BIZ',
    title: '平台经营',
    description: '租户增长、套餐、合同、续费、收入与使用量的经营边界。',
    links: [['/control/tenants', '查看租户与套餐状态']],
  },
  support: {
    code: 'CONTROL / SUP',
    title: '售后支持',
    description: '支持工单、只读 Preview 与诊断能力从 Tenant Workspace 发起，不使用身份模拟。',
    links: [['/control/governance', '选择租户并创建 Workspace']],
  },
  audit: {
    code: 'CONTROL / AUD',
    title: '平台审计',
    description: '平台 Authority、capability 与跨租户访问事件的只读审计入口。',
    links: [['/control/governance', '查看治理证据']],
  },
  settings: {
    code: 'CONTROL / SET',
    title: '平台设置',
    description: '平台身份、角色授权与控制平面配置。租户设置不在此处出现。',
    links: [['/control/platform-members', '管理平台成员与角色']],
  },
} as const

const section = computed(() => sections[(route.meta.controlSection as keyof typeof sections) || 'overview'])
</script>

<template>
  <section class="control-section">
    <header class="control-section__header">
      <span class="module-code">{{ section.code }}</span>
      <h1>{{ section.title }}</h1>
      <p>{{ section.description }}</p>
    </header>
    <div class="control-links">
      <RouterLink v-for="link in section.links" :key="link[0]" :to="link[0]" class="control-link">
        <span>{{ link[1] }}</span>
        <span aria-hidden="true">→</span>
      </RouterLink>
    </div>
  </section>
</template>

<style scoped>
.control-section { max-width: 920px; display: grid; gap: 28px; }
.control-section__header { border-bottom: 1px solid var(--border-default); padding-bottom: 24px; }
.module-code { font-family: var(--font-mono); color: var(--accent); font-size: var(--font-size-xs); letter-spacing: .12em; }
h1 { margin: 10px 0 12px; font-family: var(--font-mono); font-size: var(--font-size-3xl); color: var(--text-primary); }
p { max-width: 720px; margin: 0; color: var(--text-secondary); line-height: 1.7; }
.control-links { display: grid; gap: 10px; }
.control-link { display: flex; justify-content: space-between; align-items: center; min-height: 56px; padding: 0 18px; border: 1px solid var(--border-default); border-radius: var(--radius-md); background: var(--bg-elevated); color: var(--text-primary); font-family: var(--font-mono); text-decoration: none; }
.control-link:hover { border-color: var(--accent); transform: translate(-2px, -2px); box-shadow: var(--shadow-hard-sm); }
</style>
