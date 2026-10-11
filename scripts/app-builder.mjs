#!/usr/bin/env node
// app-builder.mjs: a tiny local web UI around scripts/bundle.mjs. Pick models (your own .gguf from
// Downloads or anywhere, catalog models), reference-library folders, a target, and press Build.
// Remembers your last choices in bundles/local/app-builder-state.json (git-ignored).
// Listens on 127.0.0.1 only, behind a random token in the URL; exits after 30 idle minutes.
//
//   node scripts/app-builder.mjs [--port 8787] [--no-open]
import { spawn } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, '..');
const IDLE_MS = 30 * 60 * 1000;
const DEFAULT_PORT = 8787;
const STATE_FILE = path.join(ROOT, 'bundles', 'local', 'app-builder-state.json');

/** The last choices made in the page (null when none). */
export function readState() {
  try {
    return JSON.parse(fs.readFileSync(STATE_FILE, 'utf8'));
  } catch {
    return null;
  }
}

export function writeState(state) {
  if (!state || typeof state !== 'object' || Array.isArray(state)) throw new Error('bad state');
  fs.mkdirSync(path.dirname(STATE_FILE), { recursive: true });
  fs.writeFileSync(STATE_FILE, JSON.stringify(state, null, 2) + '\n');
}

export function listDir(dir) {
  const abs = path.resolve(dir);
  const entries = [];
  for (const name of fs.readdirSync(abs)) {
    if (name.startsWith('.')) continue;
    try {
      const full = path.join(abs, name);
      const st = fs.statSync(full);
      if (st.isDirectory()) entries.push({ name, path: full, dir: true, size: 0 });
      else if (/\.(gguf)$/i.test(name)) entries.push({ name, path: full, dir: false, size: st.size });
    } catch {
      /* unreadable entry */
    }
  }
  entries.sort((a, b) => Number(b.dir) - Number(a.dir) || a.name.localeCompare(b.name));
  const parent = path.dirname(abs);
  return { dir: abs, parent: parent === abs ? null : parent, entries };
}

function ggufsIn(dir) {
  try {
    return listDir(dir).entries.filter((e) => !e.dir);
  } catch {
    return [];
  }
}

/** Turn a build request into bundle.mjs arguments. Throws on bad input. */
export function buildArgs(b) {
  const args = [path.join(HERE, 'bundle.mjs'), '--yes'];
  const name = String(b.name || 'my-bundle').trim();
  if (!/^[A-Za-z0-9 _.-]{1,60}$/.test(name)) throw new Error('Use letters, digits, spaces, dashes in the name.');
  args.push('--name', name);
  for (const g of b.ggufs ?? []) {
    if (typeof g.path !== 'string' || !/\.gguf$/i.test(g.path) || !fs.existsSync(g.path)) throw new Error(`Not a .gguf file: ${g.path}`);
    const id = String(g.id ?? '').trim();
    const label = String(g.name ?? '').trim();
    args.push('--gguf', id || label ? `${g.path}=${id}=${label}` : g.path);
  }
  for (const c of b.catalog ?? []) args.push('--catalog', String(c));
  for (const k of b.knowledge ?? []) {
    if (typeof k !== 'string' || !fs.existsSync(k) || !fs.statSync(k).isDirectory()) throw new Error(`Not a folder: ${k}`);
    args.push('--knowledge', k);
  }
  if (b.default) args.push('--default', String(b.default));
  if (!['mac', 'windows', 'windows-arm64', 'all'].includes(b.target)) throw new Error('Choose a target.');
  args.push('--target', b.target);
  if (['auto', 'installer', 'portable'].includes(b.format)) args.push('--format', b.format);
  return args;
}

