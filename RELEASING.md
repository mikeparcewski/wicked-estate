# Releasing wicked-estate to crates.io

`cargo install wicked-estate` works only after the workspace is published to crates.io. This is a
multi-crate workspace, so the crates must be published **in dependency order** (a crate can't be
published until the crates it depends on already exist on crates.io at the pinned version).

> ⚠️ **crates.io publishes are irreversible.** A published version can be *yanked* (hidden from new
> resolution) but **never deleted**. Double-check before you run this. Bump the version first.

## Prerequisites

1. A crates.io account + an API token with publish scope: <https://crates.io/settings/tokens>
2. `cargo login <token>` (once per machine).
3. All crate names are claimed/available under the `wicked-estate` / `wicked-estate-*` namespace
   (verified: `wicked-estate` and every `wicked-estate-*` lib are free; the short `we-core` name was
   already taken on crates.io, which is why the libs are namespaced under the brand).

## 1. Bump the version

All crates share one version via `[workspace.package]` in the root `Cargo.toml`:

```toml
[workspace.package]
version = "0.0.1"   # bump this (semver) before every release
```

Internal deps are pinned to this version (`{ path = "...", version = "0.0.1" }`), so cargo rewrites
them to plain `version` deps on publish. Keep them in sync with the workspace version.

## 2. Validate the leaf crate (optional)

`cargo publish --dry-run` only fully works for the dependency-free leaf, because dependent crates
can't verify against not-yet-published deps:

```sh
cargo publish -p wicked-estate-core --dry-run
```

## 3. Publish (dependency order)

Use the script — it publishes in topological order and waits for index propagation between crates:

```sh
./scripts/publish.sh             # real publish
./scripts/publish.sh --dry-run   # validates the leaf; dependents only validate at real publish (their deps must be on crates.io first)
```

The order (deps before dependents):

```
wicked-estate-tree-sitter-rpg   # vendored RPG grammar (dep of -extract)
wicked-estate-core
wicked-estate-store
wicked-estate-extract
wicked-estate-rank
wicked-estate-resolve
wicked-estate-retrieve
wicked-estate          # the CLI binary crate (this is what `cargo install wicked-estate` fetches)
wicked-estate-mcp
```
(`wicked-estate-bench` is internal tooling — marked `publish = false`, not published.)

### Alternative: cargo-workspaces

[`cargo-workspaces`](https://crates.io/crates/cargo-workspaces) automates ordered workspace publishing:

```sh
cargo install cargo-workspaces
cargo workspaces publish --from-git    # resolves order + version bumps for you
```

## 4. After publishing

```sh
cargo install wicked-estate                      # CLI
cargo install wicked-estate --features model2vec # + static semantic search
cargo install wicked-estate-mcp                  # MCP server (separate binary)
```

## Notes

- The `fastembed` / `model2vec` features pull heavy optional deps (ONNX runtime, model downloads) and
  are **off by default**, so the published default build stays slim.
- The vendored RPG grammar (`crates/wicked-estate-extract/vendor/tree-sitter-rpg`) is a path dependency
  excluded from the workspace; confirm it packages correctly (`cargo publish -p wicked-estate-extract
  --dry-run` after core is live) — vendored grammar source is committed, its `target/` is gitignored.
