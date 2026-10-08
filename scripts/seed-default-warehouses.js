/**
 * Seed default warehouses and region rules on first deployment.
 * Only runs when the warehouses table is empty.
 * Data sourced from logistics-service.js hardcoded matrix.
 */
const { db } = require('../src/db');
const { shanghaiNowIsoString } = require('../src/utils/time');
const crypto = require('crypto');

function genId() {
  return crypto.randomUUID();
}

const SHANGHAI_MATRIX = {
  '上海': { shippingDays: 1, returnDays: 1 },
  '江苏': { shippingDays: 1, returnDays: 1 },
  '浙江': { shippingDays: 1, returnDays: 1 },
  '安徽': { shippingDays: 1, returnDays: 2 },
  '北京': { shippingDays: 2, returnDays: 2 },
  '天津': { shippingDays: 2, returnDays: 2 },
  '河北': { shippingDays: 2, returnDays: 2 },
  '山东': { shippingDays: 2, returnDays: 2 },
  '河南': { shippingDays: 2, returnDays: 2 },
  '山西': { shippingDays: 2, returnDays: 3 },
  '湖北': { shippingDays: 2, returnDays: 2 },
  '湖南': { shippingDays: 2, returnDays: 3 },
  '江西': { shippingDays: 1, returnDays: 2 },
  '福建': { shippingDays: 2, returnDays: 2 },
  '广东': { shippingDays: 2, returnDays: 3 },
  '广西': { shippingDays: 3, returnDays: 3 },
  '海南': { shippingDays: 3, returnDays: 4 },
  '辽宁': { shippingDays: 2, returnDays: 3 },
  '吉林': { shippingDays: 3, returnDays: 3 },
  '黑龙江': { shippingDays: 3, returnDays: 4 },
  '内蒙古': { shippingDays: 3, returnDays: 4 },
  '陕西': { shippingDays: 2, returnDays: 3 },
  '甘肃': { shippingDays: 3, returnDays: 4 },
  '宁夏': { shippingDays: 3, returnDays: 4 },
  '青海': { shippingDays: 4, returnDays: 5 },
  '新疆': { shippingDays: 4, returnDays: 5 },
  '西藏': { shippingDays: 4, returnDays: 5 },
  '四川': { shippingDays: 2, returnDays: 3 },
  '重庆': { shippingDays: 2, returnDays: 2 },
  '贵州': { shippingDays: 3, returnDays: 3 },
  '云南': { shippingDays: 3, returnDays: 4 },
};

function seedDefaultWarehouses() {
  const count = db.prepare('SELECT COUNT(*) AS cnt FROM warehouses').get();
  if (count && count.cnt > 0) {
    console.log('[seed] warehouses already exist — skipping seed');
    return;
  }

  console.log('[seed] initializing default warehouse (上海仓)...');

  const now = shanghaiNowIsoString();
  const warehouseId = genId();

  const seed = db.transaction(() => {
    db.prepare(`
      INSERT INTO warehouses (id, name, type, enabled, address, contactName, contactPhone, notes, capacity, createdAt, updatedAt)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `).run(warehouseId, '上海仓', 'owned', 1, '上海市浦东新区', '', '', '', 0, now, now);

    const insertRule = db.prepare(`
      INSERT INTO warehouse_region_rules (id, warehouseId, province, shippingDays, returnDays, isPrimary, createdAt, updatedAt)
      VALUES (?, ?, ?, ?, ?, 1, ?, ?)
    `);

    for (const [province, days] of Object.entries(SHANGHAI_MATRIX)) {
      insertRule.run(genId(), warehouseId, province, days.shippingDays, days.returnDays, now, now);
    }

    console.log(`[seed] 上海仓 created with ${Object.keys(SHANGHAI_MATRIX).length} provinces`);
  });

  seed();
}

module.exports = { seedDefaultWarehouses };
