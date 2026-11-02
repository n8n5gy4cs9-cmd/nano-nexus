#!/usr/bin/env node
// Task-owned build workspace. Source tool only: invoking `run` still needs owner build approval.
import fs from 'node:fs';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const base = path.join(repo, '.portable-build');
const marker = '.nexus-build-scratch.json';
const active = '.active.json';
const GiB = 1024 ** 3;

function directory(p) {
  const stat = fs.lstatSync(p);
  if (!stat.isDirectory() || stat.isSymbolicLink()) throw new Error('Workspace directory is missing or linked');
  return fs.realpathSync(p);
}

function location(id) {
  if (!/^[a-zA-Z0-9_-]{1,64}$/.test(id ?? '')) throw new Error('Use an ID containing 1–64 letters, digits, underscores or hyphens');
  const realRepo = directory(repo);
  const realBase = directory(base);
  if (path.dirname(realBase) !== realRepo) throw new Error('Workspace base escapes the repository');
  return path.join(realBase, id);
}

function integer(value, fallback, min, max) {
  const number = value === undefined ? fallback : Number(value);
  if (!Number.isSafeInteger(number) || number < min || number > max) throw new Error(`Expected a whole number between ${min} and ${max}`);
  return number;
}

function space(p) {
  const stat = fs.statfsSync(p, { bigint: true });
  return stat.bavail * stat.bsize;
}

function writeNew(p, value) {
  const fd = fs.openSync(p, 'wx', 0o600);
  try {
    fs.writeFileSync(fd, `${JSON.stringify(value, null, 2)}\n`);
    fs.fsyncSync(fd);
  } finally { fs.closeSync(fd); }
}

function readSmall(p) {
  const stat = fs.lstatSync(p);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 8192) throw new Error('Unsafe workspace metadata');
  return JSON.parse(fs.readFileSync(p, 'utf8'));
}

function owned(id) {
  const root = location(id);
  if (directory(root) !== root) throw new Error('Workspace root is linked');
  const info = readSmall(path.join(root, marker));
  if (info.schema !== 1 || info.owner !== 'nanonexus-portable-build' || info.id !== id || info.root !== root
      || typeof info.token !== 'string' || !/^[0-9a-f-]{36}$/.test(info.token)
      || !Number.isSafeInteger(info.budgetBytes) || info.budgetBytes < GiB || info.budgetBytes > 512 * GiB
      || !Number.isSafeInteger(info.reserveBytes) || info.reserveBytes < GiB || info.reserveBytes > 128 * GiB) {
    throw new Error('Workspace ownership or budget metadata is invalid');
  }
  return { root, info };
}

function bytes(p) {
  let total = 0;
  const pending = [p];
  let entries = 0;
  while (pending.length) {
    if (++entries > 2_000_000) throw new Error("Scratch entry count exceeds bounded monitoring capacity");
    const item = pending.pop();
    let stat;
    try { stat = fs.lstatSync(item); } catch (error) {
      if (error.code === 'ENOENT') continue; // compiler temp file disappeared during sampling
      throw error;
    }
    if (stat.isSymbolicLink()) throw new Error('Linked paths are outside scratch monitoring coverage');
    if (stat.isDirectory()) {
      for (const name of fs.readdirSync(item)) pending.push(path.join(item, name));
    } else if (stat.isFile()) {
      total += stat.size;
      if (!Number.isSafeInteger(total)) throw new Error('Workspace size exceeds supported accounting');
    } else throw new Error('Special files are outside scratch monitoring coverage');
  }
  return total;
}

function alive(pid) {
  try { process.kill(pid, 0); return true; } catch (error) {
    if (error.code === 'ESRCH') return false;
    throw error; // no permission to prove absence: fail closed
  }
}

function signalGroup(pid, signal) {
  try { process.kill(-pid, signal); } catch (error) { if (error.code !== 'ESRCH') throw error; }
}

