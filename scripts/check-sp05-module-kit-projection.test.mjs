#!/usr/bin/env node

import assert from 'node:assert/strict'
import { checkSp05Projection, POLICY_MARKER, RULE } from './check-sp05-module-kit-projection.mjs'

const cleanFiles = {
  'backend/system/core/Cargo.toml': `[features]\ndefault = []\nexperimental-orchestration = []`,
  'backend/system/core/src/lib.rs': `
#[cfg(feature = "experimental-orchestration")]
pub mod experimental;
pub trait DataTransport {}
pub struct TransportMetadata;
pub enum TransportPriority { Normal }
pub struct TransportOptions;
pub struct TransportMessage;
pub struct TransportHealth;`,
  'backend/system/core/src/experimental/mod.rs': `
pub mod orchestration;
pub use orchestration::{
  CompensationLog,
  ModuleOp,
  OrchestrationError,
  SagaStep,
};`,
  'backend/system/core/src/experimental/orchestration.rs': `
pub struct ModuleOp;
pub struct SagaStep;
pub struct OrchestrationError;
pub struct CompensationLog;`,
  'module-kit/INIT.md': `${POLICY_MARKER}
implementation_commit
copy backend/system/core
copy backend/system/core-derive
不得从 Markdown 重新手写 system-core
experimental-orchestration
system_core::experimental
DataTransport`,
  'module-kit/MODULE_TEMPLATE.md': `${POLICY_MARKER}
experimental-orchestration
system_core::experimental::{ModuleOp, SagaStep}`,
}

assert.deepEqual(checkSp05Projection(cleanFiles), [], 'clean projection must pass')

const inlineDuplicate = structuredClone(cleanFiles)
inlineDuplicate['module-kit/INIT.md'] += '\npub trait DataTransport {}'
assert.equal(checkSp05Projection(inlineDuplicate)[0]?.rule, RULE, 'inline Core duplication must fail')

const missingCopyAuthority = structuredClone(cleanFiles)
missingCopyAuthority['module-kit/INIT.md'] = missingCopyAuthority['module-kit/INIT.md'].replace('backend/system/core-derive', '')
assert.equal(checkSp05Projection(missingCopyAuthority)[0]?.rule, RULE, 'missing authoritative crate copy must fail')

const missingFeaturePath = structuredClone(cleanFiles)
missingFeaturePath['module-kit/INIT.md'] = missingFeaturePath['module-kit/INIT.md'].replace('system_core::experimental', '')
assert.equal(checkSp05Projection(missingFeaturePath)[0]?.rule, RULE, 'missing experimental import path must fail')

const operationalSaga = structuredClone(cleanFiles)
operationalSaga['module-kit/MODULE_TEMPLATE.md'] += '\nSaga::new().execute();'
assert.equal(checkSp05Projection(operationalSaga)[0]?.rule, RULE, 'operational Saga claim must fail')

const splitDefinition = structuredClone(cleanFiles)
splitDefinition['backend/system/core/src/orchestration.rs'] = 'pub struct SagaStep;'
assert(
  checkSp05Projection(splitDefinition).some((entry) => entry.path.endsWith('/orchestration.rs')),
  'orchestration definitions outside experimental must fail',
)

const defaultModule = structuredClone(cleanFiles)
defaultModule['backend/system/core/src/lib.rs'] += '\npub mod orchestration;'
assert(
  checkSp05Projection(defaultModule).some((entry) => entry.evidence.includes('must not expose an orchestration module')),
  'default orchestration module must fail',
)

const multilineReExport = structuredClone(cleanFiles)
multilineReExport['backend/system/core/src/lib.rs'] += `
pub use experimental::{
  ModuleOp,
  SagaStep,
};`
assert(
  checkSp05Projection(multilineReExport).some((entry) => entry.evidence.includes('re-export is forbidden')),
  'multiline default re-export must fail',
)

const missingTransport = structuredClone(cleanFiles)
missingTransport['backend/system/core/src/lib.rs'] = missingTransport['backend/system/core/src/lib.rs'].replace('pub trait DataTransport {}', '')
assert.equal(checkSp05Projection(missingTransport)[0]?.rule, RULE, 'missing canonical transport must fail')

const duplicateTransport = structuredClone(cleanFiles)
duplicateTransport['backend/system/core/src/experimental/transport.rs'] = 'pub trait DataTransport {}'
assert(
  checkSp05Projection(duplicateTransport).some((entry) => entry.evidence.includes('must not be duplicated')),
  'experimental transport duplication must fail',
)

const missingExperimentalType = structuredClone(cleanFiles)
missingExperimentalType['backend/system/core/src/experimental/orchestration.rs'] = missingExperimentalType['backend/system/core/src/experimental/orchestration.rs'].replace('pub struct ModuleOp;', '')
assert.equal(checkSp05Projection(missingExperimentalType)[0]?.rule, RULE, 'missing experimental type must fail')

const historicalEvidence = structuredClone(cleanFiles)
historicalEvidence['docs/historical/core.md'] = 'pub struct ModuleOp; pub trait DataTransport {}'
assert.deepEqual(checkSp05Projection(historicalEvidence), [], 'historical evidence must remain outside the gate')

console.log('SP-05 module-kit projection checker self-test passed: 12 cases.')
