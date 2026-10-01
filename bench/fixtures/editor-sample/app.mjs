import { readFileSync } from 'node:fs';

export function readConfig(path = new URL('./config.json', import.meta.url)) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

export function requestOptions(config) {
  return { url: config.endpoint, timeout: config.timeoutMs };
}
