# Cloud Collector Implementation Plan

**Purpose:** Drive the W10.2 real SDK implementation for AWS, Azure, and GCP `CloudCollector`s.
**Status:** Interface + mock built; real impls designed-not-built (creds-blocked).
**ADR:** ADR-004 — Infrastructure & Estate Mapping.
**Date authored:** 2026-06-15.

---

## 1. CloudResource struct — field sufficiency audit

The current `CloudResource` struct in `crates/wicked-estate-extract/src/cloud.rs`:

```rust
pub struct CloudResource {
    pub id: String,          // stable physical identity (ARN / Azure resource id / GCP full-name)
    pub kind: String,        // provider resource type string
    pub region: Option<String>,
    pub depends_on: Vec<String>, // physical ids of depended-upon resources
    pub attributes: Vec<(String, String)>, // flattened observable attributes; secrets must be redacted
}
```

### Gaps vs real SDK output

| Missing field | Rationale | Recommended fix |
|---|---|---|
| `account_id` / `subscription_id` / `project_id` | Multi-account/project graphs require a scope discriminator; otherwise two accounts with identically-named resources collide on `(kind, name)` during drift. The physical `id` is scoped (ARN embeds account; Azure resource id embeds subscription; GCP full name embeds project), so the drift key already works — but having `account_id` as a first-class field lets collectors expose it explicitly and lets queries filter on it cheaply. | Add `pub account_id: Option<String>` |
| `tags` | All three providers surface key-value tags as a first-class concept, distinct from configuration attributes. Tags are how teams identify ownership/environment; they are also the fallback drift key when the physical id is missing. Lumping tags into `attributes` via `attr.tag.<key>` prefixes is workable but lossy (no way to distinguish a tag from a config attribute). | Add `pub tags: Vec<(String, String)>` and write them as `metadata["tag.<key>"]` in `resource_to_node`, separate from `metadata["attr.<key>"]` |
| `name` | Some resources have a human-readable logical name separate from the physical id (e.g. an EC2 instance has ARN as id but a `Name` tag or a value from the `name` attribute). Having a `name` field avoids requiring callers to dig into `attributes` to reconstruct a display name for reports. | Add `pub name: Option<String>` and write it to `metadata["name"]` |
| `collector_timestamp` | ADR-004 §5 requires every collected node to record an audit timestamp. Current impl records `metadata["collector"]` via convention but there is no timestamp field. | Add to `resource_to_node`: insert `metadata["collected_at"] = <RFC3339 timestamp>` at collection time in each real impl; no struct field needed (ephemeral). |

### No structural changes needed for graph compatibility

`cloud_resources_to_nodes` already produces `NodeKind::Other("resource")` nodes with `metadata["origin"] = "live"` and `metadata["type"] = resource.kind`, which is exactly what `estate_drift` keys on. No store or drift detector changes are required for any field additions above.

**Recommended minimal additions to `CloudResource`:**

```rust
pub struct CloudResource {
    pub id: String,
    pub kind: String,
    pub region: Option<String>,
    pub account_id: Option<String>,    // NEW: account/subscription/project scope
    pub name: Option<String>,          // NEW: human-readable logical name
    pub tags: Vec<(String, String)>,   // NEW: provider tags, separated from config attrs
    pub depends_on: Vec<String>,
    pub attributes: Vec<(String, String)>,
}
```

And update `resource_to_node` to:
- Write `account_id` → `metadata["account_id"]`
- Write `name` → `metadata["name"]`
- Write tags as `metadata["tag.<key>"] = value` (namespace distinct from `attr.*`)

These additions are purely additive and backward-compatible with `MockCloudCollector` (existing tests will compile; default `..Default::default()` fills the new fields).

---

## 2. AWS Collector

### Recommended approach: AWS Resource Explorer v2 (primary) + targeted `describe` APIs (secondary)

The module-level doc comment already names `aws-sdk-resource-explorer2` + `aws-config` as the intended path. This is correct. AWS Resource Explorer v2 provides a single paginated `Search` API that returns all resources across all enabled regions and services in an account (or org), minimizing the number of API calls. Use it as the primary enumeration source, then optionally enrich specific resource types with targeted `describe` calls for relationship data (security group rules, subnet associations, etc.) that Resource Explorer omits.

**Alternative: AWS Config `ListDiscoveredResources` / `BatchGetResourceConfig`**
- Pro: returns the full configuration JSON for each resource, including relationships.
- Con: requires AWS Config to be enabled and recording in every region; slower; costs more per call.
- Verdict: Use Resource Explorer for enumeration + Config for enrichment on resource types where relationships matter (VPC, security groups). Both are read-only.

### Rust crates

| Crate | Version (as of 2026-06) | Purpose |
|---|---|---|
| `aws-config` | `^1.5` | Ambient credential chain loading (env / profile / ECS / IRSA / IMDSv2) |
| `aws-sdk-resourceexplorer2` | `^1.0` | Resource Explorer v2 `Search` API (cross-region, cross-service enumeration) |
| `aws-sdk-ec2` | `^1.0` | `DescribeInstances`, `DescribeVpcs`, `DescribeSubnets`, `DescribeSecurityGroups`, `DescribeRouteTables` (for relationship extraction) |
| `aws-sdk-iam` | `^1.0` | `ListRoles`, `ListPolicies`, `GetRolePolicy` (for IAM relationship edges) |
| `aws-sdk-config` | `^1.0` | `ListDiscoveredResources` / `BatchGetResourceConfig` (optional enrichment) |

All `aws-sdk-*` crates share the same `aws-config` credential chain. No additional auth crate is needed.

### Services and resource types to collect

