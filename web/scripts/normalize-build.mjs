import { readFile, writeFile } from 'node:fs/promises';

for (const relative of ['../../frontend/index.html', '../../frontend/assets/app.css', '../../frontend/assets/app.js']) {
  const url = new URL(relative, import.meta.url);
  const content = await readFile(url, 'utf8');
  await writeFile(url, `${content.trimEnd()}\n`);
}
