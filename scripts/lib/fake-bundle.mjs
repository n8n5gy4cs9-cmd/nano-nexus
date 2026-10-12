import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';


/** Test helper: a throwaway repo root with fake 10 MB "models" in vendor/models. */
export const MB10 = 10 * 1024 * 1024;

export function fakeRoot(ids = ['alpha', 'beta']) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'stage-'));
  const modelsDir = path.join(root, 'vendor', 'models');
  for (const [i, id] of ids.entries()) {
    const data = Buffer.alloc(MB10, i + 1);
    data.write('GGUF');
    const file = `${id}-Q8_0.gguf`;
    fs.mkdirSync(path.join(modelsDir, id), { recursive: true });
    fs.writeFileSync(path.join(modelsDir, id, file), data);
    const lock = { schema: 1, id, name: `Model ${id}`, file, sha256: crypto.createHash('sha256').update(data).digest('hex'), bytes: data.length, license: 'MIT', catalog_id: null };
    fs.writeFileSync(path.join(modelsDir, id, 'lock.json'), JSON.stringify(lock));
  }
  fs.mkdirSync(path.join(root, 'apps', 'desktop', 'src-tauri'), { recursive: true });
  const rt = path.join(root, 'vendor', 'runtime', 'macos-arm64');
  fs.mkdirSync(path.join(rt, 'licenses'), { recursive: true });
  fs.writeFileSync(path.join(rt, 'licenses', 'LICENSE-llama'), 'MIT');
  return { root, modelsDir, profile: { schema: 1, name: 'fake', models: ids, default: ids[0], targets: ['macos-arm64'] } };
}

