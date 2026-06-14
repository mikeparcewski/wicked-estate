//! Read-only cloud `Collector` interface (AWS / Azure / GCP) — **designed-not-built seam**
//! (Wave W10.2, ADR-004).
//!
//! # Design constraints (ADR-004 §5 — Security posture)
//!
//! * **Read-only, observe-only.** Collectors never mutate cloud state. The minimal IAM / RBAC
//!   policy required is documented per provider below.
//! * **No secret storage.** Credentials are sourced from the user's **ambient credential chain**
//!   at runtime (env vars, profile, workload identity) and are never persisted to the graph store
//!   or any config file. [`CloudConfig`] carries a profile *name* (a pointer into the ambient
//!   chain) but never a raw key or secret.
//! * **Auditable.** Every collected node records `metadata["collector"]` + `metadata["origin"]`.
//!
//! # Intended real backends (designed, not built)
//!
//! | Provider | Service | Minimal read-only role |
//! |----------|---------|------------------------|
//! | **AWS**  | AWS Resource Explorer (`ResourceExplorer2:Search`) + Config | `AWSResourceExplorerReadOnlyAccess` |
//! | **Azure**| Azure Resource Graph (`resources | project …`) | `Reader` on the subscription |
//! | **GCP**  | Cloud Asset Inventory (`cloudasset.assets.searchAllResources`) | `roles/cloudasset.viewer` |
//!
//! # What is built
//!
//! * [`CloudProvider`] — discriminator enum for the three hyperscalers.
//! * [`CloudResource`] — provider-agnostic resource descriptor (no credential fields).
//! * [`CloudCollector`] trait — the one genuinely new abstraction (sibling to `Extractor`).
//! * [`cloud_resources_to_nodes`] — maps `CloudResource`s to `NodeKind::Other("resource")` nodes
//!   tagged `origin=live` + the provider, compatible with `tfstate`'s node shape so
//!   `estate_drift` can diff iac-vs-live by `(type, name)` key.
//! * [`MockCloudCollector`] — deterministic, no-network reference impl for tests and demos.
//! * [`CloudConfig`] — runtime config (no secrets).
//! * [`open_cloud_collector`] — factory that returns a *designed-not-built* error for all real
//!   providers, mirroring `open_telemetry_sink` from ADR-006. The `MockCloudCollector` is
//!   constructed directly, not via this factory.
//!
//! # What is designed, not built
//!
//! The real AWS / Azure / GCP collector impls (network SDK calls, pagination, retry). They live in
//! a future `ci-collect` crate or as match arms in [`open_cloud_collector`]; adding one requires
//! **zero caller changes** (identical to ADR-003 `open_store` / ADR-006 `open_telemetry_sink`).

use serde_json::Value;
use wicked_estate_core::{Error, Language, Location, Node, NodeKind, Result, Span, Symbol};

// ── Provider discriminator ────────────────────────────────────────────

/// The three hyperscaler cloud providers supported by the Collector seam.
///
/// Corresponds to the three intended real backends (ADR-004 §3):
/// - [`Aws`][CloudProvider::Aws]   — AWS Resource Explorer / AWS Config
/// - [`Azure`][CloudProvider::Azure] — Azure Resource Graph
/// - [`Gcp`][CloudProvider::Gcp]   — GCP Cloud Asset Inventory
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum CloudProvider {
    /// Amazon Web Services (AWS Resource Explorer + Config). **Default.**
    #[default]
    Aws,
    /// Microsoft Azure (Azure Resource Graph).
    Azure,
    /// Google Cloud Platform (Cloud Asset Inventory).
    Gcp,
}

impl CloudProvider {
    /// Short lowercase identifier used in node metadata and symbol schemes.
    ///
    /// Matches the `origin` tagging convention established by `tfstate.rs`:
    /// `metadata["provider"] = provider.as_str()`.
    pub fn as_str(&self) -> &str {
        match self {
            CloudProvider::Aws => "aws",
            CloudProvider::Azure => "azure",
            CloudProvider::Gcp => "gcp",
        }
    }
}

// ── Provider-agnostic resource descriptor ────────────────────────────

