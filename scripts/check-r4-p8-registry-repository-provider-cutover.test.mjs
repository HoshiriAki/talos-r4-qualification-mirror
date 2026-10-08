#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectRegistryRepositoryProviderSnapshot,
  validateRegistryRepositoryProviderSnapshot,
} from './check-r4-p8-registry-repository-provider-cutover.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectRegistryRepositoryProviderSnapshot())
  mutate(snapshot)
  const errors = validateRegistryRepositoryProviderSnapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    errors.some(error => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  )
}

const baseline = validateRegistryRepositoryProviderSnapshot(
  collectRegistryRepositoryProviderSnapshot(),
)
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before Registry provider mutations: ${baseline.join('; ')}`,
)

expectFailure(
  'Factory loses injectable provider field',
  snapshot => {
    snapshot.factory = snapshot.factory.replace(
      'repository_provider: Option<Arc<dyn RepositoryProvider>>',
      'repository_provider_removed: bool',
    )
  },
  'repository_provider: Option<Arc<dyn RepositoryProvider>>',
)

expectFailure(
  'Production assembler stops injecting selected provider',
  snapshot => {
    snapshot.assembler = snapshot.assembler.replace(
      '.with_repository_provider(repository_provider)',
      '.with_metrics(metrics.clone())',
    )
  },
  '.with_repository_provider(repository_provider)',
)

expectFailure(
  'Production Registry call stops passing composition-root provider',
  snapshot => {
    const anchor =
      'user_settings_module,\n            tenant_governance_module,\n            tenant_preview_module,\n            tenant_simulation_module,\n            repository_provider.clone(),\n            registry_audit_sink,'
    assert.ok(
      snapshot.main.includes(anchor),
      'Production Registry provider mutation anchor missing',
    )
    snapshot.main = snapshot.main.replace(
      anchor,
      'user_settings_module,\n            tenant_governance_module,\n            tenant_preview_module,\n            tenant_simulation_module,\n            /* repository provider removed */\n            registry_audit_sink,',
    )
  },
  'must pass the composition-root RepositoryProvider',
)

expectFailure(
  'PostgreSQL provider selection disappears from composition root',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'PostgresRepositoryProvider::new_with_metrics(',
      'SqliteRepositoryProvider::new_with_metrics(',
    )
  },
  'PostgresRepositoryProvider::new_with_metrics(',
)

expectFailure(
  'Registry provider cutover restores production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst FORBIDDEN_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the transitional production barrier',
)

console.log('R4-P8 Registry RepositoryProvider cutover mutation tests passed.')
