#!/usr/bin/env node

import assert from 'node:assert/strict';
import {
  collectP8R3SettlementSnapshot,
  validateP8R3SettlementSnapshot,
} from './check-r4-p8-r3-settlement-cutover.mjs';

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8R3SettlementSnapshot());
  mutate(snapshot);
  const errors = validateP8R3SettlementSnapshot(snapshot);
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`);
  assert.ok(
    errors.some((error) => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  );
}

const baseline = validateP8R3SettlementSnapshot(
  collectP8R3SettlementSnapshot(),
);
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before R3 mutations: ${baseline.join('; ')}`,
);

expectFailure(
  'R3 provider bypasses neutral dispatch',
  (snapshot) => {
    snapshot.provider = snapshot.provider.replaceAll(
      'r3_settlement_dispatch::ScopedR3SettlementRepository',
      'r3_settlement::LegacyR3SettlementRepository',
    );
  },
  'backend-neutral dispatch',
);

expectFailure(
  'R3 PostgreSQL dispatch disappears',
  (snapshot) => {
    snapshot.dispatch = snapshot.dispatch.replaceAll(
      'PostgresR3SettlementRepository::new(self.scoped)',
      'REMOVED_R3_PG_ROUTE',
    );
  },
  'route all 16',
);

expectFailure(
  'R3 loses R2 PostgreSQL admission authority',
  (snapshot) => {
    snapshot.writer = snapshot.writer.replaceAll(
      'admit_settlement_charge_pg',
      'REMOVED_R2_ADMISSION',
    );
  },
  'admit_settlement_charge_pg',
);

expectFailure(
  'R3 close loses same-transaction lifecycle seam',
  (snapshot) => {
    snapshot.writer = snapshot.writer.replaceAll(
      'apply_action_in_pg_transaction',
      'REMOVED_LIFECYCLE_TX_SEAM',
    );
  },
  'apply_action_in_pg_transaction',
);

expectFailure(
  'Integration Fabric loses PostgreSQL settlement admission seam',
  (snapshot) => {
    snapshot.admission = snapshot.admission.replaceAll(
      'pub(crate) async fn admit_settlement_charge_pg',
      'async fn REMOVED_PG_ADMISSION',
    );
  },
  'PostgreSQL settlement admission seam',
);

expectFailure(
  'Lifecycle loses caller-owned PostgreSQL transaction seam',
  (snapshot) => {
    snapshot.lifecyclePg = snapshot.lifecyclePg.replaceAll(
      'pub(in crate::repositories) async fn apply_action_in_pg_transaction',
      'async fn REMOVED_LIFECYCLE_PG_TX',
    );
  },
  'caller-owned transaction seam',
);

expectFailure(
  'R3 trigger repair regresses to direct heterogeneous NEW field access',
  (snapshot) => {
    snapshot.triggerRepair = snapshot.triggerRepair
      .replaceAll("new_row ->> 'settlement_case_id'", 'NEW.settlement_case_id')
      .replaceAll("new_row ->> 'order_id'", 'NEW.order_id');
  },
  'must not access heterogeneous NEW fields directly',
);

expectFailure(
  'R3 live proof drops terminal seal',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'sealed_update.is_err()',
      'true',
    );
  },
  'sealed_update.is_err()',
);

expectFailure(
  'R3 live proof drops Preview denial',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'REPOSITORY_PREVIEW_WRITE_DENIED',
      'REMOVED_PREVIEW_DENIAL',
    );
  },
  'REPOSITORY_PREVIEW_WRITE_DENIED',
);

expectFailure(
  'R3 Settlement production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'R3 Settlement gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

console.log('R4-P8 R3 settlement cutover mutation tests passed.');
