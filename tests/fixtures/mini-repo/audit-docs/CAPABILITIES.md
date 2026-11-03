# mini-repo — capabilities

Functional overview generated from source on 2026-10-17T06:44:02.9518518Z.
3 source files, 5 functions (4 documented).

> **GENERATED — do not edit.** Regenerate with `audit docs`.
>
> **Agent verification protocol:** before implementing, renaming, or claiming
> a function/module does not exist, grep `FUNCTIONS.md` (and the relevant
> `modules/*.md`) for the name or related terms. If it appears here, it exists
> at the listed `path:line` — read the source before writing anything new.
> Absence from this index means absence from the codebase as of the
> generation timestamp, not a guess.


## Domain coverage

| domain | functions |
|---|---|
| 3d | 1 |
| computational geometry | 1 |
| numerics | 1 |
| physics | 2 |

## Modules

### `shaders` — 1 function (0 documented)

_1 undocumented function — see `modules/shaders.md`._

### `src` — 4 functions (4 documented)

_Module doc comment for the fixture._

**computational geometry**

- `triangulate` (function) — Compute a delaunay triangulation of the points.

**numerics**

- `solve_linear` (method) — Solve a linear system.

**physics**

- `integrate_rk4` (function) — Integrate velocity using rk4.
- `apply_force` (method) — Apply a force to the rigidbody.

