import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { buildWindows, chooseFormat, NSIS_LIMIT } from './build-windows.mjs';
import { fakeRoot } from './lib/fake-bundle.mjs';
import { verifyPayload } from './verify-payload.mjs';

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), 'build-windows.sh');

function fakePe(machine = 0x8664) {
  const b = Buffer.alloc(0x100);
  b.write('MZ');
  b.writeUInt32LE(0x80, 0x3c);
  b.write('PE\0\0', 0x80, 'latin1');
  b.writeUInt16LE(machine, 0x84);
  return b;
}

/** A fake repo with a Windows runtime, and an `exec` that pretends to be pnpm/zip. */
function world(ids, arch = 'x64') {
  const w = fakeRoot(ids);
  const machine = arch === 'x64' ? 0x8664 : 0xaa64;
  const rt = path.join(w.root, 'vendor', 'runtime', `windows-${arch}`);
  fs.mkdirSync(path.join(rt, 'licenses'), { recursive: true });
  fs.writeFileSync(path.join(rt, 'llama-server.exe'), fakePe(machine));
  fs.writeFileSync(path.join(rt, 'ggml.dll'), fakePe(machine));
  const triple = arch === 'x64' ? 'x86_64-pc-windows-msvc' : 'aarch64-pc-windows-msvc';
  const calls = [];
  const exec = (cmd, args, opts) => {
    calls.push([cmd, ...args]);
    const release = path.join(w.root, 'target', triple, 'release');
    if (cmd === 'pnpm' && args.includes('tauri')) {
      fs.mkdirSync(path.join(release, 'bundle', 'nsis'), { recursive: true });
      if (args.includes('--no-bundle')) fs.writeFileSync(path.join(release, 'nano-nexus.exe'), fakePe(machine));
      else fs.writeFileSync(path.join(release, 'bundle', 'nsis', `NanoNexus_0.1.0_${arch}-setup.exe`), fakePe(machine));
    }
    if (cmd === 'zip') fs.writeFileSync(args[1], `zip of ${args[2]} from ${opts?.cwd}`);
  };
  const deps = { root: w.root, modelsDir: w.modelsDir, exec, log: () => {}, version: '0.1.0' };
  const profile = { ...w.profile, targets: ['windows-x64'] };
  return { ...w, calls, deps, profile, out: path.join(w.root, 'dist', 'fake') };
}

const run = (w, opts) => buildWindows({ profile: w.profile, skipDoctor: true, skipFetch: true, ...opts }, w.deps);

test('format rules: auto, explicit and too big for NSIS', () => {
  assert.equal(chooseFormat('auto', 1.1 * 1024 ** 3), 'installer');
  assert.equal(chooseFormat('auto', NSIS_LIMIT + 1), 'kit');
  assert.equal(chooseFormat(undefined, 5), 'installer');
  assert.equal(chooseFormat('portable', 9 * 1024 ** 3), 'portable');
  assert.equal(chooseFormat('kit', 1), 'kit');
  assert.throws(() => chooseFormat('installer', NSIS_LIMIT + 1), /over the ~1\.8 GB NSIS limit/);
  assert.throws(() => chooseFormat('msi', 1), /unknown format/);
});

test('installer: one setup.exe, built with cargo-xwin for the right target', async () => {
  const w = world(['alpha']);
  const r = await run(w, { format: 'installer' });
  assert.equal(r.format, 'installer');
  assert.ok(fs.existsSync(path.join(w.out, 'NanoNexus-0.1.0-fake-windows-x64-setup.exe')));
  const tauri = w.calls.find((c) => c.includes('tauri'));
  assert.ok(tauri.includes('cargo-xwin') && tauri.includes('x86_64-pc-windows-msvc') && tauri.includes('nsis'));
  const sums = fs.readFileSync(path.join(w.out, 'SHA256SUMS-windows-x64.txt'), 'utf8');
  assert.match(sums, /^[0-9a-f]{64} {2}NanoNexus-0\.1\.0-fake-windows-x64-setup\.exe\n$/);
  // The overlay gives Tauri the staged resources, including the runtime.
  const overlay = JSON.parse(fs.readFileSync(path.join(w.root, 'apps/desktop/src-tauri/gen/tauri.bundle.fake.windows-x64.json'), 'utf8'));
  assert.equal(overlay.bundle.resources['../../../vendor/bundle/fake/windows-x64/runtime/'], 'runtime/');
  assert.equal(overlay.bundle.windows, undefined);
});

