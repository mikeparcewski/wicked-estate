# live-brain-cli — Integration Map for git-detection + file-watcher + subscribe command

**Recon date:** 2026-06-13  
**Purpose:** unblock the follow-up coding agent that will wire git-detection + a file watcher +
a `subscribe` command into `wicked-estate`. Every section below is grounded in exact file:line
references. No inference — only facts.

**Current build state:** the workspace does NOT compile. `wicked-estate-store` has 4 E0046 errors:
`SqliteStore` and `MemStore` both fail to implement the `GraphWrite` methods `set_repo_info` /
`log_change` and the `GraphRead` methods `file_git_sha` / `repo_info` / `edge_history` /
`changes_since`. These are declared in `wicked-estate-core/src/traits.rs` but have no impl in either
store. The first task below fixes this blocker before any other work can land.

---

## 1. `index_path` per-file flow and call ordering

**File:** `crates/wicked-estate/src/lib.rs`  
**Signature:** `pub fn index_path(store: &mut dyn GraphStoreMutExt, root: &Path) -> Result<GraphStats>`  
**Line:** 110

The boundary type is `&mut dyn GraphStoreMutExt` — NOT `&mut dyn GraphStore` or a concrete
`SqliteStore`. `GraphStoreMutExt` is defined in `wicked-estate-store/src/lib.rs:529` and is a supertrait of
`GraphStore`. All calls go through the trait — there is NO concrete-store downcast anywhere in
`index_path`.

### Phase 0 — bookkeeping (lines 115–120)
1. `store.version_bump()` — invalidates the prior query cache
2. `store.meta_set_key("indexed_root", ...)` — persists the root path for staleness checks

### Phase 1 — collect + classify (lines 120–213)
3. `collect_source_files(root)` — gitignore-aware walk via `ignore` crate
4. Build `previously_indexed: HashSet<String>` from `store.all_nodes()` (derives file paths from `location.file`) (lines 127–132)
5. Build `ext_map` (per-extension extractor cache) — compiled ONCE per run (lines 134–145)
6. Parallel read: for each supported file, read bytes + compute `xxh3` digest → `Vec<FileWork>` (lines 169–177)
7. Build `current_rel_paths: HashSet<String>` from supported files (line 166)

### Phase 2 — delete DELETED files (lines 178–193)
```
deleted = previously_indexed - current_rel_paths
store.begin_batch()
for each path in deleted:
    store.remove_file(path)   ← POINT (b): file is detected as DELETED here
store.commit_batch()
```
**Point (b) — deleted file detection:** line 186: `store.remove_file(path)`.
The `log_change(ChangeOp::Remove, relpath)` call belongs here, after line 186, inside the
same loop iteration.

### Phase 3 — split CHANGED/NEW from UNCHANGED (lines 195–213)
8. For each `FileWork`, call `store.file_digest(&fw.rel)`:
   - `stored == Some(&fw.digest)` → unchanged, skip (increment `unchanged_count`)
   - otherwise → push to `changed` Vec
9. If `changed.is_empty()`, return early with `store.stats()`

### Phase 4 — purge stale contributions from CHANGED files (lines 224–231)
```
store.begin_batch()
for each fw in &changed:
    store.remove_file(&fw.rel)   ← removes old nodes/edges BEFORE re-extraction
store.commit_batch()
```
**Call ordering fact:** `remove_file` is called BEFORE extraction writes new nodes. This is
correct — it archives the OLD version before the NEW version is written. The `log_change`
call for an Upsert does NOT belong here; it belongs after the new digest is committed (step 7
below), because until then the store has no data for this file.

**Point (a) — new/changed file detection:** the classification in Phase 3 (line 199,
`stored.as_deref() == Some(&fw.digest)`) is the exact predicate. A file is "new" when
`stored == None`; "changed" when `stored == Some(old_digest) != Some(new_digest)`. Both cases
land in `changed`. The `log_change(ChangeOp::Upsert, relpath)` call belongs after Phase 5
step 7 below (after `set_file_digest`) — once the new state is durably written.