function prepare(id, options) {
  if (!fs.existsSync(base)) fs.mkdirSync(base);
  const root = location(id);
  const budgetBytes = integer(options['budget-gib'], 32, 1, 512) * GiB;
  const reserveBytes = integer(options['reserve-gib'], 4, 1, 128) * GiB;
  if (space(base) < BigInt(budgetBytes + reserveBytes)) throw new Error('Insufficient free space for the requested scratch budget plus reserve');
  fs.mkdirSync(root); // never adopt or clear an existing directory
  const info = { schema: 1, owner: 'nanonexus-portable-build', id, root, token: randomUUID(), budgetBytes, reserveBytes, createdAt: new Date().toISOString() };
  writeNew(path.join(root, marker), info);
  for (const name of ['target', 'cargo', 'temp', 'cache', 'npm', 'pnpm', 'xwin', 'outputs']) fs.mkdirSync(path.join(root, name));
  process.stdout.write(`Prepared ${root}\nBudget ${budgetBytes / GiB} GiB; free-space reserve ${reserveBytes / GiB} GiB. No build started.\n`);
}

async function run(id, command) {
  if (process.platform === 'win32') throw new Error('Run builds from the Mac; Windows process-group ownership is not implemented by this wrapper');
  if (!command.length) throw new Error('Specify the approved build command after --');
  const { root, info } = owned(id);
  if (fs.existsSync(path.join(root, active))) throw new Error('Workspace is active or needs explicit recover');
  const used = bytes(root);
  if (used >= info.budgetBytes || space(root) < BigInt(info.reserveBytes)) throw new Error('Scratch budget or free-space reserve is exhausted');
  // No shell interpolation. These are task-local variables, never replacements for HOME/CODEX_HOME.
  const buildEnv = {
    ...process.env,
    CARGO_TARGET_DIR: path.join(root, 'target'), CARGO_HOME: path.join(root, 'cargo'),
    TMPDIR: path.join(root, 'temp'), TMP: path.join(root, 'temp'), TEMP: path.join(root, 'temp'),
    XDG_CACHE_HOME: path.join(root, 'cache'), npm_config_cache: path.join(root, 'npm'),
    COREPACK_HOME: path.join(root, 'cache', 'corepack'),
    npm_config_store_dir: path.join(root, 'pnpm'), PNPM_HOME: path.join(root, 'pnpm'),
    XWIN_CACHE_DIR: path.join(root, 'xwin'), NEXUS_BUILD_OUTPUT_DIR: path.join(root, 'outputs'),
  };
  // Reserve active ownership before spawning; a crash here is explicitly recoverable.
  writeNew(path.join(root, active), { token: info.token, parent: process.pid, child: null });
  const child = spawn(command[0], command.slice(1), { cwd: repo, env: buildEnv, stdio: 'inherit', detached: true, shell: false });
  let failed = false;
  let killTimer;
  const stop = (reason) => {
    if (failed) return;
    failed = true;
    process.stderr.write(`${reason}; stopping the owned build process group.\n`);
    if (child.pid) {
      try { signalGroup(child.pid, 'SIGTERM'); } catch { process.stderr.write('Could not terminate the build group; workspace must be retained.\n'); }
      killTimer = setTimeout(() => {
        try { signalGroup(child.pid, 'SIGKILL'); } catch { process.stderr.write('Could not kill the build group; workspace must be retained.\n'); }
      }, 2000);
    }
  };
  const interrupt = () => stop('Build interrupted');
  process.on('SIGINT', interrupt);
  process.on('SIGTERM', interrupt);
  const monitor = setInterval(() => {
    try {
      if (bytes(root) > info.budgetBytes) stop('Scratch budget exceeded');
      if (space(root) < BigInt(info.reserveBytes)) stop('Free-space reserve reached');
    } catch { stop('Scratch accounting became unavailable'); }
  }, 1000);
  try {
    const code = await new Promise((resolve, reject) => {
      child.once('error', reject);
      child.once('spawn', () => {
        // The parent owns this file exclusively; cleanup refuses it while present.
        try {
          fs.writeFileSync(path.join(root, active), JSON.stringify({ token: info.token, parent: process.pid, child: child.pid }));
        } catch (error) { stop('Build ownership metadata could not be written'); reject(error); }
      });
      child.once('close', (code) => resolve(code ?? 1));
    });
    // A compiler's descendants must not keep writing after its direct child exits.
    if (child.pid) {
      signalGroup(child.pid, 'SIGTERM');
      await new Promise((resolve) => setTimeout(resolve, 2000));
      signalGroup(child.pid, 'SIGKILL');
      if (alive(-child.pid)) throw new Error('Build descendants are still active; retain workspace for recovery');
    }
    fs.unlinkSync(path.join(root, active));
    process.exitCode = failed || code !== 0 ? 1 : 0;
    process.stdout.write(`Build stopped. Workspace retained at ${root}; copy needed outputs before cleanup.\n`);
  } catch (error) {
    // Retain active ownership on any uncertain exit; clean must never race an orphan.
    if (child.pid) {
      try { signalGroup(child.pid, 'SIGKILL'); } catch { /* retain marker for manual recovery */ }
    }
    throw error;
  } finally {
    clearInterval(monitor);
    clearTimeout(killTimer);
    process.off('SIGINT', interrupt);
    process.off('SIGTERM', interrupt);
  }
}

