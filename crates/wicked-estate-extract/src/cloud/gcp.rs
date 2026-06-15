//! GCP cloud collector — **stub** (feature `cloud-gcp`).
//!
//! # Why stubbed?
//!
//! The original implementation targeted `google-cloud-asset = "0.7"` (a now-removed crate) and
//! `google-cloud-auth = "0.17"`.  The current versions are `google-cloud-asset-v1 = "1.11.0"`
//! and `google-cloud-auth = "1.13.0"`, which have a substantially different builder-based API.
//! Until the full migration is complete, `collect` returns a clear, actionable error; the module
//! structure, type normalisation helpers, and all unit tests remain intact.
//!
//! # Resource enumeration strategy (designed, not yet wired)
//!
//! Will use **Cloud Asset Inventory** `SearchAllResources` API which covers all GCP resource
//! types across all projects accessible to the credential. This is the authoritative enumeration
//! surface — it handles pagination, supports type filtering, and returns structured asset data.
//!
//! # Auth
//!
//! Will use Application Default Credentials (ADC) via `google-cloud-auth`:
//! - `GOOGLE_APPLICATION_CREDENTIALS` env var pointing to a service account JSON key
//! - `gcloud auth application-default login` (developer workstation)
//! - GCE / Cloud Run / GKE workload identity (service-attached SA)
//!
//! No credentials are stored; access is purely at runtime (ADR-004 §5).
//!
//! # Minimal IAM
//!
//! `roles/cloudasset.viewer` on the project(s) to enumerate.
//! The service account also needs `roles/browser` to list projects if `scope` is an organization.
//!
//! # CAI type vs Terraform kind
//!
//! Cloud Asset Inventory returns types like `compute.googleapis.com/Instance`.
//! The Terraform equivalent is `google_compute_instance`. This collector normalises CAI types
//! to the `gcp_<service>_<resource>` pattern (e.g. `gcp_compute_instance`) so IaC-vs-live
//! drift detection can match on the `kind` field.

use wicked_estate_core::{Error, Result};

use super::{CloudCollector, CloudConfig, CloudProvider, CloudResource};

// ── Kind normalisation ────────────────────────────────────────────────

/// Normalise a CAI asset type to the `gcp_<service>_<resource>` convention.
///
/// CAI format: `<service>.googleapis.com/<Type>` — e.g. `compute.googleapis.com/Instance`
/// Output:     `gcp_compute_instance`
///
/// CamelCase in the resource part is split on capital letters and joined with underscores.
///
/// # Examples
///
/// ```
/// # use wicked_estate_extract::cloud::gcp::normalize_gcp_type;
/// assert_eq!(normalize_gcp_type("compute.googleapis.com/Instance"), "gcp_compute_instance");
/// assert_eq!(normalize_gcp_type("storage.googleapis.com/Bucket"), "gcp_storage_bucket");
/// assert_eq!(normalize_gcp_type("iam.googleapis.com/ServiceAccount"), "gcp_iam_service_account");
/// assert_eq!(normalize_gcp_type("container.googleapis.com/Cluster"), "gcp_container_cluster");
/// ```
pub fn normalize_gcp_type(cai_type: &str) -> String {
    // Split on "/" — left side is "service.googleapis.com", right is "ResourceType".
    let (service_part, resource_part) = cai_type.split_once('/').unwrap_or((cai_type, ""));

    // Strip ".googleapis.com" suffix from service name.
    let service = service_part
        .trim_end_matches(".googleapis.com")
        .replace(['.', '-'], "_")
        .to_lowercase();

    // Convert CamelCase resource name to snake_case.
    let resource = camel_to_snake(resource_part);

    if resource.is_empty() {
        format!("gcp_{service}")
    } else {
        format!("gcp_{service}_{resource}")
    }
}

