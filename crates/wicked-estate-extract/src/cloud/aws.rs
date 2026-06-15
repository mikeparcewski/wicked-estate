//! AWS cloud collector — **real impl** (feature `cloud-aws`).
//!
//! # Resource enumeration strategy
//!
//! 1. **Primary:** AWS Resource Explorer v2 `SearchAllResources` — broad enumeration across all
//!    enabled resource types. Requires Resource Explorer to be enabled in the account (see below).
//! 2. **Supplemental:** EC2 `DescribeInstances`, `DescribeVpcs`, `DescribeSecurityGroups` for
//!    relationship data (depends_on edges) that Resource Explorer does not surface.
//! 3. **IAM:** `ListRoles` for IAM role enumeration.
//!
//! # Resource Explorer pre-requisite
//!
//! Resource Explorer v2 must be enabled in the AWS account and region before using this collector.
//! Enable it via the AWS Console (`Resource Explorer` → `Turn on Resource Explorer`) or:
//!
//! ```sh
//! aws resource-explorer-2 create-index --type LOCAL --region us-east-1
//! aws resource-explorer-2 create-view --view-name default --included-filters '[]'
//! ```
//!
//! If Resource Explorer is not enabled, `collect()` returns a clear `Error::Extraction` with
//! instructions.
//!
//! # Auth
//!
//! Uses `aws_config::load_from_env().await` which respects (in order):
//! - `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` / `AWS_SESSION_TOKEN` env vars
//! - `AWS_PROFILE` env var / `~/.aws/credentials` named profile
//! - EC2 Instance Metadata Service (IMDS)
//! - ECS task role / IRSA (IAM Roles for Service Accounts)
//!
//! No credentials are stored; access is purely at runtime (ADR-004 §5).
//!
//! # Minimal IAM policy
//!
//! ```json
//! {
//!   "Version": "2012-10-17",
//!   "Statement": [
//!     { "Effect": "Allow", "Action": ["resource-explorer-2:Search", "resource-explorer-2:GetDefaultView"],
//!       "Resource": "*" },
//!     { "Effect": "Allow", "Action": ["ec2:DescribeInstances", "ec2:DescribeVpcs",
//!       "ec2:DescribeSecurityGroups", "iam:ListRoles"],
//!       "Resource": "*" }
//!   ]
//! }
//! ```

use std::collections::HashMap;

use aws_config::BehaviorVersion;
use aws_sdk_ec2::Client as Ec2Client;
use aws_sdk_iam::Client as IamClient;
use aws_sdk_resourceexplorer2::Client as ReClient;
use wicked_estate_core::{Error, Result};

use super::{CloudCollector, CloudConfig, CloudProvider, CloudResource};

// ── Kind normalisation ────────────────────────────────────────────────

/// Normalise an AWS resource type string to snake_case for graph kind consistency.
///
/// Resource Explorer returns types like `AWS::EC2::Instance`; this function converts them to
/// `aws_ec2_instance` — the same convention used by Terraform resource types, which simplifies
/// IaC-vs-live drift detection.
///
/// # Examples
///
/// ```
/// # use wicked_estate_extract::cloud::aws::normalize_aws_type;
/// assert_eq!(normalize_aws_type("AWS::EC2::Instance"), "aws_ec2_instance");
/// assert_eq!(normalize_aws_type("AWS::S3::Bucket"), "aws_s3_bucket");
/// assert_eq!(normalize_aws_type("AWS::IAM::Role"), "aws_iam_role");
/// assert_eq!(normalize_aws_type("ec2:instance"), "ec2_instance");
/// ```
pub fn normalize_aws_type(raw: &str) -> String {
    let normalised = if raw.starts_with("AWS::") {
        format!("aws_{}", &raw["AWS::".len()..])
    } else {
        raw.to_string()
    };
    normalised
        .replace("::", "_")
        .replace([':', '-'], "_")
        .to_lowercase()
}

// ── AwsCollector ──────────────────────────────────────────────────────

/// AWS cloud collector using Resource Explorer v2 + targeted describe APIs.
///
/// Construct via [`super::open_cloud_collector`] (when the `cloud-aws` feature is enabled).
#[derive(Debug)]
pub struct AwsCollector {
    region: Option<String>,
    profile: Option<String>,
}

impl AwsCollector {
    /// Create a new `AwsCollector` from a [`CloudConfig`].
    ///
    /// Validates the config but does not make any network calls. Network calls happen in
    /// [`collect`][Self::collect].
    pub fn new(cfg: &CloudConfig) -> Result<Self> {
        Ok(Self {
            region: cfg.region.clone(),
            profile: cfg.profile.clone(),
        })
    }