/// A single live cloud resource, normalised across providers.
///
/// This struct is intentionally **credential-free** (ADR-004 §5): credentials never leave the
/// ambient chain; only observable resource data enters the graph.
///
/// # Field semantics
///
/// | Field | Meaning | Example |
/// |-------|---------|---------|
/// | `id`  | Stable physical identity (ARN, Azure resource ID, GCP full-resource name) | `"arn:aws:s3:::my-bucket"` |
/// | `kind` | Provider resource type string | `"aws_s3_bucket"`, `"Microsoft.Storage/storageAccounts"` |
/// | `region` | Cloud region / location (if applicable) | `"us-east-1"` |
/// | `depends_on` | Physical IDs of resources this resource depends on (edge targets) | `["arn:aws:iam::…"]` |
/// | `attributes` | Flattened key-value bag of observable attributes | `[("bucket_name", "my-bucket")]` |
///
/// The `attributes` field must **never** contain credential values (passwords, secrets, tokens).
/// Redact before constructing.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CloudResource {
    /// Stable physical identity (ARN, Azure resource ID, GCP full-resource name).
    pub id: String,
    /// Provider resource type, e.g. `"aws_s3_bucket"` or `"Microsoft.Network/virtualNetworks"`.
    pub kind: String,
    /// Cloud region / location, if applicable.
    pub region: Option<String>,
    /// Physical IDs of resources this resource depends on. Edge direction:
    /// `source = this resource` (dependent), `target = depended-upon resource`.
    pub depends_on: Vec<String>,
    /// Flattened observable attributes. Secrets must be redacted before populating.
    pub attributes: Vec<(String, String)>,
}

// ── CloudCollector trait ──────────────────────────────────────────────

/// Enumerates **live** cloud resources from a read-only account or state source.
///
/// `CloudCollector` is the sibling of `Extractor` for live cloud state (ADR-004 §3).
/// Where `Extractor` turns a *file* into graph, `CloudCollector` turns a *read-only account*
/// into graph. Output is always tagged `metadata["origin"] = "live"` so the drift detector
/// (`estate_drift`, W10.3) can separate it from `origin = "iac"` nodes.
///
/// # Contract
///
/// * **Read-only.** Implementations MUST NOT mutate cloud state.
/// * **Observe-only.** Implementations MUST NOT persist credentials; credential access is
///   purely at runtime via the ambient SDK chain.
/// * `collect` may be called multiple times; each call is independent.
///
/// # Example (mock)
///
/// ```rust
/// use wicked_estate_extract::cloud::{CloudCollector, CloudProvider, CloudResource, MockCloudCollector};
///
/// let collector = MockCloudCollector {
///     provider: CloudProvider::Aws,
///     resources: vec![CloudResource {
///         id: "arn:aws:s3:::my-bucket".to_string(),
///         kind: "aws_s3_bucket".to_string(),
///         region: Some("us-east-1".to_string()),
///         depends_on: vec![],
///         attributes: vec![("bucket_name".to_string(), "my-bucket".to_string())],
///     }],
/// };
/// let resources = collector.collect().unwrap();
/// assert_eq!(resources.len(), 1);
/// ```
pub trait CloudCollector: std::fmt::Debug {
    /// The provider this collector targets.
    fn provider(&self) -> CloudProvider;

    /// Enumerate live resources from the configured account / scope.
    ///
    /// Returns a flat list of [`CloudResource`]s. Dependency edges within the list are expressed
    /// via [`CloudResource::depends_on`]; cross-scope references may not resolve locally.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Extraction`] on credential failure, API error, or parse failure.
    fn collect(&self) -> Result<Vec<CloudResource>>;
}

// ── Node mapping — drift-compatible ──────────────────────────────────

/// Map a slice of [`CloudResource`]s to `NodeKind::Other("resource")` graph nodes.
///
/// Each node is tagged with:
/// - `metadata["origin"]   = "live"` — the drift discriminator (matches `tfstate.rs`)
/// - `metadata["provider"] = provider.as_str()` — `"aws"` / `"azure"` / `"gcp"`
/// - `metadata["type"]     = resource.kind` — resource type string
/// - `metadata["region"]   = resource.region` (when present)
/// - per-resource attributes are stored as `metadata["attr.<key>"] = value`
///
/// The node `name` is `resource.id` and the `signature` is `resource.kind`. This mirrors
/// `tfstate.rs`'s shape so `estate_drift` (W10.3) can diff `origin=iac` vs `origin=live` by
/// `(kind, name)` key without special-casing each collector.
///
/// # Symbol identity
///
/// Uses `Symbol::synthetic("cloud-<provider>", id)`, giving ADR-002-stable IDs keyed on the
/// physical resource identity, not content hash or line number.
pub fn cloud_resources_to_nodes(provider: CloudProvider, resources: &[CloudResource]) -> Vec<Node> {
    resources
        .iter()
        .map(|r| resource_to_node(&provider, r))
        .collect()
}

