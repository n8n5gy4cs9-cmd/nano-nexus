import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';

import { MB10, fakeRoot } from './lib/fake-bundle.mjs';
import { validate } from './lib/jsonschema.mjs';
import { locksFor, stage, targetKey } from './stage-bundle.mjs';

test('stages hard-linked models, a valid manifest and the Tauri overlay', async () => {
  const { root, modelsDir, profile } = fakeRoot();
  const locks = locksFor(profile, modelsDir);
  const st = await stage(profile, 'mac', locks, { root, modelsDir, now: '2026-01-02T03:04:05Z' });
  assert.equal(st.bytes, 2 * MB10);

  const manifest = JSON.parse(fs.readFileSync(path.join(st.dir, 'models', 'manifest.json'), 'utf8'));
  assert.deepEqual(validate(manifest, 'bundle-manifest.schema.json'), []);
  assert.equal(manifest.default, 'alpha');
  assert.deepEqual(manifest.models.map((m) => m.path), ['alpha/alpha-Q8_0.gguf', 'beta/beta-Q8_0.gguf']);

  // Hard links: no second copy on disk.
  const src = fs.statSync(path.join(modelsDir, 'alpha', 'alpha-Q8_0.gguf'));
  const dst = fs.statSync(path.join(st.dir, 'models', 'alpha', 'alpha-Q8_0.gguf'));
  assert.equal(src.ino, dst.ino);

  const overlay = JSON.parse(fs.readFileSync(st.overlay, 'utf8'));
  assert.equal(path.basename(st.overlay), 'tauri.bundle.fake.macos-arm64.json');
  assert.deepEqual(overlay.bundle.resources, {
    '../../../vendor/bundle/fake/macos-arm64/models/': 'models/',
    '../../../vendor/bundle/fake/macos-arm64/licenses/': 'licenses/',
    '../../../vendor/bundle/fake/macos-arm64/bundle.json': 'bundle.json',
  });

  const info = JSON.parse(fs.readFileSync(path.join(st.dir, 'bundle.json'), 'utf8'));
  assert.equal(info.profile, 'fake');
  assert.equal(info.built_at, '2026-01-02T03:04:05Z');
  assert.deepEqual(info.models, ['alpha', 'beta']);
  assert.match(fs.readFileSync(path.join(st.dir, 'licenses', 'models.txt'), 'utf8'), /alpha\tMIT/);
  assert.ok(fs.existsSync(path.join(st.dir, 'licenses', 'LICENSE-llama')));
});

test('a Windows target carries the runtime (without its lock file)', async () => {
  const { root, modelsDir, profile } = fakeRoot(['solo']);
  const rt = path.join(root, 'vendor', 'runtime', 'windows-x64');
  fs.mkdirSync(rt, { recursive: true });
  for (const f of ['llama-server.exe', 'ggml.dll', 'runtime.lock.json']) fs.writeFileSync(path.join(rt, f), f);
  const st = await stage(profile, 'windows', locksFor(profile, modelsDir), { root, modelsDir });
  assert.deepEqual(fs.readdirSync(path.join(st.dir, 'runtime')).sort(), ['ggml.dll', 'llama-server.exe']);
  assert.equal(JSON.parse(fs.readFileSync(st.overlay, 'utf8')).bundle.resources['../../../vendor/bundle/fake/windows-x64/runtime/'], 'runtime/');
});

test('missing models and a default outside the bundle are errors', async () => {
  const { root, modelsDir, profile } = fakeRoot(['one']);
  assert.throws(() => locksFor({ ...profile, models: ['one', 'ghost'] }, modelsDir), /ghost.*fetch-model/);
  await assert.rejects(stage({ ...profile, default: 'nope' }, 'mac', locksFor(profile, modelsDir), { root, modelsDir }), /default model "nope"/);
});

test('target names', () => {
  assert.equal(targetKey('macos-arm64'), 'mac');
  assert.equal(targetKey('windows-x64'), 'windows');
  assert.equal(targetKey('windows-arm64'), 'windows-arm64');
  assert.equal(targetKey('linux'), null);
});
