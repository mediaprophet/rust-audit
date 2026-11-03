# rust_audit — codebase → RDF/Turtle → QualiaDB

Scans a codebase into RDF: directory/file inventory with per-file processing
state, plus extracted comments, functions, and intra-procedural semantics
(Rust, Python, WGSL), emitted as Turtle graphs and convertible into a
QualiaDB `.q42` volume.

**Semantic layer** (per function, name-resolved via tree-sitter — no type
inference, no macro expansion, no cross-module resolution):
`audit:calls` (fn → canonical callee IRI), `audit:callTarget` (qualified
target text), `audit:callExpr` (target + site arity), `audit:callCount`,
`audit:invokesMacro`, `audit:paramType`, `audit:returnType`,
`audit:readsField`/`audit:writesField` (receiver member access),
`audit:mutatesReceiver`. Call edges over-approximate for overloaded names
and dynamic dispatch — join through `audit:canonical` for candidate callees,
then refine with callTarget/callExpr.

## Pipeline

```text
audit inventory <root>     # walk tree, write .audit/manifest.ttl (status: pending)
audit scan <root>          # analyze files, emit .audit/graphs/*.ttl, mark processed+datetime
audit status <root>        # pending / processed / stale / failed table
audit q42 <root>           # merge ttl -> codebase.ttl -> qualia-cli import -> codebase.q42
audit query <vault.q42> "SELECT ..."   # SPARQL over the .q42 via qualia-cli
audit compare <ref> <tgt>  # cross-library coverage: which ref fns are missing from tgt
audit site <root>          # self-contained HTML viewer -> <out>/site/index.html
```

Scanning is **segmented and resumable**: the manifest is the source of truth.
`audit scan` processes only `pending`/`stale`/`failed` files unless `--all`.
Narrow a run with `--dir <rel>`, `--lang rust|python|wgsl`, `--max N`.
Re-running `inventory` after edits marks changed files `stale` (content-hash
comparison); untouched files keep their `processedAt` timestamps.

## Layout

```text
crates/audit-rdf      minimal Turtle writer + canonical line-subset reader
crates/audit-core     inventory, hashing, manifest, domains, scan orchestration
crates/audit-extract  tree-sitter analyzers (rust / python / wgsl)
crates/audit-cli      the `audit` binary + qualia-cli integration
ontology/codebase.ttl the audit: vocabulary (OWL/RDFS, human-editable)
ontology/domains.ttl  domain keyword vocabulary (drives audit:hasDomain)
scripts/validate.mjs  parses all .ttl through QualiaDB's WASM engine
tools/                drop qualia-cli / qualia_core_db wasm assets here
```

## Extending the ontology

- **New domains**: append a block to `ontology/domains.ttl` (canonical line
  format — one triple per line, same shape as existing domains). The
  classifier word-boundary-matches each `audit:keyword` against function
  name + signature + doc.
- **New vocabulary**: add terms to `ontology/codebase.ttl` (full Turtle).
- **New extracted facts**: `audit-core/src/scan.rs::file_graph` is where
  `FileAnalysis` becomes triples; `audit-extract` is where language
  analyzers live (`LangSpec` — add a spec to support another language).
- **WordNet expansion** (roadmap): Open English WordNet ships RDF dumps;
  keywords can be expanded into synsets for better recall.

## Viewer (`audit site`)

`audit site <root>` writes `<out>/site/index.html` — a single self-contained
HTML file (data embedded as JSON; no server or runtime deps, hostable on any
static host):

- **Facets**: language, status (pending/processed/stale/failed), domain, kind
  (function/method) — counts recompute within the current selection.
- **Search**: filters paths, function names/signatures/docs/callees, comment text.
- Files grouped by directory; click to expand functions (signature, arity,
  scope, domains, doc, `→ callees`) and the comment list.

Note: for very large trees the embedded JSON gets big (qualiadb ≈ 26 MB,
~7.6k files, ~38k functions) — fine locally; consider a data.json split for
hosting if needed.

## Documentation (`audit docs`)

`audit docs <root>` writes `<root>/audit-docs/` — an authoritative, greppable
registry agents should check **before** implementing, renaming, or claiming a
function does not exist:

