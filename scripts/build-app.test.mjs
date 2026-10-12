import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const SCRIPT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), 'build-app.sh');
const run = (...args) => spawnSync('bash', [SCRIPT, ...args], { encoding: 'utf8' });

test('--dry-run lists every build step in order', () => {
  const r = run('--profile', 'basic', '--dry-run');
  assert.equal(r.status, 0, r.stderr);
  const wanted = [
    'doctor.sh --quiet',
    'fetch-model.mjs --profile basic',
    'fetch-model.mjs --verify',
    'setup-runtime.sh',
    'stage-bundle.mjs --profile basic --target macos-arm64',
    'build:web',
    'tauri build --bundles app --config',
    'verify-payload.mjs',
    'codesign --force --deep --options runtime',
    'codesign --verify --deep --strict',
    'ditto -c -k --keepParent',
    'SHA256SUMS.txt',
  ];
  let at = -1;
  for (const w of wanted) {
    const i = r.stdout.indexOf(w, at + 1);
    assert.ok(i > at, `"${w}" is missing or out of order in:\n${r.stdout}`);
    at = i;
  }
  assert.match(r.stdout, /NanoNexus-\d+\.\d+\.\d+-basic-macos-arm64\.zip/);
});

test('a signing identity is passed through, and bad input is refused', () => {
  const r = spawnSync('bash', [SCRIPT, '--dry-run', '--skip-doctor'], { encoding: 'utf8', env: { ...process.env, NEXUS_SIGN_IDENTITY: 'NanoNexus Local Signing' } });
  assert.match(r.stdout, /--sign NanoNexus Local Signing /);
  assert.doesNotMatch(r.stdout, /doctor\.sh/);
  assert.equal(run('--target', 'windows-x64', '--dry-run').status, 2);
  assert.equal(run('--profile', 'does-not-exist', '--dry-run').status, 1);
  assert.equal(run('--bogus').status, 2);
});
