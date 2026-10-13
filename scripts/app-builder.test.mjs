import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { buildArgs, buildEnv } from './app-builder.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const base = { name: 'studio', catalog: ['minicpm5-2b-q4'], default: 'minicpm5-2b-q4', target: 'windows', format: 'kit' };

test('Windows targets and formats reach bundle.mjs', () => {
  for (const target of ['mac', 'windows', 'windows-arm64', 'all']) assert.ok(buildArgs({ ...base, target }).includes(target));
  for (const format of ['auto', 'installer', 'portable', 'kit']) {
    const args = buildArgs({ ...base, format });
    assert.equal(args[args.indexOf('--format') + 1], format);
  }
  const args = buildArgs(base);
  assert.deepEqual(args.slice(args.indexOf('--target')), ['--target', 'windows', '--format', 'kit']);
  assert.ok(args.includes('--yes') && args.includes('--catalog'));
});

test('bad input is refused before anything runs', () => {
  assert.throws(() => buildArgs({ ...base, target: 'amiga' }), /Choose a target/);
  assert.throws(() => buildArgs({ ...base, name: '../evil' }), /Use letters/);
  assert.throws(() => buildArgs({ ...base, ggufs: [{ path: path.join(os.tmpdir(), 'nope.gguf') }] }), /Not a \.gguf file/);
  assert.throws(() => buildArgs({ ...base, knowledge: [path.join(os.tmpdir(), 'no-such-folder-x')] }), /Not a folder/);
  // An unknown format is dropped, not passed on.
  assert.ok(!buildArgs({ ...base, format: 'msi' }).includes('--format'));
});

test('the page offers every target and format that the server accepts', () => {
  const html = fs.readFileSync(path.join(HERE, 'app-builder', 'index.html'), 'utf8');
  for (const v of ['name="t" value="mac"', 'value="windows"', 'value="windows-arm64"', 'value="all"', '<option value="kit">', '<option value="portable">', '<option value="installer">']) {
    assert.ok(html.includes(v), v);
  }
});

test('build env appends Homebrew llvm only when installed and not already on PATH', () => {
  const llvm = '/opt/homebrew/opt/llvm/bin';
  const has = (d) => d === llvm;
  assert.equal(buildEnv({ PATH: '/usr/bin:/bin' }, has).PATH, `/usr/bin:/bin:${llvm}`);
  assert.equal(buildEnv({ PATH: `/usr/bin:${llvm}` }, has).PATH, `/usr/bin:${llvm}`);
  assert.equal(buildEnv({ PATH: '/usr/bin' }, () => false).PATH, '/usr/bin');
  assert.equal(buildEnv({ PATH: '/usr/bin' }, has).FORCE_COLOR, '0');
});