### Phase 5 — EXTRACT → WRITE (lines 234–313)
5. Parallel extraction across `changed` files (lines 240–287) → `Vec<(rel_path, Extraction, text)>`
6. Serial write batch (lines 296–307):
   ```
   store.begin_batch()
   for each (rel_path, extraction, text) in extractions:
       store.upsert_nodes(&extraction.nodes)
       store.upsert_edges(&extraction.local_edges)
       store.set_file_content(rel_path, text)   ← content store
   store.commit_batch()
   ```
7. Digest batch (lines 310–313):
   ```
   store.begin_batch()
   for each fw in &changed:
       store.set_file_digest(&fw.rel, &fw.digest)
   store.commit_batch()
   ```
   **← This is the correct insertion point for `log_change(ChangeOp::Upsert, &fw.rel)`**,
   inside this loop, after `set_file_digest`. The store now has a consistent state for
   this file.

### Phase 6 — RESOLVE (lines 333–367)
8. Build `InMemoryIndex` from ALL nodes (changed + unchanged)
9. `resolve_all(resolvers, &all_refs, &index)` → resolved edges
10. Write batch: `upsert_edges(&resolved)` + `upsert_unresolved_refs(&unresolved)`

### Phase 7 — post-processing (lines 376–408)
11. Populate `pagerank.top` cache
12. `store.prune_dangling_edges()` — cleanup

### Exact call order summary
```
remove_file(deleted)          ← Phase 2: log_change(Remove, path) goes here
remove_file(changed)          ← Phase 4: purge OLD state, no log_change yet
upsert_nodes / upsert_edges   ← Phase 5 step 6
set_file_content              ← Phase 5 step 6
set_file_digest               ← Phase 5 step 7: log_change(Upsert, path) goes here
upsert_edges (resolved)       ← Phase 6
prune_dangling_edges          ← Phase 7
```

---

## 2. `main.rs` — arg parsing, store opening, command dispatch

**File:** `crates/wicked-estate/src/main.rs`

### Arg-parsing style (lines 55–98)
**Hand-rolled match — no clap.** The parser is:
```rust
let args: Vec<String> = std::env::args().skip(1).collect();
let (cmd, rest) = args.split_first();   // line 56
// then a while-loop over rest consuming --db, --dbs, --scip-file, positionals
```
`--db` may be repeated; the LAST value wins as the single-db default (`db` variable).
`--dbs a,b,c` is a comma-separated alias (line 80).
Positional args fall through to `positional: Vec<String>`.

### How the store is opened (lines 103–105 and throughout)
Two factory functions from `wicked-estate-store`:
- `open_store_ext(&db)` → `Box<dyn GraphStoreMutExt>` — used for commands that write or need
  version/meta/cache (index, scip, tfstate, query, blast-radius, stats, rank).
- `open_store(&db)` → `Box<dyn GraphStore>` — used for read-only commands (drift, source,
  semantic). Declared in `wicked-estate-store/src/lib.rs:499`.

**`index` command (lines 101–113):**
```rust
let mut store = open_store_ext(&db).map_err(to_any)?;      // line 104
wicked_estate::index_path(store.as_mut(), Path::new(path))        // line 105
```
`store.as_mut()` produces `&mut dyn GraphStoreMutExt`.

**`compact` command (lines 393–406):** the ONLY place where a concrete `SqliteStore` is opened
directly:
```rust
let mut store = SqliteStore::open(&db).map_err(to_any)?;   // line 398
store.compact()                                             // line 399
```
This is the pattern for the `--no-history` flag (see §6 below).

### Command dispatch style (lines 100–426)
```rust
match cmd {
    "index"        => { ... }
    "scip"         => { ... }
    "tfstate"      => { ... }
    "drift"        => { ... }
    "query"        => { ... }
    "blast-radius" => { ... }
    "stats"        => { ... }
    "rank" | "hotspots" => { ... }
    "source"       => { ... }
    "semantic"     => { ... }
    "cross-graph"  => { ... }
    "compact"      => { ... }
    _              => { println!("usage...") }
}
```
All existing commands are one-shot (they execute and return `Ok(())`). There is no existing
long-running command pattern.

