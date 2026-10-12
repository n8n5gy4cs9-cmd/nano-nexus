#!/usr/bin/env node
// stage-bundle.mjs: profile + target -> the payload that goes into one build (BUNDLING §6).
//
//   node scripts/stage-bundle.mjs --profile basic --target macos-arm64
//
// Writes vendor/bundle/<profile>/<target>/ (hard-linked models, models/manifest.json, knowledge, licenses,
// runtime for Windows, bundle.json) and the generated Tauri overlay
// apps/desktop/src-tauri/gen/tauri.bundle.<profile>.<target>.json. It never downloads models: run
// `node scripts/fetch-model.mjs --profile <p>` first. Node 22+, no dependencies.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { stageKnowledge } from './knowledge-manifest.mjs';
import { validate } from './lib/jsonschema.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, '..');

/** Build targets, keyed the way bundle.mjs names them. `dir` is the profile/vendor name. */
export const TARGETS = {
  mac: { triple: 'aarch64-apple-darwin', dir: 'macos-arm64', win: false, label: 'macOS (Apple Silicon)' },
  windows: { triple: 'x86_64-pc-windows-msvc', dir: 'windows-x64', win: true, label: 'Windows x64' },
  'windows-arm64': { triple: 'aarch64-pc-windows-msvc', dir: 'windows-arm64', win: true, label: 'Windows ARM64' },
};

/** `macos-arm64` | `windows-x64` | `mac` | … -> the TARGETS key, or null. */
export function targetKey(name) {
  if (TARGETS[name]) return name;
  return Object.keys(TARGETS).find((k) => TARGETS[k].dir === name) ?? null;
}

const readJson = (p) => JSON.parse(fs.readFileSync(p, 'utf8'));

/** The lock of a model in `modelsDir` whose file is present with the right size, else null. */
export function readLock(modelsDir, id) {
  try {
    const lock = readJson(path.join(modelsDir, id, 'lock.json'));
    return fs.statSync(path.join(modelsDir, id, lock.file)).size === lock.bytes ? lock : null;
  } catch {
    return null;
  }
}

/** The ids a profile's models resolve to (catalog id, or the entry's `as`). */
export function profileIds(profile) {
  return profile.models.map((m) => (typeof m === 'string' ? m : m.as));
}

function link(src, dest) {
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  fs.rmSync(dest, { force: true });
  try {
    fs.linkSync(src, dest);
  } catch {
    fs.copyFileSync(src, dest);
  }
}

function gitCommit(root) {
  const r = spawnSync('git', ['rev-parse', '--short=12', 'HEAD'], { cwd: root, encoding: 'utf8' });
  return r.status === 0 ? r.stdout.trim() : null;
}

function appVersion(root) {
  try {
    return readJson(path.join(root, 'apps', 'desktop', 'src-tauri', 'tauri.conf.json')).version ?? null;
  } catch {
    return null;
  }
}

function catalogEntry(root, id) {
  try {
    return readJson(path.join(root, 'catalog', 'models.json')).models.find((m) => m.id === id) ?? null;
  } catch {
    return null;
  }
}

function defaultFetchRuntime(t, root) {
  const r = spawnSync('node', [path.join(HERE, 'fetch-runtime-windows.mjs'), '--arch', t.dir.replace('windows-', '')], {
    stdio: 'inherit',
    cwd: root,
  });
  if (r.status !== 0) throw new Error(`fetching the ${t.label} runtime failed`);
}

/**
 * Stage `models` (locks from vendor/models) for one target. Returns `{ dir, bytes, overlay, kb, manifest }`.
 * `opts`: `root` (repo root), `modelsDir`, `fetchRuntime(t)`, `now` (ISO string).
 */
