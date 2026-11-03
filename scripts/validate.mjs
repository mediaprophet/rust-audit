/**
 * Validate emitted .ttl files by parsing them with the actual QualiaDB engine
 * (WASM build: tools/qualia_core_db.js + qualia_core_db_bg.wasm).
 *
 * Usage: node scripts/validate.mjs [out_dir]
 *   out_dir defaults to tests/fixtures/mini-repo/.audit
 *
 * Get the WASM assets from:
 *   https://github.com/mediaprophet/qualiaDB/releases  (qualia_core_db*.tar.gz
 *   or the loose qualia_core_db.js / qualia_core_db_bg.wasm assets)
 */

import { readFileSync, readdirSync, existsSync } from 'fs';
import { join } from 'path';
import { fileURLToPath, pathToFileURL } from 'url';

const here = fileURLToPath(new URL('.', import.meta.url));
const tools = join(here, '..', 'tools');
const jsPath = join(tools, 'qualia_core_db.js');
const wasmPath = join(tools, 'qualia_core_db_bg.wasm');

if (!existsSync(jsPath) || !existsSync(wasmPath)) {
  console.error('WASM build missing — download qualia_core_db.js + qualia_core_db_bg.wasm');
  console.error('into tools/ from https://github.com/mediaprophet/qualiaDB/releases');
  process.exit(2);
}

const mod = await import(pathToFileURL(jsPath).href);
await mod.default({ module_or_path: readFileSync(wasmPath) });

const outDir = process.argv[2] ?? 'tests/fixtures/mini-repo/.audit';

function* ttlFiles(dir) {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) yield* ttlFiles(p);
    else if (e.name.endsWith('.ttl')) yield p;
  }
}

let total = 0, failures = 0, files = 0;
for (const f of ttlFiles(outDir)) {
  files++;
  try {
    const triples = mod.parse_turtle_wasm(readFileSync(f, 'utf8'));
    const n = Object.keys(triples).length;
    total += n;
    console.log(`OK   ${f}  (${n} triples)`);
  } catch (e) {
    failures++;
    console.log(`FAIL ${f}: ${e}`);
  }
}
// Also validate the ontology vocab files.
for (const f of ['ontology/codebase.ttl', 'ontology/domains.ttl']) {
  if (!existsSync(f)) continue;
  files++;
  try {
    const triples = mod.parse_turtle_wasm(readFileSync(f, 'utf8'));
    const n = Object.keys(triples).length;
    total += n;
    console.log(`OK   ${f}  (${n} triples)`);
  } catch (e) {
    failures++;
    console.log(`FAIL ${f}: ${e}`);
  }
}

console.log(`\n${files - failures}/${files} files parsed by qualia_core_db (WASM), ${total} triples`);
process.exit(failures ? 1 : 0);
