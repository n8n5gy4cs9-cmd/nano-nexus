#!/usr/bin/env node
// fetch-model.mjs: download, verify, and pin GGUF models into vendor/models/ (Node 22+, no dependencies).
//
// The NanoNexus counterpart of the Godot project's select-game-model.cjs, extended to many models:
//   - Hugging Face repo + pinned revision (commit SHA) + SHA-256 from the HF LFS oid
//   - Range-resumable downloads with retries, size + SHA-256 + GGUF-magic verification
//   - one folder per model:  vendor/models/<id>/<file>.gguf + lock.json
//   - an index of everything: vendor/models/index.json  ({ default, models: {id: …} })
//
// Usage:
//   node scripts/fetch-model.mjs                                  fetch the catalog default model
//   node scripts/fetch-model.mjs --id minicpm5-2b-q4 [--id …]     fetch catalog entries
//   node scripts/fetch-model.mjs --profile basic|studio|<path>    fetch every model in a bundle profile
//   node scripts/fetch-model.mjs --repo owner/name --quant Q4_K_M [--file f.gguf] [--revision <rev>] [--as id] [--name "…"]
//   node scripts/fetch-model.mjs --local ~/Models/x.gguf --as my-model [--name "…"]
//   node scripts/fetch-model.mjs --verify [--id …]                re-hash files against their locks
//   node scripts/fetch-model.mjs --list                           show what's in vendor/models
//   node scripts/fetch-model.mjs --print-default-path             print the default model's absolute path
//   node scripts/fetch-model.mjs --set-default <id>               choose the dev-mode default model
// Options:
//   --update    re-resolve the revision (e.g. "main") instead of reusing the pinned commit from lock.json
//   --force     re-download even if the file is present and matches
//   --quiet     less output
// Env: HF_TOKEN (gated/private repos), HF_ENDPOINT (mirror), NODE_USE_ENV_PROXY=1 (if behind a proxy)

import fs from 'node:fs';
import fsp from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import { once } from 'node:events';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const VENDOR_MODELS = path.join(ROOT, 'vendor', 'models');
const INDEX_PATH = path.join(VENDOR_MODELS, 'index.json');
const CATALOG_PATH = path.join(ROOT, 'catalog', 'models.json');
const HF = (process.env.HF_ENDPOINT || 'https://huggingface.co').replace(/\/+$/, '');
const HF_HOST = new URL(HF).host;
const HF_TOKEN = process.env.HF_TOKEN || process.env.HUGGING_FACE_HUB_TOKEN || '';
const USER_AGENT = 'NanoNexus-fetch-model/1.0';
const MAX_ATTEMPTS = 6;

class UserError extends Error {}

let QUIET = false;
const log = (...a) => { if (!QUIET) console.error(...a); };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const rel = (p) => path.relative(ROOT, p) || '.';
const escapeRe = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const expandHome = (p) => (p === '~' || p.startsWith('~/') ? path.join(os.homedir(), p.slice(1)) : p);

function fmtBytes(n) {
  if (!Number.isFinite(n)) return '∞';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  while (n >= 1024 && i < units.length - 1) { n /= 1024; i++; }
  return `${n.toFixed(i ? 1 : 0)} ${units[i]}`;
}
function fmtDur(s) {
  if (!Number.isFinite(s) || s < 0) return '--';
  const m = Math.floor(s / 60);
  return m > 0 ? `${m}m${String(Math.round(s % 60)).padStart(2, '0')}s` : `${Math.round(s)}s`;
}

// ---------- JSON files -------------------------------------------------------------------------

function readJson(p, fallback) {
  try {
    return JSON.parse(fs.readFileSync(p, 'utf8'));
  } catch (e) {
    if (e.code === 'ENOENT' && fallback !== undefined) return fallback;
    throw new UserError(`Cannot read ${rel(p)}: ${e.message}`);
  }
}
function writeJsonAtomic(p, obj) {
  fs.mkdirSync(path.dirname(p), { recursive: true });
  const tmp = `${p}.tmp-${process.pid}`;
  fs.writeFileSync(tmp, `${JSON.stringify(obj, null, 2)}\n`);
  fs.renameSync(tmp, p);
}

