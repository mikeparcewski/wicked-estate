//! JSON-based policy/rule extractors — Azure Policy (W15.5) and AWS Config Rules (W15.5).
//!
//! Both file types are plain JSON but must be **disambiguated from generic JSON** by schema
//! inspection (`.json` is shared with ARM, Swagger, package.json, …). Detection is purely
//! structural: the presence of the discriminating key.
//!
//! # Azure Policy
//!
//! Schema discriminator: `properties.policyRule` key exists.
//!
//! Node model:
//! - `RuleSet` — from `properties.displayName` (or file stem) — top-level container
//! - `Rule`    — same name; the concrete policy
//! - `Condition` — `properties.policyRule.if` serialised as the node signature
//! - `Action`    — `properties.policyRule.then.effect` value
//!
//! Edges (all `ResolutionTier::Parsed`, `Contains`):
//!   RuleSet→Rule, Rule→Condition, Rule→Action
//!
//! # AWS Config Rule
//!
//! Schema discriminator: `ConfigRuleName` key at top level.
//!
//! Node model:
//! - `RuleSet`   — file-level container (file path as name)
//! - `Rule`      — `ConfigRuleName`
//! - `Fact`      — one node per `Scope.ComplianceResourceTypes[*]`
//! - `Condition` — `Source.SourceIdentifier`
//!
//! Edges:
//! - `Contains`: RuleSet→Rule
//! - `Evaluates`: Rule→Fact (one per resource type)
//! - `Contains`: Rule→Condition

use serde_json::Value;
use wicked_estate_core::{
    Edge, EdgeKind, Error, Extraction, Language, Location, Node, NodeKind, ResolutionTier, Result,
    SourceFile, Span, Symbol, SymbolId,
};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Build a stable `SymbolId` for a rule-graph node.
///
/// Scheme encodes the extractor family; id encodes `<file_path>::<role>::<name>`.
/// Using `Symbol::synthetic` follows the same ADR-002-compliant pattern as `TfstateCollector`.
fn node_sym(scheme: &str, file_path: &str, role: &str, name: &str) -> SymbolId {
    Symbol::synthetic(scheme, format!("{file_path}::{role}::{name}")).id()
}

fn sentinel_location(file_path: &str) -> Location {
    Location::new(file_path, Span::ZERO)
}

// ── Azure Policy ──────────────────────────────────────────────────────────────

/// Extracts Azure Policy definitions from `.json` files that contain
/// `properties.policyRule`.
///
/// # Detection
///
/// The extractor silently returns an empty [`Extraction`] when:
/// - the file path does not end with `.json`, or
/// - the top-level JSON does not contain `properties.policyRule`.
///
/// This is deliberate: `.json` is a shared extension and we must not pollute the
/// graph with spurious nodes from unrelated JSON files.
pub struct AzurePolicyExtractor;

impl AzurePolicyExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AzurePolicyExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl wicked_estate_core::Extractor for AzurePolicyExtractor {
    fn languages(&self) -> Vec<Language> {
        vec![Language::new("azure-policy")]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        // Detect by extension.
        if !file.path.ends_with(".json") {
            return Ok(Extraction::default());
        }

        let root: Value = serde_json::from_str(&file.text)
            .map_err(|e| Error::Extraction(format!("azure-policy JSON parse error: {e}")))?;

        // Detect by schema: must have properties.policyRule.
        let props = match root.get("properties") {
            Some(p) if p.is_object() => p,
            _ => return Ok(Extraction::default()),
        };
        let policy_rule = match props.get("policyRule") {
            Some(pr) if pr.is_object() => pr,
            _ => return Ok(Extraction::default()),
        };

        // ── Extract names ─────────────────────────────────────────────────────
        let display_name = props
            .get("displayName")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| file_stem(&file.path));

        let description = props
            .get("description")
            .and_then(|v| v.as_str())
            .map(str::to_owned);

        // Condition: the `if` clause serialised compactly as the signature.
        let condition_sig = policy_rule
            .get("if")
            .map(|v| serde_json::to_string(v).unwrap_or_default())
            .unwrap_or_default();

        // Action: the `then.effect` string.
        let action_name = policy_rule
            .get("then")
            .and_then(|t| t.get("effect"))
            .and_then(|e| e.as_str())
            .unwrap_or("unknown")
            .to_owned();