    /// Load AWS SDK config using the ambient credential chain.
    ///
    /// Respects `AWS_PROFILE` / profile hint, region hint, and all standard SDK credential sources.
    fn load_sdk_config(&self) -> aws_config::SdkConfig {
        // Build async config, then block — the trait is sync.
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let mut loader = aws_config::defaults(BehaviorVersion::latest());

                if let Some(ref region) = self.region {
                    loader = loader.region(aws_sdk_ec2::config::Region::new(region.clone()));
                }

                if let Some(ref profile) = self.profile {
                    loader = loader.profile_name(profile);
                }

                loader.load().await
            })
        })
    }

    /// Enumerate resources via Resource Explorer v2 `SearchAllResources`.
    ///
    /// Returns all resources accessible to the ambient credentials. Paginates automatically
    /// until `next_token` is absent.
    fn collect_via_resource_explorer(
        &self,
        sdk_cfg: &aws_config::SdkConfig,
    ) -> Result<Vec<CloudResource>> {
        let client = ReClient::new(sdk_cfg);

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let mut resources: Vec<CloudResource> = Vec::new();
                let mut next_token: Option<String> = None;

                loop {
                    let mut req = client.search().query_string("*");
                    if let Some(ref tok) = next_token {
                        req = req.next_token(tok);
                    }

                    let resp = req.send().await.map_err(|e| {
                        let msg = e.to_string();
                        if msg.contains("ResourceNotFoundException")
                            || msg.contains("ResourceExplorer")
                        {
                            Error::Extraction(format!(
                                "AWS Resource Explorer is not enabled in this account/region. \
                                 Enable it with: \
                                 `aws resource-explorer-2 create-index --type LOCAL` \
                                 then retry. SDK error: {msg}"
                            ))
                        } else {
                            Error::Extraction(format!("AWS Resource Explorer search failed: {msg}"))
                        }
                    })?;

                    for r in resp.resources() {
                        let id = r.arn().unwrap_or_default().to_string();
                        if id.is_empty() {
                            continue;
                        }

                        let raw_type = r.resource_type().unwrap_or_default();
                        let kind = normalize_aws_type(raw_type);
                        let region = r.region().map(str::to_string);

                        let mut attributes: Vec<(String, String)> = Vec::new();
                        if let Some(svc) = r.service() {
                            attributes.push(("service".to_string(), svc.to_string()));
                        }
                        if let Some(raw_type_str) = r.resource_type() {
                            attributes
                                .push(("resource_type_raw".to_string(), raw_type_str.to_string()));
                        }

                        // Extract tags from resource properties if available.
                        let tags: HashMap<String, String> = HashMap::new();
                        for prop in r.properties() {
                            if let Some(name) = prop.name() {
                                if let Some(val) = prop.data() {
                                    let json_str = format!("{val:?}");
                                    attributes.push((format!("prop.{name}"), json_str));
                                }
                            }
                        }

                        // Extract account_id from ARN (arn:aws:s3:::bucket has no account but
                        // arn:aws:iam::123456789012:role/name has account at index 4).
                        let account_id = extract_account_id_from_arn(&id);

                        resources.push(CloudResource {
                            id,
                            kind,
                            region,
                            depends_on: vec![],
                            attributes,
                            account_id,
                            name: None, // populated by supplemental describe calls
                            tags,
                        });
                    }

                    next_token = resp.next_token().map(str::to_string);
                    if next_token.is_none() {
                        break;
                    }
                }

                Ok(resources)
            })
        })
    }

    /// Supplement Resource Explorer results with EC2 relationship data.
    ///
    /// Resource Explorer surfaces the existence of EC2 resources but not their internal
    /// relationships (instance → VPC, instance → security group, etc.). This pass queries
    /// EC2 describe APIs and appends `depends_on` edges + display names.
    fn supplement_ec2_relationships(
        &self,
        sdk_cfg: &aws_config::SdkConfig,
        resources: &mut [CloudResource],
    ) {
        let client = Ec2Client::new(sdk_cfg);

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Describe instances to get VPC/subnet/SG relationships.
                let mut next_token: Option<String> = None;
                loop {
                    let mut req = client.describe_instances();
                    if let Some(ref tok) = next_token {
                        req = req.next_token(tok);
                    }
                    let resp = match req.send().await {
                        Ok(r) => r,
                        Err(_) => break,
                    };

                    for reservation in resp.reservations() {
                        for instance in reservation.instances() {
                            let Some(instance_id) = instance.instance_id() else {
                                continue;
                            };
                            // Build ARN-like id to match Resource Explorer output.
                            let region = instance
                                .placement()
                                .and_then(|p| p.availability_zone())
                                .map(|az| az.trim_end_matches(|c: char| c.is_alphabetic()))
                                .unwrap_or("unknown");

                            let account_id = resources
                                .iter()
                                .find(|r| r.id.contains(instance_id))
                                .and_then(|r| r.account_id.clone())
                                .unwrap_or_default();

                            let arn =
                                format!("arn:aws:ec2:{region}:{account_id}:instance/{instance_id}");

                            let mut deps: Vec<String> = Vec::new();
                            if let Some(vpc_id) = instance.vpc_id() {
                                deps.push(format!(
                                    "arn:aws:ec2:{region}:{account_id}:vpc/{vpc_id}"
                                ));
                            }
                            for sg in instance.security_groups() {
                                if let Some(sg_id) = sg.group_id() {
                                    deps.push(format!(
                                        "arn:aws:ec2:{region}:{account_id}:security-group/{sg_id}"
                                    ));
                                }
                            }

                            // Extract Name tag.
                            let name = instance
                                .tags()
                                .iter()
                                .find(|t| t.key().is_some_and(|k| k == "Name"))
                                .and_then(|t| t.value())
                                .map(str::to_string);

                            // Extract all tags.
                            let tags: HashMap<String, String> = instance
                                .tags()
                                .iter()
                                .filter_map(|t| {
                                    Some((t.key()?.to_string(), t.value()?.to_string()))
                                })
                                .collect();

                            if let Some(r) = resources.iter_mut().find(|r| r.id == arn) {
                                r.depends_on.extend(deps);
                                if r.name.is_none() {
                                    r.name = name;
                                }
                                r.tags.extend(tags);
                            }
                        }
                    }

                    next_token = resp.next_token().map(str::to_string);
                    if next_token.is_none() {
                        break;
                    }
                }
            })
        });
    }

    /// Supplement with IAM role display names and tags.
    fn supplement_iam_roles(
        &self,
        sdk_cfg: &aws_config::SdkConfig,
        resources: &mut [CloudResource],
    ) {
        let client = IamClient::new(sdk_cfg);

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let mut marker: Option<String> = None;
                loop {
                    let mut req = client.list_roles();
                    if let Some(ref m) = marker {
                        req = req.marker(m);
                    }
                    let resp = match req.send().await {
                        Ok(r) => r,
                        Err(_) => break,
                    };

                    for role in resp.roles() {
                        let arn = role.arn();
                        let name = role.role_name().to_string();
                        if let Some(r) = resources.iter_mut().find(|r| r.id == arn) {
                            if r.name.is_none() {
                                r.name = Some(name);
                            }
                        }
                    }

                    if resp.is_truncated() {
                        marker = resp.marker().map(str::to_string);
                    } else {
                        break;
                    }
                }
            })
        });
    }
}

