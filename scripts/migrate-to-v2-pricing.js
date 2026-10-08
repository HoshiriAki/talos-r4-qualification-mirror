/**
 * Talos v2 Pricing Engine — Data Migration Script
 *
 * Run manually after deploying Phase 1-3 code:
 *   node scripts/migrate-to-v2-pricing.js
 *
 * Idempotent — safe to run multiple times.
 *
 * Steps:
 * 1. Auto-detect device models from serial prefixes
 * 2. Populate warehouses (上海仓/珠海仓/南昌友仓)
 * 3. Populate region rules (31 provinces × 3 warehouses)
 * 4. Backfill orders.province from notes
 * 5. Assign default warehouse to all devices
 */

const { randomUUID } = require('crypto');

// Bootstrap the DB just like the server does
process.env.NODE_ENV = process.env.NODE_ENV || 'development';

function log(msg) {
  process.stdout.write(`[migrate] ${msg}\n`);
}

const { db, runMigrations } = require('../src/db');
const { shanghaiNowIsoString } = require('../src/utils/time');

// Ensure migration 005 runs first
const migrationResult = runMigrations();
if (migrationResult.executed.length > 0) {
  log(`Ran migrations: ${migrationResult.executed.join(', ')}`);
}

const now = shanghaiNowIsoString();

// ── Step 1: Auto-detect device models ──

function step1DetectModels() {
  log('Step 1: Detecting device models from serial prefixes...');

  const devices = db.prepare('SELECT serialNo FROM devices').all();
  const prefixCounts = new Map();

  for (const d of devices) {
    const serial = String(d.serialNo || '').trim().toUpperCase();
    if (!serial) continue;
    const prefix = serial.slice(0, 2);
    prefixCounts.set(prefix, (prefixCounts.get(prefix) || 0) + 1);
  }

  // Known prefixes
  const knownModels = [
    { name: 'Pocket 3', category: 'camera', prefix: '5W' },
    { name: 'Action 系列', category: 'camera', prefix: 'AN' },
    { name: 'DJI Mic', category: 'audio', prefix: 'DM' },
  ];

  let created = 0;
  for (const modelDef of knownModels) {
    const existing = db.prepare('SELECT 1 FROM device_models WHERE prefix = ? LIMIT 1').get(modelDef.prefix);
    if (existing) continue;

    const id = randomUUID();
    db.prepare(`INSERT INTO device_models (id, name, category, prefix, enabled, createdAt, updatedAt) VALUES (?, ?, ?, ?, 1, ?, ?)`)
      .run(id, modelDef.name, modelDef.category, modelDef.prefix, now, now);

    // Use global pricing as default
    const globalConfig = db.prepare('SELECT * FROM pricing_configs WHERE id = 1 LIMIT 1').get();
    const wd = globalConfig ? globalConfig.baseWeekdayPrice : 8.5;
    const we = globalConfig ? globalConfig.baseWeekendPrice : 14;
    db.prepare(`INSERT INTO model_base_prices (modelId, weekdayPrice, weekendPrice, updatedBy, createdAt, updatedAt) VALUES (?, ?, ?, '', ?, ?)`)
      .run(id, wd, we, now, now);

    log(`  Created model: ${modelDef.name} (${modelDef.prefix})`);
    created++;
  }

  // Auto-create models for unknown prefixes with significant device counts
  for (const [prefix, count] of prefixCounts) {
    if (count < 3) continue;
    const alreadyCovered = knownModels.some(m => m.prefix === prefix);
    if (alreadyCovered) continue;

    const existing = db.prepare('SELECT 1 FROM device_models WHERE prefix = ? LIMIT 1').get(prefix);
    if (existing) continue;

    const id = randomUUID();
    const autoName = `Auto-${prefix}`;
    db.prepare(`INSERT INTO device_models (id, name, category, prefix, enabled, createdAt, updatedAt) VALUES (?, ?, 'other', ?, 1, ?, ?)`)
      .run(id, autoName, prefix, now, now);

    const globalConfig = db.prepare('SELECT * FROM pricing_configs WHERE id = 1 LIMIT 1').get();
    const wd = globalConfig ? globalConfig.baseWeekdayPrice : 8.5;
    const we = globalConfig ? globalConfig.baseWeekendPrice : 14;
    db.prepare(`INSERT INTO model_base_prices (modelId, weekdayPrice, weekendPrice, updatedBy, createdAt, updatedAt) VALUES (?, ?, ?, '', ?, ?)`)
      .run(id, wd, we, now, now);

    log(`  Created auto-model: ${autoName} (${count} devices)`);
    created++;
  }

  // Update devices.modelId
  const models = db.prepare('SELECT id, prefix FROM device_models WHERE enabled = 1').all();
  let updatedDevices = 0;
  for (const model of models) {
    const result = db.prepare(`
      UPDATE devices SET modelId = ?
      WHERE (modelId IS NULL OR modelId = '')
        AND UPPER(serialNo) LIKE ?
    `).run(model.id, `${model.prefix.toUpperCase()}%`);
    updatedDevices += result.changes;
  }
  log(`  Updated ${updatedDevices} devices with modelId`);

  return created;
}

// ── Step 2: Populate warehouses ──

