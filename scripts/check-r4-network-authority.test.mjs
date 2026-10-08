#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  RULE,
  checkR4NetworkAuthority,
  loadRepository,
} from './check-r4-network-authority.mjs'

const ROOT = process.cwd()
const baseline = loadRepository(ROOT)

function copyRepository() {
  return {
    files: { ...baseline.files },
    rustSources: { ...baseline.rustSources },
  }
}

function replaceRequired(repository, path, from, to) {
  assert.ok(repository.files[path]?.includes(from), `${path} must contain mutation source: ${from}`)
  // Security tokens may intentionally appear at more than one enforcement
  // point. Remove every identical occurrence so the mutation proves that the
  // structural gate detects loss of the semantic invariant rather than merely
  // observing a duplicate token elsewhere in the file.
  repository.files[path] = repository.files[path].split(from).join(to)
  if (Object.hasOwn(repository.rustSources, path)) {
    repository.rustSources[path] = repository.files[path]
  }
}

function expectFailure(name, mutate, expectedText) {
  const repository = copyRepository()
  mutate(repository)
  const findings = checkR4NetworkAuthority(repository)
  assert.ok(findings.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    findings.some((finding) =>
      `${finding.path} ${finding.evidence}`.toLowerCase().includes(expectedText.toLowerCase()),
    ),
    `${name}: no finding matched ${expectedText}; got ${JSON.stringify(findings)}`,
  )
  assert.ok(findings.every((finding) => finding.rule === RULE), `${name}: wrong rule id`)
}

const initial = checkR4NetworkAuthority(baseline)
assert.deepEqual(initial, [], `baseline R4-P5 gate must pass: ${JSON.stringify(initial)}`)

expectFailure(
  'proxy environment bypass protection',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/egress.rs',
    '.no_proxy()',
    '/* no_proxy removed */',
  ),
  '.no_proxy()',
)

expectFailure(
  'validated DNS address pin',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/egress.rs',
    'builder.resolve(&host, SocketAddr::new(address, port))',
    'builder',
  ),
  'builder.resolve',
)

expectFailure(
  'DNS resolution remains inside the connect/overall budget',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/egress.rs',
    '.resolve_and_validate_with_budget(',
    '.resolve_and_validate_without_budget(',
  ),
  'budget admission does not precede DNS',
)

expectFailure(
  'network budget admission remains before DNS resolution',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/egress.rs',
    'let _permit = match self.acquire_budget()',
    'let _permit = match self.acquire_budget_after_dns()',
  ),
  'budget admission does not precede DNS',
)

expectFailure(
  'overall deadline includes work before outbound send',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/egress.rs',
    'request.timeouts.overall.checked_sub(started.elapsed())',
    'Some(request.timeouts.overall)',
  ),
  'request.timeouts.overall.checked_sub(started.elapsed())',
)

expectFailure(
  'DNS timeout remains classified as before-dispatch',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/egress.rs',
    'TransportFailure::before_dispatch(timeout_class)',
    'TransportFailure::after_dispatch(timeout_class)',
  ),
  'TransportFailure::before_dispatch(timeout_class)',
)

expectFailure(
  'TCP/TLS connect receives only the remaining connect budget',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/egress.rs',
    '.connect_timeout(connect_remaining)',
    '.connect_timeout(request.timeouts.connect)',
  ),
  '.connect_timeout(connect_remaining)',
)

expectFailure(
  'caller cannot control HTTP message framing',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/egress.rs',
    '"content-length"',
    '"x-removed-content-length"',
  ),
  '"content-length"',
)

expectFailure(
  'caller cannot spoof gateway correlation authority',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/egress.rs',
    '"x-correlation-id"',
    '"x-removed-correlation-id"',
  ),
  '"x-correlation-id"',
)

expectFailure(
  'metadata destination classification',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/egress.rs',
    '169, 254, 169, 254',
    '169, 254, 1, 1',
  ),
  '169, 254, 169, 254',
)

expectFailure(
  'IPv4 current-network range cannot become public',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/egress.rs',
    'octets[0] == 0',
    'false',
  ),
  'octets[0] == 0',
)