| AWS Service | Resource types to collect | `CloudResource.kind` value | Key relationships (`depends_on`) |
|---|---|---|---|
| EC2 | Instances | `aws_instance` | VPC, subnet, security groups, IAM instance profile |
| EC2 | VPCs | `aws_vpc` | (root; others depend on it) |
| EC2 | Subnets | `aws_subnet` | VPC, availability zone |
| EC2 | Security Groups | `aws_security_group` | VPC |
| EC2 | Route Tables | `aws_route_table` | VPC, subnets |
| EC2 | Internet Gateways | `aws_internet_gateway` | VPC |
| EC2 | NAT Gateways | `aws_nat_gateway` | Subnet, Elastic IP |
| EC2 | Elastic Load Balancers (ELBv2) | `aws_lb` | VPC, subnets, security groups |
| EC2 | Auto Scaling Groups | `aws_autoscaling_group` | Launch template/config, VPC subnets |
| S3 | Buckets | `aws_s3_bucket` | (none typical; bucket policy may ref IAM) |
| RDS | DB Instances | `aws_db_instance` | VPC, security groups, subnet group |
| RDS | DB Clusters (Aurora) | `aws_rds_cluster` | VPC, security groups, subnet group |
| Lambda | Functions | `aws_lambda_function` | IAM role, VPC (if configured), event sources |
| ECS | Clusters | `aws_ecs_cluster` | (root) |
| ECS | Services | `aws_ecs_service` | Cluster, task definition, load balancer, subnets |
| ECS | Task Definitions | `aws_ecs_task_definition` | IAM execution role, IAM task role |
| EKS | Clusters | `aws_eks_cluster` | VPC, subnets, security groups, IAM role |
| EKS | Node Groups | `aws_eks_node_group` | EKS cluster, subnets, IAM role |
| IAM | Roles | `aws_iam_role` | Attached policies |
| IAM | Policies (customer managed) | `aws_iam_policy` | (referenced by roles/users) |
| API Gateway | REST APIs | `aws_api_gateway_rest_api` | (may ref Lambda) |
| API Gateway | HTTP APIs (v2) | `aws_apigatewayv2_api` | Lambda integrations |
| CloudFormation | Stacks | `aws_cloudformation_stack` | Resources within the stack |
| DynamoDB | Tables | `aws_dynamodb_table` | (none typical) |
| SQS | Queues | `aws_sqs_queue` | (none typical) |
| SNS | Topics | `aws_sns_topic` | SQS subscriptions, Lambda subscriptions |
| ElastiCache | Clusters/Replication Groups | `aws_elasticache_replication_group` | VPC, subnet group, security groups |
| Secrets Manager | Secrets | `aws_secretsmanager_secret` | KMS key |
| KMS | Keys (customer managed) | `aws_kms_key` | (root) |
| CloudFront | Distributions | `aws_cloudfront_distribution` | S3 origin, Lambda@Edge |

### Exact API calls

**Phase 1 — Enumeration via Resource Explorer v2** (single API, all services/regions):

```
resourceexplorer2::Client::search(SearchInput {
    query_string: "*",          // match all resources
    max_results: 1000,          // paginate with next_token
    ..
})
```

Returns: `resource_arn`, `resource_type`, `region`, `account_id`, `properties` (limited). This gives ARN + type + region for every resource Explorer knows about. Resource Explorer must be enabled in the account (one-time setup by the user; not done by the collector).

**Phase 2 — Selective enrichment for relationship extraction** (targeted `describe` calls):

Only call the targeted `describe` APIs for resource types where `depends_on` relationships are needed for drift accuracy:

```
ec2::Client::describe_instances(...)          → security_groups, subnet_id, vpc_id, iam_instance_profile
ec2::Client::describe_vpcs(...)               → no relationships (root)
ec2::Client::describe_subnets(...)            → vpc_id
ec2::Client::describe_security_groups(...)    → vpc_id
iam::Client::list_roles(...)                  → attached_policies (via list_attached_role_policies)
lambda::Client::list_functions(...)           → role, vpc_config (subnet_ids, security_group_ids)
eks::Client::list_clusters(...) + describe_cluster(...)  → resources_vpc_config
```

**Phase 3 — Pagination** (mandatory for all calls):

All Resource Explorer and SDK calls return paginated results. The collector must loop until `next_token` is `None`.

### `CloudResource` field mapping for AWS

| `CloudResource` field | Source in API response |
|---|---|
| `id` | `resource_arn` from Resource Explorer or the ARN from `describe` output |
| `kind` | Resource Explorer `resource_type` (e.g. `AWS::EC2::Instance`) → normalize to `aws_instance` (lowercase, underscore) by stripping `AWS::` prefix and replacing `::` with `_` and lowercasing |
| `region` | Resource Explorer `region` field |
| `account_id` | Resource Explorer `account_id` field |
| `name` | `Name` tag if present; fallback to last segment of ARN |
| `tags` | Resource Explorer `properties.tags` or `describe` API `Tags[]` array |
| `depends_on` | Constructed from enrichment calls (e.g. `vpc_id`, `subnet_ids`, `role_arn`) — look up the ARN of each referenced resource |
| `attributes` | Key configuration attributes from the `describe` response (e.g. `instance_type`, `ami_id`, `state`); redact any field with "password", "secret", "key" in the name |

**Kind normalization function** (implement in the AWS collector module):

```rust
fn normalize_aws_type(resource_type: &str) -> String {
    // "AWS::EC2::Instance" → "aws_ec2_instance"
    // "AWS::S3::Bucket"    → "aws_s3_bucket"
    resource_type
        .strip_prefix("AWS::")
        .unwrap_or(resource_type)
        .to_lowercase()
        .replace("::", "_")
}
```

