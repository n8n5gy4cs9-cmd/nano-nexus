// A small JSON Schema checker for NanoNexus's own schemas (no dependencies, Node 22+).
// Supports what schemas/*.json use: type, const, enum, required, properties, additionalProperties,
// items, minItems, uniqueItems, minLength, pattern, minimum, maximum, oneOf, anyOf, and $ref
// (`#/...` inside a file and `other.schema.json#/...` next to it).
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SCHEMAS = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'schemas');
const cache = new Map();

/** Load `schemas/<file>`. */
export function loadSchema(file) {
  if (!cache.has(file)) cache.set(file, JSON.parse(fs.readFileSync(path.join(SCHEMAS, file), 'utf8')));
  return cache.get(file);
}

const typeOf = (v) => (v === null ? 'null' : Array.isArray(v) ? 'array' : Number.isInteger(v) ? 'integer' : typeof v);
const isType = (v, t) => (t === 'number' ? typeof v === 'number' : t === 'integer' ? Number.isInteger(v) : typeOf(v) === t);
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

function resolveRef(ref, ctx) {
  const [file, pointer = ''] = ref.split('#');
  const root = file ? loadSchema(file) : ctx.root;
  let node = root;
  for (const part of pointer.split('/').filter(Boolean)) node = node?.[part.replace(/~1/g, '/').replace(/~0/g, '~')];
  if (node === undefined) throw new Error(`unresolved $ref ${ref}`);
  return { node, root };
}

function check(value, schema, at, ctx, errors) {
  if (schema.$ref) {
    const { node, root } = resolveRef(schema.$ref, ctx);
    return check(value, node, at, { root }, errors);
  }
  if ('const' in schema && !same(value, schema.const)) errors.push(`${at}: must be ${JSON.stringify(schema.const)}`);
  if (schema.enum && !schema.enum.some((e) => same(e, value))) errors.push(`${at}: must be one of ${schema.enum.join(', ')}`);
  if (schema.type) {
    const types = [].concat(schema.type);
    if (!types.some((t) => isType(value, t))) return void errors.push(`${at}: must be ${types.join(' or ')}, got ${typeOf(value)}`);
  }
  if (schema.oneOf || schema.anyOf) {
    const alts = (schema.oneOf ?? schema.anyOf).map((s) => {
      const e = [];
      check(value, s, at, ctx, e);
      return e;
    });
    const matches = alts.filter((e) => e.length === 0).length;
    if (matches === 0) errors.push(`${at}: matches none of the allowed forms (${alts.map((e) => e[0]).join(' | ')})`);
    else if (schema.oneOf && matches > 1) errors.push(`${at}: matches more than one form`);
  }
  if (typeof value === 'string') {
    if (schema.minLength !== undefined && value.length < schema.minLength) errors.push(`${at}: too short`);
    if (schema.pattern && !new RegExp(schema.pattern).test(value)) errors.push(`${at}: "${value}" does not match ${schema.pattern}`);
  }
  if (typeof value === 'number') {
    if (schema.minimum !== undefined && value < schema.minimum) errors.push(`${at}: must be >= ${schema.minimum}`);
    if (schema.maximum !== undefined && value > schema.maximum) errors.push(`${at}: must be <= ${schema.maximum}`);
  }
  if (Array.isArray(value)) {
    if (schema.minItems !== undefined && value.length < schema.minItems) errors.push(`${at}: needs at least ${schema.minItems} item(s)`);
    if (schema.uniqueItems && new Set(value.map((v) => JSON.stringify(v))).size !== value.length) errors.push(`${at}: items must be unique`);
    if (schema.items) value.forEach((v, i) => check(v, schema.items, `${at}[${i}]`, ctx, errors));
  }
  if (typeOf(value) === 'object') {
    for (const key of schema.required ?? []) if (!(key in value)) errors.push(`${at}: missing "${key}"`);
    const props = schema.properties ?? {};
    for (const [key, v] of Object.entries(value)) {
      if (props[key]) check(v, props[key], `${at}.${key}`, ctx, errors);
      else if (schema.additionalProperties === false) errors.push(`${at}: unknown property "${key}"`);
      else if (typeof schema.additionalProperties === 'object') check(v, schema.additionalProperties, `${at}.${key}`, ctx, errors);
    }
  }
}

/** Validate `value` against `schema` (an object, or a file name in schemas/). Returns a list of problems (empty = valid). */
export function validate(value, schema, at = '$') {
  const root = typeof schema === 'string' ? loadSchema(schema) : schema;
  const errors = [];
  check(value, root, at, { root }, errors);
  return errors;
}