        // ── Build symbols ─────────────────────────────────────────────────────
        let scheme = "azure-policy";
        let ruleset_sym = node_sym(scheme, &file.path, "ruleset", &display_name);
        let rule_sym = node_sym(scheme, &file.path, "rule", &display_name);
        let condition_sym = node_sym(scheme, &file.path, "condition", &display_name);
        let action_sym = node_sym(scheme, &file.path, "action", &action_name);

        let lang = Language::new(scheme);
        let loc = sentinel_location(&file.path);

        // ── Build nodes ───────────────────────────────────────────────────────
        let mut ruleset_node = Node::new(
            ruleset_sym.clone(),
            NodeKind::RuleSet,
            display_name.clone(),
            lang.clone(),
            loc.clone(),
        );
        if let Some(d) = &description {
            ruleset_node.doc = Some(d.clone());
        }

        let mut rule_node = Node::new(
            rule_sym.clone(),
            NodeKind::Rule,
            display_name.clone(),
            lang.clone(),
            loc.clone(),
        );
        if let Some(d) = &description {
            rule_node.doc = Some(d.clone());
        }
        rule_node.signature = Some(display_name.clone());

        let mut condition_node = Node::new(
            condition_sym.clone(),
            NodeKind::Condition,
            display_name.clone(),
            lang.clone(),
            loc.clone(),
        );
        condition_node.signature = Some(condition_sig);

        let action_node = Node::new(
            action_sym.clone(),
            NodeKind::Action,
            action_name.clone(),
            lang.clone(),
            loc,
        );

        // ── Build edges ───────────────────────────────────────────────────────
        // RuleSet→Rule (Contains), Rule→Condition (Contains), Rule→Action (Contains)
        let edges = vec![
            Edge::new(
                ruleset_sym,
                rule_sym.clone(),
                EdgeKind::Contains,
                ResolutionTier::Parsed,
                "azure-policy-extractor",
            ),
            Edge::new(
                rule_sym.clone(),
                condition_sym,
                EdgeKind::Contains,
                ResolutionTier::Parsed,
                "azure-policy-extractor",
            ),
            Edge::new(
                rule_sym,
                action_sym,
                EdgeKind::Contains,
                ResolutionTier::Parsed,
                "azure-policy-extractor",
            ),
        ];

        Ok(Extraction {
            nodes: vec![ruleset_node, rule_node, condition_node, action_node],
            local_edges: edges,
            refs: vec![],
        })
    }
}

// ── AWS Config Rule ───────────────────────────────────────────────────────────

/// Extracts AWS Config Rule definitions from `.json` files that have
/// `ConfigRuleName` at the top level.
///
/// # Detection
///
/// The extractor silently returns an empty [`Extraction`] when:
/// - the file path does not end with `.json`, or
/// - the top-level JSON does not contain `ConfigRuleName`.
pub struct AwsConfigRuleExtractor;

