#!/usr/bin/env node
// bundle.mjs: the one-command bundler. Models (catalog, Hugging Face, or your own .gguf) + knowledge
// libraries -> a finished macOS app and/or Windows installer/portable folder. Node 22+, no dependencies.
//
//   node scripts/bundle.mjs --name studio --gguf ~/Models/my-model.gguf --knowledge ~/Books/reaper --target all
//
// Model options (repeat as needed; order: your --gguf files, then --hf, then --catalog; the first is the default unless --default is given):
//   --catalog <id>                       a model from catalog/models.json
//   --gguf <file>[=<id>[=<Display name>]]  your own downloaded .gguf (id defaults to the file name)
//   --hf <owner/repo>[:<quant>]          a Hugging Face GGUF repo
//   --default <id>                       which model is selected on first launch
// Other options:
//   --knowledge <folder>                 an offline reference library (repeat for several)
//   --name <profile>                     name of this bundle (default: "my-bundle"); saved in bundles/local/
//   --profile <name|path>                use an existing profile instead of the options above
//   --target mac|windows|windows-arm64|all   (default: the OS you are on)
//   --format installer|portable|auto     Windows only; auto = portable when the payload is over 1.8 GB
//   --stage-only                         prepare the payload and the Tauri overlay, do not build
//   --yes                                do not ask before building
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { ROOT, VENDOR_MODELS, ensureProfileEntry, loadCatalog, resolveProfile } from './fetch-model.mjs';
import { stageKnowledge } from './knowledge-manifest.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const NSIS_LIMIT = 1.8 * 1024 ** 3;
const TARGETS = {
  mac: { triple: 'aarch64-apple-darwin', dir: 'macos-arm64', win: false, label: 'macOS (Apple Silicon)' },
  windows: { triple: 'x86_64-pc-windows-msvc', dir: 'windows-x64', win: true, label: 'Windows x64' },
  'windows-arm64': { triple: 'aarch64-pc-windows-msvc', dir: 'windows-arm64', win: true, label: 'Windows ARM64' },
};

const say = (m) => console.log(m);
const step = (m) => console.log(`\n\x1b[1m▶ ${m}\x1b[0m`);
const fail = (m) => {
  console.error(`\n✖ ${m}`);
  process.exit(1);
};
const fmt = (b) => (b >= 1024 ** 3 ? `${(b / 1024 ** 3).toFixed(2)} GB` : `${(b / 1024 ** 2).toFixed(1)} MB`);
const slug = (s) => s.toLowerCase().replace(/[^a-z0-9.]+/g, '-').replace(/^-+|-+$/g, '') || 'model';
const readJson = (p) => JSON.parse(fs.readFileSync(p, 'utf8'));

export function parseArgs(argv) {
  const o = { catalog: [], gguf: [], hf: [], knowledge: [], name: 'my-bundle', target: null, format: 'auto' };
  const multi = new Set(['--catalog', '--gguf', '--hf', '--knowledge']);
  const single = new Set(['--name', '--profile', '--target', '--format', '--default']);
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '-h' || a === '--help') o.help = true;
    else if (a === '--stage-only') o.stageOnly = true;
    else if (a === '--yes') o.yes = true;
    else if (multi.has(a) || single.has(a)) {
      const v = argv[++i];
      if (v === undefined || v.startsWith('--')) fail(`${a} needs a value`);
      if (multi.has(a)) o[a.slice(2)].push(v);
      else o[a.slice(2)] = v;
    } else fail(`Unknown option ${a} (try --help)`);
  }
  return o;
}

/** `file.gguf=my-id=Display Name` -> a profile `local` entry. */
export function ggufEntry(spec) {
  const [file, id, ...name] = spec.split('=');
  const base = path.basename(file).replace(/\.gguf$/i, '');
  return { local: file, as: id ? slug(id) : slug(base), name: name.join('=') || base };
}

export function profileFromArgs(o) {
  const models = [
    ...o.gguf.map(ggufEntry),
    ...o.hf.map((s) => {
      const [repo, quant] = s.split(':');
      return { repo, ...(quant ? { quant } : {}), as: slug(`${repo.split('/').pop()}-${quant ?? 'gguf'}`) };
    }),
    ...o.catalog,
  ];
  if (models.length === 0) fail('Add at least one model: --gguf <file>, --catalog <id>, or --hf <repo>. (See --help.)');
  const idOf = (m) => (typeof m === 'string' ? m : m.as);
  const def = o.default ?? idOf(models[0]);
  if (!models.some((m) => idOf(m) === def)) fail(`--default ${def} is not one of the models: ${models.map(idOf).join(', ')}`);
  const targets = o.target === 'all' ? ['macos-arm64', 'windows-x64'] : [];
  return {
    schema: 1,
    name: slug(o.name),
    description: 'Created by scripts/bundle.mjs',
    models,
    default: def,
    targets: targets.length ? targets : ['macos-arm64'],
    ...(o.knowledge.length ? { knowledge: o.knowledge.map((p) => path.resolve(p)) } : {}),
  };
}