export function loadCatalog(p = CATALOG_PATH) {
  const c = readJson(p);
  if (!Array.isArray(c.models)) throw new UserError(`${rel(p)} has no "models" array`);
  return c;
}
export function loadIndex() {
  return readJson(INDEX_PATH, { schema: 1, default: null, models: {} });
}
function saveIndex(idx) {
  writeJsonAtomic(INDEX_PATH, idx);
}
function register(lock) {
  const idx = loadIndex();
  idx.models[lock.id] = {
    name: lock.name ?? lock.id,
    file: `${lock.id}/${lock.file}`,
    sha256: lock.sha256,
    bytes: lock.bytes,
    source: lock.source,
    repo: lock.repo ?? null,
    revision: lock.revision ?? null,
    catalog_id: lock.catalog_id ?? null,
    license: lock.license ?? 'unknown',
  };
  if (!idx.default) idx.default = lock.id;
  saveIndex(idx);
}

// ---------- HTTP (manual redirects: the token is only sent to the HF host) ----------------------

async function http(url, { headers = {}, method = 'GET' } = {}) {
  let current = url;
  for (let hop = 0; hop <= 10; hop++) {
    const u = new URL(current);
    const h = { 'User-Agent': USER_AGENT, ...headers };
    if (HF_TOKEN && u.host === HF_HOST) h.Authorization = `Bearer ${HF_TOKEN}`;
    const res = await fetch(current, { method, headers: h, redirect: 'manual' });
    const loc = res.headers.get('location');
    if (res.status >= 300 && res.status < 400 && loc) {
      await res.body?.cancel().catch(() => {});
      current = new URL(loc, current).toString();
      continue;
    }
    return res;
  }
  throw new Error(`too many redirects for ${url}`);
}

async function getJson(url) {
  const res = await http(url, { headers: { Accept: 'application/json' } });
  if (!res.ok) {
    const body = await res.text().catch(() => '');
    if (res.status === 401 || res.status === 403) {
      throw new UserError(`Hugging Face answered ${res.status} for ${url}. The repo may be gated or private: set HF_TOKEN.`);
    }
    if (res.status === 404) throw new UserError(`Not found on Hugging Face: ${url}`);
    throw new UserError(`Hugging Face answered ${res.status} for ${url}: ${body.slice(0, 200)}`);
  }
  return { json: await res.json(), res };
}

async function resolveRevision(repo, revision) {
  const { json } = await getJson(`${HF}/api/models/${repo}/revision/${encodeURIComponent(revision)}`);
  if (!json.sha) throw new UserError(`Could not resolve revision "${revision}" of ${repo}`);
  const license = json.cardData?.license ?? (json.tags || []).find((t) => t.startsWith('license:'))?.slice(8) ?? null;
  return { sha: json.sha, license };
}

async function listTree(repo, sha) {
  let url = `${HF}/api/models/${repo}/tree/${sha}?recursive=true`;
  const out = [];
  for (let page = 0; url && page < 50; page++) {
    const { json, res } = await getJson(url);
    if (!Array.isArray(json)) throw new UserError(`Unexpected tree response for ${repo}`);
    out.push(...json);
    const link = res.headers.get('link');
    const m = link && link.match(/<([^>]+)>;\s*rel="next"/);
    url = m ? new URL(m[1], HF).toString() : null;
  }
  return out.filter((e) => e.type === 'file');
}

function toRegex(pattern) {
  // Catalog patterns are Rust-style; translate the inline (?i) flag for JavaScript.
  return pattern.startsWith('(?i)') ? new RegExp(pattern.slice(4), 'i') : new RegExp(pattern);
}

