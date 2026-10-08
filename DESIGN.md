---
name: Talos Rental Management System
description: 相机设备租赁全链路管理系统 — 机械蓝图美学，工业级可靠性
colors:
  void-black: "#0D0D0F"
  blueprint-surface: "#161618"
  blueprint-raised: "#1E1E22"
  blueprint-field: "#252528"
  blueprint-muted: "#2A2A2E"
  machine-red: "#FF453A"
  machine-red-hover: "#FF6B5A"
  machine-red-deep: "#8B2520"
  signal-green: "#30D158"
  signal-amber: "#FF9F0A"
  signal-blue: "#64D2FF"
  ink-primary: "#FFFFFF"
  ink-secondary: "#A0A0A8"
  ink-muted: "#6A6A72"
  border-steel: "#2A2A2E"
  border-steel-bright: "#3D3D42"
typography:
  display:
    fontFamily: "Space Mono, Courier New, Microsoft YaHei, monospace"
    fontWeight: 700
    fontSize: "48px"
    lineHeight: 1.1
    letterSpacing: "-0.02em"
  headline:
    fontFamily: "Space Mono, Courier New, Microsoft YaHei, monospace"
    fontWeight: 700
    fontSize: "28px"
    lineHeight: 1.2
  title:
    fontFamily: "Space Mono, Courier New, Microsoft YaHei, monospace"
    fontWeight: 700
    fontSize: "16px"
    lineHeight: 1.4
    letterSpacing: "0.03em"
  body:
    fontFamily: "Inter, Microsoft YaHei, -apple-system, sans-serif"
    fontWeight: 400
    fontSize: "14px"
    lineHeight: 1.5
  label:
    fontFamily: "Space Mono, Courier New, Microsoft YaHei, monospace"
    fontWeight: 500
    fontSize: "11px"
    lineHeight: 1.4
    letterSpacing: "0.08em"
    textTransform: "uppercase"
  kpi:
    fontFamily: "Space Mono, Courier New, Microsoft YaHei, monospace"
    fontWeight: 700
    fontSize: "36px"
    lineHeight: 1.1
rounded:
  none: "0px"
  micro: "4px"
  input: "8px"
  button: "10px"
  card: "12px"
  squircle: "16px"
spacing:
  xs: "6px"
  sm: "12px"
  md: "16px"
  lg: "24px"
  xl: "48px"
components:
  button-primary:
    backgroundColor: "{colors.machine-red}"
    textColor: "#FFFFFF"
    rounded: "{rounded.button}"
    padding: "12px 20px"
  button-primary-hover:
    backgroundColor: "{colors.machine-red-hover}"
  button-secondary:
    backgroundColor: "transparent"
    textColor: "{colors.ink-primary}"
    rounded: "{rounded.button}"
    padding: "12px 20px"
  button-danger:
    backgroundColor: "{colors.machine-red-deep}"
    textColor: "{colors.machine-red-hover}"
    rounded: "{rounded.button}"
    padding: "12px 20px"
  card-standard:
    backgroundColor: "{colors.blueprint-surface}"
    rounded: "{rounded.card}"
    padding: "20px"
  input-default:
    backgroundColor: "{colors.blueprint-field}"
    textColor: "{colors.ink-primary}"
    rounded: "{rounded.input}"
    padding: "10px 12px"
---

# Design System: Talos

## 1. Overview

**Creative North Star: "The Mechanical Blueprint"**

Talos 不是一个"暗黑主题的 SaaS 后台"。它是一张活着的机械蓝图——冷却液管道的走向、伺服电机的接线图、数控机床的加工路径。功能裸露即是美，结构本身即是界面。

这个系统讲的是**精密度**，不是装饰。每一像素对应一个物理约束：一条 2px 边框是承重墙，一个硬偏移阴影是材料厚度，一个 8×8px 状态方块是信号指示灯。颜色只用六种暗色阶加一种强调红，因为在真正的蓝图里多余的色彩是噪声而不是信息。

