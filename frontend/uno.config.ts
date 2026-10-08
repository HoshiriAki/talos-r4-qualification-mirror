import { defineConfig, presetWind3 } from 'unocss'

export default defineConfig({
  presets: [
    presetWind3(),
  ],
  rules: [
    // Industrial Avant duration utilities (smooth fluid transitions)
    ['duration-instant', { 'transition-duration': 'var(--duration-instant)' }],
    ['duration-press', { 'transition-duration': 'var(--duration-press)' }],
    ['duration-micro', { 'transition-duration': 'var(--duration-micro)' }],
    ['duration-fade', { 'transition-duration': 'var(--duration-fade)' }],
    ['duration-normal', { 'transition-duration': 'var(--duration-normal)' }],
    ['duration-slow', { 'transition-duration': 'var(--duration-slow)' }],
    ['duration-page', { 'transition-duration': 'var(--duration-page)' }],
    // Glass (v1.1) — 仅用于浮起临时层
    ['glass', {
      'background': 'var(--glass-bg)',
      'backdrop-filter': 'blur(var(--glass-blur))',
      '-webkit-backdrop-filter': 'blur(var(--glass-blur))',
      'border': 'var(--glass-border)',
      'box-shadow': 'var(--shadow-offset-default)',
    }],
  ],
  theme: {
    colors: {
      base: 'var(--bg-base)',
      surface: 'var(--bg-surface)',
      'surface-hover': 'var(--bg-elevated)',
      'surface-raised': 'var(--bg-elevated)',
      elevated: 'var(--bg-elevated)',
      input: 'var(--bg-field)',
      field: 'var(--bg-field)',
      border: {
        DEFAULT: 'var(--border-base)',
        base: 'var(--border-base)',
        strong: 'var(--border-strong)',
        accent: 'var(--border-accent)',
        // 向后兼容别名
        hover: 'var(--border-strong)',
        active: 'var(--border-accent)',
        focus: 'var(--border-accent)',
      },
      text: {
        primary: 'var(--text-primary)',
        secondary: 'var(--text-secondary)',
        tertiary: 'var(--text-tertiary)',
        accent: 'var(--text-accent)',
        inverse: 'var(--text-inverse)',
        // 向后兼容别名
        muted: 'var(--text-tertiary)',
      },
      btn: {
        primary: 'var(--btn-primary-bg)',
        'primary-text': 'var(--btn-primary-text)',
        secondary: 'var(--btn-secondary-bg)',
        'secondary-text': 'var(--btn-secondary-text)',
        danger: 'var(--btn-danger-bg)',
        'danger-text': 'var(--btn-danger-text)',
      },
      status: {
        success: 'var(--status-success)',
        warning: 'var(--status-warning)',
        error: 'var(--status-error)',
        info: 'var(--status-info)',
      },
      accent: {
        DEFAULT: 'var(--accent)',
        hover: 'var(--accent-hover)',
        muted: 'var(--accent-muted)',
      },
    },
    fontFamily: {
      mono: "'Space Mono', 'Courier New', 'Microsoft YaHei', monospace",
      sans: "'Inter', 'Microsoft YaHei', -apple-system, sans-serif",
    },
    fontSize: {
      caption: ['11px', '1.2'],
      '2xs': ['11px', '1.2'],
      xs: ['12px', '1.2'],
      sm: ['13px', '1.5'],
      base: ['14px', '1.5'],
      md: ['15px', '1.5'],
      lg: ['16px', '1.5'],
      xl: ['18px', '1.2'],
      '2xl': ['22px', '1.2'],
      '3xl': ['28px', '1.2'],
      kpi: ['36px', '1.1'],
      hero: ['48px', '1.1'],
    },
    borderRadius: {
      none: '0px',
      sm: '4px',    // 状态指示器、图表柱形/图例方块（微圆角）
      md: '8px',    // 输入框、选择器、Ghost/Text 按钮
      lg: '10px',   // 按钮、Badge
      xl: '12px',   // 卡片、模态框/Dialog
      full: '16px', // 头像（Squircle）
    },
    transitionDuration: {
      instant: '0ms',
      press: '80ms',
      micro: '120ms',
      fade: '150ms',
      normal: '200ms',
      slow: '300ms',
      page: '480ms',
    },
  },
  shortcuts: {
    // ── Layout ──────────────────────────────────────────────────────
    'layout-grid': 'grid grid-cols-[1fr] lg:grid-cols-[248px_1fr] h-screen',
    'sidebar': 'flex flex-col h-full overflow-hidden fixed lg:static inset-y-0 left-0 z-30 w-[260px] lg:w-full transition-transform duration-300',
    'main-content': 'flex-1 overflow-hidden flex flex-col lg:ml-0',
    'content-area': 'flex-1 overflow-auto px-4',

    // ── Sidebar navigation ──────────────────────────────────────────
    'sidebar-nav': 'flex flex-col gap-0.5 p-3',
    'sidebar-link': 'flex items-center gap-3 px-3 py-2.5 text-sm font-mono text-text-secondary no-underline hover:text-text-primary hover:bg-elevated transition-all duration-micro border border-transparent rounded-lg',
    'sidebar-link-active': 'text-text-primary bg-elevated shadow-[inset_2px_0_0_var(--text-primary)]',

    // ── Sidebar groups ──────────────────────────────────────────────
    'sidebar-group': 'mb-1',
    'sidebar-group-header': 'flex items-center gap-2 w-full px-3 py-2 text-left font-mono text-caption tracking-wider uppercase text-text-tertiary hover:text-text-primary cursor-pointer border border-transparent hover:bg-elevated transition-colors duration-instant select-none',
    'sidebar-group-chevron': 'inline-block w-3 text-center leading-none transition-transform duration-micro text-text-tertiary',
    'sidebar-group-label': 'font-bold',
    'sidebar-group-ns': 'ml-auto text-text-tertiary/50 truncate hidden lg:block',
    'sidebar-group-items': 'flex flex-col gap-0.5',

    // ── Panels ──────────────────────────────────────────────────────
    'panel': 'glass p-4',           /* 玻璃浮层面板 — 自动含边缘高光 */
    'panel-solid': 'bg-surface border border-border p-5 rounded-xl shadow-[var(--shadow-offset-default)]', /* 不透明卡片面板 */
    'panel-title': 'font-mono font-bold text-lg text-text-primary tracking-wider mb-3',

    // ── Buttons — 4 级系统 ──────────────────────────────────────────
    'btn': 'px-5 py-2.5 font-mono text-sm font-600 tracking-wider uppercase cursor-pointer transition-all duration-micro border-2 select-none disabled:opacity-35 disabled:cursor-not-allowed h-9 min-w-9 rounded-[10px]',
    'btn-primary': 'btn bg-accent text-white border-accent hover:bg-accent-hover hover:border-accent-hover active:translate-y-0.5 shadow-[var(--shadow-offset-accent)] hover:shadow-[var(--shadow-offset-hover)]',
    'btn-secondary': 'btn bg-transparent text-text-primary border-border hover:border-accent active:translate-y-0.5 shadow-[var(--shadow-offset-default)]',
    'btn-danger': 'btn bg-accent-muted text-accent-hover border-accent hover:border-accent active:translate-y-0.5 shadow-[var(--shadow-offset-accent)]',
    'btn-ghost': 'bg-transparent border border-transparent text-text-secondary hover:text-text-primary hover:bg-surface-hover cursor-pointer transition-colors duration-micro font-mono text-sm px-3 py-1.5 rounded-lg',

    // ── Form inputs ─────────────────────────────────────────────────
    'input': 'bg-field border border-border px-3 py-2.5 font-mono text-sm text-text-primary w-full outline-none focus:border-2 focus:border-accent transition-colors duration-micro rounded-lg',
    'input-mono': 'input font-mono tracking-wider',
    'textarea': 'input resize-y min-h-[60px]',
    'select': 'input appearance-none cursor-pointer',

    // ── Table ───────────────────────────────────────────────────────
    'table': 'w-full border-collapse text-sm',
    'table-th': 'text-left px-5 py-3.5 text-text-tertiary font-mono font-500 text-xs tracking-wider uppercase border-b border-border bg-elevated',
    'table-td': 'px-5 py-3.5 border-b border-border text-text-secondary font-sans text-sm',
    'table-tr': 'hover:bg-elevated transition-colors duration-micro',

    // ── Status badges — 方块指示器 ──────────────────────────────────
    'badge': 'inline-block px-2.5 py-1 text-xs font-mono font-bold border transition-colors duration-instant rounded-[10px]',
    'badge-success': 'badge border-status-success text-status-success',
    'badge-warning': 'badge border-status-warning text-status-warning',
    'badge-error': 'badge border-status-error text-status-error',
    'badge-info': 'badge border-status-info text-status-info',
    'badge-muted': 'badge border-text-muted text-text-muted',
    'badge-admin': 'badge border-accent text-accent',
    'badge-staff': 'badge border-status-info text-status-info',

    // ── Tab bar — 选项卡栏（玻璃 pill 容器）──────────────────────────
    'tab-bar': 'flex items-center gap-1.5 p-1 glass rounded-lg',
    'tab-item': 'inline-flex items-center h-7 px-3.5 font-sans text-sm font-500 text-text-tertiary no-underline rounded-md transition-colors duration-micro cursor-pointer',
    'tab-item-hover': 'hover:text-text-primary',
    'tab-item-active': 'bg-[var(--text-primary)] text-[var(--bg-base)] font-600',

    // ── Cards — 4 级卡片系统 (v1.1: 不透明 bg + 12px 圆角 + 偏移阴影) ─
    'card': 'bg-surface border border-border p-5 rounded-xl shadow-[var(--shadow-offset-default)]',
    'card-emphasis': 'bg-surface border-2 border-[var(--border-hover)] p-5 rounded-xl shadow-[var(--shadow-offset-accent)]',
    'card-focus': 'bg-elevated border-2 border-accent p-6 rounded-xl shadow-[var(--shadow-offset-accent)]',
    'card-alert': 'bg-surface border-2 border-status-warning p-5 rounded-xl shadow-[var(--shadow-offset-default)]',
    'card-hover': 'bg-surface border border-border hover:border-border-hover hover:shadow-[var(--shadow-offset-hover)] transition-all duration-micro p-5 rounded-xl shadow-[var(--shadow-offset-default)]',

    // ── Modals/Overlays ─────────────────────────────────────────────
    'overlay': 'fixed inset-0 bg-base z-40 flex items-center justify-center',
    'dialog': 'bg-surface border-2 border-border p-6 max-w-lg w-full mx-4 z-50 rounded-xl',
    'dialog-title': 'font-mono font-bold text-lg text-text-primary mb-4 tracking-wider',

    // ── Calendar ────────────────────────────────────────────────────
    'calendar-grid': 'grid grid-cols-7 gap-0.5',
    'calendar-day': 'min-h-[34px] border border-border flex items-center justify-center cursor-pointer font-mono text-xs hover:border-hover hover:text-text-primary transition-all duration-micro text-text-secondary',
    'calendar-day-active': 'border-accent text-text-primary bg-accent-muted',
    'calendar-day-selected': 'border-accent text-white bg-accent font-bold',
    'calendar-day-in-range': 'bg-surface-hover border-hover text-text-primary',
    'calendar-header': 'text-center font-mono text-caption tracking-wider uppercase text-text-muted py-1',
    'calendar-label': 'text-center font-mono text-caption tracking-wider uppercase text-text-muted',

    // ── Toast — 无阴影 ──────────────────────────────────────────────
    'toast': 'fixed bottom-6 right-6 z-50 bg-surface border-2 border-border px-4 py-3 font-mono text-sm text-text-primary rounded-xl',

    // ── Misc ────────────────────────────────────────────────────────
    'divider': 'border-t border-border my-2',
    'tooltip': 'absolute bg-elevated border border-border px-2 py-1 text-xs font-mono text-text-secondary whitespace-nowrap z-50 rounded-md',
    'mono-label': 'font-mono text-xs tracking-wider uppercase text-text-tertiary',
    'heading': 'font-mono font-bold',

    // ── Structure line — 贯穿全页的结构线 ────────────────────────────
    'structure-line': 'w-full border-t-2 border-border-hover',
    'structure-line-accent': 'w-full border-t-2 border-accent opacity-60',
    'guide-line': 'w-full border-t border-dashed border-text-muted/40',

    // ── Module numbering ────────────────────────────────────────────
    'module-number': 'font-mono text-caption tracking-wider uppercase text-text-tertiary',
    'component-number': 'font-mono text-2xs tracking-wider uppercase text-text-tertiary',

    // ── Status indicators — 8×8px 方块，4px 微圆角 ─────────────────
    'status-indicator': 'inline-block w-2 h-2 rounded-sm flex-shrink-0',
    'status-indicator-success': 'status-indicator bg-status-success',
    'status-indicator-warning': 'status-indicator bg-status-warning',
    'status-indicator-pending': 'status-indicator border border-text-tertiary bg-transparent',
    'status-indicator-error': 'status-indicator bg-status-error',
    'status-indicator-cancelled': 'status-indicator border border-text-tertiary bg-transparent',
    // Legacy aliases
    'status-dot': 'status-indicator',
    'status-dot-success': 'status-indicator-success',
    'status-dot-warning': 'status-indicator-warning',
    'status-dot-error': 'status-indicator-error',
    'status-dot-info': 'status-indicator bg-status-info',
    'status-dot-muted': 'status-indicator-pending',

    // ── Hero anchor — 超大字号视觉锚点 ────────────────────────────
    'hero-number': 'font-mono text-hero font-bold leading-tight tracking-tight',
    'hero-kpi': 'font-mono text-kpi font-bold leading-tight',

    // ── Version footer — YYYY — VN.NN — REV.N ────────────────────
    'version-footer': 'font-mono text-caption tracking-wider text-text-tertiary mt-6 pt-4 border-t border-border',

    // ── Status pill — 状态药丸（表格内使用）─────────────────────────
    'status-pill': 'inline-flex items-center gap-1.5 text-xs px-2.5 py-1 rounded-md font-sans',
    'status-pill-done': 'status-pill text-text-secondary bg-elevated',
    'status-pill-progress': 'status-pill text-text-primary bg-elevated',
    'status-pill-overdue': 'status-pill text-accent bg-accent-muted',

    // ── Work / HUD mode switching (v1.2) ─────────────────────────────
    'mode-switch': 'inline-flex items-center gap-1 p-1 bg-elevated border border-border rounded-btn font-mono text-xs',
    'hud-rail': 'fixed inset-x-3 top-3 z-40 flex items-center justify-between border border-border rounded-xl bg-[var(--glass-bg)] backdrop-blur-md',
    'hud-dock': 'fixed left-1/2 bottom-4 z-40 -translate-x-1/2 flex items-center gap-2 border border-border rounded-xl bg-[var(--glass-bg)] backdrop-blur-md',
    'work-panel': 'bg-surface border border-border rounded-xl shadow-[var(--shadow-offset-default)]',
  },
})