function pickFile(files, { file, quant, pattern }) {
  const ggufs = files.filter((f) => f.path.toLowerCase().endsWith('.gguf') && !/mmproj/i.test(f.path));
  if (ggufs.length === 0) throw new UserError('This repo has no .gguf files.');
  if (file) {
    const exact = ggufs.find((f) => f.path === file || path.posix.basename(f.path) === file);
    if (exact) return exact;
    log(`  note: "${file}" not found; falling back to pattern/quant matching`);
  }
  const rx = pattern ? toRegex(pattern) : quant ? new RegExp(`(^|[-_.])${escapeRe(quant)}\\.gguf$`, 'i') : null;
  if (!rx) throw new UserError('Specify --file or --quant (or use a catalog entry with file_pattern).');
  const hits = ggufs.filter((f) => rx.test(path.posix.basename(f.path)));
  if (hits.length === 0) {
    throw new UserError(`No .gguf matches ${rx}. Available:\n  ${ggufs.map((f) => f.path).join('\n  ')}`);
  }
  if (hits.some((h) => /-\d{5}-of-\d{5}\.gguf$/i.test(h.path))) {
    throw new UserError('Split GGUF files (…-00001-of-0000N.gguf) are not supported by fetch-model yet. Pick a single-file quant or merge with llama-gguf-split --merge.');
  }
  if (hits.length === 1) return hits[0];
  const depth = (f) => f.path.split('/').length;
  hits.sort((a, b) => depth(a) - depth(b) || a.path.length - b.path.length);
  if (depth(hits[0]) < depth(hits[1])) return hits[0];
  throw new UserError(`Several files match: ${hits.map((h) => h.path).join(', ')}. Pass --file <name>.`);
}

// ---------- files, hashing, downloads ------------------------------------------------------------

function freeBytes(dir) {
  try {
    const s = fs.statfsSync(dir);
    return Number(s.bavail) * Number(s.bsize);
  } catch {
    return Infinity;
  }
}

function assertGguf(p) {
  const fd = fs.openSync(p, 'r');
  try {
    const buf = Buffer.alloc(4);
    fs.readSync(fd, buf, 0, 4, 0);
    if (buf.toString('latin1') !== 'GGUF') throw new UserError(`${p} is not a GGUF file (bad magic)`);
  } finally {
    fs.closeSync(fd);
  }
}

function makeProgress(label, total) {
  const t0 = Date.now();
  let lastPrint = 0, lastBytes = -1, lastT = t0, speed = 0, lastBucket = -1;
  return (done, final = false) => {
    if (QUIET) return;
    const now = Date.now();
    if (!final && now - lastPrint < 500) return;
    if (lastBytes >= 0) {
      const dt = (now - lastT) / 1000;
      if (dt > 0) {
        const inst = (done - lastBytes) / dt;
        speed = speed ? speed * 0.7 + inst * 0.3 : inst;
      }
    }
    lastBytes = done; lastT = now; lastPrint = now;
    const pct = total ? (done * 100) / total : 0;
    const eta = speed > 0 && total ? fmtDur((total - done) / speed) : '--';
    const text = `  ${label}  ${pct.toFixed(1)}%  ${fmtBytes(done)} / ${fmtBytes(total)}  ${fmtBytes(speed)}/s  ETA ${eta}`;
    if (process.stderr.isTTY) {
      process.stderr.write(`\r${text}   `);
      if (final) process.stderr.write('\n');
    } else {
      const bucket = Math.floor(pct / 10);
      if (final || bucket !== lastBucket) { console.error(text); lastBucket = bucket; }
    }
  };
}

export async function hashFile(p, label = 'hashing') {
  const total = fs.statSync(p).size;
  const h = crypto.createHash('sha256');
  const progress = makeProgress(label, total);
  let done = 0;
  for await (const chunk of fs.createReadStream(p, { highWaterMark: 4 << 20 })) {
    h.update(chunk);
    done += chunk.length;
    progress(done);
  }
  progress(done, true);
  return h.digest('hex');
}

