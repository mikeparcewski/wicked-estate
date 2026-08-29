//! `wicked-estate-extract` — language coverage registry + (Wave 2.1) tree-sitter `Extractor` impls.
//!
//! **Coverage commitment:** parity with the language set (73 today) **plus the ability
//! to add more without surgery**. Languages are *data* (`languages.toml`), not code — adding one
//! is a manifest row + a `<name>.scm` query file, no core change (rules-as-data; see `CLAUDE.md`).
//!
//! Each language declares its extraction `tier` + `caps`, so the capability matrix is **generated
//! from data**, not hand-maintained — the thing the matrix wanted but never built.
//! The precise axes prior art is stuck on (extends-vs-implements, cross-file refs) are delivered
//! by the SCIP/TSG/LSP resolution tiers (`wicked-estate-resolve`), not by tree-sitter.

pub mod treesitter;
pub use treesitter::{IaCExtractor, SYMBOL_ID_SCHEME, TreeSitterExtractor};

/// Runtime language plugins — drop-in tree-sitter grammars loaded from a plugins directory.
pub mod plugin;

pub mod extra_edge;
pub use extra_edge::{EdgeRule, ExtraEdgeExtractor, ExtraExtraction};

pub mod tfstate;
pub use tfstate::TfstateCollector;

pub mod jcl;
pub use jcl::JclExtractor;

pub mod hlasm;
pub use hlasm::HlasmExtractor;

pub mod racf;
pub use racf::RacfExtractor;

pub mod ims;
pub use ims::ImsExtractor;

pub mod mq;
pub use mq::MqExtractor;

pub mod cics_sql;
pub use cics_sql::CicsSqlExtractor;

pub mod json_rules;
pub use json_rules::{AwsConfigRuleExtractor, AzurePolicyExtractor};

pub mod odm;
pub use odm::OdmExtractor;

// Shared length-preserving comment/string maskers for the heuristic rules extractors.
mod rules_text;

// W15.9 — Drools DRL (`.drl`) heuristic rules extractor (no feature gate; pure regex).
pub mod drl;
pub use drl::DrlExtractor;

// W15.8 — OPA/Rego (`.rego`) rules-layer extractor (supplements the tree-sitter code parse).
pub mod rego_rules;
pub use rego_rules::RegoRulesExtractor;

// W15.11 — FICO Blaze Advisor `.brl` SRL heuristic extractor (no feature gate; pure regex).
pub mod blaze_brl;
pub use blaze_brl::BlazeBrlExtractor;

// W15.2 — XML rules extractor (feature-gated: `xml-rules`).
#[cfg(feature = "xml-rules")]
pub mod xml_rules;
#[cfg(feature = "xml-rules")]
pub use xml_rules::{EdgeMapping, EngineConfig, NodeMapping, XmlRulesConfig, XmlRulesExtractor};

// W15.4 — Camunda DMN extractor (feature-gated: `xml-rules`).
#[cfg(feature = "xml-rules")]
pub mod dmn;
#[cfg(feature = "xml-rules")]
pub use dmn::CamundaDmnExtractor;

// W15.5 — Drools GDST guided decision table extractor (feature-gated: `xml-rules`).
#[cfg(feature = "xml-rules")]
pub mod drools_gdst;
#[cfg(feature = "xml-rules")]
pub use drools_gdst::DroolsGdstExtractor;

// W15.6 — Salesforce Flow extractor (feature-gated: `xml-rules`).
#[cfg(feature = "xml-rules")]
pub mod salesforce_flow;
#[cfg(feature = "xml-rules")]
pub use salesforce_flow::SalesforceFlowExtractor;

// W15.10 — Progress Corticon `.ers`/`.erf`/`.ecore` extractor (feature-gated: `xml-rules`).
#[cfg(feature = "xml-rules")]
pub mod corticon;
#[cfg(feature = "xml-rules")]
pub use corticon::CorticonExtractor;

// W15.3 — Excel/XLSX decision table extractor (feature-gated: `excel-rules`).
#[cfg(feature = "excel-rules")]
pub mod excel_rules;
#[cfg(feature = "excel-rules")]
pub use excel_rules::{
    ColumnConfig, ColumnRole, ExcelEngineConfig, ExcelRulesConfig, ExcelRulesExtractor, SheetConfig,
};

pub mod clips;
pub use clips::ClipsExtractor;

pub mod cloud;
pub use cloud::{
    CloudCollector, CloudConfig, CloudProvider, CloudResource, MockCloudCollector,
    cloud_resources_to_nodes, open_cloud_collector,
};

use serde::Deserialize;

