import { ref, computed, onUnmounted } from 'vue'

export type ThemeMode = 'dark' | 'light'

/** HTML-escape for chart tooltips — prevents XSS via user-supplied data fields */
export function escapeHtml(str: string): string {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

// ── Terminal Brutalist Chart Color Palettes ─────────────────────────────────
// Dark: vibrant on #161618 background — refined saturation
// Light: deeper/saturated on #F4F4F6 background (matches base.css custom properties)

export const DARK_PALETTE = {
  blue:     '#64D2FF',   // Industrial Avant info
  green:    '#30D158',   // Industrial Avant success
  yellow:   '#FF9F0A',   // Industrial Avant warning
  red:      '#FF453A',   // Industrial Avant accent/error
  purple:   '#BF5AF2',   // kept distinct
  teal:     '#64D2FF',   // = info (teal pulls from info)
  orange:   '#FF9F0A',   // = warning
  gray:     '#6A6A72',   // text-muted
  darkGray: '#2A2A2E',   // border-default
  bgFill:   'rgba(22,22,24,0.6)',
  grid:     '#2A2A2E',  // --border-default — solid instrument grid
  gridStrong: '#2A2A2E',
  tooltipBg: '#161618',      // bg-base — deeper industrial tooltip
  tooltipBorder: '#2A2A2E',  // border-default
  tooltipText: '#FFFFFF',    // text-primary
  marker:    '#FF453A',      // accent
} as const

export const LIGHT_PALETTE = {
  blue:     '#1A80CC',   // Industrial Avant info (light)
  green:    '#1A8C3A',   // Industrial Avant success (light)
  yellow:   '#CC8000',   // Industrial Avant warning (light)
  red:      '#FF453A',   // Industrial Avant accent (same in both themes)
  purple:   '#8E3ABF',
  teal:     '#1A80CC',
  orange:   '#CC8000',
  gray:     '#7A7A78',
  darkGray: '#D4D4D2',
  bgFill:   'rgba(240,239,237,0.6)',
  grid:     '#D4D4D2',  // solid instrument grid for light theme
  gridStrong: '#D4D4D2',
  tooltipBg: '#EDECEA',      // bg-base for light — industrial tooltip
  tooltipBorder: '#D4D4D2',  // border-default
  tooltipText: '#0D0D0F',
  marker:    '#FF453A',
} as const

export interface ChartPalette {
  blue: string; green: string; yellow: string; red: string
  purple: string; teal: string; orange: string; gray: string
  darkGray: string; bgFill: string; grid: string
  gridStrong: string; tooltipBg: string; tooltipBorder: string
  tooltipText: string; marker: string
}

// Multi-series palettes (8 colors each)
export const DARK_SERIES = [
  DARK_PALETTE.blue,
  DARK_PALETTE.green,
  DARK_PALETTE.yellow,
  DARK_PALETTE.red,
  DARK_PALETTE.purple,
  DARK_PALETTE.teal,
  DARK_PALETTE.orange,
  DARK_PALETTE.gray,
] as const

export const LIGHT_SERIES = [
  LIGHT_PALETTE.blue,
  LIGHT_PALETTE.green,
  LIGHT_PALETTE.yellow,
  LIGHT_PALETTE.red,
  LIGHT_PALETTE.purple,
  LIGHT_PALETTE.teal,
  LIGHT_PALETTE.orange,
  LIGHT_PALETTE.gray,
] as const

// Device status colors (semantic — matches base.css status vars)
export const DARK_STATUS = {
  '已入库':   DARK_PALETTE.green,
  '租赁中':   DARK_PALETTE.blue,
  '返厂维修': DARK_PALETTE.yellow,
  '确认丢失': DARK_PALETTE.red,
  '已报废':   DARK_PALETTE.gray,
} as const

export const LIGHT_STATUS = {
  '已入库':   LIGHT_PALETTE.green,
  '租赁中':   LIGHT_PALETTE.blue,
  '返厂维修': LIGHT_PALETTE.yellow,
  '确认丢失': LIGHT_PALETTE.red,
  '已报废':   LIGHT_PALETTE.gray,
} as const

// Sequential palette for ranking charts (8 stops, blue family)
export const DARK_RANKING = ['#FF453A', '#FF6B5A', '#E03D32', '#C0302A', '#A02822', '#801F1B', '#601713', '#400F0C'] as const
export const LIGHT_RANKING = ['#FF453A', '#E03D32', '#C0302A', '#A02822', '#801F1B', '#601713', '#400F0C', '#200505'] as const

// ── Singleton theme detection ──────────────────────────────────────────────

let _mode: ReturnType<typeof ref<ThemeMode>> | null = null
let _observer: MutationObserver | null = null
let _refCount = 0

function detectTheme(): ThemeMode {
  if (typeof document === 'undefined') return 'dark'
  return document.documentElement.getAttribute('data-theme') === 'light' ? 'light' : 'dark'
}

function ensureObserver() {
  if (_observer || typeof document === 'undefined') return
  _observer = new MutationObserver(() => {
    const next = detectTheme()
    if (_mode && _mode.value !== next) {
      _mode.value = next
    }
  })
  _observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['data-theme'],
  })
}

function teardownObserver() {
  if (_refCount > 0) return
  _observer?.disconnect()
  _observer = null
  _mode = null
}

/**
 * Singleton composable — provides reactive theme mode, palettes, and G2 theme.
 * All chart components share one MutationObserver on <html data-theme>.
 */
export function useChartTheme() {
  if (!_mode) {
    _mode = ref(detectTheme())
  }
  _refCount++
  ensureObserver()

  const mode = _mode!
  const isDark = computed(() => mode.value === 'dark')
  const palettes = computed<ChartPalette>(() => mode.value === 'dark' ? DARK_PALETTE as unknown as ChartPalette : LIGHT_PALETTE as unknown as ChartPalette)
  const series = computed((): string[] => mode.value === 'dark' ? [...DARK_SERIES] : [...LIGHT_SERIES])
  const statusColors = computed((): Record<string, string> => mode.value === 'dark' ? { ...DARK_STATUS } : { ...LIGHT_STATUS })
  const ranking = computed((): string[] => mode.value === 'dark' ? [...DARK_RANKING] : [...LIGHT_RANKING])
  // Industrial Avant G2 v5 theme — oscilloscope/signal trace, not business chart
  const g2Theme = computed(() =>
    mode.value === 'dark'
      ? {
          type: 'classicDark',
          category10: DARK_SERIES,
          view: { viewFill: 'transparent' },
          line: { lineWidth: 2, lineCap: 'round' },
          point: { size: 4, shape: 'square', style: { lineWidth: 0 } },
          area: { style: { fillOpacity: 0.08 } },
          axis: {
            title: false,
            labelFontSize: 10,
            labelFontFamily: 'Space Mono, monospace',
            labelFill: '#6A6A72',
            gridStroke: '#2A2A2E',
            gridLineWidth: 0.5,
            lineStroke: '#2A2A2E',
            tickStroke: '#2A2A2E',
          },
          legend: {
            itemLabelFontSize: 10,
            itemLabelFontFamily: 'Space Mono, monospace',
            itemLabelFill: '#A0A0A8',
            itemMarker: 'square',
            itemMarkerSize: 8,
          },
        } as const
      : 'light' as const,
  )

  onUnmounted(() => {
    _refCount--
    teardownObserver()
  })

  return { mode, isDark, palettes, series, statusColors, ranking, g2Theme }
}
