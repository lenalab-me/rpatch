import { readFileSync } from 'node:fs';

const MAX_ITEMS = 20;

export function loadItems(path) {
  const items = JSON.parse(readFileSync(path, 'utf8'));
  return items.slice(0, MAX_ITEMS);
}

export function formatItem(item) {
  return `${item.id}: ${item.title}`;
}