function hostTarget() {
  return process.platform === 'darwin' ? 'mac' : process.platform === 'win32' ? 'windows' : null;
}

function run(cmd, args, opts = {}) {
  say(`$ ${cmd} ${args.join(' ')}`);
  const r = spawnSync(cmd, args, { stdio: 'inherit', cwd: ROOT, shell: process.platform === 'win32', ...opts });
  if (r.status !== 0) fail(`${cmd} failed (exit ${r.status ?? 'signal'})`);
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

/** Stage models, knowledge, licenses (and the Windows runtime) for one target. Returns { dir, bytes }. */
export async function stage(profile, key, models) {
  const t = TARGETS[key];
  const dir = path.join(ROOT, 'vendor', 'bundle', profile.name, t.dir);
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  const manifest = {
    schema: 1,
    profile: profile.name,
    built_at: new Date().toISOString(),
    default: profile.default,
    models: [],
  };
  let bytes = 0;
  for (const lock of models) {
    link(path.join(VENDOR_MODELS, lock.id, lock.file), path.join(dir, 'models', lock.id, lock.file));
    manifest.models.push({
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
    bytes += lock.bytes;
  }
  fs.writeFileSync(path.join(dir, 'models', 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n');

  const kb = stageKnowledge(profile, ROOT, path.join(dir, 'knowledge'));
  if (kb) bytes += kb.libraries.reduce((n, l) => n + l.bytes, 0);

  const rt = path.join(ROOT, 'vendor', 'runtime', t.dir);
  fs.mkdirSync(path.join(dir, 'licenses'), { recursive: true });
  if (fs.existsSync(path.join(rt, 'licenses'))) fs.cpSync(path.join(rt, 'licenses'), path.join(dir, 'licenses'), { recursive: true });
  if (t.win) {
    if (!fs.existsSync(path.join(rt, 'llama-server.exe'))) {
      step(`Fetching the ${t.label} runtime`);
      run('node', [path.join(HERE, 'fetch-runtime-windows.mjs'), '--arch', t.dir.replace('windows-', '')]);
    }
    fs.cpSync(rt, path.join(dir, 'runtime'), { recursive: true, filter: (s) => !s.endsWith('runtime.lock.json') });
    bytes += fs.readdirSync(path.join(dir, 'runtime')).reduce((n, f) => n + fs.statSync(path.join(dir, 'runtime', f)).size, 0);
  }
  fs.writeFileSync(path.join(dir, 'bundle.json'), JSON.stringify({ profile: profile.name, built_at: manifest.built_at, models: models.map((m) => m.id), knowledge: kb ? kb.libraries.map((l) => l.id) : [] }, null, 2));

  // Tauri overlay: paths are relative to apps/desktop/src-tauri.
  const rel = path.relative(path.join(ROOT, 'apps', 'desktop', 'src-tauri'), dir).split(path.sep).join('/');
  const resources = { [`${rel}/models/`]: 'models/', [`${rel}/licenses/`]: 'licenses/', [`${rel}/bundle.json`]: 'bundle.json' };
  if (kb) resources[`${rel}/knowledge/`] = 'knowledge/';
  if (t.win) resources[`${rel}/runtime/`] = 'runtime/';
  const overlay = path.join(ROOT, 'apps', 'desktop', 'src-tauri', 'gen', `tauri.bundle.${profile.name}.${t.dir}.json`);
  fs.mkdirSync(path.dirname(overlay), { recursive: true });
  fs.writeFileSync(overlay, JSON.stringify({ bundle: { resources } }, null, 2));
  return { dir, bytes, overlay, kb };
}

function build(key, st, format, outDir) {
  const t = TARGETS[key];
  const tauri = ['-C', 'apps/desktop', 'tauri', 'build', '--config', st.overlay];
  fs.mkdirSync(outDir, { recursive: true });
  if (!t.win) {
    if (!fs.existsSync(path.join(ROOT, 'vendor', 'runtime', 'macos-arm64', 'llama-server-aarch64-apple-darwin'))) {
      step('Building the macOS runtime (one time)');
      run('bash', [path.join(HERE, 'setup-runtime.sh')]);
    }
    run('pnpm', [...tauri, '--bundles', 'app']);
    const app = path.join(ROOT, 'target', 'release', 'bundle', 'macos', 'NanoNexus.app');
    if (!fs.existsSync(app)) fail(`Build finished but ${app} is missing`);
    const dest = path.join(outDir, 'NanoNexus.app');
    fs.rmSync(dest, { recursive: true, force: true });
    fs.cpSync(app, dest, { recursive: true });
    run('codesign', ['--force', '--deep', '--sign', '-', dest]); // ad-hoc so it launches on this Mac
    return [dest];
  }
  const cross = process.platform !== 'win32' ? ['--runner', 'cargo-xwin'] : [];
  const portable = format === 'portable';
  run('pnpm', [...tauri, ...cross, '--target', t.triple, ...(portable ? ['--no-bundle'] : ['--bundles', 'nsis'])]);
  const rel = path.join(ROOT, 'target', t.triple, 'release');
  if (!portable) {
    const nsis = path.join(rel, 'bundle', 'nsis');
    const files = fs.existsSync(nsis) ? fs.readdirSync(nsis).filter((f) => f.endsWith('.exe')) : [];
    if (!files.length) fail(`No installer found in ${nsis}`);
    return files.map((f) => {
      const dest = path.join(outDir, f);
      fs.copyFileSync(path.join(nsis, f), dest);
      return dest;
    });
  }
  // Portable folder: the exe next to runtime\, models\, knowledge\ and portable.flag (data goes to .\data\).
  const folder = path.join(outDir, `NanoNexus-${t.dir}`);
  fs.rmSync(folder, { recursive: true, force: true });
  fs.mkdirSync(folder, { recursive: true });
  fs.copyFileSync(path.join(rel, 'nano-nexus.exe'), path.join(folder, 'nano-nexus.exe'));
  for (const entry of fs.readdirSync(st.dir)) {
    fs.cpSync(path.join(st.dir, entry), path.join(folder, entry), { recursive: true });
  }
  fs.writeFileSync(path.join(folder, 'portable.flag'), '');
  fs.writeFileSync(path.join(folder, 'README.txt'), 'Run nano-nexus.exe. Everything (settings, chats) stays in the data\\ folder next to it.\r\nNeeds Windows 11, or Windows 10 with the WebView2 runtime.\r\n');
  return [folder];
}

async function main() {
  const o = parseArgs(process.argv.slice(2));
  if (o.help) {
    console.log(fs.readFileSync(fileURLToPath(import.meta.url), 'utf8').split('\n').slice(1, 25).map((l) => l.replace(/^\/\/ ?/, '')).join('\n'));
    return;
  }
  const host = hostTarget();
  const wanted = o.target === 'all' ? ['mac', 'windows'] : [o.target ?? host ?? fail('Choose a target with --target mac|windows|all')];
  for (const w of wanted) if (!TARGETS[w]) fail(`Unknown target "${w}" (mac, windows, windows-arm64, all)`);
  if (wanted.includes('mac') && host !== 'mac') fail('macOS apps can only be built on a Mac.');
  if (wanted.some((w) => TARGETS[w].win) && host === null) fail('Windows builds run on a Mac or Windows PC.');

  const profile = o.profile ? resolveProfile(o.profile).profile : profileFromArgs(o);
  if (!o.profile) {
    fs.mkdirSync(path.join(ROOT, 'bundles', 'local'), { recursive: true });
    fs.writeFileSync(path.join(ROOT, 'bundles', 'local', `${profile.name}.json`), JSON.stringify(profile, null, 2) + '\n');
    say(`Saved profile bundles/local/${profile.name}.json (reuse it with --profile ${profile.name})`);
  }

  step('Preparing models');
  const catalog = loadCatalog();
  const locks = [];
  for (const entry of profile.models) locks.push(await ensureProfileEntry(entry, catalog, {}));
  const total = locks.reduce((n, l) => n + l.bytes, 0);
  say(`${locks.length} model(s), ${fmt(total)}; default: ${profile.default}`);

  const outDir = path.join(ROOT, 'dist-bundles', profile.name);
  const results = [];
  for (const key of wanted) {
    const t = TARGETS[key];
    step(`Staging ${t.label}`);
    const st = await stage(profile, key, locks);
    say(`Payload: ${fmt(st.bytes)}${st.kb ? ` (includes ${st.kb.libraries.length} knowledge librar${st.kb.libraries.length === 1 ? 'y' : 'ies'})` : ''}`);
    let format = o.format;
    if (t.win) {
      if (format === 'auto') format = st.bytes > NSIS_LIMIT ? 'portable' : 'installer';
      if (format === 'installer' && st.bytes > NSIS_LIMIT) say(`⚠ ${fmt(st.bytes)} is over the ~1.8 GB NSIS limit; the installer may fail. Use --format portable.`);
      say(`Windows format: ${format}`);
    }
    if (o.stageOnly) {
      results.push(st.dir);
      continue;
    }
    step(`Building ${t.label}`);
    results.push(...build(key, st, format, outDir));
  }

  step('Done');
  for (const r of results) say(`  ${r}`);
  if (!o.stageOnly) say('\nVerify the knowledge files with: node scripts/knowledge-manifest.mjs verify <app>/Contents/Resources/knowledge');
}

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) main().catch((e) => fail(e.stack || String(e)));
