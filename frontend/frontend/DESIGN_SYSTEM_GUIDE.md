# Industrial Avant UI Design System — 应用指南

**版本：** v1.1  
**项目：** Talos Rental Management System  
**设计语言：** Brutalism（结构） + Avant-Garde（运动） + Modern SaaS（可用性） + Neo-brutalist Glass（材质）

---

## 快速开始

### 1. 色彩使用

**✅ 正确：**
```vue
<div class="bg-surface text-text-primary border border-border">
  <span class="text-accent">重点内容</span>
</div>
```

**❌ 错误：**
```vue
<div style="background: #161618; color: #FFFFFF; border: 1px solid #2A2A2E">
  <span style="color: #FF453A">重点内容</span>
</div>
```

**规则：** 永远使用 CSS 变量 token，禁止原始 hex 颜色。

---

### 2. 卡片系统（4级）

#### 标准卡片
```vue
<div class="card">
  <div class="panel-title">SYS_01 · 标题</div>
  <p class="font-sans text-sm">卡片内容使用 Inter 字体...</p>
</div>
```
- 12px 圆角
- 不透明 `bg-surface`
- 硬偏移阴影 `shadow-offset-default`
- **禁止玻璃态** — 卡片是信息承载层，不是浮层

#### 强调卡片
```vue
<div class="card-emphasis">
  <div class="panel-title">重要指标</div>
  <div class="hero-kpi">24,890</div>
</div>
```
- 2px 边框 + accent 阴影

#### 焦点卡片
```vue
<div class="card-focus">
  <div class="module-number">A-01</div>
  <div class="hero-number">128</div>
</div>
```
- 高对比反转
- accent 边框 + 阴影

#### 警示卡片
```vue
<div class="card-alert">
  <Icon icon="lucide:alert-triangle" />
  <span>逾期警告</span>
</div>
```
- warning 边框
- 可选对角条纹角标

---

### 3. 按钮系统

```vue
<!-- Primary: 主要操作 -->
<button class="btn-primary">确认订单</button>

<!-- Secondary: 次要操作 -->
<button class="btn-secondary">取消</button>

<!-- Danger: 危险操作 -->
<button class="btn-danger">删除</button>

<!-- Ghost: 低权重操作 -->
<button class="btn-ghost">更多选项 →</button>
```

**规则：**
- Primary/Secondary/Danger = 10px 圆角
- Ghost/Text = 8px 圆角
- 所有按钮：Space Mono + uppercase + 2px 边框
- 悬停：`translateY(2px)` 机械按压感

---

### 4. 字体双系统

| 元素 | 字体 | 示例 |
|------|------|------|
| 标题、数据、编号、导航 | **Space Mono** | `class="font-mono"` |
| 正文、表格、表单、描述 | **Inter** | `class="font-sans"` |

```vue
<!-- ✅ 正确 -->
<div class="card">
  <h3 class="font-mono text-lg font-bold">MODULE-01</h3>
  <p class="font-sans text-sm">这是正文内容...</p>
  <div class="font-mono text-kpi">12,480</div>
</div>

<!-- ❌ 错误：正文用等宽字体 -->
<p class="font-mono text-sm">这段长文字很难读...</p>
```

---

### 5. 状态指示器（8×8px 方块）

```vue
<template>
  <div class="flex items-center gap-2">
    <span class="status-indicator-success" />
    <span class="font-sans text-sm">已完成</span>
  </div>
</template>
```

**5种状态：**
- `status-indicator-success` — 实心绿色（已完成）
- `status-indicator-warning` — 半填充橙色（进行中）
- `status-indicator-pending` — 空心灰色（待处理）
- `status-indicator-error` — 实心红色+条纹（错误）
- `status-indicator-cancelled` — 空心+X（已取消）

**规则：**
- 4px 微圆角（保留方块基本形）
- **必须配文字标签** — 色盲友好
- 禁止 `border-radius: 50%` 圆形指示器

---

### 6. 表格

```vue
<table class="table">
  <thead>
    <tr>
      <th class="table-th">#</th>
      <th class="table-th">名称</th>
      <th class="table-th">状态</th>
    </tr>
  </thead>
  <tbody>
    <tr class="table-tr">
      <td class="table-td">1</td>
      <td class="table-td">张三</td>
      <td class="table-td">
        <span class="status-indicator-success" />
        <span>已完成</span>
      </td>
    </tr>
  </tbody>
</table>
```

**规则：**
- 表头：Space Mono + uppercase + `bg-elevated`
- 表体：Inter + 斑马条纹
- **无竖线** — 靠对齐区分列
- 悬停行：`bg-elevated`

---

### 7. 输入框 & 选择器

```vue
<!-- 普通输入框 -->
<input class="input" placeholder="输入内容..." />

<!-- 代码/序列号输入框 -->
<input class="input-mono" placeholder="设备序列号..." />

<!-- 文本域 -->
<textarea class="textarea" placeholder="备注..."></textarea>

<!-- 下拉选择 -->
<select class="select">
  <option>选项 1</option>
</select>
```

