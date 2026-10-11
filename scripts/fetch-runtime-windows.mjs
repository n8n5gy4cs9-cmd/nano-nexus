#!/usr/bin/env node
// fetch-runtime-windows.mjs: download the official llama.cpp Windows release for the pinned tag,
// verify its SHA-256 against the GitHub release-asset digest, extract it, prune it, and lock it.
// (Node 22+, no dependencies; BUNDLING.md §3.)
//
// Usage:
//   node scripts/fetch-runtime-windows.mjs [--arch x64|arm64] [--cpu-only] [--force]
//   node scripts/fetch-runtime-windows.mjs --verify [--arch x64|arm64]
// Output: vendor/runtime/windows-<arch>/{llama-server.exe, *.dll, licenses/, runtime.lock.json}
// Env: GITHUB_TOKEN (rate limits), GITHUB_API (mirror / tests), NODE_USE_ENV_PROXY=1 (behind a proxy)

import crypto from 'node:crypto';
import fs from 'node:fs';
import fsp from 'node:fs/promises';
import path from 'node:path';
import zlib from 'node:zlib';
import { fileURLToPath, pathToFileURL } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DEFAULT_ROOT = path.resolve(HERE, '..');
const GITHUB_API = (process.env.GITHUB_API || 'https://api.github.com').replace(/\/+$/, '');

export class UserError extends Error {}

/** Parse `KEY=value` lines of scripts/runtime.pins. */
export function parsePins(text) {
  const pins = {};
  for (const line of text.split('\n')) {
    const m = /^\s*([A-Z0-9_]+)=(.*?)\s*$/.exec(line);
    if (m && !line.trim().startsWith('#')) pins[m[1]] = m[2];
  }
  return pins;
}

/** `https://github.com/owner/name.git` -> `owner/name`. */
export function repoSlug(url) {
  const m = /github\.com[/:]([^/]+\/[^/]+?)(?:\.git)?\/?$/.exec(url);
  if (!m) throw new UserError(`Cannot read the GitHub repo from LLAMA_CPP_REPO=${url}`);
  return m[1];
}

/** Which release asset to use. */
export function assetName(tag, arch, cpuOnly) {
  if (arch === 'arm64') return `llama-${tag}-bin-win-cpu-arm64.zip`;
  if (arch === 'x64') return `llama-${tag}-bin-win-${cpuOnly ? 'cpu' : 'vulkan'}-x64.zip`;
  throw new UserError(`Unsupported --arch ${arch} (use x64 or arm64)`);
}

/** Keep llama-server.exe, every DLL, and license files; drop everything else. */
export function keepFile(name) {
  const base = name.split('/').pop() ?? name;
  const lower = base.toLowerCase();
  return lower === 'llama-server.exe' || lower.endsWith('.dll') || /^licen[sc]e/i.test(base);
}

// ---------- minimal zip reader (stored + deflate; no zip64, no encryption) ----------------------

/** List and read entries of a zip held in memory. */
export function readZip(buf) {
  let eocd = -1;
  for (let i = buf.length - 22; i >= Math.max(0, buf.length - 65557); i--) {
    if (buf.readUInt32LE(i) === 0x06054b50) { eocd = i; break; }
  }
  if (eocd < 0) throw new UserError('Not a zip file (no end-of-central-directory record)');
  const count = buf.readUInt16LE(eocd + 10);
  let p = buf.readUInt32LE(eocd + 16);
  const entries = [];
  for (let n = 0; n < count; n++) {
    if (buf.readUInt32LE(p) !== 0x02014b50) throw new UserError('Corrupt zip central directory');
    const method = buf.readUInt16LE(p + 10);
    const compSize = buf.readUInt32LE(p + 20);
    const size = buf.readUInt32LE(p + 24);
    const nameLen = buf.readUInt16LE(p + 28);
    const extraLen = buf.readUInt16LE(p + 30);
    const commentLen = buf.readUInt16LE(p + 32);
    const localOffset = buf.readUInt32LE(p + 42);
    const name = buf.toString('utf8', p + 46, p + 46 + nameLen);
    entries.push({ name, method, compSize, size, localOffset });
    p += 46 + nameLen + extraLen + commentLen;
  }
  const read = (e) => {
    const o = e.localOffset;
    if (buf.readUInt32LE(o) !== 0x04034b50) throw new UserError(`Corrupt zip entry ${e.name}`);
    const start = o + 30 + buf.readUInt16LE(o + 26) + buf.readUInt16LE(o + 28);
    const data = buf.subarray(start, start + e.compSize);
    if (e.method === 0) return Buffer.from(data);
    if (e.method === 8) return zlib.inflateRawSync(data);
    throw new UserError(`Unsupported zip compression method ${e.method} for ${e.name}`);
  };
  return { entries: entries.filter((e) => !e.name.endsWith('/')), read };
}