async function downloadOnce(url, part, bytes, label) {
  let start = fs.existsSync(part) ? fs.statSync(part).size : 0;
  if (start > bytes) { fs.rmSync(part, { force: true }); start = 0; }
  if (start === bytes) return;

  const res = await http(url, { headers: start > 0 ? { Range: `bytes=${start}-` } : {} });
  if (res.status === 416) { await res.body?.cancel().catch(() => {}); return; }
  if (res.status === 401 || res.status === 403) {
    throw new UserError(`Download refused (${res.status}). The repo may be gated: set HF_TOKEN.`);
  }
  if (res.status !== 200 && res.status !== 206) throw new Error(`HTTP ${res.status}`);
  if (start > 0 && res.status === 200) { log('\n  server ignored the Range request; restarting from 0'); start = 0; }
  if (start > 0) log(`  resuming at ${fmtBytes(start)}`);

  const ws = fs.createWriteStream(part, { flags: start > 0 ? 'a' : 'w' });
  let writeError = null;
  ws.on('error', (e) => { writeError = e; });
  const progress = makeProgress(label, bytes);
  let done = start;
  try {
    for await (const chunk of res.body) {
      if (writeError) throw writeError;
      if (!ws.write(chunk)) await once(ws, 'drain');
      done += chunk.length;
      progress(done);
    }
  } finally {
    ws.end();
    await once(ws, 'close').catch(() => {});
  }
  if (writeError) throw new UserError(`Cannot write ${part}: ${writeError.message}`);
  progress(done, true);
  if (done < bytes) throw new Error(`connection closed early at ${fmtBytes(done)} of ${fmtBytes(bytes)}`);
}

async function downloadVerified({ url, dest, sha256, bytes, label }) {
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  const part = `${dest}.part`;
  const meta = `${part}.json`;
  if (fs.existsSync(part)) {
    const m = readJson(meta, null);
    if (!m || m.sha256 !== sha256 || m.bytes !== bytes) {
      log('  discarding a stale partial download');
      fs.rmSync(part, { force: true });
    }
  }
  writeJsonAtomic(meta, { url, sha256, bytes });

  const have = fs.existsSync(part) ? fs.statSync(part).size : 0;
  const need = bytes - have + 512 * 1024 * 1024;
  const free = freeBytes(path.dirname(dest));
  if (free < need) throw new UserError(`Not enough disk space: need ${fmtBytes(need)}, have ${fmtBytes(free)}.`);

  for (let attempt = 1; ; attempt++) {
    try {
      await downloadOnce(url, part, bytes, label);
      break;
    } catch (e) {
      if (e instanceof UserError || attempt >= MAX_ATTEMPTS) throw e;
      const wait = Math.min(2 ** (attempt - 1), 16);
      log(`\n  network error (${e.message}); retrying in ${wait}s (attempt ${attempt + 1}/${MAX_ATTEMPTS})`);
      await sleep(wait * 1000);
    }
  }

  const size = fs.statSync(part).size;
  if (size !== bytes) throw new UserError(`Size mismatch for ${path.basename(dest)}: got ${size}, expected ${bytes}.`);
  const got = await hashFile(part, 'verifying');
  if (got !== sha256) {
    fs.rmSync(part, { force: true });
    fs.rmSync(meta, { force: true });
    throw new UserError(`SHA-256 mismatch for ${path.basename(dest)} (got ${got}, expected ${sha256}). The partial file was deleted. Try again.`);
  }
  assertGguf(part);
  fs.renameSync(part, dest);
  fs.rmSync(meta, { force: true });
}

function removeOtherGgufs(dir, keep) {
  if (!fs.existsSync(dir)) return;
  for (const e of fs.readdirSync(dir)) {
    if (e !== keep && (e.endsWith('.gguf') || e.endsWith('.gguf.part') || e.endsWith('.gguf.part.json'))) {
      fs.rmSync(path.join(dir, e), { force: true });
    }
  }
}

// ---------- model sources ----------------------------------------------------------------------

