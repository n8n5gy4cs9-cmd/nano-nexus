#!/usr/bin/env node
// build-windows.mjs: a Windows build, cross-compiled on the Mac (M13, BUNDLING §7).
//
//   scripts/build-windows.sh --profile studio --arch x64 --format auto
//
//   --profile <name|path>              a bundle profile (default: basic)
//   --arch x64|arm64                   default x64
//   --format installer|portable|kit|auto   auto = installer, or kit when the payload is over 1.8 GB
//   --skip-doctor --skip-fetch         skip the toolchain check / the model + runtime download
//   (--skip-models: library option, the caller already fetched the models)
//   --dry-run                          print the steps, run nothing
//
// installer  dist/<p>/NanoNexus-<v>-<p>-windows-<arch>-setup.exe
// portable   dist/<p>/NanoNexus-<v>-<p>-windows-<arch>-portable.zip   (unzip, run; data stays in .\data\)
// kit        dist/<p>/NanoNexus-<v>-<p>-windows-<arch>-kit/  setup.exe (default model) + models-extra\ + INSTALL.txt
// Every format ends with verify-payload.mjs --windows and a SHA256SUMS.txt. Node 22+, no dependencies.
import { spawnSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { validate } from './lib/jsonschema.mjs';
import { makePack } from './make-model-pack.mjs';
import { locksFor, profileIds, stage } from './stage-bundle.mjs';
import { verifyPayload } from './verify-payload.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, '..');

/** The installer toolchain (NSIS) cannot produce much more than 2 GB; stay below it. */
export const NSIS_LIMIT = 1.8 * 1024 ** 3;

export const ARCHES = {
  x64: { key: 'windows', triple: 'x86_64-pc-windows-msvc', dir: 'windows-x64', label: 'Windows x64' },
  arm64: { key: 'windows-arm64', triple: 'aarch64-pc-windows-msvc', dir: 'windows-arm64', label: 'Windows ARM64' },
};

/** `auto` picks installer unless the payload is too big for NSIS; an explicit installer that is too big is an error. */
export function chooseFormat(requested, payloadBytes) {
  const fmt = requested ?? 'auto';
  if (!['auto', 'installer', 'portable', 'kit'].includes(fmt)) throw new Error(`unknown format "${fmt}" (installer, portable, kit, auto)`);
  if (fmt === 'auto') return payloadBytes > NSIS_LIMIT ? 'kit' : 'installer';
  if (fmt === 'installer' && payloadBytes > NSIS_LIMIT) {
    throw new Error(`the payload is ${(payloadBytes / 1024 ** 3).toFixed(2)} GB, over the ~1.8 GB NSIS limit; use --format kit (models next to the installer) or --format portable`);
  }
  return fmt;
}

function sha256File(file) {
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

function dirBytes(dir) {
  let n = 0;
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    n += e.isDirectory() ? dirBytes(p) : fs.statSync(p).size;
  }
  return n;
}

const PORTABLE_README = 'Run nano-nexus.exe. Everything (settings, chats) stays in the data\\ folder next to it.\r\nNeeds Windows 11, or Windows 10 with the WebView2 runtime.\r\n';

function kitInstallText(extraNames) {
  return [
    'NanoNexus offline kit',
    '',
    '1. Run the *-setup.exe in this folder. Keep the models-extra folder next to it while it runs.',
    '2. The installer needs no internet (the WebView2 runtime is inside it).',
    '3. Open NanoNexus. The first start checks and adds these models, which can take a minute:',
    ...extraNames.map((n) => `     - ${n}`),
    '',
    'Windows may say "Windows protected your PC" because the installer is not signed: choose More info, then Run anyway.',
    '',
  ].join('\r\n');
}

/** Real command runner; `--dry-run` swaps in one that only prints. */
export function realExec(cmd, args, opts = {}) {
  console.log(`  $ ${cmd} ${args.join(' ')}`);
  const r = spawnSync(cmd, args, { stdio: 'inherit', cwd: REPO, ...opts });
  if (r.status !== 0) throw new Error(`${cmd} failed (exit ${r.status ?? 'signal'})`);
}

/**
 * Build one Windows target. `deps` lets tests replace the outside world: `exec(cmd, args)`, `root`,
 * `modelsDir`, `log`, `dryRun`, `version`.
 */
export async function buildWindows(opts, deps = {}) {
  const root = deps.root ?? REPO;
  const modelsDir = deps.modelsDir ?? path.join(root, 'vendor', 'models');
  const exec = deps.exec ?? realExec;
  const log = deps.log ?? ((m) => console.log(m));
  const dry = !!deps.dryRun;
  const step = (m) => log(`\n\x1b[1m▶ ${m}\x1b[0m`);
  const arch = ARCHES[opts.arch ?? 'x64'];
  if (!arch) throw new Error(`unknown arch "${opts.arch}" (x64, arm64)`);
  const profile = opts.profile;
  const problems = validate(profile, 'bundle-profile.schema.json');
  if (problems.length) throw new Error(`profile "${profile.name}" is invalid:\n  ${problems.join('\n  ')}`);
  const version = deps.version ?? JSON.parse(fs.readFileSync(path.join(root, 'apps', 'desktop', 'src-tauri', 'tauri.conf.json'), 'utf8')).version;
  const stem = `NanoNexus-${version}-${profile.name}-windows-${arch.dir.replace('windows-', '')}`;
  const outDir = path.join(root, 'dist', profile.name);
  const runtimeDir = path.join(root, 'vendor', 'runtime', arch.dir);
  const cpuOnly = arch.dir === 'windows-arm64' || profile.windows?.runtime === 'cpu';

  if (!opts.skipDoctor) {
    step('1/8 Checking the toolchain');
    // Only what this build needs: its own architecture, and NSIS only when an installer is made.
    const wanted = opts.format ?? profile.windows?.format ?? 'auto';
    exec(path.join(HERE, 'doctor.sh'), ['--windows', '--arch', arch.dir.replace('windows-', ''), ...(wanted === 'portable' ? ['--no-nsis'] : []), '--quiet']);
  }
  if (!opts.skipFetch) {
    step('2/8 Models and runtime');
    if (!opts.skipModels) exec('node', [path.join(HERE, 'fetch-model.mjs'), '--profile', opts.profileArg ?? profile.name]);
    exec('node', [path.join(HERE, 'fetch-runtime-windows.mjs'), '--arch', arch.dir.replace('windows-', ''), ...(cpuOnly && arch.dir === 'windows-x64' ? ['--cpu-only'] : [])]);
  }

  step('3/8 Sizes and format');
  const locks = dry ? [] : locksFor(profile, modelsDir);
  const runtimeBytes = !dry && fs.existsSync(runtimeDir) ? dirBytes(runtimeDir) : 0;
  const modelBytes = locks.reduce((n, l) => n + l.bytes, 0);
  const defaultLock = locks.find((l) => l.id === profile.default);
  const requested = opts.format ?? profile.windows?.format ?? 'auto';
  const format = dry && requested === 'auto' ? 'installer' : chooseFormat(requested, modelBytes + runtimeBytes);
  log(`${locks.length} model(s) ${(modelBytes / 1024 ** 3).toFixed(2)} GB + runtime ${(runtimeBytes / 1024 ** 2).toFixed(0)} MB -> format: ${format}`);

  step('4/8 Staging');
  // A kit's installer carries only the default model; the others travel next to it in models-extra\.
  const inInstaller = format === 'kit' ? (defaultLock ? [defaultLock] : []) : locks;
  const kitProfile = format === 'kit' ? { ...profile, models: profileIds(profile).filter((id) => id === profile.default) } : profile;
  const webview = format === 'kit' ? 'offlineInstaller' : undefined;
  let staged = null;
  if (dry) {
    exec('node', [path.join(HERE, 'stage-bundle.mjs'), '--profile', profile.name, '--target', arch.dir, ...(format === 'kit' ? ['--only-default', '--webview', 'offlineInstaller'] : [])]);
  } else {
    staged = await stage(kitProfile, arch.key, inInstaller, { root, modelsDir, ...(webview ? { webview } : {}) });
    log(`staged ${staged.manifest.models.length} model(s) into ${path.relative(root, staged.dir)}`);
    const v = verifyPayload(staged.dir, { windows: true, arch: opts.arch ?? 'x64', manifest: path.join(staged.dir, 'models', 'manifest.json') });
    if (v.problems.length) throw new Error(`the staged payload is wrong:\n  ${v.problems.join('\n  ')}`);
  }

  step('5/8 Web UI');
  exec('pnpm', ['-C', 'apps/desktop', 'build:web']);

  step('6/8 tauri build (cargo-xwin)');
  const overlay = staged?.overlay ?? path.join(root, 'apps', 'desktop', 'src-tauri', 'gen', `tauri.bundle.${profile.name}.${arch.dir}.json`);
  const tauri = ['-C', 'apps/desktop', 'tauri', 'build', '--runner', 'cargo-xwin', '--target', arch.triple, '--config', overlay];
  exec('pnpm', [...tauri, ...(format === 'portable' ? ['--no-bundle'] : ['--bundles', 'nsis'])]);

  step('7/8 Packaging');
  const artifacts = [];
  const release = path.join(root, 'target', arch.triple, 'release');
  if (!dry) fs.mkdirSync(outDir, { recursive: true });
  if (format === 'installer' || format === 'kit') {
    const nsisDir = path.join(release, 'bundle', 'nsis');
    const found = dry ? [] : fs.existsSync(nsisDir) ? fs.readdirSync(nsisDir).filter((f) => f.endsWith('-setup.exe')) : [];
    if (!dry && found.length === 0) throw new Error(`no installer was produced in ${nsisDir}`);
    const target = format === 'kit' ? path.join(outDir, `${stem}-kit`) : outDir;
    const name = `${stem}-setup.exe`;
    log(`installer -> ${path.relative(root, path.join(target, name))}`);
    if (!dry) {
      fs.rmSync(format === 'kit' ? target : path.join(outDir, name), { recursive: true, force: true });
      fs.mkdirSync(target, { recursive: true });
      fs.copyFileSync(path.join(nsisDir, found[0]), path.join(target, name));
      artifacts.push(path.join(target, name));
      if (format === 'kit') {
        const extras = profileIds(profile).filter((id) => id !== profile.default);
        if (extras.length > 0) {
          const tmp = fs.mkdtempSync(path.join(outDir, '.pack-'));
          const { dir, pack } = makePack({ ids: extras, out: tmp, name: 'extra', modelsDir });
          fs.renameSync(dir, path.join(target, 'models-extra'));
          fs.rmSync(tmp, { recursive: true, force: true });
          fs.writeFileSync(path.join(target, 'INSTALL.txt'), kitInstallText(pack.models.map((m) => m.name)));
        } else {
          fs.writeFileSync(path.join(target, 'INSTALL.txt'), kitInstallText([]));
        }
        artifacts.push(path.join(target, 'INSTALL.txt'));
      }
    }
  } else {
    const folder = path.join(outDir, `${stem}-portable`);
    log(`portable folder -> ${path.relative(root, folder)}`);
    if (!dry) {
      fs.rmSync(folder, { recursive: true, force: true });
      fs.mkdirSync(folder, { recursive: true });
      fs.copyFileSync(path.join(release, 'nano-nexus.exe'), path.join(folder, 'nano-nexus.exe'));
      for (const entry of fs.readdirSync(staged.dir)) fs.cpSync(path.join(staged.dir, entry), path.join(folder, entry), { recursive: true });
      fs.writeFileSync(path.join(folder, 'portable.flag'), '');
      fs.writeFileSync(path.join(folder, 'README-WINDOWS.txt'), PORTABLE_README);
      const v = verifyPayload(folder, { windows: true, arch: opts.arch ?? 'x64' });
      if (v.problems.length) throw new Error(`the portable folder is wrong:\n  ${v.problems.join('\n  ')}`);
    }
    const zip = path.join(outDir, `${stem}-portable.zip`);
    if (!dry) fs.rmSync(zip, { force: true });
    exec('zip', ['-qr', zip, path.basename(folder)], { cwd: outDir });
    artifacts.push(zip);
  }

  step('8/8 Checksums');
  if (!dry) {
    const lines = [];
    for (const a of artifacts.filter((f) => !f.endsWith('INSTALL.txt'))) lines.push(`${sha256File(a)}  ${path.relative(outDir, a).split(path.sep).join('/')}`);
    fs.writeFileSync(path.join(outDir, `SHA256SUMS-windows-${arch.dir.replace('windows-', '')}.txt`), lines.join('\n') + '\n');
    log(`\n✔ ${format} for ${arch.label}:\n${artifacts.map((a) => `  ${path.relative(root, a)}`).join('\n')}`);
  }
  return { format, artifacts, stem };
}

function parseArgs(argv) {
  const o = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (['--profile', '--arch', '--format'].includes(a)) o[a.slice(2)] = argv[++i];
    else if (a === '--skip-doctor') o.skipDoctor = true;
    else if (a === '--skip-fetch') o.skipFetch = true;
    else if (a === '--dry-run') o.dryRun = true;
    else if (a === '-h' || a === '--help') o.help = true;
    else throw new Error(`Unknown option ${a} (try --help)`);
  }
  return o;
}

async function main() {
  const o = parseArgs(process.argv.slice(2));
  if (o.help) {
    console.log(fs.readFileSync(fileURLToPath(import.meta.url), 'utf8').split('\n').slice(1, 17).map((l) => l.replace(/^\/\/ ?/, '')).join('\n'));
    return;
  }
  const { resolveProfile } = await import('./fetch-model.mjs');
  const { profile } = resolveProfile(o.profile ?? 'basic');
  if (o.dryRun) {
    const exec = (cmd, args) => console.log(`  + ${cmd} ${args.join(' ')}`);
    await buildWindows({ profile, profileArg: o.profile ?? 'basic', arch: o.arch, format: o.format, skipDoctor: o.skipDoctor, skipFetch: o.skipFetch }, { exec, dryRun: true });
    return;
  }
  await buildWindows({ profile, profileArg: o.profile ?? 'basic', arch: o.arch, format: o.format, skipDoctor: o.skipDoctor, skipFetch: o.skipFetch });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((e) => {
    console.error(`\n✖ ${e.message}`);
    process.exit(1);
  });
}