### Where to insert the two new commands
Add two new arms to the `match cmd` block (before the `_` fallback):

1. `"watch" =>` — long-running; opens the store with `open_store_ext`, calls `index_path`
   once, then enters a `notify` debouncer loop.
2. `"subscribe" =>` — short-read; opens the store with `open_store_ext`, calls
   `store.changes_since(since_cursor)`, prints results, returns.

Both new arms need `--db` (already parsed). `watch` also takes a positional `<path>`.
`subscribe` takes `--since <seq>` which needs to be parsed in the shared flag loop
(add `"--since"` case alongside `"--db"` at lines 73–96).

### `stats` command (lines 234–241)
```rust
"stats" => {
    let store = open_store_ext(&db).map_err(to_any)?;
    maybe_print_staleness(store.as_ref(), &db);
    let s = store.stats().map_err(to_any)?;
    println!("nodes={} edges={} files={}", s.node_count, s.edge_count, s.file_count);
    for (k, v) in &s.edges_by_kind {
        println!("  edge {k} = {v}");
    }
}
```
The repo-info line (commit/branch/dirty) should be added after the `edges_by_kind` loop.
Read it via `store.repo_info()` (once the trait method is implemented).

---

## 3. Git detection — `RepoInfo` population plan

**Existing type:** `wicked-estate-core/src/repo.rs`
```rust
pub struct RepoInfo {
    pub commit: Option<String>,
    pub branch: Option<String>,
    pub remote: Option<String>,
    pub dirty: bool,
}
```
**Existing precedent:** `wicked_estate::commits_behind` (lines 593–621 of `lib.rs`) already uses
`std::process::Command::new("git").args(["-C", root_str, ...])` with graceful `Option` fallback.
The new `collect_repo_info` function follows the exact same pattern.

### Exact git commands (one `Command` per field, graceful fallback)
```rust
pub fn collect_repo_info(path: &Path) -> RepoInfo {
    let r = path.to_string_lossy();
    let run = |args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("git")
            .args(args)
            .output()
            .ok()?;
        if !out.status.success() { return None; }
        let s = String::from_utf8(out.stdout).ok()?;
        Some(s.trim().to_string())
    };

    let commit = run(&["-C", &r, "rev-parse", "HEAD"]);
    let branch = run(&["-C", &r, "rev-parse", "--abbrev-ref", "HEAD"])
        .filter(|b| b != "HEAD");  // detached HEAD → None
    let remote = run(&["-C", &r, "remote", "get-url", "origin"]);
    let dirty   = run(&["-C", &r, "status", "--porcelain"])
        .map(|s| !s.is_empty())
        .unwrap_or(false);

    RepoInfo { commit, branch, remote, dirty }
}
```

### Repo-root detection (for watch path normalization)
```
git -C <path> rev-parse --show-toplevel
```
Use this in the `watch` command to normalize the watched path to the repo root, so
file paths logged to the change-log are repo-relative (stripping the repo-root prefix —
same `rel(root, path)` helper already at `lib.rs:91–93`).

### Where to call `collect_repo_info`
In `index_path`, after `store.meta_set_key("indexed_root", ...)` (line 118), add:
```rust
let repo = collect_repo_info(root);
let _ = store.set_repo_info(&repo);   // GraphWrite::set_repo_info
```
Non-fatal if git is absent — the default `RepoInfo` is valid (all `None`, `dirty=false`).

---

## 4. `notify` crate — watch command dependency plan

### Stable versions (from crates.io, as of 2026-06-13)
```toml
notify                  = "8.2.0"
notify-debouncer-full   = "0.7.0"
```
**Use `notify-debouncer-full` — not `-mini`.** The `full` debouncer emits
`DebouncedEvent` with a `Vec<Event>` (deduplicated), proper rename tracking, and configurable
debounce duration. The `mini` debouncer is simpler but does not coalesce renames.