/// Convert a CamelCase string to snake_case.
fn camel_to_snake(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let mut result = String::with_capacity(s.len() + 4);
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_uppercase() {
            if !result.is_empty() {
                // Don't insert underscore if previous char was already underscore or if
                // next char is lowercase (handles "IAMPolicy" → "iam_policy", not "i_a_m_policy").
                let prev_is_sep = result.ends_with('_');
                let next_is_lower = chars.peek().is_some_and(|nc| nc.is_lowercase());
                if !prev_is_sep && next_is_lower {
                    result.push('_');
                }
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

// ── GcpCollector ──────────────────────────────────────────────────────

/// GCP cloud collector using Cloud Asset Inventory.
///
/// Construct via [`super::open_cloud_collector`] (when the `cloud-gcp` feature is enabled).
///
/// **Current status:** `collect` returns `Err(Error::Unsupported(...))` until the full
/// migration from `google-cloud-asset` to `google-cloud-asset-v1 = "1.11.0"` is complete.
#[derive(Debug)]
pub struct GcpCollector {
    /// GCP project ID (from `cfg.profile`). Required for Cloud Asset Inventory scope.
    project_id: Option<String>,
    region: Option<String>,
}

impl GcpCollector {
    /// Create a new `GcpCollector` from a [`CloudConfig`].
    ///
    /// The `cfg.profile` field is interpreted as the GCP project ID.
    pub fn new(cfg: &CloudConfig) -> Result<Self> {
        Ok(Self {
            project_id: cfg.profile.clone(),
            region: cfg.region.clone(),
        })
    }
}

impl CloudCollector for GcpCollector {
    fn provider(&self) -> CloudProvider {
        CloudProvider::Gcp
    }

    fn collect(&self) -> Result<Vec<CloudResource>> {
        // Stub: the google-cloud-asset-v1 = "1.11.0" API (Client::builder() pattern) differs
        // substantially from the prior google-cloud-asset = "0.7" API.  Full wiring is pending.
        // Suppress unused-field warnings by reading the fields.
        let _ = &self.project_id;
        let _ = &self.region;
        Err(Error::Extraction(
            "GCP collector: rebuild with cloud-gcp feature and GOOGLE_APPLICATION_CREDENTIALS — \
             full google-cloud-asset-v1 wiring pending migration from 0.7 API"
                .to_string(),
        ))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────

/// Extract the GCP project ID from a Cloud Asset Inventory full resource name.
///
/// CAI full resource names have the form:
/// `//compute.googleapis.com/projects/{project}/zones/{zone}/instances/{name}`
///
/// This extracts `{project}`.
#[cfg(test)]
fn extract_gcp_project(full_name: &str) -> Option<String> {
    // Look for "/projects/" segment.
    if let Some(idx) = full_name.find("/projects/") {
        let rest = &full_name[idx + "/projects/".len()..];
        let end = rest.find('/').unwrap_or(rest.len());
        let proj = &rest[..end];
        if !proj.is_empty() {
            return Some(proj.to_string());
        }
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_gcp_type_compute_instance() {
        assert_eq!(
            normalize_gcp_type("compute.googleapis.com/Instance"),
            "gcp_compute_instance"
        );
    }

    #[test]
    fn normalize_gcp_type_storage_bucket() {
        assert_eq!(
            normalize_gcp_type("storage.googleapis.com/Bucket"),
            "gcp_storage_bucket"
        );
    }

    #[test]
    fn normalize_gcp_type_iam_service_account() {
        assert_eq!(
            normalize_gcp_type("iam.googleapis.com/ServiceAccount"),
            "gcp_iam_service_account"
        );
    }

    #[test]
    fn normalize_gcp_type_container_cluster() {
        assert_eq!(
            normalize_gcp_type("container.googleapis.com/Cluster"),
            "gcp_container_cluster"
        );
    }

    #[test]
    fn normalize_gcp_type_sql_instance() {
        assert_eq!(
            normalize_gcp_type("sqladmin.googleapis.com/Instance"),
            "gcp_sqladmin_instance"
        );
    }

    #[test]
    fn normalize_gcp_type_pubsub_topic() {
        assert_eq!(
            normalize_gcp_type("pubsub.googleapis.com/Topic"),
            "gcp_pubsub_topic"
        );
    }

    #[test]
    fn normalize_gcp_type_no_googleapis_suffix() {
        // Graceful handling of non-standard type strings.
        assert_eq!(
            normalize_gcp_type("compute/Instance"),
            "gcp_compute_instance"
        );
    }

    #[test]
    fn camel_to_snake_simple() {
        assert_eq!(camel_to_snake("Instance"), "instance");
    }

    #[test]
    fn camel_to_snake_multiword() {
        assert_eq!(camel_to_snake("ServiceAccount"), "service_account");
    }

    #[test]
    fn camel_to_snake_already_lower() {
        assert_eq!(camel_to_snake("bucket"), "bucket");
    }

    #[test]
    fn camel_to_snake_empty() {
        assert_eq!(camel_to_snake(""), "");
    }

    #[test]
    fn extract_gcp_project_standard() {
        assert_eq!(
            extract_gcp_project(
                "//compute.googleapis.com/projects/my-proj-123/zones/us-central1-a/instances/vm-1"
            ),
            Some("my-proj-123".to_string())
        );
    }

    #[test]
    fn extract_gcp_project_no_projects_segment() {
        assert_eq!(
            extract_gcp_project("//storage.googleapis.com/b/my-bucket"),
            None
        );
    }

    #[test]
    fn extract_gcp_project_global_resource() {
        assert_eq!(
            extract_gcp_project("//cloudresourcemanager.googleapis.com/projects/my-project"),
            Some("my-project".to_string())
        );
    }

    #[test]
    fn gcp_collector_returns_unsupported_stub() {
        let cfg = super::super::CloudConfig {
            provider: CloudProvider::Gcp,
            region: None,
            profile: Some("my-project".to_string()),
        };
        let collector = GcpCollector::new(&cfg).expect("GcpCollector::new must not fail");
        let err = collector.collect().expect_err("stub must return Err");
        let msg = err.to_string();
        assert!(
            msg.contains("GCP collector"),
            "error must mention GCP collector; got: {msg}"
        );
    }
}
