#!/usr/bin/env node
// verify-payload.mjs: re-check a built app (or a staged/portable folder) against its manifest and runtime lock.
//
//   node scripts/verify-payload.mjs <NanoNexus.app | folder> [--manifest <staged manifest.json>]
//        [--runtime-lock <runtime.lock.json>] [--windows [--arch x64|arm64]]
//
// Checks: every bundled model's size + SHA-256 against models/manifest.json (and that no unlisted
// .gguf is present), the manifest against an expected copy (--manifest), the knowledge libraries,
// bundle.json, the runtime against its lock, and the platform details: macOS -> Info.plist keys,
// `lipo`/`otool` (arm64 only, system libraries only); Windows -> PE machine type of the exe and DLLs.
// Exit 0 = payload OK. Node 22+, no dependencies.
import { spawnSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { verifyKnowledge } from './knowledge-manifest.mjs';
import { validate } from './lib/jsonschema.mjs';

export const PLIST_KEYS = [
  'CFBundleIdentifier',
  'NSDesktopFolderUsageDescription',
  'NSDocumentsFolderUsageDescription',
  'NSDownloadsFolderUsageDescription',
  'NSRemovableVolumesUsageDescription',
  'NSNetworkVolumesUsageDescription',
];

const PE_MACHINES = { x64: 0x8664, arm64: 0xaa64 };

export function sha256File(file) {
  const h = crypto.createHash('sha256');
  const fd = fs.openSync(file, 'r');
  try {
    const buf = Buffer.alloc(8 * 1024 * 1024);
    for (let n; (n = fs.readSync(fd, buf, 0, buf.length, null)) > 0; ) h.update(buf.subarray(0, n));
  } finally {
    fs.closeSync(fd);
  }
  return h.digest('hex');
}

/** The PE machine type of a Windows binary (0x8664 x64, 0xaa64 arm64), or null if it is not a PE file. */
export function peMachine(file) {
  const fd = fs.openSync(file, 'r');
  try {
    const head = Buffer.alloc(64);
    if (fs.readSync(fd, head, 0, 64, 0) < 64 || head.toString('latin1', 0, 2) !== 'MZ') return null;
    const at = head.readUInt32LE(0x3c);
    const pe = Buffer.alloc(6);
    if (fs.readSync(fd, pe, 0, 6, at) < 6 || pe.toString('latin1', 0, 4) !== 'PE\0\0') return null;
    return pe.readUInt16LE(4);
  } finally {
    fs.closeSync(fd);
  }
}

function listModelFiles(dir) {
  const out = [];
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const full = path.join(d, e.name);
      if (e.isDirectory()) walk(full);
      else if (/\.gguf$/i.test(e.name)) out.push(path.relative(dir, full).split(path.sep).join('/'));
    }
  };
  if (fs.existsSync(dir)) walk(dir);
  return out;
}

function tool(cmd, args) {
  const r = spawnSync(cmd, args, { encoding: 'utf8' });
  return r.error ? null : r;
}

/**
 * Verify a payload. `opts`: `manifest` (expected manifest path), `runtimeLock` (path), `windows` (bool), `arch`.
 * Returns `{ problems, notes }`; empty `problems` means OK.
 */