/** Ensure a Hugging Face model is present, verified, locked and indexed. Returns its lock. */
export async function ensureHfModel({ id, name, repo, revision, file, quant, pattern, license, catalogId, update = false, force = false }) {
  if (!/^[a-z0-9][a-z0-9.]*(-[a-z0-9.]+)*$/.test(id)) throw new UserError(`Invalid model id "${id}" (lowercase letters, digits, dots, dashes).`);
  const dir = path.join(VENDOR_MODELS, id);
  const lockPath = path.join(dir, 'lock.json');
  const prev = readJson(lockPath, null);

  // Pinned by default: reuse the locked commit unless --update or an explicit --revision was given.
  const wantRev = revision && revision !== 'main' ? revision : !update && prev?.revision ? prev.revision : revision || 'main';
  const info = await resolveRevision(repo, wantRev);
  const files = await listTree(repo, info.sha);
  const f = pickFile(files, { file: file ?? (prev && !update ? prev.file : undefined), quant, pattern });
  if (!f.lfs?.oid) throw new UserError(`${f.path} is not stored in LFS, so its SHA-256 can't be verified.`);
  const sha256 = f.lfs.oid;
  const bytes = f.lfs.size ?? f.size;
  const base = path.posix.basename(f.path);
  const dest = path.join(dir, base);

  if (!force && prev && prev.sha256 === sha256 && fs.existsSync(dest) && fs.statSync(dest).size === bytes) {
    log(`✔ ${id} already present (${base}, ${fmtBytes(bytes)})`);
    register(prev);
    return prev;
  }

  log(`↓ ${id}: ${repo} @ ${info.sha.slice(0, 10)}  ${base}  (${fmtBytes(bytes)})`);
  const url = `${HF}/${repo}/resolve/${info.sha}/${f.path.split('/').map(encodeURIComponent).join('/')}`;
  await downloadVerified({ url, dest, sha256, bytes, label: id });
  removeOtherGgufs(dir, base);

  const lock = {
    schema: 1, id, name: name ?? id, source: 'huggingface', repo, revision: info.sha, file: base,
    sha256, bytes, license: license || info.license || 'unknown', catalog_id: catalogId ?? null,
    source_url: url, fetched_at: new Date().toISOString(),
  };
  writeJsonAtomic(lockPath, lock);
  register(lock);
  log(`✔ ${id} verified (sha256 ${sha256.slice(0, 16)}…)`);
  return lock;
}

/** Hash a local GGUF and hard-link (or copy) it into vendor/models/<id>/. Returns its lock. */
export async function ensureLocalModel({ id, name, local, force = false }) {
  if (!/^[a-z0-9][a-z0-9.]*(-[a-z0-9.]+)*$/.test(id)) throw new UserError(`Invalid model id "${id}".`);
  const src = path.resolve(expandHome(local));
  if (!fs.existsSync(src)) throw new UserError(`Local file not found: ${local}`);
  assertGguf(src);
  const dir = path.join(VENDOR_MODELS, id);
  const base = path.basename(src);
  const dest = path.join(dir, base);
  const lockPath = path.join(dir, 'lock.json');
  const prev = readJson(lockPath, null);
  const bytes = fs.statSync(src).size;

  log(`# ${id}: hashing ${src} (${fmtBytes(bytes)})`);
  const sha256 = await hashFile(src, id);
  if (!force && prev && prev.sha256 === sha256 && fs.existsSync(dest)) {
    log(`✔ ${id} already present`);
    register(prev);
    return prev;
  }
  fs.mkdirSync(dir, { recursive: true });
  removeOtherGgufs(dir, '');
  try {
    fs.linkSync(src, dest);
    log('  hard-linked (no extra disk space used)');
  } catch {
    log('  copying (different volume)…');
    await fsp.copyFile(src, dest);
  }
  const lock = {
    schema: 1, id, name: name ?? id, source: 'local', file: base, sha256, bytes, license: 'unknown',
    catalog_id: null, source_url: pathToFileURL(src).href, fetched_at: new Date().toISOString(),
  };
  writeJsonAtomic(lockPath, lock);
  register(lock);
  log(`✔ ${id} imported`);
  return lock;
}

function specFromCatalog(entry) {
  return {
    id: entry.id, name: entry.name, repo: entry.repo, revision: entry.revision || 'main',
    file: entry.file, quant: entry.quant, pattern: entry.file_pattern, license: entry.license, catalogId: entry.id,
  };
}

/** Resolve a bundle profile by name or path. Returns { profile, file }. */
export function resolveProfile(nameOrPath) {
  const candidates = [
    path.resolve(nameOrPath),
    path.join(ROOT, 'bundles', `${nameOrPath}.json`),
    path.join(ROOT, 'bundles', 'local', `${nameOrPath}.json`),
    path.join(ROOT, 'bundles', `${nameOrPath}.example.json`),
  ];
  const file = candidates.find((p) => fs.existsSync(p) && fs.statSync(p).isFile());
  if (!file) throw new UserError(`Bundle profile not found: ${nameOrPath} (looked in bundles/ and bundles/local/)`);
  const profile = readJson(file);
  if (profile.schema !== 1 || !Array.isArray(profile.models)) throw new UserError(`${rel(file)} is not a valid bundle profile`);
  return { profile, file };
}