This produces `kind` values that match Terraform's convention (`aws_ec2_instance`, `aws_s3_bucket`) — consistent with tfstate-originated nodes so drift can key on `(kind, name)`.

### Auth pattern

Use `aws_config::load_from_env()` (or `from_env().profile_name(profile).load().await` when `CloudConfig.profile` is set). This resolves credentials through the standard chain in order:

1. `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` / `AWS_SESSION_TOKEN` environment variables
2. Named profile in `~/.aws/credentials` / `~/.aws/config`
3. ECS container credential endpoint (`AWS_CONTAINER_CREDENTIALS_RELATIVE_URI`)
4. EC2/EKS IMDS v2 workload identity (IRSA / Pod Identity)

No credentials are stored. The `CloudConfig.profile` field maps to `profile_name(...)` — a pointer, not a secret.

The collector must request only the read-only policy set documented in the module comment: `AWSResourceExplorerReadOnlyAccess` + readonly policies for any `describe` APIs used.

---

## 3. Azure Collector

### Recommended approach: Azure Resource Graph (primary)

Azure Resource Graph's `resources | project ...` KQL query is the single most efficient way to enumerate all resources across all subscriptions and resource groups accessible to the credential. It returns `id`, `type`, `location`, `subscriptionId`, `resourceGroup`, `tags`, and `properties` in a single paginated response.

### Rust crates

The Azure SDK for Rust (`azure-sdk-for-rust`) uses the `azure_*` crate family. As of mid-2026, the relevant crates are:

| Crate | Version | Purpose |
|---|---|---|
| `azure_identity` | `^0.20` | `DefaultAzureCredential` — ambient chain (env, managed identity, Azure CLI, VS Code) |
| `azure_mgmt_resources` | `^0.20` | Azure Resource Graph `resources` table queries |
| `azure_core` | `^0.20` | Shared pagination types, `ClientOptions` |

**Important:** The `azure_mgmt_resources` crate provides Resource Graph via `ResourceGraphClient`. Alternatively, the `azure_mgmt_*` service-specific clients (e.g. `azure_mgmt_compute`, `azure_mgmt_network`) can be used for selective enrichment, analogous to the AWS Phase 2 approach.

**Version note:** The azure-sdk-for-rust crate versioning is still rapidly evolving (pre-1.0 in most service crates). Pin to a specific minor version and document it. The `azure_identity` crate is more stable.

### Services and resource types to collect

| Azure Service | Resource types to collect | `CloudResource.kind` value | Key relationships |
|---|---|---|---|
| Compute | Virtual Machines | `microsoft.compute/virtualmachines` | VNet NIC, NSG, availability set, managed disk |
| Compute | Virtual Machine Scale Sets | `microsoft.compute/virtualmachinescalesets` | VNet subnet, NSG |
| Compute | Managed Disks | `microsoft.compute/disks` | VM (if attached) |
| Storage | Storage Accounts | `microsoft.storage/storageaccounts` | VNet service endpoint (if configured) |
| Network | Virtual Networks | `microsoft.network/virtualnetworks` | (root) |
| Network | Subnets | `microsoft.network/virtualnetworks/subnets` | VNet, NSG, route table |
| Network | Network Security Groups | `microsoft.network/networksecuritygroups` | (root; associated to NIC or subnet) |
| Network | Load Balancers | `microsoft.network/loadbalancers` | Backend pool (VMs/VMSS), frontend IP |
| Network | Application Gateways | `microsoft.network/applicationgateways` | Subnet, backend pool |
| Network | Public IP Addresses | `microsoft.network/publicipaddresses` | NIC or Load Balancer frontend |
| Databases | Azure SQL Servers | `microsoft.sql/servers` | VNet service endpoint or private endpoint |
| Databases | Azure SQL Databases | `microsoft.sql/servers/databases` | SQL Server |
| Databases | Cosmos DB Accounts | `microsoft.documentdb/databaseaccounts` | VNet (if configured) |
| Databases | Azure Database for PostgreSQL | `microsoft.dbforpostgresql/servers` | VNet |
| Functions | Function Apps | `microsoft.web/sites` (kind=functionapp) | Storage account, App Service Plan, VNet integration |
| App Service | Web Apps | `microsoft.web/sites` (kind=app) | App Service Plan, VNet integration |
| App Service | App Service Plans | `microsoft.web/serverfarms` | (root) |
| Containers | AKS Clusters | `microsoft.containerservice/managedclusters` | VNet subnet, node resource group |
| Containers | Container Registries | `microsoft.containerregistry/registries` | (root) |
| Key Vault | Key Vaults | `microsoft.keyvault/vaults` | VNet (if configured), access policies → AAD objects |
| Service Bus | Namespaces | `microsoft.servicebus/namespaces` | (root) |
| Event Hubs | Namespaces | `microsoft.eventhub/namespaces` | (root) |
| API Management | Services | `microsoft.apimanagement/service` | VNet (if configured), backends |

### Exact API call

**Single Resource Graph query (KQL):**

```kusto
resources
| project id, name, type, location, subscriptionId, resourceGroup, tags, properties
| order by type
```

Executed via `ResourceGraphClient::resources(...)` with pagination (`skip_token`). This is a POST request to `management.azure.com/providers/Microsoft.ResourceGraph/resources`.

For relationship extraction, `properties` contains the full resource configuration JSON. Parse `properties.networkProfile.networkInterfaces[*].id`, `properties.subnet.id`, `properties.virtualNetworkSubnetResourceId`, etc. to build `depends_on` lists.

### `CloudResource` field mapping for Azure