**规则：**
- 8px 圆角
- `bg-field` 背景
- 焦点：2px accent 边框
- 正文输入用 Inter，代码/ID 用 Space Mono

---

### 8. 玻璃态（仅浮层）

```vue
<!-- ✅ 正确：导航栏、侧边栏、弹窗 -->
<nav class="panel">...</nav>
<aside class="panel">...</aside>
<Dialog class="panel">...</Dialog>

<!-- ❌ 错误：数据卡片不能用玻璃 -->
<div class="panel">  <!-- 错误！应该用 card -->
  <table>...</table>
</div>
```

**玻璃适用：**
- ✅ 导航栏、侧边栏、下拉菜单、弹窗、通知条
- ❌ 数据卡片、表格、图表容器、正文区域

**为什么：** 玻璃会让文字对比度随背景波动，影响可读性。

---

### 9. 页面必备元素

每个页面必须包含：

```vue
<template>
  <div class="page-root">
    <!-- 1. MODULE-NN bar（必需） -->
    <div class="module-bar mb-6">
      <span class="module-number-label">MODULE-03</span>
      <div class="structure-line" />
      <span class="module-page-label">设备管理</span>
    </div>

    <!-- 2. Hero 锚点（每页一个大视觉元素） -->
    <div class="card mb-6">
      <div class="hero-number">12,480</div>
      <div class="font-sans text-sm text-text-muted">当前活跃订单数</div>
    </div>

    <!-- 3. 内容区域 -->
    <div class="grid grid-cols-12 gap-4">
      <!-- 使用不对称布局：8:4、6:3:2:1，禁止 3:3:3:3 -->
      <div class="col-span-8">...</div>
      <div class="col-span-4">...</div>
    </div>

    <!-- 4. Version footer（必需） -->
    <div class="version-footer">2026 — V1.05 — REV.3</div>
  </div>
</template>
```

---

### 10. 运动语言

**微交互（120ms）：**
```vue
<button class="btn-primary">
  <!-- 悬停：边框变 accent + translateX(2px) -->
</button>
```

**页面过渡（360ms）：**
```vue
<router-view v-slot="{ Component }">
  <Transition name="page-fade-slide" mode="out-in">
    <component :is="Component" />
  </Transition>
</router-view>
```

**加载动画（禁用 Spinner）：**
```vue
<CadLoading />  <!-- CAD 蓝图绘制加载 -->
```

---

### 11. 常见错误 & 修正

#### 错误 1：原始 hex 颜色
```vue
<!-- ❌ -->
<div style="color: #FF453A">错误</div>

<!-- ✅ -->
<div class="text-accent">正确</div>
```

#### 错误 2：卡片用玻璃
```vue
<!-- ❌ -->
<div class="panel">
  <table>数据表格</table>
</div>

<!-- ✅ -->
<div class="card">
  <table class="table">数据表格</table>
</div>
```

#### 错误 3：圆形状态点
```vue
<!-- ❌ -->
<span class="w-2 h-2 rounded-full bg-green-500" />

<!-- ✅ -->
<span class="status-indicator-success" />
<span class="font-sans text-sm">已完成</span>
```

#### 错误 4：正文用等宽字体
```vue
<!-- ❌ -->
<p class="font-mono">这是一段很长的正文...</p>

<!-- ✅ -->
<p class="font-sans">这是一段很长的正文...</p>
```

#### 错误 5：模糊阴影
```vue
<!-- ❌ -->
<div style="box-shadow: 0 4px 12px rgba(0,0,0,0.3)">

<!-- ✅ -->
<div class="card">  <!-- 自动应用 shadow-offset-default -->
```

---

## 检查清单

在提交代码前，检查：

- [ ] 无原始 hex 颜色（grep `#[0-9a-fA-F]{6}`）
- [ ] 数据卡片不用玻璃（grep `panel` in data cards）
- [ ] 状态指示器不是圆形（grep `rounded-full`）
- [ ] 正文用 Inter，标题/数据用 Space Mono
- [ ] 所有阴影都是硬偏移（无 blur 参数）
- [ ] 页面有 MODULE-NN bar + Hero 锚点
- [ ] 按钮用 10px/8px 圆角（不是 0px 或其他值）

---

## 参考文件

- **Token 定义：** `frontend/src/assets/styles/base.css`
- **UnoCSS 快捷方式：** `frontend/uno.config.ts`
- **完整规范：** `.claude/skills/talos-design-system/industrial-avant-ui-complete.md`
- **组件语法：** `.claude/skills/talos-design-system/component-grammar.json`
- **Token JSON：** `.claude/skills/talos-design-system/design-tokens.json`

---

**v1.1 核心变更：**
- 圆角系统：8-16px（实用主义妥协）
- 阴影系统：硬偏移 + 纯色（替代零阴影）
- 玻璃态：仅浮层，数据层保持不透明
- 页面过渡：允许 fade+slide（放宽 v1.0 纯正交限制）
