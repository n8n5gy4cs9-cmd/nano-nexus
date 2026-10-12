#!/usr/bin/env node
// eval-assistants.mjs: run the golden set (tests/golden/*.json) against a model and print a pass table (T15.07).
//   node scripts/eval-assistants.mjs [--endpoint http://127.0.0.1:8080] [--assistant tldr] [--no-fail] [--json out.json]
// Without --endpoint it starts llama-server from vendor/ with the default model (fetch-model.mjs --print-default-path).
// Covers the chat assistants. Pipeline assistants (Folder Scout, Rename Forge, ...) are tested by their pipelines.
// A run passes when every assistant reaches 80% of its cases (M15 exit check). Node 22+, no dependencies.
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const GOLDEN = path.join(ROOT, 'tests', 'golden');
export const THRESHOLD = 0.8;

const words = (s) => (s.trim() ? s.trim().split(/\s+/).length : 0);

/** Remove thinking blocks the server did not split off. */
export function clean(output) {
  return output.replace(/<think>[\s\S]*?<\/think>/g, '').replace(/<think>[\s\S]*$/, '').trim();
}

/** Check one answer against a case's `checks`. Returns the list of failed checks (empty = pass). */
export function evaluate(raw, checks = {}) {
  const out = clean(raw);
  const lower = out.toLowerCase();
  const fails = [];
  if (out === '') fails.push('empty answer');
  if (/<\/?(think|tool_result|file|doc|facts)\b/i.test(out)) fails.push('copied an internal tag into the answer');
  if (checks.maxWords !== undefined && words(out) > checks.maxWords) fails.push(`${words(out)} words (max ${checks.maxWords})`);
  if (checks.minWords !== undefined && words(out) < checks.minWords) fails.push(`${words(out)} words (min ${checks.minWords})`);
  if (checks.containsAny && !checks.containsAny.some((s) => lower.includes(s.toLowerCase()))) fails.push(`none of: ${checks.containsAny.join(' | ')}`);
  for (const s of checks.containsAll ?? []) if (!lower.includes(s.toLowerCase())) fails.push(`missing "${s}"`);
  for (const s of checks.notContains ?? []) if (lower.includes(s.toLowerCase())) fails.push(`must not contain "${s}"`);
  for (const r of checks.matches ?? []) if (!new RegExp(r, 'i').test(out)) fails.push(`no match for /${r}/`);
  if (checks.json) {
    const body = out.replace(/^```(?:json)?\s*/i, '').replace(/```\s*$/, '');
    try {
      JSON.parse(body);
    } catch {
      fails.push('not valid JSON');
    }
  }
  return fails;
}

/** Pass table rows from results `[{ assistant, case, fails }]`. */
export function summarize(results) {
  const by = new Map();
  for (const r of results) {
    const row = by.get(r.assistant) ?? { assistant: r.assistant, passed: 0, total: 0, failures: [] };
    row.total += 1;
    if (r.fails.length === 0) row.passed += 1;
    else row.failures.push(`${r.case}: ${r.fails.join('; ')}`);
    by.set(r.assistant, row);
  }
  return [...by.values()].map((r) => ({ ...r, rate: r.passed / r.total, ok: r.passed / r.total >= THRESHOLD }));
}

export function loadGolden(only) {
  return fs
    .readdirSync(GOLDEN)
    .filter((f) => f.endsWith('.json'))
    .map((f) => JSON.parse(fs.readFileSync(path.join(GOLDEN, f), 'utf8')))
    .filter((g) => !only || g.assistant === only);
}

function renderPrompt(id) {
  const r = spawnSync('cargo', ['run', '-q', '-p', 'nexus-agent', '--example', 'render_prompt', '--', id], { cwd: ROOT, encoding: 'utf8' });
  if (r.status !== 0) throw new Error(`could not render the prompt of ${id}: ${r.stderr}`);
  return JSON.parse(r.stdout);
}

