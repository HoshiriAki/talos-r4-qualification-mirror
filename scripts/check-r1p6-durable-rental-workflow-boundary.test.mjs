#!/usr/bin/env node
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { spawnSync } from 'node:child_process'
const root = process.cwd(); const checker = path.join(root, 'scripts/check-r1p6-durable-rental-workflow-boundary.mjs')
const files = ['backend/src/db/migrations/057_durable_rental_workflow.sql','backend/src/db/migrations/postgres/057_durable_rental_workflow.sql','backend/src/db/migrations.rs','backend/src/db/migrations_pg.rs','backend/src/repositories/workflow.rs','backend/src/repositories/contracts/provider.rs','backend/src/application/rental_workflow.rs','backend/src/application/workers.rs','backend/src/main.rs','backend/src/repositories/lifecycle.rs','backend/src/repositories/reservation.rs','backend/src/application/workflow_tests.rs','backend/system/core/src/experimental/orchestration.rs','package.json']
function run(fixtureRoot = root) { return spawnSync(process.execPath, [checker], { cwd: root, env: { ...process.env, TALOS_R1P6_ROOT: fixtureRoot }, encoding: 'utf8' }) }
function fixture() { const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'talos-r1p6-')); for (const rel of files) { const to = path.join(temp, rel); fs.mkdirSync(path.dirname(to), { recursive: true }); fs.copyFileSync(path.join(root, rel), to) } return temp }
function expectFailure(name, rel, search, replacement, evidence) { const temp = fixture(); try { const file = path.join(temp, rel); fs.writeFileSync(file, fs.readFileSync(file, 'utf8').replace(search, replacement)); const result = run(temp); if (result.status === 0 || !`${result.stdout}${result.stderr}`.includes(evidence)) throw new Error(`${name} did not fail closed\n${result.stdout}\n${result.stderr}`) } finally { fs.rmSync(temp, { recursive: true, force: true }) } }
const baseline = run(); if (baseline.status !== 0) throw new Error(`baseline TALOS-OPS-032 failed\n${baseline.stdout}\n${baseline.stderr}`)
expectFailure('missing inbox uniqueness','backend/src/db/migrations/057_durable_rental_workflow.sql','UNIQUE (tenant_id, message_id)','UNIQUE (message_id)','UNIQUE (tenant_id, message_id)')
expectFailure('raw pool exposure','backend/src/repositories/workflow.rs','pub struct ScopedWorkflowRepository','pub struct ScopedWorkflowRepository { pub pool: String }\npub struct Removed','forbidden "pub pool:"')
expectFailure('worker bypass','backend/src/application/workers.rs','consume_domain_events','removed_event_consumption','consume_domain_events')
expectFailure('definition hash removal','backend/src/repositories/workflow.rs','pub const RENTAL_DEFINITION_HASH','const REMOVED_DEFINITION_HASH','pub const RENTAL_DEFINITION_HASH')
expectFailure('provider access','backend/src/application/rental_workflow.rs','use std::sync::Arc;','use std::sync::Arc;\nuse reqwest;','forbidden "reqwest"')
const crlf = fixture(); try { for (const rel of files) { const file = path.join(crlf, rel); fs.writeFileSync(file, fs.readFileSync(file, 'utf8').replaceAll('\r\n','\n').replaceAll('\n','\r\n')) } const result = run(crlf); if (result.status !== 0) throw new Error(`CRLF parity failed\n${result.stdout}\n${result.stderr}`) } finally { fs.rmSync(crlf, { recursive: true, force: true }) }
process.stdout.write('TALOS-OPS-032 negative fixtures and CRLF parity passed.\n')
