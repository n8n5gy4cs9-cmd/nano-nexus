import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { clean, evaluate, GOLDEN, loadGolden, summarize } from './eval-assistants.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('thinking blocks are removed before checking', () => {
  assert.equal(clean('<think>hmm</think>Paris.'), 'Paris.');
  assert.equal(clean('<think>never closed'), '');
});

test('each kind of check passes and fails as described', () => {
  assert.deepEqual(evaluate('Paris is the capital.', { containsAll: ['paris'], maxWords: 10 }), []);
  assert.match(evaluate('Lyon.', { containsAll: ['paris'] })[0], /missing "paris"/);
  assert.match(evaluate('one two three', { maxWords: 2 })[0], /3 words/);
  assert.match(evaluate('hi', { minWords: 3 })[0], /min 3/);
  assert.match(evaluate('abc', { containsAny: ['x', 'y'] })[0], /none of/);
  assert.match(evaluate('abc', { notContains: ['b'] })[0], /must not contain/);
  assert.deepEqual(evaluate('- one\n- two', { matches: ['(^|\\n)\\s*[-*]\\s'] }), []);
  assert.deepEqual(evaluate('```json\n{"a":1}\n```', { json: true }), []);
  assert.match(evaluate('nope', { json: true })[0], /JSON/);
  assert.match(evaluate('', {})[0], /empty/);
  assert.match(evaluate('Answer <tool_result>x</tool_result>', {})[0], /internal tag/);
});

test('the pass table applies the 80% rule per assistant', () => {
  const r = (assistant, ok) => ({ assistant, case: 'c', fails: ok ? [] : ['x'] });
  const rows = summarize([r('a', true), r('a', true), r('a', true), r('a', true), r('a', false), r('b', true), r('b', false), r('b', false)]);
  assert.deepEqual(rows.map((x) => [x.assistant, x.passed, x.total, x.ok]), [['a', 4, 5, true], ['b', 1, 3, false]]);
});

test('the golden set covers the chat assistants with at least three cases each', () => {
  const golden = loadGolden();
  const ids = new Set(fs.readdirSync(path.join(ROOT, 'assistants')).map((f) => f.replace(/\.toml$/, '')));
  assert.ok(golden.length >= 10);
  for (const g of golden) {
    assert.ok(ids.has(g.assistant), `${g.assistant} is not an assistant`);
    assert.ok(g.cases.length >= 3, `${g.assistant} has ${g.cases.length} cases`);
    const names = g.cases.map((c) => c.name);
    assert.equal(new Set(names).size, names.length, `${g.assistant}: duplicate case names`);
    for (const c of g.cases) {
      assert.ok(c.input && c.checks, `${g.assistant}/${c.name}`);
      for (const k of Object.keys(c.checks)) assert.ok(['maxWords', 'minWords', 'containsAny', 'containsAll', 'notContains', 'matches', 'json'].includes(k), `unknown check ${k}`);
    }
  }
  assert.ok(fs.existsSync(GOLDEN));
});
