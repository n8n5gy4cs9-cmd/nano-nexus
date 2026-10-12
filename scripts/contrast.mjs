#!/usr/bin/env node
// contrast.mjs: WCAG contrast of the design-token pairs (UI-DESIGN §7, T15.05).
//   node scripts/contrast.mjs            print the table, exit 1 when a pair is below its minimum
// Reads apps/desktop/src/styles/tokens.css (OKLCH colours; alpha is composited over the page background).
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const TOKENS = path.join(HERE, '..', 'apps', 'desktop', 'src', 'styles', 'tokens.css');

/** "oklch(0.7 0.18 288 / 0.5)" -> linear sRGB + alpha. */
export function parseOklch(value) {
  const m = /oklch\(\s*([\d.]+)\s+([\d.]+)\s+([\d.]+)(?:\s*\/\s*([\d.]+))?\s*\)/.exec(value);
  if (!m) throw new Error(`not an oklch colour: ${value}`);
  const [L, C, h] = [Number(m[1]), Number(m[2]), (Number(m[3]) * Math.PI) / 180];
  const a = C * Math.cos(h);
  const b = C * Math.sin(h);
  const l_ = L + 0.3963377774 * a + 0.2158037573 * b;
  const m_ = L - 0.1055613458 * a - 0.0638541728 * b;
  const s_ = L - 0.0894841775 * a - 1.291485548 * b;
  const [l, mm, s] = [l_ ** 3, m_ ** 3, s_ ** 3];
  const rgb = [
    4.0767416621 * l - 3.3077115913 * mm + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * mm - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * mm + 1.707614701 * s,
  ].map((v) => Math.min(1, Math.max(0, v)));
  return { rgb, alpha: m[4] === undefined ? 1 : Number(m[4]) };
}

export const luminance = ({ rgb }) => 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];

const encode = (v) => (v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055);
const decode = (v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);

/** Composite `fg` (with alpha) over an opaque `bg`, the way a browser does: blending the gamma-encoded sRGB values. */
export const over = (fg, bg) => ({
  rgb: fg.rgb.map((c, i) => decode(encode(c) * fg.alpha + encode(bg.rgb[i]) * (1 - fg.alpha))),
  alpha: 1,
});

export function ratio(fg, bg) {
  const f = over(fg, bg);
  const [hi, lo] = [luminance(f), luminance(bg)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** Token values per theme/accent selector, as written (strings). */
export function readTokens(css = fs.readFileSync(TOKENS, 'utf8')) {
  const blocks = [];
  for (const m of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selector = m[1].replace(/\/\*[\s\S]*?\*\//g, '').trim();
    const vars = {};
    for (const v of m[2].matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) vars[v[1]] = v[2].trim();
    blocks.push({ selector, vars });
  }
  return blocks;
}

/** The colours of a theme (+ optional accent), with later blocks overriding earlier ones. */
export function themeColours(blocks, theme, accent) {
  const vars = {};
  const applies = (sel) =>
    sel.split(',').some((s) => {
      s = s.trim();
      if (s === ':root') return true;
      if (s === `:root[data-theme="${theme}"]`) return true;
      return accent !== undefined && s === `:root[data-theme="${theme}"][data-accent="${accent}"]`;
    });
  // Dark is the default theme: `:root` carries dark; the light block overrides it.
  for (const b of blocks) if (applies(b.selector)) Object.assign(vars, b.vars);
  const resolve = (name, depth = 0) => {
    const raw = vars[name];
    if (raw === undefined || depth > 5) return undefined;
    const ref = /^var\((--[\w-]+)\)$/.exec(raw);
    return ref ? resolve(ref[1], depth + 1) : raw;
  };
  const out = {};
  for (const name of Object.keys(vars)) {
    const raw = resolve(name);
    if (raw?.startsWith('oklch(')) out[name.slice(2)] = parseOklch(raw);
  }
  return out;
}

/** [foreground, background, minimum ratio, why] */
export const PAIRS = [
  ['text', 'bg', 4.5, 'body text'],
  ['text', 'surface', 4.5, 'text on cards'],
  ['text', 'surface-2', 4.5, 'text on raised areas'],
  ['text', 'surface-3', 4.5, 'text on selected rows'],
  ['text-muted', 'bg', 4.5, 'secondary text'],
  ['text-muted', 'surface', 4.5, 'secondary text on cards'],
  ['text-muted', 'surface-2', 4.5, 'secondary text on raised areas'],
  ['text-muted', 'surface-3', 4.5, 'secondary text on selected rows'],
  ['text-faint', 'bg', 3, 'placeholders and hints (large or supplementary)'],
  ['text-faint', 'surface', 3, 'hints on cards'],
  ['accent-fg', 'accent', 4.5, 'primary button label'],
  ['accent-fg', 'accent-2', 3, 'primary button label at the gradient end (bold, 14px+)'],
  ['accent-fg', 'danger', 4.5, 'danger button label'],
  ['warning', 'surface-2', 4.5, 'connection banner text'],
  ['success', 'surface', 4.5, 'success text'],
  ['warning', 'surface', 4.5, 'warning text'],
  ['danger', 'surface', 4.5, 'error text'],
];
/** Accent used as text or a control outline (links, focus): 3:1 is the WCAG minimum for UI components. */
export const ACCENT_PAIRS = [['accent', 'bg', 3, 'links and focus colour']];

export function check() {
  const blocks = readTokens();
  const rows = [];
  const accents = [undefined, 'cyan', 'emerald', 'amber', 'rose', 'graphite'];
  for (const theme of ['dark', 'light']) {
    for (const accent of accents) {
      const c = themeColours(blocks, theme, accent);
      const pairs = accent === undefined ? [...PAIRS, ...ACCENT_PAIRS] : [['accent-fg', 'accent', 4.5, 'primary button label'], ['accent-fg', 'accent-2', 3, 'primary button label at the gradient end (bold, 14px+)'], ...ACCENT_PAIRS];
      for (const [fg, bg, min, why] of pairs) {
        if (!c[fg] || !c[bg]) throw new Error(`missing token ${fg} or ${bg} in ${theme}`);
        const r = ratio(c[fg], c[bg]);
        rows.push({ theme, accent: accent ?? 'violet', fg, bg, min, why, ratio: r, ok: r >= min });
      }
    }
  }
  return rows;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const rows = check();
  for (const r of rows) console.log(`${r.ok ? '✔' : '✖'} ${r.theme.padEnd(5)} ${r.accent.padEnd(8)} ${r.fg} on ${r.bg}: ${r.ratio.toFixed(2)} (min ${r.min}) ${r.why}`);
  const bad = rows.filter((r) => !r.ok);
  console.log(`\n${bad.length ? `✖ ${bad.length} pair(s) below the minimum` : '✔ every token pair meets its minimum'}`);
  process.exit(bad.length ? 1 : 0);
}
