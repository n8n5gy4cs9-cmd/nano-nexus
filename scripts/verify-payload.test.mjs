import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';

import { fakeRoot } from './lib/fake-bundle.mjs';
import { peMachine, verifyPayload } from './verify-payload.mjs';
import { locksFor, stage } from './stage-bundle.mjs';

async function staged(ids) {
  const { root, modelsDir, profile } = fakeRoot(ids);
  const st = await stage(profile, 'mac', locksFor(profile, modelsDir), { root, modelsDir });
  return { root, st, dir: st.dir };
}

test('a freshly staged payload verifies', async () => {
  const { dir, st } = await staged(['a', 'b']);
  const r = verifyPayload(dir, { manifest: path.join(dir, 'models', 'manifest.json') });
  assert.deepEqual(r.problems, []);
  assert.equal(r.notes.filter((n) => n.startsWith('model ')).length, 2);
  assert.ok(st.dir);
});

test('flipping one byte of a model fails the check (tamper test)', async () => {
  const { dir } = await staged(['a']);
  const file = path.join(dir, 'models', 'a', 'a-Q8_0.gguf');
  // Hard links share the inode with vendor/models: copy first so only the payload changes.
  const bytes = fs.readFileSync(file);
  fs.rmSync(file);
  bytes[5000] ^= 0xff;
  fs.writeFileSync(file, bytes);
  const { problems } = verifyPayload(dir);
  assert.equal(problems.length, 1);
  assert.match(problems[0], /a: SHA-256 mismatch/);
});

test('missing, truncated, unlisted and unsafe entries are reported', async () => {
  const { dir } = await staged(['a', 'b']);
  fs.rmSync(path.join(dir, 'models', 'a', 'a-Q8_0.gguf'));
  const b = path.join(dir, 'models', 'b', 'b-Q8_0.gguf');
  const data = fs.readFileSync(b);
  fs.rmSync(b);
  fs.writeFileSync(b, data.subarray(0, 100));
  fs.writeFileSync(path.join(dir, 'models', 'stray.gguf'), 'x');
  fs.rmSync(path.join(dir, 'bundle.json'));
  const text = verifyPayload(dir).problems.join('\n');
  assert.match(text, /a: .*missing/);
  assert.match(text, /b: size 100/);
  assert.match(text, /stray\.gguf: not in the manifest/);
  assert.match(text, /bundle\.json is missing/);
});

test('a manifest that differs from the staged one is caught', async () => {
  const { dir } = await staged(['a']);
  const expected = path.join(dir, '..', 'expected.json');
  const m = JSON.parse(fs.readFileSync(path.join(dir, 'models', 'manifest.json'), 'utf8'));
  m.models[0].sha256 = '0'.repeat(64);
  fs.writeFileSync(expected, JSON.stringify(m));
  assert.match(verifyPayload(dir, { manifest: expected }).problems.join('\n'), /differs from the staged one/);
});

function fakePe(machine) {
  const b = Buffer.alloc(0x100);
  b.write('MZ');
  b.writeUInt32LE(0x80, 0x3c);
  b.write('PE\0\0', 0x80, 'latin1');
  b.writeUInt16LE(machine, 0x84);
  return b;
}

test('Windows payloads: PE architecture and the runtime lock', async () => {
  const { dir } = await staged(['a']);
  const rt = path.join(dir, 'runtime');
  fs.mkdirSync(rt);
  fs.writeFileSync(path.join(rt, 'llama-server.exe'), fakePe(0x8664));
  fs.writeFileSync(path.join(rt, 'ggml.dll'), fakePe(0x8664));
  assert.equal(peMachine(path.join(rt, 'ggml.dll')), 0x8664);
  assert.deepEqual(verifyPayload(dir, { windows: true, arch: 'x64' }).problems, []);
  assert.match(verifyPayload(dir, { windows: true, arch: 'arm64' }).problems.join('\n'), /llama-server\.exe: machine 0x8664 is not arm64/);
  fs.writeFileSync(path.join(rt, 'ggml.dll'), 'not a pe');
  assert.match(verifyPayload(dir, { windows: true }).problems.join('\n'), /ggml\.dll: not a Windows executable/);

  const lock = path.join(dir, '..', 'rt.lock.json');
  fs.writeFileSync(lock, JSON.stringify({ files: [{ name: 'llama-server.exe', sha256: '1'.repeat(64) }, { name: 'gone.dll', sha256: '2'.repeat(64) }] }));
  const text = verifyPayload(dir, { windows: true, runtimeLock: lock }).problems.join('\n');
  assert.match(text, /llama-server\.exe: hash differs/);
  assert.match(text, /gone\.dll: missing/);
});

test('a macOS app needs the Info.plist usage strings and the sidecar', async () => {
  const { dir, root } = await staged(['a']);
  const app = path.join(root, 'NanoNexus.app');
  fs.mkdirSync(path.join(app, 'Contents', 'MacOS'), { recursive: true });
  fs.cpSync(dir, path.join(app, 'Contents', 'Resources'), { recursive: true });
  let text = verifyPayload(app).problems.join('\n');
  assert.match(text, /Info\.plist is missing/);
  assert.match(text, /llama-server is missing/);
  const plist = `<dict>${['CFBundleIdentifier', 'NSDesktopFolderUsageDescription', 'NSDocumentsFolderUsageDescription', 'NSDownloadsFolderUsageDescription', 'NSRemovableVolumesUsageDescription'].map((k) => `<key>${k}</key><string>x</string>`).join('')}</dict>`;
  fs.writeFileSync(path.join(app, 'Contents', 'Info.plist'), plist);
  fs.writeFileSync(path.join(app, 'Contents', 'MacOS', 'llama-server'), 'bin');
  text = verifyPayload(app).problems.join('\n');
  assert.match(text, /missing NSNetworkVolumesUsageDescription/);
  assert.doesNotMatch(text, /Info\.plist is missing|llama-server is missing/);
});