### Backend coverage (cross-platform, all pure Rust)
| Platform | Backend |
|---|---|
| macOS | FSEvents (kqueue fallback) |
| Linux | inotify |
| Windows | ReadDirectoryChangesW |
| Other | poll-based fallback |

All backends are pure Rust — no C system libraries beyond what the OS provides via libc
(already a transitive dep). **The single-binary goal is not broken.** The `notify` crate uses
conditional compilation to select the correct backend.

### `Cargo.toml` additions for `wicked-estate`
```toml
# crates/wicked-estate/Cargo.toml  [dependencies] section
notify                 = "8.2.0"
notify-debouncer-full  = "0.7.0"
```

### Minimal idiomatic usage sketch
```rust
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use notify::{RecursiveMode, EventKind, event::ModifyKind};
use std::time::Duration;

// Inside the `watch` command arm:
let (tx, rx) = std::sync::mpsc::channel::<DebounceEventResult>();
let mut debouncer = new_debouncer(Duration::from_millis(300), None, tx)?;
debouncer.watch(Path::new(path), RecursiveMode::Recursive)?;

// Initial full index before watching
let mut store = open_store_ext(&db)?;
wicked_estate::index_path(store.as_mut(), Path::new(path))?;

// Event loop (blocking — this is the long-running part)
for result in rx {
    match result {
        Ok(events) => {
            for event in events {
                match event.kind {
                    EventKind::Create(_)
                    | EventKind::Modify(ModifyKind::Data(_))
                    | EventKind::Modify(ModifyKind::Name(_))
                    | EventKind::Remove(_) => {
                        // Re-index the whole path (incremental skip-by-digest makes this cheap)
                        // The changed files will be detected by the digest comparison in index_path.
                        wicked_estate::index_path(store.as_mut(), Path::new(path))?;
                        break; // process accumulated events in one re-index pass
                    }
                    _ => {}
                }
            }
        }
        Err(errs) => {
            for e in errs {
                eprintln!("watch error: {e}");
            }
        }
    }
}
```

**Design note:** the implementation strategy is to call `index_path` (incremental) on any
change event, not to process individual file paths from the events. This is intentional:
`index_path`'s digest-comparison skip (Phase 3) means unchanged files cost almost nothing,
and the event payload from `notify` does not always include the full path (e.g. rename events
may come as a pair). One `index_path` call per debounced batch is correct and cheap.

---

## 5. `stats` command — adding repo-info line

**File:** `crates/wicked-estate/src/main.rs`, lines 234–241.

Current output:
```
nodes=N edges=M files=K
  edge <kind> = V
```

After the follow-up lands, add after the `edges_by_kind` loop:
```rust
if let Ok(Some(info)) = store.repo_info() {
    print!("repo:");
    if let Some(c) = &info.commit { print!("  commit={}", &c[..8.min(c.len())]); }
    if let Some(b) = &info.branch { print!("  branch={b}"); }
    if info.dirty { print!("  dirty"); }
    println!();
}
```

This requires `store.repo_info()` to be implemented (blocked on §blocker below).

---

## 6. `--no-history` flag and `SqliteStore::set_history_enabled`

### The concrete-store seam
The ONLY place a concrete `SqliteStore` is opened directly in `main.rs` is the `compact`
command (line 398):
```rust
let mut store = SqliteStore::open(&db).map_err(to_any)?;
```

**The `--no-history` flag pattern:** `set_history_enabled` is a proposed **inherent** method
on `SqliteStore` (not on any trait — mirrors the `compact()` pattern). It must be called
on the concrete store BEFORE `index_path` is called.

**Implementation approach:** add a second concrete open in the `index` arm:
```rust
"index" => {
    ensure_db_dir(&db)?;
    if no_history_flag {
        // Open concrete to call set_history_enabled, then coerce to trait object
        let mut concrete = SqliteStore::open(&db).map_err(to_any)?;
        concrete.set_history_enabled(false);
        let mut store: Box<dyn GraphStoreMutExt> = Box::new(concrete);
        wicked_estate::index_path(store.as_mut(), Path::new(path))?;
    } else {
        let mut store = open_store_ext(&db).map_err(to_any)?;
        wicked_estate::index_path(store.as_mut(), Path::new(path))?;
    }
}
```

