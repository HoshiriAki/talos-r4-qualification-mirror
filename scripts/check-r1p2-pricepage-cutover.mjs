#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import process from 'node:process'

const RULE = 'TALOS-OPS-028'
const PATHS = Object.freeze({
  page: 'frontend/src/pages/PricePage.vue',
  accessories: 'frontend/src/components/price/DeviceAccessoryPanel.vue',
  api: 'frontend/src/api/quotes.ts',
  record: 'policy/qualification/legacy-evidence/docs/proposals/talos-ops-business-closure/r1-p2-quote-pricing-orderline.md',
})

function finding(path, evidence) {
  return { rule: RULE, path, evidence, occurrences: 1 }
}

function requireText(failures, source, token, path, evidence) {
  if (!source.includes(token)) failures.push(finding(path, evidence))
}

function forbidText(failures, source, token, path, evidence) {
  if (source.includes(token)) failures.push(finding(path, evidence))
}

export function checkR1P2PricePageCutover(files) {
  const failures = []
  for (const path of Object.values(PATHS)) {
    if (!Object.hasOwn(files, path)) failures.push(finding(path, 'required PricePage cutover evidence is missing'))
  }
  if (failures.length) return failures

  const page = files[PATHS.page]
  const accessories = files[PATHS.accessories]
  const api = files[PATHS.api]
  const record = files[PATHS.record]

  for (const token of [
    'createQuote',
    'confirmQuote',
    'createOrderFromQuote',
    'fetchCustomers',
    'SERVER GENERATED',
    'CLIENT PRICE AUTHORITY: NONE',
    'DEVICE SERIALS',
    'NOT ALLOCATED',
  ]) requireText(failures, page, token, PATHS.page, `active PricePage missing ${token}`)

  for (const forbidden of [
    'useOrdersStore',
    'orders.create(',
    'orderNo.value',
    'ACCESSORY_DAILY',
    'accessoryDailyPrice',
    'estimateModelId',
    'usePricingCalc',
  ]) forbidText(failures, page, forbidden, PATHS.page, `legacy PricePage authority remains: ${forbidden}`)

  requireText(failures, accessories, '价格由服务器报价', PATHS.accessories, 'accessory selector must disclose server price authority')
  requireText(failures, accessories, 'SERVER PRICE', PATHS.accessories, 'accessory selector must not display client prices')
  forbidText(failures, accessories, 'price: number', PATHS.accessories, 'accessory UI model must not own price values')
  forbidText(failures, accessories, '¥{{ acc.price', PATHS.accessories, 'hardcoded accessory price display is forbidden')

  for (const token of [
    "requestJson('/api/v2/quotes'",
    '/api/v2/quotes/${encodeURIComponent(id)}/confirm',
    '/api/v2/quotes/${encodeURIComponent(id)}/orders',
    "requestJson('/api/v2/customers'",
  ]) requireText(failures, api, token, PATHS.api, `Quote V2 frontend API missing ${token}`)

  requireText(failures, record, 'PRICEPAGE_QUOTE_V2_CUTOVER', PATHS.record, 'R1-P2 record must bind active PricePage cutover evidence')
  return failures
}

function loadFiles(root) {
  const files = {}
  for (const path of Object.values(PATHS)) {
    const absolute = join(root, path)
    if (existsSync(absolute)) files[path] = readFileSync(absolute, 'utf8')
  }
  return files
}

const failures = checkR1P2PricePageCutover(loadFiles(process.cwd()))
if (failures.length) {
  console.error('R1-P2 PricePage cutover check failed:')
  for (const item of failures) console.error(`- ${item.rule} ${item.path}: ${item.evidence}`)
  process.exitCode = 1
} else {
  console.log('R1-P2 PricePage cutover check passed.')
}
