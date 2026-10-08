import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve(process.env.TALOS_R1_CLOSURE_ROOT || process.cwd());
const read = (relative) => fs.readFileSync(path.join(root, relative), 'utf8');
const failures = [];

if (fs.existsSync(path.join(root, 'backend/src/services/order_service.rs'))) failures.push('legacy order_service.rs still exists');

const routes = read('backend/src/routes/orders.rs');
for (const forbidden of ['order_service::', '"update_order"', '"delete_order"', 'batch_update_order_status', 'link_device_to_order(&state.pool']) {
  if (routes.includes(forbidden)) failures.push(`orders route retains forbidden write: ${forbidden}`);
}

const support = read('backend/src/services/order_compatibility_support.rs');
for (const forbidden of ['pub fn create_order(', 'pub fn update_order(', 'pub fn delete_order(', 'pub fn link_device_to_order(', 'pub fn detach_device_from_order(', 'pub fn batch_update_order_status(']) {
  if (support.includes(forbidden)) failures.push(`compatibility support retains write owner: ${forbidden}`);
}

const application = read('backend/src/application/order_lifecycle_compatibility.rs');
for (const required of ['change_draft_dates', 'change_draft_address', 'change_notes', 'import_order', 'allocate_device', 'allocate_devices_batch', 'release_device', 'fixture_dispatch']) {
  if (!application.includes(required)) failures.push(`named command missing: ${required}`);
}
for (const forbidden of ['self.legacy.execute("create_order"', 'self.legacy.execute("update_order"', 'self.legacy.execute("delete_order"']) {
  if (application.includes(forbidden)) failures.push(`compatibility application delegates retired generic write: ${forbidden}`);
}
if (!routes.includes('"allocate_devices_batch"')) failures.push('device import must use the canonical atomic batch allocation command');

if (failures.length) {
  console.error(failures.join('\n'));
  process.exit(1);
}
console.log('R1 Order closure authority boundary: PASS');
