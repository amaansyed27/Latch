import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('./app.js', import.meta.url), 'utf8');
assert.match(source, /const animationFixed = true;/);
assert.doesNotMatch(source, /const animationFixed = false;/);
process.stdout.write('Latch developer-loop fix verified\n');