Parse `--no-history` in the shared flag loop (lines 72–98) alongside `--db`.

**Note:** `set_history_enabled` is NOT on any trait. It controls whether `log_change` /
edge-history writes to the `history` and `changes` tables. For `watch` mode, history
should be ON (default). For bulk re-index of a large repo without a prior DB, `--no-history`
avoids the history-archive write amplification.

---

## 7. Blocker — Wave 7 trait methods not implemented

The workspace currently fails to compile (`cargo build --workspace` gives 4 E0046 errors).
The follow-up agent MUST implement stub bodies for all missing methods before anything else.

### Missing from `SqliteStore` (`wicked-estate-store/src/sqlite.rs`)
These are required by the `GraphWrite` trait:
- `fn set_repo_info(&mut self, info: &RepoInfo) -> Result<()>`
- `fn log_change(&mut self, op: ChangeOp, target: &str) -> Result<()>`

These are required by the `GraphRead` trait:
- `fn file_git_sha(&self, file: &str) -> Result<Option<String>>`
- `fn repo_info(&self) -> Result<Option<RepoInfo>>`
- `fn edge_history(&self, file: &str) -> Result<Vec<HistoricalEdge>>`
- `fn changes_since(&self, cursor: u64) -> Result<Vec<Change>>`

### Missing from `MemStore` (`wicked-estate-store/src/lib.rs`)
Same six methods — MemStore has no Wave 7 fields yet.

### Minimal stub plan (to unblock compilation)
Add a `changes` table to the SQLite schema (a single-column `changes` table with `seq INTEGER PRIMARY KEY AUTOINCREMENT`, `op TEXT`, `target TEXT`) and a `repo_info` row in the `meta` table. Implement:
- `set_repo_info` → `meta_set("repo_info", &serde_json::to_string(info)?)`
- `repo_info` → `meta_get("repo_info").map(|v| serde_json::from_str(&v))`
- `log_change` → `INSERT INTO changes(op, target) VALUES(?, ?)`
- `changes_since` → `SELECT ... FROM changes WHERE seq > ?`
- `file_git_sha` → `SELECT git_sha FROM files WHERE path=?` (requires a `git_sha` column on the `files` table; can return `Ok(None)` until populated)
- `edge_history` → `SELECT ... FROM edge_history WHERE file=?` (requires a `edge_history` table; can return `Ok(vec![])` until populated)

For `MemStore`, add `Vec<Change>` and `Option<RepoInfo>` fields and implement similarly.

**Schema file:** `crates/wicked-estate-store/src/schema.sql` (not read in this recon — check it before
adding tables to avoid duplicate `CREATE TABLE` conflicts).

---

## 8. Ordered implementation checklist for the coding agent

Work in this order; each step depends on the previous.

**Step 0 — Fix compilation blocker (§7)**
- [ ] Read `crates/wicked-estate-store/src/schema.sql` (not read in this recon)
- [ ] Add `changes` table + `git_sha` column on `files` + `edge_history` table to schema
- [ ] Implement the 6 missing methods on `SqliteStore` (see §7 stubs)
- [ ] Implement the 6 missing methods on `MemStore`
- [ ] `cargo build --workspace` and `cargo test --workspace` must be green before proceeding

**Step 1 — `collect_repo_info` in `wicked-estate/src/lib.rs` (§3)**
- [ ] Add `pub fn collect_repo_info(path: &Path) -> RepoInfo` to `lib.rs` using the exact git commands in §3
- [ ] Call it in `index_path` after line 118 (`meta_set_key("indexed_root", ...)`)
- [ ] Call `store.set_repo_info(&repo)` immediately after
- [ ] Add `log_change(ChangeOp::Remove, path)` inside the deleted-file loop (Phase 2, after `remove_file`)
- [ ] Add `log_change(ChangeOp::Upsert, &fw.rel)` inside the digest loop (Phase 5 step 7, after `set_file_digest`)
- [ ] All tests still green