字体分两路：Space Mono 是结构的骨骼——所有标题、数据、编号、导航都是等宽的，让眼睛在扫描页面时每一列的起点精确对齐。Inter 是信息的肌肉——正文、表格、表单需要可读性而不是结构感。这种双字体策略不是 stylistic choice，是功能需求。

运动正在从 120ms 机械节拍进化到更丰富的工业生命感：滚轮驱动的时间线展开、背景信号脉冲对真实网络请求的响应、模块交错的建造序列。不是装饰性"动画"，是系统状态的真实可视化。

**Key Characteristics:**
- 精密蓝图 — 暗色 6 级灰阶底座 + 单一红色强调，结构线暴露，编号系统贯穿
- 材料诚实 — 硬偏移阴影（0px 模糊），不透明卡片，玻璃仅用于浮起临时层
- 双字体引擎 — Space Mono 负责结构/数据/编号，Inter 负责正文/表格/表单
- 工业运动 — 正交微交互 + 滚动驱动叙事 + 网络脉冲可视化 + 5 种工业加载
- 通用 SaaS 模板的绝对反面 — 不用圆角过度装饰，不用柔和渐变，不用 Inter 灰调正文

## 2. Colors

色板像一张工程图纸的配色方案：大量暗色渐变区分层级，一种红色负责所有关键信号。

