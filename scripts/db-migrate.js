const { runMigrations } = require('../src/db');

function main() {
  const result = runMigrations();
  if (result.executed.length > 0) {
    console.log(`Applied migrations: ${result.executed.join(', ')}`);
  } else {
    console.log('No pending migrations.');
  }
}

main();