| `CloudResource` field | Source in Resource Graph response |
|---|---|
| `id` | Resource Graph `id` field (full Azure resource id: `/subscriptions/.../providers/.../resourceName`) |
| `kind` | Resource Graph `type` lowercased (e.g. `microsoft.compute/virtualmachines`) |
| `region` | Resource Graph `location` (e.g. `westeurope`, `eastus`) |
| `account_id` | Resource Graph `subscriptionId` |
| `name` | Resource Graph `name` field |
| `tags` | Resource Graph `tags` object → flatten to `Vec<(String, String)>` |
| `depends_on` | Parse from `properties` JSON: subnet ids, NIC ids, NSG ids, etc. |
| `attributes` | Flattened subset of `properties` JSON (exclude secrets: `connectionString`, `primaryKey`, `secondaryKey`, `adminPassword`) |

**Redaction:** Before flattening `properties` into `attributes`, recursively scan keys and skip any field whose lowercased name contains: `password`, `key`, `secret`, `connectionstring`, `credentials`, `token`, `sas`.

### Auth pattern

Use `azure_identity::DefaultAzureCredential`. Resolution order:

1. `AZURE_CLIENT_ID` + `AZURE_CLIENT_SECRET` + `AZURE_TENANT_ID` (service principal env vars)
2. `AZURE_CLIENT_CERTIFICATE_PATH` (certificate-based SP)
3. Workload Identity (Kubernetes federated identity via `AZURE_FEDERATED_TOKEN_FILE`)
4. Managed Identity (Azure VM / VMSS / App Service / AKS pod identity)
5. Azure CLI token (`az login`)
6. Azure PowerShell token

`CloudConfig.profile` maps to `subscription_id` — a resource scope pointer, not a secret. `CloudConfig.region` can scope the Resource Graph query to a specific location.

The required RBAC role is `Reader` assigned at the subscription (or management group) scope. This is purely read-only and grants no mutation capability.

---

## 4. GCP Collector

### Recommended approach: Cloud Asset Inventory (CAI)

GCP Cloud Asset Inventory's `SearchAllResources` API returns all assets (resources + policies) in a project, folder, or organization with a single paginated call. It returns `name` (full resource name), `assetType`, `location`, `project`, `labels`, and `additionalAttributes`.

### Rust crates

The GCP Rust ecosystem is fragmented. Recommended options:

| Option | Crates | Maturity |
|---|---|---|
| **Primary (recommended):** `google-cloud-asset` | `google-cloud-asset = "0.10"` (part of `google-cloud-rust` / `googleapis-tonic` family) | Usable; uses tonic gRPC |
| **Alternative: REST via `reqwest`** | `reqwest`, `google-oauth2` or `yup-oauth2` | Higher control; lower abstraction |
| **Alternative: `gcloud_sdk`** | `gcloud_sdk = "0.24"` (wraps `gcloud` binary via CLI or gRPC) | Simpler auth; less idiomatic |

**Recommended path:** Use `google-cloud-asset` with `google-cloud-auth` for Application Default Credentials. This is the most idiomatic Rust approach.

```toml
google-cloud-asset = "0.10"
google-cloud-auth = "0.16"
tokio = { version = "1", features = ["rt-multi-thread"] }
```

**Version note:** The `google-cloud-*` crates are pre-1.0. The `google-cloud-asset` crate specifically provides the Asset Inventory v1 API. Verify the current version before implementation; these bump frequently.

### Services and resource types to collect

GCP asset types use the format `<service>.googleapis.com/<ResourceType>`. All types are filterable via `asset_types` in the `SearchAllResources` call.

| GCP Service | Asset type | `CloudResource.kind` value | Key relationships |
|---|---|---|---|
| Compute Engine | `compute.googleapis.com/Instance` | `compute.googleapis.com/instance` | VPC network, subnetwork, service account, disks |
| Compute Engine | `compute.googleapis.com/Network` | `compute.googleapis.com/network` | (root VPC) |
| Compute Engine | `compute.googleapis.com/Subnetwork` | `compute.googleapis.com/subnetwork` | Network (VPC), region |
| Compute Engine | `compute.googleapis.com/Firewall` | `compute.googleapis.com/firewall` | Network |
| Compute Engine | `compute.googleapis.com/BackendService` | `compute.googleapis.com/backendservice` | Instance groups, health checks |
| Compute Engine | `compute.googleapis.com/ForwardingRule` | `compute.googleapis.com/forwardingrule` | Backend service, target proxy |
| Compute Engine | `compute.googleapis.com/Disk` | `compute.googleapis.com/disk` | VM instance (if attached) |
| Cloud Storage | `storage.googleapis.com/Bucket` | `storage.googleapis.com/bucket` | (none typical; IAM refs service accounts) |
| Cloud SQL | `sqladmin.googleapis.com/Instance` | `sqladmin.googleapis.com/instance` | VPC (if private IP), authorized networks |
| Cloud Functions | `cloudfunctions.googleapis.com/CloudFunction` | `cloudfunctions.googleapis.com/cloudfunction` | VPC connector, service account, trigger (Pub/Sub topic) |
| Cloud Run | `run.googleapis.com/Service` | `run.googleapis.com/service` | VPC connector, service account |
| GKE | `container.googleapis.com/Cluster` | `container.googleapis.com/cluster` | VPC network, subnetwork, node service account |
| BigQuery | `bigquery.googleapis.com/Dataset` | `bigquery.googleapis.com/dataset` | (root) |
| BigQuery | `bigquery.googleapis.com/Table` | `bigquery.googleapis.com/table` | Dataset |
| Pub/Sub | `pubsub.googleapis.com/Topic` | `pubsub.googleapis.com/topic` | (root) |
| Pub/Sub | `pubsub.googleapis.com/Subscription` | `pubsub.googleapis.com/subscription` | Topic, push endpoint |
| Cloud Spanner | `spanner.googleapis.com/Instance` | `spanner.googleapis.com/instance` | (root) |
| IAM | `iam.googleapis.com/ServiceAccount` | `iam.googleapis.com/serviceaccount` | (root; referenced by many resources) |
| KMS | `cloudkms.googleapis.com/CryptoKey` | `cloudkms.googleapis.com/cryptokey` | KMS KeyRing |
| Artifact Registry | `artifactregistry.googleapis.com/Repository` | `artifactregistry.googleapis.com/repository` | (root) |
| Memorystore | `redis.googleapis.com/Instance` | `redis.googleapis.com/instance` | VPC network |
| Secret Manager | `secretmanager.googleapis.com/Secret` | `secretmanager.googleapis.com/secret` | (root; access policy refs service accounts) |

