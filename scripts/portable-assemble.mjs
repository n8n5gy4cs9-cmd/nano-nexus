#!/usr/bin/env node
// Non-destructive DIST assembler (UP.050). Source only: not run, no builds, no downloads.
//
//   node scripts/portable-assemble.mjs --spec dist-spec.json --out <new DIST folder>
//   node scripts/portable-assemble.mjs --spec dist-spec.json --out <existing DIST> --update
//
// The spec lists prebuilt inputs; nothing is compiled or fetched. Layout: docs/portable/STORAGE.md.
// Rules: never overwrite or delete user data (models/, data/, outputs/, cache/, logs/); a fresh run refuses a
// non-empty --out; --update only replaces versioned app/runtime/licence entries listed in the spec and
// keeps the previous version under .rollback/<timestamp>/ inside the DIST. Symlinks and special files are
// refused (exFAT has none). Every copied file gets a SHA-256 in manifest.json.

import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

const MARKER_TEXT = 'NANONEXUS_PORTABLE_DIST_REQUIRED_V1';
const USER_DIRS = ['models', 'data', 'outputs', 'cache', 'logs'];
const PLATFORMS = ['macos-arm64', 'windows-x64'];
const COMPONENT_ROOTS = ['apps', 'runtimes', 'licenses'];

function fail(msg) {
  console.error(`portable-assemble: ${msg}`);
  process.exit(1);
}

function args(argv) {
  const out = { update: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--update') out.update = true;
    else if (a === '--spec' || a === '--out') out[a.slice(2)] = argv[++i];
    else fail(`unknown argument ${a}`);
  }
  if (!out.spec || !out.out) fail('usage: --spec <file> --out <dir> [--update]');
  return out;
}

/** A destination must be a relative path inside apps/, runtimes/ or licenses/ with no `..`. */
export function checkDest(dest) {
  if (typeof dest !== 'string' || !dest || path.isAbsolute(dest) || dest.includes('\\')) {
    throw new Error(`bad destination ${JSON.stringify(dest)}`);
  }
  const parts = dest.split('/');
  if (parts.some((p) => p === '' || p === '.' || p === '..')) throw new Error(`bad destination ${dest}`);
  if (!COMPONENT_ROOTS.includes(parts[0]) || parts.length < 2) {
    throw new Error(`destination ${dest} must be inside ${COMPONENT_ROOTS.join(', ')}`);
  }
  if (parts[0] === 'apps' && !PLATFORMS.includes(parts[1])) {
    throw new Error(`unsupported platform folder ${parts[1]}`);
  }
  return dest;
}

function sha256(file) {
  const h = crypto.createHash('sha256');
  const fd = fs.openSync(file, 'r');
  try {
    const buf = Buffer.alloc(1 << 20);
    for (let n; (n = fs.readSync(fd, buf, 0, buf.length, null)) > 0; ) h.update(buf.subarray(0, n));
  } finally {
    fs.closeSync(fd);
  }
  return h.digest('hex');
}

function walk(src) {
  const st = fs.lstatSync(src);
  if (st.isSymbolicLink()) throw new Error(`symlink refused: ${src}`);
  if (st.isFile()) return [''];
  if (!st.isDirectory()) throw new Error(`special file refused: ${src}`);
  const rels = [];
  const visit = (dir, prefix) => {
    for (const name of fs.readdirSync(dir).sort()) {
      const p = path.join(dir, name);
      const s = fs.lstatSync(p);
      if (s.isSymbolicLink()) throw new Error(`symlink refused: ${p}`);
      if (s.isDirectory()) visit(p, `${prefix}${name}/`);
      else if (s.isFile()) rels.push(`${prefix}${name}`);
      else throw new Error(`special file refused: ${p}`);
    }
  };
  visit(src, '');
  return rels;
}

function containsMarker(file) {
  const needle = Buffer.from(MARKER_TEXT);
  return fs.readFileSync(file).includes(needle);
}

