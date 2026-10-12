import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { fakeRoot } from './lib/fake-bundle.mjs';
import { validate } from './lib/jsonschema.mjs';
import { makePack, verifyPack } from './make-model-pack.mjs';

test('a pack holds pack.json, README and verified copies', () => {
  const { modelsDir } = fakeRoot(['alpha', 'beta']);
  const out = fs.mkdtempSync(path.join(os.tmpdir(), 'pack-'));
  const seen = [];
  const { dir, pack } = makePack({ ids: ['alpha', 'beta'], out, name: 'Studio Pack', modelsDir, now: '2026-01-01T00:00:00Z', onProgress: (id) => seen.push(id) });
  assert.equal(path.basename(dir), 'NanoNexus-ModelPack-studio-pack');
  assert.deepEqual(seen, ['alpha', 'beta']);
  assert.deepEqual(validate(JSON.parse(fs.readFileSync(path.join(dir, 'pack.json'), 'utf8')), 'model-pack.schema.json'), []);
  assert.equal(pack.models[0].path, 'alpha/alpha-Q8_0.gguf');
  assert.ok(fs.existsSync(path.join(dir, 'models', 'beta', 'beta-Q8_0.gguf')));
  assert.match(fs.readFileSync(path.join(dir, 'README.txt'), 'utf8'), /Import model pack/);
  assert.deepEqual(verifyPack(dir), []);
});

test('verify catches a changed byte; missing models and reused names are errors', () => {
  const { modelsDir } = fakeRoot(['alpha']);
  const out = fs.mkdtempSync(path.join(os.tmpdir(), 'pack-'));
  const { dir } = makePack({ ids: ['alpha'], out, modelsDir });
  const file = path.join(dir, 'models', 'alpha', 'alpha-Q8_0.gguf');
  const data = fs.readFileSync(file);
  data[100] ^= 1;
  fs.writeFileSync(file, data);
  assert.match(verifyPack(dir).join('\n'), /alpha: hash mismatch/);
  assert.throws(() => makePack({ ids: ['alpha'], out, modelsDir }), /already holds a pack/);
  assert.throws(() => makePack({ ids: ['ghost'], out, modelsDir }), /ghost.*fetch-model/);
  assert.throws(() => makePack({ ids: [], out, modelsDir }), /no models/);
});

test('the CLI explains itself', () => {
  const script = path.join(path.dirname(fileURLToPath(import.meta.url)), 'make-model-pack.mjs');
  const r = spawnSync('node', [script, '--help'], { encoding: 'utf8' });
  assert.equal(r.status, 0);
  assert.match(r.stdout, /--profile studio --out/);
  assert.equal(spawnSync('node', [script], { encoding: 'utf8' }).status, 2);
});
