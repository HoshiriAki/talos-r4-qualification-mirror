#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  backendImage: 'Dockerfile',
  frontendImage: 'Dockerfile.frontend',
  productionCompose: 'deploy/compose.production.yml',
  productionNginx: 'nginx/production.conf',
  dockerignore: '.dockerignore',
  evidence: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r4-p9-sp01-deployment-profile.md',
  exactHead: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectP9Sp01Snapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

function requireTokens(errors, source, label, tokens) {
  for (const token of tokens) {
    if (!source.includes(token)) {
      errors.push(`${label} missing: ${token}`)
    }
  }
}

export function validateP9Sp01Snapshot(snapshot) {
  const errors = []

  requireTokens(errors, snapshot.backendImage, 'Backend image invariant', [
    'FROM rust:1.98.1-slim-bookworm AS builder',
    'cargo build --release --locked --no-default-features --features postgres',
    'FROM debian:bookworm-20260918-slim',
    'USER talos',
    'ENV PUBLIC_DIR=/app/public',
  ])
  for (const forbidden of [
    'ENV DATABASE_URL=',
    'COPY public/',
    'cargo build --release --features postgres',
  ]) {
    if (snapshot.backendImage.includes(forbidden)) {
      errors.push(`Backend image must not contain: ${forbidden}`)
    }
  }

  requireTokens(errors, snapshot.frontendImage, 'Frontend image invariant', [
    'FROM node:24.21.0-slim AS builder',
    'corepack prepare pnpm@10.33.4 --activate',
    'COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./',
    'COPY patches/ patches/',
    'pnpm install --frozen-lockfile --filter talos-frontend...',
    'pnpm --dir frontend build',
    'FROM nginx:1.31.5-trixie',
  ])
  if (snapshot.frontendImage.includes('npm ci')) {
    errors.push('Frontend image must not create an npm dependency graph')
  }

  requireTokens(errors, snapshot.productionCompose, 'Production Compose invariant', [
    'image: postgres:18.6-alpine3.24',
    'pgdata:/var/lib/postgresql',
    'POSTGRES_PASSWORD: ${DB_PASSWORD:?DB_PASSWORD is required}',
    'NODE_ENV: production',
    'HOST: 0.0.0.0',
    'PORT: "8080"',
    'DB_BACKEND: postgres',
    'DATABASE_URL: ${TALOS_PRODUCTION_DATABASE_URL:?TALOS_PRODUCTION_DATABASE_URL is required}',
    'PUBLIC_HTTPS: "true"',
    'CORS_ALLOWED_ORIGIN: ${CORS_ALLOWED_ORIGIN:?CORS_ALLOWED_ORIGIN is required}',
    'TRUSTED_PROXY_COUNT: "1"',
    'TRUSTED_PROXY_CIDRS: 172.29.0.0/24',
    'ipv4_address: 172.29.0.30',
    '${TLS_CERT_FILE:?TLS_CERT_FILE is required}:/etc/talos/tls/tls.crt:ro',
    '${TLS_KEY_FILE:?TLS_KEY_FILE is required}:/etc/talos/tls/tls.key:ro',
    'image: prom/prometheus:v3.14.0',
    'image: grafana/grafana:13.2.2',
    'GF_SECURITY_ADMIN_USER: ${GRAFANA_USER:?GRAFANA_USER is required}',
    'GF_SECURITY_ADMIN_PASSWORD: ${GRAFANA_PASSWORD:?GRAFANA_PASSWORD is required}',
    'context: ..',
    '../nginx/production.conf:/etc/nginx/conf.d/default.conf:ro',
    '../prometheus/prometheus.yml:/etc/prometheus/prometheus.yml:ro',
    '../grafana/dashboards:/etc/grafana/provisioning/dashboards:ro',
  ])
  for (const forbidden of [
    'changeme',
    ':latest',
    '"5432:5432"',
    '"8080:8080"',
    'DB_BACKEND: sqlite',
    'PUBLIC_HTTPS: "false"',
    'TRUSTED_PROXY_CIDRS: 0.0.0.0/0',
    'pgdata:/var/lib/postgresql/data',
  ]) {
    if (snapshot.productionCompose.includes(forbidden)) {
      errors.push(`Production Compose must not contain: ${forbidden}`)
    }
  }

  const composeLines = snapshot.productionCompose
    .split(/\r?\n/)
    .map(line => line.trim())
  for (const forbiddenLine of [
    'context: .',
    '- ./nginx/production.conf:/etc/nginx/conf.d/default.conf:ro',
    '- ./prometheus/prometheus.yml:/etc/prometheus/prometheus.yml:ro',
    '- ./grafana/dashboards:/etc/grafana/provisioning/dashboards:ro',
  ]) {
    if (composeLines.includes(forbiddenLine)) {
      errors.push(`Production Compose must not contain exact line: ${forbiddenLine}`)
    }
  }

  requireTokens(errors, snapshot.productionNginx, 'Production nginx invariant', [
    'listen 80;',
    'return 308 https://$host$request_uri;',
    'listen 443 ssl;',
    'ssl_certificate /etc/talos/tls/tls.crt;',
    'ssl_certificate_key /etc/talos/tls/tls.key;',
    'proxy_set_header X-Real-IP $remote_addr;',
    'proxy_set_header X-Forwarded-For $remote_addr;',
    'proxy_set_header X-Forwarded-Proto https;',
    'proxy_set_header X-Forwarded-Host $host;',
    'location /audit-logs/ {',
    'location /tenant-memberships/ {',
  ])
  if (snapshot.productionNginx.includes('$proxy_add_x_forwarded_for')) {
    errors.push('Production nginx must overwrite, not append, public X-Forwarded-For')
  }

  requireTokens(errors, snapshot.dockerignore, 'Docker context boundary', [
    '.git',
    'node_modules',
    'frontend/node_modules',
    'backend/target',
    'public',
    '.env',
    '.env.*',
    '*.db',
  ])

  requireTokens(errors, snapshot.evidence, 'SP01 evidence', [
    'P9_DEPLOYMENT_PROFILE_PASS',
    'exact tags alone are not final artifact provenance',
    'does not claim',
  ])

  const testCall = 'node scripts/check-r4-p9-sp01-deployment-profile.test.mjs'
  const gateCall = 'node scripts/check-r4-p9-sp01-deployment-profile.mjs'
  const composeCall = 'node scripts/check-r4-p9-sp01-compose-config.mjs'
  const testCount = snapshot.exactHead.split(testCall).length - 1
  const gateCount = snapshot.exactHead.split(gateCall).length - 1
  const composeCount = snapshot.exactHead.split(composeCall).length - 1
  if (testCount !== 1 || gateCount !== 1 || composeCount !== 1) {
    errors.push(
      `Exact-Head must run SP01 mutation, structural and executable Compose gates once; found test=${testCount}, gate=${gateCount}, compose=${composeCount}`,
    )
  }

  return errors
}

function main() {
  const errors = validateP9Sp01Snapshot(collectP9Sp01Snapshot())
  if (errors.length > 0) {
    console.error('R4-P9-SP01 deployment profile gate failed:')
    for (const error of errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P9-SP01 deployment profile gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
