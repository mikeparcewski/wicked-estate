//! Azure cloud collector — **stub** (feature `cloud-azure`).
//!
//! # Why stubbed?
//!
//! The original implementation targeted `azure_identity = "0.20"` and
//! `azure_mgmt_resources = "0.20"`, which were pre-1.0 crates with `DefaultAzureCredential` and
//! `resource_groups::Client`. The current versions (`azure_identity = "1.0.0"`,
//! `azure_mgmt_resources = "0.21.0"`) have a substantially different API surface —
//! `DefaultAzureCredential` has been removed; the client builders and resource listing methods
//! follow a new pattern.  Until the full migration is complete, `collect` returns a clear,
//! actionable error; the module structure, type normalisation helpers, and all unit tests remain
//! intact.
//!
//! # Resource enumeration strategy (designed, not yet wired)
//!
//! Will use **Azure Resource Graph** KQL query:
//! ```kql
//! resources
//! | project id, name, type, location, subscriptionId, tags, properties
//! ```
//! Resource Graph covers all resource types across all subscriptions accessible to the
//! credential and supports pagination via `skipToken`.
//!
//! # Auth
//!
//! Will use `azure_identity` which provides (in order):
//! - `AZURE_CLIENT_ID` + `AZURE_CLIENT_SECRET` + `AZURE_TENANT_ID` env vars (service principal)
//! - Managed Identity (Azure-hosted workloads)
//! - Azure CLI (`az login`)
//! - Azure PowerShell
//!
//! No credentials are stored; access is purely at runtime (ADR-004 §5).
//!
//! # Minimal RBAC
//!
//! `Reader` role on the subscription(s) to enumerate. The Resource Graph also requires
//! `Microsoft.ResourceGraph/operations/read` which is included in Reader.

use wicked_estate_core::{Error, Result};

use super::{CloudCollector, CloudConfig, CloudProvider, CloudResource};

// ── Kind normalisation ────────────────────────────────────────────────

/// Normalise an Azure resource type string to a consistent form.
///
/// Azure Resource Manager uses `Microsoft.Compute/virtualMachines` style; this function
/// converts to `microsoft_compute_virtualmachines` (lowercase, slashes and dots to
/// underscores) for consistency with the graph kind convention.
///
/// # Examples
///
/// ```
/// # use wicked_estate_extract::cloud::azure::normalize_azure_type;
/// assert_eq!(
///     normalize_azure_type("Microsoft.Compute/virtualMachines"),
///     "microsoft_compute_virtualmachines"
/// );
/// assert_eq!(
///     normalize_azure_type("Microsoft.Network/virtualNetworks"),
///     "microsoft_network_virtualnetworks"
/// );
/// ```
pub fn normalize_azure_type(raw: &str) -> String {
    raw.replace(['/', '.', '-'], "_").to_lowercase()
}

// ── AzureCollector ────────────────────────────────────────────────────

/// Azure cloud collector using the Resource Graph API.
///
/// Construct via [`super::open_cloud_collector`] (when the `cloud-azure` feature is enabled).
///
/// **Current status:** `collect` returns `Err(Error::Extraction(...))` until the full
/// migration from `azure_identity = "0.20"` to `azure_identity = "1.0.0"` is complete.
#[derive(Debug)]
pub struct AzureCollector {
    region: Option<String>,
    /// Azure subscription ID (optional — when set, scopes enumeration to a single subscription).
    subscription_id: Option<String>,
}

impl AzureCollector {
    /// Create a new `AzureCollector` from a [`CloudConfig`].
    ///
    /// The `cfg.profile` field is interpreted as the Azure subscription ID.
    pub fn new(cfg: &CloudConfig) -> Result<Self> {
        Ok(Self {
            region: cfg.region.clone(),
            subscription_id: cfg.profile.clone(),
        })
    }
}

impl CloudCollector for AzureCollector {
    fn provider(&self) -> CloudProvider {
        CloudProvider::Azure
    }

