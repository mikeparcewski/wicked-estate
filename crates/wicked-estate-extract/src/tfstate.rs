//! Terraform-state (`.tfstate`) collector — **no-credentials LIVE estate path** (ADR-004 W10).
//!
//! A `.tfstate` file is the ground-truth record of what Terraform actually deployed. Parsing it
//! gives `origin=live` resource nodes that can be graph-diffed against `origin=iac` nodes produced
//! by the HCL extractor (Wave W9) to detect drift.
//!
//! # Parsed schema fields
//!
//! Terraform state v4 JSON top-level:
//! - `version`            — must be 4 (we fail fast otherwise)
//! - `resources[]`        — array of resource records:
//!   - `module`           — optional module path (e.g. `"module.networking"`)
//!   - `mode`             — `"managed"` | `"data"` | `"module"` (we skip non-managed)
//!   - `type`             — resource type (e.g. `"aws_s3_bucket"`)
//!   - `name`             — logical name within the module (e.g. `"app"`)
//!   - `provider`         — provider source string
//!   - `instances[]`      — one entry per resource (count/for_each > 1 yields multiple):
//!     - `attributes`     — deployed attribute bag; `"id"` is the physical resource id
//!     - `dependencies[]` — terraform addresses of resources this one depends on
//!
//! # Node model
//!
//! One `NodeKind::Other("resource")` node per resource instance:
//! - `name`      = terraform address, e.g. `aws_s3_bucket.app` or `module.networking.aws_vpc.main`
//! - `signature` = resource type (e.g. `aws_s3_bucket`)
//! - `metadata`  = `{ "type", "provider", "origin": "live", "physical_id"? }`
//!
//! # Edge model
//!
//! `EdgeKind::Other("depends_on")` edges — direction follows the invariant:
//! **source = dependent (the resource that has the dependency), target = dependency**.
//! So blast-radius on a resource finds everything that depends on it.
//!
//! When the dependency address resolves to a node in the same state file the edge is emitted as a
//! local edge (full `Edge`). When it does not resolve (cross-module forward refs, etc.) it is
//! emitted as an `UnresolvedRef` keyed by the terraform address so a future resolver can wire it.
//!
//! # Origin tag and drift detection
//!
//! Every node carries `metadata["origin"] = "live"`. The IaC extractors (CFN, K8s, HCL) will
//! carry `metadata["origin"] = "iac"`. A graph-diff keyed on `(type, name)` between the two sets
//! gives the drift surface: resources in `iac` but not `live` = to-be-created; resources in `live`
//! but not `iac` = to-be-destroyed; attributes differ = to-be-updated. That diff lives in W10's
//! drift detector, not here — this module only emits the LIVE side.

use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use wicked_estate_core::{
    Descriptor, Edge, EdgeKind, Error, Extraction, Location, Node, NodeKind, ResolutionTier,
    Result, Span, Suffix, Symbol, SymbolId, UnresolvedRef,
};

// ── serde structures matching Terraform state v4 JSON ────────────────────────

#[derive(Debug, Deserialize)]
struct TfState {
    version: u64,
    #[serde(default)]
    resources: Vec<TfResource>,
}

#[derive(Debug, Deserialize)]
struct TfResource {
    /// Present only for module-scoped resources: `"module.networking"`.
    #[serde(default)]
    module: Option<String>,
    /// `"managed"` | `"data"` | `"module"`. We skip everything except `"managed"`.
    mode: String,
    /// Resource type, e.g. `"aws_s3_bucket"`.
    #[serde(rename = "type")]
    resource_type: String,
    /// Logical name, e.g. `"app"`.
    name: String,
    /// Provider source string, e.g. `"provider[\"registry.terraform.io/hashicorp/aws\"]"`.
    provider: String,
    #[serde(default)]
    instances: Vec<TfInstance>,
}

#[derive(Debug, Deserialize)]
struct TfInstance {
    /// Attribute bag. The `"id"` key is the physical resource id (AWS resource id, ARN, etc.).
    #[serde(default)]
    attributes: Value,
    /// Terraform addresses of resources this instance depends on.
    #[serde(default)]
    dependencies: Vec<String>,
    /// `index_key` is present when count/for_each > 1 (integer or string). We include it in the
    /// address so multi-instance resources have distinct node identities.
    #[serde(default)]
    index_key: Option<Value>,
}

