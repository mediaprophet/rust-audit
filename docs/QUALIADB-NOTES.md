# QualiaDB — limitations & open questions

Findings from integrating `qualia-cli` / `qualia_core_db` WASM
(verified against release **v0.0.40.15**, 2026-10-17). Things to revisit
or work around later.

## Confirmed limitations / quirks

- **SPARQL: no `;` predicate sugar.** `?f a T ; p ?o .` →
  `invalid term token: Punct(';')`. Write one triple pattern per line
  (`?f a T . ?f p ?o .`). Multi-pattern joins DO work (`range-backed hash-join`).
- **SPARQL parser rejects `#` comment lines** — `Unsupported query form`.
  `audit query -f` strips them before passing the query.
- **SPARQL `MINUS` works** via a fallback "resident compatibility executor"
  (probed 2026-10-17): set-difference queries like coverage gaps return
  correct results — see generated `gaps.sparql`.
- **`FILTER NOT EXISTS` silently returns wrong results.** Probed: returned
  `0 result(s)` where 3 were expected, no error. Do NOT rely on
  `FILTER`/`NOT EXISTS` — use `MINUS` for negation.
- **SPARQL feature surface partially unprobed.** `OPTIONAL`, `UNION`,
  `BIND` are named in the range-executor's error message as supported;
  aggregates, `FILTER` (non-negation), property paths, `?s ?p ?o` with
  variables elsewhere — untested. `query sparql-star` exists, untested.
  → TODO: probe systematically, record what subset works.
- **`import` takes a single input file.** Multi-graph projects must
  concatenate `.ttl` files first (we do this in `audit q42`).
- **`;`-free writing applies to queries only.** Our emitted Turtle uses
  `;` grouping — the Turtle *parser* accepts it fine; it's the SPARQL
  *query* grammar that's restricted.
- **License: CC BY-NC-ND 4.0.** Non-commercial use, no derivatives.
  OK for personal tooling; cannot ship derivatives of the binaries/WASM.
- **q42 format is proprietary/undocumented.** Author `.q42` only via
  `qualia-cli import` (or the WASM session APIs) — don't hand-emit bytes.

## WASM API notes (`qualia_core_db.js` + `_bg.wasm`, works under Node 24)

- `parse_turtle_wasm(ttl)` → array of `{subject, predicate, object}` —
  good as a parse/validation path (used by `scripts/validate.mjs`).
- `parse_n3logic_wasm(payload)` exists — **untested**. N3 rule execution
  support unknown; the site lists N3 among 21 "modalities" but rule
  evaluation semantics are undocumented.
- `WasmQ42Session` + `pack_quins_into_superblock` are exposed but need a
  `world_did_hash` / digest inputs and a term→u64 lexicon encoding that
  is undocumented → TODO if a pure-WASM q42 pipeline is wanted later.
- Init under Node: `mod.default({ module_or_path: wasmBytes })`.

## Nice-to-have upstream asks

- Accept `;` (and `,`) in SPARQL queries.
- Document q42 layout / lexicon hashing for direct authoring.
- N3 rule surface: which log:/list:/math: builtins are implemented.

## Workarounds in use here

- `;`-free SPARQL — emit explicit triple patterns (see README example).
- Coverage/diff queries are computed in Rust AND emitted as a `MINUS`
  SPARQL query (`gaps.sparql`) that runs on the q42 today; `gaps.n3`
  rules are emitted as the intended N3 encoding once N3 is verified.
