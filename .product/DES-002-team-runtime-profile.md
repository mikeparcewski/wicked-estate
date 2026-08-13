# DES-002 — Team Runtime Profile: WICKED_RUNTIME Seam

**Product:** wicked-estate  
**Phase:** 8 — foundation team profile  
**Status:** IMPLEMENTED for the graph store layer; MCP/memory/knowledge are LOCAL-ONLY today (tracked follow-ups below)  
**Depends on:** REQ-002 §technology-constraints (decision #3: team infra = self-hosted shared Postgres; local zero-infra SQLite stays the default)

## 1. Problem

wicked-estate is zero-infra local-first: the default graph, memory, and knowledge
stores are SQLite files in `~/.wicked/`. A team deployment (shared estate among
multiple engineers, CI, and agents) needs a single environment switch that
retargets the foundation stores coherently — without changing any individual
binary invocation or configuration file, and without ever silently falling back
to local state under a team profile.

## 2. The `WICKED_RUNTIME` profile switch

| `WICKED_RUNTIME` | Graph store | Memory/knowledge | Notes |
|---|---|---|---|
| unset or `local` | `~/.wicked-estate/graph.db` (SQLite) | `~/.wicked/{memory,knowledge}.db` (SQLite) | Zero-infra default |
| `team` | `WICKED_STORE_URL` (must be `postgres://…`) | LOCAL-ONLY today — see §5 | Fail-loud if URL absent or non-postgres |

Any other value is a loud boot error (a typo must never silently fall back).

## 3. Resolution rules (the `resolve_store_spec` contract)

Priority — narrow-explicit beats broad-profile beats ambient:

1. **Explicit spec** (`--db <spec>`): the operator named the exact store. A
   crew-style repo-scoped code-graph store stays repo-scoped under any profile.
2. **`WICKED_RUNTIME=team`** → `WICKED_STORE_URL` (must be `postgres://` or
   `postgresql://`; missing or non-postgres → loud error).
3. **`WICKED_ESTATE_DB`** — the ambient local override (unchanged).
4. The caller's default (`.wicked-estate/graph.db`).

Under `local`, a set-but-unused `WICKED_STORE_URL` is inert — shared shell
profiles may export it permanently.

## 4. Graph store: implemented (including Postgres conformance suite)

The `GraphStore` contract passes fully against both SQLite and Postgres:

- `SqliteStore` — always built; the local-first default.
- `PostgresStore` — built behind `--features postgres` (enabled in the CI
  `postgres backend round-trip` job and the `team_runtime.rs` test suite).

Batches run in one `READ COMMITTED` transaction: concurrent readers never see
a torn batch (locked decision #8).

### Verify

```bash
TEST_POSTGRES_URL=postgres://<user>:<password>@localhost:5432/estate_test \
  cargo test -p wicked-estate-store --features postgres
```

Runs the PG conformance suite, the torn-read concurrency regression, and the
full profile resolution→factory→conformance chain (`tests/team_runtime.rs`).

### Team bring-up

```bash
WICKED_PG_PASSWORD=change-me docker compose -f deploy/docker-compose.team.yml up -d
export WICKED_RUNTIME=team
export WICKED_STORE_URL=postgres://wicked:$WICKED_PG_PASSWORD@<host>:5432/wicked_estate
```

No separate migration step — `PostgresStore::open` runs `CREATE TABLE IF NOT
EXISTS` DDL on first open.

## 5. Honest coverage matrix

| Surface | Team mode | Status |
|---|---|---|
| `wicked-estate` CLI (index / blast-radius / graph-view / …) | ✅ `--features postgres` build | `open_store`/`open_store_ext` factory has the PostgresStore arm (ADR-003). |
| Graph store layer (`GraphStore` contract) | ✅ | Full conformance suite passes against Postgres. |
| `wicked-estate-mcp` server | ❌ fails loud at startup | MCP async graph path uses SqlitePool; memory/knowledge/embedding stores open SQLite directly. **Follow-up: AsyncGraphStore-for-Postgres + PG homes for memory/knowledge.** |
| wicked-vault | ❌ fails loud (CLI pre-I/O) | Only driver: `store_mode: 'in-repo'` (git-native — shareable through git). **Follow-up: a server-backed driver.** |
| wicked-ledger | ❌ fails loud (`createDomainStore`) | JSON + better-sqlite3. **Follow-up: a Postgres driver behind `createDomainStore()`.** |
| wicked-bus | Stays local in this leg | Cross-host delivery = the PostgresBus track. Bench verdict: **GO** — LISTEN/NOTIFY hybrid (wicked-bus#58). Implementation tracked in wicked-bus#58. |

## 6. PostgresBus: go/no-go decision

The Phase 8 deliverable for wicked-bus is the **go/no-go verdict**, not the
full implementation in this leg.

**Bench results** (wicked-bus#58, BENCH-REPORT.md; Apple Silicon, PG 16 in Docker):

| Variant | p50 (ms) | p95 (ms) | throughput (ev/s) |
|---|---|---|---|
| sqlite-push (daemon, current bar) | 1.28 | 2.52 | 1,323 |
| pg-listen-notify (two processes) | 3.43 | 7.40 | 625 |
| pg-skip-locked-poll (two processes) | 4.44 | 7.58 | 641 |

**Verdict: GO — LISTEN/NOTIFY hybrid** (NOTIFY as wake-up, durable SKIP
LOCKED cursor read as delivery). ~3× the local push bar; low-single-digit-ms
is far below any agent-workflow need. Delivery stayed honest at-least-once
(5,000/5,000 events, zero loss). The ~1 ms Docker proxy tax drops in a
collocated production deployment.

Implementation is tracked in wicked-bus#58 (the spike branch carries the
prototype in `experimental/postgres-bus/`; follow-up is to promote into `lib/`).

## 7. Identity note

The team profile shares state across engineers; the actor/identity contract
(workload tokens for agents, OIDC for humans — decision #6) is enforced at the
crew API and tracked in wicked-crew's DES-AUTH-001.

## 8. Reference

- Implementation: `crates/wicked-estate-store/src/lib.rs` — `resolve_store_spec_from`, `open_store_any`
- Postgres conformance: `crates/wicked-estate-store/tests/team_runtime.rs`, `tests/postgres.rs`
- User-facing doc: `docs/team-runtime.md` (authoritative operator guide)
- Docker Compose: `deploy/docker-compose.team.yml`
- PostgresBus verdict: `wicked-bus#58` + `experimental/postgres-bus/BENCH-REPORT.md`
- Cross-reference: `wicked-crew/.product/DES-AUTH-001-team-profile-identity.md`
