import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { validate } from './lib/jsonschema.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const readJson = (p) => JSON.parse(fs.readFileSync(p, 'utf8'));

test('every committed bundle profile matches the schema', () => {
  const dir = path.join(ROOT, 'bundles');
  const files = fs.readdirSync(dir).filter((f) => f.endsWith('.json'));
  assert.ok(files.includes('basic.json') && files.includes('studio.example.json'));
  for (const f of files) {
    const profile = readJson(path.join(dir, f));
    assert.deepEqual(validate(profile, 'bundle-profile.schema.json'), [], f);
    const ids = profile.models.map((m) => (typeof m === 'string' ? m : m.as));
    assert.ok(ids.includes(profile.default), `${f}: default "${profile.default}" must be one of the models`);
  }
});

test('the profile schema rejects bad profiles', () => {
  const ok = { schema: 1, name: 'x', models: ['a'], default: 'a', targets: ['macos-arm64'] };
  assert.deepEqual(validate(ok, 'bundle-profile.schema.json'), []);
  const bad = (patch) => validate({ ...ok, ...patch }, 'bundle-profile.schema.json');
  assert.ok(bad({ name: 'Not Valid' }).length > 0);
  assert.ok(bad({ models: [] }).length > 0);
  assert.ok(bad({ targets: ['macos-arm64', 'macos-arm64'] }).length > 0);
  assert.ok(bad({ targets: ['amiga'] }).length > 0);
  assert.ok(bad({ models: [{ repo: 'a/b' }] }).length > 0); // missing "as"
  assert.ok(bad({ extra: 1 }).length > 0);
  assert.ok(bad({ windows: { format: 'zip' } }).length > 0);
});

test('manifest and model pack schemas validate a model entry', () => {
  const model = { id: 'a', name: 'A', path: 'a/a.gguf', sha256: 'f'.repeat(64), bytes: 10 };
  const manifest = { schema: 1, profile: 'p', built_at: 'now', default: 'a', models: [model] };
  assert.deepEqual(validate(manifest, 'bundle-manifest.schema.json'), []);
  const pack = { schema: 1, kind: 'model-pack', name: 'p', created_at: 'now', models: [model] };
  assert.deepEqual(validate(pack, 'model-pack.schema.json'), []);
  assert.ok(validate({ ...pack, models: [{ ...model, sha256: 'xyz' }] }, 'model-pack.schema.json').length > 0);
  assert.ok(validate({ ...pack, kind: 'other' }, 'model-pack.schema.json').length > 0);
});
