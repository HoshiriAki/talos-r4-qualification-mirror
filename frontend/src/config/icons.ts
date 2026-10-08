/**
 * Icon System Configuration
 *
 * Priority:
 * 1. Material Symbols Outlined (CSS, via Google Fonts) - 主要图标
 * 2. Line MD (Animated icons) - 动画图标（如主题切换）
 * 3. Fluent UI System Icons - 备选图标
 *
 * 使用方式:
 * <Icon :icon="icons.menu" /> 或 <Icon icon="material-symbols:menu" />
 */

export const iconPrefix = {
  material: 'material-symbols',  // 主要使用 Material Symbols
  lineMd: 'line-md',              // 动画图标
  fluent: 'fluent',               // 备选 Fluent UI
} as const

/**
 * 图标映射表 - 应用中使用的所有图标
 * 优先级: Material Symbols > Fluent UI
 */
export const icons = {
  // Navigation
  menu: 'material-symbols:menu',
  home: 'material-symbols:home',
  dashboard: 'material-symbols:dashboard',

  // Actions
  search: 'material-symbols:search',
  add: 'material-symbols:add',
  edit: 'material-symbols:edit',
  delete: 'material-symbols:delete',
  save: 'material-symbols:save',
  close: 'material-symbols:close',
  check: 'material-symbols:check',

  // UI Controls
  chevronDown: 'material-symbols:keyboard-arrow-down',
  chevronUp: 'material-symbols:keyboard-arrow-up',
  chevronLeft: 'material-symbols:keyboard-arrow-left',
  chevronRight: 'material-symbols:keyboard-arrow-right',
  expandMore: 'material-symbols:expand-more',

  // User & Profile
  user: 'material-symbols:person',
  users: 'material-symbols:group',
  settings: 'material-symbols:settings',
  logout: 'material-symbols:logout',

  // Communication
  bell: 'material-symbols:notifications',
  mail: 'material-symbols:mail',

  // Content
  file: 'material-symbols:description',
  folder: 'material-symbols:folder',
  image: 'material-symbols:image',
  attach: 'material-symbols:attach-file',

  // Status & Indicators
  info: 'material-symbols:info',
  warning: 'material-symbols:warning',
  error: 'material-symbols:error',
  success: 'material-symbols:check-circle',

  // Business
  calendar: 'material-symbols:calendar-today',
  clock: 'material-symbols:schedule',
  money: 'material-symbols:payments',
  receipt: 'material-symbols:receipt',

  // Device & Hardware
  device: 'material-symbols:devices',
  computer: 'material-symbols:computer',
  phone: 'material-symbols:phone-iphone',

  // Data & Analytics
  chart: 'material-symbols:bar-chart',
  analytics: 'material-symbols:analytics',
  trending: 'material-symbols:trending-up',

  // UI Theme (使用 Line MD 动画图标)
  moon: 'line-md:sunny-outline-to-moon-alt-loop-transition',  // 切换到暗色模式
  sun: 'line-md:moon-alt-to-sunny-outline-loop-transition',   // 切换到亮色模式
  moonAlt: 'line-md:moon-alt-filled-loop',                     // 月亮循环动画
  language: 'material-symbols:language',

  // More Actions
  moreVert: 'material-symbols:more-vert',
  moreHoriz: 'material-symbols:more-horiz',

  // System
  refresh: 'material-symbols:refresh',
  download: 'material-symbols:download',
  upload: 'material-symbols:upload',
  filter: 'material-symbols:filter-list',
  sort: 'material-symbols:sort',
} as const

/**
 * 备选图标映射（如果 Material Symbols 不可用）
 */
export const iconsFallback = {
  // Fluent UI fallbacks
  menu: 'fluent:navigation-24-regular',
  home: 'fluent:home-24-regular',
  dashboard: 'fluent:grid-24-regular',
  search: 'fluent:search-24-regular',
  add: 'fluent:add-24-regular',
  edit: 'fluent:edit-24-regular',
  delete: 'fluent:delete-24-regular',
  save: 'fluent:save-24-regular',
  close: 'fluent:dismiss-24-regular',
  check: 'fluent:checkmark-24-regular',
  user: 'fluent:person-24-regular',
  bell: 'fluent:alert-24-regular',
  moon: 'fluent:weather-moon-24-regular',
  sun: 'fluent:weather-sunny-24-regular',
  language: 'fluent:local-language-24-regular',
} as const

/**
 * 图标类型定义
 */
export type IconName = keyof typeof icons
export type IconValue = typeof icons[IconName]