**Step 2 — `subscribe` command (§2)**
- [ ] Parse `--since <seq: u64>` in the shared flag loop in `main.rs`
- [ ] Add `"subscribe" =>` arm: call `store.changes_since(since)`, print as JSON lines
- [ ] Add to help text

**Step 3 — `watch` command (§4)**
- [ ] Add `notify = "8.2.0"` and `notify-debouncer-full = "0.7.0"` to `wicked-estate/Cargo.toml`
- [ ] Add `"watch" =>` arm with the pattern in §4
- [ ] Repo-root detection: run `git -C <path> rev-parse --show-toplevel`; fall back to the path itself when not a git repo
- [ ] Initial `index_path` call before entering the event loop
- [ ] Debounce loop → `index_path` on any create/modify/remove event
- [ ] Add to help text

**Step 4 — `stats` repo-info line (§5)**
- [ ] Add the `store.repo_info()` print block to the `stats` arm

**Step 5 — `--no-history` flag (§6)**
- [ ] Add `SqliteStore::set_history_enabled(bool)` inherent method (controls whether `log_change` / `edge_history` writes happen)
- [ ] Parse `--no-history` in the shared flag loop
- [ ] Add concrete-store branch in the `index` arm

---

## Key file references

| What | File:Line |
|---|---|
| `index_path` signature | `wicked-estate/src/lib.rs:110` |
| `remove_file` loop (deleted) | `wicked-estate/src/lib.rs:184–192` |
| Digest comparison (new/changed detection) | `wicked-estate/src/lib.rs:199–205` |
| `remove_file` loop (changed, pre-extraction) | `wicked-estate/src/lib.rs:225–231` |
| `upsert_nodes` + `set_file_content` write batch | `wicked-estate/src/lib.rs:298–307` |
| `set_file_digest` batch — Upsert log goes here | `wicked-estate/src/lib.rs:310–314` |
| `rel()` helper | `wicked-estate/src/lib.rs:91–93` |
| `commits_behind` git precedent | `wicked-estate/src/lib.rs:593–621` |
| `main` arg-parsing loop | `wicked-estate/src/main.rs:72–98` |
| `match cmd` dispatch | `wicked-estate/src/main.rs:100` |
| `stats` command | `wicked-estate/src/main.rs:234–241` |
| `compact` — only concrete-store open | `wicked-estate/src/main.rs:398` |
| `GraphWrite` trait (all write methods) | `wicked-estate-core/src/traits.rs:111–136` |
| `GraphRead` trait (all read methods) | `wicked-estate-core/src/traits.rs:67–106` |
| `log_change` declaration | `wicked-estate-core/src/traits.rs:135` |
| `set_repo_info` declaration | `wicked-estate-core/src/traits.rs:126` |
| `changes_since` declaration | `wicked-estate-core/src/traits.rs:104` |
| `ChangeOp` + `Change` types | `wicked-estate-core/src/change.rs` |
| `RepoInfo` type | `wicked-estate-core/src/repo.rs` |
| `HistoricalEdge` type | `wicked-estate-core/src/history.rs` |
| `GraphStoreMutExt` extension trait | `wicked-estate-store/src/lib.rs:529–556` |
| `open_store_ext` factory | `wicked-estate-store/src/lib.rs:563–576` |
| `open_store` factory | `wicked-estate-store/src/lib.rs:499–511` |
| `SqliteStore::open` | `wicked-estate-store/src/sqlite.rs:76–81` |
| `SqliteStore::remove_file` | `wicked-estate-store/src/sqlite.rs:497–536` |
| `MemStore::remove_file` | `wicked-estate-store/src/lib.rs:223–248` |
| `SqliteStore::compact` (inherent pattern) | `wicked-estate-store/src/sqlite.rs:311–346` |
