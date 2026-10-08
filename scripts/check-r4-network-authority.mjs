#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import process from 'node:process'

export const RULE = 'TALOS-R4-P5-NETWORK-AUTHORITY'

const PATHS = Object.freeze({
  coreGrant: 'backend/system/core/src/transport/network.rs',
  coreTransportMod: 'backend/system/core/src/transport/mod.rs',
  coreWebhookTombstone: 'backend/system/core/src/transport/webhook.rs',
  egress: 'backend/src/integration/egress.rs',
  egressDispatch: 'backend/src/integration/egress_dispatch.rs',
  integrationMod: 'backend/src/integration/mod.rs',
  webhook: 'backend/src/integration/webhook.rs',
  webhookEvent: 'backend/src/integration/webhook_event.rs',
  webhookEventPgTest: 'backend/src/integration/webhook_event_pg_tests.rs',
  governedWorker: 'backend/src/integration/governed_worker.rs',
  store: 'backend/src/integration/store.rs',
  routes: 'backend/src/routes/integrations.rs',
  main: 'backend/src/main.rs',
  factory: 'backend/src/registry/factory.rs',
})

const REQUIRED = Object.freeze({
  coreGrant: [
    'pub struct EgressGrant',
    'pub enum EgressProtocol',
    'pub enum EgressMethodClass',
    'pub enum EgressRedirectPolicy',
    'allows_secret_purpose',
    "host.contains(':')",
    "byte.is_ascii_digit() || byte == b'.'",
    'host_authority_requires_canonical_dns_or_ipv4_serialization',
    'self.protocols.len() != 1',
    'self.method_classes.len() != 1',
    'multiple_authority_dimensions_fail_closed',
  ],
  egress: [
    'pub struct GovernedEgressTransport',
    'pub trait DestinationResolver',
    'SystemDestinationResolver',
    'DestinationClass::Private',
    'DestinationClass::Loopback',
    'DestinationClass::LinkLocal',
    'DestinationClass::Metadata',
    'DestinationClass::Special',
    '169, 254, 169, 254',
    'to_ipv4_mapped',
    '.redirect(reqwest::redirect::Policy::none())',
    '.no_proxy()',
    'builder.resolve(&host, SocketAddr::new(address, port))',
    'forbidden_authority_header',
    '"content-length"',
    '"transfer-encoding"',
    '"x-correlation-id"',
    'exact_method_path_port_timeout_and_reserved_headers_are_enforced_before_dns',
    'ambiguous_authority_path',
    'ambiguous_authority_path_is_denied_before_dns',
    'routing_override_header',
    'sensitive_credential_header',
    'authority_override_and_plaintext_credential_headers_are_denied_before_dns',
    '[192, 31, 196, 0]',
    '[192, 52, 193, 0]',
    '[192, 88, 99, 0]',
    '[192, 175, 48, 0]',
    'segments[0] == 0x2620',
    'segments[1] == 0x004f',
    'segments[2] == 0x8000',
    'single_authority_fixture_grant',
    'max_response_bytes',
    'acquire_budget',
    'budget_exhaustion_prevents_dns_resolution',
    'record_evidence',
    'embedded_v4_tail',
    'return classify_v4(embedded_v4_tail(address));',
    'segments[0] == 0x5f00',
    'octets[0] == 0',
    '(segments[0] & 0xffc0) == 0xfec0',
    'resolve_and_validate_with_budget',
    'request.timeouts.overall.checked_sub(started.elapsed())',
    'request.timeouts.connect.checked_sub(started.elapsed())',
    '.connect_timeout(connect_remaining)',
    'TransportFailure::before_dispatch(timeout_class)',
    'dns_resolution_obeys_connect_and_overall_deadlines_before_dispatch',
    'cancellation_during_dns_is_before_dispatch',
    'mixed_dns_answer_fails_closed_and_resolver_is_called_once',
    'translation_prefix_cannot_smuggle_metadata_ipv4_through_dns',
    'redirect_oversize_and_slow_responses_are_terminal_and_audited',
    'IANA_ALLOCATED_PROVIDER_PUBLIC_IPV6',
    'protocol != EgressProtocol::Https',
    'secret_bearing_egress_requires_https_before_dns',
    '"2d00::1"',
    '"3000::1"',
    '"4000::1"',
  ],
  egressDispatch: [
    'pub(crate) enum GovernedDispatchError',
    'pub(crate) struct GovernedEgressDispatcher',
    'pub(crate) fn system() -> Self',
    'pub(crate) async fn send(',
    'GovernedEgressTransport::new_with_resolver(',
    '.send(request)',
  ],
  webhook: [
    'pub trait WebhookVerifier',
    'raw_payload: &[u8]',
    'record_verified_webhook',
    'record_rejected_webhook',
    'claim_next_webhook',
    'complete_webhook',
    'retry_webhook',
    'dead_letter_webhook',
  ],
  webhookEvent: [
    'pub enum WebhookEventLane',
    'Postgres(PostgresDurableDriver)',
    'InterconnectWebhookEventPort',
    'MessageKind::Event',
    'integration.webhook.normalized',
    'SELECT created_at_ms FROM interconnect_events',
    'stable_created_at_ms',
    'error.code == InterconnectErrorCode::ContractIncompatible',
    'replay_created_at_ms',
    'payloadHash',
    'replay_is_idempotent_on_the_normalized_event_lane',
  ],
  webhookEventPgTest: [
    'live_pg18_webhook_event_replay_reuses_persisted_envelope_identity',
    'PostgresDurableDriver::new(pool.clone())',
    'ensure_schema()',
    'assert_eq!(second_row.0, 1)',
    'assert_eq!(second_row.1, first_row.1)',
  ],
  governedWorker: [
    'pub struct GovernedIntegrationWorker',
    'InterconnectWebhookEventPort',
    '.run_once(&tenant_id, &FixtureWebhookMapper, &self.webhook_events)',
  ],
  routes: [
    '/api/integrations/webhooks/{endpoint_token}',
    'MAX_FIXTURE_WEBHOOK_BODY_BYTES',
    'fixture_webhook_rate_limit',
    'WebhookReceiptInput',
    'verification_headers_json',
    'stored_headers_json',
  ],
  store: [
    'record_verified_webhook',
    'webhook_inbox',
    'received_at',
    "status = 'processing'",
  ],
  main: [
    'Arc::new(NoopHttpClient)',
    'GovernedIntegrationWorker::new_with_metrics',
    'WebhookEventLane::postgres',
    'R4-P5 production webhook Event Lane requires PostgreSQL 18 durable driver',
    'R4-P5 production webhook Event Lane requires a postgres-enabled build',
  ],
  factory: [
    'legacy_provider_modules_are_not_built_even_when_the_factory_is_constructed',
    'type-only imports keep the descriptor/factory projection complete',
  ],
})

