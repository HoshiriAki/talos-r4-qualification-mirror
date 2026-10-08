#!/usr/bin/env node

import { readFileSync, readdirSync, statSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const ERROR_PATH = 'backend/src/error.rs'
const SOURCE_ROOT = 'backend/src'

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function collectRustSources(relativeDir = SOURCE_ROOT, out = {}) {
  const absolute = path.join(ROOT, relativeDir)
  for (const entry of readdirSync(absolute)) {
    const relative = path.join(relativeDir, entry)
    const full = path.join(ROOT, relative)
    const stat = statSync(full)
    if (stat.isDirectory()) {
      collectRustSources(relative, out)
    } else if (entry.endsWith('.rs')) {
      out[relative.replaceAll('\\', '/')] = readFileSync(full, 'utf8')
    }
  }
  return out
}

export function collectAppErrorSnapshot() {
  return {
    error: read(ERROR_PATH),
    sources: collectRustSources(),
    workflow: read('.github/workflows/exact-head-qualification.yml'),
  }
}

export function validateAppErrorSnapshot(snapshot) {
  const errors = []
  const errorSource = snapshot.error

  for (const token of [
    'Storage',
    'impl From<rusqlite::Error> for AppError',
    'impl From<r2d2::Error> for AppError',
    'AppError::Storage => "storage"',
    '| AppError::Storage',
    'sqlite_and_pool_errors_convert_into_backend_neutral_storage_surface',
  ]) {
    if (!errorSource.includes(token)) {
      errors.push('Backend-neutral AppError surface missing: ' + token)
    }
  }

  for (const token of [
    'Sqlite(#[from] rusqlite::Error)',
    'Pool(#[from] r2d2::Error)',
  ]) {
    if (errorSource.includes(token)) {
      errors.push('Backend-specific AppError variant restored: ' + token)
    }
  }

  for (const [relative, source] of Object.entries(snapshot.sources)) {
    if (source.includes('AppError::Sqlite')) {
      errors.push('Backend-specific AppError::Sqlite usage restored in ' + relative)
    }
    if (source.includes('AppError::Pool')) {
      errors.push('Backend-specific AppError::Pool usage restored in ' + relative)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-app-error-backend-neutral.test.mjs',
    'node scripts/check-r4-p8-app-error-backend-neutral.mjs',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head AppError qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateAppErrorSnapshot(collectAppErrorSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 AppError backend-neutral gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 AppError backend-neutral gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
