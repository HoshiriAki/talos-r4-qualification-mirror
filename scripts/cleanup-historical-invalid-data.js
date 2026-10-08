const { runMigrations } = require('../src/db');
const { cleanupHistoricalInvalidData } = require('../src/services/data-maintenance-service');

function main() {
  runMigrations();
  const result = cleanupHistoricalInvalidData();

  console.log(`已删除不合规设备 ${result.removedDevices} 条`);
  console.log(`已删除不合规订单 ${result.removedOrders} 条`);
  console.log(`已删除无效关联 ${result.removedRelations} 条`);
}

main();
