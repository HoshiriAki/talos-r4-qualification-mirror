#!/usr/bin/env node

import assert from 'node:assert/strict'
import { checkR4LegacyClosure, SERVICE_CLASSIFICATION } from './check-r4-legacy-closure.mjs'

const PATHS = {
  routeMod: 'backend/src/routes/mod.rs',
  bookingRouteTombstone: 'backend/src/routes/booking.rs',
  bookingModule: 'backend/system/admin/src/booking.rs',
  router: 'frontend/src/router/index.ts',
  sidebar: 'frontend/src/constants/sidebarGroups.ts',
  settings: 'frontend/src/utils/settings.ts',
  record: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r4-p2-legacy-compatibility-closure.md',
}

function baseline() {
  const files = {
    [PATHS.routeMod]: 'pub mod booking;\n// intentionally unmounted',
    [PATHS.bookingRouteTombstone]: '//! R4 tombstone\n//! canonical reservation_v2\n',
    [PATHS.bookingModule]: `
      CommandMetadata::new("availability", A, &[], S),
      CommandMetadata::new("estimate", A, &[], S),
      CommandMetadata::new("device_search", A, &[], S),
    `,
    [PATHS.router]: "{ path: '/app/booking', redirect: '/app/orders' }",
    [PATHS.sidebar]: "{ path: '/app/orders' }",
    [PATHS.settings]: "const HOME_ROUTES = new Set(['/app/orders'])",
    [PATHS.record]: [
      'R4_P1_BASELINE_BOUND',
      'R4_P2_PUBLIC_BOOKING_RETIRED',
      'INTERNAL_BOOKING_READ_HOLD_UNTIL_R4_P3',
      'ORDER_USERS_COMPATIBILITY_HOLD',
      'DEVICE_SERIAL_COMPATIBILITY_HOLD',
      'LEGACY_SERVICE_CLASSIFICATION_COMPLETE',
      'TALOS-R4-P2-LEGACY-CLOSURE',
    ].join('\n'),
  }
  return {
    files,
    existingPaths: new Set(),
    serviceFiles: new Set(Object.keys(SERVICE_CLASSIFICATION)),
  }
}

function expectFailure(mutator, needle) {
  const fixture = baseline()
  mutator(fixture)
  const failures = checkR4LegacyClosure(fixture)
  assert.ok(failures.some(item => item.evidence.includes(needle)), JSON.stringify(failures, null, 2))
}

assert.deepEqual(checkR4LegacyClosure(baseline()), [])

expectFailure(
  fixture => { fixture.files[PATHS.routeMod] += '\n.merge(booking::booking_routes())' },
  'must stay unmounted',
)
expectFailure(
  fixture => { fixture.files[PATHS.bookingRouteTombstone] += '\n.route("/api/booking/reserve", post(reserve))' },
  'must not define an HTTP route',
)
expectFailure(
  fixture => { fixture.existingPaths.add('frontend/src/pages/BookingPage.vue') },
  'must not be recreated',
)
expectFailure(
  fixture => { fixture.files[PATHS.router] += "\ncomponent: () => import('@/pages/BookingPage.vue')" },
  'must not be dynamically reachable',
)
expectFailure(
  fixture => { fixture.files[PATHS.sidebar] += "\n'/app/booking'" },
  'must not be advertised',
)
expectFailure(
  fixture => { fixture.files[PATHS.bookingModule] += '\nCommandMetadata::new("reserve", A, &[], S)' },
  'must not be advertised',
)
expectFailure(
  fixture => { fixture.serviceFiles.add('new_legacy_write_service.rs') },
  'no R4-P2 authority classification',
)
expectFailure(
  fixture => { fixture.serviceFiles.add('report_service.rs') },
  'no R4-P2 authority classification',
)

console.log('R4-P2 legacy compatibility closure mutation tests passed.')
