# Cross-repo links

A project database holds several repos, each indexed with its own label
(`wicked-estate index <repo> --db project.db --repo <label>`). Each repo's edges resolve inside that
repo only. `wicked-estate link` adds the links between repos, and `blast-radius` reports them as
**crossings**, each with its evidence.

```sh
wicked-estate link --db project.db --project web --labels api,studio,core   # link the members
wicked-estate link --db project.db --project web --status                   # generation + staleness
wicked-estate blast-radius api/packages/api-types/index.d.ts --db project.db --json
```

## What links today (slice X1)

File-level **package imports**: a file in one repo imports a package that another member repo
provides. The link goes from the importing file to the provider's entry file. Supported:

| Ecosystem | Consumer declares | Provider is |
|---|---|---|
| npm | `dependencies` / `devDependencies` / `peerDependencies` / `optionalDependencies` (incl. `npm:` aliases), npm and pnpm workspaces | a `package.json` `name`; entry from `exports` (subpaths too; when `exports` exists it is the only way in), else `types`, `typings`, `module`, `main` |
| cargo | `[dependencies]` tables (incl. `package =` renames, `{ workspace = true }` with its path read from the workspace root, target tables) | a `[package]`; imported by its `[lib] name`; entry `[lib] path` or `src/lib.rs` |
| python (PEP 621) | `[project] dependencies`, `optional-dependencies`, poetry dependencies; `[tool.uv.sources]` paths for dependencies declared there (a source alone declares nothing) | `[project] name`; import names = its top-level packages |

Go waits for a later slice.

## Links come from identity, never from names

A dependency is linked only when the provider is proven. Every link names the rule that proved it
(`link.identity`):

1. **path**: `file:` or `link:` specs (the lockfile, when it links the dependency, must agree), a
   `workspace:` spec naming a member of the consumer's workspace (`workspaces` in the root
   `package.json`, or `pnpm-workspace.yaml`), a cargo `path =`, a uv/poetry `path`, or an npm
   lockfile entry that links a local package of that name.
2. **git-source**: the spec's and the lockfile's git URLs (both, when both are present, and they
   must agree) equal the provider repo's recorded git remote. npm installs a git dependency from the
   repository's top-level package, so only that package can be the provider.
3. **registry-source** (npm): the lockfile resolves the dependency to a registry tarball; the
   *installed* `node_modules/<pkg>/package.json` has that package's name and version and names the
   provider's repository (and `repository.directory`, compared against the provider's position in
   its work tree); and the installed-tree record (`node_modules/.package-lock.json`) has the same
   `version`, `resolved` and `integrity`. Without that record the link is
   `registry-source(version-only)`, at lower confidence; a record without an entry for the package
   is a stale install, never a link. The installed manifest and the lockfile entries are read when
   the consumer is indexed (a lockfile is often too large for the index to keep), confined to the
   checkout and redacted like any manifest.

Repository URLs compare as `host/path`: `github.com` and `bitbucket.org` repositories are exactly
`owner/repo`; every other host keeps the whole path (GitLab subgroups differ).

Anything else is a **possible** item, never an edge. Each one is reported with its candidates:

| Reason | Meaning |
|---|---|
| `dep:provider-unverified` | declared, a provider with that name exists, nothing proves it is that one |
| `dep:provider-ambiguous` | the evidence matches more than one provider |
| `dep:provider-contradicted` | the evidence names a different repository |
| `dep:undeclared` | imported but not declared by the importing package: never linked |
| `link:entry-unmapped` | provider proven, but its entry is not an indexed file. A `dist/*.d.ts` entry is mapped back to source only when the `build` script is one plain `tsc` (optionally after output cleanup), read through the config it names (`-p`/`-b`, else `tsconfig.json`; `extends` paths resolved where they are written; no `rootDir` means the sources' common directory); a bundler, a second compile, another npm script or an output-moving flag is not mapped |
| `dep:override` | npm `overrides` / yarn `resolutions` replace the package with an alias, path or git source |
| `dep:patched` | cargo `[patch]` / `[replace]` swaps the dependency's source |

Every possible item also says **why** in one line a person can act on (`registry: no installed
manifest recorded at index time`, `workspace: no workspace member of that name`, ...): `link --json`
lists them under `possible`, and text mode prints them.

## In answers

`blast-radius --json` (CLI and MCP) carries:

- `crossings`: the traversed links between two repos, shallowest first, at most 20
  (`crossings_total` counts all, `crossings_truncated` says when some are not listed), each with
  `from`/`to` repos and the evidence (`ranks: ["package-manifest"]`, `identity`, `package`,
  `ecosystem`, `confidence`, `skew`, versions). Only edges the walk admitted count; an import
  placeholder shared by two repos is never a crossing.
- `via_crossing` on each dependent reached through one (from the walk that reached it at its
  smallest depth), and `via_crossing_omitted` when that crossing is past the listed cap.

`path --json` (CLI and MCP) carries `crossings` for the route's hops that cross repos.

The CLI `blast-radius --json` also carries `link`: per project, its `generation`, whether it is
`stale` (a member was re-indexed, removed, is mid-index or failed, the last link errored or was
refused, or the linker or its rules changed) and which labels moved, plus `coverage`: per member,
links in and out, package imports considered, manifests read, and counts of what could not be linked
by reason (`link --status --json` lists the packages). The MCP answer does not carry `link` yet.

A link edge's own confidence is its identity rule's (`path`, `git-source`, `registry-source` 0.75;
`registry-source(version-only)` 0.6).

## Writers

Every estate write command (`index`, `watch` for each index pass, `scip`, `evidence`, `tfstate`,
`import-telemetry`, `annotate`, `supports retract`, `compact`, `link`, `redact-store`) takes
`<db>.writer.lock`, named after the database's canonical path, so two spellings of one file share
it. A second writer waits, by default up to 120 s (`WICKED_ESTATE_WRITER_WAIT_SECS`), and then
refuses with `WRITER-BUSY`. `link` publishes its links and their validation record in one
transaction: a failure leaves the previous generation and its record intact. An unchanged input
relinks as `replayed`, which writes only the validation record. A member whose last index did not
complete is reported `incomplete`; the run then publishes nothing and the project reads stale until
that member's index completes and `link` runs again.

One project per database: `link` refuses a second project id in a database already linked as
another one. A store backend that cannot list parked imports (Postgres today) refuses `link`
instead of publishing an empty generation.

`cross-graph` (name matching across separate databases) is deprecated in favour of this. It keeps
working until the governed read fences drop it.
