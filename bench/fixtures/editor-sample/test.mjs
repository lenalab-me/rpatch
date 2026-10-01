import assert from 'node:assert/strict';
import { readConfig, requestOptions } from './app.mjs';

const config = readConfig();
assert.equal(config.endpoint, 'https://example.test/api');
assert.deepEqual(requestOptions(config), {
  url: 'https://example.test/api',
  timeout: 1000,
});
console.log('ok');