const GRANT_FIELDS = Object.freeze([
  'pub grant_id:',
  'pub destination_host:',
  'pub ports:',
  'pub protocols:',
  'pub method_classes:',
  'pub path_prefixes:',
  'pub redirect_policy:',
  'pub max_connect_timeout_ms:',
  'pub max_read_timeout_ms:',
  'pub max_overall_timeout_ms:',
  'pub max_response_bytes:',
  'pub max_concurrency:',
  'pub rate_limit_per_minute:',
  'pub secret_purposes:',
])

const APPROVED_REQWEST = new Set([
  'backend/src/http/reqwest_client.rs',
  'backend/src/integration/transport.rs',
  'backend/src/integration/egress.rs',
])

const APPROVED_GOVERNED_TRANSPORT_CONSTRUCTION = new Set([
  PATHS.egress,
  PATHS.egressDispatch,
])

function finding(path, evidence) {
  return { rule: RULE, path, evidence, occurrences: 1 }
}

function requiredTokens(failures, path, source, tokens) {
  for (const token of tokens) {
    if (!source.includes(token)) failures.push(finding(path, `missing required token: ${token}`))
  }
}

function block(source, startToken) {
  const start = source.indexOf(startToken)
  if (start < 0) return ''
  let depth = 0
  let opened = false
  for (let index = start; index < source.length; index += 1) {
    if (source[index] === '{') {
      opened = true
      depth += 1
    } else if (source[index] === '}') {
      depth -= 1
      if (opened && depth === 0) return source.slice(start, index + 1)
    }
  }
  return source.slice(start)
}