fn resource_to_node(provider: &CloudProvider, resource: &CloudResource) -> Node {
    let scheme = format!("cloud-{}", provider.as_str());
    let symbol = Symbol::synthetic(&scheme, &resource.id).id();

    let mut metadata = serde_json::Map::new();
    // Drift discriminator — must match tfstate.rs's "live" tag exactly.
    metadata.insert("origin".to_string(), Value::String("live".to_string()));
    metadata.insert(
        "provider".to_string(),
        Value::String(provider.as_str().to_string()),
    );
    metadata.insert("type".to_string(), Value::String(resource.kind.clone()));

    if let Some(ref region) = resource.region {
        metadata.insert("region".to_string(), Value::String(region.clone()));
    }

    for (k, v) in &resource.attributes {
        // Namespace attribute keys to avoid colliding with reserved keys above.
        metadata.insert(format!("attr.{k}"), Value::String(v.clone()));
    }

    let mut node = Node::new(
        symbol,
        NodeKind::Other("resource".to_string()),
        resource.id.clone(),
        // Cloud resources have no file language; use a sentinel matching the provider.
        Language::new(format!("cloud-{}", provider.as_str())),
        // No source file location — use a sentinel path equal to the resource id.
        Location::new(resource.id.clone(), Span::ZERO),
    );
    node.signature = Some(resource.kind.clone());
    node.metadata = metadata;
    node
}

// ── MockCloudCollector — deterministic, no-network reference impl ─────

/// Deterministic mock collector for tests, demos, and CI.
///
/// Returns its canned [`resources`][MockCloudCollector::resources] slice unchanged — no network,
/// no credentials. Construct directly; do **not** use [`open_cloud_collector`] (the factory
/// returns a designed-not-built error for all real providers).
///
/// # Example
///
/// ```rust
/// use wicked_estate_extract::cloud::{CloudCollector, CloudProvider, CloudResource, MockCloudCollector};
///
/// let mock = MockCloudCollector {
///     provider: CloudProvider::Gcp,
///     resources: vec![],
/// };
/// assert_eq!(mock.collect().unwrap(), vec![]);
/// assert_eq!(mock.provider(), CloudProvider::Gcp);
/// ```
#[derive(Debug, Default)]
pub struct MockCloudCollector {
    /// Provider identity reported by [`CloudCollector::provider`].
    pub provider: CloudProvider,
    /// Canned resources returned by [`CloudCollector::collect`].
    pub resources: Vec<CloudResource>,
}

impl CloudCollector for MockCloudCollector {
    fn provider(&self) -> CloudProvider {
        self.provider.clone()
    }

    fn collect(&self) -> Result<Vec<CloudResource>> {
        Ok(self.resources.clone())
    }
}

// ── CloudConfig — runtime config (no secrets) ─────────────────────────

/// Runtime configuration for a cloud collector factory call.
///
/// **No credentials.** Credential access uses the provider's ambient SDK chain at runtime:
/// - **AWS** — environment variables (`AWS_ACCESS_KEY_ID` / `AWS_SESSION_TOKEN`) or named profile
///   in `~/.aws/credentials`, or instance / ECS / IRSA workload identity.
/// - **Azure** — `AZURE_CLIENT_ID` / `AZURE_TENANT_ID` + managed identity or Azure CLI login.
/// - **GCP** — Application Default Credentials (`GOOGLE_APPLICATION_CREDENTIALS` or `gcloud auth`).
///
/// Keys and secrets are supplied via these ambient chains and are **never** stored in this struct,
/// in the graph, or in any persistent artefact (ADR-004 §5).
#[derive(Debug, Clone)]
pub struct CloudConfig {
    /// The hyperscaler to connect to.
    pub provider: CloudProvider,
    /// Cloud region to scope enumeration to (e.g. `"us-east-1"`, `"westeurope"`, `"us-central1"`).
    /// `None` means "all accessible regions" (implementation-defined).
    pub region: Option<String>,
    /// Named credential profile (pointer into the ambient chain, **not** a secret).
    /// - AWS: profile name from `~/.aws/config` (e.g. `"default"`, `"prod-ro"`).
    /// - Azure: subscription / tenant id (no secret).
    /// - GCP: project id.
    pub profile: Option<String>,
}

// ── Factory — open_cloud_collector ────────────────────────────────────

