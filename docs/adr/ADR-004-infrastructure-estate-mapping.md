# ADR-004 — Infrastructure & Estate Mapping (IaC + Live Cloud + Drift)

**Status:** Accepted (design); not built — build path is Waves W9/W10 · **Date:** 2026-06-12
**Relates to:** ADR-001 (schema), ADR-002 (identity), `wicked-estate-extract` registry, W6.1 drop-in extractors.

## Context

wicked_estate should map not just code but the **infrastructure estate**: declared IaC across
clouds/tools (Terraform/HCL, CloudFormation, Azure ARM/Bicep, GCP Deployment Manager / Config
Connector, Kubernetes/Helm, Pulumi, Ansible, …) **and** the **live** state of real accounts (via
read-only access), so we can detect **drift** — where the scripts no longer match reality. The
question this ADR answers: does this fit the existing graph model, and what is the minimal new
surface? Answer: it fits with **one new abstraction** (`Collector`) and **zero changes** to the
schema, the stores, or the resolvers' shape.

## Decision

### 1. IaC is "just more languages" (reuses everything)
An IaC file is a source file; a resource declaration is a `Node`; a dependency/reference is an
`Edge`. So IaC parsing reuses the existing tree-sitter `Extractor` + `.scm` pattern:

| Dialect | Grammar / format | Already in manifest? |
|---|---|---|
| Terraform / HCL | `tree-sitter-hcl` | **yes** (`hcl`) |
| CloudFormation / K8s / Helm / Ansible | YAML / JSON | **yes** (`yaml`) |
| Azure Bicep | `tree-sitter-bicep` | add a row |
| Pulumi | host language (TS/Py/Go) — already covered | n/a |

- Resource nodes use `NodeKind::Other("resource")` (or a new `Resource` kind), with
  `metadata = { provider, type, region, attributes… }`.
- Edges use `EdgeKind::Other("depends_on")` / `References` for `${aws_instance.web.id}`
  interpolations, `depends_on`, CFN `Ref`/`Fn::GetAtt`, module wiring. Same dependent→dependency
  invariant: `aws_eip.ip → aws_instance.web`.
- **No core/schema change** — `NodeKind::Other`/`EdgeKind::Other` + `metadata` already absorb this.

### 2. Resource identity (extends ADR-002)
A canonical infra `Symbol` so the *same real resource* from IaC and from a live account is linkable:
- **IaC:** `Symbol::global("iac-terraform", pkg=None, [Namespace(module), Type("aws_instance"), Term("web")])`.
- **Live:** `Symbol::global("cloud-aws", pkg=account/region, [Type("aws_instance"), Term(<physical-id|arn>)])`.
- A normalization step maps an IaC resource to its live physical id when known (from tfstate / tags
  / the provider's `address`), so drift can be keyed by a shared logical identity. Where the
  physical id is unknown, drift falls back to matching on (provider, type, name/tags).

### 3. Live cloud ingestion — a new `Collector` trait (sibling to `Extractor`)
The one genuinely new abstraction. `Extractor` turns a *file* into graph; `Collector` turns a
*read-only account or state source* into graph:

```rust
// planned, wicked-estate-core (added when W10 starts — not now)
pub trait Collector: Send + Sync {
    fn id(&self) -> &str;                 // "aws-live", "azure-live", "gcp-live", "tfstate"
    fn collect(&self, scope: &CollectScope) -> Result<Extraction>; // reuses Extraction!
}
```
- Implementations shell out to / use read-only SDK calls: AWS (`aws ... describe/list`, Config,
  Resource Explorer), Azure (`az resource list`, Resource Graph), GCP (Asset Inventory,
  `gcloud asset`). Also a cheap **`tfstate` collector** (read a Terraform state file/remote backend)
  as a no-cloud-creds path.
- Output is tagged `provenance = Extractor("<id>")` + `metadata.origin = "live"` (IaC carries
  `origin = "iac"`), so the two coexist in one graph and are separable.
- It reuses `Extraction` and the same `GraphWrite` ingestion + crash-safe batching — **no new
  store surface**.

### 4. Drift detection — a new `RetrievalTool` (`estate_drift`)
A graph diff by resource identity between the `origin=iac` and `origin=live` subgraphs:
- **Unmanaged** — live-only resources (exist in the account, absent from IaC) → security/cost risk.
- **Undeployed / deleted** — iac-only resources (declared, not present live).
- **Config drift** — present in both, divergent `metadata.attributes`.
Exposed as an MCP tool + a CLI `estate drift` command; emits a ranked report.

### 5. Security posture (hard constraints)
- **Read-only, observe-only.** Collectors never mutate cloud state. Require least-privilege
  read-only roles (e.g. AWS `ReadOnlyAccess` / `ViewOnlyAccess`); document the minimal policy.
- **No secret storage.** Use the user's ambient credential chain (env / profile / workload
  identity); never persist keys. Redact secrets from collected attributes before they hit the graph.
- **Auditable.** Every collected node records the collector id + timestamp; the run is logged.

## What this does NOT require
- No change to `Node`/`Edge`/schema, `GraphStore`/`GraphRead`/`GraphWrite`, or the existing
  resolvers' trait. Infra rides the same rails as code + the W6.1 drop-in extractors.

## Build path (deferred to Waves W9/W10)
- **W9 — IaC extraction:** Terraform/HCL + CloudFormation/K8s (YAML) extractors + `.scm`; an
  **infra resolver** (interpolation / Ref / GetAtt / module-output → resource node); resource
  identity normalization. Estate graph from files alone.
- **W10 — Live estate + drift:** the `Collector` trait + AWS/Azure/GCP + tfstate collectors
  (read-only); `estate_drift` tool; the minimal IAM policy docs.

## Consequences
- "Map the estate" is achievable from IaC files immediately at W9 (no cloud creds), with live +
  drift at W10.
- The same blast-radius / lineage queries work on infra: "what depends on this VPC?",
  "what breaks if I change this security group?" — because resources are just nodes and
  depends-on is just an edge.
- It strengthens the cross-artifact-graph thesis (prior art's SDLC-manifest-graph, prior art's
  non-code extractors): code + infra + (later) other SDLC artifacts in one queryable graph.