export async function stage(profile, key, models, opts = {}) {
  const root = opts.root ?? REPO;
  const modelsDir = opts.modelsDir ?? path.join(root, 'vendor', 'models');
  const t = TARGETS[key];
  if (!t) throw new Error(`unknown target ${key}`);
  const dir = path.join(root, 'vendor', 'bundle', profile.name, t.dir);
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });

  const builtAt = opts.now ?? new Date().toISOString();
  const commit = gitCommit(root);
  const version = appVersion(root);
  const manifest = {
    schema: 1,
    profile: profile.name,
    built_at: builtAt,
    ...(version ? { app_version: version } : {}),
    ...(commit ? { git_commit: commit } : {}),
    default: profile.default,
    models: [],
  };
  let bytes = 0;
  for (const lock of models) {
    link(path.join(modelsDir, lock.id, lock.file), path.join(dir, 'models', lock.id, lock.file));
    const cat = catalogEntry(root, lock.catalog_id ?? lock.id);
    manifest.models.push({
      id: lock.id,
      name: lock.name ?? cat?.name ?? lock.id,
      path: `${lock.id}/${lock.file}`,
      sha256: lock.sha256,
      bytes: lock.bytes,
      ...(lock.repo ? { repo: lock.repo } : {}),
      ...(lock.revision ? { revision: lock.revision } : {}),
      ...(lock.license ? { license: lock.license } : {}),
      catalog_id: lock.catalog_id ?? null,
      ...(cat?.sampling ? { sampling: cat.sampling } : {}),
      ...(cat?.capabilities ? { capabilities: cat.capabilities } : {}),
    });
    bytes += lock.bytes;
  }
  if (!manifest.models.some((m) => m.id === manifest.default)) {
    throw new Error(`default model "${manifest.default}" is not in the bundle (${manifest.models.map((m) => m.id).join(', ')})`);
  }
  const problems = validate(manifest, 'bundle-manifest.schema.json');
  if (problems.length) throw new Error(`the generated manifest is invalid:\n  ${problems.join('\n  ')}`);
  fs.writeFileSync(path.join(dir, 'models', 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n');

  const kb = stageKnowledge(profile, root, path.join(dir, 'knowledge'));
  if (kb) bytes += kb.libraries.reduce((n, l) => n + l.bytes, 0);

  const rt = path.join(root, 'vendor', 'runtime', t.dir);
  fs.mkdirSync(path.join(dir, 'licenses'), { recursive: true });
  if (fs.existsSync(path.join(rt, 'licenses'))) fs.cpSync(path.join(rt, 'licenses'), path.join(dir, 'licenses'), { recursive: true });
  const modelLicenses = manifest.models.map((m) => `${m.id}\t${m.license ?? 'unknown'}\t${m.repo ?? 'local file'}`).join('\n');
  fs.writeFileSync(path.join(dir, 'licenses', 'models.txt'), `Bundled models (id, license, source)\n${modelLicenses}\n`);
  if (t.win) {
    if (!fs.existsSync(path.join(rt, 'llama-server.exe'))) (opts.fetchRuntime ?? ((x) => defaultFetchRuntime(x, root)))(t);
    fs.cpSync(rt, path.join(dir, 'runtime'), { recursive: true, filter: (s) => !s.endsWith('runtime.lock.json') });
    bytes += fs.readdirSync(path.join(dir, 'runtime')).reduce((n, f) => n + fs.statSync(path.join(dir, 'runtime', f)).size, 0);
  }
  // Shown in Settings → About.
  fs.writeFileSync(
    path.join(dir, 'bundle.json'),
    JSON.stringify(
      {
        profile: profile.name,
        built_at: builtAt,
        ...(commit ? { git_commit: commit } : {}),
        ...(version ? { app_version: version } : {}),
        target: t.dir,
        models: models.map((m) => m.id),
        knowledge: kb ? kb.libraries.map((l) => l.id) : [],
      },
      null,
      2,
    ) + '\n',
  );

  // Tauri overlay: paths are relative to apps/desktop/src-tauri.
  const rel = path.relative(path.join(root, 'apps', 'desktop', 'src-tauri'), dir).split(path.sep).join('/');
  const resources = { [`${rel}/models/`]: 'models/', [`${rel}/licenses/`]: 'licenses/', [`${rel}/bundle.json`]: 'bundle.json' };
  if (kb) resources[`${rel}/knowledge/`] = 'knowledge/';
  if (t.win) resources[`${rel}/runtime/`] = 'runtime/';
  const overlay = path.join(root, 'apps', 'desktop', 'src-tauri', 'gen', `tauri.bundle.${profile.name}.${t.dir}.json`);
  fs.mkdirSync(path.dirname(overlay), { recursive: true });
  fs.writeFileSync(overlay, JSON.stringify({ bundle: { resources } }, null, 2) + '\n');
  return { dir, bytes, overlay, kb, manifest };
}

/** The locks of every model in the profile, from vendor/models. Throws with a hint when one is missing. */
export function locksFor(profile, modelsDir) {
  return profileIds(profile).map((id) => {
    const lock = readLock(modelsDir, id);
    if (!lock) throw new Error(`model "${id}" is not in ${modelsDir}; run: node scripts/fetch-model.mjs --profile ${profile.name}`);
    return lock;
  });
}

function parseArgs(argv) {
  const o = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--profile' || a === '--target') o[a.slice(2)] = argv[++i];
    else if (a === '-h' || a === '--help') o.help = true;
    else throw new Error(`Unknown option ${a}`);
  }
  return o;
}

async function main() {
  const o = parseArgs(process.argv.slice(2));
  if (o.help || !o.profile || !o.target) {
    console.log('Usage: node scripts/stage-bundle.mjs --profile <name|path> --target macos-arm64|windows-x64|windows-arm64');
    process.exit(o.help ? 0 : 2);
  }
  const { resolveProfile, VENDOR_MODELS } = await import('./fetch-model.mjs');
  const key = targetKey(o.target);
  if (!key) throw new Error(`Unknown target "${o.target}"`);
  const { profile } = resolveProfile(o.profile);
  const problems = validate(profile, 'bundle-profile.schema.json');
  if (problems.length) throw new Error(`profile "${o.profile}" is invalid:\n  ${problems.join('\n  ')}`);
  const st = await stage(profile, key, locksFor(profile, VENDOR_MODELS));
  console.log(`staged ${st.manifest.models.length} model(s), ${st.bytes} bytes -> ${path.relative(REPO, st.dir)}`);
  console.log(`overlay: ${path.relative(REPO, st.overlay)}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((e) => {
    console.error(`✖ ${e.message}`);
    process.exit(1);
  });
}
