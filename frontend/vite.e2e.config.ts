// Vite config for Playwright e2e — forces plain HTTP on a fixed port and
// disables the HTTP→HTTPS redirect plugin so the test harness never needs
// TLS. Everything else (aliases, proxy, PrimeVue pre-bundling) is inherited.
import { defineConfig } from 'vite'
import baseConfig from './vite.config'

const plugins = (baseConfig.plugins ?? []).filter((plugin) => plugin?.name !== 'http-redirect')

export default defineConfig({
  ...baseConfig,
  plugins,
  server: {
    ...(baseConfig.server ?? {}),
    https: false,
    host: '127.0.0.1',
    port: Number.parseInt(process.env.UI_LAB_E2E_PORT ?? '5199', 10),
    strictPort: true,
  },
  preview: {
    port: 5198,
    strictPort: true,
  },
})