/** Validate the whole spec before anything is written. */
export function validateSpec(spec, baseDir) {
  if (!spec || spec.schema !== 1 || !spec.version || !Array.isArray(spec.components)) {
    throw new Error('spec must be {"schema":1,"version":"...","components":[...]}');
  }
  const seen = new Set();
  for (const c of spec.components) {
    checkDest(c.dest);
    if (seen.has(c.dest)) throw new Error(`duplicate destination ${c.dest}`);
    seen.add(c.dest);
    const src = path.resolve(baseDir, c.source ?? '');
    if (!fs.existsSync(src)) throw new Error(`missing source for ${c.dest}: ${src}`);
    walk(src);
    if (c.dest.startsWith('apps/')) {
      if (!c.executable) throw new Error(`${c.dest}: app components must name their "executable"`);
      const exe = path.join(src, c.executable);
      if (!fs.existsSync(exe)) throw new Error(`${c.dest}: executable ${c.executable} not found in source`);
      if (!containsMarker(exe)) {
        throw new Error(`${c.dest}: ${c.executable} was not built with --features portable-dist`);
      }
    }
  }
  return spec;
}

function copyFile(from, to) {
  fs.mkdirSync(path.dirname(to), { recursive: true });
  fs.copyFileSync(from, to, fs.constants.COPYFILE_EXCL);
  fs.chmodSync(to, fs.statSync(from).mode & 0o755);
}

function launcher(name, body) {
  return { name, body };
}

const LAUNCHERS = [
  launcher(
    'Start-Mac.command',
    '#!/bin/sh\ncd "$(dirname "$0")" || exit 1\nopen "apps/macos-arm64/NanoNexus.app"\n',
  ),
  launcher('Start-Windows.cmd', '@echo off\r\ncd /d "%~dp0"\r\nstart "" "apps\\windows-x64\\NanoNexus.exe"\r\n'),
];

function main() {
  const a = args(process.argv.slice(2));
  const specPath = path.resolve(a.spec);
  const spec = validateSpec(JSON.parse(fs.readFileSync(specPath, 'utf8')), path.dirname(specPath));
  const out = path.resolve(a.out);
  const exists = fs.existsSync(out);
  if (!a.update && exists && fs.readdirSync(out).length > 0) fail(`${out} is not empty; use --update for an existing DIST`);
  if (a.update) {
    if (!exists || !fs.existsSync(path.join(out, 'portable.json'))) fail(`${out} is not an existing DIST`);
    if (fs.existsSync(path.join(out, 'data', 'nanonexus.lock')) || fs.existsSync(path.join(out, '.running'))) {
      fail('the DIST appears to be in use; close the app first');
    }
  }
  fs.mkdirSync(out, { recursive: true });

  const stamp = new Date().toISOString().replace(/[:.]/g, '-');
  const rollback = path.join(out, '.rollback', stamp);
  const manifest = { schema: 1, version: spec.version, files: {} };

  for (const c of spec.components) {
    const src = path.resolve(path.dirname(specPath), c.source ?? '');
    const dst = path.join(out, c.dest);
    if (fs.existsSync(dst)) {
      if (!a.update) fail(`${c.dest} already exists`);
      fs.mkdirSync(path.dirname(path.join(rollback, c.dest)), { recursive: true });
      fs.renameSync(dst, path.join(rollback, c.dest));
    }
    const single = fs.lstatSync(src).isFile();
    for (const rel of walk(src)) {
      const from = single ? src : path.join(src, rel);
      const to = single ? dst : path.join(dst, rel);
      copyFile(from, to);
      manifest.files[path.posix.join(c.dest, rel)] = sha256(to);
    }
  }

  // Create only what is missing; existing user data and the existing marker are never touched.
  const marker = path.join(out, 'portable.json');
  if (!fs.existsSync(marker)) fs.writeFileSync(marker, '{"schema":1}');
  for (const d of USER_DIRS) fs.mkdirSync(path.join(out, d), { recursive: true });
  for (const l of LAUNCHERS) {
    const p = path.join(out, l.name);
    if (!fs.existsSync(p) || a.update) fs.writeFileSync(p, l.body, { mode: 0o755 });
  }
  // Top-level documents: [{ "name": "quick-start-guide.html", "source": "path" }]; plain file names only.
  for (const d of spec.docs ?? []) {
    if (!/^[A-Za-z0-9._-]+$/.test(d.name) || d.name.startsWith('.')) throw new Error(`bad document name ${d.name}`);
    const from = path.resolve(path.dirname(specPath), d.source);
    const to = path.join(out, d.name);
    fs.copyFileSync(from, to);
    manifest.files[d.name] = sha256(to);
  }
  fs.writeFileSync(path.join(out, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`assembled ${Object.keys(manifest.files).length} files into ${out}`);
  if (fs.existsSync(rollback)) console.log(`previous versions kept in ${rollback}`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    main();
  } catch (e) {
    fail(e.message);
  }
}