test('portable: exe + runtime + models + portable.flag, zipped, and it verifies', async () => {
  const w = world(['alpha', 'beta']);
  const r = await run(w, { format: 'portable' });
  assert.equal(r.format, 'portable');
  const folder = path.join(w.out, 'NanoNexus-0.1.0-fake-windows-x64-portable');
  for (const f of ['nano-nexus.exe', 'portable.flag', 'README-WINDOWS.txt', 'bundle.json', 'runtime/llama-server.exe', 'models/manifest.json', 'models/beta/beta-Q8_0.gguf']) {
    assert.ok(fs.existsSync(path.join(folder, f)), f);
  }
  assert.deepEqual(verifyPayload(folder, { windows: true, arch: 'x64' }).problems, []);
  assert.ok(w.calls.some((c) => c[0] === 'zip' && c.at(-1) === path.basename(folder)));
  assert.ok(!w.calls.some((c) => c.includes('nsis')) && w.calls.some((c) => c.includes('--no-bundle')));
  assert.ok(fs.existsSync(path.join(w.out, 'NanoNexus-0.1.0-fake-windows-x64-portable.zip')));
});

test('kit: the installer holds the default model, the others go to models-extra with a pack.json', async () => {
  const w = world(['alpha', 'beta', 'gamma']);
  const r = await run(w, { format: 'kit' });
  assert.equal(r.format, 'kit');
  const kit = path.join(w.out, 'NanoNexus-0.1.0-fake-windows-x64-kit');
  assert.ok(fs.existsSync(path.join(kit, 'NanoNexus-0.1.0-fake-windows-x64-setup.exe')));
  const pack = JSON.parse(fs.readFileSync(path.join(kit, 'models-extra', 'pack.json'), 'utf8'));
  assert.deepEqual(pack.models.map((m) => m.id), ['beta', 'gamma']);
  assert.ok(fs.existsSync(path.join(kit, 'models-extra', 'models', 'gamma', 'gamma-Q8_0.gguf')));
  assert.match(fs.readFileSync(path.join(kit, 'INSTALL.txt'), 'utf8'), /Model beta/);
  // Only the default model is staged for the installer, and WebView2 is embedded for offline PCs.
  const staged = JSON.parse(fs.readFileSync(path.join(w.root, 'vendor/bundle/fake/windows-x64/models/manifest.json'), 'utf8'));
  assert.deepEqual(staged.models.map((m) => m.id), ['alpha']);
  const overlay = JSON.parse(fs.readFileSync(path.join(w.root, 'apps/desktop/src-tauri/gen/tauri.bundle.fake.windows-x64.json'), 'utf8'));
  assert.equal(overlay.bundle.windows.webviewInstallMode.type, 'offlineInstaller');
  assert.deepEqual(verifyPayload(path.join(kit, 'models-extra'), { windows: false }).problems.filter((p) => !/manifest|bundle\.json/.test(p)), []);
});

test('arm64 builds use the arm64 triple and check the PE machine type', async () => {
  const w = world(['alpha'], 'arm64');
  await buildWindows({ profile: w.profile, arch: 'arm64', format: 'installer', skipDoctor: true, skipFetch: true }, w.deps);
  assert.ok(w.calls.find((c) => c.includes('tauri')).includes('aarch64-pc-windows-msvc'));
  assert.ok(fs.existsSync(path.join(w.out, 'NanoNexus-0.1.0-fake-windows-arm64-setup.exe')));
  // An x64 runtime in an arm64 build is caught before anything is built.
  const bad = world(['alpha'], 'arm64');
  fs.writeFileSync(path.join(bad.root, 'vendor/runtime/windows-arm64/llama-server.exe'), fakePe(0x8664));
  await assert.rejects(buildWindows({ profile: bad.profile, arch: 'arm64', format: 'installer', skipDoctor: true, skipFetch: true }, bad.deps), /machine 0x8664 is not arm64/);
  assert.ok(!bad.calls.some((c) => c.includes('tauri')));
});

test('the CLI prints a dry run without touching anything', () => {
  const r = spawnSync('bash', [SCRIPT, '--profile', 'basic', '--arch', 'arm64', '--format', 'portable', '--dry-run'], { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
  for (const w of ['doctor.sh --windows', 'fetch-runtime-windows.mjs --arch arm64', 'stage-bundle.mjs', 'build:web', 'aarch64-pc-windows-msvc', '--no-bundle', 'zip -qr']) assert.ok(r.stdout.includes(w), `${w} missing in:\n${r.stdout}`);
  assert.notEqual(spawnSync('bash', [SCRIPT, '--format', 'msi', '--dry-run'], { encoding: 'utf8' }).status, 0);
});
