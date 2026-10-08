<script setup lang="ts">
import { ref, computed } from 'vue'
import { Icon } from '@iconify/vue'

const currentTab = ref('overview')
const sampleData = ref([
  { id: 1, name: '张三', status: 'completed', value: 890 },
  { id: 2, name: '李四', status: 'active', value: 1200 },
  { id: 3, name: '王五', status: 'pending', value: 450 },
])

// Search functionality
const searchQuery = ref('')
const categories = [
  { id: 'cards', label: '卡片系统', icon: 'material-symbols:layers', count: 4 },
  { id: 'buttons', label: '按钮', icon: 'material-symbols:touch-app', count: 4 },
  { id: 'v12-new', label: 'v1.2 新组件', icon: 'material-symbols:auto-awesome', count: 5, badge: 'NEW' },
  { id: 'forms', label: '表单', icon: 'material-symbols:edit-note', count: 4 },
  { id: 'tables', label: '表格', icon: 'material-symbols:table', count: 1 },
  { id: 'status', label: '状态指示器', icon: 'material-symbols:activity-zone', count: 7 },
  { id: 'typography', label: '字体', icon: 'material-symbols:font-download', count: 2 },
  { id: 'glass', label: '玻璃态', icon: 'material-symbols:water-drop', count: 1 },
]

// v1.2 Demo Data
const megaCTAProgress = ref({ current: 24, total: 50 })
const notificationCount = ref(12)
const quickActions = ref([
  { id: 1, icon: 'material-symbols:add', label: '新建订单', isNew: true, route: '/price' },
  { id: 2, icon: 'material-symbols:group', label: '客户管理', hasAlert: false, route: '/customers' },
  { id: 3, icon: 'material-symbols:error', label: '逾期订单', hasAlert: true, alertCount: 12, route: '/overdue' },
])

const filteredCategories = computed(() => {
  if (!searchQuery.value) return categories
  const query = searchQuery.value.toLowerCase()
  return categories.filter(cat =>
    cat.label.toLowerCase().includes(query) ||
    cat.id.toLowerCase().includes(query)
  )
})

const totalComponents = computed(() =>
  categories.reduce((sum, cat) => sum + cat.count, 0)
)
</script>

