#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import process from 'node:process';
import { spawnSync } from 'node:child_process';

const root = process.cwd();
const checker = path.join(root, 'scripts/check-r1-closure-order-authority.mjs');
const files = [
  'backend/src/application/order_lifecycle_compatibility.rs',
  'backend/src/routes/orders.rs',
  'backend/src/services/order_compatibility_support.rs',
];

function fixture() {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'talos-r1-closure-'));
  for (const relative of files) {
    const target = path.join(temp, relative);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.copyFileSync(path.join(root, relative), target);
  }
  return temp;
}

function run(fixtureRoot = root) {
  return spawnSync(process.execPath, [checker], {
    cwd: root,
    env: { ...process.env, TALOS_R1_CLOSURE_ROOT: fixtureRoot },
    encoding: 'utf8',
  });
}

function expectFailure(name, mutate, expected) {
  const temp = fixture();
  try {
    mutate(temp);
    const result = run(temp);
    const output = `${result.stdout}${result.stderr}`;
    if (result.status === 0 || !output.includes(expected)) {
      throw new Error(`${name} did not fail closed\n${output}`);
    }
  } finally {
    fs.rmSync(temp, { recursive: true, force: true });
  }
}

const baseline = run();
if (baseline.status !== 0) throw new Error(`${baseline.stdout}${baseline.stderr}`);

expectFailure('legacy service resurrection', (temp) => {
  const file = path.join(temp, 'backend/src/services/order_service.rs');
  fs.writeFileSync(file, 'pub fn update_order() {}\n');
}, 'legacy order_service.rs still exists');

expectFailure('generic route update regression', (temp) => {
  const file = path.join(temp, 'backend/src/routes/orders.rs');
  fs.appendFileSync(file, '\nconst REGRESSION: &str = "update_order";\n');
}, 'orders route retains forbidden write');

expectFailure('named command removal', (temp) => {
  const file = path.join(temp, 'backend/src/application/order_lifecycle_compatibility.rs');
  fs.writeFileSync(file, fs.readFileSync(file, 'utf8').replaceAll('fixture_dispatch', 'removed_dispatch'));
}, 'named command missing: fixture_dispatch');

expectFailure('generic application delegation regression', (temp) => {
  const file = path.join(temp, 'backend/src/application/order_lifecycle_compatibility.rs');
  fs.appendFileSync(file, '\nconst REGRESSION: &str = "self.legacy.execute("update_order"";\n');
}, 'compatibility application delegates retired generic write');

expectFailure('device import batch command regression', (temp) => {
  const file = path.join(temp, 'backend/src/routes/orders.rs');
  fs.writeFileSync(file, fs.readFileSync(file, 'utf8').replaceAll('allocate_devices_batch', 'allocate_device'));
}, 'device import must use the canonical atomic batch allocation command');

process.stdout.write('R1 Order closure negative fixtures: PASS\n');
