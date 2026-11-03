# Documentation coverage

A `///`/`/**`/docstring attached to a function counts as documented. Test functions, private helpers and WGSL (no doc-comment convention) lower the raw rate — track **public coverage** as the KPI.

> **GENERATED — do not edit.** Regenerate with `audit docs`.
>
> **Agent verification protocol:** before implementing, renaming, or claiming
> a function/module does not exist, grep `FUNCTIONS.md` (and the relevant
> `modules/*.md`) for the name or related terms. If it appears here, it exists
> at the listed `path:line` — read the source before writing anything new.
> Absence from this index means absence from the codebase as of the
> generation timestamp, not a guess.

| module | functions | documented | coverage | undocumented | undocumented public |
|---|---|---|---|---|---|
| `shaders` | 1 | 0 | 0% | 1 | 1 |
| `src` | 4 | 4 | 100% | 0 | 0 |
| **total** | **5** | **4** | **80%** | **1** | **1** |

**Public-surface coverage: 4/5 (80%)** — the KPI worth tracking.

## Undocumented-public worklist

Top 25 per module (full list in `modules/*.md` — blank `doc` column):

### `shaders` (1 undocumented public)

- `vertex_normal` — `shaders/noise.wgsl:6`

