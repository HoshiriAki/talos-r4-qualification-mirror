import { fileURLToPath, URL } from 'node:url'
import { readFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { defineConfig, type Plugin } from 'vite'
import vue from '@vitejs/plugin-vue'
import UnoCSS from 'unocss/vite'
import Components from 'unplugin-vue-components/vite'
import { PrimeVueResolver } from '@primevue/auto-import-resolver'

function loadSsl() {
  try {
    const certDir = fileURLToPath(new URL('../certs', import.meta.url))
    return {
      key: readFileSync(`${certDir}/localhost.key`),
      cert: readFileSync(`${certDir}/localhost.crt`),
    }
  } catch { return undefined }
}

// Proxy target: env override → SSL-aware default → plain HTTP fallback
// VITE_BACKEND_TARGET is set by start.ps1 when backend runs on a non-default port
const ssl = loadSsl()
const backendTarget = process.env.VITE_BACKEND_TARGET
  || (ssl ? 'https://127.0.0.1:3000' : 'http://127.0.0.1:3000')
const proxyConfig = Object.fromEntries(
  ['/auth', '/users', '/devices', '/audit-logs', '/tenant-memberships', '/api', '/meta'].map(p => [
    p, {
      target: backendTarget,
      secure: false,
      configure(proxy: any) {
        proxy.on('proxyReq', (_proxyReq: any, _req: any) => {
          _proxyReq.setHeader('x-forwarded-by', 'vite')
        })
      },
    }
  ])
)

// ── HTTP→HTTPS redirect plugin ─────────────────────────────────────
// When SSL is enabled, starts a tiny HTTP server on HTTP_FRONTEND_PORT
// (default 80) that 301-redirects all requests to the HTTPS dev server.
// Set HTTP_FRONTEND_PORT=0 to disable.
function httpRedirectPlugin(): Plugin | null {
  if (!ssl) return null
  const httpPort = (() => {
    const v = process.env.HTTP_FRONTEND_PORT
    if (v === undefined || v === '') return 80
    const n = Number(v)
    return Number.isFinite(n) && n >= 0 ? n : 80
  })()
  if (httpPort <= 0) return null

  let httpServer: ReturnType<typeof createServer> | null = null

  return {
    name: 'http-redirect',
    configureServer(server) {
      const { host, port } = server.config.server
      const targetHost = (host === true || host === '0.0.0.0' || host === undefined) ? 'localhost' : String(host)
      const targetPort = port ?? 5173

      httpServer = createServer((_req, res) => {
        const tlsPort = targetPort !== 443 ? `:${targetPort}` : ''
        res.writeHead(301, { Location: `https://${targetHost}${tlsPort}${_req.url}` })
        res.end()
      }).listen(httpPort, () => {
        console.log(`  HTTP→HTTPS 重定向：http://localhost:${httpPort} → https://${targetHost}:${targetPort}`)
      })
    },
    closeBundle() {
      if (httpServer) { httpServer.close(); httpServer = null }
    },
  }
}

// ── PrimeVue pre-bundle ─────────────────────────────────────────────
// Every PrimeVue component used anywhere in the app MUST be listed here
// so Vite pre-bundles it upfront. Otherwise the first navigation to a
// page using a new component triggers "new dependencies optimized"
// followed by a full page reload that loses the route.
//
// RULE: when adding `import X from 'primevue/<name>'` or `<Xxx>` in a
// template, add `'primevue/<name>'` to this array immediately.
// ────────────────────────────────────────────────────────────────────
const primevueDeps = [
  // Auto-imported by PrimeVueResolver (template-only tags — no explicit import)
  'primevue/badge',
  'primevue/button',
  'primevue/card',
  'primevue/checkbox',
  'primevue/dataview',
  'primevue/datepicker',
  'primevue/dialog',
  'primevue/inputnumber',
  'primevue/inputswitch',
  'primevue/inputtext',
  'primevue/overlaypanel',
  'primevue/paginator',
  'primevue/select',
  'primevue/selectbutton',
  'primevue/slider',
  'primevue/skeleton',
  'primevue/toggleswitch',
  'primevue/tag',
  'primevue/textarea',
  // Explicitly imported in <script setup> blocks
  'primevue/accordion',
  'primevue/tabview',
  'primevue/tabpanel',
  'primevue/tabs',
  'primevue/tablist',
  'primevue/tabpanels',
  'primevue/tab',
  'primevue/accordionpanel',
  'primevue/column',
  'primevue/datatable',
  'primevue/timeline',
  'primevue/toast',
  'primevue/confirmdialog',
  'primevue/config',
  'primevue/toastservice',
  'primevue/confirmationservice',
  'primevue/usetoast',
  'primevue/useconfirm',
]

export default defineConfig({
  plugins: [
    vue(),
    UnoCSS(),
    Components({
      resolvers: [PrimeVueResolver()]
    }),
    httpRedirectPlugin(),
  ].filter(Boolean) as Plugin[],
  optimizeDeps: {
    include: primevueDeps,
  },
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url))
    }
  },
  server: {
    host: '0.0.0.0',
    port: 5173,
    https: ssl,
    proxy: proxyConfig,
    strictPort: false,
  },
  build: {
    outDir: '../public',
    emptyOutDir: true,
  }
})