export function startServer({ port = DEFAULT_PORT, open = true } = {}) {
  const token = crypto.randomBytes(16).toString('hex');
  let last = Date.now();
  let job = null; // { child, log: string[], done: boolean, code: number|null }
  const clients = new Set();
  const push = (line) => {
    if (!job) return;
    job.log.push(line);
    for (const c of clients) c.write(`data: ${JSON.stringify(line)}\n\n`);
  };

  const json = (res, code, body) => {
    res.writeHead(code, { 'content-type': 'application/json' });
    res.end(JSON.stringify(body));
  };

  const server = http.createServer((req, res) => {
    last = Date.now();
    const url = new URL(req.url, 'http://127.0.0.1');
    // Only our own origin may talk to us (DNS-rebinding / cross-site protection).
    const host = req.headers.host ?? '';
    if (!/^(127\.0\.0\.1|localhost)(:\d+)?$/.test(host)) return json(res, 403, { error: 'forbidden' });
    if (url.pathname === '/') {
      if (url.searchParams.get('t') !== token) return json(res, 403, { error: 'open the URL printed in the terminal' });
      res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
      return res.end(fs.readFileSync(path.join(HERE, 'app-builder', 'index.html')));
    }
    if (url.searchParams.get('t') !== token && req.headers['x-token'] !== token) return json(res, 403, { error: 'bad token' });

    try {
      if (url.pathname === '/api/info') {
        const catalog = JSON.parse(fs.readFileSync(path.join(ROOT, 'catalog', 'models.json'), 'utf8')).models.map((m) => ({
          id: m.id,
          name: m.name,
          quant: m.quant,
          params_b: m.params_b,
        }));
        const downloads = path.join(os.homedir(), 'Downloads');
        return json(res, 200, {
          platform: process.platform,
          home: os.homedir(),
          downloads,
          downloadsGguf: ggufsIn(downloads),
          catalog,
          running: !!job && !job.done,
        });
      }
      if (url.pathname === '/api/state') {
        if (req.method !== 'PUT') return json(res, 200, readState());
        let body = '';
        req.on('data', (c) => body.length < 200_000 && (body += c));
        req.on('end', () => {
          try {
            writeState(JSON.parse(body || 'null'));
            json(res, 200, { ok: true });
          } catch (e) {
            json(res, 400, { error: e.message });
          }
        });
        return;
      }
      if (url.pathname === '/api/ls') return json(res, 200, listDir(url.searchParams.get('dir') || os.homedir()));
      if (url.pathname === '/api/log') {
        res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
        for (const l of job?.log ?? []) res.write(`data: ${JSON.stringify(l)}\n\n`);
        if (job?.done) res.write(`event: done\ndata: ${job.code}\n\n`);
        clients.add(res);
        req.on('close', () => clients.delete(res));
        return;
      }
      if (url.pathname === '/api/build' && req.method === 'POST') {
        if (job && !job.done) return json(res, 409, { error: 'A build is already running.' });
        let body = '';
        req.on('data', (c) => (body += c));
        req.on('end', () => {
          let args;
          try {
            args = buildArgs(JSON.parse(body || '{}'));
          } catch (e) {
            return json(res, 400, { error: e.message });
          }
          const child = spawn(process.execPath, args, { cwd: ROOT, env: { ...process.env, FORCE_COLOR: '0' } });
          job = { child, log: [], done: false, code: null };
          push(`$ node scripts/bundle.mjs ${args.slice(1).join(' ')}`);
          const feed = (d) => d.toString().split(/\r?\n/).filter(Boolean).forEach(push);
          child.stdout.on('data', feed);
          child.stderr.on('data', feed);
          child.on('close', (code) => {
            job.done = true;
            job.code = code;
            for (const c of clients) c.write(`event: done\ndata: ${code}\n\n`);
          });
          json(res, 200, { started: true });
        });
        return;
      }
      if (url.pathname === '/api/cancel' && req.method === 'POST') {
        if (job && !job.done) job.child.kill();
        return json(res, 200, { ok: true });
      }
      return json(res, 404, { error: 'not found' });
    } catch (e) {
      return json(res, 500, { error: e.message });
    }
  });

  const idle = setInterval(() => {
    if ((!job || job.done) && Date.now() - last > IDLE_MS) {
      console.log('Idle for 30 minutes: exiting.');
      process.exit(0);
    }
  }, 60_000);
  idle.unref();

  // A fixed port keeps the page's origin (and its remembered choices) the same between runs;
  // if it is taken, use any free port (the server-side state file still restores the choices).
  return new Promise((resolve) => {
    const onListening = () => {
      const url = `http://127.0.0.1:${server.address().port}/?t=${token}`;
      console.log(`NanoNexus App Builder: ${url}\n(Ctrl-C to stop)`);
      if (open) {
        const cmd = process.platform === 'darwin' ? ['open', [url]] : process.platform === 'win32' ? ['cmd', ['/c', 'start', '', url]] : ['xdg-open', [url]];
        spawn(cmd[0], cmd[1], { stdio: 'ignore', detached: true }).on('error', () => {}).unref();
      }
      resolve({ server, url, token });
    };
    server.once('error', (e) => {
      if (e.code === 'EADDRINUSE' && port !== 0) server.listen(0, '127.0.0.1', onListening);
      else throw e;
    });
    server.listen(port, '127.0.0.1', onListening);
  });
}

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  const a = process.argv.slice(2);
  const pi = a.indexOf('--port');
  startServer({ port: pi >= 0 ? Number(a[pi + 1]) : DEFAULT_PORT, open: !a.includes('--no-open') });
}
