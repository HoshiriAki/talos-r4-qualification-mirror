#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP9Sp01Snapshot,
  validateP9Sp01Snapshot,
} from './check-r4-p9-sp01-deployment-profile.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP9Sp01Snapshot())
  mutate(snapshot)
  const errors = validateP9Sp01Snapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    errors.some(error => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  )
}

const baseline = validateP9Sp01Snapshot(collectP9Sp01Snapshot())
assert.deepEqual(baseline, [], `baseline must pass: ${baseline.join('; ')}`)

expectFailure(
  'backend listener drifts from the nginx upstream port',
  snapshot => {
    snapshot.productionCompose = snapshot.productionCompose.replace(
      'PORT: "8080"',
      'PORT: "3000"',
    )
  },
  'PORT: "8080"',
)

expectFailure(
  'PostgreSQL 18 volume regresses to the retired data subdirectory mount',
  snapshot => {
    snapshot.productionCompose = snapshot.productionCompose.replace(
      'pgdata:/var/lib/postgresql',
      'pgdata:/var/lib/postgresql/data',
    )
  },
  'pgdata:/var/lib/postgresql/data',
)

expectFailure(
  'production profile silently returns to SQLite',
  snapshot => {
    snapshot.productionCompose = snapshot.productionCompose.replace(
      'DB_BACKEND: postgres',
      'DB_BACKEND: sqlite',
    )
  },
  'DB_BACKEND: postgres',
)

expectFailure(
  'production HTTPS is disabled',
  snapshot => {
    snapshot.productionCompose = snapshot.productionCompose.replace(
      'PUBLIC_HTTPS: "true"',
      'PUBLIC_HTTPS: "false"',
    )
  },
  'PUBLIC_HTTPS: "true"',
)

expectFailure(
  'database password gets a convenience default',
  snapshot => {
    snapshot.productionCompose = snapshot.productionCompose.replace(
      '${DB_PASSWORD:?DB_PASSWORD is required}',
      '${DB_PASSWORD:-changeme}',
    )
  },
  'POSTGRES_PASSWORD',
)

expectFailure(
  'production compose build context drifts into deploy directory',
  snapshot => {
    snapshot.productionCompose = snapshot.productionCompose.replace(
      'context: ..',
      'context: .',
    )
  },
  'must not contain exact line: context: .',
)

expectFailure(
  'invalid deploy-local build context coexists with the required root context',
  snapshot => {
    snapshot.productionCompose = snapshot.productionCompose.replace(
      'context: ..',
      'context: ..\n      # mutation keeps the valid token but adds an invalid duplicate\n      context: .',
    )
  },
  'must not contain exact line: context: .',
)

expectFailure(
  'production nginx bind source resolves inside deploy directory',
  snapshot => {
    snapshot.productionCompose = snapshot.productionCompose.replace(
      '../nginx/production.conf:/etc/nginx/conf.d/default.conf:ro',
      './nginx/production.conf:/etc/nginx/conf.d/default.conf:ro',
    )
  },
  '../nginx/production.conf',
)

expectFailure(
  'trusted proxy boundary becomes internet-wide',
  snapshot => {
    snapshot.productionCompose = snapshot.productionCompose.replace(
      'TRUSTED_PROXY_CIDRS: 172.29.0.0/24',
      'TRUSTED_PROXY_CIDRS: 0.0.0.0/0',
    )
  },
  'TRUSTED_PROXY_CIDRS: 172.29.0.0/24',
)

expectFailure(
  'backend image drops Cargo.lock enforcement',
  snapshot => {
    snapshot.backendImage = snapshot.backendImage.replace(
      'cargo build --release --locked --no-default-features --features postgres',
      'cargo build --release --no-default-features --features postgres',
    )
  },
  '--locked',
)

expectFailure(
  'frontend image restores npm dependency resolution',
  snapshot => {
    snapshot.frontendImage = snapshot.frontendImage.replace(
      'pnpm install --frozen-lockfile --filter talos-frontend...',
      'npm ci',
    )
  },
  'frozen-lockfile',
)

expectFailure(
  'public XFF is appended instead of overwritten',
  snapshot => {
    snapshot.productionNginx = snapshot.productionNginx.replaceAll(
      'proxy_set_header X-Forwarded-For $remote_addr;',
      'proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;',
    )
  },
  'overwrite, not append',
)

expectFailure(
  'production TLS listener is removed',
  snapshot => {
    snapshot.productionNginx = snapshot.productionNginx.replace(
      'listen 443 ssl;',
      'listen 443;',
    )
  },
  'listen 443 ssl',
)

expectFailure(
  'monitoring image becomes floating latest',
  snapshot => {
    snapshot.productionCompose = snapshot.productionCompose.replace(
      'image: prom/prometheus:v3.14.0',
      'image: prom/prometheus:latest',
    )
  },
  'prom/prometheus:v3.14.0',
)

expectFailure(
  'SP01 executable Compose gate is removed from one Exact-Head phase',
  snapshot => {
    snapshot.exactHead = snapshot.exactHead.replace(
      'node scripts/check-r4-p9-sp01-compose-config.mjs',
      'node scripts/REMOVED-r4-p9-sp01-compose-config.mjs',
    )
  },
  'executable Compose gates once',
)

expectFailure(
  'SP01 structural gate is removed from one Exact-Head phase',
  snapshot => {
    snapshot.exactHead = snapshot.exactHead.replace(
      'node scripts/check-r4-p9-sp01-deployment-profile.mjs',
      'node scripts/REMOVED-r4-p9-sp01-deployment-profile.mjs',
    )
  },
  'Exact-Head must run SP01',
)

console.log('R4-P9-SP01 deployment profile mutation tests passed.')