async function ask(endpoint, prompt, input) {
  const m = prompt.model;
  const res = await fetch(`${endpoint}/v1/chat/completions`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      messages: [
        { role: 'system', content: prompt.system },
        { role: 'user', content: input },
      ],
      temperature: m.temperature,
      top_p: m.top_p,
      min_p: m.min_p,
      top_k: m.top_k,
      repeat_penalty: m.repeat_penalty,
      max_tokens: m.max_tokens,
      stream: false,
      chat_template_kwargs: { enable_thinking: m.thinking },
    }),
  });
  if (!res.ok) throw new Error(`the model server answered ${res.status}`);
  const j = await res.json();
  return j.choices?.[0]?.message?.content ?? '';
}

const freePort = () =>
  new Promise((resolve) => {
    const s = net.createServer().listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });

async function startServer() {
  const model = spawnSync('node', [path.join(ROOT, 'scripts', 'fetch-model.mjs'), '--print-default-path'], { encoding: 'utf8' }).stdout.trim();
  if (!model) throw new Error('no default model: run node scripts/fetch-model.mjs first, or pass --endpoint');
  const dir = path.join(ROOT, 'vendor', 'runtime');
  const bin = fs.existsSync(path.join(dir, 'macos-arm64')) ? path.join(dir, 'macos-arm64', 'llama-server-aarch64-apple-darwin') : path.join(dir, 'windows-x64', 'llama-server.exe');
  const port = await freePort();
  const child = spawn(bin, ['-m', model, '--host', '127.0.0.1', '--port', String(port), '--ctx-size', '8192', '--jinja', '--reasoning-format', 'deepseek', '--no-webui'], { stdio: 'ignore' });
  const endpoint = `http://127.0.0.1:${port}`;
  for (let i = 0; i < 120; i++) {
    try {
      if ((await fetch(`${endpoint}/health`)).ok) return { endpoint, stop: () => child.kill() };
    } catch {
      /* not up yet */
    }
    await new Promise((r) => setTimeout(r, 1000));
  }
  child.kill();
  throw new Error('the model server did not become ready in 2 minutes');
}

async function main() {
  const a = process.argv.slice(2);
  const opt = (name) => (a.includes(name) ? a[a.indexOf(name) + 1] : undefined);
  const golden = loadGolden(opt('--assistant'));
  if (golden.length === 0) throw new Error('no golden cases found');
  const server = opt('--endpoint') ? { endpoint: opt('--endpoint'), stop: () => {} } : await startServer();
  const results = [];
  try {
    for (const g of golden) {
      const prompt = renderPrompt(g.assistant);
      for (const c of g.cases) {
        let fails;
        try {
          fails = evaluate(await ask(server.endpoint, prompt, c.input), c.checks);
        } catch (e) {
          fails = [`error: ${e.message}`];
        }
        results.push({ assistant: g.assistant, case: c.name, fails });
        process.stderr.write(fails.length ? '✖' : '✔');
      }
    }
  } finally {
    server.stop();
  }
  process.stderr.write('\n\n');
  const rows = summarize(results);
  console.log('| assistant | passed | rate | |');
  console.log('|---|---|---|---|');
  for (const r of rows) console.log(`| ${r.assistant} | ${r.passed}/${r.total} | ${Math.round(r.rate * 100)}% | ${r.ok ? '✔' : '✖'} |`);
  for (const r of rows) for (const f of r.failures) console.log(`\n${r.assistant}: ${f}`);
  if (opt('--json')) fs.writeFileSync(opt('--json'), JSON.stringify({ rows, results }, null, 2));
  const bad = rows.filter((r) => !r.ok);
  console.log(bad.length ? `\n✖ below ${THRESHOLD * 100}%: ${bad.map((r) => r.assistant).join(', ')}` : `\n✔ every assistant reaches ${THRESHOLD * 100}%`);
  if (bad.length && !a.includes('--no-fail')) process.exit(1);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((e) => {
    console.error(`✖ ${e.message}`);
    process.exit(2);
  });
}
