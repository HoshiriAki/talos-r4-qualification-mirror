#!/usr/bin/env node

import { spawnSync } from 'node:child_process'

const composeArgs = ['compose', '-f', 'deploy/compose.production.yml', 'config']
const qualificationEnv = {
  ...process.env,
  DB_PASSWORD: 'p9-sp01-not-a-production-secret',
  TALOS_PRODUCTION_DATABASE_URL: 'postgres://talos:p9-sp01-not-a-production-secret@db:5432/talos',
  CORS_ALLOWED_ORIGIN: 'https://p9-sp01.invalid',
  TLS_CERT_FILE: '/tmp/p9-sp01-tls.crt',
  TLS_KEY_FILE: '/tmp/p9-sp01-tls.key',
  GRAFANA_USER: 'p9-sp01-admin',
  GRAFANA_PASSWORD: 'p9-sp01-not-a-production-secret',
  METRICS_SCRAPE_TOKEN_FILE: '/dev/null',
}

function run(env) {
  return spawnSync('docker', composeArgs, {
    cwd: process.cwd(),
    env,
    encoding: 'utf8',
  })
}

const baseline = run(qualificationEnv)
if (baseline.error) {
  console.error(`docker compose is unavailable: ${baseline.error.message}`)
  process.exit(1)
}
if (baseline.status !== 0) {
  process.stderr.write(baseline.stderr || '')
  process.stdout.write(baseline.stdout || '')
  console.error('P9-SP01 production Compose config must render with explicit qualification inputs')
  process.exit(1)
}

for (const required of ['DB_PASSWORD', 'CORS_ALLOWED_ORIGIN']) {
  const env = { ...qualificationEnv }
  delete env[required]
  const result = run(env)
  if (result.status === 0) {
    console.error(`P9-SP01 production Compose must fail closed when ${required} is missing`)
    process.exit(1)
  }
}

console.log('R4-P9-SP01 executable Compose config check passed.')
