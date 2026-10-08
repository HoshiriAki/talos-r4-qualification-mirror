#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

export const RULE = 'TALOS-R4-P2-LEGACY-CLOSURE'

export const SERVICE_CLASSIFICATION = Object.freeze({
  'api_key_service.rs': 'Compatibility',
  'audit_service.rs': 'Infrastructure',
  'auth_rate_limit.rs': 'Infrastructure',
  'auth_service.rs': 'Infrastructure',
  'dashboard_service.rs': 'QueryProjection',
  'device_bounded_read.rs': 'QueryProjection',
  'device_service.rs': 'DomainWriteLegacy',
  'excel_import_service.rs': 'Compatibility',
  'logistics_service.rs': 'QueryProjection',
  'machine_api.rs': 'Infrastructure',
  'machine_api_security_tests.rs': 'TestOnly',
  'model_service.rs': 'DomainWriteLegacy',
  'mod.rs': 'Infrastructure',
  'multipart_import.rs': 'Infrastructure',
  'order_compatibility_support.rs': 'Compatibility',
  'order_export_service.rs': 'QueryProjection',
  'pricing_service.rs': 'DomainWriteLegacy',
  'query_builder.rs': 'Infrastructure',
  'session_security.rs': 'Infrastructure',
  'tenant_helpers.rs': 'Infrastructure',
  'totp_login.rs': 'Infrastructure',
  'user_settings_service.rs': 'ProfileSpecific',
  'warehouse_routing_service.rs': 'QueryProjection',
  'warehouse_service.rs': 'DomainWriteLegacy',
})

const PATHS = Object.freeze({
  routeMod: 'backend/src/routes/mod.rs',
  bookingRouteTombstone: 'backend/src/routes/booking.rs',
  bookingModule: 'backend/system/admin/src/booking.rs',
  router: 'frontend/src/router/index.ts',
  sidebar: 'frontend/src/constants/sidebarGroups.ts',
  settings: 'frontend/src/utils/settings.ts',
  agents: 'AGENTS.md',
  record: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r4-p2-legacy-compatibility-closure.md',
})

const RETIRED_FRONTEND = Object.freeze([
  'frontend/src/pages/BookingPage.vue',
  'frontend/src/stores/booking.ts',
  'frontend/src/api/booking.ts',
])

