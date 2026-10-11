// Tests for fetch-runtime-windows.mjs against a mocked GitHub API. Run: node --test scripts/
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import zlib from 'node:zlib';

import { assetName, fetchRuntime, keepFile, parsePins, readZip, repoSlug, verifyRuntime } from './fetch-runtime-windows.mjs';

const sha = (b) => crypto.createHash('sha256').update(b).digest('hex');

function crc32(buf) {
  let c, crc = 0xffffffff;
  for (let n = 0; n < buf.length; n++) {
    c = (crc ^ buf[n]) & 0xff;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    crc = (crc >>> 8) ^ c;
  }
  return (crc ^ 0xffffffff) >>> 0;
}

/** Build a zip in memory: files = [{name, data, deflate}]. */
function makeZip(files) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const f of files) {
    const name = Buffer.from(f.name);
    const raw = Buffer.from(f.data);
    const body = f.deflate ? zlib.deflateRawSync(raw) : raw;
    const lh = Buffer.alloc(30);
    lh.writeUInt32LE(0x04034b50, 0); lh.writeUInt16LE(20, 4); lh.writeUInt16LE(f.deflate ? 8 : 0, 8);
    lh.writeUInt32LE(crc32(raw), 14); lh.writeUInt32LE(body.length, 18); lh.writeUInt32LE(raw.length, 22);
    lh.writeUInt16LE(name.length, 26);
    locals.push(lh, name, body);
    const ch = Buffer.alloc(46);
    ch.writeUInt32LE(0x02014b50, 0); ch.writeUInt16LE(20, 4); ch.writeUInt16LE(20, 6); ch.writeUInt16LE(f.deflate ? 8 : 0, 10);
    ch.writeUInt32LE(crc32(raw), 16); ch.writeUInt32LE(body.length, 20); ch.writeUInt32LE(raw.length, 24);
    ch.writeUInt16LE(name.length, 28); ch.writeUInt32LE(offset, 42);
    centrals.push(ch, name);
    offset += 30 + name.length + body.length;
  }
  const cd = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(files.length, 8); end.writeUInt16LE(files.length, 10);
  end.writeUInt32LE(cd.length, 12); end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, cd, end]);
}

const PINS = 'LLAMA_CPP_REPO=https://github.com/ggml-org/llama.cpp.git\nLLAMA_CPP_TAG=b11552\n';
const FILES = [
  { name: 'llama-server.exe', data: 'MZ-server', deflate: true },
  { name: 'ggml-vulkan.dll', data: 'dll-bytes', deflate: false },
  { name: 'llama-cli.exe', data: 'cli', deflate: true },
  { name: 'LICENSE', data: 'MIT', deflate: true },
  { name: 'README.txt', data: 'readme', deflate: true },
];

function tmpRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nn-frw-'));
  fs.mkdirSync(path.join(root, 'scripts'));
  fs.writeFileSync(path.join(root, 'scripts', 'runtime.pins'), PINS);
  return root;
}

function mockFetch({ zip, digest, assets, size } = {}) {
  const calls = [];
  const zipBuf = zip ?? makeZip(FILES);
  const name = 'llama-b11552-bin-win-vulkan-x64.zip';
  const fn = async (url) => {
    calls.push(String(url));
    if (String(url).includes('/releases/tags/b11552')) {
      return {
        ok: true,
        json: async () => ({
          assets: assets ?? [
            { name, size: size ?? zipBuf.length, digest: digest === undefined ? `sha256:${sha(zipBuf)}` : digest, browser_download_url: 'https://example.test/asset.zip' },
          ],
        }),
      };
    }
    if (String(url) === 'https://example.test/asset.zip') return { ok: true, arrayBuffer: async () => zipBuf.buffer.slice(zipBuf.byteOffset, zipBuf.byteOffset + zipBuf.length) };
    return { ok: false, status: 404 };
  };
  fn.calls = calls;
  return fn;
}