<template>
  <div class="page-root">
    <!-- ═══ MODULE-NN bar — 每页必需 ═══ -->
    <div class="module-bar mb-8">
      <span class="module-number-label">MODULE-99</span>
      <div class="structure-line" />
      <span class="module-page-label">Industrial Avant UI · Design System</span>
    </div>

    <!-- ═══ Hero Section with Search ═══ -->
    <div class="hero-section mb-12">
      <div class="grid grid-cols-1 lg:grid-cols-12 gap-6">
        <!-- Left: Hero Stats -->
        <div class="lg:col-span-7">
          <div class="card-emphasis h-full">
            <div class="flex items-start justify-between mb-6">
              <div>
                <div class="module-number mb-3">SYS_01</div>
                <h1 class="font-mono text-4xl md:text-5xl font-bold text-text-primary leading-tight mb-3">
                  Design System
                </h1>
                <p class="font-sans text-base text-text-secondary leading-relaxed max-w-xl">
                  完整的 Brutalism + Avant-Garde + Neo-brutalist Glass 设计语言。
                  包含 {{ totalComponents }} 个生产就绪组件。
                </p>
              </div>
              <div class="flex flex-col gap-2 items-end">
                <span class="status-indicator-success" />
                <span class="font-mono text-2xs text-text-muted uppercase tracking-wider">System Nominal</span>
              </div>
            </div>

            <!-- Quick Stats -->
            <div class="grid grid-cols-3 gap-4 mt-6 pt-6 border-t border-border">
              <div>
                <div class="font-mono text-3xl font-bold text-accent">{{ totalComponents }}</div>
                <div class="font-sans text-xs text-text-muted mt-1">组件</div>
              </div>
              <div>
                <div class="font-mono text-3xl font-bold text-text-primary">161</div>
                <div class="font-sans text-xs text-text-muted mt-1">Token 变量</div>
              </div>
              <div>
                <div class="font-mono text-3xl font-bold text-text-primary">v1.1</div>
                <div class="font-sans text-xs text-text-muted mt-1">版本</div>
              </div>
            </div>
          </div>
        </div>

        <!-- Right: Search & Quick Nav -->
        <div class="lg:col-span-5 flex flex-col gap-4">
          <!-- Search Bar -->
          <div class="card">
            <label class="mono-label block mb-3">快速搜索组件</label>
            <div class="relative">
              <Icon
                icon="material-symbols:search"
                width="18"
                height="18"
                class="absolute left-3 top-1/2 -translate-y-1/2 text-text-muted pointer-events-none"
              />
              <input
                v-model="searchQuery"
                type="text"
                class="input pl-10 w-full"
                placeholder="输入组件名称..."
              />
            </div>
          </div>

          <!-- Categories Grid -->
          <div class="card flex-1">
            <h3 class="font-mono text-sm font-bold text-text-primary mb-4 uppercase tracking-wider">组件分类</h3>
            <div class="grid grid-cols-2 gap-2">
              <button
                v-for="cat in filteredCategories.slice(0, 6)"
                :key="cat.id"
                class="category-btn"
                :class="{ 'category-btn-active': currentTab === cat.id }"
                @click="currentTab = cat.id"
              >
                <Icon :icon="cat.icon" width="16" height="16" />
                <span class="font-mono text-xs">{{ cat.label }}</span>
                <span class="font-mono text-2xs text-text-muted ml-auto">{{ cat.count }}</span>
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- ═══ Overview Tab ═══ -->
    <div v-if="currentTab === 'overview'" class="space-y-8">
      <div class="card-focus">
        <h2 class="font-mono text-2xl font-bold text-text-primary mb-4">设计系统核心原则</h2>
        <div class="grid grid-cols-1 md:grid-cols-3 gap-6">
          <div class="principle-item">
            <div class="flex items-center gap-2 mb-3">
              <Icon icon="material-symbols:inventory-2" width="20" height="20" class="text-accent" />
              <h3 class="font-mono text-base font-bold text-text-primary">Brutalism</h3>
            </div>
            <p class="font-sans text-sm text-text-secondary leading-relaxed">
              尖锐边框、网格布局、工业编号系统。结构即美学。
            </p>
          </div>
          <div class="principle-item">
            <div class="flex items-center gap-2 mb-3">
              <Icon icon="material-symbols:bolt" width="20" height="20" class="text-accent" />
              <h3 class="font-mono text-base font-bold text-text-primary">Avant-Garde</h3>
            </div>
            <p class="font-sans text-sm text-text-secondary leading-relaxed">
              不对称构图、正交运动、CAD 动画。视觉张力与节奏。
            </p>
          </div>
          <div class="principle-item">
            <div class="flex items-center gap-2 mb-3">
              <Icon icon="material-symbols:water-drop" width="20" height="20" class="text-accent" />
              <h3 class="font-mono text-base font-bold text-text-primary">Neo-brutalist Glass</h3>
            </div>
            <p class="font-sans text-sm text-text-secondary leading-relaxed">
              玻璃态浮层、硬偏移阴影、边缘高光。现代材质语言。
            </p>
          </div>
        </div>
      </div>

      <!-- Quick Reference Cards -->
      <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        <div class="card cursor-pointer hover:border-accent transition-colors duration-micro" @click="currentTab = 'cards'">
          <div class="flex items-center justify-between mb-3">
            <Icon icon="material-symbols:layers" width="24" height="24" class="text-accent" />
            <span class="badge-success">4 级</span>
          </div>
          <h3 class="font-mono text-base font-bold text-text-primary mb-2">卡片系统</h3>
          <p class="font-sans text-sm text-text-secondary">标准、强调、焦点、警示四级卡片</p>
        </div>

        <div class="card cursor-pointer hover:border-accent transition-colors duration-micro" @click="currentTab = 'buttons'">
          <div class="flex items-center justify-between mb-3">
            <Icon icon="material-symbols:touch-app" width="24" height="24" class="text-accent" />
            <span class="badge-info">4 种</span>
          </div>
          <h3 class="font-mono text-base font-bold text-text-primary mb-2">按钮系统</h3>
          <p class="font-sans text-sm text-text-secondary">Primary、Secondary、Danger、Ghost</p>
        </div>

        <div class="card cursor-pointer hover:border-accent transition-colors duration-micro" @click="currentTab = 'status'">
          <div class="flex items-center justify-between mb-3">
            <Icon icon="material-symbols:activity-zone" width="24" height="24" class="text-accent" />
            <span class="badge-warning">5 状态</span>
          </div>
          <h3 class="font-mono text-base font-bold text-text-primary mb-2">状态指示器</h3>
          <p class="font-sans text-sm text-text-secondary">8×8px 方块 + 文字标签（色盲友好）</p>
        </div>
      </div>
    </div>

    <!-- ═══ Cards Demo ═══ -->
    <div v-if="currentTab === 'cards'" class="space-y-8">
      <div class="flex items-center justify-between">
        <div>
          <h2 class="font-mono text-2xl font-bold text-text-primary mb-2">卡片系统</h2>
          <p class="font-sans text-sm text-text-secondary">4级卡片层级，适应不同信息权重</p>
        </div>
        <span class="badge-info">4 级</span>
      </div>

      <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
        <!-- 标准卡片 -->
        <div class="demo-wrapper">
          <div class="demo-label">
            <span class="component-number">A-01</span>
            <span class="font-mono text-xs text-text-muted uppercase">Standard</span>
          </div>
          <div class="card">
            <div class="flex items-center justify-between mb-3">
              <Icon icon="material-symbols:inventory-2" width="20" height="20" class="text-text-muted" />
              <span class="badge-muted">常用</span>
            </div>
            <h3 class="font-mono text-base font-bold text-text-primary mb-2">标准卡片</h3>
            <p class="font-sans text-sm text-text-secondary leading-relaxed mb-4">
              12px 圆角 + 不透明 bg-surface + 硬偏移阴影。用于常规信息展示。
            </p>
            <div class="code-snippet">
              <code class="font-mono text-xs text-text-primary">&lt;div class="card"&gt;...&lt;/div&gt;</code>
            </div>
          </div>
        </div>

        <!-- 强调卡片 -->
        <div class="demo-wrapper">
          <div class="demo-label">
            <span class="component-number">A-02</span>
            <span class="font-mono text-xs text-text-muted uppercase">Emphasis</span>
          </div>
          <div class="card-emphasis">
            <div class="flex items-center justify-between mb-3">
              <Icon icon="material-symbols:bolt" width="20" height="20" class="text-accent" />
              <span class="badge-warning">重点</span>
            </div>
            <h3 class="font-mono text-base font-bold text-text-primary mb-2">强调卡片</h3>
            <p class="font-sans text-sm text-text-secondary leading-relaxed mb-4">
              2px 边框 + accent 阴影 + 可选左侧 accent 条。用于重点内容。
            </p>
            <div class="mt-4 pt-4 border-t border-border">
              <div class="hero-kpi text-accent">24,890</div>
              <div class="font-sans text-xs text-text-muted mt-1">关键指标示例</div>
            </div>
          </div>
        </div>

        <!-- 焦点卡片 -->
        <div class="demo-wrapper">
          <div class="demo-label">
            <span class="component-number">A-03</span>
            <span class="font-mono text-xs text-text-muted uppercase">Focus</span>
          </div>
          <div class="card-focus">
            <div class="flex items-center justify-between mb-3">
              <Icon icon="material-symbols:flag" width="20" height="20" class="text-accent" />
              <span class="badge-success">最重要</span>
            </div>
            <h3 class="font-mono text-base font-bold text-text-primary mb-2">焦点卡片</h3>
            <p class="font-sans text-sm text-text-secondary leading-relaxed mb-4">
              高对比反转 + accent 边框。用于最重要的单一数据点。
            </p>
            <div class="code-snippet">
              <code class="font-mono text-xs text-text-primary">&lt;div class="card-focus"&gt;...&lt;/div&gt;</code>
            </div>
          </div>
        </div>

        <!-- 警示卡片 -->
        <div class="demo-wrapper">
          <div class="demo-label">
            <span class="component-number">A-04</span>
            <span class="font-mono text-xs text-text-muted uppercase">Alert</span>
          </div>
          <div class="card-alert">
            <div class="flex items-center gap-3 mb-3">
              <Icon icon="material-symbols:warning" width="20" height="20" class="text-status-warning" />
              <span class="badge-error">注意</span>
            </div>
            <h3 class="font-mono text-base font-bold text-text-primary mb-2">警示卡片</h3>
            <p class="font-sans text-sm text-text-secondary leading-relaxed mb-4">
              warning 边框 + 对角条纹角标。用于需要注意的信息。
            </p>
            <div class="code-snippet">
              <code class="font-mono text-xs text-text-primary">&lt;div class="card-alert"&gt;...&lt;/div&gt;</code>
            </div>
          </div>
        </div>
      </div>

      <!-- Usage Guidelines -->
      <div class="card bg-bg-elevated">
        <h3 class="font-mono text-sm font-bold text-text-primary mb-4 flex items-center gap-2">
          <Icon icon="material-symbols:info" width="16" height="16" class="text-status-info" />
          使用指南
        </h3>
        <div class="space-y-3 font-sans text-sm text-text-secondary">
          <div class="flex items-start gap-3">
            <Icon icon="material-symbols:check" width="16" height="16" class="text-status-success shrink-0 mt-0.5" />
            <span>卡片必须使用不透明背景（bg-surface），数据卡片禁止玻璃态</span>
          </div>
          <div class="flex items-start gap-3">
            <Icon icon="material-symbols:check" width="16" height="16" class="text-status-success shrink-0 mt-0.5" />
            <span>所有卡片使用 12px 圆角和硬偏移阴影（无模糊）</span>
          </div>
          <div class="flex items-start gap-3">
            <Icon icon="material-symbols:close" width="16" height="16" class="text-status-error shrink-0 mt-0.5" />
            <span>避免嵌套卡片 — 嵌套卡片永远是错误的设计</span>
          </div>
        </div>
      </div>
    </div>

    <!-- ═══ Buttons Demo ═══ -->
    <div v-if="currentTab === 'buttons'" class="space-y-8">
      <div class="flex items-center justify-between">
        <div>
          <h2 class="font-mono text-2xl font-bold text-text-primary mb-2">按钮系统</h2>
          <p class="font-sans text-sm text-text-secondary">4种按钮类型，覆盖所有交互场景</p>
        </div>
        <span class="badge-info">4 种</span>
      </div>

      <!-- Interactive Demo -->
      <div class="card-emphasis">
        <h3 class="font-mono text-base font-bold text-text-primary mb-4 flex items-center gap-2">
          <Icon icon="material-symbols:touch-app" width="18" height="18" class="text-accent" />
          交互演示（可点击）
        </h3>
        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
          <div class="demo-btn-group">
            <span class="demo-label-inline">Primary</span>
            <button class="btn-primary w-full">确认订单</button>
            <span class="font-sans text-2xs text-text-muted">主要操作</span>
          </div>
          <div class="demo-btn-group">
            <span class="demo-label-inline">Secondary</span>
            <button class="btn-secondary w-full">取消</button>
            <span class="font-sans text-2xs text-text-muted">次要操作</span>
          </div>
          <div class="demo-btn-group">
            <span class="demo-label-inline">Danger</span>
            <button class="btn-danger w-full">删除数据</button>
            <span class="font-sans text-2xs text-text-muted">危险操作</span>
          </div>
          <div class="demo-btn-group">
            <span class="demo-label-inline">Ghost</span>
            <button class="btn-ghost w-full">更多选项 →</button>
            <span class="font-sans text-2xs text-text-muted">低权重操作</span>
          </div>
        </div>
      </div>

      <!-- States Grid -->
      <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
        <!-- Normal States -->
        <div class="card">
          <h3 class="font-mono text-sm font-bold text-text-primary mb-4 uppercase tracking-wider">正常状态</h3>
          <div class="space-y-3">
            <button class="btn-primary w-full">Primary Button</button>
            <button class="btn-secondary w-full">Secondary Button</button>
            <button class="btn-danger w-full">Danger Button</button>
            <button class="btn-ghost w-full">Ghost Button →</button>
          </div>
          <div class="mt-4 pt-4 border-t border-border">
            <div class="flex items-start gap-2">
              <Icon icon="material-symbols:info" width="14" height="14" class="text-status-info shrink-0 mt-0.5" />
              <p class="font-sans text-xs text-text-muted leading-relaxed">
                Space Mono + uppercase + 10px 圆角 + 2px 边框 + 硬偏移阴影
              </p>
            </div>
          </div>
        </div>

        <!-- Disabled States -->
        <div class="card">
          <h3 class="font-mono text-sm font-bold text-text-primary mb-4 uppercase tracking-wider">禁用状态</h3>
          <div class="space-y-3">
            <button class="btn-primary w-full" disabled>已禁用</button>
            <button class="btn-secondary w-full" disabled>已禁用</button>
            <button class="btn-danger w-full" disabled>已禁用</button>
            <button class="btn-ghost w-full" disabled>已禁用</button>
          </div>
          <div class="mt-4 pt-4 border-t border-border">
            <div class="flex items-start gap-2">
              <Icon icon="material-symbols:info" width="14" height="14" class="text-status-info shrink-0 mt-0.5" />
              <p class="font-sans text-xs text-text-muted leading-relaxed">
                opacity: 0.35 + cursor: not-allowed
              </p>
            </div>
          </div>
        </div>
      </div>

      <!-- Size Variants -->
      <div class="card">
        <h3 class="font-mono text-sm font-bold text-text-primary mb-4 uppercase tracking-wider">尺寸变体</h3>
        <div class="flex flex-wrap items-center gap-4">
          <button class="btn-primary text-xs px-3 py-1.5">Small (32px)</button>
          <button class="btn-primary">Default (36px)</button>
          <button class="btn-primary text-base px-6 py-2.5">Large (44px)</button>
        </div>
        <div class="mt-4 pt-4 border-t border-border">
          <p class="font-sans text-xs text-text-muted">
            最小触控目标 44×44px (iOS) / 48×48px (Android)
          </p>
        </div>
      </div>

      <!-- Usage Guidelines -->
      <div class="card bg-bg-elevated">
        <h3 class="font-mono text-sm font-bold text-text-primary mb-4 flex items-center gap-2">
          <Icon icon="material-symbols:book" width="16" height="16" class="text-status-info" />
          使用场景
        </h3>
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div class="space-y-3">
            <div class="flex items-start gap-3">
              <div class="w-3 h-3 rounded-sm bg-accent shrink-0 mt-1" />
              <div>
                <div class="font-mono text-xs font-bold text-text-primary mb-1">Primary</div>
                <div class="font-sans text-xs text-text-secondary">表单提交、确认对话框、主要 CTA</div>
              </div>
            </div>
            <div class="flex items-start gap-3">
              <div class="w-3 h-3 rounded-sm border-2 border-border shrink-0 mt-1" />
              <div>
                <div class="font-mono text-xs font-bold text-text-primary mb-1">Secondary</div>
                <div class="font-sans text-xs text-text-secondary">取消操作、返回、备选方案</div>
              </div>
            </div>
          </div>
          <div class="space-y-3">
            <div class="flex items-start gap-3">
              <div class="w-3 h-3 rounded-sm bg-accent shrink-0 mt-1 opacity-40" />
              <div>
                <div class="font-mono text-xs font-bold text-text-primary mb-1">Danger</div>
                <div class="font-sans text-xs text-text-secondary">删除、清空、不可逆操作</div>
              </div>
            </div>
            <div class="flex items-start gap-3">
              <div class="w-3 h-3 rounded-sm bg-transparent border border-border shrink-0 mt-1" />
              <div>
                <div class="font-mono text-xs font-bold text-text-primary mb-1">Ghost</div>
                <div class="font-sans text-xs text-text-secondary">导航链接、更多操作、低优先级功能</div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- ═══ v1.2 NEW Components Demo ═══ -->
    <div v-if="currentTab === 'v12-new'" class="space-y-8">
      <div class="flex items-center justify-between">
        <div>
          <h2 class="font-mono text-2xl font-bold text-text-primary mb-2">v1.2 新组件</h2>
          <p class="font-sans text-sm text-text-secondary">游戏 UI 整合 — 空间叙事与 3D 美学</p>
        </div>
        <span class="badge-new">NEW v1.2</span>
      </div>

      <!-- Version Comparison -->
      <div class="card-emphasis">
        <h3 class="font-mono text-base font-bold text-text-primary mb-4">版本进化对比</h3>
        <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
          <div>
            <div class="font-mono text-sm font-bold text-text-muted mb-3 uppercase">v1.1 基础</div>
            <ul class="space-y-2 font-sans text-sm text-text-secondary">
              <li class="flex items-start gap-2">
                <Icon icon="material-symbols:check" width="16" height="16" class="text-status-success shrink-0 mt-0.5" />
                <span>Brutalism + Avant-Garde + Glass</span>
              </li>
              <li class="flex items-start gap-2">
                <Icon icon="material-symbols:check" width="16" height="16" class="text-status-success shrink-0 mt-0.5" />
                <span>硬偏移阴影系统</span>
              </li>
              <li class="flex items-start gap-2">
                <Icon icon="material-symbols:check" width="16" height="16" class="text-status-success shrink-0 mt-0.5" />
                <span>2D 平面布局</span>
              </li>
            </ul>
          </div>
          <div>
            <div class="font-mono text-sm font-bold text-accent mb-3 uppercase">v1.2 增强</div>
            <ul class="space-y-2 font-sans text-sm text-text-secondary">
              <li class="flex items-start gap-2">
                <Icon icon="material-symbols:auto-awesome" width="16" height="16" class="text-accent shrink-0 mt-0.5" />
                <span>+ Spatial Narrative（空间叙事）</span>
              </li>
              <li class="flex items-start gap-2">
                <Icon icon="material-symbols:auto-awesome" width="16" height="16" class="text-accent shrink-0 mt-0.5" />
                <span>+ Isometric Aesthetics（3D 等角）</span>
              </li>
              <li class="flex items-start gap-2">
                <Icon icon="material-symbols:auto-awesome" width="16" height="16" class="text-accent shrink-0 mt-0.5" />
                <span>+ 柔和阴影 + 游戏红色系</span>
              </li>
            </ul>
          </div>
        </div>
      </div>

      <!-- 1. Quick Action Panel -->
      <div class="space-y-4">
        <div class="flex items-center justify-between">
          <h3 class="font-mono text-xl font-bold text-text-primary">1. Quick Action Panel</h3>
          <span class="component-number">v1.2-A</span>
        </div>

        <div class="card">
          <div class="mb-4">
            <h4 class="font-mono text-sm font-bold text-text-primary mb-2">左侧 3D 快捷面板</h4>
            <p class="font-sans text-sm text-text-secondary leading-relaxed">
              固定在左侧的快捷操作卡片，具有 3D 透视效果。灵感来源于游戏 UI 的左侧导航。
            </p>
          </div>

          <!-- Demo -->
          <div class="demo-container">
            <div class="quick-actions-panel-demo">
              <div v-for="action in quickActions" :key="action.id"
                   class="action-card-demo"
                   :class="{ 'action-card-demo--new': action.isNew }">
                <!-- NEW 标签 -->
                <span v-if="action.isNew" class="badge-new-demo">NEW</span>

                <!-- 警告指示器 -->
                <span v-if="action.hasAlert" class="alert-indicator-demo">{{ action.alertCount }}</span>

                <!-- 图标 -->
                <Icon :icon="action.icon" width="32" height="32" class="text-text-primary" />

                <!-- 标签 -->
                <span class="action-label-demo">{{ action.label }}</span>
              </div>
            </div>
          </div>

          <!-- Specs -->
          <div class="mt-4 pt-4 border-t border-border">
            <div class="grid grid-cols-2 gap-4 text-xs">
              <div>
                <div class="font-mono font-bold text-text-primary mb-1">尺寸规格</div>
                <div class="font-sans text-text-secondary">120px × 100px</div>
                <div class="font-sans text-text-secondary">间距: 8px</div>
              </div>
              <div>
                <div class="font-mono font-bold text-text-primary mb-1">3D 效果</div>
                <div class="font-sans text-text-secondary">perspective: 800px</div>
                <div class="font-sans text-text-secondary">rotateY: 5deg</div>
              </div>
            </div>
          </div>
        </div>

        <!-- Code Example -->
        <div class="card bg-bg-elevated">
          <h4 class="font-mono text-sm font-bold text-text-primary mb-3">使用示例</h4>
          <div class="code-snippet">
            <pre class="font-mono text-xs text-text-primary"><code>&lt;aside class="quick-actions-panel"&gt;
  &lt;div class="action-card action-card--new"&gt;
    &lt;span class="badge-new"&gt;NEW&lt;/span&gt;
    &lt;Icon icon="material-symbols:add" /&gt;
    &lt;span&gt;新建订单&lt;/span&gt;
  &lt;/div&gt;
