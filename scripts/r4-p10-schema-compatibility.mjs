#!/usr/bin/env node

import process from 'node:process'

function fail(message, code = 1) {
  console.error(message)
  process.exit(code)
}

function arg(name) {
  const index = process.argv.indexOf(name)
  if (index < 0 || !process.argv[index + 1]) fail('missing ' + name)
  return process.argv[index + 1]
}

function migration(value, label) {
  const match = /^(\d{3})_[a-z0-9_]+$/.exec(value)
  if (!match) fail(label + ' must be a canonical migration id: ' + JSON.stringify(value))
  return { id: value, ordinal: Number.parseInt(match[1], 10) }
}

const candidateRef = arg('--candidate-ref')
const candidateMax = migration(arg('--candidate-max'), 'candidate max')
const restoredLatest = migration(arg('--restored-latest'), 'restored latest')

if (!/^[A-Za-z0-9._/-]+$/.test(candidateRef)) fail('invalid candidate ref')

if (restoredLatest.ordinal > candidateMax.ordinal) {
  console.error(
    'ROLLBACK_SCHEMA_INCOMPATIBLE candidate=' + candidateRef +
    ' candidate_max=' + candidateMax.id +
    ' restored_latest=' + restoredLatest.id +
    ' candidate_process_started=false schema_downgrade_attempted=false',
  )
  process.exit(42)
}

console.log(
  'ROLLBACK_SCHEMA_COMPATIBLE candidate=' + candidateRef +
  ' candidate_max=' + candidateMax.id +
  ' restored_latest=' + restoredLatest.id +
  ' schema_downgrade_required=false',
)