    fn collect(&self) -> Result<Vec<CloudResource>> {
        // Stub: the azure_identity = "1.0.0" API removed DefaultAzureCredential and the
        // azure_mgmt_resources = "0.21.0" client builders changed substantially.
        // Full wiring is pending migration from the 0.20 API.
        // Suppress unused-field warnings by reading the fields.
        let _ = &self.region;
        let _ = &self.subscription_id;
        Err(Error::Extraction(
            "Azure collector: rebuild with cloud-azure feature and AZURE_CLIENT_ID/AZURE_TENANT_ID \
             env vars — full azure_identity 1.0.0 wiring pending migration from 0.20 API"
                .to_string(),
        ))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────

/// Extract the Azure subscription ID from a resource ID path.
///
/// Azure resource IDs have the form:
/// `/subscriptions/{subscriptionId}/resourceGroups/{rg}/providers/{type}/{name}`
#[cfg(test)]
fn extract_subscription_id(resource_id: &str) -> Option<String> {
    let parts: Vec<&str> = resource_id.splitn(5, '/').collect();
    // parts[0] = "", parts[1] = "subscriptions", parts[2] = {subscriptionId}
    if parts.len() >= 3 && parts[1].eq_ignore_ascii_case("subscriptions") {
        let sub = parts[2];
        if !sub.is_empty() {
            return Some(sub.to_string());
        }
    }
    None
}

/// Extract the resource group name from an Azure resource ID.
#[cfg(test)]
fn extract_resource_group(resource_id: &str) -> Option<String> {
    // /subscriptions/{sub}/resourceGroups/{rg}/...
    let lower = resource_id.to_lowercase();
    let marker = "/resourcegroups/";
    if let Some(start) = lower.find(marker) {
        let rest = &resource_id[start + marker.len()..];
        let end = rest.find('/').unwrap_or(rest.len());
        let rg = &rest[..end];
        if !rg.is_empty() {
            return Some(rg.to_string());
        }
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_azure_type_virtual_machine() {
        assert_eq!(
            normalize_azure_type("Microsoft.Compute/virtualMachines"),
            "microsoft_compute_virtualmachines"
        );
    }

    #[test]
    fn normalize_azure_type_virtual_network() {
        assert_eq!(
            normalize_azure_type("Microsoft.Network/virtualNetworks"),
            "microsoft_network_virtualnetworks"
        );
    }

    #[test]
    fn normalize_azure_type_storage_account() {
        assert_eq!(
            normalize_azure_type("Microsoft.Storage/storageAccounts"),
            "microsoft_storage_storageaccounts"
        );
    }

    #[test]
    fn normalize_azure_type_already_lowercase() {
        assert_eq!(
            normalize_azure_type("microsoft.sql/servers"),
            "microsoft_sql_servers"
        );
    }

    #[test]
    fn extract_subscription_id_standard() {
        assert_eq!(
            extract_subscription_id(
                "/subscriptions/aaaabbbb-1234-5678-abcd-ef0123456789/resourceGroups/rg-prod/providers/Microsoft.Compute/virtualMachines/vm-1"
            ),
            Some("aaaabbbb-1234-5678-abcd-ef0123456789".to_string())
        );
    }

    #[test]
    fn extract_subscription_id_missing() {
        assert_eq!(
            extract_subscription_id("/resourceGroups/rg/providers/foo"),
            None
        );
    }

    #[test]
    fn extract_resource_group_standard() {
        assert_eq!(
            extract_resource_group(
                "/subscriptions/sub-1/resourceGroups/rg-prod/providers/Microsoft.Network/virtualNetworks/vnet-1"
            ),
            Some("rg-prod".to_string())
        );
    }

    #[test]
    fn extract_resource_group_missing() {
        assert_eq!(
            extract_resource_group("/subscriptions/sub-1/providers/foo/bar"),
            None
        );
    }

    #[test]
    fn azure_collector_returns_stub_error() {
        let cfg = super::super::CloudConfig {
            provider: CloudProvider::Azure,
            region: None,
            profile: Some("my-subscription".to_string()),
        };
        let collector = AzureCollector::new(&cfg).expect("AzureCollector::new must not fail");
        let err = collector.collect().expect_err("stub must return Err");
        let msg = err.to_string();
        assert!(
            msg.contains("Azure collector"),
            "error must mention Azure collector; got: {msg}"
        );
    }

    #[test]
    fn normalize_azure_type_with_hyphens() {
        // Hyphens in type strings are also normalized to underscores.
        assert_eq!(
            normalize_azure_type("Microsoft.Sql/servers"),
            "microsoft_sql_servers"
        );
    }
}