export function checkR4NetworkAuthority({ files, rustSources }) {
  const failures = []

  for (const [key, path] of Object.entries(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(finding(path, `required R4-P5 ${key} evidence is missing`))
  }
  if (failures.length) return failures

  for (const [key, tokens] of Object.entries(REQUIRED)) {
    requiredTokens(failures, PATHS[key], files[PATHS[key]], tokens)
  }

  const grantStruct = block(files[PATHS.coreGrant], 'pub struct EgressGrant')
  requiredTokens(failures, PATHS.coreGrant, grantStruct, GRANT_FIELDS)

  const grantValidation = block(files[PATHS.coreGrant], 'pub fn validate(&self)')
  if (!grantValidation.includes('self.protocols.len() != 1')) {
    failures.push(finding(PATHS.coreGrant, 'single protocol authority is not enforced'))
  }
  if (!grantValidation.includes('self.method_classes.len() != 1')) {
    failures.push(finding(PATHS.coreGrant, 'single method-class authority is not enforced'))
  }

  if (!files[PATHS.coreTransportMod].includes('pub mod network;')) {
    failures.push(finding(PATHS.coreTransportMod, 'stable network grant contract is not compiled by system-core'))
  }
  for (const token of [
    'pub mod egress;',
    'pub(crate) mod egress_dispatch;',
    'pub mod governed_worker;',
    'pub mod webhook_event;',
    'mod webhook_event_pg_tests;',
  ]) {
    if (!files[PATHS.integrationMod].includes(token)) {
      failures.push(finding(PATHS.integrationMod, `integration module missing ${token}`))
    }
  }

  const sendBlock = block(files[PATHS.egress], 'async fn send(&self, request: ExternalRequest)')
  const budgetIndex = sendBlock.indexOf('let _permit = match self.acquire_budget()')
  const dnsIndex = sendBlock.indexOf('.resolve_and_validate_with_budget(')
  if (budgetIndex < 0 || dnsIndex < 0 || budgetIndex >= dnsIndex) {
    failures.push(
      finding(PATHS.egress, 'rate/concurrency budget admission does not precede DNS resolution'),
    )
  }

  const staticAuthority = block(files[PATHS.egress], 'fn static_authority(&self, request: &ExternalRequest, url: &Url)')
  if (
    !staticAuthority.includes('self.identity.secret_purpose.is_some()')
    || !staticAuthority.includes('protocol != EgressProtocol::Https')
  ) {
    failures.push(
      finding(PATHS.egress, 'secret-bearing provider/application egress is not forced to HTTPS before DNS'),
    )
  }
  if (!staticAuthority.includes('ambiguous_authority_path(path)')) {
    failures.push(
      finding(PATHS.egress, 'ambiguous encoded path authority is not rejected before DNS'),
    )
  }

  const classifyV6 = block(files[PATHS.egress], 'fn classify_v6(address: Ipv6Addr)')
  if (
    !classifyV6.includes('IANA_ALLOCATED_PROVIDER_PUBLIC_IPV6')
    || !classifyV6.includes('DestinationClass::Special')
  ) {
    failures.push(
      finding(PATHS.egress, 'IPv6 public classification is not fail-closed to the IANA allocated snapshot'),
    )
  }

  const tombstone = files[PATHS.coreWebhookTombstone]
  if (/pub\s+trait\s+WebhookHandler\b/.test(tombstone) || /fn\s+path_prefix\s*\(/.test(tombstone)) {
    failures.push(finding(PATHS.coreWebhookTombstone, 'provider-owned webhook listener API was reintroduced'))
  }
  if (!tombstone.includes('ProviderOwnedWebhookListenerRetired')) {
    failures.push(finding(PATHS.coreWebhookTombstone, 'provider-owned listener retirement marker is missing'))
  }

  const eventRuntime = files[PATHS.webhookEvent].split('#[cfg(all(test', 1)[0]
  for (const forbidden of ['raw_payload', 'verification_headers', 'stored_headers_json', 'endpoint_token']) {
    if (eventRuntime.includes(forbidden)) {
      failures.push(finding(PATHS.webhookEvent, `normalized Event Lane leaks ingress-only material: ${forbidden}`))
    }
  }

  const evidenceStruct = block(files[PATHS.egress], 'pub struct EgressEvidence')
  for (const forbidden of ['headers', 'body', 'secret_value', 'authorization']) {
    if (evidenceStruct.toLowerCase().includes(forbidden)) {
      failures.push(finding(PATHS.egress, `egress evidence contains secret/body-bearing field: ${forbidden}`))
    }
  }

  const main = files[PATHS.main]
  if (/ReqwestHttpClient\s*::\s*new\s*\(/.test(main) || /ReqwestExternalCallTransport\s*::\s*new\s*\(/.test(main)) {
    failures.push(finding(PATHS.main, 'production composition root regained ambient HTTP network authority'))
  }
  if (!main.includes('let http_client: Arc<dyn HttpClient> = Arc::new(NoopHttpClient);')) {
    failures.push(finding(PATHS.main, 'legacy ExecutionContext HTTP slot is not fail-closed at composition root'))
  }

  const pgDurabilityGuard = main.indexOf('if config.is_production && pg_pool.is_none()')
  const noPgDurabilityGuard = main.indexOf('#[cfg(not(feature = "postgres"))]\n    if config.is_production')
  const firstSideEffect = Math.min(
    ...['sentry::init', 'let pool = create_pool(', 'ensure_initial_admin(', 'worker_runner.run_startup()']
      .map((token) => main.indexOf(token))
      .filter((index) => index >= 0),
  )
  if (
    pgDurabilityGuard < 0
    || noPgDurabilityGuard < 0
    || !Number.isFinite(firstSideEffect)
    || pgDurabilityGuard >= firstSideEffect
    || noPgDurabilityGuard >= firstSideEffect
  ) {
    failures.push(
      finding(PATHS.main, 'production webhook durability admission does not precede platform/application side effects'),
    )
  }

  const factory = files[PATHS.factory]
  for (const provider of ['FeatureSfExpress', 'FeatureWechatPay', 'FeatureAlipay', 'FeatureMiniapp']) {
    const constructedProvider = new RegExp(`(?:constructed\\s*\\(\\s*${provider}|${provider}\\s*::\\s*new\\s*\\()`)
    if (constructedProvider.test(factory)) {
      failures.push(finding(PATHS.factory, `deferred provider module became constructible in Core Registry: ${provider}`))
    }
  }

  for (const [path, source] of Object.entries(rustSources)) {
    if (source.includes('reqwest::') && !APPROVED_REQWEST.has(path) && !path.startsWith('backend/tests/')) {
      failures.push(finding(path, 'direct reqwest usage exists outside the bounded/compatibility transport files'))
    }
    if (path !== 'backend/src/integration/transport.rs' && source.includes('ReqwestExternalCallTransport::new(')) {
      failures.push(finding(path, 'legacy Stage-2 reqwest transport was constructed outside its compatibility module'))
    }
    if (path !== 'backend/src/http/reqwest_client.rs' && source.includes('ReqwestHttpClient::new(')) {
      failures.push(finding(path, 'legacy synchronous HTTP client was constructed outside its compatibility module'))
    }
    if (
      !APPROVED_GOVERNED_TRANSPORT_CONSTRUCTION.has(path)
      && (source.includes('GovernedEgressTransport::new(')
        || source.includes('GovernedEgressTransport::new_with_resolver('))
    ) {
      failures.push(
        finding(path, 'P5 governed egress transport was runtime-constructed before P6 shared budget lifecycle authority'),
      )
    }
  }

  return failures
}

function collectRust(root, directory, output) {
  const absolute = join(root, directory)
  if (!existsSync(absolute)) return
  for (const entry of readdirSync(absolute, { withFileTypes: true })) {
    const child = join(directory, entry.name)
    if (entry.isDirectory()) collectRust(root, child, output)
    else if (entry.isFile() && entry.name.endsWith('.rs')) {
      output[child.replaceAll('\\', '/')] = readFileSync(join(root, child), 'utf8')
    }
  }
}

export function loadRepository(root) {
  const files = {}
  for (const path of Object.values(PATHS)) {
    const absolute = join(root, path)
    if (existsSync(absolute)) files[path] = readFileSync(absolute, 'utf8')
  }
  const rustSources = {}
  collectRust(root, 'backend', rustSources)
  return { files, rustSources }
}

function main() {
  const failures = checkR4NetworkAuthority(loadRepository(process.cwd()))
  if (failures.length) {
    console.error('R4-P5 network authority check failed:')
    for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
    process.exitCode = 1
    return
  }
  console.log('R4-P5 network authority check passed.')
}

const invokedPath = process.argv[1] ? pathToFileURL(process.argv[1]).href : null
if (invokedPath === import.meta.url) main()
