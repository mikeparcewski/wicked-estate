---
id: wicked-estate-adr-008
title: "Retain `wicked-estate-memory-api` as a Re-Export Shim"
status: active
date: 2026-07-21
---
# ADR-008 — Retain `wicked-estate-memory-api` as a Re-Export Shim

**Status:** Accepted
**Date:** 2026-07-21
**Wave:** v0.13.0 Consolidation
**Implements:** DoD §1.5 — absorbed crates resolution

---

## Context

The v0.13.0 consolidation absorbed the standalone `wicked-memory` repository into the
`wicked-estate` workspace. That absorption introduced two internal crates:

- `wicked-estate-memory-core` — all memory types, the `MemoryApi` trait, and the scoring
  algorithms (`rrf_fuse`, `budget_pack`, `decay`, `Candidate`).
- `wicked-estate-memory-api` — previously the public surface crate that external consumers
  (tools, downstream agents, any code that `cargo add wicked-estate-memory-api`) depended on.

After the absorption, `wicked-estate-memory-api/src/lib.rs` was reduced to a single-file
re-export shim:

```rust
//! Re-export shim — all types moved to `wicked-estate-memory-core` in v0.13.0.
pub use wicked_estate_memory_core::{
    CaptureRequest, MemoryApi, MemoryCoverage, RecallQuery, RecalledItem, ReflectResult,
};
```

All internal callers (`wicked-estate-mcp/src/lib.rs`, `wicked-estate-memory/src/*.rs`) were
migrated to import from `wicked_estate_memory_core` directly; no Rust `use` statement in the
workspace references `wicked_estate_memory_api` by name.

The DoD (REQ-005 §1.5) required a decision: **delete the shim** (if superseded by the absorbed
implementation) or **reconcile it** (if it still defines an interface spec that the absorbed
crate conforms to) — with the decision documented in an ADR.

---

## Decision

**Retain `wicked-estate-memory-api` as a re-export shim.** Do not delete it in v0.13.x.

The shim carries no duplicate implementation. It re-exports the canonical types from
`wicked-estate-memory-core` verbatim. Any external project that listed
`wicked-estate-memory-api` as a Cargo dependency and imports its types continues to compile
without modification.

---

## Rationale

1. **Zero maintenance burden.** The shim is four lines of `pub use`. It adds no implementation
   surface, no tests, no upgrade complexity. The cost of retaining it is negligible.

2. **External backward compatibility.** The crate was previously published as the public API
   surface for the memory domain. Removing it would be a breaking change for any downstream
   crate that depends on it by name. Retaining it preserves semver compatibility for v0.13.x.

3. **Incorrect to call it "superseded".** The DoD option "deleted if superseded by absorbed
   implementation" applies when the shim's interface is replaced by a different or incompatible
   contract. Here the interface is identical — the shim is additive compatibility scaffolding,
   not a conflicting implementation.

4. **Internal migration already complete.** All workspace-internal callers already use
   `wicked_estate_memory_core` directly. The shim's continued presence does not slow or confuse
   internal development.

---

## Removal path

The shim should be removed when a major version break (v1.0 or later) allows a clean break:

1. Bump the workspace to a new major version.
2. Delete `crates/wicked-estate-memory-api/` from the workspace.
3. Update `Cargo.toml` files that list it as a dependency (currently `wicked-estate-mcp`
   and `wicked-estate-memory`).
4. Publish a deprecation notice in the changelog pointing callers to
   `wicked-estate-memory-core`.

This is not on the critical path for v0.13.x.

---

## References

- `crates/wicked-estate-memory-api/src/lib.rs` — the shim itself
- `crates/wicked-estate-memory-core/src/lib.rs` — canonical type definitions
- `crates/wicked-estate-mcp/src/lib.rs` — uses `wicked_estate_memory_core` directly
- `.product/REQ-005-dod-criteria.md` §1.5 — the DoD requirement this ADR satisfies
