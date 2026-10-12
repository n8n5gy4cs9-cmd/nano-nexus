#!/usr/bin/env node
// T14.15: LAN Share end to end in a real browser. The real share server (pairing, cookies, WebSocket,
// the embedded web UI) runs in front of a scripted fake backend (crates/nexus-share/examples/e2e_server.rs).
// Two viewports: a phone (390x844) and a desktop browser (1280x800).
//   pair -> chat -> stream -> stop -> history -> revoke
// Run with scripts/e2e.sh (it builds the web UI and the server first).
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import path from 'node:path';
import readline from 'node:readline';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const require = createRequire(import.meta.url);
const { chromium } = (() => {
  try {
    return require('playwright');
  } catch {
    return require(process.env.PLAYWRIGHT_PATH ?? '/opt/node-tools/node_modules/playwright');
  }
})();

/** Start the fake-backed share server; resolves with its URL and PIN plus a way to talk to it. */
function startServer() {
  const bin = path.join(ROOT, 'target', 'debug', 'examples', process.platform === 'win32' ? 'e2e_server.exe' : 'e2e_server');
  const child = spawn(bin, [], { stdio: ['pipe', 'pipe', 'inherit'] });
  const lines = readline.createInterface({ input: child.stdout });
  const waiting = [];
  const seen = [];
  lines.on('line', (l) => {
    let v;
    try {
      v = JSON.parse(l);
    } catch {
      return;
    }
    const w = waiting.shift();
    if (w) w(v);
    else seen.push(v);
  });
  const next = () => new Promise((resolve) => (seen.length ? resolve(seen.shift()) : waiting.push(resolve)));
  return next().then((first) => ({
    ...first,
    next,
    command: (c) => child.stdin.write(`${c}\n`),
    stop: () => {
      child.stdin.write('quit\n');
      child.stdin.end();
    },
  }));
}

const VIEWPORTS = [
  { name: 'phone 390x844', viewport: { width: 390, height: 844 }, mobile: true },
  { name: 'desktop 1280x800', viewport: { width: 1280, height: 800 }, mobile: false },
];

let browser;
test.before(async () => {
  browser = await chromium.launch({ args: ['--no-sandbox'] });
});
test.after(async () => {
  await browser?.close();
});

for (const vp of VIEWPORTS) {
  test(`pair, chat, stream, stop, history and revoke (${vp.name})`, async () => {
    const server = await startServer();
    const context = await browser.newContext({ viewport: vp.viewport, isMobile: vp.mobile, hasTouch: vp.mobile });
    const page = await context.newPage();
    try {
      await page.goto(server.url);

      // --- pairing: a wrong code is refused, the right PIN works -------------------------------
      await page.getByRole('heading', { name: 'Pair with NanoNexus' }).waitFor();
      await page.getByLabel('PIN or code').fill('000000');
      await page.getByRole('button', { name: 'Pair' }).click();
      await page.getByRole('alert').filter({ hasText: 'did not work' }).waitFor();
      await page.getByLabel('PIN or code').fill(server.pin);
      await page.getByRole('button', { name: 'Pair' }).click();
      const composer = page.getByRole('textbox', { name: 'Message' });
      await composer.waitFor();
      assert.equal(await page.evaluate((pin) => location.href.includes(pin) || location.href.includes('pair'), server.pin), false, 'the code is not left in the address bar');

      // The phone layout must not scroll sideways.
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
      assert.ok(overflow <= 0, `horizontal overflow of ${overflow}px`);

      // --- chat: the answer streams in --------------------------------------------------------
      await composer.fill('Say hello please');
      await composer.press('Enter');
      await page.getByText('Hello from the fake').waitFor();
      assert.equal(await page.getByText('of it.').count(), 0, 'the answer is still streaming');
      await page.getByText('of it.').waitFor({ timeout: 10_000 });

      // --- stop: a second answer is cut short and the partial text is kept ---------------------
      await composer.fill('Now stop in the middle');
      await composer.press('Enter');
      await page.getByText('fake model.').nth(1).waitFor();
      await page.getByRole('button', { name: 'Stop generating' }).click();
      await page.getByRole('button', { name: 'Send message' }).waitFor();
      const partial = await page.getByText('Hello from the fake').nth(1).textContent();
      assert.ok(partial && !partial.includes('of it.'), `stop did not cut the answer: ${partial}`);
      await page.waitForTimeout(800);
      assert.equal(await page.getByText('of it.').count(), 1, 'nothing more arrived after Stop');

      // --- history: the conversation is listed, and opening it shows both answers --------------
      if (vp.mobile) await page.getByRole('button', { name: 'History' }).click();
      const row = page.getByText('Say hello please').first();
      await row.waitFor();
      await page.reload();
      await composer.waitFor(); // the session survives a reload (cookie)
      if (vp.mobile) await page.getByRole('button', { name: 'History' }).click();
      await page.getByText('Say hello please').first().click();
      await page.getByText('Now stop in the middle').waitFor();
      assert.equal(await page.getByText('of it.').count(), 1);

      // --- revoke: the device is signed out at once and cannot come back ------------------------
      server.command('revoke');
      assert.deepEqual(await server.next(), { revoked: true });
      await page.getByRole('heading', { name: 'Pair with NanoNexus' }).waitFor({ timeout: 10_000 });
      const status = await page.evaluate(() => fetch('/api/session').then((r) => r.status));
      assert.equal(status, 401);
      // A fresh code pairs again.
      server.command('pin');
      const again = await server.next();
      await page.getByLabel('PIN or code').fill(again.pin);
      await page.getByRole('button', { name: 'Pair' }).click();
      await composer.waitFor();
    } finally {
      await context.close();
      server.stop();
    }
  });
}