impl CloudCollector for AwsCollector {
    fn provider(&self) -> CloudProvider {
        CloudProvider::Aws
    }

    fn collect(&self) -> Result<Vec<CloudResource>> {
        let sdk_cfg = self.load_sdk_config();
        let mut resources = self.collect_via_resource_explorer(&sdk_cfg)?;

        // Supplement with EC2 and IAM relationship data; errors are best-effort (non-fatal).
        self.supplement_ec2_relationships(&sdk_cfg, &mut resources);
        self.supplement_iam_roles(&sdk_cfg, &mut resources);

        Ok(resources)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────

/// Extract the AWS account ID from an ARN string.
///
/// ARN format: `arn:partition:service:region:account-id:resource`
/// Account-id is the 5th colon-delimited field (index 4). Some ARNs (e.g. S3 buckets
/// `arn:aws:s3:::bucket`) have an empty account-id field; this returns `None` in that case.
fn extract_account_id_from_arn(arn: &str) -> Option<String> {
    let parts: Vec<&str> = arn.splitn(6, ':').collect();
    if parts.len() >= 5 {
        let acct = parts[4];
        if !acct.is_empty() {
            return Some(acct.to_string());
        }
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_aws_type_ec2_instance() {
        assert_eq!(normalize_aws_type("AWS::EC2::Instance"), "aws_ec2_instance");
    }

    #[test]
    fn normalize_aws_type_s3_bucket() {
        assert_eq!(normalize_aws_type("AWS::S3::Bucket"), "aws_s3_bucket");
    }

    #[test]
    fn normalize_aws_type_iam_role() {
        assert_eq!(normalize_aws_type("AWS::IAM::Role"), "aws_iam_role");
    }

    #[test]
    fn normalize_aws_type_rds_db_instance() {
        assert_eq!(
            normalize_aws_type("AWS::RDS::DBInstance"),
            "aws_rds_dbinstance"
        );
    }

    #[test]
    fn normalize_aws_type_already_lowercase() {
        assert_eq!(normalize_aws_type("ec2:instance"), "ec2_instance");
    }

    #[test]
    fn normalize_aws_type_hyphen_in_type() {
        assert_eq!(
            normalize_aws_type("AWS::ElasticLoadBalancingV2::LoadBalancer"),
            "aws_elasticloadbalancingv2_loadbalancer"
        );
    }

    #[test]
    fn extract_account_id_from_standard_arn() {
        assert_eq!(
            extract_account_id_from_arn("arn:aws:iam::123456789012:role/my-role"),
            Some("123456789012".to_string())
        );
    }

    #[test]
    fn extract_account_id_s3_bucket_no_account() {
        assert_eq!(extract_account_id_from_arn("arn:aws:s3:::my-bucket"), None);
    }

    #[test]
    fn extract_account_id_ec2_instance() {
        assert_eq!(
            extract_account_id_from_arn("arn:aws:ec2:us-east-1:123456789012:instance/i-0abc123"),
            Some("123456789012".to_string())
        );
    }

    #[test]
    fn extract_account_id_malformed_arn() {
        assert_eq!(extract_account_id_from_arn("not-an-arn"), None);
    }
}