// ── Public collector ──────────────────────────────────────────────────────────

/// Parses Terraform state v4 JSON into LIVE resource nodes and `depends_on` edges.
///
/// This is the no-credentials LIVE estate path (ADR-004, W10). It requires no AWS/Azure/GCP
/// credentials — the `.tfstate` file is a local artifact produced by `terraform apply`.
///
/// # Usage
///
/// ```no_run
/// use wicked_estate_extract::TfstateCollector;
///
/// let json = std::fs::read_to_string("terraform.tfstate").unwrap();
/// let extraction = TfstateCollector::new().collect(&json).unwrap();
/// ```
pub struct TfstateCollector;

impl TfstateCollector {
    pub fn new() -> Self {
        Self
    }

    /// Parse a Terraform state v4 JSON string and return an [`Extraction`].
    ///
    /// Returns `Err` on JSON parse failure or on an unsupported state version.
    /// Individual resources that are not `mode = "managed"` are silently skipped.
    pub fn collect(&self, tfstate_json: &str) -> Result<Extraction> {
        let state: TfState = serde_json::from_str(tfstate_json)
            .map_err(|e| Error::Extraction(format!("tfstate JSON parse error: {e}")))?;

        if state.version != 4 {
            return Err(Error::Extraction(format!(
                "unsupported tfstate version {}; only v4 is supported",
                state.version
            )));
        }

        let mut nodes: Vec<Node> = Vec::new();
        let mut local_edges: Vec<Edge> = Vec::new();
        let mut refs: Vec<UnresolvedRef> = Vec::new();

        // First pass: build address → SymbolId map so dependency edges can be resolved locally.
        let mut addr_to_symbol: HashMap<String, SymbolId> = HashMap::new();
        for res in &state.resources {
            if res.mode != "managed" {
                continue;
            }
            for inst in &res.instances {
                let addr = terraform_address(res, inst);
                let sym = resource_symbol(&addr);
                addr_to_symbol.insert(addr, sym);
            }
        }

        // Second pass: emit nodes and edges.
        for res in &state.resources {
            if res.mode != "managed" {
                continue;
            }

            for inst in &res.instances {
                let addr = terraform_address(res, inst);
                let symbol = resource_symbol(&addr);

                // ── Build metadata ────────────────────────────────────────────
                let mut metadata = serde_json::Map::new();
                metadata.insert("type".to_string(), Value::String(res.resource_type.clone()));
                metadata.insert("provider".to_string(), Value::String(res.provider.clone()));
                // origin=live is the drift discriminator: separates this from origin=iac nodes.
                metadata.insert("origin".to_string(), Value::String("live".to_string()));

                // Best-effort: extract the physical resource id from the `id` attribute.
                if let Some(id_val) = inst.attributes.get("id") {
                    if !id_val.is_null() {
                        metadata.insert("physical_id".to_string(), id_val.clone());
                    }
                }

                // ── Node ──────────────────────────────────────────────────────
                let mut node = Node::new(
                    symbol.clone(),
                    NodeKind::Other("resource".to_string()),
                    addr.clone(),
                    // tfstate resources have no file language — use a sentinel.
                    wicked_estate_core::Language::new("tfstate"),
                    // No source file location; use a sentinel path equal to the address.
                    Location::new(addr.clone(), Span::ZERO),
                );
                node.signature = Some(res.resource_type.clone());
                node.metadata = metadata;
                nodes.push(node);

                // ── Dependency edges ──────────────────────────────────────────
                // Edge direction invariant: source = dependent, target = dependency.
                for dep_addr in &inst.dependencies {
                    if let Some(dep_sym) = addr_to_symbol.get(dep_addr) {
                        // Resolved within this state file → local edge.
                        local_edges.push(Edge::new(
                            symbol.clone(),
                            dep_sym.clone(),
                            EdgeKind::Other("depends_on".to_string()),
                            ResolutionTier::Parsed,
                            "tfstate-collector",
                        ));
                    } else {
                        // Not resolved in this file (cross-module / partial state) → unresolved ref.
                        refs.push(UnresolvedRef::new(
                            symbol.clone(),
                            dep_addr.clone(),
                            EdgeKind::Other("depends_on".to_string()),
                            Location::new(addr.clone(), Span::ZERO),
                        ));
                    }
                }
            }
        }

        Ok(Extraction {
            nodes,
            local_edges,
            refs,
        })
    }
}