### Exact API call

**Cloud Asset Inventory `SearchAllResources`:**

```
asset_v1::AssetServiceClient::search_all_resources(SearchAllResourcesRequest {
    scope: "projects/my-project-id",   // or "folders/..." or "organizations/..."
    asset_types: vec![],               // empty = all types
    page_size: 500,
    ..
})
```

`scope` is set from `CloudConfig.profile` (project id) or `CloudConfig.region` is ignored (GCP CAI scope is project/folder/org, not region — pass region as an asset query filter if scoping is needed). Paginate via `next_page_token`.

The response `ResourceSearchResult` contains:
- `name`: full resource name (`//compute.googleapis.com/projects/proj/zones/us-c1-a/instances/vm1`)
- `asset_type`: GCP asset type string (`compute.googleapis.com/Instance`)
- `location`: region or zone (e.g. `us-central1`, `us-central1-a`)
- `project`: project number or id
- `labels`: key-value map (equivalent to tags)
- `additional_attributes`: JSON blob with resource-specific fields
- `display_name`: human-readable name

### `CloudResource` field mapping for GCP

| `CloudResource` field | Source |
|---|---|
| `id` | `name` from `ResourceSearchResult` (the full resource name, globally unique) |
| `kind` | `asset_type` lowercased (e.g. `compute.googleapis.com/instance`) |
| `region` | `location` field (may be a zone like `us-central1-a`; strip zone suffix to get region) |
| `account_id` | `project` field |
| `name` | `display_name` field |
| `tags` | `labels` map → flatten to `Vec<(String, String)>` |
| `depends_on` | Parse from `additional_attributes` JSON: `networkInterfaces[*].network`, `serviceAccount`, `topic`, etc. |
| `attributes` | Flattened subset of `additional_attributes` (skip secrets: any field with `password`, `key`, `privateKey`, `accessToken` in name) |

**Kind normalization:** GCP asset types already use a consistent lowercase dot-slash format. Store them as-is. This differs from the Terraform convention (`google_compute_instance`) but is unambiguous and maps 1:1 to the GCP API. The drift detector keys on `(kind, name)` — as long as the IaC extractor also uses the same type strings (or a normalization map), the diff works.

**Note on IaC consistency:** Terraform's GCP provider uses `google_compute_instance`, but GCP's own API (and tfstate) uses `compute.googleapis.com/Instance`. The IaC extractor (HCL via `arborium-hcl`) emits Terraform's convention. The live collector will emit the CAI convention. Drift matching requires a normalization table mapping `google_compute_instance` ↔ `compute.googleapis.com/instance`. This normalization should live in the `estate_drift` tool, not in the collector — the collector reports what the provider says, not a translated value.

### Auth pattern

Use Application Default Credentials (ADC). The `google-cloud-auth` crate resolves in order:

1. `GOOGLE_APPLICATION_CREDENTIALS` environment variable pointing to a service account JSON key file
2. Well-known gcloud credential file (`~/.config/gcloud/application_default_credentials.json`)
3. GCE/GKE/Cloud Run metadata service workload identity

`CloudConfig.profile` maps to the GCP project id (the `scope` parameter of CAI queries).

Required IAM role: `roles/cloudasset.viewer` at the project, folder, or organization scope. This is purely read-only. Optionally add `roles/browser` if the CAI scope is organization-level and the service account needs project enumeration.

---

## 5. Auth pattern summary

| Provider | Credential resolution (ambient chain) | `CloudConfig.profile` meaning | Required read-only role |
|---|---|---|---|
| AWS | Env vars → named profile → ECS → IMDSv2/IRSA | AWS profile name (e.g. `"prod-ro"`) | `AWSResourceExplorerReadOnlyAccess` + `ReadOnlyAccess` |
| Azure | Env SP → Workload Identity → Managed Identity → Azure CLI | Azure subscription id | `Reader` at subscription scope |
| GCP | `GOOGLE_APPLICATION_CREDENTIALS` → gcloud ADC → metadata service | GCP project id | `roles/cloudasset.viewer` |

**Invariants enforced by all implementations:**
- `collect()` is `&self` — no mutation of self, no side-effects.
- No credential values enter `CloudResource.attributes`, `CloudResource.tags`, or any `Node.metadata` field. Redaction must happen before `CloudResource` is constructed.
- `CloudConfig` carries only the profile name (a pointer), never a raw key or secret.
- The `CloudResource` struct has no credential fields — this is a compile-time enforcement: you cannot accidentally store a credential because there is nowhere to put it.

---

## 6. Cargo.toml changes

### Strategy: optional features, not always-on

The cloud SDK dependencies (especially AWS and GCP) are large. Making them always-on would bloat the binary for users who only want code graph analysis without cloud collection. Use Cargo optional features.

**In `crates/wicked-estate-extract/Cargo.toml`:**