expectFailure(
  'deprecated IPv6 site-local range cannot become public',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/egress.rs',
    '(segments[0] & 0xffc0) == 0xfec0',
    'false',
  ),
  '(segments[0] & 0xffc0) == 0xfec0',
)

expectFailure(
  'IANA-unallocated IPv6 remains fail-closed',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/egress.rs',
    '"2d00::1"',
    '"2c00::1"',
  ),
  '"2d00::1"',
)

expectFailure(
  'secret-bearing provider egress cannot downgrade to HTTP',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/egress.rs',
    'protocol != EgressProtocol::Https',
    'false',
  ),
  'secret-bearing provider/application egress',
)

expectFailure(
  'ambiguous encoded path cannot regain prefix authority',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/egress.rs',
    'ambiguous_authority_path',
    'removed_path_guard',
  ),
  'ambiguous_authority_path',
)

expectFailure(
      'routing override headers remain gateway authority',
      (repository) => replaceRequired(repository, 'backend/src/integration/egress.rs', 'routing_override_header', 'removed_routing_guard'),
      'routing_override_header',
    )

    expectFailure(
      'plaintext credential headers retain HTTPS backstop',
      (repository) => replaceRequired(repository, 'backend/src/integration/egress.rs', 'sensitive_credential_header', 'removed_secret_header_guard'),
      'sensitive_credential_header',
    )

    expectFailure(
      'IANA IPv4 AS112 special-use range cannot become ambient public',
      (repository) => replaceRequired(repository, 'backend/src/integration/egress.rs', '[192, 31, 196, 0]', '[192, 31, 197, 0]'),
      '[192, 31, 196, 0]',
    )

    expectFailure(
      'IANA IPv6 AS112 special-use range cannot become ambient public',
      (repository) => replaceRequired(repository, 'backend/src/integration/egress.rs', 'segments[2] == 0x8000', 'false'),
      'segments[2] == 0x8000',
    )

expectFailure(
  'direct IPv6 grant syntax remains fail-closed',
  (repository) => replaceRequired(
    repository,
    'backend/system/core/src/transport/network.rs',
    "host.contains(':')",
    'false',
  ),
  "host.contains(':')",
)

expectFailure(
  'legacy numeric host cannot acquire DNS semantics',
  (repository) => replaceRequired(
    repository,
    'backend/system/core/src/transport/network.rs',
    "byte.is_ascii_digit() || byte == b'.'",
    'false',
  ),
  'byte.is_ascii_digit',
)

expectFailure(
  'NAT64 embedded IPv4 keeps IPv4 destination classification',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/egress.rs',
    'return classify_v4(embedded_v4_tail(address));',
    'return DestinationClass::Public;',
  ),
  'embedded_v4_tail',
)

expectFailure(
  'special IPv6 registry range cannot silently become public',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/egress.rs',
    'segments[0] == 0x5f00',
    'false',
  ),
  'segments[0] == 0x5f00',
)

expectFailure(
  'single protocol grant authority',
  (repository) => replaceRequired(
    repository,
    'backend/system/core/src/transport/network.rs',
    'self.protocols.len() != 1',
    'false',
  ),
  'single protocol authority',
)

expectFailure(
  'single method-class grant authority',
  (repository) => replaceRequired(
    repository,
    'backend/system/core/src/transport/network.rs',
    'self.method_classes.len() != 1',
    'false',
  ),
  'single method-class authority',
)

expectFailure(
  'secret-purpose grant dimension',
  (repository) => replaceRequired(
    repository,
    'backend/system/core/src/transport/network.rs',
    'secret_purposes',
    'removed_secret_dimension',
  ),
  'secret_purposes',
)

expectFailure(
  'P6 must own shared budget lifecycle before runtime construction',
  (repository) => {
    const path = 'backend/src/integration/governed_worker.rs'
    repository.rustSources[path] += '\nfn forbidden_mount() { let _ = GovernedEgressTransport::new(todo!(), todo!(), todo!(), todo!()); }\n'
  },
  'shared budget lifecycle authority',
)

