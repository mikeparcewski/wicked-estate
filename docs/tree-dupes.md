# Workspace Dependency Duplicates

`cargo tree -d --workspace` output as of v0.13.0 (2026-07-06).

## Pre-existing duplicates (not introduced by Wave E / v0.13.0)

These version conflicts existed in the workspace before the Unified Foundation absorption
waves. They are driven by transitive dependency chains in `reqwest`, `rusqlite`, and
`fastembed` optional deps. No new duplicates were introduced by Waves A–E.

| Crate | Versions | Root cause |
|---|---|---|
| `thiserror` / `thiserror-impl` | 1.0.69 + 2.0.18 | `rusqlite` (memory/knowledge engines) pins 1.x; workspace tooling moved to 2.x |
| `core-foundation` | 0.9.4 + 0.10.1 | `reqwest` (observe crate) vs `system-configuration` transitive chain |
| `hashbrown` | 0.14.5 + 0.17.1 | `indexmap` vs `ahash` transitive chains |
| `getrandom` | 0.3.4 + 0.4.2 | `rand` 0.8 vs 0.9 transitive chains |

`memchr v2.8.2`, `serde_core v1.0.228`, `serde_json v1.0.150` appear once each in
`cargo tree -d` output as diamond-dependency repeats, not true version conflicts.

## Allowed exception (TEST-001 §3.5)

`thiserror-impl v1.0.69` and `thiserror-impl v2.0.18` are both proc-macro crates
pinned by transitive dependencies. Resolution would require patching `rusqlite` or
waiting for upstream upgrades — out of scope for v0.13.0.

## Action item

Track as a post-v0.13.0 cleanup in the workspace. No code change needed for v0.13.0
— all duplicates are transitive, compile cleanly, and produce zero warnings.