/// Extraction depth a language reaches at the tree-sitter layer. Precise axes come from resolvers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtractTier {
    /// Config/markup — indexed as a document, no code symbols.
    Document,
    /// Grammar registered, captures not yet wired.
    Detected,
    /// Symbols only.
    Tags,
    /// Symbols + calls + imports (+ heritage where the grammar distinguishes it).
    Structural,
    /// Structural + precise cross-file refs / extends-vs-implements (via SCIP/TSG/LSP).
    Precise,
}

/// A capture family a language's query file provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtractCap {
    Symbols,
    Calls,
    Imports,
    Extends,
    Implements,
    /// Framework-relationship edges carried on `EdgeKind::Other` (DI wiring, route handlers,
    /// event pub/sub) — produced by the generic `@di.*` / `@route.*` / `@event.*` capture roles
    /// when a language's `.scm` opts in. Advertised in the generated capability matrix so
    /// consumers know a language surfaces framework edges, not just structural ones.
    Framework,
}

/// One row of the language manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct LanguageSpec {
    pub name: String,
    /// Source-file extensions (no dot). May be empty pending W2.1 wiring.
    #[serde(default)]
    pub ext: Vec<String>,
    /// tree-sitter grammar crate (e.g. `tree-sitter-rust`).
    pub grammar: String,
    pub tier: ExtractTier,
    #[serde(default)]
    pub caps: Vec<ExtractCap>,
}

impl LanguageSpec {
    pub fn supports(&self, cap: ExtractCap) -> bool {
        self.caps.contains(&cap)
    }
}

#[derive(Debug, Deserialize)]
struct Manifest {
    language: Vec<LanguageSpec>,
}

/// The manifest is embedded at build time; regenerate with `scripts/gen-language-manifest.py`.
const MANIFEST: &str = include_str!("../languages.toml");

/// All registered languages. Adding a language is a manifest row, not a code change.
pub fn registry() -> Vec<LanguageSpec> {
    let m: Manifest = toml::from_str(MANIFEST).expect("languages.toml must be valid TOML");
    m.language
}

/// Look up a language by source-file extension (leading dot optional, case-insensitive).
pub fn by_extension(ext: &str) -> Option<LanguageSpec> {
    let needle = ext.trim_start_matches('.').to_lowercase();
    registry()
        .into_iter()
        .find(|l| l.ext.iter().any(|e| e == &needle))
}

/// Look up a language by name.
pub fn by_name(name: &str) -> Option<LanguageSpec> {
    registry().into_iter().find(|l| l.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_language_parity() {
        let r = registry();
        assert!(
            r.len() >= 73,
            "must cover >= the 73 languages, got {}",
            r.len()
        );
    }

    #[test]
    fn manifest_is_well_formed() {
        for l in registry() {
            assert!(!l.name.is_empty());
            // tree-sitter-* (official crates) or arborium-* (the arborium grammar family) — both
            // are tree-sitter grammars; arborium just packages ABI-15 parsers under its own prefix.
            assert!(
                l.grammar.starts_with("tree-sitter-") || l.grammar.starts_with("arborium-"),
                "grammar '{}' must be a tree-sitter or arborium grammar",
                l.grammar
            );
        }
    }

    #[test]
    fn typescript_is_structural_with_full_caps() {
        let ts = by_name("typescript").expect("typescript present");
        assert_eq!(ts.tier, ExtractTier::Structural);
        for c in [ExtractCap::Symbols, ExtractCap::Calls, ExtractCap::Imports] {
            assert!(ts.supports(c), "typescript should support {c:?}");
        }
    }

    #[test]
    fn java_advertises_framework_edges() {
        // Java emits framework-relationship edges (DI wiring, route handlers, event pub/sub) via
        // the generic @di.*/@route.*/@event.* capture roles, so the generated capability matrix
        // must advertise the `Framework` cap — not just the structural ones.
        let java = by_name("java").expect("java present");
        assert!(
            java.supports(ExtractCap::Framework),
            "java must advertise framework edges; caps={:?}",
            java.caps
        );
        // It still advertises the structural caps it had before.
        for c in [
            ExtractCap::Symbols,
            ExtractCap::Calls,
            ExtractCap::Imports,
            ExtractCap::Extends,
            ExtractCap::Implements,
        ] {
            assert!(java.supports(c), "java should still support {c:?}");
        }
    }

    #[test]
    fn lookup_by_extension_works() {
        assert_eq!(by_extension("rs").map(|l| l.name), Some("rust".to_string()));
        assert_eq!(
            by_extension(".py").map(|l| l.name),
            Some("python".to_string())
        );
    }
}