```toml
[features]
default = []
cloud-aws   = ["aws-config", "aws-sdk-resourceexplorer2", "aws-sdk-ec2", "aws-sdk-iam", "tokio"]
cloud-azure = ["azure_identity", "azure_mgmt_resources", "azure_core", "tokio"]
cloud-gcp   = ["google-cloud-asset", "google-cloud-auth", "tokio"]
cloud-all   = ["cloud-aws", "cloud-azure", "cloud-gcp"]

[dependencies]
# existing dependencies unchanged ...

# AWS cloud collector (optional)
aws-config                = { version = "^1.5",  optional = true }
aws-sdk-resourceexplorer2 = { version = "^1.0",  optional = true }
aws-sdk-ec2               = { version = "^1.0",  optional = true }
aws-sdk-iam               = { version = "^1.0",  optional = true }

# Azure cloud collector (optional)
azure_identity      = { version = "^0.20", optional = true }
azure_mgmt_resources = { version = "^0.20", optional = true }
azure_core          = { version = "^0.20", optional = true }

# GCP cloud collector (optional)
google-cloud-asset = { version = "^0.10", optional = true }
google-cloud-auth  = { version = "^0.16", optional = true }

# Async runtime — needed by all three SDK families
tokio = { version = "1", features = ["rt-multi-thread", "macros"], optional = true }
```

**Note on async:** All three SDKs are async. The current `CloudCollector::collect()` is synchronous (`fn collect(&self) -> Result<Vec<CloudResource>>`). Two options:

1. **Keep sync, block in collect():** Use `tokio::runtime::Handle::current().block_on(...)` or `tokio::task::block_in_place(...)` inside each real `collect()` impl. This is the minimal-change path and consistent with how the LSP resolver works (`wicked-estate-resolve/src/lsp.rs` uses blocking). The collector is called from the indexer which already runs inside a `tokio` runtime (see `AsyncGraphStore` in `traits.rs`).

2. **Make `CloudCollector::collect()` async:** Would require changing the trait signature to `async fn collect(&self) -> Result<Vec<CloudResource>>`, which changes the `MockCloudCollector` too and would require `#[async_trait]`. This is cleaner but is a breaking trait change.

**Recommendation:** Start with option 1 (sync wrapper, block on the async SDK call inside `collect()`). This is zero trait-change, zero caller-change — exactly ADR-004's constraint. The LSP resolver precedent confirms this pattern works.

---

## 7. Implementation structure

### Option A: Submodules within `wicked-estate-extract` (recommended for W10.2)

This minimizes disruption — all cloud code lives in one crate behind feature flags, consistent with how `tfstate.rs`, `cloud.rs`, and the other collectors live there now.

```
crates/wicked-estate-extract/src/
  cloud.rs          ← existing: trait, mock, factory, mapping (already built)
  cloud/            ← NEW directory
    mod.rs          ← re-exports AwsCollector, AzureCollector, GcpCollector
    aws.rs          ← #[cfg(feature = "cloud-aws")] struct AwsCollector + impl CloudCollector
    azure.rs        ← #[cfg(feature = "cloud-azure")] struct AzureCollector + impl CloudCollector
    gcp.rs          ← #[cfg(feature = "cloud-gcp")] struct GcpCollector + impl CloudCollector
```

Restructure `cloud.rs` to remain a facade that declares the trait, config, mapping, and mock, while delegating to the submodule:

```rust
// in cloud.rs — add after MockCloudCollector
#[cfg(feature = "cloud-aws")]
pub use cloud_impls::aws::AwsCollector;
#[cfg(feature = "cloud-azure")]
pub use cloud_impls::azure::AzureCollector;
#[cfg(feature = "cloud-gcp")]
pub use cloud_impls::gcp::GcpCollector;
```

### Option B: New `wicked-estate-collect` crate