/** Ensure one profile model entry (catalog id | {repo,…} | {local,…}). */
export async function ensureProfileEntry(entry, catalog, flags = {}) {
  if (typeof entry === 'string') {
    const c = catalog.models.find((m) => m.id === entry);
    if (!c) throw new UserError(`Catalog has no model "${entry}"`);
    return ensureHfModel({ ...specFromCatalog(c), ...flags });
  }
  if (entry.repo) {
    const id = entry.as ?? slugFor(entry.repo, entry.quant ?? entry.file ?? 'gguf');
    return ensureHfModel({ id, name: entry.name, repo: entry.repo, revision: entry.revision || 'main', file: entry.file, quant: entry.quant, ...flags });
  }
  if (entry.local) {
    if (!entry.as) throw new UserError(`Local model ${entry.local} needs an "as" id`);
    return ensureLocalModel({ id: entry.as, name: entry.name, local: entry.local, force: flags.force });
  }
  throw new UserError(`Unrecognized profile model entry: ${JSON.stringify(entry)}`);
}

function slugFor(repo, quant) {
  const name = repo.split('/').pop().replace(/-gguf$/i, '');
  return `${name}-${quant}`.toLowerCase().replace(/[^a-z0-9.]+/g, '-').replace(/^-+|-+$/g, '');
}

// ---------- commands ---------------------------------------------------------------------------

async function cmdVerify(ids) {
  const idx = loadIndex();
  const list = ids.length ? ids : Object.keys(idx.models);
  if (list.length === 0) throw new UserError('No models in vendor/models yet. Run: node scripts/fetch-model.mjs');
  let failed = 0;
  for (const id of list) {
    const entry = idx.models[id];
    if (!entry) { console.log(`✖ ${id}: not in index`); failed++; continue; }
    const lock = readJson(path.join(VENDOR_MODELS, id, 'lock.json'), null);
    const file = path.join(VENDOR_MODELS, entry.file);
    if (!lock || !fs.existsSync(file)) { console.log(`✖ ${id}: missing ${lock ? rel(file) : 'lock.json'}`); failed++; continue; }
    const got = await hashFile(file, `verify ${id}`);
    if (got === lock.sha256 && got === entry.sha256) console.log(`✔ ${id}  ${got.slice(0, 16)}…  ${fmtBytes(lock.bytes)}`);
    else { console.log(`✖ ${id}: SHA-256 mismatch (file ${got.slice(0, 16)}…, lock ${lock.sha256.slice(0, 16)}…)`); failed++; }
  }
  if (failed) { console.error(`${failed} model(s) failed verification`); process.exitCode = 1; }
}

function cmdList() {
  const idx = loadIndex();
  const ids = Object.keys(idx.models);
  if (ids.length === 0) { console.log('No models yet. Run: node scripts/fetch-model.mjs'); return; }
  for (const id of ids) {
    const m = idx.models[id];
    const star = idx.default === id ? '★' : ' ';
    console.log(`${star} ${id.padEnd(32)} ${fmtBytes(m.bytes).padStart(9)}  ${m.source.padEnd(11)} ${m.repo ?? ''}  ${m.file}`);
  }
  console.log('\n★ = default for dev mode (change with --set-default <id>)');
}

function cmdPrintDefaultPath() {
  const idx = loadIndex();
  const id = idx.default;
  const m = id && idx.models[id];
  if (!m) throw new UserError('No default model. Run: node scripts/fetch-model.mjs');
  const p = path.join(VENDOR_MODELS, m.file);
  if (!fs.existsSync(p)) throw new UserError(`Default model file missing: ${rel(p)}`);
  console.log(p);
}

function cmdSetDefault(id) {
  const idx = loadIndex();
  if (!idx.models[id]) throw new UserError(`"${id}" is not in vendor/models (see --list)`);
  idx.default = id;
  saveIndex(idx);
  log(`✔ default model: ${id}`);
}