### Primary
- **Machine Red** (#FF453A): 唯一的强调色。用于主按钮背景、活跃边框、焦点环、链接、关键数据读数。这是系统中唯一的"热"信号——所有东西都是冷的，只有它发热。≤10% 的单屏面积。
- **Machine Red Hover** (#FF6B5A): 悬停态。比基础红色亮约 15%，模拟"按下前预亮"的机械反馈。
- **Machine Red Deep** (#8B2520): 弱化背景。用于选中的表格行背景、Danger 按钮填充、标签背景。是 Machine Red 的"冷却"版本。

### Neutral
- **Void Black** (#0D0D0F): 页面底板。最暗的一层——所有其他表面从它之上浮起。
- **Blueprint Surface** (#161618): 卡片、面板、侧边栏的标准背景。比底板亮一级，形成第一个深度台阶。
- **Blueprint Raised** (#1E1E22): 表格行悬停、次要浮层、对话框背景。第二个深度台阶。
- **Blueprint Field** (#252528): 输入框背景、代码块、选中区域。第三个深度台阶——也是"可编辑"的视觉信号。
- **Blueprint Muted** (#2A2A2E): 最亮的暗色——分割区域、特殊强调。极少使用。
- **Border Steel** (#2A2A2E): 1px 默认边框和分割线。与 Blueprint Muted 同色值——边框不需要额外的颜色。
- **Border Steel Bright** (#3D3D42): 2px 强调边框、活跃卡片、悬停边框。
- **Ink Primary** (#FFFFFF): 标题、关键数据。纯白——对比度 ~18:1 对底板。
- **Ink Secondary** (#A0A0A8): 正文、标签。对比度 ~6:1 对 surface。
- **Ink Muted** (#6A6A72): 占位符、禁用文字。对比度 ~3.5:1——仅用于非关键信息。

### Signal Colors
- **Signal Green** (#30D158): 已完成、正常状态。只在状态指示器和 Badge 中使用。
- **Signal Amber** (#FF9F0A): 逾期、待处理、警告。暖色但不与 Machine Red 混淆——红是交互，琥珀是状态。
- **Signal Blue** (#64D2FF): 中性提示、信息状态。
- **Signal Red** = Machine Red (#FF453A): 错误态与强调色共享——"错误"是系统中唯一会让 red 出现的状态异常。

### Named Rules
**The One Red Rule.** Machine Red 是系统唯一的暖色强调。它在任何单屏上占比不超过 10%。它的稀缺即是它的力量——当一个元素变红，用户知道这是关键操作、关键状态或关键数据。

**The Blueprint Depth Rule.** 暗色六阶不是装饰性的"暗色主题色板"。每一阶对应一个具体的结构角色：底板 → 卡片 → 悬停 → 输入 → 分隔 → 特殊。不允许跳过中间阶（例如，卡片直接放到底板上不能用 bg-field）。

## 3. Typography

**Display Font:** Space Mono (with Courier New, Microsoft YaHei fallback)
**Body Font:** Inter (with Microsoft YaHei, -apple-system fallback)
**Label/Mono Font:** Space Mono — 也用于导航、数据、编号、代码

**Character:** Space Mono 的等宽骨架给页面提供了精确的水平对齐——表格列、KPI 数字、导航标签全部落在同一个网格上。Inter 负责所有需要持续阅读的内容——正文、表格数据行、表单标签和描述。这不是"好看的字体配对"，这是功能分工——一个负责结构，一个负责信息。

### Hierarchy
- **Display** (700, 48px / 1.1): Hero 锚点数字或标题。每页一个。可以更大（clamp 到 120px）用于核心数据点。letter-spacing: -0.02em。
- **Headline** (700, 28px / 1.2): 页面主标题。MODULE 编号右侧。
- **Title** (700, 16px / 1.4, letter-spacing 0.03em): 卡片标题。固定在卡片顶部的分割线上方。
- **KPI** (700, 36px / 1.1): 核心指标读数。通常位于 Dashboard Hero 位置。与 Body 的 14px 形成 2.57:1 的比例跳跃。
- **Body** (400, 14px / 1.5): 正文、表格内容、表单文字。最大行宽 75ch。
- **Label** (500, 11px / 1.4, uppercase, letter-spacing 0.08em): 全大写标签、编号、版本标识、模块号。Space Mono。

### Named Rules
**The Structural-Reading Split.** Space Mono 元素告诉眼睛"这里有结构信息"（标题、数据、编号）。Inter 元素告诉眼睛"这里需要持续阅读"（表格行、描述、表单）。两者永远不互换。如果需要判断：能在一个固定的列网格里对齐 → Space Mono；需要线性阅读 → Inter。

**The No-Orphan-Number Rule.** 任何数字读数（价格、设备数、日期）放在 Space Mono 中，等宽保证数字不会因为字符宽度差异导致垂直列对不齐。

## 4. Elevation

Talos 采用**三通道深度系统**——不依靠传统的模糊阴影来区分层级。

### 通道 1: 色阶对比
六层暗色阶形成从底板到输入框的连续抬升：`#0D0D0F` → `#161618` → `#1E1E22` → `#252528` → `#2A2A2E`。每层亮约 8-10%，视觉上像一个均匀的台阶。

### 通道 2: 边框粗细
默认 1px（`border-steel`）标记标准容器。悬停/活跃态使用 2px（`border-steel-bright` 或 `machine-red`）。边框加粗 = 元素被激活。

### 通道 3: 硬偏移阴影
```
Standard:  4px 4px 0px var(--border-steel-bright)
Emphasis:  4px 4px 0px var(--machine-red)
Hover:     6px 6px 0px var(--machine-red)
Dialog:    8px 8px 0px var(--border-steel-bright)
```
模糊值永远是 `0px`——这是机械厚度，不是光学阴影。偏移方向固定为右下（X+ Y+），模拟"从底板被顶起"的物理反馈。

### 玻璃浮层 (第四通道 — 仅用于临时表面)
导航栏、侧边栏、弹窗、下拉菜单、通知条使用 55-65% 不透明度的玻璃态 + 24px 背景模糊。顶部边缘有一条 1px 高光渐变模拟玻璃边缘光线。数据卡片、表格、图表容器**永远不使用**玻璃——信息承载层保持不透明纯色。

### Named Rules
**The Flat-By-Default Rule.** 页面静止时所有元素都在同一个平面上。三通道深度仅作为对交互的响应出现（hover → border 变粗 + shadow 偏移 + bg 抬升）。没有装饰性的"浮动卡片"。

**The Glass-Is-Temporary Rule.** `backdrop-filter: blur()` 只能出现在生命短暂的、浮在内容之上的元素上（导航、弹窗、通知）。这些元素会消失——玻璃是"临时的"视觉隐喻。持久的信息承载层（卡片、表格）必须是不透明的。

## 5. Components

### Buttons
**Character:** 按钮像机器面板上的开关——方角（10px 微圆）、2px 边框、等宽大写字体、按下时向下平移 1px。
- **Shape:** 10px 圆角 (`--radius-lg`)，高度 36px，最小宽度 36px
- **Primary:** 填充 `machine-red`，白色文字，2px `machine-red` 边框。悬停：背景变 `machine-red-hover`，阴影从 4px→6px 偏移。按下：translateY(1px)，80ms。
- **Secondary:** 透明背景，`ink-primary` 文字，2px `border-steel` 边框。悬停：边框变 `machine-red`。按下：同 Primary。
- **Danger:** 填充 `machine-red-deep`，`machine-red-hover` 文字，2px `machine-red` 边框，右上角对角警示条纹。悬停：边框亮起，阴影偏移增大。
- **Ghost:** 无背景无边框，`ink-secondary` 文字，8px 圆角。悬停：文字变 `ink-primary`，背景微亮，下划线出现。

### Cards
**Character:** 不透明纯色容器——像蓝图上的一个标注区域。12px 圆角 + 1px 边框 + 硬偏移阴影。
- **Standard:** `blueprint-surface` 背景，1px `border-steel`，12px 圆角，20px 内边距，`shadow-offset-default`。顶部有 MODULE-ID 标签 + 标题 + 1px 分割线。
- **Emphasis:** 2px `border-steel-bright` 边框，`shadow-offset-accent`。可选左侧 3px `machine-red` 竖条。
- **Focus:** `blueprint-raised` 背景，2px `machine-red` 边框，`shadow-offset-accent`。用于最重要的单一数据点。
- **Alert:** 2px `signal-amber` 边框，`shadow-offset-default`，右上角对角警示条纹角标。
- **Hover:** 标准卡悬停时边框变 `border-steel-bright`，阴影偏移增大至 6px 6px 0px `machine-red`。

### Input Fields
**Character:** 暗色凹槽——内凹于底板之上。8px 圆角 + 1px 边框默认，聚焦时扩张为 2px。
- **Default:** `blueprint-field` 背景，1px `border-steel`，8px 圆角，38px 高度，12px 水平内边距。
- **Focus:** 边框变 2px `machine-red`，背景不变。文字颜色 `ink-primary`。
- **Error:** 边框变 2px `machine-red`（与聚焦共享颜色），下方出现 Inter 12px `machine-red` 错误提示。
- **Disabled:** 整体 opacity 0.35，cursor not-allowed。
- **Mono variant:** 等宽字体（Space Mono）用于订单号、设备号等固定宽度 ID。

### Data Tables
**Character:** 工业规格表——Space Mono 表头全大写 + Inter 数据行，斑马纹交替，无竖线分隔。
- **Header:** `blueprint-raised` 背景，Space Mono 11px 全大写，`ink-secondary`，加粗，1px `border-steel` 底边。
- **Rows:** Inter 14px（或 Space Mono 13px 用于 ID/编号列），`blueprint-surface` / `void-black` 交替斑马纹。
- **Hover:** 行背景变 `blueprint-raised`。
- **No vertical dividers:** 列通过文字对齐分隔——ID 左对齐（等宽保证对齐），数字右对齐，文字左对齐。

### Status Indicators
**Character:** 8×8px 微圆角方块，不是圆点。形状 + 颜色 + 文字三重编码，色盲友好。
- **Completed (●):** `signal-green` 实心填充，4px 微圆角
- **In Progress (◐):** `signal-amber` 半填充（左半填充，右半透明）+ 1px 边框
- **Pending (○):** 透明空心，1px `ink-muted` 边框
- **Overdue (◆):** `machine-red` 实心 + 45° 对角暗纹条纹
- **Cancelled (▣):** 空心 + 1px `ink-muted` 边框 + 内 X 标记

### Navigation
- **Sidebar:** 220px 固定宽，玻璃态背景（55% opacity + 24px blur + 1px 玻璃边框 + 顶部边缘高光）。导航项 Space Mono 14px，活跃项左侧 2px `machine-red` 竖条。
- **Header:** 48px 高，玻璃态背景。导航 Tabs 在玻璃 pill 容器内——12px 圆角，Inter 13px，活跃 Tab 反转填充（`ink-primary` 背景 + `void-black` 文字）。
- **Tab Bar:** 玻璃 pill 容器，36px 高，12px 圆角。Tab 项目 7px 高、10px 圆角，活跃态反转填充。

### Loading States
**Character:** 五位一体工业加载系统——永不使用 Spinner。
- **CadLoading:** 边框脉冲 + 扫描高亮扫过 + 数字淡入（1.6s 循环，ease-in-out）
- **WireframeLoading:** 立方体面/边脉冲 + 节点发光（2s 循环）
- **AssemblyLoading:** 模块依次从下往上滑入（0.25s stagger，ease-out）
- **ScanLoading:** 水平扫描线从上到下扫描（2s 循环）
- **PipelineLoading:** 强调色光点沿正交管道巡游 + 节点发光（2.5s 循环）

### Glass Elements
玻璃仅用于浮起临时层。实现：`background: rgba(22,22,24,0.55)` + `backdrop-filter: blur(24px)` + `1px solid rgba(255,255,255,0.08)` + 顶部 1px 高光渐变。
- ✅ 允许: 导航栏、侧边栏、弹窗/Dialog、下拉菜单、通知 Toast、ConfirmPopup
- ❌ 禁止: 数据卡片、表格、图表容器、正文区域、表单

## 6. Do's and Don'ts

### Do:
- **Do** 使用 CSS 自定义属性作为所有颜色的唯一来源（`var(--bg-surface)` 而不是 `#161618`）
- **Do** 使用 UnoCSS shortcuts（`btn-primary`, `card`, `input`, `panel`）而不是裸 utility 组合
- **Do** Space Mono 用于结构文本（标题、数据、导航、编号、代码、ID）
- **Do** Inter 用于阅读文本（正文、表格内容、表单标签、描述）
- **Do** 使用硬偏移阴影（第三个参数永远是 0px）
- **Do** 状态指示器用 8×8px 方块 + 文字标签，不单独依靠颜色
- **Do** 每页一个 MODULE-NN 编号 + 主结构线 + 页脚版本标识
- **Do** 12 列不对称网格——Hero 卡片占 6-8 列，次要卡片 2-4 列。禁用等分（3:3:3:3, 6:6）
- **Do** 保留 `prefers-reduced-motion` 支持——所有动画降级为 0ms 瞬时切换
- **Do** 5 种工业加载动画替代传统 Spinner——CAD / Wireframe / Assembly / Scan / Pipeline

### Don't:
- **Don't** 在数据卡片、表格、图表容器上使用玻璃/backdrop-filter——它们是信息承载层，不是浮层
- **Don't** 使用模糊阴影（`box-shadow` 第三个参数 > 0）
- **Don't** 使用 `border-radius: 50%` 在任何元素上——头像用 16px squircle，状态指示器用 4px 微圆角方块
- **Don't** 在色块上使用渐变填充——所有实色必须是平坦的（玻璃边缘高光渐变属材质效果，不违反此条）
- **Don't** 使用 raw hex 颜色——永远通过 CSS 自定义属性引用
- **Don't** 使用 Emoji 作为图标——用 @iconify/vue + Lucide
- **Don't** 让 Space Mono 出现在正文段落中——Space Mono = 结构，Inter = 阅读
- **Don't** 使用通用 SaaS 后台模板风格——圆角卡片海洋、柔和渐变、Inter 灰调正文、无边框浮动面板
- **Don't** 在页面中制造过度装饰的运动——每个动画必须传达系统状态（加载、数据刷新、页面切换、交互反馈）
- **Don't** 制造"看起来像 AI 生成的"界面——不要同一尺寸的卡片网格、不要 eyebrow 全大写标签叠在每个 section 上方、不要梯度文字、不要装饰性玻璃