impl Default for TfstateCollector {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build the canonical terraform address for a resource instance.
///
/// Format follows `terraform state list` output:
/// - module-scoped: `module.networking.aws_vpc.main`
/// - root module:   `aws_s3_bucket.app`
/// - with count/for_each index: `aws_instance.web[0]` or `aws_instance.web["prod"]`
fn terraform_address(res: &TfResource, inst: &TfInstance) -> String {
    let base = match &res.module {
        Some(m) if !m.is_empty() => format!("{}.{}.{}", m, res.resource_type, res.name),
        _ => format!("{}.{}", res.resource_type, res.name),
    };
    match &inst.index_key {
        Some(Value::Number(n)) => format!("{base}[{n}]"),
        Some(Value::String(s)) => format!("{base}[\"{s}\"]"),
        _ => base,
    }
}

/// Build a stable `SymbolId` for a terraform resource address.
///
/// Uses `Symbol::synthetic` with scheme `"tfstate"` and the address as the id.
/// This gives stable IDs regardless of which line in the state file the resource
/// appears on (ADR-002 compliant).
fn resource_symbol(address: &str) -> SymbolId {
    Symbol::synthetic("tfstate", address).id()
}

/// Build a stable `SymbolId` for a terraform resource address using the Global symbol form.
/// Kept for future use when a logical namespace path is needed (e.g. cross-file resolution).
#[allow(dead_code)]
fn resource_symbol_global(address: &str) -> SymbolId {
    Symbol::global(
        "tfstate",
        None,
        vec![Descriptor {
            name: address.to_string(),
            suffix: Suffix::Term,
            disambiguator: None,
        }],
    )
    .id()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn collector() -> TfstateCollector {
        TfstateCollector::new()
    }

    // ── helpers (mirrors languages.rs style) ─────────────────────────────────

    fn assert_resource_node<'a>(extraction: &'a Extraction, address: &str) -> &'a Node {
        extraction
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Other("resource".to_string()) && n.name == address)
            .unwrap_or_else(|| {
                let present: Vec<&str> = extraction
                    .nodes
                    .iter()
                    .filter(|n| n.kind == NodeKind::Other("resource".to_string()))
                    .map(|n| n.name.as_str())
                    .collect();
                panic!("resource node {address:?} not found; present: {present:?}")
            })
    }

    fn assert_meta_str(node: &Node, key: &str, expected: &str) {
        let val = node
            .metadata
            .get(key)
            .unwrap_or_else(|| panic!("metadata key {key:?} missing on node {:?}", node.name));
        assert_eq!(
            val.as_str(),
            Some(expected),
            "metadata[{key}] on {}: expected {expected:?}, got {val:?}",
            node.name
        );
    }

    fn assert_depends_on_edge(extraction: &Extraction, from_addr: &str, to_addr: &str) {
        let from_sym = resource_symbol(from_addr);
        let to_sym = resource_symbol(to_addr);
        let found = extraction.local_edges.iter().any(|e| {
            e.source == from_sym
                && e.target == to_sym
                && e.kind == EdgeKind::Other("depends_on".to_string())
        });
        assert!(
            found,
            "expected depends_on edge {from_addr} → {to_addr}; edges: {:?}",
            extraction
                .local_edges
                .iter()
                .map(|e| (e.source.as_str(), e.target.as_str()))
                .collect::<Vec<_>>()
        );
    }

    // ── address helper ────────────────────────────────────────────────────────

    #[test]
    fn terraform_address_root_module() {
        let res = TfResource {
            module: None,
            mode: "managed".to_string(),
            resource_type: "aws_s3_bucket".to_string(),
            name: "app".to_string(),
            provider: "provider[...]".to_string(),
            instances: vec![],
        };
        let inst = TfInstance {
            attributes: Value::Null,
            dependencies: vec![],
            index_key: None,
        };
        assert_eq!(terraform_address(&res, &inst), "aws_s3_bucket.app");
    }

    #[test]
    fn terraform_address_nested_module() {
        let res = TfResource {
            module: Some("module.networking".to_string()),
            mode: "managed".to_string(),
            resource_type: "aws_vpc".to_string(),
            name: "main".to_string(),
            provider: "provider[...]".to_string(),
            instances: vec![],
        };
        let inst = TfInstance {
            attributes: Value::Null,
            dependencies: vec![],
            index_key: None,
        };
        assert_eq!(
            terraform_address(&res, &inst),
            "module.networking.aws_vpc.main"
        );
    }

    #[test]
    fn terraform_address_with_count_index() {
        let res = TfResource {
            module: None,
            mode: "managed".to_string(),
            resource_type: "aws_instance".to_string(),
            name: "web".to_string(),
            provider: "provider[...]".to_string(),
            instances: vec![],
        };
        let inst = TfInstance {
            attributes: Value::Null,
            dependencies: vec![],
            index_key: Some(Value::Number(serde_json::Number::from(2u64))),
        };
        assert_eq!(terraform_address(&res, &inst), "aws_instance.web[2]");
    }

    // ── minimal inline fixture ────────────────────────────────────────────────

    const MINIMAL: &str = r#"
{
  "version": 4,
  "terraform_version": "1.6.0",
  "serial": 1,
  "lineage": "test",
  "resources": [
    {
      "mode": "managed",
      "type": "aws_s3_bucket",
      "name": "logs",
      "provider": "provider[\"registry.terraform.io/hashicorp/aws\"]",
      "instances": [
        {
          "attributes": { "id": "my-log-bucket", "bucket": "my-log-bucket" },
          "dependencies": []
        }
      ]
    }
  ]
}
"#;

    #[test]
    fn minimal_emits_one_resource_node() {
        let ex = collector().collect(MINIMAL).expect("minimal must parse");
        let node = assert_resource_node(&ex, "aws_s3_bucket.logs");
        assert_meta_str(node, "origin", "live");
        assert_meta_str(node, "type", "aws_s3_bucket");
        assert_eq!(node.signature.as_deref(), Some("aws_s3_bucket"));
    }

    #[test]
    fn minimal_physical_id_extracted() {
        let ex = collector().collect(MINIMAL).expect("minimal must parse");
        let node = assert_resource_node(&ex, "aws_s3_bucket.logs");
        let phys = node
            .metadata
            .get("physical_id")
            .expect("physical_id must be set");
        assert_eq!(phys.as_str(), Some("my-log-bucket"));
    }

    #[test]
    fn version_3_is_rejected() {
        let bad = r#"{"version":3,"resources":[]}"#;
        let err = collector().collect(bad).expect_err("v3 must error");
        assert!(
            err.to_string().contains("unsupported tfstate version"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn data_mode_resources_are_skipped() {
        let json = r#"
{
  "version": 4,
  "resources": [
    {
      "mode": "data",
      "type": "aws_ami",
      "name": "latest",
      "provider": "provider[...]",
      "instances": [
        { "attributes": { "id": "ami-12345" }, "dependencies": [] }
      ]
    }
  ]
}"#;
        let ex = collector()
            .collect(json)
            .expect("data mode state must parse");
        let resource_nodes: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Other("resource".to_string()))
            .collect();
        assert!(
            resource_nodes.is_empty(),
            "data-mode resources must be skipped; got {:?}",
            resource_nodes
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>()
        );
    }

    // ── fixture-based tests ───────────────────────────────────────────────────

    fn load_fixture() -> Extraction {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.tfstate");
        let json = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cannot read sample.tfstate: {e}"));
        TfstateCollector::new()
            .collect(&json)
            .expect("sample.tfstate must parse")
    }

    #[test]
    fn fixture_emits_three_resource_nodes() {
        let ex = load_fixture();
        let resource_nodes: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Other("resource".to_string()))
            .collect();
        assert_eq!(
            resource_nodes.len(),
            3,
            "expected 3 resource nodes; got: {:?}",
            resource_nodes
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fixture_root_module_resources_have_correct_addresses() {
        let ex = load_fixture();
        assert_resource_node(&ex, "aws_s3_bucket.app");
        assert_resource_node(&ex, "aws_iam_role.app_role");
    }

    #[test]
    fn fixture_module_nested_resource_has_correct_address() {
        let ex = load_fixture();
        assert_resource_node(&ex, "module.networking.aws_vpc.main");
    }

    #[test]
    fn fixture_all_nodes_have_origin_live() {
        let ex = load_fixture();
        for node in ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Other("resource".to_string()))
        {
            assert_meta_str(node, "origin", "live");
        }
    }

    #[test]
    fn fixture_bucket_has_physical_id() {
        let ex = load_fixture();
        let bucket = assert_resource_node(&ex, "aws_s3_bucket.app");
        let phys = bucket
            .metadata
            .get("physical_id")
            .expect("physical_id must be set");
        assert_eq!(phys.as_str(), Some("my-app-bucket"));
    }

    #[test]
    fn fixture_type_metadata_correct() {
        let ex = load_fixture();
        let bucket = assert_resource_node(&ex, "aws_s3_bucket.app");
        assert_meta_str(bucket, "type", "aws_s3_bucket");
        assert_eq!(bucket.signature.as_deref(), Some("aws_s3_bucket"));

        let vpc = assert_resource_node(&ex, "module.networking.aws_vpc.main");
        assert_meta_str(vpc, "type", "aws_vpc");
        assert_eq!(vpc.signature.as_deref(), Some("aws_vpc"));
    }

    #[test]
    fn fixture_depends_on_edges_emitted() {
        let ex = load_fixture();
        // aws_iam_role.app_role depends on aws_s3_bucket.app
        assert_depends_on_edge(&ex, "aws_iam_role.app_role", "aws_s3_bucket.app");
        // module.networking.aws_vpc.main depends on both
        assert_depends_on_edge(&ex, "module.networking.aws_vpc.main", "aws_s3_bucket.app");
        assert_depends_on_edge(
            &ex,
            "module.networking.aws_vpc.main",
            "aws_iam_role.app_role",
        );
    }

    #[test]
    fn fixture_no_unresolved_refs_all_local() {
        let ex = load_fixture();
        // All dependencies in sample.tfstate are within the same file → all edges are local.
        assert!(
            ex.refs.is_empty(),
            "expected 0 unresolved refs; got: {:?}",
            ex.refs
                .iter()
                .map(|r| r.raw_name.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fixture_edge_direction_invariant() {
        // Edge invariant: source = DEPENDENT, target = DEPENDENCY.
        // Blast-radius on aws_s3_bucket.app → everything that depends on it.
        let ex = load_fixture();
        let bucket_sym = resource_symbol("aws_s3_bucket.app");
        let dependents: Vec<&str> = ex
            .local_edges
            .iter()
            .filter(|e| {
                e.target == bucket_sym && e.kind == EdgeKind::Other("depends_on".to_string())
            })
            .map(|e| e.source.as_str())
            .collect();
        // Both iam_role and vpc depend on the bucket.
        assert_eq!(
            dependents.len(),
            2,
            "expected 2 dependents of aws_s3_bucket.app; got: {dependents:?}"
        );
    }

    #[test]
    fn unresolved_ref_emitted_for_cross_file_dependency() {
        // A resource that depends on an address NOT in this state file should
        // produce an UnresolvedRef, not a local edge.
        let json = r#"
{
  "version": 4,
  "resources": [
    {
      "mode": "managed",
      "type": "aws_security_group",
      "name": "web",
      "provider": "provider[...]",
      "instances": [
        {
          "attributes": { "id": "sg-abc123" },
          "dependencies": ["aws_vpc.external_module_vpc"]
        }
      ]
    }
  ]
}"#;
        let ex = collector()
            .collect(json)
            .expect("cross-file dep state must parse");
        assert!(
            !ex.refs.is_empty(),
            "expected an unresolved ref for cross-file dep; got none"
        );
        let r = &ex.refs[0];
        assert_eq!(r.raw_name, "aws_vpc.external_module_vpc");
        assert_eq!(r.kind, EdgeKind::Other("depends_on".to_string()));
    }

    #[test]
    fn invalid_json_returns_error() {
        let bad = "not json at all {{{";
        assert!(
            collector().collect(bad).is_err(),
            "invalid JSON must return Err"
        );
    }

    #[test]
    fn empty_resources_array_is_valid() {
        let json = r#"{"version":4,"resources":[]}"#;
        let ex = collector()
            .collect(json)
            .expect("empty resources must parse");
        assert!(ex.nodes.is_empty());
        assert!(ex.local_edges.is_empty());
        assert!(ex.refs.is_empty());
    }
}