export function verifyPayload(target, opts = {}) {
  const problems = [];
  const notes = [];
  const isApp = target.endsWith('.app');
  const res = isApp ? path.join(target, 'Contents', 'Resources') : target;
  if (!fs.existsSync(res)) return { problems: [`${target} does not exist`], notes };

  // Models.
  const mpath = path.join(res, 'models', 'manifest.json');
  if (!fs.existsSync(mpath)) {
    problems.push('models/manifest.json is missing');
  } else {
    const manifest = JSON.parse(fs.readFileSync(mpath, 'utf8'));
    for (const p of validate(manifest, 'bundle-manifest.schema.json')) problems.push(`manifest: ${p}`);
    const listed = new Set();
    for (const m of manifest.models ?? []) {
      if (typeof m.path !== 'string' || m.path.startsWith('/') || m.path.split('/').includes('..')) {
        problems.push(`${m.id}: unsafe path "${m.path}"`);
        continue;
      }
      listed.add(m.path);
      const file = path.join(res, 'models', ...m.path.split('/'));
      if (!fs.existsSync(file)) problems.push(`${m.id}: ${m.path} is missing`);
      else if (fs.statSync(file).size !== m.bytes) problems.push(`${m.id}: size ${fs.statSync(file).size} != ${m.bytes}`);
      else if (sha256File(file) !== m.sha256) problems.push(`${m.id}: SHA-256 mismatch (the file was changed)`);
      else notes.push(`model ${m.id} OK`);
    }
    for (const f of listModelFiles(path.join(res, 'models'))) if (!listed.has(f)) problems.push(`models/${f}: not in the manifest`);
    if (opts.manifest) {
      const want = JSON.parse(fs.readFileSync(opts.manifest, 'utf8'));
      const key = (x) => JSON.stringify((x.models ?? []).map((m) => [m.id, m.path, m.sha256, m.bytes]));
      if (key(want) !== key(manifest) || want.default !== manifest.default) problems.push('manifest differs from the staged one');
    }
  }

  // Knowledge libraries.
  if (fs.existsSync(path.join(res, 'knowledge'))) for (const p of verifyKnowledge(path.join(res, 'knowledge'))) problems.push(`knowledge: ${p}`);

  // Build info (Settings → About).
  const bpath = path.join(res, 'bundle.json');
  if (!fs.existsSync(bpath)) problems.push('bundle.json is missing');
  else if (!JSON.parse(fs.readFileSync(bpath, 'utf8')).profile) problems.push('bundle.json has no profile');

  // Runtime.
  const lock = opts.runtimeLock ? JSON.parse(fs.readFileSync(opts.runtimeLock, 'utf8')) : null;
  if (opts.windows) {
    const arch = opts.arch ?? 'x64';
    const want = PE_MACHINES[arch];
    if (!want) problems.push(`unknown arch ${arch}`);
    const rt = path.join(res, 'runtime');
    if (!fs.existsSync(path.join(rt, 'llama-server.exe'))) problems.push('runtime/llama-server.exe is missing');
    const binaries = [
      ...(fs.existsSync(rt) ? fs.readdirSync(rt).filter((f) => /\.(exe|dll)$/i.test(f)).map((f) => path.join(rt, f)) : []),
      ...['nano-nexus.exe'].map((f) => path.join(res, f)).filter((f) => fs.existsSync(f)),
    ];
    for (const f of binaries) {
      const m = peMachine(f);
      if (m === null) problems.push(`${path.basename(f)}: not a Windows executable`);
      else if (want && m !== want) problems.push(`${path.basename(f)}: machine 0x${m.toString(16)} is not ${arch}`);
    }
    if (binaries.length) notes.push(`${binaries.length} Windows binaries checked for ${arch}`);
    for (const f of lock?.files ?? []) {
      if (f.name.startsWith('licenses/')) continue;
      const p = path.join(rt, f.name);
      if (!fs.existsSync(p)) problems.push(`runtime/${f.name}: missing`);
      else if (sha256File(p) !== f.sha256) problems.push(`runtime/${f.name}: hash differs from the lock`);
    }
  } else if (isApp) {
    const plist = path.join(target, 'Contents', 'Info.plist');
    if (!fs.existsSync(plist)) problems.push('Contents/Info.plist is missing');
    else {
      const text = fs.readFileSync(plist, 'utf8');
      for (const k of PLIST_KEYS) if (!text.includes(`<key>${k}</key>`)) problems.push(`Info.plist: missing ${k}`);
    }
    const server = path.join(target, 'Contents', 'MacOS', 'llama-server');
    if (!fs.existsSync(server)) problems.push('Contents/MacOS/llama-server is missing');
    else {
      if (lock?.sha256 && sha256File(server) !== lock.sha256) problems.push('llama-server differs from runtime.lock.json');
      const lipo = tool('lipo', ['-archs', server]);
      if (lipo) {
        if (lipo.stdout.trim() !== 'arm64') problems.push(`llama-server architectures "${lipo.stdout.trim()}" (want arm64 only)`);
        const otool = tool('otool', ['-L', server]);
        const bad = (otool?.stdout ?? '')
          .split('\n')
          .slice(1)
          .map((l) => l.trim().split(' ')[0])
          .filter((l) => l && !l.startsWith('/usr/lib/') && !l.startsWith('/System/Library/'));
        for (const b of bad) problems.push(`llama-server links outside the system: ${b}`);
      } else notes.push('lipo/otool not available here; architecture and linkage not checked');
    }
  }
  return { problems, notes };
}

function parseArgs(argv) {
  const o = { target: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--manifest') o.manifest = argv[++i];
    else if (a === '--runtime-lock') o.runtimeLock = argv[++i];
    else if (a === '--arch') o.arch = argv[++i];
    else if (a === '--windows') o.windows = true;
    else if (a === '-h' || a === '--help') o.help = true;
    else if (!a.startsWith('--') && !o.target) o.target = a;
    else throw new Error(`Unknown option ${a}`);
  }
  return o;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const o = parseArgs(process.argv.slice(2));
    if (o.help || !o.target) {
      console.log(fs.readFileSync(fileURLToPath(import.meta.url), 'utf8').split('\n').slice(1, 12).map((l) => l.replace(/^\/\/ ?/, '')).join('\n'));
      process.exit(o.help ? 0 : 2);
    }
    const { problems, notes } = verifyPayload(path.resolve(o.target), o);
    notes.forEach((n) => console.log(`  ✔ ${n}`));
    problems.forEach((p) => console.error(`  ✖ ${p}`));
    console.log(problems.length ? `\n✖ payload check failed (${problems.length} problem${problems.length === 1 ? '' : 's'})` : '\n✔ payload OK');
    process.exit(problems.length ? 1 : 0);
  } catch (e) {
    console.error(`✖ ${e.message}`);
    process.exit(2);
  }
}