- `INDEX.md` — stats + the verification protocol
- `FUNCTIONS.md` — every function, alphabetical, with arity/`path:line`/doc
- `CAPABILITIES.md` — functional-purpose overview grouped by domain + module
- `DOCS-COVERAGE.md` — per-module doc coverage + an **undocumented-public
  worklist** (visibility-aware: Rust `pub`, Python non-`_`-prefixed)
- `docrefs.ttl` — RDF overlay linking every function IRI to its module doc
- `modules/*.md` — per-module registries (stale generated docs are deleted)

### External documentation via RDF

Documentation does not have to live inside source files. Two conventions:

- **`docrefs.ttl`** — emitted as `audit:Document` nodes +
  `audit:documentedBy` links per function. It is **user-editable**: repoint or
  add entries to any IRI (a docs site, spec section, or ontology node) and
  merge the file into `codebase.ttl` — functions gain external documentation
  without touching source. Only absolute/prefixed IRIs — QualiaDB's parser
  returns null on relative IRIs.
- **`docref:` marker** — put `docref:<target>` inside a `///` doc comment and
  `audit scan` emits `audit:docRef "<target>"` on the function in addition to
  `audit:doc`. `/// Computes X. docref:specs/integrators.md#rk4`

## Cross-library coverage (`audit compare`)

Point `audit compare` at a reference library (e.g. an MIT lib you want to
support) and your implementation:

```bash
audit compare /path/to/reference-lib /path/to/your-impl
```

Every extracted function gets a name-scoped canonical identity
(`audit:canonx_<name>`), and each library emits `audit:exports` links, so
two libraries' graphs unify in one store. The command writes into
`<target>/.audit/compare/`:

- `compare.ttl` — libraries + canonical functions + exports (importable to q42)
- `gaps.ttl` — an `audit:GapReport` (missing, extra, arity mismatches)
- `gaps.sparql` — a `MINUS` query that computes gaps **inside** QualiaDB today
- `gaps.n3` — equivalent N3 rules (unverified; QualiaDB N3 support TBD)

```bash
audit query <tgt>/.audit/compare/compare.q42 -f <tgt>/.audit/compare/gaps.sparql
```

Matching is name-based (case-insensitive index, exact display names), so
cross-language ports still line up; arity mismatches on name matches are
reported as warnings rather than absences. See `docs/QUALIADB-NOTES.md`
for the probed QualiaDB limitations (no `;` sugar, `FILTER NOT EXISTS`
silently wrong — use `MINUS`, no `#` comments in queries).

## QualiaDB integration

`audit q42` shells out to `qualia-cli` (resolution order: `--qualia-cli`,
`QUALIA_CLI` env, `tools/qualia-cli*`, PATH). Download assets from
<https://github.com/mediaprophet/qualiaDB/releases> — this repo was verified
against `v0.0.40.15` (`qualia-cli-windows-x86_64.exe`, `qualia_core_db.js` +
`qualia_core_db_bg.wasm`).

- `q42 audit-source` validates Turtle through the native parser before import.
- `import` packs the combined graph into a `.q42` volume.
- `scripts/validate.mjs` runs the same parser via the **WASM** build
  (`parse_turtle_wasm`) — the path to a fully browser-side pipeline later
  (session staging + `pack_quins_into_superblock` are exposed for that).

Note: QualiaDB is CC BY-NC-ND 4.0 — fine for personal/internal tooling.

## Verification

```bash
cargo build && cargo test
cargo run -p audit-cli -- inventory tests/fixtures/mini-repo
cargo run -p audit-cli -- scan tests/fixtures/mini-repo
cargo run -p audit-cli -- status tests/fixtures/mini-repo
cargo run -p audit-cli -- q42 tests/fixtures/mini-repo
node scripts/validate.mjs
```

Example SPARQL (note: no `;` sugar — use separate triple patterns):

```bash
audit query tests/fixtures/mini-repo/.audit/codebase.q42 \
  "SELECT ?name WHERE { ?f <https://audit.dev/ns#hasDomain> <https://audit.dev/ns#dom_physics> . ?f <https://audit.dev/ns#name> ?name . }"
```