// ---------- helpers -------------------------------------------------------------------------------

const sha256 = (buf) => crypto.createHash('sha256').update(buf).digest('hex');

async function getJson(fetchImpl, url, token) {
  const headers = { 'User-Agent': 'NanoNexus-fetch-runtime-windows/1.0', Accept: 'application/vnd.github+json' };
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await fetchImpl(url, { headers });
  if (!res.ok) throw new UserError(`GET ${url} failed: HTTP ${res.status}`);
  return res.json();
}

async function download(fetchImpl, url, expectedSize) {
  const res = await fetchImpl(url, { headers: { 'User-Agent': 'NanoNexus-fetch-runtime-windows/1.0' }, redirect: 'follow' });
  if (!res.ok) throw new UserError(`Download failed: HTTP ${res.status} for ${url}`);
  const buf = Buffer.from(await res.arrayBuffer());
  if (expectedSize && buf.length !== expectedSize) {
    throw new UserError(`Size mismatch: expected ${expectedSize} bytes, got ${buf.length}`);
  }
  return buf;
}

// ---------- main steps ------------------------------------------------------------------------------

/** Fetch, verify, extract, prune, and lock. Returns the lock object. */
export async function fetchRuntime({ root = DEFAULT_ROOT, arch = 'x64', cpuOnly = false, force = false, fetchImpl = fetch, log = () => {} } = {}) {
  const pins = parsePins(fs.readFileSync(path.join(root, 'scripts', 'runtime.pins'), 'utf8'));
  const tag = pins.LLAMA_CPP_TAG;
  if (!tag) throw new UserError('LLAMA_CPP_TAG missing from scripts/runtime.pins');
  const slug = repoSlug(pins.LLAMA_CPP_REPO ?? '');
  const name = assetName(tag, arch, cpuOnly);
  const outDir = path.join(root, 'vendor', 'runtime', `windows-${arch}`);
  const lockPath = path.join(outDir, 'runtime.lock.json');

  if (!force && fs.existsSync(lockPath)) {
    const lock = JSON.parse(fs.readFileSync(lockPath, 'utf8'));
    if (lock.tag === tag && lock.asset === name) {
      const problems = await verifyRuntime({ root, arch });
      if (problems.length === 0) {
        log(`windows-${arch} runtime ${tag} already present and verified`);
        return lock;
      }
    }
  }

  log(`Resolving ${slug}@${tag} …`);
  const release = await getJson(fetchImpl, `${GITHUB_API}/repos/${slug}/releases/tags/${tag}`, process.env.GITHUB_TOKEN);
  const asset = (release.assets ?? []).find((a) => a.name === name);
  if (!asset) throw new UserError(`Release ${tag} has no asset named ${name}`);
  const m = /^sha256:([0-9a-f]{64})$/i.exec(asset.digest ?? '');
  if (!m) throw new UserError(`Asset ${name} has no sha256 digest in the GitHub API; refusing to use an unverified download`);
  const expected = m[1].toLowerCase();

  log(`Downloading ${name} (${asset.size} bytes) …`);
  const zip = await download(fetchImpl, asset.browser_download_url, asset.size);
  const actual = sha256(zip);
  if (actual !== expected) throw new UserError(`SHA-256 mismatch for ${name}: expected ${expected}, got ${actual}`);

  const { entries, read } = readZip(zip);
  const kept = entries.filter((e) => keepFile(e.name));
  if (!kept.some((e) => e.name.split('/').pop()?.toLowerCase() === 'llama-server.exe')) {
    throw new UserError(`${name} does not contain llama-server.exe`);
  }

  const tmp = `${outDir}.tmp-${process.pid}`;
  await fsp.rm(tmp, { recursive: true, force: true });
  await fsp.mkdir(path.join(tmp, 'licenses'), { recursive: true });
  const files = [];
  for (const e of kept) {
    const base = e.name.split('/').pop();
    const data = read(e);
    const isLicense = /^licen[sc]e/i.test(base);
    const rel = isLicense ? `licenses/${base}` : base;
    await fsp.writeFile(path.join(tmp, rel), data);
    files.push({ name: rel, bytes: data.length, sha256: sha256(data) });
  }
  files.sort((a, b) => a.name.localeCompare(b.name));
  const lock = {
    schema: 1,
    tag,
    asset: name,
    arch,
    url: asset.browser_download_url,
    zip_sha256: expected,
    zip_bytes: zip.length,
    fetched_at: new Date().toISOString(),
    files,
  };
  await fsp.writeFile(path.join(tmp, 'runtime.lock.json'), `${JSON.stringify(lock, null, 2)}\n`);
  await fsp.rm(outDir, { recursive: true, force: true });
  await fsp.mkdir(path.dirname(outDir), { recursive: true });
  await fsp.rename(tmp, outDir);
  log(`Installed ${files.length} files into ${path.relative(root, outDir)}`);
  return lock;
}