function parseArgs(argv) {
  const o = { ids: [], flags: {} };
  const takes = new Set(['--id', '--profile', '--repo', '--quant', '--file', '--revision', '--as', '--name', '--local', '--catalog', '--set-default']);
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '-h' || a === '--help') o.help = true;
    else if (a === '--verify') o.verify = true;
    else if (a === '--list') o.list = true;
    else if (a === '--print-default-path') o.printDefault = true;
    else if (a === '--update') o.flags.update = true;
    else if (a === '--force') o.flags.force = true;
    else if (a === '--quiet') o.quiet = true;
    else if (takes.has(a)) {
      const v = argv[++i];
      if (v === undefined || v.startsWith('--')) throw new UserError(`${a} needs a value`);
      if (a === '--id') o.ids.push(v);
      else o[a.slice(2).replace(/-([a-z])/g, (_, c) => c.toUpperCase())] = v;
    } else throw new UserError(`Unknown argument: ${a} (see --help)`);
  }
  return o;
}

function printHelp() {
  const src = fs.readFileSync(fileURLToPath(import.meta.url), 'utf8').split('\n');
  const end = src.findIndex((l) => l.startsWith('import '));
  console.log(src.slice(1, end).map((l) => l.replace(/^\/\/ ?/, '')).join('\n').trim());
}

async function main() {
  const o = parseArgs(process.argv.slice(2));
  QUIET = Boolean(o.quiet || o.printDefault);
  if (o.help) return printHelp();
  if (o.list) return cmdList();
  if (o.printDefault) return cmdPrintDefaultPath();
  if (o.verify) return cmdVerify(o.ids);
  if (o.setDefault && !o.ids.length && !o.repo && !o.local && !o.profile) return cmdSetDefault(o.setDefault);

  const catalog = loadCatalog(o.catalog ? path.resolve(o.catalog) : CATALOG_PATH);
  const done = [];

  if (o.local) {
    if (!o.as) throw new UserError('--local needs --as <id>');
    done.push(await ensureLocalModel({ id: o.as, name: o.name, local: o.local, force: o.flags.force }));
  }
  if (o.repo) {
    if (!o.quant && !o.file) throw new UserError('--repo needs --quant <Q4_K_M|Q8_0|…> or --file <name.gguf>');
    const id = o.as ?? slugFor(o.repo, o.quant ?? path.basename(o.file, '.gguf'));
    done.push(await ensureHfModel({ id, name: o.name, repo: o.repo, revision: o.revision || 'main', file: o.file, quant: o.quant, ...o.flags }));
  }
  for (const id of o.ids) {
    const c = catalog.models.find((m) => m.id === id);
    if (!c) throw new UserError(`Catalog has no model "${id}". Ids: ${catalog.models.map((m) => m.id).join(', ')}`);
    done.push(await ensureHfModel({ ...specFromCatalog(c), ...o.flags }));
  }
  if (o.profile) {
    const { profile, file } = resolveProfile(o.profile);
    log(`Profile "${profile.name}" (${rel(file)}): ${profile.models.length} model(s)`);
    for (const entry of profile.models) done.push(await ensureProfileEntry(entry, catalog, o.flags));
  }
  if (!o.local && !o.repo && !o.ids.length && !o.profile) {
    const def = catalog.models.find((m) => m.isDefault);
    if (!def) throw new UserError('Catalog has no default model (isDefault: true)');
    done.push(await ensureHfModel({ ...specFromCatalog(def), ...o.flags }));
  }
  if (o.setDefault) cmdSetDefault(o.setDefault);

  const idx = loadIndex();
  console.log('\nModels ready:');
  for (const l of done) {
    console.log(`  ${idx.default === l.id ? '★' : ' '} ${l.id.padEnd(32)} ${fmtBytes(l.bytes).padStart(9)}  ${rel(path.join(VENDOR_MODELS, l.id, l.file))}`);
  }
}

const isMain = process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url;
if (isMain) {
  main().catch((e) => {
    if (e instanceof UserError) console.error(`\n✖ ${e.message}`);
    else console.error(`\n✖ Unexpected error: ${e.stack || e}`);
    process.exit(1);
  });
}