function recover(id) {
  const { root, info } = owned(id);
  const running = readSmall(path.join(root, active));
  if (running.token !== info.token || !Number.isSafeInteger(running.parent) || running.parent <= 0
      || (running.child !== null && (!Number.isSafeInteger(running.child) || running.child <= 0))) throw new Error('Invalid active ownership metadata');
  if (running.child === null) throw new Error('Spawn state is unknown. Manual process inspection is required; automatic recovery is refused');
  if (alive(running.parent) || alive(-running.child)) throw new Error('Build owner or process group still exists; recovery refused');
  fs.unlinkSync(path.join(root, active));
  process.stdout.write('Stale active marker removed. Files retained.\n');
}

async function main() {
  const [action, ...args] = process.argv.slice(2);
  const options = {};
  let command = [];
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--') { command = args.slice(i + 1); break; }
    const name = args[i];
    if (!['--id', '--budget-gib', '--reserve-gib'].includes(name) || i + 1 >= args.length) throw new Error('Unknown or incomplete option');
    const key = name.slice(2);
    if (Object.hasOwn(options, key)) throw new Error('Duplicate option');
    options[key] = args[++i];
  }
  if (action !== 'prepare' && (options['budget-gib'] !== undefined || options['reserve-gib'] !== undefined)) throw new Error('Budgets are set only by prepare');
  if (action !== 'run' && command.length) throw new Error('A command is accepted only by run');
  if (action === 'prepare') prepare(options.id, options);
  else if (action === 'run') await run(options.id, command);
  else if (action === 'recover') recover(options.id);
  else if (action === 'status') {
    const { root, info } = owned(options.id);
    process.stdout.write(`${JSON.stringify({ root, usedBytes: bytes(root), budgetBytes: info.budgetBytes, freeBytes: String(space(root)), reserveBytes: info.reserveBytes, active: fs.existsSync(path.join(root, active)) }, null, 2)}\n`);
  } else if (action === 'clean') {
    const { root } = owned(options.id);
    if (fs.existsSync(path.join(root, active))) throw new Error('Active workspace cannot be cleaned; recover stale ownership explicitly first');
    // Atomic reservation closes the race with a simultaneous run. An uncertain cleanup crash
    // deliberately leaves active ownership behind for manual inspection.
    writeNew(path.join(root, active), { token: readSmall(path.join(root, marker)).token, parent: process.pid, child: null });
    bytes(root); // refuse linked/special-file trees; never follow paths outside this workspace
    fs.rmSync(root, { recursive: true });
    process.stdout.write(`Removed only owned workspace ${root}\n`);
  } else throw new Error('Use prepare|run|status|recover|clean --id ID [prepare: --budget-gib N --reserve-gib N] [run: -- COMMAND ARGS]');
}

main().catch((error) => { process.stderr.write(`portable-build-scratch: ${error.message}\n`); process.exitCode = 1; });