/** Re-hash the installed files against runtime.lock.json. Returns a list of problems (empty = ok). */
export async function verifyRuntime({ root = DEFAULT_ROOT, arch = 'x64' } = {}) {
  const dir = path.join(root, 'vendor', 'runtime', `windows-${arch}`);
  let lock;
  try {
    lock = JSON.parse(await fsp.readFile(path.join(dir, 'runtime.lock.json'), 'utf8'));
  } catch {
    return [`no runtime.lock.json in ${path.relative(root, dir)}`];
  }
  const problems = [];
  for (const f of lock.files ?? []) {
    try {
      const data = await fsp.readFile(path.join(dir, f.name));
      if (sha256(data) !== f.sha256) problems.push(`${f.name}: hash mismatch`);
    } catch {
      problems.push(`${f.name}: missing`);
    }
  }
  if (!(lock.files ?? []).some((f) => f.name === 'llama-server.exe')) problems.push('llama-server.exe is not in the lock');
  return problems;
}

function parseArgs(argv) {
  const o = { arch: 'x64', cpuOnly: false, force: false, verify: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--arch') o.arch = argv[++i];
    else if (a === '--cpu-only') o.cpuOnly = true;
    else if (a === '--force') o.force = true;
    else if (a === '--verify') o.verify = true;
    else if (a === '-h' || a === '--help') o.help = true;
    else throw new UserError(`Unknown argument: ${a}`);
  }
  return o;
}

async function main(argv) {
  const o = parseArgs(argv);
  if (o.help) {
    console.log('Usage: node scripts/fetch-runtime-windows.mjs [--arch x64|arm64] [--cpu-only] [--force] | --verify');
    return;
  }
  const log = (m) => console.error(m);
  if (o.verify) {
    const problems = await verifyRuntime({ arch: o.arch });
    if (problems.length) throw new UserError(`Verification failed:\n  - ${problems.join('\n  - ')}`);
    log(`windows-${o.arch} runtime verified`);
    return;
  }
  await fetchRuntime({ arch: o.arch, cpuOnly: o.cpuOnly, force: o.force, log });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2)).catch((e) => {
    console.error(e instanceof UserError ? `error: ${e.message}` : e);
    process.exit(1);
  });
}
