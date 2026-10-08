import assert from 'node:assert/strict'

import {
  contract,
  validate,
} from './generate-r4-public-api-capabilities.mjs'

const result = contract()
validate(result)

assert.equal(result.schema, 'talos.public-api-capabilities/v1')
assert.equal(result.requestIdentity.requestId.header, 'x-request-id')
assert.equal(result.requestIdentity.requestId.callerValueTrusted, false)
assert.equal(result.requestIdentity.machineCorrelation.header, 'x-correlation-id')
assert.equal(result.requestIdentity.publicTraceHeader, null)

assert.equal(result.idempotency.machineExecute.requestHeader, 'idempotency-key')
assert.equal(result.idempotency.machineExecute.minLength, 16)
assert.equal(result.idempotency.machineExecute.maxLength, 128)
assert.equal(result.idempotency.machineExecute.universalReplayGuarantee, false)
assert.equal(result.idempotency.browserSession.genericIdempotencyHeaderContract, false)
assert.equal(result.idempotency.providerWebhook.providerEventIdHeader, 'x-integration-event-id')

assert.equal(result.rateQuota.browserSession.globalRequestQuota, false)
assert.equal(
  result.rateQuota.browserSession.loginFailureLimiter.durability,
  'durable auth_rate_limit_state',
)
assert.deepEqual(
  result.rateQuota.browserSession.loginFailureLimiter.additionalDimensions,
  ['normalized username', 'ResolvedClientIp'],
)
assert.equal(
  result.rateQuota.browserSession.loginFailureLimiter.keyPersistence,
  'SHA-256 digests only',
)
assert.equal(
  result.rateQuota.browserSession.changePasswordFailureLimiter.durability,
  'durable auth_rate_limit_state',
)
assert.equal(
  result.rateQuota.browserSession.changePasswordFailureLimiter.keyPersistence,
  'SHA-256 digest only',
)
assert.equal(result.rateQuota.machineBearer.preAuthIngress.limit, 30)
assert.equal(result.rateQuota.machineBearer.preAuthIngress.windowSeconds, 60)
assert.equal(result.rateQuota.machineBearer.preAuthIngress.maxBuckets, 4096)
assert.equal(result.rateQuota.machineBearer.postAuthClient.rpmMin, 1)
assert.equal(result.rateQuota.machineBearer.postAuthClient.rpmMax, 600)
assert.equal(result.rateQuota.machineBearer.postAuthClient.recheckedOnEveryRequest, true)
assert.equal(result.rateQuota.providerWebhook.preBodyIngress.limit, 60)
assert.equal(result.rateQuota.providerWebhook.preBodyIngress.windowSeconds, 60)
assert.equal(result.rateQuota.providerWebhook.preBodyIngress.maxBuckets, 4096)
assert.equal(result.rateQuota.providerWebhook.preBodyIngress.fixtureOnly, true)

assert.equal(result.responseSize.globalHardLimitBytes, null)
assert.deepEqual(result.responseSize.explicitBinaryExportRoutes, ['/users/export', '/devices/export'])
assert.equal(result.streamingInflight.webSocketPublicSurface, false)
assert.equal(result.streamingInflight.ssePublicSurface, false)
assert.equal(result.streamingInflight.globalPublicInflightLimit, null)

console.log('R4-P4 public API capability matrix tests: PASS')
