#!/usr/bin/env node
// Knowledge-library payload helper (M17 / BUNDLING §12). stage-bundle.mjs calls `stageKnowledge`;
// verify-payload.mjs calls `verifyKnowledge`. Pure Node, no dependencies.
//
//   node scripts/knowledge-manifest.mjs stage  <profile.json> <out-dir>   copy sources + write manifest.json
//   node scripts/knowledge-manifest.mjs verify <knowledge-dir>            re-hash everything against manifest.json
import { createHash } from "node:crypto";
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { basename, isAbsolute, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const SUPPORTED = new Set([
  "md", "markdown", "txt", "text", "rst", "log", "json", "csv", "tsv", "html", "htm", "xhtml", "pdf", "epub", "docx",
  "rs", "py", "js", "jsx", "ts", "tsx", "go", "java", "kt", "c", "h", "cpp", "hpp", "cs", "rb", "php", "swift", "sh",
  "lua", "sql", "toml", "yaml", "yml", "ini", "cfg", "css", "scss",
]);

export const slug = (s) =>
  s.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "library";

/** Normalize a profile's `knowledge[]` into `{ id, name, path }` (path absolute). */
export function resolveEntries(profile, root) {
  const out = [];
  const seen = new Set();
  for (const e of profile.knowledge ?? []) {
    const obj = typeof e === "string" ? { path: e } : e;
    const path = isAbsolute(obj.path) ? obj.path : resolve(root, obj.path);
    const id = obj.id ?? slug(basename(path));
    if (seen.has(id)) throw new Error(`duplicate knowledge library id \`${id}\``);
    seen.add(id);
    out.push({ id, name: obj.name ?? basename(path), path });
  }
  return out;
}

/** Supported files below `dir`, sorted, hidden entries skipped; `{ path (posix, relative), bytes }`. */
export function listFiles(dir) {
  const files = [];
  const walk = (d) => {
    for (const name of readdirSync(d).sort()) {
      if (name.startsWith(".")) continue;
      const full = join(d, name);
      const st = statSync(full);
      if (st.isDirectory()) walk(full);
      else if (SUPPORTED.has(name.split(".").pop().toLowerCase())) {
        files.push({ path: relative(dir, full).split(sep).join("/"), bytes: st.size });
      }
    }
  };
  walk(dir);
  return files;
}

export const sha256 = (file) => createHash("sha256").update(readFileSync(file)).digest("hex");

/** Copy each library into `<outDir>/<id>/` and write `<outDir>/manifest.json`. Returns the manifest. */
export function stageKnowledge(profile, root, outDir) {
  const entries = resolveEntries(profile, root);
  if (entries.length === 0) return null;
  mkdirSync(outDir, { recursive: true });
  const libraries = [];
  for (const e of entries) {
    if (!existsSync(e.path) || !statSync(e.path).isDirectory()) throw new Error(`knowledge folder not found: ${e.path}`);
    const files = listFiles(e.path);
    if (files.length === 0) throw new Error(`knowledge library \`${e.id}\` has no supported files`);
    const dest = join(outDir, e.id);
    for (const f of files) {
      const to = join(dest, ...f.path.split("/"));
      mkdirSync(join(to, ".."), { recursive: true });
      cpSync(join(e.path, ...f.path.split("/")), to);
    }
    libraries.push({
      id: e.id,
      name: e.name,
      dir: e.id,
      bytes: files.reduce((n, f) => n + f.bytes, 0),
      files: files.map((f) => ({ ...f, sha256: sha256(join(dest, ...f.path.split("/"))) })),
    });
  }
  const manifest = { schema: 1, libraries };
  writeFileSync(join(outDir, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
  return manifest;
}

/** Re-hash a staged knowledge dir. Returns a list of problems (empty = OK). */
export function verifyKnowledge(dir) {
  const problems = [];
  const mpath = join(dir, "manifest.json");
  if (!existsSync(mpath)) return [`missing ${mpath}`];
  const manifest = JSON.parse(readFileSync(mpath, "utf8"));
  for (const lib of manifest.libraries ?? []) {
    if (!lib.dir || /[\\/]/.test(lib.dir) || lib.dir === "..") {
      problems.push(`library ${lib.id}: unsafe dir \`${lib.dir}\``);
      continue;
    }
    for (const f of lib.files) {
      const full = join(dir, lib.dir, ...f.path.split("/"));
      if (!existsSync(full)) problems.push(`${lib.id}/${f.path}: missing`);
      else if (sha256(full) !== f.sha256) problems.push(`${lib.id}/${f.path}: hash mismatch`);
    }
    const known = new Set(lib.files.map((f) => f.path));
    for (const f of listFiles(join(dir, lib.dir))) {
      if (!known.has(f.path)) problems.push(`${lib.id}/${f.path}: not in manifest`);
    }
  }
  return problems;
}

/** Size impact for the App Builder: `{ libraries, files, bytes }`. */
export function knowledgeSize(profile, root) {
  const entries = resolveEntries(profile, root).filter((e) => existsSync(e.path));
  const files = entries.flatMap((e) => listFiles(e.path));
  return { libraries: entries.length, files: files.length, bytes: files.reduce((n, f) => n + f.bytes, 0) };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  const [cmd, a, b] = process.argv.slice(2);
  const root = resolve(fileURLToPath(import.meta.url), "..", "..");
  if (cmd === "stage" && a && b) {
    const m = stageKnowledge(JSON.parse(readFileSync(a, "utf8")), root, resolve(b));
    console.log(m ? `staged ${m.libraries.length} knowledge librar${m.libraries.length === 1 ? "y" : "ies"}` : "no knowledge libraries in this profile");
  } else if (cmd === "verify" && a) {
    const problems = verifyKnowledge(resolve(a));
    problems.forEach((p) => console.error(p));
    process.exit(problems.length ? 1 : 0);
  } else {
    console.error("usage: knowledge-manifest.mjs stage <profile.json> <out-dir> | verify <knowledge-dir>");
    process.exit(2);
  }
}