&lt;/aside&gt;</code></pre>
          </div>
        </div>
      </div>

      <!-- 2. Mega CTA Button -->
      <div class="space-y-4">
        <div class="flex items-center justify-between">
          <h3 class="font-mono text-xl font-bold text-text-primary">2. Mega CTA Button</h3>
          <span class="component-number">v1.2-B</span>
        </div>

        <div class="card">
          <div class="mb-4">
            <h4 class="font-mono text-sm font-bold text-text-primary mb-2">超大行动按钮</h4>
            <p class="font-sans text-sm text-text-secondary leading-relaxed">
              用于主要业务流程入口的超大按钮，带进度指示和发光效果。灵感来源于游戏 UI 的行动按钮。
            </p>
          </div>

          <!-- Demo -->
          <div class="demo-container flex justify-center py-8">
            <button class="btn-mega-demo">
              <div class="btn-mega-icon-demo">
                <Icon icon="material-symbols:bolt" width="40" height="40" />
              </div>
              <div class="btn-mega-content-demo">
                <span class="btn-mega-label-demo">快速录单</span>
                <div class="btn-mega-progress-demo">
                  <span class="progress-text-demo">{{ megaCTAProgress.current }}/{{ megaCTAProgress.total }}</span>
                  <div class="progress-bar-demo">
                    <div class="progress-fill-demo" :style="{ width: (megaCTAProgress.current / megaCTAProgress.total * 100) + '%' }"></div>
                  </div>
                </div>
              </div>
            </button>
          </div>

          <!-- Specs -->
          <div class="mt-4 pt-4 border-t border-border">
            <div class="grid grid-cols-3 gap-4 text-xs">
              <div>
                <div class="font-mono font-bold text-text-primary mb-1">最小尺寸</div>
                <div class="font-sans text-text-secondary">200px × 120px</div>
              </div>
              <div>
                <div class="font-mono font-bold text-text-primary mb-1">颜色</div>
                <div class="font-sans text-text-secondary">游戏红 #FF3B4A</div>
              </div>
              <div>
                <div class="font-mono font-bold text-text-primary mb-1">效果</div>
                <div class="font-sans text-text-secondary">发光 + 脉冲动画</div>
              </div>
            </div>
          </div>
        </div>

        <!-- Usage Guidelines -->
        <div class="card bg-bg-elevated">
          <h4 class="font-mono text-sm font-bold text-text-primary mb-3">使用指南</h4>
          <div class="space-y-3 font-sans text-sm">
            <div class="flex items-start gap-3">
              <Icon icon="material-symbols:check" width="16" height="16" class="text-status-success shrink-0 mt-0.5" />
              <span class="text-text-secondary">主要业务流程入口（快速录单、批量导入）</span>
            </div>
            <div class="flex items-start gap-3">
              <Icon icon="material-symbols:check" width="16" height="16" class="text-status-success shrink-0 mt-0.5" />
              <span class="text-text-secondary">每页最多 1 个，避免视觉过载</span>
            </div>
            <div class="flex items-start gap-3">
              <Icon icon="material-symbols:close" width="16" height="16" class="text-status-error shrink-0 mt-0.5" />
              <span class="text-text-secondary">不用于次要操作或取消按钮</span>
            </div>
          </div>
        </div>
      </div>

      <!-- 3. Notification Badge -->
      <div class="space-y-4">
        <div class="flex items-center justify-between">
          <h3 class="font-mono text-xl font-bold text-text-primary">3. Notification Badge</h3>
          <span class="component-number">v1.2-C</span>
        </div>

        <div class="card">
          <div class="mb-4">
            <h4 class="font-mono text-sm font-bold text-text-primary mb-2">通知徽章系统</h4>
            <p class="font-sans text-sm text-text-secondary leading-relaxed">
              标准化的通知徽章，用于显示未读数量和系统警告。灵感来源于游戏 UI 的通知系统。
            </p>
          </div>

          <!-- Demo -->
          <div class="demo-container">
            <div class="flex flex-wrap items-center gap-8 justify-center py-6">
              <!-- 按钮 + Badge -->
              <div class="badge-demo-wrapper">
                <button class="btn-secondary relative">
                  <Icon icon="material-symbols:notifications" width="20" height="20" />
                  <span>通知</span>
                  <span class="notification-badge-demo">{{ notificationCount }}</span>
                </button>
                <span class="demo-label-small">按钮徽章</span>
              </div>

              <!-- 图标 + Badge -->
              <div class="badge-demo-wrapper">
                <div class="relative inline-block">
                  <div class="w-12 h-12 bg-bg-elevated rounded-lg flex items-center justify-center">
                    <Icon icon="material-symbols:mail" width="24" height="24" class="text-text-primary" />
                  </div>
                  <span class="notification-badge-demo notification-badge-demo--small">5</span>
                </div>
                <span class="demo-label-small">图标徽章（小）</span>
              </div>

              <!-- 大尺寸 -->
              <div class="badge-demo-wrapper">
                <div class="relative inline-block">
                  <div class="w-16 h-16 bg-bg-elevated rounded-lg flex items-center justify-center">
                    <Icon icon="material-symbols:inbox" width="32" height="32" class="text-text-primary" />
                  </div>
                  <span class="notification-badge-demo notification-badge-demo--large">99+</span>
                </div>
                <span class="demo-label-small">大徽章（99+）</span>
              </div>
            </div>
          </div>

          <!-- Specs -->
          <div class="mt-4 pt-4 border-t border-border">
            <div class="grid grid-cols-3 gap-4 text-xs">
              <div>
                <div class="font-mono font-bold text-text-primary mb-1">尺寸</div>
                <div class="font-sans text-text-secondary">Small: 16px</div>
                <div class="font-sans text-text-secondary">Default: 20px</div>
                <div class="font-sans text-text-secondary">Large: 24px</div>
              </div>
              <div>
                <div class="font-mono font-bold text-text-primary mb-1">颜色</div>
                <div class="font-sans text-text-secondary">游戏红 #FF3B4A</div>
                <div class="font-sans text-text-secondary">白色文字</div>
              </div>
              <div>
                <div class="font-mono font-bold text-text-primary mb-1">最大数字</div>
                <div class="font-sans text-text-secondary">99+</div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- 4. Soft Shadow System -->
      <div class="space-y-4">
        <div class="flex items-center justify-between">
          <h3 class="font-mono text-xl font-bold text-text-primary">4. 柔和阴影系统</h3>
          <span class="component-number">v1.2-D</span>
        </div>

        <div class="card">
          <div class="mb-4">
            <h4 class="font-mono text-sm font-bold text-text-primary mb-2">双模式阴影</h4>
            <p class="font-sans text-sm text-text-secondary leading-relaxed">
              v1.2 新增柔和阴影选项，适用于 3D 场景和浮动元素。v1.1 硬偏移阴影保留为数据卡片默认。
            </p>
          </div>

          <!-- Demo -->
          <div class="demo-container">
            <div class="grid grid-cols-2 md:grid-cols-3 gap-6 py-6">
              <!-- Hard Offset (v1.1) -->
              <div class="shadow-demo-item">
                <div class="shadow-demo-card shadow-demo-card--offset">
                  <span class="shadow-demo-label">硬偏移</span>
                </div>
                <span class="demo-label-small">v1.1 默认</span>
              </div>

              <!-- Soft Level 2 -->
              <div class="shadow-demo-item">
                <div class="shadow-demo-card shadow-demo-card--soft2">
                  <span class="shadow-demo-label">柔和 2</span>
                </div>
                <span class="demo-label-small">常规浮起</span>
              </div>

              <!-- Soft Level 4 -->
              <div class="shadow-demo-item">
                <div class="shadow-demo-card shadow-demo-card--soft4">
                  <span class="shadow-demo-label">柔和 4</span>
                </div>
                <span class="demo-label-small">浮动面板</span>
              </div>

              <!-- Elevated -->
              <div class="shadow-demo-item">
                <div class="shadow-demo-card shadow-demo-card--elevated">
                  <span class="shadow-demo-label">组合浮起</span>
                </div>
                <span class="demo-label-small">多层阴影</span>
              </div>

              <!-- Floating -->
              <div class="shadow-demo-item">
                <div class="shadow-demo-card shadow-demo-card--floating">
                  <span class="shadow-demo-label">悬浮</span>
                </div>
                <span class="demo-label-small">最大深度</span>
              </div>

              <!-- Glow -->
              <div class="shadow-demo-item">
                <div class="shadow-demo-card shadow-demo-card--glow">
                  <span class="shadow-demo-label">发光</span>
                </div>
                <span class="demo-label-small">强调状态</span>
              </div>
            </div>
          </div>

          <!-- Usage Table -->
          <div class="mt-4 pt-4 border-t border-border">
            <table class="w-full text-xs">
              <thead>
                <tr class="border-b border-border">
                  <th class="font-mono font-bold text-text-primary text-left py-2">阴影类型</th>
                  <th class="font-mono font-bold text-text-primary text-left py-2">使用场景</th>
                </tr>
              </thead>
              <tbody class="font-sans text-text-secondary">
                <tr class="border-b border-border">
                  <td class="py-2">硬偏移</td>
                  <td class="py-2">数据卡片、按钮、表格（v1.1 默认）</td>
                </tr>
                <tr class="border-b border-border">
                  <td class="py-2">柔和阴影</td>
                  <td class="py-2">3D 场景、等角图标、浮动面板</td>
                </tr>
                <tr>
                  <td class="py-2">发光阴影</td>
                  <td class="py-2">Mega CTA、激活状态、通知</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </div>

      <!-- 5. Game Colors -->
      <div class="space-y-4">
        <div class="flex items-center justify-between">
          <h3 class="font-mono text-xl font-bold text-text-primary">5. 游戏启发色彩</h3>
          <span class="component-number">v1.2-E</span>
        </div>

        <div class="card">
          <div class="mb-4">
            <h4 class="font-mono text-sm font-bold text-text-primary mb-2">扩展色彩系统</h4>
            <p class="font-sans text-sm text-text-secondary leading-relaxed">
              v1.2 新增游戏 UI 启发的红黑白配色，用于强调行动和增强空间感。
            </p>
          </div>

          <!-- Color Swatches -->
          <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
            <div class="color-swatch-demo">
              <div class="color-block-demo" style="background: #FF3B4A;"></div>
              <div class="color-info-demo">
                <div class="font-mono text-xs font-bold text-text-primary">Game Red</div>
                <div class="font-mono text-2xs text-text-muted">#FF3B4A</div>
                <div class="font-sans text-2xs text-text-secondary mt-1">主行动色</div>
              </div>
            </div>

            <div class="color-swatch-demo">
              <div class="color-block-demo" style="background: #FF1493;"></div>
              <div class="color-info-demo">
                <div class="font-mono text-xs font-bold text-text-primary">Game Pink</div>
                <div class="font-mono text-2xs text-text-muted">#FF1493</div>
                <div class="font-sans text-2xs text-text-secondary mt-1">NEW 标签</div>
              </div>
            </div>

            <div class="color-swatch-demo">
              <div class="color-block-demo" style="background: #0A0A0A;"></div>
              <div class="color-info-demo">
                <div class="font-mono text-xs font-bold text-text-primary">Game Black</div>
                <div class="font-mono text-2xs text-text-muted">#0A0A0A</div>
                <div class="font-sans text-2xs text-text-secondary mt-1">深色面板</div>
              </div>
            </div>

            <div class="color-swatch-demo">
              <div class="color-block-demo" style="background: #FAFAFA;"></div>
              <div class="color-info-demo">
                <div class="font-mono text-xs font-bold text-text-primary">Game White</div>
                <div class="font-mono text-2xs text-text-muted">#FAFAFA</div>
                <div class="font-sans text-2xs text-text-secondary mt-1">建筑白</div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Summary -->
      <div class="card-focus">
        <h3 class="font-mono text-base font-bold text-text-primary mb-4 flex items-center gap-2">
          <Icon icon="material-symbols:info" width="20" height="20" class="text-accent" />
          v1.2 设计哲学
        </h3>
        <div class="font-sans text-sm text-text-secondary leading-relaxed space-y-3">
          <p>
            <strong class="text-text-primary">不是创建游戏 UI 变体</strong>，而是让游戏 UI 的优秀元素（空间感、深度、3D 美学、红色行动召唤）融入整个 Industrial Avant UI 设计系统。
          </p>
          <p>
            v1.2 = v1.1（Brutalism + Avant-Garde + Glass）+ <strong class="text-accent">Spatial Narrative（空间叙事）</strong> + <strong class="text-accent">Isometric Aesthetics（3D 等角）</strong>
          </p>
          <div class="mt-4 pt-4 border-t border-border">
            <div class="flex items-center gap-2">
              <Icon icon="material-symbols:check-circle" width="16" height="16" class="text-status-success" />
              <span class="font-mono text-xs font-bold text-text-primary uppercase">100% 向后兼容</span>
            </div>
            <div class="font-sans text-xs text-text-muted mt-1">
              所有 v1.1 组件保持可用，v1.2 作为增强层，可渐进式迁移。
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- ═══ Forms Demo ═══ -->
    <div v-if="currentTab === 'forms'" class="space-y-6">
      <h2 class="font-mono text-xl font-bold text-text-primary mb-4">表单系统</h2>

      <div class="card">
        <h3 class="font-mono text-sm font-bold text-text-primary mb-4">输入框 & 选择器</h3>
        <div class="space-y-4">
          <div>
            <label class="mono-label block mb-2">普通输入框（Inter 字体）</label>
            <input class="input" placeholder="输入客户姓名..." />
          </div>
          <div>
            <label class="mono-label block mb-2">代码输入框（Space Mono 字体）</label>
            <input class="input-mono" placeholder="DEVICE-20260712-001" />
          </div>
          <div>
            <label class="mono-label block mb-2">下拉选择</label>
            <select class="select">
              <option>选项 1</option>
              <option>选项 2</option>
              <option>选项 3</option>
            </select>
          </div>
          <div>
            <label class="mono-label block mb-2">文本域</label>
            <textarea class="textarea" rows="3" placeholder="输入备注信息..."></textarea>
          </div>
        </div>
        <div class="mt-4 pt-4 border-t border-border">
          <p class="font-sans text-xs text-text-muted">
            规则：8px 圆角 + bg-field 背景 + 焦点时 2px accent 边框
          </p>
        </div>
      </div>
    </div>

    <!-- ═══ Tables Demo ═══ -->
    <div v-if="currentTab === 'tables'" class="space-y-6">
      <h2 class="font-mono text-xl font-bold text-text-primary mb-4">数据表格</h2>

      <div class="card">
        <h3 class="font-mono text-sm font-bold text-text-primary mb-3">标准表格</h3>
        <table class="table w-full">
          <thead>
            <tr>
              <th class="table-th">#</th>
              <th class="table-th">客户名称</th>
              <th class="table-th">金额</th>
              <th class="table-th">状态</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="item in sampleData" :key="item.id" class="table-tr">
              <td class="table-td">{{ item.id }}</td>
              <td class="table-td">{{ item.name }}</td>
              <td class="table-td">¥{{ item.value }}</td>
              <td class="table-td">
                <div class="flex items-center gap-2">
                  <span
                    :class="{
                      'status-indicator-success': item.status === 'completed',
                      'status-indicator-warning': item.status === 'active',
                      'status-indicator-pending': item.status === 'pending',
                    }"
                  />
                  <span class="font-sans text-sm">
                    {{ item.status === 'completed' ? '已完成' : item.status === 'active' ? '进行中' : '待处理' }}
                  </span>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
        <div class="mt-4 pt-4 border-t border-border">
          <p class="font-sans text-xs text-text-muted">
            表头：Space Mono + uppercase + bg-elevated | 表体：Inter + 斑马条纹 | 无竖线分隔
          </p>
        </div>
      </div>
    </div>

    <!-- ═══ Status Indicators Demo ═══ -->
    <div v-if="currentTab === 'status'" class="space-y-6">
      <h2 class="font-mono text-xl font-bold text-text-primary mb-4">状态指示器（8×8px 方块）</h2>

      <div class="card">
        <h3 class="font-mono text-sm font-bold text-text-primary mb-4">5种状态</h3>
        <div class="space-y-3">
          <div class="flex items-center gap-3">
            <span class="status-indicator-success" />
            <span class="font-sans text-sm text-text-primary">已完成</span>
            <span class="font-sans text-xs text-text-muted ml-auto">实心绿色</span>
          </div>
          <div class="flex items-center gap-3">
            <span class="status-indicator-warning" />
            <span class="font-sans text-sm text-text-primary">进行中</span>
            <span class="font-sans text-xs text-text-muted ml-auto">半填充橙色</span>
          </div>
          <div class="flex items-center gap-3">
            <span class="status-indicator-pending" />
            <span class="font-sans text-sm text-text-primary">待处理</span>
            <span class="font-sans text-xs text-text-muted ml-auto">空心灰色</span>
          </div>
          <div class="flex items-center gap-3">
            <span class="status-indicator-error" />
            <span class="font-sans text-sm text-text-primary">已逾期</span>
            <span class="font-sans text-xs text-text-muted ml-auto">实心红色+条纹</span>
          </div>
          <div class="flex items-center gap-3">
            <span class="status-indicator-cancelled" />
            <span class="font-sans text-sm text-text-primary">已取消</span>
            <span class="font-sans text-xs text-text-muted ml-auto">空心+X</span>
          </div>
        </div>
        <div class="mt-4 pt-4 border-t border-border">
          <div class="flex items-start gap-2">
            <Icon icon="material-symbols:info" width="14" height="14" class="text-status-info shrink-0 mt-0.5" />
            <p class="font-sans text-xs text-text-muted leading-relaxed">
              规则：4px 微圆角（保留方块基本形）+ 必须配文字标签（色盲友好）+ 禁止圆形指示器
            </p>
          </div>
        </div>
      </div>

      <div class="card">
        <h3 class="font-mono text-sm font-bold text-text-primary mb-4">Badge 样式</h3>
        <div class="flex flex-wrap items-center gap-2">
          <span class="badge-success">成功</span>
          <span class="badge-warning">警告</span>
          <span class="badge-error">错误</span>
          <span class="badge-info">信息</span>
          <span class="badge-muted">默认</span>
          <span class="badge-admin">管理员</span>
          <span class="badge-staff">员工</span>
        </div>
      </div>
    </div>

    <!-- ═══ Typography Demo ═══ -->
    <div v-if="currentTab === 'typography'" class="space-y-8">
      <div class="flex items-center justify-between">
        <div>
          <h2 class="font-mono text-2xl font-bold text-text-primary mb-2">字体双系统</h2>
          <p class="font-sans text-sm text-text-secondary">Space Mono（结构）+ Inter（正文）</p>
        </div>
        <span class="badge-info">2 字体</span>
      </div>

      <!-- Space Mono -->
      <div class="card-emphasis">
        <div class="flex items-center gap-3 mb-4">
          <Icon icon="material-symbols:inventory-2" width="24" height="24" class="text-accent" />
          <h3 class="font-mono text-xl font-bold text-text-primary">Space Mono — 结构元素</h3>
        </div>
        <div class="space-y-6">
          <div>
            <div class="mono-label mb-3">标题层级</div>
            <div class="space-y-2">
              <div class="font-mono text-4xl font-bold text-text-primary">H1 — Hero Heading</div>
              <div class="font-mono text-3xl font-bold text-text-primary">H2 — Section Title</div>
              <div class="font-mono text-2xl font-bold text-text-primary">H3 — Subsection</div>
              <div class="font-mono text-xl font-bold text-text-primary">H4 — Component Title</div>
            </div>
          </div>
          <div class="divider" />
          <div>
            <div class="mono-label mb-3">数据展示</div>
            <div class="flex items-end gap-6">
              <div>
                <div class="font-mono text-hero font-bold text-accent leading-none">128</div>
                <div class="font-sans text-xs text-text-muted mt-2">Hero Number</div>
              </div>
              <div>
                <div class="font-mono text-kpi font-bold text-text-primary leading-none">24,890</div>
                <div class="font-sans text-xs text-text-muted mt-2">KPI</div>
              </div>
              <div>
                <div class="font-mono text-2xl font-bold text-text-primary leading-none">456</div>
                <div class="font-sans text-xs text-text-muted mt-2">Metric</div>
              </div>
            </div>
          </div>
          <div class="divider" />
          <div>
            <div class="mono-label mb-3">编号系统</div>
            <div class="flex flex-wrap items-center gap-3">
              <span class="module-number">MODULE-01</span>
              <span class="component-number">SYS_01</span>
              <span class="component-number">A-01</span>
              <span class="font-mono text-xs text-text-muted uppercase tracking-wider">CODE-001</span>
            </div>
          </div>
        </div>
        <div class="mt-6 pt-6 border-t border-border">
          <p class="font-sans text-sm text-text-secondary leading-relaxed">
            <strong class="text-text-primary">使用场景：</strong>标题、数据、编号、导航、代码、标签。
            等宽字体提供工业感和精确对齐。
          </p>
        </div>
      </div>

      <!-- Inter -->
      <div class="card">
        <div class="flex items-center gap-3 mb-4">
          <Icon icon="material-symbols:font-download" width="24" height="24" class="text-text-muted" />
          <h3 class="font-mono text-xl font-bold text-text-primary">Inter — 正文内容</h3>
        </div>
        <div class="space-y-4">
          <div>
            <div class="mono-label mb-3">段落排版</div>
            <p class="font-sans text-base text-text-primary leading-relaxed max-w-2xl">
              Inter 是一款为屏幕阅读优化的无衬线字体，由 Rasmus Andersson 设计。
              它具有出色的可读性，适合用于表格数据、表单标签和长段落描述文字。
              在 14px 大小下依然清晰易读，即使在低 DPI 屏幕上也能保持良好的显示效果。
            </p>
          </div>
          <div class="divider" />
          <div>
            <div class="mono-label mb-3">字重变化</div>
            <div class="space-y-2">
              <div class="font-sans text-base font-light text-text-secondary">Light 300 — 辅助信息</div>
              <div class="font-sans text-base font-normal text-text-primary">Regular 400 — 正文内容</div>
              <div class="font-sans text-base font-medium text-text-primary">Medium 500 — 次级标题</div>
              <div class="font-sans text-base font-semibold text-text-primary">Semibold 600 — 强调内容</div>
              <div class="font-sans text-base font-bold text-text-primary">Bold 700 — 重要信息</div>
            </div>
          </div>
          <div class="divider" />
          <div>
            <div class="mono-label mb-3">尺寸层级</div>
            <div class="space-y-2">
              <div class="font-sans text-xs text-text-muted">12px — Caption 说明文字</div>
              <div class="font-sans text-sm text-text-secondary">14px — Small 次要内容</div>
              <div class="font-sans text-base text-text-primary">16px — Body 正文（推荐）</div>
              <div class="font-sans text-lg text-text-primary">18px — Lead 引导段落</div>
            </div>
          </div>
        </div>
        <div class="mt-6 pt-6 border-t border-border">
          <p class="font-sans text-sm text-text-secondary leading-relaxed">
            <strong class="text-text-primary">使用场景：</strong>正文、表格、表单、描述、提示。
            正文最小 16px（移动端），行高 1.5-1.75，最大行宽 65-75ch。
          </p>
        </div>
      </div>

      <!-- Contrast Demo -->
      <div class="card bg-bg-elevated">
        <h3 class="font-mono text-sm font-bold text-text-primary mb-4 flex items-center gap-2">
          <Icon icon="material-symbols:error" width="16" height="16" class="text-status-warning" />
          常见错误
        </h3>
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div class="p-4 border border-status-error rounded-lg bg-status-error/5">
            <div class="flex items-center gap-2 mb-2">
              <Icon icon="material-symbols:close" width="16" height="16" class="text-status-error" />
              <span class="font-mono text-xs font-bold text-status-error uppercase">错误</span>
            </div>
            <p class="font-mono text-sm text-text-primary leading-relaxed">
              这是一段很长的正文内容，使用等宽字体会让阅读变得困难，
              字母间距不均匀，视觉疲劳度高。
            </p>
          </div>
          <div class="p-4 border border-status-success rounded-lg bg-status-success/5">
            <div class="flex items-center gap-2 mb-2">
              <Icon icon="material-symbols:check" width="16" height="16" class="text-status-success" />
              <span class="font-mono text-xs font-bold text-status-success uppercase">正确</span>
            </div>
            <p class="font-sans text-sm text-text-primary leading-relaxed">
              这是一段很长的正文内容，使用 Inter 字体具有良好的可读性，
              字母间距均匀，适合长时间阅读。
            </p>
          </div>
        </div>
      </div>
    </div>

    <!-- ═══ Glass Material Demo ═══ -->
    <div v-if="currentTab === 'glass'" class="space-y-8">
      <div class="flex items-center justify-between">
        <div>
          <h2 class="font-mono text-2xl font-bold text-text-primary mb-2">玻璃态材质</h2>
          <p class="font-sans text-sm text-text-secondary">Neo-brutalist Glass — 仅用于浮起的临时层</p>
        </div>
        <span class="badge-warning">谨慎使用</span>
      </div>

      <!-- Correct Usage -->
      <div class="card-emphasis">
        <h3 class="font-mono text-base font-bold text-text-primary mb-4 flex items-center gap-2">
          <Icon icon="material-symbols:check-circle" width="20" height="20" class="text-status-success" />
          正确用法 — 浮起的临时层
        </h3>
        <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
          <div>
            <div class="mono-label mb-3">导航栏示例</div>
            <div class="panel p-4">
              <div class="flex items-center justify-between">
                <div class="flex items-center gap-3">
                  <Icon icon="material-symbols:menu" width="20" height="20" class="text-text-primary" />
                  <span class="font-mono text-sm font-bold text-text-primary">Navigation</span>
                </div>
                <div class="flex items-center gap-2">
                  <button class="dash-tool-btn">
                    <Icon icon="material-symbols:search" width="16" height="16" />
                  </button>
                  <button class="dash-tool-btn">
                    <Icon icon="material-symbols:notifications" width="16" height="16" />
                  </button>
                </div>
              </div>
            </div>
            <p class="font-sans text-xs text-text-muted mt-2">24px 模糊 + 半透明背景</p>
          </div>
          <div>
            <div class="mono-label mb-3">浮动按钮组</div>
            <div class="flex gap-2">
              <button class="dash-tool-btn">
                <Icon icon="material-symbols:settings" width="16" height="16" />
              </button>
              <button class="dash-tool-btn">
                <Icon icon="material-symbols:help" width="16" height="16" />
              </button>
              <button class="dash-tool-btn">
                <Icon icon="material-symbols:person" width="16" height="16" />
              </button>
            </div>
            <p class="font-sans text-xs text-text-muted mt-2">Icon-only 控制按钮</p>
          </div>
        </div>
        <div class="mt-6 pt-6 border-t border-border">
          <div class="flex items-start gap-2">
            <Icon icon="material-symbols:info" width="14" height="14" class="text-status-info shrink-0 mt-0.5" />
            <p class="font-sans text-xs text-text-muted leading-relaxed">
              玻璃态适用场景：导航栏、侧边栏、下拉菜单、弹窗、通知条 — 这些"浮起来的临时表面"。
            </p>
          </div>
        </div>
      </div>

      <!-- Incorrect Usage -->
      <div class="card-alert">
        <h3 class="font-mono text-base font-bold text-text-primary mb-4 flex items-center gap-2">
          <Icon icon="material-symbols:close-circle" width="20" height="20" class="text-status-error" />
          错误用法 — 数据承载层
        </h3>
        <div class="space-y-4">
          <div class="panel p-4">
            <h4 class="font-mono text-sm font-bold text-text-primary mb-2">❌ 玻璃数据卡片</h4>
            <p class="font-sans text-sm text-text-secondary leading-relaxed">
              这是一张使用玻璃态的数据卡片。背景模糊会让文字对比度随背景波动，
              影响可读性。数据卡片、表格、图表容器必须使用不透明背景。
            </p>
          </div>
          <div class="flex items-start gap-3 p-4 bg-status-error/10 border border-status-error rounded-lg">
            <Icon icon="material-symbols:warning" width="20" height="20" class="text-status-error shrink-0" />
            <div>
              <div class="font-mono text-sm font-bold text-text-primary mb-1">为什么不能用？</div>
              <p class="font-sans text-sm text-text-secondary leading-relaxed">
                玻璃态会让文字对比度随背景波动，用户需要长时间阅读的数据内容会造成视觉疲劳。
                数据层是"信息本身的底层"，必须保持稳定和高对比度。
              </p>
            </div>
          </div>
        </div>
      </div>

      <!-- Technical Specs -->
      <div class="card">
        <h3 class="font-mono text-sm font-bold text-text-primary mb-4 uppercase tracking-wider">技术规范</h3>
        <div class="space-y-3 font-sans text-sm">
          <div class="flex items-start gap-3">
            <div class="w-3 h-3 rounded-sm bg-accent shrink-0 mt-1" />
            <div>
              <div class="font-mono text-xs font-bold text-text-primary mb-1">backdrop-filter</div>
              <div class="text-text-secondary">blur(24px) — 24px 高斯模糊</div>
            </div>
          </div>
          <div class="flex items-start gap-3">
            <div class="w-3 h-3 rounded-sm bg-accent shrink-0 mt-1" />
            <div>
              <div class="font-mono text-xs font-bold text-text-primary mb-1">background</div>
              <div class="text-text-secondary">rgba(22, 22, 24, 0.7) — 70% 不透明度</div>
            </div>
          </div>
          <div class="flex items-start gap-3">
            <div class="w-3 h-3 rounded-sm bg-accent shrink-0 mt-1" />
            <div>
              <div class="font-mono text-xs font-bold text-text-primary mb-1">border</div>
              <div class="text-text-secondary">1px solid rgba(255, 255, 255, 0.1) — 边缘高光</div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- ═══ Version Footer — 每页必需 ═══ -->
    <div class="version-footer">2026 — V1.05 — REV.DEMO</div>
  </div>
