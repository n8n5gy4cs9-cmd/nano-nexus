import assert from 'node:assert/strict';
import test from 'node:test';

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { ggufEntry, parseArgs, profileFromArgs, removeBuildCopy } from './bundle.mjs';

test('gguf spec parsing', () => {
  assert.deepEqual(ggufEntry('/m/My Model.Q4.gguf'), { local: '/m/My Model.Q4.gguf', as: 'my-model.q4', name: 'My Model.Q4' });
  assert.deepEqual(ggufEntry('/m/x.gguf=fast=Fast model'), { local: '/m/x.gguf', as: 'fast', name: 'Fast model' });
});

test('profile from options', () => {
  const o = parseArgs(['--gguf', '/m/a.gguf', '--catalog', 'minicpm5-2b-q4', '--knowledge', '/b/books', '--target', 'all', '--name', 'Studio One']);
  const p = profileFromArgs(o);
  assert.equal(p.name, 'studio-one');
  assert.equal(p.models.length, 2);
  assert.equal(p.default, 'a');
  assert.deepEqual(p.targets, ['macos-arm64', 'windows-x64']);
  assert.equal(p.knowledge.length, 1);
});

test('removeBuildCopy only deletes inside target/ and vendor/bundle/, never models', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nn-rm-'));
  const make = (...p) => { const f = path.join(root, ...p); fs.mkdirSync(path.dirname(f), { recursive: true }); fs.writeFileSync(f, 'x'); return f; };
  const app = make('target', 'release', 'bundle', 'macos', 'NanoNexus.app', 'big.gguf');
  const staged = make('vendor', 'bundle', 'p', 'macos-arm64', 'models', 'm.gguf');
  const model = make('vendor', 'models', 'm1', 'm.gguf');
  removeBuildCopy(path.join(root, 'target', 'release', 'bundle', 'macos', 'NanoNexus.app'), root);
  removeBuildCopy(path.join(root, 'vendor', 'bundle', 'p'), root);
  assert.ok(!fs.existsSync(app) && !fs.existsSync(staged));
  assert.ok(fs.existsSync(model), 'models must survive');
  for (const bad of [path.join(root, 'vendor', 'models'), path.join(root, 'vendor', 'models', 'm1'), path.join(root, 'target'), path.join(root, 'vendor', 'bundle'), root, os.homedir()])
    assert.throws(() => removeBuildCopy(bad, root), /refusing/);
  assert.ok(fs.existsSync(model));
  fs.rmSync(root, { recursive: true, force: true });
});

test('--keep-staging is accepted', () => {
  assert.equal(parseArgs(['--catalog', 'x', '--keep-staging']).keepStaging, true);
});