/// Open a [`CloudCollector`] from a [`CloudConfig`].
///
/// | Provider | Returns |
/// |----------|---------|
/// | [`CloudProvider::Aws`]   | `Err(Error::Extraction("… designed but not built — see ADR-004"))` |
/// | [`CloudProvider::Azure`] | same |
/// | [`CloudProvider::Gcp`]   | same |
///
/// **The [`MockCloudCollector`] is not routed through this factory** — construct it directly in
/// tests and demos.
///
/// # Real backend extensibility
///
/// Adding a real AWS / Azure / GCP impl:
/// 1. Add a `CloudCollector` impl (SDK calls, pagination, retry) in `ci-collect` (new crate) or
///    a submodule here.
/// 2. Add one match arm in this function on `cfg.provider`.
/// 3. **Zero caller changes** — identical to the `open_store` (ADR-003) and
///    `open_telemetry_sink` (ADR-006) patterns.
///
/// # Credential note
///
/// The real impls will obtain credentials solely from the provider's ambient SDK chain
/// (env / profile / workload identity) — never from this config struct (ADR-004 §5).
///
/// # Errors
///
/// Returns [`Error::Extraction`] with a clear "designed but not built" message referencing
/// ADR-004 and the intended backend SDK for each provider.
pub fn open_cloud_collector(cfg: &CloudConfig) -> Result<Box<dyn CloudCollector>> {
    let msg = match cfg.provider {
        CloudProvider::Aws => format!(
            "AWS cloud collector is designed but not built — see ADR-004. \
             To enable: add an `aws-sdk-resource-explorer2` + `aws-config` impl \
             (AWS Resource Explorer v2 / Config) and one match arm in \
             `open_cloud_collector`; zero caller changes required. \
             Region hint: {:?}, profile hint: {:?}.",
            cfg.region, cfg.profile,
        ),
        CloudProvider::Azure => format!(
            "Azure cloud collector is designed but not built — see ADR-004. \
             To enable: add an `azure_mgmt_resources` + Resource Graph impl \
             and one match arm in `open_cloud_collector`; zero caller changes required. \
             Region hint: {:?}, profile hint: {:?}.",
            cfg.region, cfg.profile,
        ),
        CloudProvider::Gcp => format!(
            "GCP cloud collector is designed but not built — see ADR-004. \
             To enable: add a `google-cloud-asset` (Cloud Asset Inventory) impl \
             and one match arm in `open_cloud_collector`; zero caller changes required. \
             Region hint: {:?}, profile hint: {:?}.",
            cfg.region, cfg.profile,
        ),
    };
    Err(Error::Extraction(msg))
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ──────────────────────────────────────────────────────

    fn aws_bucket() -> CloudResource {
        CloudResource {
            id: "arn:aws:s3:::my-bucket".to_string(),
            kind: "aws_s3_bucket".to_string(),
            region: Some("us-east-1".to_string()),
            depends_on: vec![],
            attributes: vec![("bucket_name".to_string(), "my-bucket".to_string())],
        }
    }

    fn aws_iam_role() -> CloudResource {
        CloudResource {
            id: "arn:aws:iam::123456789:role/app-role".to_string(),
            kind: "aws_iam_role".to_string(),
            region: None,
            depends_on: vec!["arn:aws:s3:::my-bucket".to_string()],
            attributes: vec![("role_name".to_string(), "app-role".to_string())],
        }
    }

    fn find_node<'a>(nodes: &'a [Node], id: &str) -> &'a Node {
        nodes.iter().find(|n| n.name == id).unwrap_or_else(|| {
            let present: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
            panic!("node with name {id:?} not found; present: {present:?}")
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

    // ── MockCloudCollector ────────────────────────────────────────────

    #[test]
    fn mock_returns_canned_resources() {
        let mock = MockCloudCollector {
            provider: CloudProvider::Aws,
            resources: vec![aws_bucket()],
        };
        let resources = mock.collect().expect("mock must not fail");
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].id, "arn:aws:s3:::my-bucket");
    }

    #[test]
    fn mock_empty_resources() {
        let mock = MockCloudCollector {
            provider: CloudProvider::Gcp,
            resources: vec![],
        };
        let resources = mock.collect().expect("empty mock must not fail");
        assert!(resources.is_empty());
    }

    #[test]
    fn mock_provider_identity() {
        let mock = MockCloudCollector {
            provider: CloudProvider::Azure,
            resources: vec![],
        };
        assert_eq!(mock.provider(), CloudProvider::Azure);
    }

    #[test]
    fn mock_collect_is_deterministic() {
        let mock = MockCloudCollector {
            provider: CloudProvider::Aws,
            resources: vec![aws_bucket(), aws_iam_role()],
        };
        let first = mock.collect().unwrap();
        let second = mock.collect().unwrap();
        assert_eq!(first, second, "collect must be deterministic");
    }

    // ── cloud_resources_to_nodes ──────────────────────────────────────

    #[test]
    fn nodes_have_origin_live() {
        let nodes = cloud_resources_to_nodes(CloudProvider::Aws, &[aws_bucket()]);
        assert_eq!(nodes.len(), 1);
        assert_meta_str(&nodes[0], "origin", "live");
    }

    #[test]
    fn nodes_have_correct_provider_tag() {
        let nodes = cloud_resources_to_nodes(CloudProvider::Azure, &[CloudResource {
            id: "/subscriptions/sub-1/resourceGroups/rg-1/providers/Microsoft.Network/virtualNetworks/vnet-1".to_string(),
            kind: "Microsoft.Network/virtualNetworks".to_string(),
            region: Some("westeurope".to_string()),
            depends_on: vec![],
            attributes: vec![],
        }]);
        assert_meta_str(&nodes[0], "provider", "azure");
    }

    #[test]
    fn nodes_have_resource_kind() {
        let nodes = cloud_resources_to_nodes(CloudProvider::Aws, &[aws_bucket()]);
        assert_meta_str(&nodes[0], "type", "aws_s3_bucket");
        assert_eq!(nodes[0].signature.as_deref(), Some("aws_s3_bucket"));
    }

    #[test]
    fn nodes_have_correct_node_kind() {
        let nodes = cloud_resources_to_nodes(CloudProvider::Aws, &[aws_bucket()]);
        assert_eq!(
            nodes[0].kind,
            NodeKind::Other("resource".to_string()),
            "node kind must be Other(resource)"
        );
    }

    #[test]
    fn node_name_is_physical_id() {
        let nodes = cloud_resources_to_nodes(CloudProvider::Aws, &[aws_bucket()]);
        assert_eq!(nodes[0].name, "arn:aws:s3:::my-bucket");
    }

    #[test]
    fn node_region_in_metadata() {
        let nodes = cloud_resources_to_nodes(CloudProvider::Aws, &[aws_bucket()]);
        assert_meta_str(&nodes[0], "region", "us-east-1");
    }

    #[test]
    fn node_without_region_has_no_region_key() {
        let nodes = cloud_resources_to_nodes(CloudProvider::Aws, &[aws_iam_role()]);
        assert!(
            nodes[0].metadata.get("region").is_none(),
            "region key must be absent when resource.region is None"
        );
    }

    #[test]
    fn node_attributes_namespaced() {
        let nodes = cloud_resources_to_nodes(CloudProvider::Aws, &[aws_bucket()]);
        assert_meta_str(&nodes[0], "attr.bucket_name", "my-bucket");
    }

    #[test]
    fn nodes_stable_symbol_ids_by_physical_id() {
        // Two separate calls with the same resource must produce the same SymbolId (ADR-002).
        let n1 = cloud_resources_to_nodes(CloudProvider::Aws, &[aws_bucket()]);
        let n2 = cloud_resources_to_nodes(CloudProvider::Aws, &[aws_bucket()]);
        assert_eq!(
            n1[0].symbol, n2[0].symbol,
            "SymbolId must be stable across calls (ADR-002)"
        );
    }

    #[test]
    fn two_providers_same_id_yield_distinct_symbols() {
        // cloud-aws vs cloud-azure — different schemes → different SymbolIds.
        let aws_nodes = cloud_resources_to_nodes(CloudProvider::Aws, &[aws_bucket()]);
        let azure_resource = CloudResource {
            id: "arn:aws:s3:::my-bucket".to_string(), // same id string, different provider
            kind: "azure_blob".to_string(),
            region: None,
            depends_on: vec![],
            attributes: vec![],
        };
        let azure_nodes = cloud_resources_to_nodes(CloudProvider::Azure, &[azure_resource]);
        assert_ne!(
            aws_nodes[0].symbol, azure_nodes[0].symbol,
            "different providers must yield distinct symbols even for identical ids"
        );
    }

    // ── depends_on mapping (drift-compatible) ─────────────────────────

    #[test]
    fn depends_on_does_not_affect_node_count() {
        // cloud_resources_to_nodes maps resources → nodes only (1:1).
        // Edge resolution is the caller's responsibility (same pattern as tfstate.rs).
        let resources = vec![aws_bucket(), aws_iam_role()];
        let nodes = cloud_resources_to_nodes(CloudProvider::Aws, &resources);
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn resource_with_depends_on_maps_to_node_correctly() {
        // aws_iam_role depends on aws_s3_bucket — the node must still be correct.
        let nodes = cloud_resources_to_nodes(CloudProvider::Aws, &[aws_iam_role()]);
        let node = find_node(&nodes, "arn:aws:iam::123456789:role/app-role");
        assert_meta_str(node, "type", "aws_iam_role");
        assert_meta_str(node, "origin", "live");
        assert_eq!(node.signature.as_deref(), Some("aws_iam_role"));
    }

    // ── open_cloud_collector — designed-not-built factory ─────────────

    #[test]
    fn factory_aws_returns_designed_not_built_error() {
        let cfg = CloudConfig {
            provider: CloudProvider::Aws,
            region: Some("us-east-1".to_string()),
            profile: None,
        };
        let err = open_cloud_collector(&cfg).expect_err("AWS factory must return Err");
        let msg = err.to_string();
        assert!(
            msg.contains("designed but not built"),
            "error must mention 'designed but not built'; got: {msg}"
        );
        assert!(
            msg.contains("ADR-004"),
            "error must reference ADR-004; got: {msg}"
        );
    }

    #[test]
    fn factory_azure_returns_designed_not_built_error() {
        let cfg = CloudConfig {
            provider: CloudProvider::Azure,
            region: None,
            profile: Some("my-subscription".to_string()),
        };
        let err = open_cloud_collector(&cfg).expect_err("Azure factory must return Err");
        let msg = err.to_string();
        assert!(
            msg.contains("designed but not built"),
            "error must mention 'designed but not built'; got: {msg}"
        );
        assert!(
            msg.contains("ADR-004"),
            "error must reference ADR-004; got: {msg}"
        );
    }

    #[test]
    fn factory_gcp_returns_designed_not_built_error() {
        let cfg = CloudConfig {
            provider: CloudProvider::Gcp,
            region: None,
            profile: None,
        };
        let err = open_cloud_collector(&cfg).expect_err("GCP factory must return Err");
        let msg = err.to_string();
        assert!(
            msg.contains("designed but not built"),
            "error must mention 'designed but not built'; got: {msg}"
        );
        assert!(
            msg.contains("ADR-004"),
            "error must reference ADR-004; got: {msg}"
        );
    }

    #[test]
    fn factory_errors_are_extraction_errors() {
        // Verify the error variant so callers can pattern-match consistently.
        for provider in [CloudProvider::Aws, CloudProvider::Azure, CloudProvider::Gcp] {
            let cfg = CloudConfig {
                provider,
                region: None,
                profile: None,
            };
            match open_cloud_collector(&cfg) {
                Err(Error::Extraction(_)) => {}
                other => panic!("expected Error::Extraction, got: {other:?}"),
            }
        }
    }

    // ── CloudProvider::as_str ─────────────────────────────────────────

    #[test]
    fn provider_as_str_values() {
        assert_eq!(CloudProvider::Aws.as_str(), "aws");
        assert_eq!(CloudProvider::Azure.as_str(), "azure");
        assert_eq!(CloudProvider::Gcp.as_str(), "gcp");
    }

    // ── GCP node shape ────────────────────────────────────────────────

    #[test]
    fn gcp_resource_nodes_tagged_gcp() {
        let gcp_vm = CloudResource {
            id: "//compute.googleapis.com/projects/my-proj/zones/us-central1-a/instances/my-vm"
                .to_string(),
            kind: "compute.googleapis.com/Instance".to_string(),
            region: Some("us-central1".to_string()),
            depends_on: vec![],
            attributes: vec![("machine_type".to_string(), "n1-standard-1".to_string())],
        };
        let nodes = cloud_resources_to_nodes(CloudProvider::Gcp, &[gcp_vm]);
        assert_meta_str(&nodes[0], "provider", "gcp");
        assert_meta_str(&nodes[0], "origin", "live");
        assert_meta_str(&nodes[0], "region", "us-central1");
        assert_meta_str(&nodes[0], "attr.machine_type", "n1-standard-1");
    }

    // ── empty slice ───────────────────────────────────────────────────

    #[test]
    fn empty_resources_yields_empty_nodes() {
        let nodes = cloud_resources_to_nodes(CloudProvider::Aws, &[]);
        assert!(nodes.is_empty());
    }
}