function step2PopulateWarehouses() {
  log('Step 2: Populating warehouses...');

  const warehouses = [
    { name: '上海仓', type: 'owned' },
    { name: '珠海仓', type: 'owned' },
    { name: '南昌友仓', type: 'partner' },
  ];

  let created = 0;
  for (const wh of warehouses) {
    const existing = db.prepare('SELECT 1 FROM warehouses WHERE name = ? LIMIT 1').get(wh.name);
    if (existing) continue;

    db.prepare(`INSERT INTO warehouses (id, name, type, enabled, createdAt, updatedAt) VALUES (?, ?, ?, 1, ?, ?)`)
      .run(randomUUID(), wh.name, wh.type, now, now);
    log(`  Created warehouse: ${wh.name} (${wh.type})`);
    created++;
  }

  return created;
}

// ── Step 3: Populate region rules ──

function step3PopulateRegionRules() {
  log('Step 3: Populating region rules...');

  const { LOGISTICS_MATRIX } = require('../src/services/logistics-service');

  const warehouses = db.prepare('SELECT id, name FROM warehouses WHERE enabled = 1').all();
  let created = 0;

  for (const wh of warehouses) {
    const whMatrix = LOGISTICS_MATRIX[wh.name];
    if (!whMatrix) continue;

    for (const [province, estimate] of Object.entries(whMatrix)) {
      const existing = db.prepare(
        'SELECT 1 FROM warehouse_region_rules WHERE warehouseId = ? AND province = ? LIMIT 1'
      ).get(wh.id, province);
      if (existing) continue;

      db.prepare(`
        INSERT INTO warehouse_region_rules (id, warehouseId, province, shippingDays, returnDays, isPrimary, createdAt, updatedAt)
        VALUES (?, ?, ?, ?, ?, 0, ?, ?)
      `).run(randomUUID(), wh.id, province, estimate.shippingDays, estimate.returnDays, now, now);
      created++;
    }
  }

  // Set primary flag for best route per province
  const provinces = Object.keys(LOGISTICS_MATRIX['上海仓']);
  for (const province of provinces) {
    const rules = db.prepare(`
      SELECT wrr.id, wrr.shippingDays
      FROM warehouse_region_rules wrr
      JOIN warehouses w ON w.id = wrr.warehouseId
      WHERE wrr.province = ? AND w.enabled = 1
      ORDER BY wrr.shippingDays ASC
    `).all(province);

    if (rules.length > 0) {
      db.prepare('UPDATE warehouse_region_rules SET isPrimary = (id = ?) WHERE province = ?')
        .run(rules[0].id, province);
    }
  }

  log(`  Created ${created} region rules`);
  return created;
}

// ── Step 4: Backfill orders.province from notes ──

function step4BackfillOrders() {
  log('Step 4: Backfilling orders.province from notes...');

  const PROVINCE_LIST = [
    '北京', '天津', '上海', '重庆',
    '河北', '山西', '辽宁', '吉林', '黑龙江',
    '江苏', '浙江', '安徽', '福建', '江西', '山东',
    '河南', '湖北', '湖南', '广东', '广西', '海南',
    '四川', '贵州', '云南', '西藏',
    '陕西', '甘肃', '青海', '宁夏', '新疆',
    '内蒙古',
  ];

  const orders = db.prepare("SELECT id, notes, province FROM orders WHERE province IS NULL OR province = ''").all();

  let updated = 0;
  for (const order of orders) {
    const notes = String(order.notes || '');
    let found = '';

    for (const p of PROVINCE_LIST) {
      if (notes.includes(p)) {
        found = p;
        break;
      }
    }

    if (found) {
      db.prepare('UPDATE orders SET province = ? WHERE id = ?').run(found, order.id);
      updated++;
    }
  }

  log(`  Backfilled ${updated}/${orders.length} orders with province`);
  return updated;
}

// ── Step 5: Assign default warehouse to devices ──

function step5AssignDeviceWarehouses() {
  log('Step 5: Assigning default warehouse to devices...');

  const shanghai = db.prepare("SELECT id FROM warehouses WHERE name = '上海仓' LIMIT 1").get();
  if (!shanghai) {
    log('  WARNING: 上海仓 not found, skipping');
    return 0;
  }

  const result = db.prepare(`
    UPDATE devices SET currentWarehouseId = ?
    WHERE currentWarehouseId IS NULL OR currentWarehouseId = ''
  `).run(shanghai.id);

  log(`  Set default warehouse for ${result.changes} devices`);
  return result.changes;
}

// ── Main ──

function main() {
  log('=== Talos v2 Pricing Migration ===');
  log('');

  const s1 = step1DetectModels();
  log('');
  const s2 = step2PopulateWarehouses();
  log('');
  const s3 = step3PopulateRegionRules();
  log('');
  const s4 = step4BackfillOrders();
  log('');
  const s5 = step5AssignDeviceWarehouses();

  log('');
  log('=== Migration complete ===');
  log(`Models created: ${s1}`);
  log(`Warehouses created: ${s2}`);
  log(`Region rules created: ${s3}`);
  log(`Orders backfilled: ${s4}`);
  log(`Devices assigned: ${s5}`);
}

main();
