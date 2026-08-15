import { stat } from 'node:fs/promises';

const budgets = [
  ['../../frontend/assets/app.js', 150 * 1024],
  ['../../frontend/assets/app.css', 50 * 1024],
];

let failed = false;
for (const [relative, budget] of budgets) {
  const url = new URL(relative, import.meta.url);
  const { size } = await stat(url);
  const name = relative.split('/').at(-1);
  const kib = (size / 1024).toFixed(1);
  const budgetKib = (budget / 1024).toFixed(0);
  console.log(`${name}: ${kib} KiB / ${budgetKib} KiB`);
  if (size > budget) {
    console.error(`${name} exceeds its uncompressed bundle budget`);
    failed = true;
  }
}

if (failed) process.exitCode = 1;