test('pins, slug and asset names', () => {
  assert.equal(parsePins(PINS).LLAMA_CPP_TAG, 'b11552');
  assert.equal(repoSlug('https://github.com/ggml-org/llama.cpp.git'), 'ggml-org/llama.cpp');
  assert.equal(assetName('b1', 'x64', false), 'llama-b1-bin-win-vulkan-x64.zip');
  assert.equal(assetName('b1', 'x64', true), 'llama-b1-bin-win-cpu-x64.zip');
  assert.equal(assetName('b1', 'arm64', false), 'llama-b1-bin-win-cpu-arm64.zip');
  assert.throws(() => assetName('b1', 'x86', false));
});

test('prune keeps the server, DLLs and licenses only', () => {
  assert.ok(keepFile('llama-server.exe') && keepFile('build/bin/ggml.dll') && keepFile('LICENSE-curl'));
  assert.ok(!keepFile('llama-cli.exe') && !keepFile('README.txt'));
});

test('zip reader handles stored and deflated entries', () => {
  const { entries, read } = readZip(makeZip(FILES));
  assert.equal(entries.length, 5);
  assert.equal(read(entries[0]).toString(), 'MZ-server');
  assert.equal(read(entries[1]).toString(), 'dll-bytes');
});

test('downloads, verifies, extracts, prunes and locks', async () => {
  const root = tmpRoot();
  const lock = await fetchRuntime({ root, fetchImpl: mockFetch() });
  const dir = path.join(root, 'vendor', 'runtime', 'windows-x64');
  assert.deepEqual(fs.readdirSync(dir).sort(), ['ggml-vulkan.dll', 'licenses', 'llama-server.exe', 'runtime.lock.json']);
  assert.equal(fs.readFileSync(path.join(dir, 'llama-server.exe'), 'utf8'), 'MZ-server');
  assert.equal(fs.readFileSync(path.join(dir, 'licenses', 'LICENSE'), 'utf8'), 'MIT');
  assert.equal(lock.tag, 'b11552');
  assert.deepEqual(lock.files.map((f) => f.name), ['ggml-vulkan.dll', 'licenses/LICENSE', 'llama-server.exe']);
  assert.deepEqual(await verifyRuntime({ root }), []);
});

test('a second run is a no-op when the files verify', async () => {
  const root = tmpRoot();
  await fetchRuntime({ root, fetchImpl: mockFetch() });
  const again = mockFetch();
  await fetchRuntime({ root, fetchImpl: again });
  assert.equal(again.calls.length, 0);
});

test('digest mismatch is refused and nothing is installed', async () => {
  const root = tmpRoot();
  await assert.rejects(fetchRuntime({ root, fetchImpl: mockFetch({ digest: `sha256:${'0'.repeat(64)}` }) }), /SHA-256 mismatch/);
  assert.ok(!fs.existsSync(path.join(root, 'vendor', 'runtime', 'windows-x64')));
});

test('a missing digest is refused', async () => {
  await assert.rejects(fetchRuntime({ root: tmpRoot(), fetchImpl: mockFetch({ digest: null }) }), /no sha256 digest/);
});

test('a missing asset or size mismatch is refused', async () => {
  await assert.rejects(fetchRuntime({ root: tmpRoot(), fetchImpl: mockFetch({ assets: [] }) }), /no asset named/);
  await assert.rejects(fetchRuntime({ root: tmpRoot(), fetchImpl: mockFetch({ size: 5 }) }), /Size mismatch/);
});

test('a zip without llama-server.exe is refused', async () => {
  const zip = makeZip([{ name: 'a.dll', data: 'x', deflate: false }]);
  await assert.rejects(fetchRuntime({ root: tmpRoot(), fetchImpl: mockFetch({ zip }) }), /does not contain llama-server.exe/);
});

test('verify detects tampering', async () => {
  const root = tmpRoot();
  await fetchRuntime({ root, fetchImpl: mockFetch() });
  fs.writeFileSync(path.join(root, 'vendor', 'runtime', 'windows-x64', 'llama-server.exe'), 'tampered');
  const problems = await verifyRuntime({ root });
  assert.deepEqual(problems, ['llama-server.exe: hash mismatch']);
});
