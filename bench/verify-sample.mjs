// Independent checks live outside the agent's working copy.
import assert from 'node:assert/strict';
import { readFileSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const { readConfig, requestOptions } = await import(pathToFileURL(resolve('app.mjs')));
const stored = JSON.parse(readFileSync('config.json', 'utf8'));
assert.equal(stored.apiBase, 'https://example.test/api');
assert.equal(Object.hasOwn(stored, 'endpoint'), false);
assert.deepEqual(requestOptions(readConfig()), { url: stored.apiBase, timeout: 1000 });
const temp = mkdtempSync(join(tmpdir(), 'rpatch-verify-'));
try {
  const file = join(temp, 'config.json');
  for (const [config, expected] of [
    [{ endpoint: 'https://legacy.test', timeoutMs: 42 }, 'https://legacy.test'],
    [{ endpoint: 'https://legacy.test', apiBase: 'https://new.test', timeoutMs: 42 }, 'https://new.test'],
  ]) {
    writeFileSync(file, JSON.stringify(config));
    assert.deepEqual(requestOptions(readConfig(file)), { url: expected, timeout: 42 });
  }
} finally {
  rmSync(temp, { recursive: true, force: true });
}
const docs = readFileSync('README.md', 'utf8');
assert.match(docs, /apiBase/);
if (process.env.RPATCH_BENCH_CASE === '02-docs') {
  const usage = docs.indexOf('## 使い方');
  const faq = docs.indexOf('## よくある質問');
  const data = docs.indexOf('## データ形式');
  assert.ok(usage >= 0 && usage < faq && faq < data, 'FAQ belongs between usage and data format');
}
console.log('independent checks: ok');
