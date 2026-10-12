import assert from 'node:assert/strict';
import test from 'node:test';

import { check, parseOklch, ratio } from './contrast.mjs';

const c = (v) => parseOklch(v);

test('the maths: black on white is 21:1, equal colours 1:1, alpha is composited', () => {
  assert.ok(Math.abs(ratio(c('oklch(0 0 0)'), c('oklch(1 0 0)')) - 21) < 0.05);
  assert.ok(Math.abs(ratio(c('oklch(0.5 0 0)'), c('oklch(0.5 0 0)')) - 1) < 1e-6);
  // 50% black over white looks like mid grey: well below black-on-white, well above 1.
  const half = ratio(c('oklch(0 0 0 / 0.5)'), c('oklch(1 0 0)'));
  assert.ok(half > 3 && half < 8, String(half));
});

test('every design token pair meets its WCAG minimum in both themes and every accent', () => {
  const rows = check();
  assert.ok(rows.length > 60, `only ${rows.length} pairs were checked`);
  const bad = rows.filter((r) => !r.ok).map((r) => `${r.theme}/${r.accent} ${r.fg} on ${r.bg}: ${r.ratio.toFixed(2)} < ${r.min}`);
  assert.deepEqual(bad, []);
});
