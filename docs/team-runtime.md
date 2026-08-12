# WICKED_RUNTIME — the foundation runtime profile

The wicked foundation packages (wicked-estate, wicked-vault, wicked-ledger,
wicked-bus) flip together on one environment switch:

| Env | Meaning |
|---|---|
| `WICKED_RUNTIME=local` (or unset) | Zero-infra local stores (SQLite / in-repo files) — **the default**. |
| `WICKED_RUNTIME=team` | Self-hosted shared Postgres (decision #3), named by `WICKED_STORE_URL=postgres://…` |

Any other `WICKED_RUNTIME` value is an **error** — a typo must never silently
fall back to a local store in a deployment that believes state is shared.

## Resolution rules (estate)

One seam resolves every binary's store spec:
`wicked_estate_store::resolve_store_spec` (pure logic in
`resolve_store_spec_from`, unit-tested). Priority — narrow-explicit beats
broad-profile beats ambient:

1. **Explicit spec** (`--db <spec>`): the operator named the exact store.
   Profiles never override it — a crew-style repo-scoped code-graph store
   stays repo-scoped under any profile.
2. **`WICKED_RUNTIME=team`** → `WICKED_STORE_URL` (must be `postgres://` /
   `postgresql://`; missing or non-postgres is a loud error). The profile
   overrides an ambient `WICKED_ESTATE_DB` — that is the point: one switch
   retargets the foundation coherently.
3. **`WICKED_ESTATE_DB`** — the ambient local override (unchanged behavior).
4. The caller's default (`.wicked-estate/graph.db`).

Under `local`, a set-but-unused `WICKED_STORE_URL` is inert (shared shell
profiles may export it permanently).

## Bring-up: the shared Postgres

```bash
WICKED_PG_PASSWORD=change-me docker compose -f deploy/docker-compose.team.yml up -d

export WICKED_RUNTIME=team
export WICKED_STORE_URL=postgres://wicked:$WICKED_PG_PASSWORD@<host>:5432/wicked_estate
```

The store creates its own schema on first open (`CREATE TABLE IF NOT EXISTS`
DDL in `PostgresStore::open`); there is no separate migration step.

## What team mode covers today (honest matrix)

| Surface | Team mode | Notes |
|---|---|---|
| `wicked-estate` CLI (index / query / blast-radius / …) | ✅ with a `--features postgres` build | The `open_store`/`open_store_ext` factory has the PostgresStore arm (ADR-003). Default builds keep sqlx out; a resolved `postgres://` spec then errors with "requires the 'postgres' feature". |
| Graph store layer (`GraphStore` contract) | ✅ | Full conformance suite passes against the team-resolved Postgres (`tests/team_runtime.rs`, run by the CI postgres job). Batches are one `READ COMMITTED` transaction — concurrent readers never see a torn batch (locked decision #8, PR #104). |
| `wicked-estate-mcp` server | ❌ fails loud at startup | The MCP async graph path is the SqlitePool, and the memory / knowledge / embedding stores open SQLite directly. **Follow-up: AsyncGraphStore-for-Postgres + PG homes for memory/knowledge.** |
| wicked-vault | ❌ fails loud (CLI, pre-I/O) | Only store driver is `store_mode: 'in-repo'` (git-native — itself team-shareable through git). **Follow-up: a server-backed driver behind the `store_mode` seam.** |
| wicked-ledger | ❌ fails loud (`createDomainStore`) | JSON + better-sqlite3, no shared-store driver. **Follow-up: a Postgres driver behind `createDomainStore()`.** |
| wicked-bus | stays local in this leg | Cross-host delivery is the PostgresBus track — bench verdict: GO / adapt as LISTEN/NOTIFY hybrid (wicked-bus#58). |

## Verifying team mode

The functional gate: the same estate operations that define the local
contract pass against a real Postgres through the profile seam.

```bash
# spin up any Postgres (the compose file above, or CI's service container)
TEST_POSTGRES_URL=postgres://<user>:<password>@localhost:5432/estate_test \
  cargo test -p wicked-estate-store --features postgres
```

This runs the PG conformance suite, the torn-read concurrency regression, and
`team_runtime.rs` (profile resolution → factory → full conformance).

## Identity note

The team profile shares state across engineers; the actor/identity contract
(OAuth/OIDC for humans + workload tokens for agents, decision #6) is the
other half of Phase 8 and is enforced at the crew API + foundation writes —
not by this seam.