</template>

<style scoped>
/* ═══ Page Layout ═══ */
.page-root {
  padding: 2rem 1.5rem;
  max-width: 1400px;
  margin: 0 auto;
  min-height: 100vh;
}

/* ═══ Hero Section ═══ */
.hero-section {
  animation: fadeInUp 0.6s ease-out;
}

/* ═══ Category Buttons ═══ */
.category-btn {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.75rem 1rem;
  background: var(--bg-elevated);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  color: var(--text-secondary);
  cursor: pointer;
  transition: all var(--duration-micro);
  font-weight: 500;
}

.category-btn:hover {
  border-color: var(--border-hover);
  background: var(--bg-field);
  color: var(--text-primary);
  transform: translateX(2px);
}

.category-btn-active {
  border-color: var(--accent);
  background: var(--accent);
  color: white;
  box-shadow: var(--shadow-offset-accent);
}

.category-btn-active:hover {
  transform: translateY(1px);
}

/* ═══ Demo Wrappers ═══ */
.demo-wrapper {
  position: relative;
}

.demo-label {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 0.75rem;
  padding-bottom: 0.5rem;
  border-bottom: 1px solid var(--border-default);
}

.demo-label-inline {
  font-family: var(--font-mono);
  font-size: var(--font-size-caption);
  font-weight: 600;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.08em;
  display: block;
  margin-bottom: 0.5rem;
}

