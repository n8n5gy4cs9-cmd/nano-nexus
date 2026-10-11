import assert from 'node:assert/strict';
import test from 'node:test';

import { ggufEntry, parseArgs, profileFromArgs } from './bundle.mjs';

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