Appropriate if cloud SDK dependencies cause compile-time bloat that contaminates the main extract crate even with optional features (Cargo's optional feature compilation is additive — the optional deps only compile when the feature is enabled, so this should not be an issue). Reserve this option for later if needed.

### Factory arm additions in `open_cloud_collector`

The only other code change is in `open_cloud_collector`:

```rust
pub fn open_cloud_collector(cfg: &CloudConfig) -> Result<Box<dyn CloudCollector>> {
    match cfg.provider {
        #[cfg(feature = "cloud-aws")]
        CloudProvider::Aws => {
            Ok(Box::new(aws::AwsCollector::new(cfg)?))
        }
        #[cfg(feature = "cloud-azure")]
        CloudProvider::Azure => {
            Ok(Box::new(azure::AzureCollector::new(cfg)?))
        }
        #[cfg(feature = "cloud-gcp")]
        CloudProvider::Gcp => {
            Ok(Box::new(gcp::GcpCollector::new(cfg)?))
        }
        // Fallback for providers whose feature is not compiled in:
        provider => Err(Error::Extraction(format!(
            "{} collector is not compiled in — enable the corresponding feature flag",
            provider.as_str()
        ))),
    }
}
```

This is exactly the ADR-004 "zero caller changes" pattern: callers of `open_cloud_collector` are unchanged; they just get an `Err` if the feature is not compiled in, rather than the "designed but not built" error they get today.

---

## 8. CloudResource → Graph mapping

The mapping from `collect()` results to graph nodes and edges follows `tfstate.rs` as the template. The `cloud_resources_to_nodes` function already handles node creation. The caller (the indexer pipeline or a new `collect` CLI subcommand) is responsible for:

1. **Call `collector.collect()`** → `Vec<CloudResource>`
2. **Call `cloud_resources_to_nodes(provider, &resources)`** → `Vec<Node>` — already built
3. **Build dependency edges from `depends_on`:**

```rust
// Pseudo-code for the edge building step (analogous to tfstate.rs's second pass)
let id_to_symbol: HashMap<String, SymbolId> = resources.iter()
    .map(|r| {
        let scheme = format!("cloud-{}", provider.as_str());
        let sym = Symbol::synthetic(&scheme, &r.id).id();
        (r.id.clone(), sym)
    })
    .collect();

let mut edges: Vec<Edge> = Vec::new();
let mut refs: Vec<UnresolvedRef> = Vec::new();

for resource in &resources {
    let source_sym = &id_to_symbol[&resource.id];
    for dep_id in &resource.depends_on {
        if let Some(target_sym) = id_to_symbol.get(dep_id) {
            edges.push(Edge::new(
                source_sym.clone(),
                target_sym.clone(),
                EdgeKind::Other("depends_on".to_string()),
                ResolutionTier::Parsed,  // live cloud data is ground truth
                format!("{}-collector", provider.as_str()),
            ));
        } else {
            // Cross-account / cross-scope reference — unresolved
            refs.push(UnresolvedRef::new(
                source_sym.clone(),
                dep_id.clone(),
                EdgeKind::Other("depends_on".to_string()),
                Location::new(resource.id.clone(), Span::ZERO),
            ));
        }
    }
}
```

4. **Return `Extraction { nodes, local_edges: edges, refs }`** — reuses the existing `Extraction` type, slots into `GraphWrite::upsert_nodes` + `upsert_edges` + `upsert_unresolved_refs` without any store changes.

**Confidence:** Live cloud state is ground truth — use `ResolutionTier::Parsed` (confidence 1.0). This is correct: a live API response is as certain as a parsed AST fact.

**Node `location.file`:** Use the resource physical id (the ARN / Azure id / GCP full name). This is the `Span::ZERO` sentinel pattern established by `tfstate.rs` and already in `cloud_resources_to_nodes`. The `estate_drift` tool already handles this.

**Collector audit metadata** (ADR-004 §5): Insert `metadata["collector"] = "<provider>-collector"` and `metadata["collected_at"] = <RFC3339>` in each real collector's `resource_to_node` path (or add to `cloud_resources_to_nodes` via a second overload that accepts a timestamp). This is the auditable trail ADR-004 requires.

### CLI integration

Add a `wicked-estate collect` subcommand to the `wicked-estate` binary, analogous to `wicked-estate tfstate`:

```
wicked-estate collect --provider aws [--region us-east-1] [--profile prod-ro] [--db graph.db]
wicked-estate collect --provider azure [--profile <subscription-id>] [--db graph.db]
wicked-estate collect --provider gcp [--profile <project-id>] [--db graph.db]
```

This calls `open_cloud_collector(cfg)`, calls `collect()`, maps to `Extraction`, and calls `store.upsert_nodes + upsert_edges + upsert_unresolved_refs` in a batch transaction. Then `estate drift` picks up the `origin=live` nodes for diffing.

---

## 9. Test strategy

### Unit tests (no credentials, no network)

Continue extending `MockCloudCollector` tests in `cloud.rs`. These test the mapping logic (`cloud_resources_to_nodes`, `resource_to_node`, edge building) with canned data. These tests are already comprehensive (21 tests in the existing file).

Add tests for:
- `account_id` appears in `metadata["account_id"]`
- `tags` appear as `metadata["tag.<key>"]` distinct from `metadata["attr.<key>"]`
- `name` appears in `metadata["name"]`
- Edge building from `depends_on` produces `EdgeKind::Other("depends_on")` edges at `Confidence(1.0)`
- Cross-scope `depends_on` (id not in resources list) produces `UnresolvedRef`

### Integration tests (no credentials required) — LocalStack for AWS

**LocalStack** (`localstack/localstack` Docker image) provides a local AWS API emulator that supports Resource Explorer, EC2, S3, IAM, Lambda, and more. Tests tagged `#[ignore]` or behind a `LOCALSTACK_ENDPOINT` environment variable can be run in CI when LocalStack is available.

```rust
#[test]
#[cfg(feature = "cloud-aws")]
#[ignore = "requires LocalStack (LOCALSTACK_ENDPOINT env var)"]
fn aws_collector_returns_ec2_instances_from_localstack() {
    // Set AWS_DEFAULT_REGION, AWS_ACCESS_KEY_ID=test, AWS_SECRET_ACCESS_KEY=test
    // Set AWS_ENDPOINT_URL=http://localhost:4566 (LocalStack)
    // Pre-populate LocalStack with a known EC2 instance
    // Assert collector returns it with correct kind + id
}
```

For Azure: **Azurite** (Azure Storage emulator) covers only storage; full Azure resource emulation is not available as a free local tool. Use integration tests that run against a real Azure subscription in CI (gated by `AZURE_TEST_SUBSCRIPTION_ID` env var).

For GCP: **GCP Emulators** exist for specific services (Pub/Sub, Firestore, etc.) but not for Cloud Asset Inventory. Use a real GCP project in CI (gated by `GOOGLE_TEST_PROJECT_ID` env var).

### Credential-gated real-cloud tests

Add a `tests/cloud_integration/` directory with tests requiring real credentials, marked `#[ignore]` or behind feature + env var gates:

```
tests/cloud_integration/
  aws_live_test.rs      # CLOUD_AWS_TEST_PROFILE must be set
  azure_live_test.rs    # AZURE_TEST_SUBSCRIPTION_ID must be set
  gcp_live_test.rs      # GOOGLE_TEST_PROJECT_ID must be set
```

These run only in the dedicated cloud-integration CI job, not in the standard `cargo test --workspace`. Each test asserts:
- `collect()` returns at least 1 resource
- All resources have a non-empty `id` and `kind`
- No resource has `attributes` containing password/key/secret field names
- `cloud_resources_to_nodes` produces stable `SymbolId`s across two calls (ADR-002 compliance)

### MockCloudCollector pattern for downstream tests

`MockCloudCollector` remains the recommended way to test code that calls `CloudCollector::collect()` without network access. The drift detector tests (`estate_drift`) already use this pattern; any new consumers should follow it.

---

## 10. Risks and gaps

### Risk 1: AWS Resource Explorer must be enabled by the user (high impact, external dependency)

AWS Resource Explorer is not enabled by default in new accounts. The collector cannot enable it (that would be a mutation). If it is not enabled, `Search` returns an error. **Mitigation:** Detect the error and return a clear `Error::Extraction("AWS Resource Explorer is not enabled in this account. Enable it at console.aws.amazon.com/resource-explorer and re-run collect.")`. Document the one-time setup requirement prominently. Alternatively, fall back to calling individual `describe*` APIs per service — more calls, but no prerequisite.

### Risk 2: GCP Cloud Asset Inventory `SearchAllResources` returns limited `additionalAttributes` for some resource types (medium impact)

For some GCP resource types, the `additionalAttributes` field in `SearchAllResources` results is sparse or absent. Full resource configuration requires calling the individual service APIs (e.g. `compute.instances.get`). **Mitigation:** Accept sparse data in Phase 1; document which resource types have incomplete attributes. Add targeted enrichment calls in Phase 2, analogous to the AWS Phase 2 approach.

### Risk 3: Azure `azure_mgmt_resources` crate API instability (medium impact)

The azure-sdk-for-rust crates are pre-1.0 and have broken API compatibility between minor versions. **Mitigation:** Pin to a specific minor version. Add a `Cargo.lock` check (the workspace already tracks `Cargo.lock`). Write an integration test against a real subscription in CI so API breakage is caught immediately.

### Risk 4: `CloudCollector::collect()` is synchronous but cloud SDKs are async (medium impact, already identified)

All three SDKs require an async runtime. The existing `collect()` signature is sync. The `block_in_place` approach works within a Tokio runtime but panics in a non-Tokio context (e.g. a pure sync test binary that does not start Tokio). **Mitigation:** Document that real collectors require a Tokio runtime; the CLI already provides one. For tests, wrap `MockCloudCollector` (which is sync and has no runtime requirement) and use `tokio::test` for real collector integration tests.

A future improvement (post-W10) would be to add `async fn collect_async(&self)` to the trait or define a separate `AsyncCloudCollector` trait following the `AsyncGraphStore` pattern already in `traits.rs`.

### Risk 5: `depends_on` relationship extraction is incomplete without enrichment calls (medium impact)

Resource Explorer returns limited relationship data. Many `depends_on` edges (e.g. EC2 instance → security group) require calling the targeted `describe` APIs. If only Resource Explorer is used, the resulting graph has nodes but sparse edges — drift detection will work for resource presence/absence but not for relationship drift. **Mitigation:** Document which relationships require enrichment. Implement Phase 2 enrichment calls for the highest-value relationship types (VPC/subnet topology, IAM role linkage) in the initial implementation. Less critical relationships can be added incrementally.

### Risk 6: Multi-account / multi-subscription / multi-project scope (low-medium impact)

The current `CloudConfig` has a single `profile` and `region`. Enumerating across all accounts in an AWS Organization, all subscriptions in an Azure tenant, or all projects in a GCP org requires either (a) iterating over accounts/subscriptions/projects externally and calling `open_cloud_collector` once per scope, or (b) building org-level enumeration into the collector itself. **Mitigation:** Phase 1 targets single-account/subscription/project scope. Multi-scope can be added as a `CloudConfig.scopes: Vec<String>` field later. Document the single-scope limitation.

### Gap 1: `cloud_resources_to_nodes` does not emit edges

The current `cloud_resources_to_nodes` function maps resources to nodes only (1:1). Edge creation from `depends_on` is described in Section 8 but is not yet in any production code path — it exists in `tfstate.rs` but not in `cloud.rs`. The caller must build edges from `CloudResource.depends_on`. This is by design (same pattern as tfstate), but there is no existing utility function for the cloud case. **Action:** Add a `cloud_resources_to_extraction(provider, resources) -> Extraction` function that wraps both node creation and edge building into a single `Extraction` return value, matching the `TfstateCollector::collect()` return type. This makes the cloud collectors a drop-in peer of `TfstateCollector`.

### Gap 2: `CloudResource` struct changes require updating `MockCloudCollector` and all existing tests

Adding `account_id`, `name`, and `tags` fields to `CloudResource` (Section 1) requires updating the 21 existing tests that construct `CloudResource` directly. With `Default` derived, `..Default::default()` will compile, but tests that assert on metadata will need updating to account for new metadata keys. **Action:** Add `#[derive(Default)]` to `CloudResource` (it already has it) and use struct update syntax in all existing tests.

### Gap 3: IaC–live kind normalization table is not yet built

As noted in Section 4, Terraform's GCP provider uses `google_compute_instance` but GCP CAI uses `compute.googleapis.com/Instance`. The same mismatch exists for Azure (Terraform's `azurerm_virtual_machine` vs Azure's `Microsoft.Compute/virtualMachines`) and for AWS (Terraform's `aws_instance` vs Resource Explorer's `AWS::EC2::Instance`). The `estate_drift` tool currently diffs on `(type, name)` — this will produce false-positive "unmanaged" and "undeployed" detections unless the normalization is in place. **Action:** Build a `kind_normalize(provider, kind) -> String` function in the drift detector that maps API-native type strings to Terraform-convention strings. A TOML data file (not compiled Rust match arms) is the right location for this mapping table, consistent with the rules-as-data principle.
