import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const TAURI = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', 'apps', 'desktop', 'src-tauri');
const readJson = (f) => JSON.parse(fs.readFileSync(path.join(TAURI, f), 'utf8'));

test('the Windows config builds a per-user, uncompressed NSIS installer', () => {
  const c = readJson('tauri.windows.conf.json');
  assert.deepEqual(c.bundle.targets, ['nsis']);
  const nsis = c.bundle.windows.nsis;
  assert.equal(nsis.installMode, 'currentUser');
  assert.equal(nsis.compression, 'none'); // GGUF files do not compress; lzma would take minutes
  assert.equal(c.bundle.windows.webviewInstallMode.type, 'embedBootstrapper');
  // No sidecar on Windows: the runtime folder is a resource, so its DLLs sit next to llama-server.exe.
  assert.equal(c.bundle.externalBin, undefined);
  const win = c.app.windows[0];
  assert.equal(win.decorations, true);
  assert.equal(win.transparent, false);
});

test('the offline-kit hook copies models-extra next to the install', () => {
  const nsis = readJson('tauri.windows.conf.json').bundle.windows.nsis;
  const hook = fs.readFileSync(path.join(TAURI, nsis.installerHooks), 'utf8');
  assert.match(hook, /!macro NSIS_HOOK_POSTINSTALL/);
  assert.match(hook, /!macroend/);
  assert.match(hook, /\$EXEDIR\\models-extra\\pack\.json/);
  assert.match(hook, /CopyFiles \/SILENT "\$EXEDIR\\models-extra\\\*\.\*" "\$INSTDIR\\models-extra"/);
  // The jump over the two lines of the block must match their number.
  assert.match(hook, /IfFileExists "[^"]+" 0 \+3/);
});

test('the macOS and Windows configs do not leak into each other', () => {
  const mac = readJson('tauri.macos.conf.json');
  const win = readJson('tauri.windows.conf.json');
  assert.deepEqual(mac.bundle.targets, ['app']);
  assert.ok(mac.bundle.externalBin?.[0].includes('macos-arm64'));
  assert.equal(JSON.stringify(win).includes('externalBin'), false);
});
