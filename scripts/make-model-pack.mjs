#!/usr/bin/env node
// make-model-pack.mjs: write a model pack (models for an offline machine) to a folder or USB stick.
//
//   node scripts/make-model-pack.mjs --profile studio --out /Volumes/USB
//   node scripts/make-model-pack.mjs --id minicpm5-2b-q4 --id qwen3.5-4b-q4 --out ./packs --name travel
//
// Result: <out>/NanoNexus-ModelPack-<name>/ with pack.json, README.txt and models/<id>/<file>.gguf.
// Import it in the app (Models → Import model pack…) or by dropping it into models-extra\ (Windows kit).
// Models come from vendor/models (run fetch-model.mjs first). Node 22+, no dependencies.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { validate } from './lib/jsonschema.mjs';
import { profileIds, readLock } from './stage-bundle.mjs';
import { sha256File } from './verify-payload.mjs';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const slug = (s) => s.toLowerCase().replace(/[^a-z0-9.]+/g, '-').replace(/^-+|-+$/g, '') || 'pack';

function readme(name, models) {
  return [
    `NanoNexus model pack: ${name}`,
    '',
    'Models in this pack:',
    ...models.map((m) => `  - ${m.name} (${m.id}, ${(m.bytes / 1024 ** 3).toFixed(2)} GB)`),
    '',
    'To install on any NanoNexus (macOS or Windows), no internet needed:',
    '  1. Open NanoNexus → Models → "Import model pack…"',
    '  2. Choose this folder.',
    '  3. Wait while every file is verified, then pick the model in the chat header.',
    '',
  ].join('\n');
}

/**
 * Write a pack. `opts`: `ids`, `out` (parent folder), `name`, `modelsDir`, `now`, `onProgress(id, i, n)`.
 * Returns `{ dir, pack }`.
 */
export function makePack(opts) {
  const modelsDir = opts.modelsDir ?? path.join(REPO, 'vendor', 'models');
  if (!opts.ids?.length) throw new Error('no models selected (use --profile or --id)');
  if (!opts.out) throw new Error('--out <folder> is required');
  const locks = opts.ids.map((id) => {
    const lock = readLock(modelsDir, id);
    if (!lock) throw new Error(`model "${id}" is not in ${modelsDir}; run: node scripts/fetch-model.mjs --id ${id}`);
    return lock;
  });
  const name = slug(opts.name ?? (opts.ids.length === 1 ? opts.ids[0] : 'models'));
  const dir = path.resolve(opts.out, `NanoNexus-ModelPack-${name}`);
  if (fs.existsSync(path.join(dir, 'pack.json'))) throw new Error(`${dir} already holds a pack; choose another --name or remove it`);

  const entries = [];
  for (const [i, lock] of locks.entries()) {
    opts.onProgress?.(lock.id, i, locks.length);
    const dest = path.join(dir, 'models', lock.id, lock.file);
    fs.mkdirSync(path.dirname(dest), { recursive: true });
    fs.copyFileSync(path.join(modelsDir, lock.id, lock.file), dest);
    if (fs.statSync(dest).size !== lock.bytes || sha256File(dest) !== lock.sha256) {
      throw new Error(`${lock.id}: the copy does not match its hash (is the drive full or failing?)`);
    }
    entries.push({
      id: lock.id,
      name: lock.name ?? lock.id,
      path: `${lock.id}/${lock.file}`,
      sha256: lock.sha256,
      bytes: lock.bytes,
      ...(lock.repo ? { repo: lock.repo } : {}),
      ...(lock.revision ? { revision: lock.revision } : {}),
      ...(lock.license ? { license: lock.license } : {}),
      catalog_id: lock.catalog_id ?? null,
    });
  }
  const pack = {
    schema: 1,
    kind: 'model-pack',
    name,
    created_at: opts.now ?? new Date().toISOString(),
    created_by: 'make-model-pack.mjs',
    models: entries,
  };
  const problems = validate(pack, 'model-pack.schema.json');
  if (problems.length) throw new Error(`generated pack.json is invalid:\n  ${problems.join('\n  ')}`);
  fs.writeFileSync(path.join(dir, 'pack.json'), JSON.stringify(pack, null, 2) + '\n');
  fs.writeFileSync(path.join(dir, 'README.txt'), readme(name, entries));
  return { dir, pack };
}

/** Re-hash a pack folder. Returns a list of problems (empty = OK). */
export function verifyPack(dir) {
  const problems = [];
  let pack;
  try {
    pack = JSON.parse(fs.readFileSync(path.join(dir, 'pack.json'), 'utf8'));
  } catch {
    return ['pack.json is missing or not JSON'];
  }
  problems.push(...validate(pack, 'model-pack.schema.json'));
  for (const m of pack.models ?? []) {
    const file = path.join(dir, 'models', ...String(m.path).split('/'));
    if (!fs.existsSync(file)) problems.push(`${m.id}: ${m.path} is missing`);
    else if (fs.statSync(file).size !== m.bytes || sha256File(file) !== m.sha256) problems.push(`${m.id}: hash mismatch`);
  }
  return problems;
}

async function main() {
  const argv = process.argv.slice(2);
  const o = { ids: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--id') o.ids.push(argv[++i]);
    else if (a === '--profile') o.profile = argv[++i];
    else if (a === '--out') o.out = argv[++i];
    else if (a === '--name') o.name = argv[++i];
    else if (a === '--verify') o.verify = argv[++i];
    else if (a === '-h' || a === '--help') o.help = true;
    else throw new Error(`Unknown option ${a} (try --help)`);
  }
  if (o.help || (!o.verify && (!o.out || (!o.profile && !o.ids.length)))) {
    console.log(fs.readFileSync(fileURLToPath(import.meta.url), 'utf8').split('\n').slice(1, 9).map((l) => l.replace(/^\/\/ ?/, '')).join('\n') + '\n  --verify <pack folder>   re-hash an existing pack');
    process.exit(o.help ? 0 : 2);
  }
  if (o.verify) {
    const problems = verifyPack(path.resolve(o.verify));
    problems.forEach((p) => console.error(`  ✖ ${p}`));
    console.log(problems.length ? '✖ pack check failed' : '✔ pack OK');
    process.exit(problems.length ? 1 : 0);
  }
  let { ids, name } = o;
  if (o.profile) {
    const { resolveProfile } = await import('./fetch-model.mjs');
    const { profile } = resolveProfile(o.profile);
    ids = [...profileIds(profile), ...ids];
    name ??= profile.name;
  }
  const { dir, pack } = makePack({ ids, out: o.out, name, onProgress: (id, i, n) => console.log(`[${i + 1}/${n}] copying ${id} …`) });
  const total = pack.models.reduce((n, m) => n + m.bytes, 0);
  console.log(`\n✔ ${pack.models.length} model(s), ${(total / 1024 ** 3).toFixed(2)} GB -> ${dir}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((e) => {
    console.error(`✖ ${e.message}`);
    process.exit(1);
  });
}