.demo-btn-group {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

/* ═══ Code Snippets ═══ */
.code-snippet {
  background: var(--bg-field);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  padding: 0.75rem 1rem;
  overflow-x: auto;
}

.code-snippet code {
  color: var(--text-primary);
  white-space: nowrap;
}

/* ═══ Principle Items ═══ */
.principle-item {
  padding: 1rem;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  transition: all var(--duration-micro);
}

.principle-item:hover {
  border-color: var(--border-hover);
  background: var(--bg-elevated);
  transform: translateY(-2px);
}

/* ═══ Animations ═══ */
@keyframes fadeInUp {
  from {
    opacity: 0;
    transform: translateY(20px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

/* ═══ Responsive ═══ */
@media (max-width: 768px) {
  .page-root {
    padding: 1.5rem 1rem;
  }

  .hero-section {
    margin-bottom: 2rem;
  }

  .category-btn {
    font-size: 0.75rem;
    padding: 0.625rem 0.75rem;
  }
}

/* ═══ Reduced Motion ═══ */
@media (prefers-reduced-motion: reduce) {
  .hero-section,
  .category-btn,
  .principle-item {
    animation: none;
    transition: none;
  }

  .category-btn:hover,
  .category-btn-active:hover,
  .principle-item:hover {
    transform: none;
  }
}

/* ═══ Focus States (Accessibility) ═══ */
.category-btn:focus-visible,
button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

/* ═══ Hover Effects ═══ */
.card.cursor-pointer {
  transition: all var(--duration-micro);
}

.card.cursor-pointer:hover {
  border-color: var(--accent);
  transform: translateY(-2px);
  box-shadow: var(--shadow-offset-hover);
}

.card.cursor-pointer:active {
  transform: translateY(0);
  box-shadow: var(--shadow-offset-default);
}

/* ═══ Tool Buttons (Glass) ═══ */
.dash-tool-btn {
  width: 36px;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--glass-bg);
  backdrop-filter: blur(var(--glass-blur));
  -webkit-backdrop-filter: blur(var(--glass-blur));
  border: var(--glass-border);
  border-radius: var(--radius-lg);
  color: var(--text-muted);
  cursor: pointer;
  transition: all var(--duration-micro);
}

.dash-tool-btn:hover {
  border-color: var(--border-hover);
  color: var(--text-primary);
  transform: translateY(-1px);
}

.dash-tool-btn:active {
  transform: translateY(0);
}

/* ═══ v1.2 NEW Components ═══ */

/* Badge NEW (v1.2) */
.badge-new {
  background: #FF1493;
  color: white;
  font-family: var(--font-mono);
  font-size: 10px;
  font-weight: 700;
  padding: 2px 8px;
  border-radius: var(--radius-lg);
  text-transform: uppercase;
  letter-spacing: 0.1em;
  box-shadow: 0 0 12px #FF1493;
  animation: pulse-glow 2s ease-in-out infinite;
}

@keyframes pulse-glow {
  0%, 100% {
    box-shadow: 0 0 8px #FF1493;
  }
  50% {
    box-shadow: 0 0 20px #FF1493;
  }
}

/* Demo Container */
.demo-container {
  background: var(--bg-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  padding: 2rem;
  min-height: 200px;
}

.demo-label-small {
  display: block;
  text-align: center;
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.08em;
  margin-top: 0.5rem;
}

/* Quick Action Panel Demo */
.quick-actions-panel-demo {
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-width: 140px;
  margin: 0 auto;
  transform: perspective(800px) rotateY(5deg);
  transform-style: preserve-3d;
}

.action-card-demo {
  width: 120px;
  height: 100px;
  background: #0A0A0A;
  border: 2px solid var(--border-default);
  border-radius: var(--radius-lg);
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  cursor: pointer;
  position: relative;
  transform: translateZ(20px);
  transition: all var(--duration-normal);
  box-shadow: 0 4px 8px rgba(0, 0, 0, 0.15);
}

.action-card-demo:hover {
  transform: translateZ(40px) scale(1.05);
  border-color: #FF3B4A;
  box-shadow: 0 20px 40px rgba(0, 0, 0, 0.3), 0 0 0 1px rgba(255, 255, 255, 0.05);
}

.action-card-demo--new {
  border-color: #FF1493;
}

.badge-new-demo {
  position: absolute;
  top: -8px;
  right: -8px;
  background: #FF1493;
  color: white;
  font-family: var(--font-mono);
  font-size: 10px;
  font-weight: 700;
  padding: 2px 8px;
  border-radius: var(--radius-lg);
  text-transform: uppercase;
  letter-spacing: 0.1em;
  box-shadow: 0 0 12px #FF1493;
}

.alert-indicator-demo {
  position: absolute;
  top: 8px;
  right: 8px;
  width: 24px;
  height: 24px;
  background: #FF3B4A;
  color: white;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  font-family: var(--font-mono);
  font-size: 10px;
  font-weight: 700;
  box-shadow: 0 0 8px #FF3B4A;
}

.action-label-demo {
  font-family: var(--font-mono);
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  text-align: center;
}

/* Mega CTA Button Demo */
.btn-mega-demo {
  position: relative;
  min-width: 240px;
  min-height: 120px;
  background: linear-gradient(135deg, #FF3B4A 0%, #CC2F3D 100%);
  border: 3px solid #FF3B4A;
  border-radius: var(--radius-xl);
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 20px 24px;
  cursor: pointer;
  overflow: hidden;
  transition: all var(--duration-normal);
  box-shadow:
    0 12px 24px rgba(0, 0, 0, 0.25),
    0 0 20px rgba(255, 59, 74, 0.3);
}

.btn-mega-demo:hover {
  transform: scale(1.05) translateY(-4px);
  box-shadow:
    0 20px 40px rgba(0, 0, 0, 0.3),
    0 0 40px rgba(255, 59, 74, 0.5);
}

.btn-mega-demo:active {
  transform: scale(1.02) translateY(-2px);
}

.btn-mega-icon-demo {
  flex-shrink: 0;
  color: white;
  filter: drop-shadow(0 2px 4px rgba(0, 0, 0, 0.3));
}

.btn-mega-content-demo {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.btn-mega-label-demo {
  font-family: var(--font-mono);
  font-size: 20px;
  font-weight: 700;
  color: white;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  text-shadow: 0 2px 4px rgba(0, 0, 0, 0.3);
}

.btn-mega-progress-demo {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.progress-text-demo {
  font-family: var(--font-mono);
  font-size: 14px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.9);
}

.progress-bar-demo {
  height: 6px;
  background: rgba(0, 0, 0, 0.3);
  border-radius: 3px;
  overflow: hidden;
}

.progress-fill-demo {
  height: 100%;
  background: white;
  border-radius: 3px;
  transition: width var(--duration-slow);
  box-shadow: 0 0 8px rgba(255, 255, 255, 0.5);
}

/* Notification Badge Demo */
.badge-demo-wrapper {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.5rem;
}

.notification-badge-demo {
  position: absolute;
  top: -4px;
  right: -4px;
  min-width: 20px;
  height: 20px;
  background: #FF3B4A;
  color: white;
  font-family: var(--font-mono);
  font-size: 11px;
  font-weight: 700;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 10px;
  padding: 0 6px;
  box-shadow:
    0 2px 4px rgba(0, 0, 0, 0.3),
    0 0 8px rgba(255, 59, 74, 0.5);
  animation: badge-appear 0.3s ease-out;
}

.notification-badge-demo--small {
  min-width: 16px;
  height: 16px;
  font-size: 9px;
  padding: 0 4px;
  border-radius: 8px;
  top: -2px;
  right: -2px;
}

.notification-badge-demo--large {
  min-width: 24px;
  height: 24px;
  font-size: 13px;
  padding: 0 8px;
  border-radius: 12px;
  top: -6px;
  right: -6px;
}

@keyframes badge-appear {
  0% {
    transform: scale(0);
    opacity: 0;
  }
  50% {
    transform: scale(1.2);
  }
  100% {
    transform: scale(1);
    opacity: 1;
  }
}

/* Shadow Demo */
.shadow-demo-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 1rem;
}

.shadow-demo-card {
  width: 120px;
  height: 80px;
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  display: flex;
  align-items: center;
  justify-content: center;
  transition: transform var(--duration-normal);
}

.shadow-demo-card:hover {
  transform: translateY(-4px);
}

.shadow-demo-label {
  font-family: var(--font-mono);
  font-size: 12px;
  font-weight: 600;
  color: var(--text-primary);
}

.shadow-demo-card--offset {
  box-shadow: 4px 4px 0px rgba(255,255,255,0.06);
}

.shadow-demo-card--soft2 {
  box-shadow: 0 4px 8px rgba(0, 0, 0, 0.15);
}

.shadow-demo-card--soft4 {
  box-shadow: 0 12px 24px rgba(0, 0, 0, 0.25);
}

.shadow-demo-card--elevated {
  box-shadow:
    0 8px 16px rgba(0, 0, 0, 0.2),
    0 2px 4px rgba(0, 0, 0, 0.1);
}

.shadow-demo-card--floating {
  box-shadow:
    0 20px 40px rgba(0, 0, 0, 0.3),
    0 0 0 1px rgba(255, 255, 255, 0.05);
}

.shadow-demo-card--glow {
  box-shadow:
    0 0 20px rgba(255, 59, 74, 0.4),
    0 0 40px rgba(255, 59, 74, 0.2);
  border-color: #FF3B4A;
}

/* Color Swatch Demo */
.color-swatch-demo {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.color-block-demo {
  width: 100%;
  height: 80px;
  border-radius: var(--radius-lg);
  border: 1px solid var(--border-default);
}

.color-info-demo {
  padding: 0.5rem;
}

/* Reduced Motion */
@media (prefers-reduced-motion: reduce) {
  .quick-actions-panel-demo {
    transform: none;
  }

  .action-card-demo {
    transform: none;
  }

  .action-card-demo:hover {
    transform: scale(1.02);
  }

  .badge-new {
    animation: none;
  }

  .btn-mega-demo:hover {
    transform: scale(1.02);
  }
}
</style>