impl AwsConfigRuleExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AwsConfigRuleExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl wicked_estate_core::Extractor for AwsConfigRuleExtractor {
    fn languages(&self) -> Vec<Language> {
        vec![Language::new("aws-config-rule")]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        // Detect by extension.
        if !file.path.ends_with(".json") {
            return Ok(Extraction::default());
        }

        let root: Value = serde_json::from_str(&file.text)
            .map_err(|e| Error::Extraction(format!("aws-config-rule JSON parse error: {e}")))?;

        // Detect by schema: must have ConfigRuleName at top level.
        let rule_name = match root.get("ConfigRuleName").and_then(|v| v.as_str()) {
            Some(n) if !n.is_empty() => n.to_owned(),
            _ => return Ok(Extraction::default()),
        };

        let description = root
            .get("Description")
            .and_then(|v| v.as_str())
            .map(str::to_owned);

        // Source identifier → Condition node.
        let source_identifier = root
            .get("Source")
            .and_then(|s| s.get("SourceIdentifier"))
            .and_then(|v| v.as_str())
            .unwrap_or("UNKNOWN")
            .to_owned();

        // Scope.ComplianceResourceTypes → Fact nodes.
        let resource_types: Vec<String> = root
            .get("Scope")
            .and_then(|s| s.get("ComplianceResourceTypes"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();

        // ── Build symbols ─────────────────────────────────────────────────────
        let scheme = "aws-config-rule";
        let ruleset_name = file_stem(&file.path);
        let ruleset_sym = node_sym(scheme, &file.path, "ruleset", &ruleset_name);
        let rule_sym = node_sym(scheme, &file.path, "rule", &rule_name);
        let condition_sym = node_sym(scheme, &file.path, "condition", &source_identifier);

        let lang = Language::new(scheme);
        let loc = sentinel_location(&file.path);

        // ── Build nodes ───────────────────────────────────────────────────────
        let ruleset_node = Node::new(
            ruleset_sym.clone(),
            NodeKind::RuleSet,
            ruleset_name,
            lang.clone(),
            loc.clone(),
        );

        let mut rule_node = Node::new(
            rule_sym.clone(),
            NodeKind::Rule,
            rule_name.clone(),
            lang.clone(),
            loc.clone(),
        );
        if let Some(d) = &description {
            rule_node.doc = Some(d.clone());
        }
        rule_node.signature = Some(rule_name.clone());

        let mut condition_node = Node::new(
            condition_sym.clone(),
            NodeKind::Condition,
            source_identifier.clone(),
            lang.clone(),
            loc.clone(),
        );
        condition_node.signature = Some(source_identifier);

        let fact_nodes: Vec<Node> = resource_types
            .iter()
            .map(|rt| {
                let fact_sym = node_sym(scheme, &file.path, "fact", rt);
                Node::new(
                    fact_sym,
                    NodeKind::Fact,
                    rt.clone(),
                    lang.clone(),
                    loc.clone(),
                )
            })
            .collect();

        // ── Build edges ───────────────────────────────────────────────────────
        let mut edges = Vec::new();

        // Contains: RuleSet→Rule
        edges.push(Edge::new(
            ruleset_sym,
            rule_sym.clone(),
            EdgeKind::Contains,
            ResolutionTier::Parsed,
            "aws-config-rule-extractor",
        ));

        // Contains: Rule→Condition
        edges.push(Edge::new(
            rule_sym.clone(),
            condition_sym,
            EdgeKind::Contains,
            ResolutionTier::Parsed,
            "aws-config-rule-extractor",
        ));

        // Evaluates: Rule→Fact (one per resource type)
        for rt in &resource_types {
            let fact_sym = node_sym(scheme, &file.path, "fact", rt);
            edges.push(Edge::new(
                rule_sym.clone(),
                fact_sym,
                EdgeKind::Evaluates,
                ResolutionTier::Parsed,
                "aws-config-rule-extractor",
            ));
        }

        let mut all_nodes = vec![ruleset_node, rule_node, condition_node];
        all_nodes.extend(fact_nodes);

        Ok(Extraction {
            nodes: all_nodes,
            local_edges: edges,
            refs: vec![],
        })
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Extract the file stem (name without extension) from a path string.
///
/// Falls back to the full path if no stem can be determined.
fn file_stem(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| path.to_owned())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::{Extractor, SourceFile};

    // ── Azure Policy ──────────────────────────────────────────────────────────

    const AZURE_POLICY_JSON: &str = r#"
{
  "properties": {
    "displayName": "Require HTTPS on storage",
    "policyType": "Custom",
    "description": "Deny storage accounts that do not use HTTPS.",
    "policyRule": {
      "if": {
        "field": "type",
        "equals": "Microsoft.Storage/storageAccounts"
      },
      "then": {
        "effect": "deny"
      }
    }
  }
}
"#;

    fn azure_extractor() -> AzurePolicyExtractor {
        AzurePolicyExtractor::new()
    }

    fn azure_file() -> SourceFile {
        SourceFile {
            path: "policies/require_https.json".to_string(),
            language: Language::new("azure-policy"),
            text: AZURE_POLICY_JSON.to_string(),
        }
    }

    #[test]
    fn azure_policy_emits_four_nodes() {
        let ex = azure_extractor()
            .extract(&azure_file())
            .expect("azure policy must parse");
        assert_eq!(
            ex.nodes.len(),
            4,
            "expected 4 nodes (RuleSet, Rule, Condition, Action); got: {:?}",
            ex.nodes
                .iter()
                .map(|n| (&n.kind, &n.name))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn azure_policy_node_kinds() {
        let ex = azure_extractor()
            .extract(&azure_file())
            .expect("azure policy must parse");
        let kinds: Vec<&NodeKind> = ex.nodes.iter().map(|n| &n.kind).collect();
        assert!(
            kinds.contains(&&NodeKind::RuleSet),
            "must have RuleSet; got: {kinds:?}"
        );
        assert!(
            kinds.contains(&&NodeKind::Rule),
            "must have Rule; got: {kinds:?}"
        );
        assert!(
            kinds.contains(&&NodeKind::Condition),
            "must have Condition; got: {kinds:?}"
        );
        assert!(
            kinds.contains(&&NodeKind::Action),
            "must have Action; got: {kinds:?}"
        );
    }

    #[test]
    fn azure_policy_display_name_used() {
        let ex = azure_extractor()
            .extract(&azure_file())
            .expect("azure policy must parse");
        let ruleset = ex
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::RuleSet)
            .expect("RuleSet node must exist");
        assert_eq!(ruleset.name, "Require HTTPS on storage");
    }

    #[test]
    fn azure_policy_action_name_is_effect() {
        let ex = azure_extractor()
            .extract(&azure_file())
            .expect("azure policy must parse");
        let action = ex
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Action)
            .expect("Action node must exist");
        assert_eq!(action.name, "deny");
    }

    #[test]
    fn azure_policy_three_edges() {
        let ex = azure_extractor()
            .extract(&azure_file())
            .expect("azure policy must parse");
        assert_eq!(
            ex.local_edges.len(),
            3,
            "expected 3 Contains edges; got: {:?}",
            ex.local_edges.iter().map(|e| &e.kind).collect::<Vec<_>>()
        );
        assert!(
            ex.local_edges.iter().all(|e| e.kind == EdgeKind::Contains),
            "all edges must be Contains; got: {:?}",
            ex.local_edges.iter().map(|e| &e.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn azure_policy_non_json_file_returns_empty() {
        let file = SourceFile {
            path: "policy.tf".to_string(),
            language: Language::new("azure-policy"),
            text: AZURE_POLICY_JSON.to_string(),
        };
        let ex = azure_extractor()
            .extract(&file)
            .expect("must not error for non-json");
        assert!(ex.nodes.is_empty(), "non-json file must produce no nodes");
    }

    #[test]
    fn azure_policy_non_matching_json_returns_empty() {
        let file = SourceFile {
            path: "package.json".to_string(),
            language: Language::new("azure-policy"),
            text: r#"{"name":"foo","version":"1.0"}"#.to_string(),
        };
        let ex = azure_extractor()
            .extract(&file)
            .expect("must not error for non-policy json");
        assert!(ex.nodes.is_empty(), "non-policy json must produce no nodes");
    }

    #[test]
    fn azure_policy_falls_back_to_file_stem_when_no_display_name() {
        let json = r#"
{
  "properties": {
    "policyRule": {
      "if": { "field": "type", "equals": "Foo" },
      "then": { "effect": "audit" }
    }
  }
}"#;
        let file = SourceFile {
            path: "policies/my_policy.json".to_string(),
            language: Language::new("azure-policy"),
            text: json.to_string(),
        };
        let ex = azure_extractor().extract(&file).expect("must parse");
        let ruleset = ex
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::RuleSet)
            .expect("RuleSet must exist");
        assert_eq!(ruleset.name, "my_policy", "must fall back to file stem");
    }

    // ── AWS Config Rule ───────────────────────────────────────────────────────

    const AWS_CONFIG_JSON: &str = r#"
{
  "ConfigRuleName": "restricted-ssh",
  "Description": "Checks that security groups do not allow unrestricted SSH.",
  "Scope": {
    "ComplianceResourceTypes": [
      "AWS::EC2::SecurityGroup"
    ]
  },
  "Source": {
    "Owner": "AWS",
    "SourceIdentifier": "INCOMING_SSH_DISABLED"
  }
}
"#;

    fn aws_extractor() -> AwsConfigRuleExtractor {
        AwsConfigRuleExtractor::new()
    }

    fn aws_file() -> SourceFile {
        SourceFile {
            path: "rules/restricted_ssh.json".to_string(),
            language: Language::new("aws-config-rule"),
            text: AWS_CONFIG_JSON.to_string(),
        }
    }

    #[test]
    fn aws_config_emits_correct_node_count() {
        // RuleSet + Rule + Condition + 1 Fact = 4
        let ex = aws_extractor()
            .extract(&aws_file())
            .expect("aws config rule must parse");
        assert_eq!(
            ex.nodes.len(),
            4,
            "expected 4 nodes; got: {:?}",
            ex.nodes
                .iter()
                .map(|n| (&n.kind, &n.name))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn aws_config_node_kinds() {
        let ex = aws_extractor()
            .extract(&aws_file())
            .expect("aws config rule must parse");
        let kinds: Vec<&NodeKind> = ex.nodes.iter().map(|n| &n.kind).collect();
        assert!(kinds.contains(&&NodeKind::RuleSet), "must have RuleSet");
        assert!(kinds.contains(&&NodeKind::Rule), "must have Rule");
        assert!(kinds.contains(&&NodeKind::Condition), "must have Condition");
        assert!(kinds.contains(&&NodeKind::Fact), "must have Fact");
    }

    #[test]
    fn aws_config_rule_name() {
        let ex = aws_extractor()
            .extract(&aws_file())
            .expect("aws config rule must parse");
        let rule = ex
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Rule)
            .expect("Rule node must exist");
        assert_eq!(rule.name, "restricted-ssh");
    }

    #[test]
    fn aws_config_fact_name() {
        let ex = aws_extractor()
            .extract(&aws_file())
            .expect("aws config rule must parse");
        let fact = ex
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Fact)
            .expect("Fact node must exist");
        assert_eq!(fact.name, "AWS::EC2::SecurityGroup");
    }

    #[test]
    fn aws_config_condition_is_source_identifier() {
        let ex = aws_extractor()
            .extract(&aws_file())
            .expect("aws config rule must parse");
        let cond = ex
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Condition)
            .expect("Condition node must exist");
        assert_eq!(cond.name, "INCOMING_SSH_DISABLED");
    }

    #[test]
    fn aws_config_edges_correct() {
        let ex = aws_extractor()
            .extract(&aws_file())
            .expect("aws config rule must parse");
        // Contains: RuleSet→Rule, Rule→Condition = 2
        // Evaluates: Rule→Fact = 1
        // Total = 3
        assert_eq!(
            ex.local_edges.len(),
            3,
            "expected 3 edges; got: {:?}",
            ex.local_edges.iter().map(|e| &e.kind).collect::<Vec<_>>()
        );
        let contains_count = ex
            .local_edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Contains)
            .count();
        let evaluates_count = ex
            .local_edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Evaluates)
            .count();
        assert_eq!(contains_count, 2, "expected 2 Contains edges");
        assert_eq!(evaluates_count, 1, "expected 1 Evaluates edge");
    }

    #[test]
    fn aws_config_multiple_resource_types() {
        let json = r#"
{
  "ConfigRuleName": "multi-rule",
  "Scope": {
    "ComplianceResourceTypes": ["AWS::S3::Bucket", "AWS::EC2::Instance", "AWS::IAM::Role"]
  },
  "Source": { "Owner": "AWS", "SourceIdentifier": "MULTI_CHECK" }
}
"#;
        let file = SourceFile {
            path: "rules/multi.json".to_string(),
            language: Language::new("aws-config-rule"),
            text: json.to_string(),
        };
        let ex = aws_extractor().extract(&file).expect("must parse");
        let fact_count = ex.nodes.iter().filter(|n| n.kind == NodeKind::Fact).count();
        assert_eq!(fact_count, 3, "expected 3 Fact nodes; got {fact_count}");
        let evaluates_count = ex
            .local_edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Evaluates)
            .count();
        assert_eq!(
            evaluates_count, 3,
            "expected 3 Evaluates edges; got {evaluates_count}"
        );
    }

    #[test]
    fn aws_config_non_json_returns_empty() {
        let file = SourceFile {
            path: "rule.yaml".to_string(),
            language: Language::new("aws-config-rule"),
            text: "ConfigRuleName: foo".to_string(),
        };
        let ex = aws_extractor()
            .extract(&file)
            .expect("must not error for non-json");
        assert!(ex.nodes.is_empty());
    }

    #[test]
    fn aws_config_non_matching_json_returns_empty() {
        let file = SourceFile {
            path: "package.json".to_string(),
            language: Language::new("aws-config-rule"),
            text: r#"{"name":"foo"}"#.to_string(),
        };
        let ex = aws_extractor().extract(&file).expect("must not error");
        assert!(ex.nodes.is_empty());
    }

    #[test]
    fn aws_config_no_scope_still_emits_rule() {
        let json = r#"
{
  "ConfigRuleName": "no-scope-rule",
  "Source": { "Owner": "AWS", "SourceIdentifier": "SOME_CHECK" }
}
"#;
        let file = SourceFile {
            path: "rules/no_scope.json".to_string(),
            language: Language::new("aws-config-rule"),
            text: json.to_string(),
        };
        let ex = aws_extractor().extract(&file).expect("must parse");
        // RuleSet + Rule + Condition = 3
        assert_eq!(
            ex.nodes.len(),
            3,
            "expected 3 nodes with no scope; got {}",
            ex.nodes.len()
        );
        let fact_count = ex.nodes.iter().filter(|n| n.kind == NodeKind::Fact).count();
        assert_eq!(fact_count, 0, "no Fact nodes expected when Scope absent");
    }
}