function finding(path, evidence) {
  return { rule: RULE, path, evidence, occurrences: 1 }
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

function requireText(failures, source, token, path, evidence) {
  if (!source.includes(token)) failures.push(finding(path, evidence))
}

function forbidText(failures, source, token, path, evidence) {
  if (source.includes(token)) failures.push(finding(path, evidence))
}

export function checkR4LegacyClosure({ files, existingPaths, serviceFiles }) {
  const failures = []

  for (const path of Object.values(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(finding(path, 'required R4-P2 evidence is missing'))
  }
  if (failures.length > 0) return failures

  const routeMod = files[PATHS.routeMod]
  const bookingRouteTombstone = files[PATHS.bookingRouteTombstone]
  const bookingModule = files[PATHS.bookingModule]
  const router = files[PATHS.router]
  const sidebar = files[PATHS.sidebar]
  const settings = files[PATHS.settings]
  const agents = files[PATHS.agents]
  const record = files[PATHS.record]

  forbidText(
    failures,
    routeMod,
    '.merge(booking::booking_routes())',
    PATHS.routeMod,
    'public /api/booking compatibility routes must stay unmounted',
  )
  requireText(
    failures,
    bookingRouteTombstone,
    'R4 tombstone',
    PATHS.bookingRouteTombstone,
    'retired Booking route path must remain an explicit tombstone until old R1 structural lineage is migrated',
  )
  requireText(
    failures,
    bookingRouteTombstone,
    'reservation_v2',
    PATHS.bookingRouteTombstone,
    'Booking tombstone must name the canonical Reservation replacement',
  )
  forbidText(
    failures,
    bookingRouteTombstone,
    '.route(',
    PATHS.bookingRouteTombstone,
    'Booking tombstone must not define an HTTP route',
  )

  for (const path of RETIRED_FRONTEND) {
    if (existingPaths.has(path)) failures.push(finding(path, 'retired Booking frontend source must not be recreated'))
  }

  requireText(
    failures,
    router,
    "{ path: '/app/booking', redirect: '/app/orders' }",
    PATHS.router,
    'legacy Booking deep link must redirect to the governed order workflow during the compatibility window',
  )
  forbidText(
    failures,
    router,
    "import('@/pages/BookingPage.vue')",
    PATHS.router,
    'retired Booking page must not be dynamically reachable',
  )
  forbidText(failures, sidebar, "'/app/booking'", PATHS.sidebar, 'Booking must not be advertised in tenant navigation')
  forbidText(failures, settings, "'/app/booking'", PATHS.settings, 'Booking must not be selectable as a saved home page')

  // Internal FeatureBooking remains only as a bounded read-only compatibility
  // hold until R4-P3 supplies replacement generic Query/Capability contracts.
  for (const command of ['availability', 'estimate', 'device_search']) {
    requireText(
      failures,
      compact(bookingModule),
      `CommandMetadata::new("${command}"`,
      PATHS.bookingModule,
      `bounded internal Booking read compatibility missing ${command}`,
    )
  }
  for (const command of ['reserve', 'confirm']) {
    forbidText(
      failures,
      compact(bookingModule),
      `CommandMetadata::new("${command}"`,
      PATHS.bookingModule,
      `legacy Booking write command ${command} must not be advertised`,
    )
  }

  for (const token of [
    '/users` is the current Order compatibility API',
    '`deviceSerialNo` is legacy single-device compatibility',
  ]) {
    requireText(failures, agents, token, PATHS.agents, `root compatibility authority missing ${token}`)
  }

  const actualServices = [...serviceFiles].sort()
  const classifiedServices = Object.keys(SERVICE_CLASSIFICATION).sort()
  for (const name of actualServices) {
    if (!Object.hasOwn(SERVICE_CLASSIFICATION, name)) {
      failures.push(finding(`backend/src/services/${name}`, 'service has no R4-P2 authority classification'))
    }
  }
  for (const name of classifiedServices) {
    if (!serviceFiles.has(name)) {
      failures.push(finding(`backend/src/services/${name}`, 'classification references a service file that no longer exists'))
    }
  }

  for (const token of [
    'R4_P1_BASELINE_BOUND',
    'R4_P2_PUBLIC_BOOKING_RETIRED',
    'INTERNAL_BOOKING_READ_HOLD_UNTIL_R4_P3',
    'ORDER_USERS_COMPATIBILITY_HOLD',
    'DEVICE_SERIAL_COMPATIBILITY_HOLD',
    'LEGACY_SERVICE_CLASSIFICATION_COMPLETE',
    'TALOS-R4-P2-LEGACY-CLOSURE',
  ]) {
    requireText(failures, record, token, PATHS.record, `R4-P2 record missing ${token}`)
  }

  return failures
}

function loadRepository(root) {
  const files = {}
  for (const path of Object.values(PATHS)) {
    const absolute = join(root, path)
    if (existsSync(absolute)) files[path] = readFileSync(absolute, 'utf8')
  }
  const existingPaths = new Set(RETIRED_FRONTEND.filter(path => existsSync(join(root, path))))
  const servicesDir = join(root, 'backend/src/services')
  const serviceFiles = new Set(
    readdirSync(servicesDir, { withFileTypes: true })
      .filter(entry => entry.isFile() && entry.name.endsWith('.rs'))
      .map(entry => entry.name),
  )
  return { files, existingPaths, serviceFiles }
}

function main() {
  const failures = checkR4LegacyClosure(loadRepository(process.cwd()))
  if (failures.length > 0) {
    console.error('R4-P2 legacy compatibility closure check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P2 legacy compatibility closure check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
