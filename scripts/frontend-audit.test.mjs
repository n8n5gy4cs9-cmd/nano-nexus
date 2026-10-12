import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

// T14.01: feature code reaches the backend only through src/lib/api.ts (the Transport facade), so the same
// UI runs over Tauri IPC and over the LAN Share WebSocket.
const SRC = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', 'apps', 'desktop', 'src');

function files(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name);
    return e.isDirectory() ? files(p) : /\.(ts|tsx)$/.test(e.name) ? [p] : [];
  });
}

const rel = (p) => path.relative(SRC, p).split(path.sep).join('/');
// Test files may quote the forbidden code (lib/lint-rules.test.ts checks the ESLint rule with it).
const feature = files(SRC).filter((f) => !rel(f).startsWith('lib/transport/') && rel(f) !== 'bindings.ts' && !/\.test\.tsx?$/.test(f));

test('only the Tauri transport imports the Tauri API or calls invoke', () => {
  const offenders = [];
  for (const f of feature) {
    const text = fs.readFileSync(f, 'utf8');
    if (/from\s+["']@tauri-apps\//.test(text) || /\binvoke\s*\(/.test(text) || /__TAURI/.test(text)) offenders.push(rel(f));
  }
  assert.deepEqual(offenders, []);
});

test('feature code takes only types from the generated bindings, never commands or events', () => {
  const offenders = [];
  for (const f of feature) {
    const text = fs.readFileSync(f, 'utf8');
    for (const m of text.matchAll(/import\s+(type\s+)?\{([^}]*)\}\s+from\s+["']@\/bindings["']/g)) {
      const names = m[2].split(',').map((n) => n.trim().replace(/^type\s+/, '').split(/\s+as\s+/)[0]);
      if (!m[1] && names.some((n) => n === 'commands' || n === 'events')) offenders.push(rel(f));
    }
  }
  assert.deepEqual(offenders, []);
});

test('the scan sees the code it is meant to guard', () => {
  assert.ok(feature.length > 80, `only ${feature.length} files scanned`);
  assert.ok(files(path.join(SRC, 'lib', 'transport')).some((f) => f.endsWith('tauriTransport.ts')));
});
