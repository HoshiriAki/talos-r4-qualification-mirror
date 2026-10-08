#!/usr/bin/env node

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { checkR1P2PricePageCutover } from './check-r1p2-pricepage-cutover.mjs'

const paths = [
  'frontend/src/pages/PricePage.vue',
  'frontend/src/components/price/DeviceAccessoryPanel.vue',
  'frontend/src/api/quotes.ts',
  'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p2-quote-pricing-orderline.md',
]

const baseline = Object.fromEntries(paths.map(path => [path, readFileSync(join(process.cwd(), path), 'utf8')]))
assert.deepEqual(checkR1P2PricePageCutover(baseline), [])

const cases = [
  {
    name: 'rejects legacy Order create authority',
    path: 'frontend/src/pages/PricePage.vue',
    mutate: source => `${source}\n<!-- orders.create( legacy fixture -->`,
  },
  {
    name: 'rejects client order number authority',
    path: 'frontend/src/pages/PricePage.vue',
    mutate: source => `${source}\n<!-- orderNo.value legacy fixture -->`,
  },
  {
    name: 'rejects first-model pricing shortcut',
    path: 'frontend/src/pages/PricePage.vue',
    mutate: source => `${source}\n<!-- estimateModelId legacy fixture -->`,
  },
  {
    name: 'rejects accessory UI price ownership',
    path: 'frontend/src/components/price/DeviceAccessoryPanel.vue',
    mutate: source => source.replace('interface AccItem { id: string; name: string }', 'interface AccItem { id: string; name: string; price: number }'),
  },
  {
    name: 'requires Quote V2 confirm endpoint',
    path: 'frontend/src/api/quotes.ts',
    mutate: source => source.replace('/confirm', '/accept-removed'),
  },
  {
    name: 'requires documented active PricePage cutover',
    path: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p2-quote-pricing-orderline.md',
    mutate: source => source.replaceAll('PRICEPAGE_QUOTE_V2_CUTOVER', 'PRICEPAGE_CUTOVER_REMOVED'),
  },
]

for (const testCase of cases) {
  const files = { ...baseline, [testCase.path]: testCase.mutate(baseline[testCase.path]) }
  assert.ok(checkR1P2PricePageCutover(files).length > 0, `${testCase.name}: expected fail-closed finding`)
}

console.log(`R1-P2 PricePage cutover fixtures passed: ${cases.length} negative`)