expectFailure(
  'ambient production HTTP is denied',
  (repository) => replaceRequired(
    repository,
    'backend/src/main.rs',
    'let http_client: Arc<dyn HttpClient> = Arc::new(NoopHttpClient);',
    'let http_client: Arc<dyn HttpClient> = Arc::new(ReqwestHttpClient::new());',
  ),
  'ambient http network authority',
)

expectFailure(
  'production webhook needs durable PostgreSQL lane',
  (repository) => replaceRequired(
    repository,
    'backend/src/main.rs',
    'R4-P5 production webhook Event Lane requires PostgreSQL 18 durable driver',
    'webhook event lane unavailable',
  ),
  'PostgreSQL 18 durable driver',
)

expectFailure(
  'production durability guard remains before startup side effects',
  (repository) => {
    const path = 'backend/src/main.rs'
    const source = repository.files[path]
    const guard = '    #[cfg(feature = "postgres")]\n    if config.is_production && pg_pool.is_none()'
    assert.ok(source.includes(guard), `${path} must contain production durability guard`)
    repository.files[path] = source.replace(guard, '    sentry::init(sentry::ClientOptions::default());\n' + guard)
    repository.rustSources[path] = repository.files[path]
  },
  'durability admission does not precede platform/application side effects',
)

expectFailure(
  'governed worker cannot regress to fixture receipt worker',
  (repository) => replaceRequired(
    repository,
    'backend/src/main.rs',
    'GovernedIntegrationWorker::new_with_metrics',
    'FixtureIntegrationWorker::new_with_metrics',
  ),
  'GovernedIntegrationWorker',
)

expectFailure(
  'provider-owned webhook listener stays retired',
  (repository) => {
    const path = 'backend/system/core/src/transport/webhook.rs'
    repository.files[path] += '\npub trait WebhookHandler: Send + Sync { fn path_prefix(&self) -> &str; }\n'
    repository.rustSources[path] = repository.files[path]
  },
  'provider-owned webhook listener API was reintroduced',
)

expectFailure(
  'normalized event replay timestamp must bind to persisted P3 event',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/webhook_event.rs',
    'SELECT created_at_ms FROM interconnect_events',
    'SELECT 0',
  ),
  'SELECT created_at_ms FROM interconnect_events',
)

expectFailure(
  'concurrent replay conflict must use bounded timestamp reconciliation',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/webhook_event.rs',
    'error.code == InterconnectErrorCode::ContractIncompatible',
    'false',
  ),
  'ContractIncompatible',
)

expectFailure(
  'live PG18 webhook replay identity evidence remains compiled',
  (repository) => replaceRequired(
    repository,
    'backend/src/integration/webhook_event_pg_tests.rs',
    'live_pg18_webhook_event_replay_reuses_persisted_envelope_identity',
    'removed_live_pg18_webhook_event_replay_test',
  ),
  'live_pg18_webhook_event_replay_reuses_persisted_envelope_identity',
)

expectFailure(
  'normalized event cannot carry raw webhook material',
  (repository) => {
    const path = 'backend/src/integration/webhook_event.rs'
    repository.files[path] = repository.files[path].replace(
      'const WEBHOOK_EVENT_CONTRACT',
      'const raw_payload: &str = "forbidden";\nconst WEBHOOK_EVENT_CONTRACT',
    )
    repository.rustSources[path] = repository.files[path]
  },
  'raw_payload',
)

expectFailure(
  'new provider code cannot bypass gateway with reqwest',
  (repository) => {
    repository.rustSources['backend/thirdparty/fixture/src/network.rs'] =
      'fn bypass() { let _ = reqwest::Client::new(); }'
  },
  'direct reqwest usage',
)

expectFailure(
  'legacy provider cannot be remounted in Core Registry',
  (repository) => {
    const path = 'backend/src/registry/factory.rs'
    repository.files[path] += '\nfn forbidden_provider_mount() { let _ = FeatureSfExpress::new(); }\n'
    repository.rustSources[path] = repository.files[path]
  },
  'deferred provider module became constructible',
)

console.log('R4-P5 network authority mutation tests passed.')
