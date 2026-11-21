//! `wicked-estate` (lib) — the indexing pipeline + query helpers.
//!
//! `index_path` wires **EXTRACT** (`wicked-estate-extract`) → **RESOLVE** (`wicked-estate-resolve`) → **STORE**
//! (`wicked-estate-store`) entirely through the `wicked-estate-core` traits, so it runs against any `GraphStore`
//! (including the `Box<dyn GraphStore>` from `wicked_estate_store::open_store`, i.e. a future external DB).
//!
//! ## Incremental indexing (Wave 2.6)
//!
//! On re-index, CHANGED or NEW files and callers affected by homonym changes are re-extracted.
//! Other UNCHANGED files (same xxh3 digest) are skipped — their nodes, edges, and unresolved refs
//! remain in the store. DELETED files (previously indexed but no longer present) have contributions
//! removed via `store.remove_file(path)`.
//!
//! "Previously indexed" means [`GraphRead::indexed_files`] — this indexer's own digest rows — never
//! "every node in the store". A store may be shared with a writer that keeps non-source nodes in
//! it; sweeping those is data loss, not incremental indexing (FINDING-067).
//!
//! ### Importer re-extraction scope (lane relative-imports, Decision J)
//!
//! - **DELETED (or renamed) file**: its direct importers — files with a `File → File` `Imports`
//!   edge into it — are FORCED through re-extraction in the same run, so their relative-import
//!   refs re-park honestly instead of silently losing the edge to `prune_dangling_edges`.
//!   The importer set is collected BEFORE the removal batch (the batch destroys the very edges
//!   the discovery walks) and only from the original deleted list — no transitive cascade.
//! - **MODIFIED file**: nothing is forced. The importer's `File → File` edge survives
//!   `remove_file` by store semantics (the DELETE matches `file = ?1 OR source IN
//!   nodes-of-file`; the importer's edge has `file = importer`), and the target's File node is
//!   re-created under the same path-keyed SymbolId before `prune_dangling_edges` runs — the
//!   edge stays valid with zero importer re-extraction. Forcing importers on modification
//!   would re-parse every importer of a hub file on every save, for nothing.
//!
//! ### Parked-ref back-fill (lane importer-backfill, #141)
//!
//! The reverse of Decision J — Decision J walks existing edges to re-park refs when their target
//! is DELETED; a PARKED ref waiting on a target that later APPEARS has no edge to be discovered
//! by (D01-7 audit). The back-fill pass closes that: after the main resolve pass, exactly the
//! parked rows this run's additions or removals could bind are re-resolved:
//!
//! - **name lane**: each distinct extracted or lost name is probed against the indexed
//!   `unresolved_refs.raw_name` column (one exact indexed lookup per name — never a table scan),
//!   so an unchanged file F whose call to S parked gets its edge the run a changed file defines S;
//! - **import lane**: when the run indexes at least one NEW file path, the parked relative-import
//!   rows are fetched in one kind-scoped pass and re-resolved against the File map, so a parked
//!   `./b` binds the run `b.ts` appears (target-add, or completing a rename whose importers were
//!   fixed in an earlier run).
//!
//! A re-resolved row is DELETED in the same batch that writes its edge; a still-parked candidate
//! is left in place, never re-inserted — the pass is idempotent (edge upsert dedups by key), and
//! its counts are logged (`BACKFILL: …`) and attached to the index span.
//!
//! The rebind scan considers only resolver-produced edges into structural same-name targets
//! whose name gains or loses a structural candidate. Their unchanged owners are re-extracted
//! once, except scoped edges whose winning tier excludes the changed candidates. This
//! over-approximates import-map and cross-file bindings: even when an import keeps the edge bound,
//! a same-name candidate gain or loss re-extracts its owner. An unfinished prior run forces full
//! re-extraction, including after digest commits, so every post-write repair runs again (#404).
//!
//! ### Known limitations (still true)
//!
//! Stored parked rows carry no hints (only the 7 site columns persist), so hint-driven
//! resolution (`ImportMapResolver`) never fires on back-fill. Those refs self-heal when their
//! file changes or a full re-index runs (#141).
//!
//! ## Many repos, one graph
//!
//! [`index_path_as`] takes an optional repo label. With one, every path this run stores is
//! namespaced (`ledger/src/lib.rs`) — including the paths embedded in SymbolIds — so several repos
//! co-exist in one db without colliding, each with its own provenance and its own delete-sweep
//! scope. Without one, nothing is prefixed and the run is byte-identical to before. Either way the
//! [`repo_scope::guard`] refuses, before writing anything, an index that would overwrite another
//! repo's rows. Co-location only: **edges do not resolve across repos** — resolution is scoped to
//! the indexed repo's own nodes. See [`repo_scope`].

// (estate#333, wicked-core#957) A diagnostic line never kills the thread that writes it. std's
// `eprintln!` PANICS when the write fails, and a Node host (wicked-crew loading wicked-core through
// its napi addon, which links this crate) hands the process a stderr that is a non-blocking pipe:
// when the reader lags, the write fails with EAGAIN and the thread dies with no trace. This
// crate-wide shadow (textual scope: every module declared below) writes the same line and drops it
// when stderr refuses it. Unit tests keep std's macro, whose output the test harness captures.
#[cfg(not(test))]
macro_rules! eprintln {
    ($($arg:tt)*) => {{
        use ::std::io::Write as _;
        let _ = ::std::writeln!(::std::io::stderr(), $($arg)*);
    }};
}

pub mod confined;
pub mod freshness;
pub mod link;
pub mod repo_scope;
pub mod writer_lock;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use rayon::prelude::*;
use wicked_estate_core::{
    ChangeOp, Edge, EdgeKind, Extraction, Extractor, FlowEvidence, FlowFact, FlowSemantics,
    GraphRead, GraphStats, Language, Location, Node, NodeKind, NodeSemantics, RepoInfo,
    ResolutionTier, Resolver, Result, SourceFile, Span, Symbol, SymbolId, SymbolIndex, SymbolQuery,
    TraversalSpec, edge_tags, is_flow_edge, is_structural_symbol, merge_flow_edges,
};
use wicked_estate_extract::{
    BlazeBrlExtractor, DrlExtractor, ExtraEdgeExtractor, HlasmExtractor, IaCExtractor,
    ImsExtractor, JclExtractor, MqExtractor, RacfExtractor, RegoRulesExtractor, SYMBOL_ID_SCHEME,
    TfstateCollector, doc_convert,
    treesitter::{ExtractorCache, is_minified_or_huge},
};
use wicked_estate_resolve::{
    ApexlangRefResolver, DynamicCallResolver, ImportMapResolver, InfraResolver, NameResolver,
    RulesBridgeResolver, ScopedNameResolver, resolve_all_with_coverage,
};
use wicked_estate_retrieve::Embedder;
use wicked_estate_store::GraphStoreMutExt;
use wicked_estate_store::SqliteStore;
use xxhash_rust::xxh3::xxh3_64;

/// Extensions handled by the grammar-less line extractors (JCL job streams and cataloged PROCs,
/// HLASM assembler), whose dispatch lives in `index_path` rather than the tree-sitter `ext_map`.
pub(crate) fn is_grammarless_ext(ext: &str) -> bool {
    matches!(
        ext,
        "jcl"
            | "job"
            | "cntl"
            | "prc"
            | "proc"
            | "hlasm"
            | "asm"
            | "mlc"
            | "bms"
            | "racf"
            | "dbd"
            | "psb"
            | "mqsc"
            | "drl"
            | "brl"
    ) || is_xml_rules_ext(ext)
}

/// VB5/VB6 controls start with a VERSION header; mainframe control cards do not.
fn is_vb6_source(bytes: &[u8]) -> bool {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    bytes
        .split(|&b| b == b'\n')
        .find(|line| !line.iter().all(u8::is_ascii_whitespace))
        .is_some_and(|line| {
            line.get(..8)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"VERSION "))
        })
}

const NOT_INDEXED_SECRET: &str = "matches the secret-file rule; never indexed";
const NOT_INDEXED_READ: &str = "could not be read inside the repository root";
const NOT_INDEXED_CTL: &str =
    "mainframe control card (not VB6): no extractor yet (wicked-estate#447)";

fn not_indexed_reason(ext: &str) -> &'static str {
    match ext {
        "ddl" | "db2" => "Db2 DDL: no DDL extractor yet (wicked-estate#447)",
        "csd" => "CICS CSD resource definitions: no extractor yet (wicked-estate#447)",
        "mac" => "HLASM macro library member: no macro extractor yet (wicked-estate#447)",
        "ca7" | "controlm" => "scheduler definition: no extractor yet (wicked-estate#447)",
        "ctl" => NOT_INDEXED_CTL,
        _ => "no extractor for this file type",
    }
}

/// Dispatch a grammar-less (non-tree-sitter) extractor by file extension — the mainframe estate
/// languages (JCL, HLASM, RACF security, IMS DBD/PSB, MQ MQSC) plus the heuristic rules-engine
/// formats (Drools DRL, FICO Blaze `.brl`) and, behind the `xml-rules` feature, the XML rules
/// formats (Camunda DMN, Progress Corticon). Returns `None` for everything else.
fn grammarless_extractor(ext: &str) -> Option<Box<dyn Extractor>> {
    match ext {
        "jcl" | "job" | "cntl" | "prc" | "proc" => Some(Box::new(JclExtractor::new())),
        "hlasm" | "asm" | "mlc" | "bms" => Some(Box::new(HlasmExtractor::new())),
        "racf" => Some(Box::new(RacfExtractor::new())),
        "dbd" | "psb" => Some(Box::new(ImsExtractor::new())),
        "mqsc" => Some(Box::new(MqExtractor::new())),
        "drl" => Some(Box::new(DrlExtractor::new())),
        "brl" => Some(Box::new(BlazeBrlExtractor::new())),
        _ => xml_rules_extractor(ext),
    }
}

/// XML rules-engine extensions, dispatched only when the `xml-rules` feature is enabled.
#[cfg(feature = "xml-rules")]
fn is_xml_rules_ext(ext: &str) -> bool {
    matches!(ext, "ers" | "erf" | "ecore" | "dmn")
}
#[cfg(not(feature = "xml-rules"))]
fn is_xml_rules_ext(_ext: &str) -> bool {
    false
}

/// Dispatch the XML rules-engine extractors (Progress Corticon, Camunda DMN). Compiled in only with
/// the `xml-rules` feature; a no-op (always `None`) otherwise so the base binary stays dep-light.
#[cfg(feature = "xml-rules")]
fn xml_rules_extractor(ext: &str) -> Option<Box<dyn Extractor>> {
    use wicked_estate_extract::{CamundaDmnExtractor, CorticonExtractor};
    match ext {
        "ers" | "erf" | "ecore" => Some(Box::new(CorticonExtractor::new())),
        "dmn" => Some(Box::new(CamundaDmnExtractor::new())),
        _ => None,
    }
}
#[cfg(not(feature = "xml-rules"))]
fn xml_rules_extractor(_ext: &str) -> Option<Box<dyn Extractor>> {
    None
}

/// Language-based extraction dispatch for ONE file: grammar-less line extractors first, then the
/// IaC sniff for YAML/JSON, then tree-sitter by extension. Returns `None` when no extractor claims
/// the file or extraction fails — the caller decides whether the file still matters (e.g. because
/// a drop-in extra-edge rule targets it).
fn base_extraction(
    rel_path: &str,
    ext: &str,
    text: &str,
    ext_map: &ExtractorCache,
) -> Option<Extraction> {
    // Grammar-less mainframe estate languages (JCL batch, HLASM assembler, RACF security,
    // IMS DBD/PSB data, MQ MQSC messaging): line/macro extractors dispatched by extension.
    // They map the estate into the same graph as the tree-sitter languages.
    if let Some(extractor) = grammarless_extractor(ext) {
        let language = extractor.languages().into_iter().next()?;
        let sf = SourceFile {
            path: rel_path.to_string(),
            language: language.clone(),
            text: text.to_string(),
        };
        let mut extraction = extractor.extract(&sf).ok()?;
        // Give every grammar-less file the same File node + Contains edges the tree-sitter
        // path emits, so the indexed-file count is honest and file-level blast-radius
        // ("what's defined in queues.mqsc?") works uniformly across all languages.
        let file_symbol = Symbol::file(rel_path).id();
        let floc = Location::new(rel_path, Span::ZERO);
        let child_syms: Vec<SymbolId> = extraction.nodes.iter().map(|n| n.symbol.clone()).collect();
        for sym in child_syms {
            extraction.local_edges.push(
                Edge::new(
                    file_symbol.clone(),
                    sym,
                    EdgeKind::Contains,
                    ResolutionTier::Parsed,
                    "grammarless",
                )
                .with_location(floc.clone()),
            );
        }
        extraction.nodes.push(Node::new(
            file_symbol,
            NodeKind::File,
            rel_path.to_string(),
            language,
            floc,
        ));
        return Some(extraction);
    }

    // IaC sniff dispatch for YAML / JSON files: cheap string check before extraction.
    // CloudFormation: top-level `Resources:` key.
    // Kubernetes: `kind:` + `apiVersion:` keys present.
    // All other YAML/JSON: fall through to the normal tree-sitter extractor.
    if matches!(ext, "yaml" | "yml" | "json") {
        let is_cfn = text.contains("Resources:")
            && (text.contains("AWSTemplateFormatVersion")
                || text.contains("CloudFormation")
                || text
                    .lines()
                    .any(|l| l.trim_start_matches(' ') == "Resources:"));
        let is_k8s = text.contains("kind:") && text.contains("apiVersion:");

        if is_cfn {
            let extractor = IaCExtractor::cloudformation();
            let language = extractor.languages().into_iter().next()?;
            let sf = SourceFile {
                path: rel_path.to_string(),
                language,
                text: text.to_string(),
            };
            return extractor.extract(&sf).ok();
        } else if is_k8s {
            let extractor = IaCExtractor::kubernetes();
            let language = extractor.languages().into_iter().next()?;
            let sf = SourceFile {
                path: rel_path.to_string(),
                language,
                text: text.to_string(),
            };
            return extractor.extract(&sf).ok();
        }
        // Non-IaC YAML/JSON — fall through to normal extractor below.
    }

    let extractor = ext_map.get(ext)?;
    let language = extractor
        .languages()
        .into_iter()
        .next()
        .expect("extractor has a language");
    let sf = SourceFile {
        path: rel_path.to_string(),
        language,
        text: text.to_string(),
    };
    let mut extraction = extractor.extract(&sf).ok()?;
    // COBOL: add what the grammar cannot see (EXEC CICS / EXEC SQL are opaque nodes, COPY is a
    // compiler directive): program → program / map / table / copybook / DD-binding links, field
    // uses, dynamic CALL literals. Every link starts from this extraction's program Module (or
    // copybook) node — see `cobol_links`.
    if matches!(ext, "cbl" | "cob" | "cobol" | "cpy" | "dcl") {
        wicked_estate_extract::cobol_links::supplement(&sf, &mut extraction);
    }
    // Rego: supplement the tree-sitter code parse (rules-as-functions) with the W15 rules
    // graph (RuleSet/Rule/Condition/Action/Fact), so policies surface in RulesInventory.
    if ext == "rego" {
        if let Ok(rules) = RegoRulesExtractor::new().extract(&sf) {
            extraction.nodes.extend(rules.nodes);
            extraction.local_edges.extend(rules.local_edges);
            extraction.refs.extend(rules.refs);
        }
    }
    Some(extraction)
}

/// Symbol index built ONCE from the store for the resolver pass. The resolver calls `by_name`
/// once per reference; at ~190k refs × N resolvers, a per-call SQL lookup was the dominant cost
/// (≈0.5M queries). Loading `all_nodes()` into in-memory maps makes resolution lookups O(1).
struct InMemoryIndex {
    by_name: HashMap<String, Vec<Node>>,
    by_id: HashMap<SymbolId, Node>,
    /// language name → family, from `wicked_estate_extract::registry()` (the manifest is the
    /// data source — D5: the family table is DATA, not per-language Rust arms) PLUS the
    /// content-sniffed IaC logical languages (`wicked_estate_extract::LOGICAL_LANGUAGES` —
    /// kubernetes/cloudformation, own-name families, so a code Calls ref cannot name-bind to a
    /// resource node; `InfraResolver`'s exclusive-resource-name bind has no family check and is
    /// unaffected). Languages absent from BOTH (mainframe extractors, `synthetic`/`tfstate`
    /// tags) stay absent, so `language_family` returns `None` and the resolver guard allows them.
    families: HashMap<String, String>,
}

impl InMemoryIndex {
    /// `scope` restricts the index to one repo's nodes (path prefix `<label>/`). A labelled index
    /// resolves against its OWN repo only — same candidate set it would see in a private db, so
    /// co-locating repos neither creates cross-repo edges nor makes a name ambiguous that was
    /// unique before. `None` = every node, the single-repo behaviour.
    fn build(store: &dyn GraphRead, scope: Option<&str>) -> Result<Self> {
        let mut by_name: HashMap<String, Vec<Node>> = HashMap::new();
        let mut by_id = HashMap::new();
        for n in store.all_nodes()? {
            if scope.is_some_and(|p| !n.location.file.starts_with(p)) {
                continue;
            }
            by_name.entry(n.name.clone()).or_default().push(n.clone());
            by_id.insert(n.symbol.clone(), n);
        }
        let mut families: HashMap<String, String> = wicked_estate_extract::registry()
            .into_iter()
            .map(|l| {
                let fam = l.family().to_string();
                (l.name, fam)
            })
            .collect();
        // The IaC logical languages are registered outside the manifest (content-sniffed, no
        // extension); give each an OWN-NAME family from the same single-source const the
        // extractor uses, so the D5 guard sees them (F-A sibling class, §11).
        for name in wicked_estate_extract::LOGICAL_LANGUAGES {
            families.insert((*name).to_string(), (*name).to_string());
        }
        Ok(Self {
            by_name,
            by_id,
            families,
        })
    }

    /// Every node, for passes that derive edges from the whole population (the estate join) —
    /// reuses this already-loaded index instead of re-reading `all_nodes()` from the store.
    fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.by_id.values()
    }
}

impl SymbolIndex for InMemoryIndex {
    fn by_name(&self, name: &str) -> Vec<Node> {
        self.by_name.get(name).cloned().unwrap_or_default()
    }
    fn get(&self, id: &SymbolId) -> Option<Node> {
        self.by_id.get(id).cloned()
    }
    fn all_nodes(&self) -> wicked_estate_core::Result<Vec<Node>> {
        Ok(self.by_id.values().cloned().collect())
    }
    fn language_family(&self, language: &str) -> Option<String> {
        self.families.get(language).cloned()
    }
}

fn value_symbol(owner: &SymbolId, role: &str, name: &str) -> SymbolId {
    Symbol::synthetic("value", format!("{}:{role}:{name}", owner.0)).id()
}

fn value_node(
    symbol: SymbolId,
    kind: NodeKind,
    name: impl Into<String>,
    language: Language,
    location: Location,
    role: &str,
) -> Node {
    Node::new(symbol, kind, name.into(), language, location).with_value_role(role)
}

/// One `File`→value-node `Contains` edge, so a call-derived value node is reachable from a real
/// code node instead of sitting on an island (wicked-estate#207 review, C13) and `remove_file`
/// can retire it. Mirrors the tree-sitter emitter; Contains stays File→node (D4).
fn value_containment_edge(node: &Node) -> Edge {
    Edge::new(
        Symbol::file(&node.location.file).id(),
        node.symbol.clone(),
        EdgeKind::Contains,
        ResolutionTier::Parsed,
        "call-value-flow",
    )
    .with_location(Location::new(&node.location.file, Span::ZERO))
}

/// A call-derived flow edge. It inherits the CAUSAL call edge's confidence, provenance and
/// `resolved_by` wholesale: the callee being uniquely selected says nothing about how well that
/// call was resolved, so a 0.5 `name-resolver` binding yields 0.5 flow, never Parsed
/// (`docs/ENGINE-CONTRACT.md` §3.2). The classification is orthogonal — argument→parameter and
/// return→callsite both carry the producer's value whole, hence `ValuePreserving`.
fn flow_edge(
    source: SymbolId,
    target: SymbolId,
    call_edge: &Edge,
    construct: &str,
    semantics: FlowSemantics,
    evidence: FlowEvidence,
) -> Edge {
    let mut edge = Edge::new(
        source,
        target,
        edge_tags::other(edge_tags::FLOWS_TO),
        ResolutionTier::Heuristic,
        call_edge.resolved_by.clone(),
    );
    edge.confidence = call_edge.confidence;
    edge.provenance = call_edge.provenance.clone();
    edge.location = call_edge.location.clone();
    FlowFact::new(semantics, evidence, construct, CALL_FLOW_PRODUCER).apply(&mut edge);
    edge
}

/// Rule-id producer segment for engine-derived (not query-file-derived) flow facts. These are
/// language-independent — the call-resolution pipeline, not a `.scm` — so they are not namespaced
/// by language.
const CALL_FLOW_PRODUCER: &str = "engine";

type CallSiteKey = (String, u32, u32, u32);

fn call_site_key(location: &Location) -> CallSiteKey {
    (
        location.file.clone(),
        location.span.start_line,
        location.span.start_byte,
        location.span.end_byte,
    )
}

fn call_value_ref_hints(
    refs: &[wicked_estate_core::UnresolvedRef],
) -> HashMap<CallSiteKey, serde_json::Value> {
    refs.iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .filter_map(|r| {
            r.hints
                .get("value_flow")
                .cloned()
                .map(|hint| (call_site_key(&r.location), hint))
        })
        .collect()
}

/// What a callee-only edit must leave alone for the #220 replay to equal a re-extraction of the
/// callers: the definitions caller resolution can see, and the return endpoints a recorded
/// `call_result` hop can stand on.
#[derive(Debug, Default)]
struct ReplaySignature {
    /// `(symbol, kind)` of every definition node. File and Import nodes are left out: the File
    /// node never changes id, and an Import node is shared by specifier and homed on whichever
    /// importer sorts first, so it would read as a change on nearly every edit.
    defs: std::collections::BTreeSet<(String, String)>,
    returns: std::collections::BTreeSet<String>,
}

impl ReplaySignature {
    fn add(&mut self, node: &Node) {
        if node.is_value_flow_node() {
            if node.metadata.get("value_role").and_then(|v| v.as_str()) == Some("Return") {
                self.returns.insert(node.symbol.0.clone());
            }
        } else if !matches!(node.kind, NodeKind::File | NodeKind::Import) {
            self.defs
                .insert((node.symbol.0.clone(), format!("{:?}", node.kind)));
        }
    }

    /// Same definitions, and no return endpoint lost (a gained one only adds hops).
    fn replay_is_exact(&self, new: &Self) -> bool {
        self.defs == new.defs && self.returns.is_subset(&new.returns)
    }
}

/// A Calls ref that carries call-site value-flow facts — the refs [`call_value_flow`] reads.
fn is_value_flow_call_ref(r: &wicked_estate_core::UnresolvedRef) -> bool {
    r.kind == EdgeKind::Calls && r.hints.contains_key("value_flow")
}

/// The value-flow call refs `file`'s last extraction recorded (#220), or `None` when the store
/// holds no readable record — an older DB, a backend that does not persist them, or a record
/// that does not parse. `None` always means "re-extract to recover them", never "there are none".
fn recorded_call_refs(
    store: &dyn GraphRead,
    file: &str,
) -> Result<Option<Vec<wicked_estate_core::UnresolvedRef>>> {
    Ok(store
        .file_call_refs(file)?
        .and_then(|json| serde_json::from_str(&json).ok()))
}

fn call_value_ref_hints_from_files(
    store: &dyn GraphRead,
    files: impl IntoIterator<Item = String>,
    ext_map: &ExtractorCache,
) -> Result<HashMap<CallSiteKey, serde_json::Value>> {
    let mut refs = Vec::new();
    for file in files {
        // #220: the recorded refs are exactly what re-parsing would produce; parse only without.
        if let Some(recorded) = recorded_call_refs(store, &file)? {
            refs.extend(recorded);
            continue;
        }
        let Some(text) = store.file_content(&file)? else {
            continue;
        };
        let ext = Path::new(&file)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if let Some(extraction) = base_extraction(&file, &ext, &text, ext_map) {
            refs.extend(extraction.refs);
        }
    }
    Ok(call_value_ref_hints(&refs))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueFlowHintKind {
    Local,
    Field,
    Parameter,
}

impl ValueFlowHintKind {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "local" => Some(Self::Local),
            "field" => Some(Self::Field),
            "parameter" => Some(Self::Parameter),
            _ => None,
        }
    }

    fn node_kind(self) -> NodeKind {
        match self {
            Self::Field => NodeKind::Field,
            Self::Parameter => NodeKind::Parameter,
            Self::Local => NodeKind::Variable,
        }
    }

    fn role(self) -> &'static str {
        match self {
            Self::Field => "Field",
            Self::Parameter => "Parameter",
            Self::Local => "Local",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ValueFlowHintError {
    key: String,
    kind: String,
}

fn value_flow_hint_decode_error(
    site: &CallSiteKey,
    error: ValueFlowHintError,
) -> wicked_estate_core::Error {
    wicked_estate_core::Error::Invalid(format!(
        "unsupported call value-flow hint kind at {}:{}:{}-{} key={} kind={}",
        site.0, site.1, site.2, site.3, error.key, error.kind
    ))
}

#[derive(Debug)]
struct ValueFlowEndpoint {
    name: String,
    kind: ValueFlowHintKind,
    symbol: Option<SymbolId>,
    slot: Option<usize>,
}

impl ValueFlowEndpoint {
    fn node_kind(&self) -> NodeKind {
        self.kind.node_kind()
    }

    fn role(&self) -> &str {
        self.kind.role()
    }
}

fn value_flow_endpoint_kind(
    raw: &str,
    key: &str,
) -> std::result::Result<ValueFlowHintKind, ValueFlowHintError> {
    ValueFlowHintKind::parse(raw).ok_or_else(|| ValueFlowHintError {
        key: key.to_string(),
        kind: raw.to_string(),
    })
}

fn value_flow_endpoints(
    hint: &serde_json::Value,
    key: &str,
) -> std::result::Result<Vec<ValueFlowEndpoint>, ValueFlowHintError> {
    hint.get(key)
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let raw_kind = item.get("kind")?.as_str()?;
            let name = item.get("name")?.as_str()?.to_string();
            let symbol = item
                .get("symbol")
                .and_then(|v| v.as_str())
                .map(|s| SymbolId(s.to_string()));
            let slot = item
                .get("slot")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            Some(
                value_flow_endpoint_kind(raw_kind, key).map(|kind| ValueFlowEndpoint {
                    name,
                    kind,
                    symbol,
                    slot,
                }),
            )
        })
        .collect()
}

fn sorted_value_flow_endpoints(
    hint: &serde_json::Value,
    key: &str,
) -> std::result::Result<Vec<ValueFlowEndpoint>, ValueFlowHintError> {
    let mut endpoints = value_flow_endpoints(hint, key)?;
    endpoints.sort_by_key(|endpoint| endpoint.slot.unwrap_or(usize::MAX));
    Ok(endpoints)
}

fn value_flow_result(
    hint: &serde_json::Value,
) -> std::result::Result<Option<ValueFlowEndpoint>, ValueFlowHintError> {
    let Some(result) = hint.get("result") else {
        return Ok(None);
    };
    let Some(raw_kind) = result.get("kind").and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    let Some(name) = result
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_string)
    else {
        return Ok(None);
    };
    let symbol = result
        .get("symbol")
        .and_then(|v| v.as_str())
        .map(|s| SymbolId(s.to_string()));
    Ok(Some(ValueFlowEndpoint {
        name,
        kind: value_flow_endpoint_kind(raw_kind, "result")?,
        symbol,
        slot: None,
    }))
}

/// The callee's POSITIONAL parameter slots: one entry per declared parameter, `None` where the
/// extractor captured no value node for it (a destructured or optional parameter). The index IS
/// the argument slot — see the `value_params` note in `wicked-estate-extract` (C5d).
fn callable_param_names(callee: &Node) -> Vec<Option<String>> {
    callee
        .metadata
        .get("value_params")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .map(|v| v.as_str().map(str::to_string))
        .collect()
}

fn call_value_flow(
    site_edges: &[Edge],
    index: &InMemoryIndex,
    ref_hints: &HashMap<CallSiteKey, serde_json::Value>,
) -> Result<(Vec<Node>, Vec<Edge>)> {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    // #209: a BTreeMap, not a HashMap — two call sites in one owner that pass the same identifier
    // mint the same value node, and the FIRST site (in key order) must win on every run; hash
    // order made the stored location differ index to index.
    let mut by_site: std::collections::BTreeMap<CallSiteKey, HashMap<SymbolId, Edge>> =
        std::collections::BTreeMap::new();

    for call_edge in site_edges {
        if call_edge.kind != EdgeKind::Calls {
            continue;
        }
        let Some(loc) = &call_edge.location else {
            continue;
        };
        let target_edges = by_site.entry(call_site_key(loc)).or_default();
        target_edges
            .entry(call_edge.target.clone())
            .and_modify(|existing| {
                if call_edge.confidence.get() > existing.confidence.get() {
                    *existing = call_edge.clone();
                }
            })
            .or_insert_with(|| call_edge.clone());
    }

    for (site_key, target_edges) in by_site {
        let Some(hint) = ref_hints.get(&site_key) else {
            continue;
        };
        if target_edges.len() != 1 {
            continue;
        }
        let call_edge = target_edges.into_values().next().unwrap();
        let Some(caller) = index.get(&call_edge.source) else {
            continue;
        };
        let Some(callee) = index.get(&call_edge.target) else {
            continue;
        };

        let args = sorted_value_flow_endpoints(hint, "args")
            .map_err(|error| value_flow_hint_decode_error(&site_key, error))?;
        let target = value_flow_result(hint)
            .map_err(|error| value_flow_hint_decode_error(&site_key, error))?;
        // ADR-014 S6b: `"await"` / `"then"` when the target binds what the returned promise
        // settles to; absent for a plain result.
        let settled = hint
            .get("result")
            .and_then(|result| result.get("settled"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let params = callable_param_names(&callee);
        let Some(site) = call_edge.location.clone() else {
            continue;
        };

        for arg in &args {
            let Some(slot) = arg.slot else {
                continue;
            };
            // Slot-exact: a parameter the extractor could not capture yields a LOST hop, never a
            // hop onto a neighbouring parameter (wicked-estate#207 review, C5d).
            let Some(Some(param)) = params.get(slot) else {
                continue;
            };
            let arg_symbol = arg
                .symbol
                .clone()
                .unwrap_or_else(|| value_symbol(&caller.symbol, "local", &arg.name));
            // #216: a parameter owns its own `:param:` slot, distinct from any same-named local.
            let param_symbol = value_symbol(&callee.symbol, "param", param);
            if index.get(&arg_symbol).is_none() {
                let node = value_node(
                    arg_symbol.clone(),
                    arg.node_kind(),
                    arg.name.clone(),
                    caller.language.clone(),
                    site.clone(),
                    arg.role(),
                );
                edges.push(value_containment_edge(&node));
                nodes.push(node);
            }
            edges.push(flow_edge(
                param_symbol,
                arg_symbol,
                &call_edge,
                "call_argument",
                FlowSemantics::ValuePreserving,
                FlowEvidence::CallDerived,
            ));
        }

        let return_symbol = value_symbol(&callee.symbol, "return", "value");
        // #210: the callee's return endpoint exists only when it has a literal `return <ident>`
        // or a classified return construct (`return raw.replace(..)`, ADR-014 S5b);
        // a `call_result` edge to a missing endpoint was minted, then pruned as dangling, leaving
        // the consumer local an orphan. No endpoint, no hop, no orphan node.
        let target = target.filter(|_| index.get(&return_symbol).is_some());
        if let Some(target) = target {
            let target_symbol = target
                .symbol
                .clone()
                .unwrap_or_else(|| value_symbol(&caller.symbol, "local", &target.name));
            if index.get(&target_symbol).is_none() {
                let node = value_node(
                    target_symbol.clone(),
                    target.node_kind(),
                    target.name.clone(),
                    caller.language.clone(),
                    site,
                    target.role(),
                );
                edges.push(value_containment_edge(&node));
                nodes.push(node);
            }
            // ADR-014 S6b: a SETTLED result (`await f(x)`, `f(x).then(r => …)`) is what the
            // returned promise settles to, not the returned value itself: `may_influence`. The
            // `then` binding rests on a naming convention that proves no Promise, so its evidence
            // is `convention` and it is no surer than a Heuristic edge.
            let (construct, semantics, evidence) = match settled.as_deref() {
                Some("await") => (
                    "await_result",
                    FlowSemantics::MayInfluence,
                    FlowEvidence::CallDerived,
                ),
                Some("then") => (
                    "then_result",
                    FlowSemantics::MayInfluence,
                    FlowEvidence::Convention,
                ),
                _ => (
                    "call_result",
                    FlowSemantics::ValuePreserving,
                    FlowEvidence::CallDerived,
                ),
            };
            let mut edge = flow_edge(
                target_symbol,
                return_symbol,
                &call_edge,
                construct,
                semantics,
                evidence,
            );
            if evidence == FlowEvidence::Convention {
                let cap = ResolutionTier::Heuristic.default_confidence();
                if edge.confidence.get() > cap.get() {
                    edge.confidence = cap;
                }
            }
            edges.push(edge);
        }
    }

    // Same lattice the extractor uses (TS-S1): two call sites in one caller that bind the same
    // argument to the same parameter share `(source, target, kind)` and would otherwise collapse
    // last-writer-wins in the store, silently picking one site's location as THE location.
    Ok((nodes, merge_flow_edges(edges)))
}

/// Entry names both walks skip: vendor/build dirs, wicked's own generated measurement artifacts
/// (see [`collect_source_files`]) and governed-run worktrees.
///
/// `wicked-worktrees` is where a run's worktree lives inside the repo checkout
/// (`<repo>/wicked-worktrees/<run>/`). It holds a full copy of the repo plus the run's unreviewed
/// creator output (a cancelled run's retained worktree is kept on disk). Indexing it duplicates
/// every symbol and serves that output back as if it were the codebase (crew#620).
const SKIPPED_ENTRY_NAMES: &[&str] = &[
    "target",
    "node_modules",
    ".wicked-estate",
    ".reference",
    "dist",
    "build",
    "coverage-report.json",
    "requirements_graph.json",
    "wicked-worktrees",
];

/// True when a walk must not descend into (or collect) `e`: a [`SKIPPED_ENTRY_NAMES`] entry, or the
/// hidden-dir worktree root `.wicked/worktrees` (only the hidden-inclusive extra-rule walk reaches
/// it; the source walk already skips hidden dirs).
fn is_skipped_entry(e: &ignore::DirEntry) -> bool {
    let name = e.file_name().to_string_lossy();
    if SKIPPED_ENTRY_NAMES.contains(&name.as_ref()) {
        return true;
    }
    name == "worktrees"
        && e.path()
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|p| p == ".wicked")
}

/// Collect source files under `root` using the `ignore` crate — gitignore-aware, skips hidden +
/// VCS/build/vendor dirs (so indexing a real repo doesn't drown in `target/`, `node_modules/`, etc.).
fn collect_source_files(root: &Path) -> Vec<PathBuf> {
    WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
        .require_git(false)
        // Vendor/build dirs AND wicked's own generated measurement artifacts. The latter close a
        // self-reference trap: the domain-extraction coverage phase writes `coverage-report.json`
        // (and the modeler exports `requirements_graph.json`) through a RELATIVE `--out`, so on any
        // launch whose CWD is an indexed root — an onboard index, a bare `wicked-core coverage`
        // call — the report lands at the repo root, un-hidden. Indexing it back in turns each of its
        // JSON keys into a behavior-bearing `Struct` node that can never be domain-annotated, so the
        // NEXT coverage run is pinned below 1.0 forever: the measurement corrupts the measurand.
        // Governed worktree runs escape the trap only because their CWD is under `.wicked/` (hidden);
        // this skip closes the hole for every other launch path regardless of where the file lands.
        .filter_entry(|e| !is_skipped_entry(e))
        .build()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(ignore::DirEntry::into_path)
        .collect()
}

/// The warning for previously indexed files removed from the index that the secret-file rule
/// excludes (an older build indexed them, or the walk can no longer see them): their symbols, and
/// every requirement annotation on those symbols, leave with them, so that is said out loud
/// (wicked-estate#433). Classified by the rule, not by the walk, so a hidden or ignored file is
/// reported too. `None` when no removed file matches the rule. Paths only, never content.
fn secret_exclusion_note(deleted: &[String]) -> Option<String> {
    let mut hit: Vec<&str> = deleted
        .iter()
        .filter(|p| wicked_estate_extract::redact::is_excluded(p))
        .map(String::as_str)
        .collect();
    if hit.is_empty() {
        return None;
    }
    hit.sort_unstable();
    Some(format!(
        "SECRET-EXCLUDED: {} previously indexed file(s) match the secret-file rule and were \
         removed from the index, with their symbols and any requirement annotations on them: {}",
        hit.len(),
        hit.join(", ")
    ))
}

// The notes `secret_exclusion_note` emitted on this thread, so a test sees what the sweep reported.
#[cfg(test)]
thread_local! {
    static SEMANTICS_DROPS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    static SEMANTICS_UNSUPPORTED: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static SECRET_NOTES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    // #422: (source-byte budget, file cap), overridden only by this thread's index tests.
    static TEST_EXTRACT_CHUNK_LIMITS: std::cell::Cell<Option<(usize, usize)>> = const { std::cell::Cell::new(None) };
    static TEST_MAX_EXTRACTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    // #404: fail immediately after one of the two durable index batches.
    static TEST_INDEX_FAIL_AFTER: std::cell::Cell<Option<&'static str>> =
        const { std::cell::Cell::new(None) };
    static TEST_REBIND_FILES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    // #515: probe another writer before each extraction chunk and the final write batch.
    static TEST_INDEX_WRITE_HOOK: std::cell::RefCell<Option<Box<dyn FnMut()>>> = const { std::cell::RefCell::new(None) };
    // #477: interrupt after graph commits, before certifying the extractor markers.
    static TEST_INDEX_STAMP_HOOK: std::cell::RefCell<Option<Box<dyn FnMut()>>> = const { std::cell::RefCell::new(None) };
    // The documents the converter skipped on this thread's index runs: (path, reason).
    static DOC_SKIPS: std::cell::RefCell<Vec<(String, String)>> = const { std::cell::RefCell::new(Vec::new()) };
    // Walked files not admitted or newly unreadable on this thread's index runs: (path, reason).
    static NOT_INDEXED: std::cell::RefCell<Vec<(String, String)>> =
        const { std::cell::RefCell::new(Vec::new()) };
    // The files reported as retaining old extractor output on this thread: (path, reason).
    static UNREADABLE_FILES: std::cell::RefCell<Vec<(String, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn test_index_write_hook() {
    TEST_INDEX_WRITE_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook();
        }
    });
}

#[cfg(test)]
fn test_index_fail_after(point: &str) -> Result<()> {
    if TEST_INDEX_FAIL_AFTER.with(|hook| hook.get() == Some(point)) {
        return Err(wicked_estate_core::Error::Invalid(format!(
            "interrupted after {point}"
        )));
    }
    Ok(())
}

// Absolute source paths whose read a test forces to fail in the work list (#471): a deterministic
// read failure that does not depend on file permissions (root reads a mode-000 file). A global
// set, not a thread-local, because the work list is read on rayon's threads; each test uses its
// own tempdir path, so parallel tests do not see each other's entries.
#[cfg(test)]
static TEST_READ_FAILS: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

#[cfg(test)]
fn test_read_fails(abs: &Path) -> bool {
    TEST_READ_FAILS
        .lock()
        .map(|v| v.iter().any(|p| p == abs))
        .unwrap_or(false)
}

/// Written before a forced full re-extraction changes the graph. No real version or digest
/// starts with it, so an interrupted run forces full re-extraction even after rollback (#477).
const PENDING_FULL_PREFIX: &str = "pending-full:";

/// Certify the current extractor markers and record files that still hold old output (#476).
/// Those files retry individually, without forcing unrelated files through extraction again.
fn stamp_extractor_markers(
    store: &mut dyn GraphStoreMutExt,
    version_key: &str,
    digest_key: &str,
    extra_digest: &str,
    unreadable_key: &str,
    unreadable_files: &[(String, String)],
) {
    #[cfg(test)]
    TEST_INDEX_STAMP_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook();
        }
    });
    // Persist the retry record before clearing either pending marker: interruption must not
    // certify current output without preserving the skipped files' need for re-extraction.
    let record = unreadable_files
        .iter()
        .map(|(path, reason)| format!("{path}\t{reason}"))
        .collect::<Vec<_>>()
        .join("\n");
    store.meta_set_key(unreadable_key, &record);
    store.meta_set_key(version_key, env!("CARGO_PKG_VERSION"));
    store.meta_set_key(digest_key, extra_digest);
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// What [`ensure_redacted`] did to a store (wicked-estate#405).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedactionOutcome {
    /// Already at the current policy, physical cleanup complete. Nothing done.
    Current,
    /// A store with no code graph yet: the policy epoch was recorded.
    Fresh,
    /// Text written before the policy was cleaned in place.
    Migrated(wicked_estate_store::RedactionReport),
    /// Already migrated; the leftover physical cleanup (WAL truncate + VACUUM) was retried.
    /// `true` when it completed.
    CleanupRetried(bool),
}

/// Make `store` safe to serve under the current secret-redaction policy (wicked-estate#405).
///
/// A code graph (any indexed file, or a recorded `indexed_root`) whose `redaction_epoch` is older
/// than this build's is migrated in place ([`GraphStoreMutExt::migrate_redaction`]); a fresh store
/// only records the epoch; a migrated store whose physical cleanup did not complete gets it
/// retried. A backend with no migration is an ERROR, never reported as clean. Progress goes to
/// stderr as counts, never values.
pub fn ensure_redacted(store: &mut dyn GraphStoreMutExt) -> Result<RedactionOutcome> {
    use wicked_estate_core::redact::{REDACTION_EPOCH, REDACTION_EPOCH_KEY};
    let epoch = store
        .meta_get_key(REDACTION_EPOCH_KEY)
        .and_then(|v| v.parse::<u32>().ok());
    if epoch.is_some_and(|e| e >= REDACTION_EPOCH) {
        if store.redaction_cleanup_pending() {
            let done = store.finish_redaction_cleanup()?;
            if !done {
                eprintln!(
                    "REDACTION: the WAL could not be truncated (a reader is attached); old pages \
                     stay in it until `wicked-estate redact-store` runs with no reader attached"
                );
            }
            return Ok(RedactionOutcome::CleanupRetried(done));
        }
        return Ok(RedactionOutcome::Current);
    }
    let digest = wicked_estate_extract::redact::rules_digest();
    let code_graph =
        store.meta_get_key("indexed_root").is_some() || !store.indexed_files()?.is_empty();
    if !code_graph {
        store.meta_set_key(REDACTION_EPOCH_KEY, &REDACTION_EPOCH.to_string());
        store.meta_set_key("redaction_rules", &digest);
        return Ok(RedactionOutcome::Fresh);
    }
    // Credential files are never indexed now; text an older build stored for one is masked whole
    // (the next index sweeps the file, since the walk no longer admits it).
    let redact = |path: &str, text: &str| -> String {
        if wicked_estate_extract::redact::is_excluded(path) {
            wicked_estate_extract::redact::mask_all(text)
        } else {
            wicked_estate_extract::redact::redact_source(path, text)
        }
    };
    let Some(rep) = store.migrate_redaction(&redact, &digest)? else {
        return Err(wicked_estate_core::Error::Invalid(
            "this store backend cannot clean text written before secret redaction (#405); \
             re-create the store to clean it"
                .into(),
        ));
    };
    eprintln!(
        "REDACTION: cleaned a store written before secret redaction (#405): {} file texts \
         re-masked, {} old blobs dropped, {} node rows and {} other rows rewritten, {} files \
         marked for re-extraction{}",
        rep.files_rewritten,
        rep.blobs_dropped,
        rep.nodes_rewritten,
        rep.rows_scrubbed,
        rep.files_marked,
        if rep.wal_truncated {
            ""
        } else {
            "; a live reader kept the WAL from being truncated: re-run `wicked-estate \
             redact-store` when no reader is attached"
        }
    );
    Ok(RedactionOutcome::Migrated(rep))
}

/// Load every drop-in extra-edge rule file (`.wicked-estate-extractors/*.toml`) under `root`.
///
/// Returns the compiled extractor (`None` when the dir is absent, has no rules, or the rules fail
/// to parse — parse failures print a LOUD `EXTRA-EDGE:` marker, never silently no-op) plus a
/// digest of the raw rule bytes. The digest is compared against the store's `extra_rules_digest`
/// meta key so *editing the rules* forces a full re-extract — otherwise edges produced by the old
/// rule set would linger on unchanged files.
fn load_extra_edge_rules(root: &Path) -> (Option<ExtraEdgeExtractor>, String) {
    let dir = root.join(".wicked-estate-extractors");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return (None, String::new());
    };
    // #405: a rule file that resolves outside the repository (a symlink out) is never read.
    let confined = confined::ConfinedRoot::new(root);
    let mut paths: Vec<PathBuf> = rd
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
        .filter_map(|p| confined.as_ref()?.resolve(&p).filter(|c| c.is_file()))
        .collect();
    paths.sort();

    let mut entries: Vec<(String, String)> = Vec::new();
    for p in &paths {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match std::fs::read_to_string(p) {
            Ok(contents) => entries.push((name, contents)),
            Err(e) => eprintln!("EXTRA-EDGE: cannot read {}: {e} — skipping", p.display()),
        }
    }
    if entries.is_empty() {
        return (None, String::new());
    }
    // Digest the raw bytes (name + contents), not the parsed rules, so ANY edit — including one
    // that breaks parsing — changes the digest and triggers the full re-extract.
    let mut digest_input = String::new();
    for (name, contents) in &entries {
        digest_input.push_str(name);
        digest_input.push('\0');
        digest_input.push_str(contents);
        digest_input.push('\0');
    }
    let digest = file_digest(digest_input.as_bytes());

    match ExtraEdgeExtractor::from_toml_named(&entries) {
        Ok(x) if x.rule_count() > 0 => (Some(x), digest),
        Ok(_) => (None, digest),
        Err(e) => {
            eprintln!("EXTRA-EDGE: {e} — extra-edge rules DISABLED for this run");
            (None, digest)
        }
    }
}

/// Second walk for extra-edge rule targets: INCLUDES hidden paths, because drop-in rules routinely
/// target dot-dir catalogs (e.g. `.claude-plugin/archetypes.json`) that [`collect_source_files`]'s
/// hidden-filter skips. Still gitignore-aware; skips `.git`, the rules dir itself, and exactly the
/// same build/vendor dir names as [`collect_source_files`] — the two walks must agree on what is
/// visible so a rule behaves the same whether or not its target is also an ordinary source file
/// (anything beyond that list, e.g. a committed Go-style `vendor/`, is gitignore's call in BOTH
/// walks). Only files matching at least one rule's `file_glob` are returned.
fn collect_extra_rule_files(root: &Path, rules: &ExtraEdgeExtractor) -> Vec<PathBuf> {
    WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .require_git(false)
        .filter_entry(|e| {
            !matches!(
                e.file_name().to_string_lossy().as_ref(),
                ".git" | ".wicked-estate-extractors"
            ) && !is_skipped_entry(e)
        })
        .build()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(ignore::DirEntry::into_path)
        .filter(|p| rules.matches_path(&rel(root, p)))
        .collect()
}

/// Compute an xxh3 hex digest for a byte slice.
/// The stored digest of a source file: its bytes, folded with its sidecar when it has one (an
/// APEXlang `application.apx` and its `.apex/apexlang.json`, #316). The ONE definition the indexer
/// and the freshness check share, so a file is never "modified" just because it has a sidecar.
pub(crate) fn source_digest(bytes: &[u8], sidecar: Option<&[u8]>) -> String {
    match sidecar {
        Some(meta) => {
            let mut both = bytes.to_vec();
            both.extend_from_slice(b"\0apexlang-export-metadata\0");
            both.extend_from_slice(meta);
            file_digest(&both)
        }
        None => file_digest(bytes),
    }
}

fn node_backfill_probes(node: &Node) -> impl Iterator<Item = String> {
    std::iter::once(node.name.clone())
        .chain(wicked_estate_resolve::apexlang::backfill_probes(node))
        .chain((node.kind == NodeKind::Module).then(|| {
            format!(
                "{}{}",
                wicked_estate_core::refs::COBOL_DYNAMIC_CALL_PREFIX,
                node.name
            )
        }))
}

/// The source and sidecar read together for one extraction or digest comparison.
struct IndexSource {
    bytes: Vec<u8>,
    /// APEXlang sidecar: `Some(None)` means expected but missing or unreadable (#316).
    export_metadata: Option<Option<Vec<u8>>>,
    digest: String,
}

fn read_index_source(
    confined_root: Option<&confined::ConfinedRoot>,
    root: &Path,
    abs: &Path,
    raw: &str,
) -> Option<IndexSource> {
    #[cfg(test)]
    if test_read_fails(abs) {
        return None;
    }
    let confined = confined_root?;
    let bytes = confined.read(abs)?;
    let export_metadata = wicked_estate_extract::apexlang::export_metadata_path(raw)
        .map(|meta| confined.read(&root.join(meta)));
    let digest = source_digest(&bytes, export_metadata.as_ref().and_then(|m| m.as_deref()));
    Some(IndexSource {
        bytes,
        export_metadata,
        digest,
    })
}

pub(crate) fn file_digest(bytes: &[u8]) -> String {
    format!("{:016x}", xxh3_64(bytes))
}

/// Collect git provenance for the repo rooted at `path`.
///
/// Shells `git -C <path>` once per field; every failure (git absent, not a repo, non-zero exit)
/// produces `None`/`false` for that field — never panics.  Follows the same pattern as
/// [`commits_behind`].
pub fn collect_repo_info(path: &Path) -> RepoInfo {
    let r = path.to_string_lossy();
    // Helper: run one git command, return trimmed stdout or None on any failure.
    let run = |args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("git").args(args).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8(out.stdout).ok()?;
        Some(s.trim().to_string())
    };

    let commit = run(&["-C", &r, "rev-parse", "HEAD"]);
    // Detached HEAD reports the literal string "HEAD" — treat that as None.
    let branch = run(&["-C", &r, "rev-parse", "--abbrev-ref", "HEAD"]).filter(|b| b != "HEAD");
    let remote = run(&["-C", &r, "remote", "get-url", "origin"]);
    let dirty = run(&["-C", &r, "status", "--porcelain"])
        .map(|s| !s.is_empty())
        .unwrap_or(false);

    RepoInfo {
        commit,
        branch,
        remote,
        dirty,
    }
}

/// Index every supported source file under `root` into `store`, then resolve cross-file edges.
/// Returns the resulting graph stats. This is the two-phase EXTRACT→RESOLVE→STORE pipeline.
///
/// On the first run the store is empty, so every file is "new" (full index). On subsequent runs
/// only CHANGED or NEW files are re-extracted; UNCHANGED files are skipped (incremental).
///
/// W7.4: stores the canonical root path in store meta so staleness checks can locate the git repo.
/// W9.4: InfraResolver is included in the resolver slice so IaC/tfstate resource refs resolve.
/// W11.3: bumps the store version at the start (invalidating stale cache) and populates the
///   `pagerank.top` cache entry at the end with the top-N PageRank scores.
pub fn index_path(store: &mut dyn GraphStoreMutExt, root: &Path) -> Result<GraphStats> {
    index_path_as(store, root, None)
}

/// [`index_path`] with an optional repo label — the multi-repo entry point.
///
/// `repo = Some("ledger")` namespaces every path this run stores as `ledger/<rel>`, which is what
/// makes `files.path`, `nodes.file` and the path-embedded SymbolIds unique per repo; it scopes the
/// delete-sweep and the resolver's candidate set to that repo, and records provenance under
/// `repo:ledger:*` instead of the singular `repo_*` keys. `repo = None` is the single-repo path
/// and behaves exactly as it always has.
///
/// Refuses (before any write) when the run would overwrite another repo's rows — see
/// [`repo_scope::guard`].
pub fn index_path_as(
    store: &mut dyn GraphStoreMutExt,
    root: &Path,
    repo: Option<&str>,
) -> Result<GraphStats> {
    let stats = match index_path_inner(store, root, repo) {
        Ok(stats) => stats,
        Err(e) => {
            // #438: an error may leave the destructive write batch open — `begin_batch` is a no-op
            // while a batch is already open (SqliteStore / PostgresStore), so a long-lived caller
            // that keeps the store after a failed re-index (the `watch` loop) would reuse the
            // half-written transaction on its next tick and commit the removals WITHOUT their
            // restored semantic fields. Roll the open short batch back so the next run starts
            // clean; previously committed chunks keep their restored fields (#515). No-op when
            // no batch is open; MemStore has no batch to discard (its default returns an error,
            // deliberately ignored here).
            let _ = store.rollback_batch();
            return Err(e);
        }
    };
    // X1 (DES-S2 §6): publish the label's new generation only after a complete index. The
    // `indexing` marker set at the start stays on a failed run, so readers say `incomplete` and
    // the linker refuses the label.
    link::record_dep_artifacts(store, root, repo);
    let graph_gen_v = store
        .meta_get_key(&repo_scope::meta_key(repo, "indexing"))
        .filter(|g| !g.is_empty());
    if let Some(graph_gen_v) = graph_gen_v {
        store.meta_set_key(&repo_scope::meta_key(repo, "graph_gen"), &graph_gen_v);
    }
    store.meta_set_key(&repo_scope::meta_key(repo, "indexing"), "");
    Ok(stats)
}

/// Write the semantic fields saved before `remove_file` back onto the symbols the re-extraction
/// emitted again — the original validator and time kept as they were, description/requirement
/// re-masked by the store's redaction backstop (wicked-estate#438). A symbol that is gone loses
/// its fields, as a removed node always did; the per-file count of those is returned for the
/// caller to print as `SEMANTICS-DROPPED`, so the loss is never silent. Called inside the final
/// write batch, after the re-extraction's and resolution's upserts, so a renamed symbol (same
/// `SymbolId`, new file) is present and restored while a truly-deleted one is absent and counted.
fn restore_semantics(
    store: &mut dyn GraphStoreMutExt,
    saved: Vec<(String, SymbolId, NodeSemantics)>,
) -> Result<std::collections::BTreeMap<String, usize>> {
    let mut dropped: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for (file, _, _) in restore_pending_semantics(store, saved)? {
        *dropped.entry(file).or_default() += 1;
    }
    Ok(dropped)
}

/// Meta key holding the durable #438 snapshot of semantic fields whose symbol is not (yet)
/// re-created (#515). A JSON array of `(file, symbol, semantics)`; written in the same batch that
/// removes the file's rows and cleared in the batch that restores or drops the entries, so a
/// killed or failed run never loses the fields. Empty/absent when nothing is pending.
const PENDING_SEMANTICS_KEY: &str = "index.pending_semantics";

type PendingSemantics = Vec<(String, SymbolId, NodeSemantics)>;

fn pending_semantics_load(store: &dyn GraphStoreMutExt) -> PendingSemantics {
    let Some(raw) = store
        .meta_get_key(PENDING_SEMANTICS_KEY)
        .filter(|v| !v.is_empty())
    else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_else(|e| {
        eprintln!("PENDING-SEMANTICS-UNREADABLE: {e}; leftover fields from an interrupted index cannot be restored");
        Vec::new()
    })
}

/// Persist `pending` (this run's entries) plus `others` (entries of other repo labels, untouched).
fn pending_semantics_save(
    store: &mut dyn GraphStoreMutExt,
    others: &PendingSemantics,
    pending: &PendingSemantics,
) -> Result<()> {
    let all: Vec<&(String, SymbolId, NodeSemantics)> =
        others.iter().chain(pending.iter()).collect();
    let json = if all.is_empty() {
        String::new()
    } else {
        serde_json::to_string(&all)
            .map_err(|e| wicked_estate_core::Error::Invalid(e.to_string()))?
    };
    // Nothing pending and nothing stored: leave the meta table untouched.
    if json.is_empty() && store.meta_get_key(PENDING_SEMANTICS_KEY).is_none() {
        return Ok(());
    }
    store.meta_put(PENDING_SEMANTICS_KEY, &json)
}

/// Restore available symbols inside a chunk's write batch, keeping missing ids pending until a
/// later chunk or resolution re-creates them (#515). Callers enqueue a file's saved fields only
/// once its old contributions have been removed, so a future chunk cannot erase an early restore.
fn restore_pending_semantics(
    store: &mut dyn GraphStoreMutExt,
    saved: Vec<(String, SymbolId, NodeSemantics)>,
) -> Result<Vec<(String, SymbolId, NodeSemantics)>> {
    if !store.can_restore_node_semantics() {
        return Ok(saved);
    }
    let mut pending = Vec::new();
    for (file, symbol, semantics) in saved {
        if store.get_node(&symbol)?.is_some() {
            store.restore_node_semantics(&symbol, &semantics)?;
        } else {
            pending.push((file, symbol, semantics));
        }
    }
    Ok(pending)
}

fn index_path_inner(
    store: &mut dyn GraphStoreMutExt,
    root: &Path,
    repo: Option<&str>,
) -> Result<GraphStats> {
    let t = std::time::Instant::now();

    if let Some(label) = repo {
        repo_scope::validate_label(label)?;
    }
    // Path prefix carried by every row this run writes. Also the delete-sweep's scope.
    let scope = repo.map(repo_scope::prefix);

    if !store.can_restore_node_semantics() {
        let note = "SEMANTICS-UNSUPPORTED: this store cannot restore semantic fields; symbols in \
                    re-extracted files lose their description, requirement and validation";
        eprintln!("{note}");
        #[cfg(test)]
        SEMANTICS_UNSUPPORTED.with(|notes| notes.borrow_mut().push(note.to_string()));
    }

    // #515: re-apply fields a killed or failed earlier run left durably pending, before anything
    // else. Entries whose symbol exists are restored and cleared; the rest stay pending (and, for
    // this repo's label, are counted dropped when this run's final batch commits).
    let (mut pending_semantics, other_pending_semantics): (PendingSemantics, PendingSemantics) = {
        let leftover = pending_semantics_load(store);
        if leftover.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            store.begin_batch()?;
            let leftover = restore_pending_semantics(store, leftover)?;
            let (mine, others): (PendingSemantics, PendingSemantics) =
                leftover.into_iter().partition(|(file, _, _)| {
                    scope
                        .as_deref()
                        .is_none_or(|prefix| file.starts_with(prefix))
                });
            pending_semantics_save(store, &others, &mine)?;
            store.commit_batch()?;
            (mine, others)
        }
    };

    // W7: capture git provenance once per index run. Non-fatal: git absent / not a repo →
    // all-None RepoInfo, which is a valid default. Collected BEFORE the guard runs — the remote
    // it carries is the evidence the guard decides repo identity on.
    let repo_info = collect_repo_info(root);

    // ── Derive the set of previously-indexed file paths ─────────────────────────────────────
    // Every path a prior `index_path` recorded through a file-writing call — `set_file_digest` or
    // `set_file_content` — and nothing else. Both count: this indexer issues both for every file it
    // takes, and scoping to digests alone would miss a path whose content was stored but whose
    // digest write did not land. `remove_file` drops nodes and the file row atomically, so a path
    // removed last run does not reappear here.
    //
    // This USED to read `all_nodes()`, mapping each node's `location.file`. That answers a
    // different question: not "what did I index?" but "what is in this store?" — and the sweep
    // below deletes everything in that set which is not on disk right now. The two sets are equal
    // only in a store no other writer touches, and that assumption does not survive contact with a
    // shared store.
    //
    // It did not survive here. An orchestrator sharing this store kept its operational domain
    // objects as nodes with synthetic `location.file` values — `agent_session/<id>`,
    // `work_unit/<id>`, `validator_vault/<pin>`, `repo_entry/<id>`. Indexing one repo subdirectory
    // into that store swept all 833 of them: every session, work unit, workflow, work output,
    // validator vault entry, policy and repo registration, in one transaction, including the run
    // whose worker issued the index — which then died with `run not found` (FINDING-067).
    //
    // A digest row can only come from this indexer, so scoping to it makes reaching a node we did
    // not create impossible rather than merely unlikely. Nothing about the deleted-file behaviour
    // changes: a real source file that vanished still has its digest row from last run and is
    // still swept.
    let all_indexed: HashSet<String> = store
        .indexed_files()?
        .into_iter()
        .filter(|f| !f.is_empty())
        .collect();

    // THE GUARD. Read-only, and it runs before the first mutation below: a refusal must leave the
    // graph exactly as it was.
    repo_scope::guard(store, &all_indexed, repo, root, &repo_info)?;

    // A labelled run owns only its own prefix. Un-prefixed: the whole set, as before.
    let previously_indexed: HashSet<String> = match &scope {
        Some(p) => all_indexed
            .into_iter()
            .filter(|f| f.starts_with(p.as_str()))
            .collect(),
        None => all_indexed,
    };

    // #405: a store written before secret redaction is cleaned before this run writes anything;
    // a fresh store records the policy epoch so readers serve it.
    ensure_redacted(store)?;

    let interrupted = store
        .meta_get_key(&repo_scope::meta_key(repo, "indexing"))
        .filter(|marker| !marker.is_empty());

    // X1 (DES-S2 §6): mark the label as mid-index with its next generation before the first
    // write; `index_path_as` publishes it as `graph_gen` and clears the marker on success.
    let next_gen = store
        .meta_get_key(&repo_scope::meta_key(repo, "graph_gen"))
        .and_then(|g| g.parse::<u64>().ok())
        .unwrap_or(0)
        + 1;
    store.meta_set_key(
        &repo_scope::meta_key(repo, "indexing"),
        &next_gen.to_string(),
    );

    // W11.3: bump the graph version so any prior cache entries become stale.
    store.version_bump();

    // W7.4 / #248: persist the indexed root CANONICAL (absolute, symlinks resolved) so freshness
    // is not cwd-dependent — `index .` used to store "." and every other working directory then
    // read "STALENESS: unknown for .". Readers still accept the old relative spelling
    // ([`indexed_root_path`]). In a multi-repo graph this is the LAST root indexed; each repo's
    // own root is in its `repo:<label>:root`.
    store.meta_set_key("indexed_root", &canonical_root(root));
    // #201: WHEN this graph was built, so "which commit, built when" is one read (`stats --json`).
    store.meta_set_key("indexed_at", &rfc3339_now());
    // Read the previously-stored binary version BEFORE overwriting it so we can detect a
    // version upgrade and force full re-extraction when the binary has changed. Per repo: an
    // upgrade must force a re-extract of EACH repo the next time it is indexed, not just the
    // first one to run under the new binary.
    let version_key = repo_scope::meta_key(repo, "indexed_version");
    let prev_version = store.meta_get_key(&version_key);
    let mut force_full = prev_version
        .as_deref()
        .is_some_and(|v| v != env!("CARGO_PKG_VERSION"));
    if force_full {
        eprintln!(
            "VERSION CHANGE detected (v{prev} → v{cur}): forcing full re-extraction",
            prev = prev_version.as_deref().unwrap_or("?"),
            cur = env!("CARGO_PKG_VERSION"),
        );
    }

    if let Some(generation) = interrupted {
        force_full = true;
        eprintln!("INTERRUPTED INDEX detected ({generation}): forcing full re-extraction");
    }

    // Drop-in extra-edge rules (`.wicked-estate-extractors/*.toml`, extractor SDK Part 2).
    // A changed rule set forces a full re-extract: extra edges live in per-file extractions, so
    // only re-extracting every file purges edges the OLD rules injected into unchanged files.
    // The rules are read from THIS root, so the digest is per repo too.
    let (extra, extra_digest) = load_extra_edge_rules(root);
    let digest_key = repo_scope::meta_key(repo, "extra_rules_digest");
    let prev_extra = store.meta_get_key(&digest_key).unwrap_or_default();
    if prev_extra != extra_digest {
        if prev_version.is_some() {
            eprintln!("EXTRA-EDGE rules changed: forcing full re-extraction");
        }
        force_full = true;
    }

    // Plugin-override gate (ADR-010): a parser-plugin override (query-only or armed grammar)
    // changes extraction output without touching a single source byte, so stored graphs indexed
    // under a different override state must fully re-extract. The per-repo `plugin_overrides`
    // meta key holds the canonical descriptor of the effective override set; reading
    // `override_state()` here makes plugin-registry loading unconditional on every index run
    // (dlopen moves from first-lookup to index start — owned by ADR-010). The digest inside each
    // descriptor line is over the registry's CACHED bytes — what extraction will actually use —
    // never a fresh disk read, so a long-lived process stays old-query/old-digest consistent.
    //
    // Like the id-scheme and version/rules gates, the key is written
    // write-LAST: at run end and in the gate-guarded no-change early return — a crash mid-run
    // leaves the old key and the gate re-fires. Idempotent, never falsely certified.
    let override_key = repo_scope::meta_key(repo, "plugin_overrides");
    let prev_overrides = store.meta_get_key(&override_key).unwrap_or_default();
    let cur_overrides = wicked_estate_extract::plugin::override_state();
    for line in cur_overrides.lines() {
        let mut parts = line.split('|');
        let lang = parts.next().unwrap_or("?");
        let mode = parts.next().unwrap_or("?");
        let dir = parts.next().unwrap_or("?");
        if mode == "grammar" {
            eprintln!("GRAMMAR OVERRIDE active: {lang} <- {dir}");
        } else {
            eprintln!("query override active: {lang} <- {dir}");
        }
    }
    let override_gate = prev_overrides != cur_overrides;
    if override_gate {
        if prev_version.is_some() {
            // The old->new descriptor-line diff makes the CLI-vs-editor env-skew footgun
            // diagnosable from the output alone.
            eprintln!("PLUGIN-OVERRIDE state changed: forcing full re-extraction");
            for l in prev_overrides
                .lines()
                .filter(|l| !cur_overrides.lines().any(|c| c == *l))
            {
                eprintln!("- {l}");
            }
            for l in cur_overrides
                .lines()
                .filter(|l| !prev_overrides.lines().any(|p| p == *l))
            {
                eprintln!("+ {l}");
            }
        }
        force_full = true;
    }

    // Symbol-id scheme gate (ADR-002 amendment): rows minted under an older id scheme must be
    // fully re-extracted or the graph silently mixes flat and type-nested ids behind unchanged
    // digests. Fires on ANY previously-indexed repo — `previously_indexed` is label-scoped and is
    // direct evidence of a prior index, unlike the version key, which pre-version DBs lack (they
    // hold nodes + digests but no `indexed_version`; keying on it would skip exactly the stalest
    // stores). Absent scheme key = scheme "1" (the implicit flat scheme).
    //
    // Deliberately UNLIKE the two gates above, the key is NOT written here: it is written only
    // after the re-extraction completed (end of this fn, plus the gate-guarded no-change early
    // return below). Writing at the check site would let a crash mid-run leave a DB stamped with
    // the new scheme whose rows are still old — permanently mixed, because every later run skips
    // the unchanged digests. Written last, a crash leaves the old key and the gate re-fires:
    // idempotent.
    let scheme_key = repo_scope::meta_key(repo, "id_scheme");
    let prev_scheme = store.meta_get_key(&scheme_key);
    let scheme_gate =
        !previously_indexed.is_empty() && prev_scheme.as_deref() != Some(SYMBOL_ID_SCHEME);
    if scheme_gate {
        force_full = true;
        eprintln!(
            "SYMBOL-ID SCHEME changed (v{prev} → v{cur}): forcing full re-extraction",
            prev = prev_scheme.as_deref().unwrap_or("1"),
            cur = SYMBOL_ID_SCHEME,
        );
    }

    // #477: invalidate BOTH old markers before any extraction batch can commit. A killed run
    // must not let rollback match the old markers and keep output from the interrupted run.
    // Every successful exit replaces these sentinels with current markers and a retry record.
    if force_full {
        store.meta_set_key(
            &version_key,
            &format!("{PENDING_FULL_PREFIX}{}", env!("CARGO_PKG_VERSION")),
        );
        store.meta_set_key(&digest_key, &format!("{PENDING_FULL_PREFIX}{extra_digest}"));
    }
    let unreadable_key = repo_scope::meta_key(repo, "unreadable_files");
    let prev_unreadable: HashSet<String> = store
        .meta_get_key(&unreadable_key)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once('\t').map(|(path, _)| path.to_string()))
        .collect();

    // W7: persist the git provenance collected above. A labelled run writes `repo:<label>:*` and
    // leaves the singular `repo_*` keys untouched — that is what stops the second repo indexed
    // into a graph from clobbering the first's commit/branch/remote/dirty.
    match repo {
        // #248 (codex round 1 on #264): the registry root is canonical too, so a co-located
        // graph's per-repo freshness is not cwd-dependent either.
        Some(label) => {
            repo_scope::write_record(store, label, Path::new(&canonical_root(root)), &repo_info)
        }
        None => {
            let _ = store.set_repo_info(&repo_info);
        }
    }

    let mut files = collect_source_files(root);

    // Admit extra-edge rule targets the main walk cannot see (hidden dot-dir files). Rule-matched
    // files that are ALSO ordinary source files stay deduped — they get both passes below.
    if let Some(x) = &extra {
        let mut known: HashSet<PathBuf> = files.iter().cloned().collect();
        for p in collect_extra_rule_files(root, x) {
            if known.insert(p.clone()) {
                files.push(p);
            }
        }
    }

    // Per-pass extractor cache: each language's query is compiled ONCE, and only when a file of
    // it is actually extracted (ADR-014 "Cumulative": compiling every present extension's query
    // up front was the fixed cost of a one-file re-index).
    let ext_map = ExtractorCache::for_extensions(files.iter().map(|path| {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        // A binary document is extracted as the Markdown it converts to.
        if doc_convert::is_doc_ext(&ext) {
            "md".to_string()
        } else {
            ext
        }
    }));

    // ── Classify files: UNCHANGED / CHANGED+NEW ──────────────────────────────────────────────
    // Read and drop source bytes for digest comparison; retain only lightweight work records.
    // Parallelise the read+digest step across all files — I/O-bound, safe to fan out.
    struct FileWork {
        abs: PathBuf,
        /// Storage identity: repo-relative, and `<label>/`-prefixed under a repo label. Every
        /// path-derived id (file row, `nodes.file`, the path inside a SymbolId) comes from this.
        rel: String,
        /// The same path WITHOUT the label — what drop-in extra-edge rules glob against, so a
        /// rule written `src/**/*.js` keeps matching once the repo is labelled.
        raw: String,
        /// Classification length bounds chunk sizing without retaining source bytes.
        len: usize,
        digest: String,
    }

    // Documents the converter could not read (needs OCR, encrypted, too large, ...): path + reason.
    let doc_skips: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());
    let skip_doc = |rel: &str, reason: String| {
        if let Ok(mut v) = doc_skips.lock() {
            v.push((rel.to_string(), reason));
        }
    };
    let walked = files.len();
    let confined_root = confined::ConfinedRoot::new(root);
    let mut not_indexed = Vec::new();
    let supported: Vec<(PathBuf, String)> = files
        .into_iter()
        .filter_map(|p| {
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let rel_path = rel(root, &p);
            // #405: credential files (`.env`, private keys, kubeconfigs, `credentials*`, ...) are
            // never indexed, even when committed.
            if wicked_estate_extract::redact::is_excluded(&rel_path) {
                not_indexed.push((
                    repo_scope::namespaced(repo, &rel_path),
                    ext,
                    NOT_INDEXED_SECRET,
                ));
                return None;
            }
            // #447: .ctl also names mainframe control cards. Only a VB6 header admits it;
            // unreadable files still reach the existing read-failure handling below.
            if ext == "ctl"
                && confined_root
                    .as_ref()
                    .and_then(|c| c.read(&p))
                    .is_some_and(|bytes| !is_vb6_source(&bytes))
            {
                not_indexed.push((
                    repo_scope::namespaced(repo, &rel_path),
                    ext,
                    NOT_INDEXED_CTL,
                ));
                return None;
            }
            // Admit tree-sitter languages AND the grammar-less line extractors (JCL / HLASM), whose
            // dispatch lives in the per-file closure below — they are NOT in `ext_map`. Files
            // matched by a drop-in extra-edge rule are admitted too (they may have no language
            // extractor at all — e.g. a JSON catalog only the rules care about).
            let extra_match = extra.as_ref().is_some_and(|x| x.matches_path(&rel_path));
            // Binary documents are admitted too: they are converted to Markdown below. One over
            // the converter's input cap is skipped here, by its size, before it is ever read.
            let is_doc = doc_convert::is_doc_ext(&ext);
            if is_doc {
                let len = std::fs::metadata(&p).map_or(0, |m| m.len());
                if len > doc_convert::MAX_INPUT_BYTES as u64 {
                    let reason = doc_convert::Skipped::TooLarge(len).to_string();
                    skip_doc(&repo_scope::namespaced(repo, &rel_path), reason);
                    return None;
                }
            }
            if ext_map.claims(&ext) || is_grammarless_ext(&ext) || is_doc || extra_match {
                Some((p, rel_path))
            } else {
                let reason = not_indexed_reason(&ext);
                not_indexed.push((repo_scope::namespaced(repo, &rel_path), ext, reason));
                None
            }
        })
        .collect();

    // The filter above is sequential; reads below add new unreadable files from rayon workers.
    let not_indexed = std::sync::Mutex::new(not_indexed);
    let skip_new_unreadable = |rel_path: &str| {
        let path = repo_scope::namespaced(repo, rel_path);
        if !previously_indexed.contains(&path) {
            let ext = Path::new(rel_path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if let Ok(mut entries) = not_indexed.lock() {
                entries.push((path, ext, NOT_INDEXED_READ));
            }
        }
    };
    // Report fixed reasons and paths only, on every successful exit, including no-change runs.
    let flush_doc_skips = |unreadable: &[(String, String)]| {
        let mut skips = doc_skips
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default();
        skips.sort();
        for (rel_path, reason) in &skips {
            eprintln!("DOC-SKIPPED: {rel_path}: {reason}");
        }
        #[cfg(test)]
        DOC_SKIPS.with(|n| n.borrow_mut().extend(skips.iter().cloned()));
        for (rel_path, reason) in unreadable {
            eprintln!(
                "UNREADABLE: {rel_path}: {reason} (old extractor output kept; retried each run)"
            );
        }
        #[cfg(test)]
        UNREADABLE_FILES.with(|n| n.borrow_mut().extend(unreadable.iter().cloned()));

        let entries = not_indexed
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default();
        if entries.is_empty() {
            return;
        }
        #[cfg(test)]
        NOT_INDEXED.with(|n| {
            n.borrow_mut().extend(
                entries
                    .iter()
                    .map(|(path, _, reason)| (path.clone(), (*reason).to_string())),
            );
        });
        let total = entries.len();
        let mut groups: std::collections::BTreeMap<(String, &str), Vec<String>> =
            std::collections::BTreeMap::new();
        for (path, ext, reason) in entries {
            groups.entry((ext, reason)).or_default().push(path);
        }
        for ((ext, reason), mut paths) in groups {
            paths.sort();
            let count = paths.len();
            paths.truncate(3);
            let label = if ext.is_empty() {
                "(no extension)".to_string()
            } else {
                format!(".{ext}")
            };
            eprintln!(
                "NOT-INDEXED: {label}: {count} file(s): {reason} (e.g. {})",
                paths.join(", ")
            );
        }
        eprintln!("NOT-INDEXED: {total} of {walked} walked file(s) not indexed");
    };

    let current_rel_paths: HashSet<String> = supported
        .iter()
        .map(|(_, r)| repo_scope::namespaced(repo, r))
        .collect();

    let work: Vec<FileWork> = supported
        .into_par_iter()
        .filter_map(|(abs, rel_path)| {
            // #405: confined — a path that resolves outside the root is never read.
            if doc_convert::is_doc_path(&rel_path) {
                // A binary document: read capped, digested as raw bytes (what freshness compares),
                // and its bytes NOT held for the whole run (a repo of PDFs would sit in memory at
                // once): it is read again when, and only if, it converts.
                let Some(resolved) = confined_root.as_ref().and_then(|c| c.resolve(&abs)) else {
                    skip_new_unreadable(&rel_path);
                    return None;
                };
                let bytes = match doc_convert::read_capped(&resolved) {
                    Ok(b) => b,
                    Err(reason) => {
                        skip_doc(&repo_scope::namespaced(repo, &rel_path), reason.to_string());
                        return None;
                    }
                };
                let digest = source_digest(&bytes, None);
                return Some(FileWork {
                    abs,
                    rel: repo_scope::namespaced(repo, &rel_path),
                    raw: rel_path,
                    len: bytes.len(),
                    digest,
                });
            }
            let Some(source) = read_index_source(confined_root.as_ref(), root, &abs, &rel_path)
            else {
                skip_new_unreadable(&rel_path);
                return None;
            };
            Some(FileWork {
                abs,
                rel: repo_scope::namespaced(repo, &rel_path),
                raw: rel_path,
                len: source.bytes.len(),
                digest: source.digest,
            })
        })
        .collect();

    // #476: keep old output for unreadable indexed files, but retry only those that missed a
    // full re-extraction or already held old output last run. Ordinary incremental read failures
    // need no record: changed bytes will still mismatch their stored digest once readable.
    let doc_skip_reasons: HashMap<String, String> = doc_skips
        .lock()
        .map(|skips| skips.iter().cloned().collect())
        .unwrap_or_default();
    let skipped_prev_indexed: HashSet<&String> = {
        let work_rels: HashSet<&str> = work.iter().map(|fw| fw.rel.as_str()).collect();
        previously_indexed
            .iter()
            .filter(|p| current_rel_paths.contains(*p) && !work_rels.contains(p.as_str()))
            .collect()
    };
    let mut unreadable_files: Vec<(String, String)> = skipped_prev_indexed
        .into_iter()
        .filter(|path| force_full || prev_unreadable.contains(path.as_str()))
        .map(|path| {
            let reason = doc_skip_reasons
                .get(path)
                .cloned()
                .unwrap_or_else(|| "read failed".to_string());
            (path.clone(), reason)
        })
        .collect();
    unreadable_files.sort();

    // ── Remove DELETED files ─────────────────────────────────────────────────────────────────
    let deleted: Vec<String> = previously_indexed
        .iter()
        .filter(|p| !current_rel_paths.contains(*p))
        .cloned()
        .collect();
    // Lane relative-imports (Decision J): collect each DELETED file's direct importers — files
    // holding a File→File `Imports` edge into it — BEFORE the removal batch, which destroys the
    // very edges this discovery walks. The set is computed once, from the original deleted list
    // only (no transitive cascade), and forces those files through re-extraction below so their
    // relative-import refs re-park honestly. A merely-MODIFIED target needs nothing: its
    // importers' edges survive `remove_file` by store semantics (see the module doc).
    let mut forced_importers: HashSet<String> = HashSet::new();
    for path in &deleted {
        let target_id = Symbol::file(path.clone()).id();
        for e in store.neighbors(&target_id, wicked_estate_core::Direction::Dependents)? {
            if e.kind != EdgeKind::Imports {
                continue;
            }
            if let Some(n) = store.get_node(&e.source)? {
                if matches!(n.kind, NodeKind::File) {
                    forced_importers.insert(n.location.file.clone());
                }
            }
        }
    }
    for d in &deleted {
        forced_importers.remove(d); // a deleted importer is gone, not re-extracted
    }
    // Keep DELETED files until their semantic fields have been snapshotted below (#438). The
    // forced-importer discovery above already read the Imports edges it needs before removal.

    // ── Split CHANGED/NEW from UNCHANGED ────────────────────────────────────────────────────
    let work_by_rel: HashMap<String, FileWork> =
        work.into_iter().map(|fw| (fw.rel.clone(), fw)).collect();
    let mut stored_digest_by_rel: HashMap<String, Option<String>> = HashMap::new();
    let mut changed_seed: HashSet<String> = HashSet::new();
    for (rel, fw) in &work_by_rel {
        let stored = store.file_digest(rel)?;
        if force_full
            || prev_unreadable.contains(rel)
            || forced_importers.contains(rel)
            || stored.as_deref() != Some(fw.digest.as_str())
        {
            changed_seed.insert(rel.clone());
        }
        stored_digest_by_rel.insert(rel.clone(), stored);
    }

    // Call-derived value-flow edges can be stored with a changed callee-owned parameter/return
    // endpoint but sourced from an unchanged caller's call-site facts. Re-extract the reverse
    // Calls/flows_to closure before remove_file() deletes those endpoint edges, otherwise a
    // callee-only edit drops the caller-derived semantic flow until the caller changes.
    //
    // The invariant is exactly ONE hop: a call-site fact in the caller joined to the callee's own
    // parameter/return endpoint. A caller-of-a-caller holds no fact about this callee, so the
    // transitive fixed point this used to run bought nothing and cost everything — on a 905-file
    // TypeScript repo a one-line edit to a leaf test file forced 719 files into re-extraction
    // (20.7 s / 1.2 GB, ~3× the full index) and `watch` paid it on every save
    // (wicked-estate#207 review, C2). Pinned by
    // `incremental_leaf_edit_forces_only_direct_callers`; the one-hop invariant it protects is
    // pinned by `incremental_callee_only_edit_preserves_call_derived_value_flow`.
    //
    // NOT gated on the store already holding a `flows_to` edge, deliberately: the edit that makes
    // a callee flow-capable for the FIRST time (a destructured parameter becoming a plain one)
    // is exactly when a store holds none, and skipping it strands the caller's transient
    // call-site facts — the same full-vs-incremental divergence this change exists to remove.
    // Pinned by `an_edit_that_first_makes_a_callee_flow_capable_forces_its_callers`.
    let mut forced_value_flow_callers: HashSet<String> = HashSet::new();
    // #229: edges a forced, content-UNCHANGED file's re-extraction deletes on behalf of OTHER,
    // unchanged files. `remove_file` deletes every edge SOURCED at the file's nodes, and a
    // call-derived `call_argument` edge (callee parameter ← caller argument) is sourced at the
    // callee but OWNED by the caller (its location is the caller's call site). Forcing the
    // one-hop callers of an edit re-extracts them, so their own callers' call-derived edges into
    // them vanished and nothing re-emitted them (446 `flows_to` lost on a 564-file TS repo, 97 on
    // the 905-file one). The forced file's extraction is byte-identical, so those edges are
    // still exactly right: they are captured here and restored after the write.
    let mut collateral_edges: Vec<Edge> = Vec::new();
    // #220: an unchanged direct caller whose value-flow call refs are on record is NOT
    // re-extracted. Its recorded refs are re-resolved against the post-write index instead and
    // `call_value_flow` re-derives exactly its sites into the changed callees — the same function
    // of (site Calls edges, index, hints) a re-extraction would have fed, minus the re-parse. On a
    // 905-file repo a hub edit re-parsed 121 callers to recover facts computed once already. A
    // caller with no readable record (a DB from before the record existed) still takes the
    // re-extract path below, so there is no flag day.
    let mut changed_symbols: HashSet<SymbolId> = HashSet::new();
    let mut rederive_files: HashSet<String> = HashSet::new();
    let mut rederive_refs: Vec<wicked_estate_core::UnresolvedRef> = Vec::new();
    let mut pre_run_symbols = HashSet::new();
    let mut old_candidates = Vec::new();
    let mut file_by_symbol: HashMap<SymbolId, String> = HashMap::new();
    if !force_full && (!changed_seed.is_empty() || !deleted.is_empty()) {
        // Keep the pre-run population and removed candidates for homonym invalidation (#425).
        let deleted_files: HashSet<&str> = deleted.iter().map(String::as_str).collect();
        let mut old_signature = ReplaySignature::default();
        for node in store.all_nodes()? {
            if scope
                .as_deref()
                .is_some_and(|p| !node.location.file.starts_with(p))
            {
                continue;
            }
            pre_run_symbols.insert(node.symbol.clone());
            if changed_seed.contains(&node.location.file) {
                changed_symbols.insert(node.symbol.clone());
                old_signature.add(&node);
            }
            if changed_seed.contains(&node.location.file)
                || deleted_files.contains(node.location.file.as_str())
            {
                file_by_symbol.insert(node.symbol.clone(), node.location.file.clone());
                old_candidates.push(node);
            } else {
                file_by_symbol.insert(node.symbol, node.location.file);
            }
        }
        // Deletions need the node population, but do not add an all-edges scan.
        let all_edges = if changed_seed.is_empty() {
            Vec::new()
        } else {
            store.all_edges()?
        };
        for edge in &all_edges {
            // Only a DIRECT caller of a changed callee holds call-site facts about it.
            if edge.kind != EdgeKind::Calls || !changed_symbols.contains(&edge.target) {
                continue;
            }
            for candidate in [
                edge.location.as_ref().map(|loc| loc.file.as_str()),
                file_by_symbol.get(&edge.source).map(String::as_str),
            ]
            .into_iter()
            .flatten()
            {
                if !changed_seed.contains(candidate)
                    && current_rel_paths.contains(candidate)
                    && work_by_rel.contains_key(candidate)
                    && !rederive_files.contains(candidate)
                    && !forced_value_flow_callers.contains(candidate)
                {
                    match recorded_call_refs(&*store, candidate)? {
                        Some(refs) => {
                            rederive_files.insert(candidate.to_string());
                            rederive_refs.extend(refs);
                        }
                        None => {
                            forced_value_flow_callers.insert(candidate.to_string());
                        }
                    }
                }
            }
        }
        // #220 review: replaying the recorded refs is exact only while the edit changes no
        // caller's RESOLUTION — the changed files define the same symbols of the same kinds — and
        // takes away no return endpoint a recorded `call_result` hop stands on (a full index would
        // then not mint that hop's consumer node at all). Anything else re-extracts the callers,
        // which re-parks a call that no longer resolves and retires what it no longer emits.
        if !rederive_files.is_empty() {
            // The post-edit signature, from a parse of just the changed files (pure; the write
            // below parses them again). `None` = cannot tell (a rule-matched, minified,
            // non-UTF-8 or converted-document file): re-extract.
            let new_signature = (|| {
                let mut signature = ReplaySignature::default();
                for rel in &changed_seed {
                    let fw = work_by_rel.get(rel)?;
                    if extra.as_ref().is_some_and(|x| x.matches_path(&fw.raw))
                        || doc_convert::is_doc_path(&fw.raw)
                    {
                        return None;
                    }
                    let source = read_index_source(confined_root.as_ref(), root, &fw.abs, &fw.raw)?;
                    let text = std::str::from_utf8(&source.bytes).ok()?;
                    if is_minified_or_huge(text) {
                        return None;
                    }
                    // #405: the replay signature must come from the same masked text the write
                    // below extracts from.
                    let text = &wicked_estate_extract::redact::redact_source(&fw.raw, text);
                    let ext = fw
                        .abs
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if let Some(extraction) = base_extraction(&fw.rel, &ext, text, &ext_map) {
                        extraction.nodes.iter().for_each(|n| signature.add(n));
                    }
                }
                Some(signature)
            })();
            let exact = new_signature.is_some_and(|new| old_signature.replay_is_exact(&new));
            if !exact {
                forced_value_flow_callers.extend(rederive_files.drain());
                rederive_refs.clear();
            }
        }
        // Files re-extracted with identical content: the forced value-flow callers, plus the
        // Decision-J forced importers whose digest did not change.
        let unchanged_forced: HashSet<&str> = changed_seed
            .iter()
            .filter(|rel| {
                stored_digest_by_rel
                    .get(rel.as_str())
                    .and_then(|d| d.as_deref())
                    .zip(work_by_rel.get(rel.as_str()))
                    .is_some_and(|(stored, fw)| stored == fw.digest)
            })
            .map(String::as_str)
            .chain(forced_value_flow_callers.iter().map(String::as_str))
            .collect();
        if !unchanged_forced.is_empty() {
            for edge in all_edges {
                let Some(owner) = edge.location.as_ref().map(|l| l.file.as_str()) else {
                    continue;
                };
                // Only value-flow edges: the call-derived hops are the edges another file's
                // facts source at a callee's nodes (codex review: keep the restore that narrow).
                if !is_flow_edge(&edge) {
                    continue;
                }
                let source_file = file_by_symbol.get(&edge.source).map(String::as_str);
                if source_file.is_some_and(|f| unchanged_forced.contains(f))
                    && owner != source_file.unwrap_or_default()
                    && !changed_seed.contains(owner)
                    && !forced_value_flow_callers.contains(owner)
                {
                    collateral_edges.push(edge);
                }
            }
        }
    }

    let mut changed: Vec<FileWork> = Vec::new();
    let mut unchanged_count: usize = 0;
    let mut unchanged_work: HashMap<String, FileWork> = HashMap::new();
    // Paths with NO digest row — never indexed before (or removed and re-created since). The
    // count gates the back-fill pass's import lane below: only a NEW File path can be what a
    // parked relative-import ref was waiting for.
    let mut new_file_count: usize = 0;
    for (rel, fw) in work_by_rel {
        let stored = stored_digest_by_rel.remove(&rel).unwrap_or(None);
        // A direct importer of a DELETED file is forced into `changed` even with a matching
        // digest (Decision J): consulted HERE, while the FileWork is still alive — the
        // unchanged arm retains only its lightweight work record.
        // #476: a file recorded as still holding old extractor output is re-extracted alone once
        // readable, though its bytes (and so its digest) never changed.
        let forced = forced_importers.contains(&rel)
            || forced_value_flow_callers.contains(&rel)
            || prev_unreadable.contains(&rel);
        if !force_full && !forced && stored.as_deref() == Some(&fw.digest) {
            // UNCHANGED: skip extraction entirely; its nodes/edges already in the store.
            //
            // Safe for MULTI-FILE symbols by construction (M4 / Option A — wicked-estate#152):
            // this skip used to be the third leg of a data-loss triad — `remove_file` deleted a
            // shared node (a `.h` prototype + `.cpp` definition minting ONE SymbolId) when EITHER
            // file went away, and the unchanged survivor was never re-extracted, so the node
            // stayed gone until the survivor itself changed. The store now records per-(symbol,
            // file) contributions (`node_files`): `remove_file` retires only the removed file's
            // contribution and re-homes the node onto the surviving file's stored record, so an
            // unchanged survivor's nodes/edges genuinely ARE "already in the store" — exactly
            // what this skip assumes. No forced re-extraction of co-declaring files is needed
            // (contrast the Decision-J forced-importer re-extract above, which exists for parked
            // RELATIVE-IMPORT refs, a different mechanism).
            unchanged_count += 1;
            unchanged_work.insert(rel, fw);
        } else {
            if stored.is_none() {
                new_file_count += 1;
            }
            changed.push(fw);
        }
    }

    // If nothing changed AND nothing was deleted, skip all phases. A deletion-only run still has
    // work to do below: it must remove the deleted files inside the write batch and report every
    // annotated symbol it drops (#438). The early return is allowed only when BOTH are empty.
    if changed.is_empty() && deleted.is_empty() {
        // Stamp the id scheme only when the gate did NOT fire this run: a fresh DB or an
        // already-current one gets the key; a gate-fired run that found zero source files leaves
        // the old key so the gate re-fires next time (idempotent, never falsely certified).
        if !scheme_gate {
            store.meta_set_key(&scheme_key, SYMBOL_ID_SCHEME);
        }
        // Same write-last discipline for the plugin-override key (ADR-010): certify only a run
        // whose gate did not fire.
        if !override_gate {
            store.meta_set_key(&override_key, cur_overrides);
        }
        // #476: certify current markers; skipped files retain their individual retry record.
        stamp_extractor_markers(
            store,
            &version_key,
            &digest_key,
            &extra_digest,
            &unreadable_key,
            &unreadable_files,
        );
        flush_doc_skips(&unreadable_files);
        return store.stats();
    }

    // Activation table: docs/ENGINE-CONTRACT.md §3.1 — guarded by tests::slice_matches_engine_contract_table.
    // InfraResolver handles IaC/tfstate resource refs; it does not interfere with code
    // resolvers (it only fires when raw_name maps exclusively to resource nodes).
    // RelativeImportResolver binds relative JS/TS Imports refs to their target File node
    // (exact-path, root-guarded against the repo/label root; lane relative-imports).
    let relative = wicked_estate_resolve::RelativeImportResolver::new(scope.as_deref());
    let resolvers: &[&dyn Resolver] = &[
        &NameResolver,
        &ScopedNameResolver,
        &ImportMapResolver,
        &relative,
        &InfraResolver,
        &RulesBridgeResolver,
        &DynamicCallResolver,
        &ApexlangRefResolver,
    ];

    // #422: bound each parallel extraction by source bytes AND file count. An oversized file
    // runs alone so every chunk makes progress. Source bytes, extractions and redacted text are
    // held only for the current chunk.
    const EXTRACT_CHUNK_BYTES: usize = 16 * 1024 * 1024;
    let (chunk_byte_budget, chunk_file_cap) = (EXTRACT_CHUNK_BYTES, 512);
    #[cfg(test)]
    let (chunk_byte_budget, chunk_file_cap) = TEST_EXTRACT_CHUNK_LIMITS
        .with(|v| v.get())
        .unwrap_or((chunk_byte_budget, chunk_file_cap));
    let skipped_minified: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

    // ── EXTRACT + WRITE ────────────────────────────────────────────────────────────────────
    // write-nodes: upsert nodes rows (hot path — prepare_cached, one transaction per chunk).
    // write-content: zstd-compress + upsert content/files rows.
    // write-FTS: bulk populate nodes_fts after the chunk's nodes are written.
    //
    // #515: extract outside a batch; hold the write lock only for each chunk's remove + write +
    // semantic restore. This trades whole-run atomicity for progress by other SQLite writers.
    // Digests are written only with the final resolution batch, so a failed run re-extracts the
    // committed chunks on retry. Missing symbols' fields stay pending for cross-chunk moves and
    // call-derived ids (#470), and are counted as dropped only after the final batch commits.
    let mut all_refs = Vec::new();

    // #438/#463: snapshot semantics inside the batch that removes and restores the rows. Each
    // batch enqueues only its own files' entries; future chunks' entries stay out of pending
    // until their old contributions are removed, so an early restore onto a symbol still in a
    // later chunk's file cannot lose that file's semantics when its chunk runs.
    // Deleted files get their own short batch, committed before extraction begins.
    if !deleted.is_empty() {
        let rels: Vec<&str> = deleted.iter().map(String::as_str).collect();
        store.begin_batch()?;
        let snapshot = store.semantics_in_files(&rels)?;
        for path in &deleted {
            store.remove_file(path)?;
            // W7.1: log the removal so subscribers see the deletion delta.
            let _ = store.log_change(ChangeOp::Remove, path);
        }
        pending_semantics.extend(snapshot);
        pending_semantics_save(store, &other_pending_semantics, &pending_semantics)?;
        store.commit_batch()?;
        #[cfg(test)]
        test_index_fail_after("deletions")?;
    }
    let mut total_symbols = 0usize;
    let mut files_extracted = 0usize;
    let mut extracted_symbols = HashSet::new();
    let mut backfill_probes = Vec::new();
    let mut seen_probes = HashSet::new();
    let mut read_digests = Vec::new();
    let mut chunk_start = 0;
    for pass in 0..2 {
        while chunk_start < changed.len() {
            let mut chunk_end = chunk_start + 1;
            let mut chunk_bytes = changed[chunk_start].len;
            while chunk_end < changed.len()
                && chunk_end - chunk_start < chunk_file_cap
                && chunk_bytes.saturating_add(changed[chunk_end].len) <= chunk_byte_budget
            {
                chunk_bytes = chunk_bytes.saturating_add(changed[chunk_end].len);
                chunk_end += 1;
            }
            let chunk = &changed[chunk_start..chunk_end];
            #[cfg(test)]
            test_index_write_hook();
            let read_failures = std::sync::Mutex::new(Vec::new());
            let chunk_digests = std::sync::Mutex::new(Vec::new());
            let extractions: Vec<(String, wicked_estate_core::Extraction, String)> = chunk
                .par_iter()
                .filter_map(|fw| {
                    let mut ext = fw
                        .abs
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    let source = if doc_convert::is_doc_ext(&ext) {
                        None
                    } else {
                        match read_index_source(confined_root.as_ref(), root, &fw.abs, &fw.raw) {
                            Some(source) => Some(source),
                            None => {
                                read_failures
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .push(fw.rel.clone());
                                return None;
                            }
                        }
                    };
                    // Every readable source keeps its chunk-time digest, even when extraction
                    // is skipped. Documents retain their raw-byte classification digest.
                    chunk_digests
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push((
                            fw.rel.clone(),
                            source
                                .as_ref()
                                .map_or_else(|| fw.digest.clone(), |source| source.digest.clone()),
                        ));
                    // Convert binary documents to Markdown for redaction, parsing and storage
                    // under the document's own path. Conversion failures are reported and skipped.
                    // The digest stays the raw bytes', so unchanged documents are not converted.
                    let converted: Option<String> = if doc_convert::is_doc_ext(&ext) {
                        let bytes = confined_root
                            .as_ref()
                            .and_then(|c| c.resolve(&fw.abs))
                            .ok_or(doc_convert::Skipped::Unreadable("outside the root"))
                            .and_then(|p| doc_convert::read_capped(&p));
                        let bytes = match bytes {
                            Ok(b) => b,
                            Err(reason) => {
                                skip_doc(&fw.rel, reason.to_string());
                                return None;
                            }
                        };
                        // Convert only bytes matching the classification digest. A document that
                        // changed between reads is skipped and converted on the next run.
                        if source_digest(&bytes, None) != fw.digest {
                            skip_doc(&fw.rel, "changed while indexing; converted next run".into());
                            return None;
                        }
                        match doc_convert::to_markdown(&fw.raw, &bytes) {
                            Ok(md) => {
                                ext = "md".to_string();
                                Some(md)
                            }
                            Err(reason) => {
                                skip_doc(&fw.rel, reason.to_string());
                                return None;
                            }
                        }
                    } else {
                        None
                    };
                    // #405: mask secrets before parsing, so every stored fact derives from masked
                    // text. Byte length and spans stay valid; the digest stays the file's own.
                    let committed = match &converted {
                        Some(md) => md.as_str(),
                        None => std::str::from_utf8(&source.as_ref()?.bytes).ok()?,
                    };
                    let redact_path = match converted {
                        Some(_) => std::borrow::Cow::Owned(format!("{}.md", fw.raw)),
                        None => std::borrow::Cow::Borrowed(fw.raw.as_str()),
                    };
                    let text =
                        wicked_estate_extract::redact::redact_source(&redact_path, committed);
                    let extra_rules = extra.as_ref().filter(|x| x.matches_path(&fw.raw));

                    // Skip minified or huge files before parsing. Extra-edge rule targets still
                    // get the regex pass. Converted documents keep their content and File node:
                    // text is already capped, and long prose lines are valid document content.
                    let minified = is_minified_or_huge(&text);
                    if minified && extra_rules.is_none() && converted.is_none() {
                        skipped_minified
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push(fw.rel.clone());
                        return None;
                    }

                    let mut extraction = if minified {
                        Extraction::default()
                    } else {
                        // A file with no language extractor still gets the extra-edge pass below.
                        base_extraction(&fw.rel, &ext, &text, &ext_map).unwrap_or_default()
                    };
                    if let Some(meta) = source.as_ref().and_then(|s| s.export_metadata.as_ref()) {
                        wicked_estate_extract::apexlang::apply_export_metadata(
                            &mut extraction,
                            &fw.rel,
                            meta.as_deref(),
                        );
                    }

                    // ── Extra-edge pass: drop-in domain rules ─────────────────────────────────
                    if let Some(x) = extra_rules {
                        // Rules glob against the RAW repo-relative path; the ids they mint are then
                        // rewritten into the repo's namespace (no-op when un-labelled).
                        // A rule reads its chosen file as committed: captures bypass the default
                        // config masking, but stored names/ids pass the token-shape backstop
                        // (#405; docs/secret-redaction.md).
                        let sf = SourceFile {
                            path: fw.raw.clone(),
                            language: Language::new("text"),
                            text: wicked_estate_core::redact::scrub(committed).into_owned(),
                        };
                        let mut ee = x.extract_extra(&sf);
                        if let Some(label) = repo {
                            repo_scope::namespace_extra(&mut ee, label, &fw.rel);
                        }
                        // Injected edges need a File node even without a language extractor.
                        let file_symbol = Symbol::file(&fw.rel).id();
                        if !extraction.nodes.iter().any(|n| n.symbol == file_symbol) {
                            extraction.nodes.push(Node::new(
                                file_symbol,
                                NodeKind::File,
                                fw.rel.clone(),
                                Language::new("text"),
                                Location::new(&fw.rel, Span::ZERO),
                            ));
                        }
                        extraction.nodes.extend(ee.nodes);
                        extraction.local_edges.extend(ee.edges);
                        extraction.refs.extend(ee.unresolved_refs);
                    }

                    if converted.is_some() {
                        // Store document content even when the grammar found no heading.
                        let file_symbol = Symbol::file(&fw.rel).id();
                        if !extraction.nodes.iter().any(|n| n.symbol == file_symbol) {
                            extraction.nodes.push(Node::new(
                                file_symbol,
                                NodeKind::File,
                                fw.rel.clone(),
                                Language::new("markdown"),
                                Location::new(&fw.rel, Span::ZERO),
                            ));
                        }
                    }
                    if extraction.nodes.is_empty()
                        && extraction.local_edges.is_empty()
                        && extraction.refs.is_empty()
                    {
                        return None;
                    }
                    Some((fw.rel.clone(), extraction, text))
                })
                .collect();
            #[cfg(test)]
            TEST_MAX_EXTRACTIONS.with(|v| v.set(v.get().max(extractions.len())));
            read_digests.extend(
                chunk_digests
                    .into_inner()
                    .unwrap_or_else(|e| e.into_inner()),
            );

            let read_failures: HashSet<String> = read_failures
                .into_inner()
                .unwrap_or_else(|e| e.into_inner())
                .into_iter()
                .collect();
            for file in &read_failures {
                unreadable_files.push((file.clone(), "read failed".to_string()));
            }
            let rels: Vec<&str> = chunk
                .iter()
                .filter(|fw| !read_failures.contains(&fw.rel))
                .map(|fw| fw.rel.as_str())
                .collect();
            store.begin_batch()?;
            let snapshot = store.semantics_in_files(&rels)?;
            // A failed re-read keeps the old contribution and digest for a recorded retry (#476).
            for rel in &rels {
                store.remove_file(rel)?;
            }
            pending_semantics.extend(snapshot);
            for (rel_path, extraction, text) in extractions {
                store.upsert_nodes_skip_fts(&extraction.nodes)?;
                store.upsert_edges(&extraction.local_edges)?;
                // Store source text for the file (best-effort; log on failure, don't abort).
                if let Err(e) = store.set_file_content(&rel_path, &text) {
                    eprintln!("warning: set_file_content({rel_path}) failed: {e}");
                }
                // #220: record value-flow call refs (`[]` means none) so later callee edits can
                // re-derive this file's call-derived flows without re-parsing it.
                let call_refs: Vec<&wicked_estate_core::UnresolvedRef> = extraction
                    .refs
                    .iter()
                    .filter(|r| is_value_flow_call_ref(r))
                    .collect();
                let call_refs_json = serde_json::to_string(&call_refs).map_err(|e| {
                    wicked_estate_core::Error::Invalid(format!("call refs of {rel_path}: {e}"))
                })?;
                store.set_file_call_refs(&rel_path, &call_refs_json)?;
                total_symbols += extraction.nodes.len();
                files_extracted += 1;
                for n in &extraction.nodes {
                    extracted_symbols.insert(n.symbol.clone());
                    if n.name.is_empty() {
                        continue;
                    }
                    // Preserve encounter order for exact-name, APEXlang and program probes.
                    for probe in node_backfill_probes(n) {
                        if seen_probes.insert(probe.clone()) {
                            backfill_probes.push(probe);
                        }
                    }
                }
                all_refs.extend(extraction.refs);
            }
            store.bulk_rebuild_fts_for_files(
                &chunk.iter().map(|fw| fw.rel.as_str()).collect::<Vec<_>>(),
            )?;
            // The snapshot is made durable in the batch that removed the rows, then shrinks in the
            // batch that restores entries (#515).
            pending_semantics = restore_pending_semantics(store, pending_semantics)?;
            pending_semantics_save(store, &other_pending_semantics, &pending_semantics)?;
            store.commit_batch()?;
            // The chunk's extractions and source text are dropped before extracting the next one.
            chunk_start = chunk_end;
        }
        if pass == 1 || force_full || unchanged_work.is_empty() {
            break;
        }

        // Scan only resolver-produced edges into structural same-name targets when a structural
        // candidate is gained or lost. Scoped bindings can exclude changes outside their tier.
        // This over-approximates import-map and cross-file bindings: their unchanged owner is
        // re-extracted even when an import keeps the edge bound after a same-name gain or loss.
        let mut changed_names: HashMap<String, HashSet<String>> = HashMap::new();
        for symbol in extracted_symbols.difference(&pre_run_symbols) {
            if let Some(node) = store
                .get_node(symbol)?
                .filter(|node| is_structural_symbol(node) && !node.name.is_empty())
            {
                changed_names
                    .entry(node.name)
                    .or_default()
                    .insert(node.location.file);
            }
        }
        let mut lost_by_name: HashMap<String, Vec<SymbolId>> = HashMap::new();
        for node in &old_candidates {
            if node.name.is_empty() || store.get_node(&node.symbol)?.is_some() {
                continue;
            }
            // Removed homonyms can make a parked name unique, including typed probe forms.
            for probe in node_backfill_probes(node) {
                if seen_probes.insert(probe.clone()) {
                    backfill_probes.push(probe);
                }
            }
            if !is_structural_symbol(node) {
                continue;
            }
            changed_names
                .entry(node.name.clone())
                .or_default()
                .insert(node.location.file.clone());
            lost_by_name
                .entry(node.name.clone())
                .or_default()
                .push(node.symbol.clone());
        }
        let mut rebind_files = HashSet::new();
        for (name, candidate_files) in changed_names {
            let mut targets: HashSet<SymbolId> = store
                .find_symbols(&SymbolQuery {
                    exact_name: Some(name.clone()),
                    ..Default::default()
                })?
                .into_iter()
                .filter(is_structural_symbol)
                .filter(|n| {
                    scope
                        .as_deref()
                        .is_none_or(|p| n.location.file.starts_with(p))
                })
                .map(|n| n.symbol)
                .collect();
            targets.extend(lost_by_name.remove(&name).unwrap_or_default());
            let candidate_files: Vec<&str> = candidate_files.iter().map(String::as_str).collect();
            for target in targets {
                for edge in store.neighbors(&target, wicked_estate_core::Direction::Dependents)? {
                    if !resolvers
                        .iter()
                        .any(|resolver| edge.resolved_by == resolver.id())
                    {
                        continue;
                    }
                    let owner = match edge.location.as_ref() {
                        Some(location) => location.file.clone(),
                        None => match store.get_node(&edge.source)? {
                            Some(node) => node.location.file,
                            None => continue,
                        },
                    };
                    if unchanged_work.contains_key(&owner)
                        && !wicked_estate_resolve::scoped_binding_survives(
                            &edge,
                            &owner,
                            &candidate_files,
                        )
                    {
                        rebind_files.insert(owner);
                    }
                }
            }
        }
        if rebind_files.is_empty() {
            break;
        }
        eprintln!(
            "REBIND: re-extracting {} file(s) whose name-bound refs a homonym change can re-bind",
            rebind_files.len(),
        );
        #[cfg(test)]
        TEST_REBIND_FILES.with(|count| count.set(count.get() + rebind_files.len()));
        // #229: only other still-unchanged owners' outgoing value-flow edges survive a forced
        // file's identical re-extraction. Its own refs are resolved afresh in the main pass.
        collateral_edges.retain(|edge| {
            edge.location
                .as_ref()
                .is_none_or(|location| !rebind_files.contains(&location.file))
        });
        for (symbol, source_file) in &file_by_symbol {
            if !rebind_files.contains(source_file) {
                continue;
            }
            for edge in store.neighbors(symbol, wicked_estate_core::Direction::Dependencies)? {
                if is_flow_edge(&edge)
                    && edge.location.as_ref().is_some_and(|location| {
                        location.file != *source_file
                            && unchanged_work.contains_key(&location.file)
                            && !rebind_files.contains(&location.file)
                    })
                {
                    collateral_edges.push(edge);
                }
            }
        }
        rederive_refs.retain(|r| !rebind_files.contains(&r.location.file));
        unchanged_count -= rebind_files.len();
        for file in rebind_files {
            if let Some(fw) = unchanged_work.remove(&file) {
                changed.push(fw);
            }
        }
        // Identical source cannot introduce another name; pass two never schedules a third.
    }
    drop(seen_probes);
    // Release the pre-run population before materializing the resolution index.
    drop(pre_run_symbols);
    drop(old_candidates);
    drop(file_by_symbol);
    drop(unchanged_work);

    unreadable_files.sort();
    flush_doc_skips(&unreadable_files);
    let mut skipped_min = skipped_minified
        .into_inner()
        .unwrap_or_else(|e| e.into_inner());
    skipped_min.sort();
    if !skipped_min.is_empty() {
        eprintln!("SKIPPED_MINIFIED: {} file(s)", skipped_min.len());
        for path in &skipped_min {
            eprintln!("SKIPPED_MINIFIED: {path}");
        }
    }

    // ── Emit extraction counters (best-effort) ───────────────────────────────────────────────
    {
        let sink = wicked_estate_observe::init_sink_from_env();
        let resource = wicked_estate_core::observability::Resource::service(
            "wicked_estate",
            env!("CARGO_PKG_VERSION"),
        );
        let scope =
            wicked_estate_core::observability::InstrumentationScope::new("wicked_estate.index");
        use wicked_estate_core::observability::*;
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let files_metric = Metric {
            name: "wicked_estate.extract.files_total".to_string(),
            description: String::new(),
            unit: "1".to_string(),
            data: MetricData::Sum {
                data_points: vec![NumberDataPoint {
                    attributes: vec![],
                    start_time_unix_nano: t,
                    time_unix_nano: t,
                    value: MetricValue::I64(files_extracted as i64),
                }],
                temporality: AggregationTemporality::Delta,
                is_monotonic: true,
            },
        };
        if let Err(e) = sink.export_metrics(&resource, &scope, &[files_metric]) {
            eprintln!("telemetry: {e}");
        }

        if files_extracted > 0 {
            let avg_symbols = total_symbols as f64 / files_extracted as f64;
            let sym_metric = Metric {
                name: "wicked_estate.extract.symbols_per_file".to_string(),
                description: String::new(),
                unit: "1".to_string(),
                data: MetricData::Histogram {
                    data_points: vec![HistogramDataPoint {
                        attributes: vec![],
                        start_time_unix_nano: t,
                        time_unix_nano: t,
                        count: files_extracted as u64,
                        sum: avg_symbols * files_extracted as f64,
                        bucket_counts: vec![files_extracted as u64],
                        explicit_bounds: vec![],
                    }],
                    temporality: AggregationTemporality::Delta,
                },
            };
            if let Err(e) = sink.export_metrics(&resource, &scope, &[sym_metric]) {
                eprintln!("telemetry: {e}");
            }
        }
    }

    // ── RESOLVE (changed files only) ─────────────────────────────────────────────────────────
    // Build the in-memory index from ALL nodes (unchanged + newly written). Resolve only the refs
    // that came from the changed files. The resolved edges are written to the store.
    //
    // Importer scope (see the module doc): direct importers of DELETED files were already
    // forced into `changed` above (Decision J); parked refs whose names gained or lost
    // candidates this run are re-resolved by back-fill below (#141, #425).
    //
    // The index owns its data (clones of all nodes), so it outlives the read borrow and is
    // shared with the back-fill pass.
    let index = {
        let reader: &dyn GraphRead = &*store;
        // Scoped to this repo: a labelled run resolves against its own nodes only, so a name that
        // is unique inside the repo stays unique no matter how many repos share the graph.
        InMemoryIndex::build(reader, scope.as_deref())?
    };
    let resolution = resolve_all_with_coverage(resolvers, &all_refs, &index)?;
    let call_ref_hints = call_value_ref_hints(&all_refs);
    let (call_flow_nodes, call_flow_edges) =
        call_value_flow(&resolution.site_edges, &index, &call_ref_hints)?;
    // Estate cross-domain join: RACF profiles → the datasets/MQ assets they protect, by RACF
    // generic profile matching (most-specific wins). Derived from the full node population (a
    // profile pattern can match assets declared in any file), reusing the index just built.
    let estate = wicked_estate_resolve::estate_edges(index.nodes());

    // Unresolved refs come from the same attribution as the edges — the one definition in
    // docs/ENGINE-CONTRACT.md §2.1 (a ref is unresolved iff no resolver emitted an edge
    // attributed to it, per site).
    // Resolution above reads the committed chunks without a batch. Write its results and the
    // digests together now; call-derived ids must exist before the remaining restore (#470).
    #[cfg(test)]
    test_index_write_hook();
    store.begin_batch()?;
    for (file, digest) in &read_digests {
        store.set_file_digest(file, digest)?;
        let _ = store.log_change(ChangeOp::Upsert, file);
    }
    store.upsert_nodes(&call_flow_nodes)?;
    store.upsert_edges(&resolution.edges)?;
    store.upsert_edges(&call_flow_edges)?;
    store.upsert_edges(&estate)?;
    store.upsert_unresolved_refs(&resolution.unresolved)?;
    let dropped = restore_semantics(store, pending_semantics)?;
    pending_semantics_save(store, &other_pending_semantics, &Vec::new())?;
    store.commit_batch()?;
    #[cfg(test)]
    test_index_fail_after("digests")?;
    // Report the drops only after the batch durably committed, so the line never describes a loss
    // a rollback undid.
    for (file, n) in dropped {
        eprintln!("SEMANTICS-DROPPED: {n} symbol(s) in {file}");
        #[cfg(test)]
        SEMANTICS_DROPS.with(|notes| {
            notes
                .borrow_mut()
                .push(format!("SEMANTICS-DROPPED: {n} symbol(s) in {file}"))
        });
    }
    // wicked-estate#433: a file the secret rule excludes leaves with its requirement annotations;
    // say so once the removal has landed.
    if let Some(note) = secret_exclusion_note(&deleted) {
        eprintln!("{note}");
        #[cfg(test)]
        SECRET_NOTES.with(|n| n.borrow_mut().push(note));
    }

    // ── RE-DERIVE call-derived value flow for unchanged callers (#220) ──────────────────────
    // The callees were just re-extracted (their parameter/return endpoints exist again); the
    // callers' recorded refs re-resolve against the same index a full run would build. Only the
    // sites that bind a changed callee — before or after the edit — are re-derived: every other
    // site's edges were never touched. The callers' own `Calls` edges and parked rows are left
    // as they are, exactly as for any unchanged dependent of an edit.
    if !rederive_refs.is_empty() {
        let mut targets = changed_symbols.clone();
        targets.extend(extracted_symbols);
        let rederived = resolve_all_with_coverage(resolvers, &rederive_refs, &index)?;
        let sites: HashSet<CallSiteKey> = rederived
            .site_edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Calls && targets.contains(&e.target))
            .filter_map(|e| e.location.as_ref().map(call_site_key))
            .collect();
        let site_edges: Vec<Edge> = rederived
            .site_edges
            .into_iter()
            .filter(|e| {
                e.location
                    .as_ref()
                    .is_some_and(|l| sites.contains(&call_site_key(l)))
            })
            .collect();
        let hints = call_value_ref_hints(&rederive_refs);
        let (flow_nodes, flow_edges) = call_value_flow(&site_edges, &index, &hints)?;
        if !flow_nodes.is_empty() || !flow_edges.is_empty() {
            store.begin_batch()?;
            store.upsert_nodes(&flow_nodes)?;
            store.upsert_edges(&flow_edges)?;
            store.commit_batch()?;
        }
    }

    // ── BACK-FILL previously-parked refs (lane importer-backfill, #141) ─────────────────────
    // The main pass resolves CHANGED files' refs only; a ref parked in an EARLIER run — an
    // unchanged importer of `./b` whose `b.ts` just appeared, an unchanged caller of a symbol
    // a changed file just defined — has no edge to be discovered by (D01-7). Re-resolve exactly
    // the parked rows this run's additions or removals could bind (see the module doc).
    //
    // Exclusions: refs from files (re-)extracted THIS run (their rows were just rewritten by
    // the main pass), and — on a labelled run — refs outside this repo's scope (edges must not
    // resolve across repos). Idempotent by construction: a re-resolved row is DELETED in the
    // same batch that writes its edge (edge upsert dedups by key, so a replay is a no-op); a
    // still-parked candidate is left in place, NEVER re-inserted. Stored rows carry no hints,
    // so hint-driven resolution (ImportMapResolver) does not fire here — same as a full
    // re-index would do for a hint-less ref, and strictly more than the pre-#141 behaviour.
    //
    // Skipped under force_full (everything was just re-resolved) and when no file is unchanged
    // (a surviving candidate can only live in an unchanged file) — so a first index and a full
    // re-extract pay nothing.
    let mut backfill_resolved = 0usize;
    let mut backfill_still_parked = 0usize;
    if !force_full && unchanged_count > 0 {
        let changed_files: HashSet<&str> = changed.iter().map(|fw| fw.rel.as_str()).collect();
        let admissible = |r: &wicked_estate_core::UnresolvedRef| {
            !changed_files.contains(r.location.file.as_str())
                && scope
                    .as_deref()
                    .is_none_or(|p| r.location.file.starts_with(p))
        };
        // Persisted site identity — the 7 columns the store keeps per parked row.
        fn site_key(
            r: &wicked_estate_core::UnresolvedRef,
        ) -> (String, String, EdgeKind, String, u32, u32, u32) {
            (
                r.from.0.clone(),
                r.raw_name.clone(),
                r.kind.clone(),
                r.location.file.clone(),
                r.location.span.start_line,
                r.location.span.start_byte,
                r.location.span.end_byte,
            )
        }
        let mut seen: HashSet<(String, String, EdgeKind, String, u32, u32, u32)> = HashSet::new();
        let mut candidates: Vec<wicked_estate_core::UnresolvedRef> = Vec::new();
        // Name lane: exact indexed lookups for extracted and lost nodes' probe forms.
        for probe in &backfill_probes {
            for r in store.unresolved_refs_for_name(probe)? {
                if admissible(&r) && seen.insert(site_key(&r)) {
                    candidates.push(r);
                }
            }
        }
        // Import lane: only a NEW File path can bind a parked relative import.
        if new_file_count > 0 {
            for r in store.parked_relative_import_refs()? {
                if admissible(&r) && seen.insert(site_key(&r)) {
                    candidates.push(r);
                }
            }
        }
        if !candidates.is_empty() {
            let backfill = resolve_all_with_coverage(resolvers, &candidates, &index)?;
            let still: HashSet<_> = backfill.unresolved.iter().map(site_key).collect();
            let bound: Vec<wicked_estate_core::UnresolvedRef> = candidates
                .iter()
                .filter(|r| !still.contains(&site_key(r)))
                .cloned()
                .collect();
            backfill_resolved = bound.len();
            backfill_still_parked = backfill.unresolved.len();
            let backfill_call_files: HashSet<String> = backfill
                .site_edges
                .iter()
                .filter(|edge| edge.kind == EdgeKind::Calls)
                .filter_map(|edge| edge.location.as_ref().map(|loc| loc.file.clone()))
                .collect();
            let backfill_call_hints =
                call_value_ref_hints_from_files(&*store, backfill_call_files, &ext_map)?;
            let (backfill_flow_nodes, backfill_flow_edges) =
                call_value_flow(&backfill.site_edges, &index, &backfill_call_hints)?;
            if !bound.is_empty()
                || !backfill.edges.is_empty()
                || !backfill_flow_nodes.is_empty()
                || !backfill_flow_edges.is_empty()
            {
                store.begin_batch()?;
                store.upsert_nodes(&backfill_flow_nodes)?;
                store.upsert_edges(&backfill.edges)?;
                store.upsert_edges(&backfill_flow_edges)?;
                store.delete_unresolved_refs(&bound)?;
                store.commit_batch()?;
            }
            // Honest telemetry: what got back-filled, what is still waiting.
            eprintln!(
                "BACKFILL: re-resolved {backfill_resolved} previously-parked ref(s) into edges; \
                 {backfill_still_parked} candidate(s) still parked"
            );
        }
    }

    // #229: restore the edges unchanged files owned that the forced re-extraction deleted (see
    // `collateral_edges`), when both endpoints exist again. Before PageRank and the dangling
    // prune, so both see the same graph a full index would.
    if !collateral_edges.is_empty() {
        let mut restore = Vec::with_capacity(collateral_edges.len());
        for edge in collateral_edges {
            if store.get_node(&edge.source)?.is_some() && store.get_node(&edge.target)?.is_some() {
                restore.push(edge);
            }
        }
        if !restore.is_empty() {
            store.begin_batch()?;
            store.upsert_edges(&restore)?;
            store.commit_batch()?;
        }
    }

    // W11.3: populate the pagerank.top cache so `important_symbols` (graph-view, the bench) can
    // serve from cache instead of recomputing. CLI `rank` computes live since #193 — this cache
    // is not refreshed by `scip` or overlay edges. Best-effort: failure is non-fatal.
    const PAGERANK_CACHE_N: usize = 100;
    let pr_result = {
        let reader: &dyn GraphRead = &*store;
        wicked_estate_rank::ranked_symbols(reader, &[], PAGERANK_CACHE_N)
    };
    match pr_result {
        Ok(ranked) => {
            let json_val = serde_json::to_string(&ranked).unwrap_or_default();
            store.cache_put_key("pagerank.top", &json_val);
        }
        Err(e) => {
            eprintln!("pagerank cache population failed (non-fatal): {e}");
        }
    }

    // Task D: prune dangling edges left by incremental symbol removals. After changed files
    // are removed and re-extracted, edges whose target symbol was deleted (but whose edge row
    // was not covered by the file-scoped DELETE) remain in the store. This cheap O(n_edges)
    // pass cleans them up so blast-radius never returns nodes that no longer exist.
    match store.prune_dangling_edges() {
        Ok(n) if n > 0 => {
            eprintln!("GRAPH-CLEANUP: pruned {n} dangling edge(s) after index");
        }
        Ok(_) => {}
        Err(e) => {
            // Non-fatal: a failure to prune is stale data, not corrupt data.
            eprintln!("warning: prune_dangling_edges failed (non-fatal): {e}");
        }
    }

    // Reclaim freelist pages accumulated by PRAGMA auto_vacuum=INCREMENTAL.
    // Non-fatal: a failure here (e.g. read-only DB) should not abort an otherwise successful index.
    if let Err(e) = store.incremental_vacuum() {
        eprintln!("warning: incremental_vacuum failed (non-fatal): {e}");
    }

    // The id-scheme key is written LAST — only after extraction + resolution durably completed —
    // so an interrupted migration re-fires the scheme gate instead of certifying a mixed DB.
    // Replace the pending version/rules markers with current values (#477); files that still
    // hold old output are recorded for individual retries (#476).
    stamp_extractor_markers(
        store,
        &version_key,
        &digest_key,
        &extra_digest,
        &unreadable_key,
        &unreadable_files,
    );
    store.meta_set_key(&scheme_key, SYMBOL_ID_SCHEME);
    // The plugin-override key follows the same write-LAST rule (ADR-010): stamped only once the
    // re-extraction under the current override state durably completed.
    store.meta_set_key(&override_key, cur_overrides);

    let stats = store.stats()?;
    // Warn when the DB crosses 500 MB — a signal to run `compact`.
    if stats.db_size_bytes > 500 * 1_048_576 {
        eprintln!(
            "wicked-estate: db is {:.0}MB — run `wicked-estate compact` to reclaim space",
            stats.db_size_bytes as f64 / 1_048_576.0
        );
    }

    // Emit a span recording the total index duration and file/node counts.
    let sink = wicked_estate_observe::init_sink_from_env();
    let resource = wicked_estate_core::observability::Resource::service(
        "wicked_estate",
        env!("CARGO_PKG_VERSION"),
    );
    let scope = wicked_estate_core::observability::InstrumentationScope::new("wicked_estate.index");
    let elapsed_ns = t.elapsed().as_nanos() as u64;
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let span = wicked_estate_core::observability::SpanData {
        context: wicked_estate_core::observability::SpanContext {
            trace_id: wicked_estate_core::observability::TraceId::INVALID,
            span_id: wicked_estate_core::observability::SpanId::INVALID,
            trace_flags: 0,
            is_remote: false,
        },
        parent_span_id: None,
        name: "wicked_estate.index_path".to_string(),
        kind: wicked_estate_core::observability::SpanKind::Internal,
        start_time_unix_nano: now_ns.saturating_sub(elapsed_ns),
        end_time_unix_nano: now_ns,
        attributes: vec![
            wicked_estate_core::observability::KeyValue::int(
                "wicked_estate.files_changed",
                changed.len() as i64,
            ),
            wicked_estate_core::observability::KeyValue::int(
                "wicked_estate.files_unchanged",
                unchanged_count as i64,
            ),
            wicked_estate_core::observability::KeyValue::int(
                "wicked_estate.nodes",
                stats.node_count as i64,
            ),
            wicked_estate_core::observability::KeyValue::int(
                "wicked_estate.edges",
                stats.edge_count as i64,
            ),
            // Back-fill pass accounting (lane importer-backfill, #141): parked refs this run
            // re-resolved into edges vs candidates that remain parked.
            wicked_estate_core::observability::KeyValue::int(
                "wicked_estate.backfill_resolved",
                backfill_resolved as i64,
            ),
            wicked_estate_core::observability::KeyValue::int(
                "wicked_estate.backfill_still_parked",
                backfill_still_parked as i64,
            ),
        ],
        events: vec![],
        links: vec![],
        status: wicked_estate_core::observability::SpanStatus::ok(),
    };
    // Best-effort: telemetry failure must never abort indexing.
    let _ = sink.export_spans(&resource, &scope, &[span]);

    Ok(stats)
}

/// Set semantic annotations on a symbol — the requirement↔functionality link API. Partial update:
/// each `Some(..)` writes that column, `None` leaves it unchanged.
///
/// `validated_by` names the actor asserting the requirement is satisfied; it is REQUIRED whenever
/// `validated` is given. A claim with no author is what wicked-core#131 was made of — 34,897 nodes
/// self-validated by the agent that wrote them, with nothing recording who decided.
pub fn set_semantics(
    store: &mut dyn GraphStoreMutExt,
    symbol: &str,
    description: Option<&str>,
    requirement: Option<&str>,
    validated: Option<bool>,
    validated_by: Option<&str>,
) -> Result<()> {
    let claim =
        match (validated, validated_by) {
            (Some(v), Some(by)) => Some(
                wicked_estate_core::ValidationClaim::new(v, by)
                    .map_err(|e| wicked_estate_core::Error::Invalid(e.to_string()))?,
            ),
            (Some(_), None) => return Err(wicked_estate_core::Error::Invalid(
                "validating a requirement requires naming the actor asserting it (--validated-by)"
                    .to_string(),
            )),
            (None, _) => None,
        };
    store.set_node_semantics(
        &SymbolId(symbol.to_string()),
        description,
        requirement,
        claim.as_ref(),
    )
}

/// Read a symbol's semantic annotations (description / requirement / validated).
pub fn get_semantics(store: &dyn GraphStoreMutExt, symbol: &str) -> Result<Option<NodeSemantics>> {
    store.node_semantics(&SymbolId(symbol.to_string()))
}

/// All symbols annotated with a requirement — answers "which functionality satisfies R?".
pub fn symbols_for_requirement(
    store: &dyn GraphStoreMutExt,
    requirement: &str,
) -> Result<Vec<Node>> {
    store.find_by_requirement(requirement)
}

/// Find symbols by exact name.
///
/// Synthetic value-flow slots are excluded: a bare NAME never resolves to one. They are named with
/// the bare source identifier, so a local named `map` would otherwise answer every `map` lookup —
/// `blast-radius map` reported dependents of a local instead of "no resolved dependents"
/// (wicked-estate#207 review, C1/C4). They are addressed by exact [`SymbolId`] only (the shape the
/// `Lineage` `flows_to` relation takes), and `SearchEntity`'s `include_values` opt-in surfaces
/// them deliberately.
pub fn search(store: &dyn GraphRead, name: &str) -> Result<Vec<Node>> {
    let q = SymbolQuery {
        exact_name: Some(name.to_string()),
        ..Default::default()
    };
    Ok(store
        .find_symbols(&q)?
        .into_iter()
        .filter(is_structural_symbol)
        .collect())
}

/// The answer [`blast_radius_by_name`] gives: the dependents AND why the answer may be a floor.
///
/// Returning the bare `Vec<Node>` is what made wicked-estate#190 invisible on the CLI — the
/// `Subgraph`'s honesty flags were computed by the store, then dropped on the floor here, so
/// `blast-radius` printed a depth-cut answer as if it were the whole truth. Any caller that only
/// wants the rows can still use `.dependents`; a caller that reports completeness MUST read
/// [`BlastRadius::truncated`].
///
/// `#[non_exhaustive]`: the next incompleteness cause can be added without a breaking release.
/// Outside this crate, build one with `BlastRadius::default()` and assign the fields.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct BlastRadius {
    /// Transitive dependents, deduped across every symbol that matched the name.
    pub dependents: Vec<Node>,
    /// A traversal's `max_nodes` budget dropped at least one reachable dependent.
    pub node_cap_reached: bool,
    /// The `depth` horizon cut the walk short — real dependents exist BEYOND these rows.
    pub depth_horizon_reached: bool,
    /// (#191) Hops from the target to each dependent, keyed by symbol id — the SMALLEST across
    /// the walks when several same-named symbols matched. `1` is a direct dependent. The MCP
    /// `BlastRadius` tool reads the same `Subgraph::depths`; the CLI row carries it from here.
    pub depths: std::collections::BTreeMap<String, u32>,
    /// How much to believe the dependency edges that admitted [`Self::dependents`]
    /// (wicked-estate#194): edges whose source is a returned row and whose target the walk
    /// reached. Structural `Contains`/`Defines` are excluded, so a File row admitted only by
    /// containment adds no evidence. See `blast_radius_by_name`.
    pub confidence: EdgeConfidence,
    /// X1 (DES-S2 §5): the cross-repo crossings the walk traversed (link-owned edges between two
    /// member repos), each with the link's evidence; at most
    /// [`CROSSINGS_CAP`](wicked_estate_core::crossing::CROSSINGS_CAP), shallowest first.
    pub crossings: Vec<wicked_estate_core::crossing::Crossing>,
    /// Every crossing the walk traversed (more than `crossings` lists when capped).
    pub crossings_total: usize,
    /// Dependent symbol id -> the id of the crossing on its predecessor chain.
    pub via_crossing: std::collections::BTreeMap<String, String>,
}

/// Confidence summary over a set of edges: `{min, avg, edge_count}`, the MCP `BlastRadius`
/// envelope's shape. `min`/`avg` are `None` when there are no edges, never a fabricated 1.0.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EdgeConfidence {
    pub min: Option<f32>,
    pub avg: Option<f32>,
    pub edge_count: usize,
}

impl EdgeConfidence {
    fn from_confidences(confs: impl IntoIterator<Item = f32>) -> Self {
        let (mut min, mut sum, mut n) = (f32::INFINITY, 0.0f64, 0usize);
        for c in confs {
            min = min.min(c);
            sum += f64::from(c);
            n += 1;
        }
        if n == 0 {
            return Self::default();
        }
        Self {
            min: Some(min),
            avg: Some((sum / n as f64) as f32),
            edge_count: n,
        }
    }
}

/// Is `kind` evidence that one symbol DEPENDS on another, as opposed to structural membership?
/// `Contains`/`Defines` are parse-certain (confidence 1.0) and say only where a symbol lives;
/// counting them would let a file's containment edges make a name-guess impact set look
/// compiler-verified — the S1 repair finding "summaries counted non-flow `Contains` edges".
fn is_dependency_evidence(kind: &wicked_estate_core::EdgeKind) -> bool {
    use wicked_estate_core::EdgeKind;
    !matches!(kind, EdgeKind::Contains | EdgeKind::Defines)
}

impl BlastRadius {
    /// True if the dependent list is a FLOOR rather than the complete set, for any reason.
    pub fn truncated(&self) -> bool {
        self.node_cap_reached || self.depth_horizon_reached
    }
}

/// Blast radius: transitive dependents (callers) of every symbol named `name`, up to `depth`.
///
/// The traversal walks ALL edge kinds (the locked decision); the RESULT is classified through
/// `Subgraph::code_dependents` so import-transit File nodes never surface as dependents of a
/// symbol, while a File start keeps its importers (lane relative-imports Decision G).
///
/// Returns a [`BlastRadius`], not a bare `Vec<Node>`: the store's incompleteness flags are
/// ORed across every matching symbol's traversal and handed to the caller (wicked-estate#190).
pub fn blast_radius_by_name(store: &dyn GraphRead, name: &str, depth: u32) -> Result<BlastRadius> {
    blast_radius_from(store, search(store, name)?, depth)
}

/// The structural symbol with exactly this [`SymbolId`], or `None` (unknown id, or a synthetic
/// value slot — the same exclusion [`search`] applies to names; wicked-estate#451).
pub fn symbol_by_id(store: &dyn GraphRead, id: &str) -> Result<Option<Node>> {
    Ok(store
        .get_node(&SymbolId(id.to_string()))?
        .filter(is_structural_symbol))
}

/// [`blast_radius_by_name`] for ONE symbol, addressed by exact [`SymbolId`]: same shape, but a
/// name shared by several symbols no longer unions their dependents (wicked-estate#451).
pub fn blast_radius_by_id(store: &dyn GraphRead, id: &str, depth: u32) -> Result<BlastRadius> {
    blast_radius_from(store, symbol_by_id(store, id)?.into_iter().collect(), depth)
}

fn blast_radius_from(store: &dyn GraphRead, starts: Vec<Node>, depth: u32) -> Result<BlastRadius> {
    let mut out = BlastRadius::default();
    let mut crossing_set = wicked_estate_core::crossing::CrossingSet::default();
    let mut seen = std::collections::HashSet::new();
    // Eligible edges, deduped across walks: two same-named symbols can share a caller.
    let mut evidence: HashMap<(SymbolId, SymbolId, wicked_estate_core::EdgeKind), f32> =
        HashMap::new();
    for sym in starts {
        let spec = TraversalSpec::blast_radius(depth);
        let mut sub = store.traverse(&sym.symbol, &spec)?;
        // Judge the horizon on the projected rows, not the raw walk: past the horizon an
        // import-transit File is dropped by code_dependents, so it is not a missing dependent.
        sub.refine_code_dependents_horizon(store, Some(&sym.kind), &spec)?;
        // A cut on ANY matching symbol's walk makes the union a floor.
        out.node_cap_reached |= sub.node_cap_reached;
        out.depth_horizon_reached |= sub.depth_horizon_reached;
        let answer: Vec<&Node> = sub.code_dependents(&sym.symbol, Some(&sym.kind));
        // Admission evidence: in a Dependents walk an edge `row → x` is WHY `row` is in the
        // answer, so count dependency edges whose source is a returned row and whose target is
        // the start or any node this walk reached — including an import-transit File that
        // `code_dependents` dropped as a row (the edge into it still admitted its dependent).
        // Not counted: an edge sourced at a node that is not a row (a dropped transit File, a
        // frontier node past the horizon), and structural `Contains`/`Defines`, so a File row
        // admitted only by containment contributes no evidence.
        let rows: HashSet<&SymbolId> = answer.iter().map(|n| &n.symbol).collect();
        let reached: HashSet<&SymbolId> = sub
            .nodes
            .iter()
            .map(|n| &n.symbol)
            .chain(std::iter::once(&sym.symbol))
            .collect();
        for e in &sub.edges {
            if is_dependency_evidence(&e.kind)
                && rows.contains(&e.source)
                && reached.contains(&e.target)
            {
                evidence
                    .entry((e.source.clone(), e.target.clone(), e.kind.clone()))
                    .or_insert(e.confidence.get());
            }
        }
        for n in answer {
            if let Some(d) = sub.depths.get(n.symbol.as_str()).copied() {
                out.depths
                    .entry(n.symbol.as_str().to_string())
                    .and_modify(|cur| *cur = (*cur).min(d))
                    .or_insert(d);
            }
            if seen.insert(n.symbol.clone()) {
                out.dependents.push(n.clone());
            }
        }
        crossing_set.add_walk(&sub);
    }
    let (crossings, total, via) = crossing_set.finish(wicked_estate_core::crossing::CROSSINGS_CAP);
    out.crossings = crossings;
    out.crossings_total = total;
    out.via_crossing = via;
    out.confidence = EdgeConfidence::from_confidences(evidence.into_values());
    Ok(out)
}

/// Ingest a SCIP index file into the store's **support plane** (TS-S2C) and return the number of
/// distinct public edges its facts now support (several sites can support one edge). See
/// [`ingest_scip_report_as`] for the full report.
///
/// # Prerequisite
/// The project must already be indexed (`index_path`) so that nodes exist to correlate against.
/// Running `ingest_scip` on an empty store projects nothing (no nodes → nothing to correlate).
pub fn ingest_scip(
    store: &mut dyn GraphStoreMutExt,
    root: &Path,
    scip_path: &Path,
) -> Result<usize> {
    ingest_scip_as(store, root, scip_path, None)
}

/// [`ingest_scip`] against one repo of a multi-repo graph; the count of distinct edges supported.
pub fn ingest_scip_as(
    store: &mut dyn GraphStoreMutExt,
    root: &Path,
    scip_path: &Path,
    repo: Option<&str>,
) -> Result<usize> {
    Ok(ingest_scip_report_as(store, root, scip_path, repo)?.edges_projected)
}

/// The support-owner snapshot of a SCIP index: `<repo label or .>:<index path>` — the index path
/// relative to `root` when it lies under it (with `/` separators), else its file name. It names
/// the stable unit a re-run replaces, never the index's contents.
pub fn scip_snapshot(root: &Path, scip_path: &Path, repo: Option<&str>) -> String {
    let rel = std::fs::canonicalize(root)
        .ok()
        .zip(std::fs::canonicalize(scip_path).ok())
        .and_then(|(r, p)| p.strip_prefix(&r).ok().map(Path::to_path_buf))
        .or_else(|| scip_path.strip_prefix(root).ok().map(Path::to_path_buf))
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_else(|| {
            scip_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| scip_path.to_string_lossy().into_owned())
        });
    format!("{}:{rel}", repo.unwrap_or("."))
}

/// Translate a SCIP index (`wicked_estate_resolve::scip_evidence`) and ingest it with
/// [`ingest_semantic_evidence`]. SCIP declares definitions and references only — it has no call
/// role — so this never writes a `Calls` edge. The report folds in what the adapter dropped.
///
/// SCIP documents carry REPO-relative paths, while a labelled repo's nodes carry `<label>/…`; the
/// label is stripped for correlation and re-applied to the written locations (see
/// [`ingest_semantic_evidence`]).
pub fn ingest_scip_report_as(
    store: &mut dyn GraphStoreMutExt,
    root: &Path,
    scip_path: &Path,
    repo: Option<&str>,
) -> Result<wicked_estate_core::evidence::EvidenceReport> {
    // Validate here too, not only in `index_path_as` (Copilot on #117): the label becomes the
    // `<label>/` prefix re-applied to written locations, so an unvalidated one forges paths.
    if let Some(label) = repo {
        repo_scope::validate_label(label)?;
    }
    let bytes = std::fs::read(scip_path).map_err(|e| {
        wicked_estate_core::Error::Io(std::io::Error::other(format!(
            "scip: cannot read {:?}: {e}",
            scip_path
        )))
    })?;
    let translated =
        wicked_estate_resolve::scip_evidence(&bytes, &scip_snapshot(root, scip_path, repo))?;
    let mut report = ingest_semantic_evidence(store, Some(root), &translated.evidence, repo)?;
    for (reason, n) in translated.skipped {
        report.skip(reason, n);
    }
    Ok(report)
}

/// Ingest one [`SemanticEvidence`](wicked_estate_core::evidence::SemanticEvidence) envelope:
/// correlate it against the graph's nodes and make the accepted facts its owner's complete
/// support set (`docs/ENGINE-CONTRACT.md` §3.5). Nothing is written for an invalid envelope or a
/// rejected generation.
///
/// With `repo`, only that repo's nodes (`<label>/…`) are correlated, the documents are read as
/// repo-relative, and written locations carry the label. `root` is the repo checkout, used only to
/// read a document's text when its columns are not UTF-8.
pub fn ingest_semantic_evidence(
    store: &mut dyn GraphStoreMutExt,
    root: Option<&Path>,
    evidence: &wicked_estate_core::evidence::SemanticEvidence,
    repo: Option<&str>,
) -> Result<wicked_estate_core::evidence::EvidenceReport> {
    if let Some(label) = repo {
        repo_scope::validate_label(label)?;
    }
    let prefix = repo.map(repo_scope::prefix);
    let nodes: Vec<Node> = store
        .all_nodes()?
        .into_iter()
        .filter_map(|mut n| match &prefix {
            None => Some(n),
            Some(p) => {
                let rest = n.location.file.strip_prefix(p.as_str())?.to_string();
                n.location.file = rest;
                Some(n)
            }
        })
        .collect();
    // #405: evidence documents are read only inside the repository root.
    let confined = root.and_then(confined::ConfinedRoot::new);
    let read =
        |doc: &str| -> Option<String> { confined.as_ref()?.read_rel_to_string(Path::new(doc)) };
    let mut projection = wicked_estate_core::evidence::project_evidence(evidence, &nodes, &read)?;
    if let Some(p) = &prefix {
        projection.facts = projection
            .facts
            .into_iter()
            .map(|f| {
                let id = f.fact_id.clone();
                let mut edge = f.edge;
                if let Some(loc) = &mut edge.location {
                    loc.file = format!("{p}{}", loc.file);
                }
                wicked_estate_core::SupportFact::new(id, edge)
            })
            .collect::<wicked_estate_core::Result<_>>()?;
    }
    let report = wicked_estate_core::evidence::apply_projection(
        &mut *store,
        projection,
        evidence.generation,
    )?;
    if !report.replayed {
        store.version_bump();
    }
    Ok(report)
}

/// The most important symbols by global PageRank (the "where do I even start" view for a complex
/// codebase). Returns up to `top_n` `(Node, score)` pairs, highest first.
///
/// W11.3: reads from the `pagerank.top` cache when available (populated at `index_path` end).
/// Falls back to live PageRank computation when the cache is absent or stale.
pub fn important_symbols(store: &dyn GraphStoreMutExt, top_n: usize) -> Result<Vec<(Node, f32)>> {
    // Try the cache first (W11.3).
    if let Some(cached) = store.cache_get_key("pagerank.top") {
        if let Ok(ranked) = serde_json::from_str::<Vec<(String, f32)>>(&cached) {
            let mut out = Vec::new();
            for (sym_str, score) in ranked {
                if out.len() >= top_n {
                    break;
                }
                let id = SymbolId(sym_str);
                if let Some(node) = store.get_node(&id)? {
                    // BR-1 (lane relative-imports): a pre-upgrade `pagerank.top` cache on an
                    // un-reindexed DB still carries File/Import rows (the write path filters
                    // them now, but old caches persist until a re-index). Clean them at READ
                    // time so no consumer of important_symbols ever serves one — the
                    // precondition for graph-view dropping its post-hoc exclusion.
                    // TS-S1 joins the same read-time hygiene: a `pagerank.top` cache written by a
                    // 0.17.0 binary can hold synthetic value-flow slots, and that cache outlives
                    // the fix until the DB is re-indexed.
                    if matches!(node.kind, NodeKind::File | NodeKind::Import)
                        || !is_structural_symbol(&node)
                    {
                        continue;
                    }
                    out.push((node, score));
                }
            }
            if !out.is_empty() {
                return Ok(out);
            }
        }
    }
    // Cache miss or empty — compute live.
    let reader: &dyn GraphRead = store;
    let mut out = Vec::new();
    for (symbol, score) in wicked_estate_rank::ranked_symbols(reader, &[], top_n)? {
        if let Some(node) = store.get_node(&symbol)? {
            out.push((node, score));
        }
    }
    Ok(out)
}

// ── New lib fns (W10 drift, W7.4 staleness, Task B tfstate ingest) ─────────────

/// Ingest a Terraform state file (`*.tfstate`) into `store`.
///
/// Parses `tfstate_json` via [`TfstateCollector`], upserts the resulting resource nodes and
/// edges so the LIVE side of the estate is indexed alongside IaC nodes.
///
/// Returns the number of resource nodes upserted.
pub fn ingest_tfstate(store: &mut dyn GraphStoreMutExt, tfstate_json: &str) -> Result<usize> {
    let collector = TfstateCollector::new();
    let extraction = collector.collect(tfstate_json)?;
    let node_count = extraction.nodes.len();
    store.begin_batch()?;
    store.upsert_nodes(&extraction.nodes)?;
    store.upsert_edges(&extraction.local_edges)?;
    store.upsert_unresolved_refs(&extraction.refs)?;
    store.commit_batch()?;
    Ok(node_count)
}

/// W10 drift report: classification of estate resources.
#[derive(Debug)]
pub struct DriftReport {
    /// Resources present in the live state but absent from IaC (unmanaged / shadow resources).
    pub unmanaged: Vec<Node>,
    /// Resources declared in IaC but absent from live state (not deployed / deleted).
    pub undeployed: Vec<Node>,
    /// Resources present in both IaC and live state (managed + deployed).
    pub managed: Vec<Node>,
}

/// Compute a drift report by partitioning all `Other("resource")` nodes by their `origin`
/// metadata field (`"iac"` vs `"live"`).
///
/// A resource is identified by the pair `(type, logical_name)` extracted from the node name
/// (e.g. `"aws_s3_bucket.app"` → type=`"aws_s3_bucket"`, name=`"app"`). Two nodes are
/// considered the same resource when their `(type, name)` identity matches regardless of
/// origin.
///
/// # Origin convention
///
/// - IaC extractors (CloudFormation, Kubernetes, HCL) set `metadata["origin"] = "iac"`.
/// - `TfstateCollector` sets `metadata["origin"] = "live"`.
/// - Nodes without an `origin` key are treated as `"iac"` (conservative assumption).
pub fn estate_drift(store: &dyn GraphRead) -> Result<DriftReport> {
    let all_nodes = store.all_nodes()?;

    // Collect only resource nodes.
    let resource_nodes: Vec<Node> = all_nodes
        .into_iter()
        .filter(|n| matches!(&n.kind, NodeKind::Other(k) if k == "resource"))
        .collect();

    // Partition by origin.
    let mut iac: Vec<Node> = Vec::new();
    let mut live: Vec<Node> = Vec::new();
    for n in &resource_nodes {
        let origin = n
            .metadata
            .get("origin")
            .and_then(|v| v.as_str())
            .unwrap_or("iac");
        if origin == "live" {
            live.push(n.clone());
        } else {
            iac.push(n.clone());
        }
    }

    // Build identity key: normalise the node name to (resource_type, logical_name).
    // For tfstate addresses like "aws_s3_bucket.app[0]" we strip the index suffix.
    fn identity(node: &Node) -> String {
        // Strip array index suffix e.g. "[0]", "[1]" for terraform count resources.
        let name = node.name.trim();
        let name = if let Some(pos) = name.rfind('[') {
            &name[..pos]
        } else {
            name
        };
        name.to_lowercase()
    }

    use std::collections::HashSet;
    let iac_ids: HashSet<String> = iac.iter().map(identity).collect();
    let live_ids: HashSet<String> = live.iter().map(identity).collect();

    let managed_ids: HashSet<String> = iac_ids.intersection(&live_ids).cloned().collect();

    let unmanaged: Vec<Node> = live
        .iter()
        .filter(|n| !managed_ids.contains(&identity(n)))
        .cloned()
        .collect();
    let undeployed: Vec<Node> = iac
        .iter()
        .filter(|n| !managed_ids.contains(&identity(n)))
        .cloned()
        .collect();
    let managed: Vec<Node> = resource_nodes
        .iter()
        .filter(|n| managed_ids.contains(&identity(n)))
        .cloned()
        .collect();

    Ok(DriftReport {
        unmanaged,
        undeployed,
        managed,
    })
}

/// W7.4: Compute how many commits have been made to the repo at `root` since the database
/// file at `db_path` was last written.
///
/// Returns `None` when:
/// - `root` is not a git repository.
/// - `git` is not on `$PATH`.
/// - The database file does not exist (no mtime to compare against).
///
/// Never panics or returns an `Err`; all failures produce `None` (degraded gracefully).
pub fn commits_behind(root: &Path, db_path: &str) -> Option<u64> {
    if db_path == ":memory:" {
        return None;
    }
    // Get the file modification time of the database.
    let db_file = std::path::Path::new(db_path);
    let mtime = db_file.metadata().ok()?.modified().ok()?;

    // Format the mtime as an ISO 8601 string for git --since.
    let since = {
        use std::time::UNIX_EPOCH;
        let secs = mtime.duration_since(UNIX_EPOCH).ok()?.as_secs();
        // git --since accepts "@<unix_timestamp>" format.
        format!("@{secs}")
    };

    // Run: git -C <root> rev-list --count --since=<mtime> HEAD
    let output = std::process::Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "rev-list",
            "--count",
            &format!("--since={since}"),
            "HEAD",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let s = String::from_utf8(output.stdout).ok()?;
    s.trim().parse::<u64>().ok()
}

/// #243: commits in `root` since `baseline` — the commit the graph was indexed at
/// (`repo_commit`, or `repo:<label>:commit` in a co-located graph) — falling back to
/// [`commits_behind`]'s db-mtime measure only when no baseline was recorded (a pre-0.21 graph)
/// or git no longer knows it (rewritten history, a shallow clone deepened past it). The mtime
/// measure is wrong on its own terms: the server's cache write moves the file's mtime, so a
/// graph indexed at X read as "current" after any read that writes. `None` when neither measure
/// can answer (not a git repository, git absent).
pub fn commits_behind_since(root: &Path, baseline: Option<&str>, db_path: &str) -> Option<u64> {
    if let Some(base) = baseline.map(str::trim).filter(|b| !b.is_empty()) {
        let counted = std::process::Command::new("git")
            .args([
                "-C",
                &root.to_string_lossy(),
                "rev-list",
                "--count",
                &format!("{base}..HEAD"),
            ])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u64>().ok());
        if counted.is_some() {
            return counted;
        }
    }
    commits_behind(root, db_path)
}

/// `root` as [`index_path`] records it in `indexed_root` (#248): absolute with symlinks
/// resolved, or as written when it cannot be resolved (a root that is gone is still recorded).
fn canonical_root(root: &Path) -> String {
    // `dunce`: on Windows a drive-letter path (`D:\repo`), not the verbatim `\\?\D:\repo` that
    // `std::fs::canonicalize` returns and that other tools (git, MinGW, a user's shell) mishandle
    // (#360). Identical to `std::fs::canonicalize` everywhere else.
    dunce::canonicalize(root)
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// The root recorded in `indexed_root`, as a path a freshness check can use (#248):
/// [`recorded_root_path`] of the meta value.
pub fn indexed_root_path(store: &dyn GraphStoreMutExt, db_path: &str) -> Option<PathBuf> {
    store
        .meta_get_key("indexed_root")
        .map(|root| recorded_root_path(&root, db_path))
}

/// A recorded root (`indexed_root`, or a registry record's `repo:<label>:root`) as a path a
/// freshness check can use (#248). Roots are written canonical since 0.21.1, so an absolute
/// root is returned as is. A relative one was written by an older binary as the caller spelled
/// it (`index .`), relative to a working directory nobody recorded; the db file's own directory
/// is the best evidence of where that was (`index . --db graph.db`), so the root resolves
/// against it whenever THAT resolution is a directory, and otherwise stays as spelled — which
/// is what the pre-#248 code always did (codex round 2 on #264: a missing db directory must not
/// rewrite the recorded spelling into another path that does not exist).
pub fn recorded_root_path(root: &str, db_path: &str) -> PathBuf {
    let root = PathBuf::from(root);
    if root.is_absolute() {
        return root;
    }
    let base = Path::new(db_path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let from_db = base.join(&root);
    if from_db.is_dir() { from_db } else { root }
}

/// Now, as RFC 3339 UTC — the `indexed_at` meta value.
fn rfc3339_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    rfc3339_utc(secs)
}

/// Unix seconds as RFC 3339 UTC (`2026-10-07T01:52:03Z`), without a date crate: the proleptic
/// Gregorian civil-from-days conversion (H. Hinnant), exact for every non-negative instant.
pub fn rfc3339_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

// ─────────────────────────────────────────────────────────────────────────────
// W12 — Cross-graph / federated query API
// ─────────────────────────────────────────────────────────────────────────────

/// Result of a federated query: `(tagged_nodes, per_db_errors)`.
///
/// Each entry in the first vec is `(repo_db_path, Node)`.  Per-db errors (unopenable files,
/// corrupt DBs) are collected into the second vec rather than propagating as `Err`.
type FedResult = wicked_estate_core::Result<(Vec<(String, wicked_estate_core::Node)>, Vec<String>)>;

/// Open `db_path` as a read-only [`GraphRead`] store via `wicked_estate_store::open_store`.
///
/// Returns an `Err` when the path cannot be opened (missing file, corrupt DB, etc.); the
/// caller is responsible for logging and skipping that DB in a federation fan-out.
fn open_read_store(db_path: &str) -> wicked_estate_core::Result<Box<dyn GraphRead>> {
    wicked_estate_store::open_store(db_path).map(|b| b as Box<dyn GraphRead>)
}

/// **Federated symbol search** across multiple per-repo databases.
///
/// Opens each database in `db_paths`, runs [`search`] on each, and tags every match with
/// the database path it came from (the "repo identifier").  The union is returned as an
/// ordered `Vec<(repo_db, Node)>` — all matches from the first DB first, then the second, etc.
///
/// # Cross-repo matching semantics
///
/// Matching is by **exact simple name** (`SymbolId`'s logical name component).  This means:
///
/// * A symbol named `process_payment` in `repo-a.db` AND in `repo-b.db` will both appear in
///   the result — the caller sees both occurrences tagged by repo.
/// * Where a [`Symbol::Global`] carries [`Package`] coordinates (`manager`, `name`, `version`),
///   callers can further filter to the same package identity.  This function returns the raw
///   nodes so the caller decides whether package-equality is required.
///
/// **Honest limitation**: cross-repo *edges* (calls from repo-A into a symbol in repo-B) are
/// NOT resolved here.  Each repo's graph only has edges within its own indexed codebase.
/// Precise cross-repo edge resolution requires a package-aware import resolver that can look up
/// symbols across repo boundaries — a future step (package-resolver tier).  The federation
/// results are therefore a best-effort **name-union**; treat them as candidate matches, not as
/// a connected cross-repo call graph.
///
/// # Errors
///
/// Per-database errors are collected but do NOT abort the whole fan-out.  A DB that fails to
/// open simply contributes no results; the caller gets a partial result.  The returned
/// `Vec<String>` contains error messages for DBs that could not be queried (empty when all
/// succeeded).
pub fn cross_graph_search(db_paths: &[String], name: &str) -> FedResult {
    let mut results: Vec<(String, wicked_estate_core::Node)> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for db_path in db_paths {
        match open_read_store(db_path) {
            Err(e) => {
                errors.push(format!("{db_path}: failed to open — {e}"));
            }
            Ok(store) => match search(store.as_ref(), name) {
                Err(e) => {
                    errors.push(format!("{db_path}: search error — {e}"));
                }
                Ok(nodes) => {
                    for node in nodes {
                        results.push((db_path.clone(), node));
                    }
                }
            },
        }
    }

    Ok((results, errors))
}

/// (#358) Did-you-mean suggestions for a name that [`cross_graph_search`] matched in NO database:
/// each database ranks its own candidates ([`wicked_estate_retrieve::suggest::suggest`]), and the
/// union is re-ranked by the same order (tier, distance, name, symbol) and cut to `k`. The
/// databases are opened read-only, so no miss is logged. A database that fails to open or rank
/// contributes nothing (its open error is already reported by `cross_graph_search`).
pub fn cross_graph_suggest(
    db_paths: &[String],
    name: &str,
    k: usize,
) -> Vec<(String, wicked_estate_retrieve::suggest::Suggestion)> {
    let mut all: Vec<(String, wicked_estate_retrieve::suggest::Suggestion)> = Vec::new();
    for db_path in db_paths {
        if let Ok(store) = open_read_store(db_path) {
            if let Ok((found, _)) =
                wicked_estate_retrieve::suggest::suggest(store.as_ref(), name, k)
            {
                all.extend(found.into_iter().map(|s| (db_path.clone(), s)));
            }
        }
    }
    all.sort_by(|(ra, a), (rb, b)| {
        (a.reason, a.distance, &a.node.name, &a.node.symbol.0, ra).cmp(&(
            b.reason,
            b.distance,
            &b.node.name,
            &b.node.symbol.0,
            rb,
        ))
    });
    all.truncate(k);
    all
}

/// **Federated blast-radius** across multiple per-repo databases.
///
/// Opens each database in `db_paths`, runs [`blast_radius_by_name`] on each, and tags every
/// dependent node with the database path it came from.  The union is the set of all known
/// dependents of `name` across the org's repos.
///
/// See [`cross_graph_search`] for the cross-repo matching semantics and the honest limitation
/// regarding cross-repo edges.
pub fn cross_graph_blast_radius(
    db_paths: &[String],
    name: &str,
    depth: u32,
) -> Result<FedBlastRadius> {
    let mut out = FedBlastRadius::default();
    let FedBlastRadius {
        dependents: results,
        errors,
        confidence,
    } = &mut out;

    for db_path in db_paths {
        match open_read_store(db_path) {
            Err(e) => {
                errors.push(format!("{db_path}: failed to open — {e}"));
            }
            Ok(store) => match blast_radius_by_name(store.as_ref(), name, depth) {
                Err(e) => {
                    errors.push(format!("{db_path}: blast-radius error — {e}"));
                }
                Ok(br) => {
                    // A per-repo cut must not vanish into the union: surface it in `errors`,
                    // which is the channel this federated path already prints as a warning
                    // (wicked-estate#190). Silently unioning floors reads as a complete answer.
                    if br.depth_horizon_reached {
                        errors.push(format!(
                            "{db_path}: DEPTH-HORIZON — dependents of '{name}' extend beyond \
                             depth {depth}; this repo's rows are a floor (single-repo `blast-radius --depth N` \
                             follows further)"
                        ));
                    }
                    if br.node_cap_reached {
                        errors.push(format!(
                            "{db_path}: NODE-CAP — the traversal node budget dropped reachable \
                             dependents of '{name}'; this repo's rows are a floor"
                        ));
                    }
                    confidence.push((db_path.clone(), br.confidence));
                    for node in br.dependents {
                        results.push((db_path.clone(), node));
                    }
                }
            },
        }
    }

    Ok(out)
}

/// The answer [`cross_graph_blast_radius`] gives.
///
/// `confidence` is kept PER REPO (wicked-estate#194), never merged: each repo was resolved by
/// its own tiers, and one pooled average would let a well-resolved repo hide a repo whose
/// dependents are all name guesses.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct FedBlastRadius {
    /// `(repo_db_path, dependent)`, repo by repo in `db_paths` order.
    pub dependents: Vec<(String, Node)>,
    /// Per-db failures and per-repo DEPTH-HORIZON / NODE-CAP notices.
    pub errors: Vec<String>,
    /// `(repo_db_path, envelope)` for every repo whose walk ran — see
    /// [`BlastRadius::confidence`].
    pub confidence: Vec<(String, EdgeConfidence)>,
}

// ─────────────────────────────────────────────────────────────────────────────
// W5.2 — compute_embeddings (opt-in, separate from index_path)
// ─────────────────────────────────────────────────────────────────────────────

/// The embedder used by `index --embeddings` and `semantic`. With the `fastembed` feature it is
/// the ONNX/BGE [`FastEmbedder`](wicked_estate_retrieve::FastEmbedder) (real semantic vectors, 384-dim);
/// otherwise the lexical [`HashEmbedder`](wicked_estate_retrieve::HashEmbedder). If the feature is on but the
/// model can't load (no network on first run, etc.), it prints a LOUD marker and falls back to the
/// lexical embedder for the whole run — never silently presents lexical results as semantic.
///
/// The same factory is used at index-time and query-time, so the stored and query vectors always
/// share a dimension (384 vs 128 differ across embedders).
pub fn default_embedder() -> Box<dyn Embedder> {
    // Tier 1: FastEmbedder (contextual ONNX/BGE) — highest quality if its feature is on + loads.
    #[cfg(feature = "fastembed")]
    {
        match wicked_estate_retrieve::FastEmbedder::new() {
            Ok(e) => return Box::new(e),
            Err(err) => {
                eprintln!(
                    "EMBED-FALLBACK: fastembed model unavailable ({err}); trying lighter tiers"
                )
            }
        }
    }
    // Tier 2: Model2VecEmbedder (static distilled) — real semantic, light, no ONNX.
    #[cfg(feature = "model2vec")]
    {
        match wicked_estate_retrieve::Model2VecEmbedder::new() {
            Ok(e) => return Box::new(e),
            Err(err) => {
                eprintln!(
                    "EMBED-FALLBACK: model2vec model unavailable ({err}); using lexical HashEmbedder"
                )
            }
        }
    }
    // Tier 3: HashEmbedder — lexical, zero-dep. If a semantic feature was enabled but the model
    // couldn't load, the loud markers above already explained why this is lexical, not semantic.
    Box::new(wicked_estate_retrieve::HashEmbedder::default())
}

/// Compute and store embedding vectors for every node currently in `store`.
///
/// Iterates all nodes, builds a text representation (`name [kind] signature? doc-first-line?`),
/// embeds it with `embedder`, and persists the vector via `store.set_embedding`.  Nodes that
/// already have a stored embedding are silently overwritten (idempotent re-run).
///
/// Returns the count of nodes embedded.
///
/// # Design note
///
/// This is an **inherent-store** call (`SqliteStore`) rather than a trait call because
/// `set_embedding` lives on the concrete type (it is not part of `GraphStoreMutExt`).
/// The function is intentionally **separate** from `index_path` so that `index_path`'s
/// public signature remains unchanged (wicked-estate-bench calls it).  The CLI `index` command
/// invokes this as an optional second step when `--embeddings` is passed.
///
/// # Dim-guard meta (DoD-A6a / §3.1, §3.3)
///
/// After **all** vectors are persisted, this writes `meta["embedder_id"] = embedder.id()` and
/// `meta["embedder_dim"] = embedder.dim()` so the MCP server can refuse to advertise/dispatch
/// SemanticSearch when the store's embedder identity does not match the runtime's. The meta write
/// is performed **LAST, after the loop** for crash-safety: a crash mid-embed leaves a partially
/// re-embedded store whose `meta` still reflects the PRIOR complete state (or `None` on a
/// first-ever embed) — never a half-written identity that would let mixed-dim rows be served.
/// Re-running overwrites both the vectors and the meta atomically-enough for the guard's purpose.
pub fn compute_embeddings(store: &mut SqliteStore, embedder: &dyn Embedder) -> Result<usize> {
    let nodes = GraphRead::all_nodes(store)?;
    let mut count = 0usize;
    for node in &nodes {
        // Build a plain-text representation from the stable, always-present fields.
        // Signature and doc are appended when present so the hash vector captures them.
        let mut text = format!("{} {:?}", node.name, node.kind);
        if let Some(sig) = &node.signature {
            text.push(' ');
            text.push_str(sig);
        }
        if let Some(doc) = &node.doc {
            // First line only — mirrors render_stub in wicked-estate-retrieve.
            if let Some(first) = doc.lines().next() {
                let trimmed = first.trim();
                if !trimmed.is_empty() {
                    text.push(' ');
                    text.push_str(trimmed);
                }
            }
        }
        let vec = embedder.embed(&text);
        store.set_embedding(&node.symbol, &vec)?;
        count += 1;
    }
    // Tag the store with the embedder identity + dim — LAST, after every vector is persisted, so a
    // crash before this point leaves meta at the prior complete state (or None) → fail-closed.
    store.meta_set_key("embedder_id", embedder.id());
    store.meta_set_key("embedder_dim", &embedder.dim().to_string());
    Ok(count)
}

/// Open an async-capable store from a spec string.
///
/// Supported specs:
/// - `sqlite:<path>` or bare `<path>` — `SqlitePool` via deadpool, 8 connections
///
/// Returns a `SqlitePool` which implements `AsyncGraphStore`. Deadpool manages
/// the internal `Arc`, so the pool is cheap to clone if multiple owners are needed.
#[cfg(feature = "serve")]
pub fn open_async_store(spec: &str) -> crate::Result<wicked_estate_store::SqlitePool> {
    let path = if let Some(p) = spec.strip_prefix("sqlite:") {
        p
    } else {
        spec
    };
    wicked_estate_store::open_sqlite_pool(path, 8)
}

/// [`open_async_store`] for a read-only server over an on-disk SQLite `path` (already parsed: no
/// scheme): every pooled connection is opened `SQLITE_OPEN_READONLY`, so the graph file is never
/// written (no WAL switch, no schema DDL) and a graph in a directory this process cannot write still
/// answers (wicked-crew#1497).
#[cfg(feature = "serve")]
pub fn open_async_store_readonly(path: &str) -> crate::Result<wicked_estate_store::SqlitePool> {
    wicked_estate_store::open_sqlite_pool_readonly(path, 8)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::{
        Annotation, Change, Direction, EdgeSupport, GraphWrite, HistoricalEdge, Language, Location,
        Node, NodeKind, Span, StoreCapabilities, Subgraph, SupportFact, SupportOwner,
        SupportOwnerState, SupportReplacement, SymbolId, UnresolvedRef, ValidationClaim,
    };
    use wicked_estate_store::{GraphStoreMutExt, MemStore};

    // A downstream store compiled against the pre-#438 write surface.
    struct UnsupportedSemanticsStore(MemStore);

    impl GraphRead for UnsupportedSemanticsStore {
        fn capabilities(&self) -> StoreCapabilities {
            GraphRead::capabilities(&self.0)
        }

        fn get_node(&self, id: &SymbolId) -> Result<Option<Node>> {
            GraphRead::get_node(&self.0, id)
        }

        fn find_symbols(&self, query: &SymbolQuery) -> Result<Vec<Node>> {
            GraphRead::find_symbols(&self.0, query)
        }

        fn count_symbols(&self, query: &SymbolQuery) -> Result<usize> {
            GraphRead::count_symbols(&self.0, query)
        }

        fn find_structural_symbols(&self, query: &SymbolQuery) -> Result<Vec<Node>> {
            GraphRead::find_structural_symbols(&self.0, query)
        }

        fn neighbors(&self, id: &SymbolId, dir: Direction) -> Result<Vec<Edge>> {
            GraphRead::neighbors(&self.0, id, dir)
        }

        fn find_symbols_by_name_prefix(
            &self,
            prefix: &str,
            limit: Option<usize>,
        ) -> Result<Vec<Node>> {
            GraphRead::find_symbols_by_name_prefix(&self.0, prefix, limit)
        }

        fn traverse(&self, start: &SymbolId, spec: &TraversalSpec) -> Result<Subgraph> {
            GraphRead::traverse(&self.0, start, spec)
        }

        fn traverse_multi(&self, starts: &[SymbolId], spec: &TraversalSpec) -> Result<Subgraph> {
            GraphRead::traverse_multi(&self.0, starts, spec)
        }

        fn all_nodes(&self) -> Result<Vec<Node>> {
            GraphRead::all_nodes(&self.0)
        }

        fn all_edges(&self) -> Result<Vec<Edge>> {
            GraphRead::all_edges(&self.0)
        }

        fn unresolved_refs_for_name(&self, name: &str) -> Result<Vec<UnresolvedRef>> {
            GraphRead::unresolved_refs_for_name(&self.0, name)
        }

        fn file_digest(&self, file: &str) -> Result<Option<String>> {
            GraphRead::file_digest(&self.0, file)
        }

        fn indexed_files(&self) -> Result<Vec<String>> {
            GraphRead::indexed_files(&self.0)
        }

        fn file_git_sha(&self, file: &str) -> Result<Option<String>> {
            GraphRead::file_git_sha(&self.0, file)
        }

        fn repo_info(&self) -> Result<Option<RepoInfo>> {
            GraphRead::repo_info(&self.0)
        }

        fn edge_history(&self, file: &str) -> Result<Vec<HistoricalEdge>> {
            GraphRead::edge_history(&self.0, file)
        }

        fn file_content(&self, file: &str) -> Result<Option<String>> {
            GraphRead::file_content(&self.0, file)
        }

        fn file_call_refs(&self, file: &str) -> Result<Option<String>> {
            GraphRead::file_call_refs(&self.0, file)
        }

        fn symbol_source(&self, node: &Node) -> Result<Option<String>> {
            GraphRead::symbol_source(&self.0, node)
        }

        fn changes_since(&self, cursor: u64) -> Result<Vec<Change>> {
            GraphRead::changes_since(&self.0, cursor)
        }

        fn node_semantics(&self, symbol: &SymbolId) -> Result<Option<NodeSemantics>> {
            GraphRead::node_semantics(&self.0, symbol)
        }

        fn semantics_in_files(
            &self,
            files: &[&str],
        ) -> Result<Vec<(String, SymbolId, NodeSemantics)>> {
            GraphRead::semantics_in_files(&self.0, files)
        }

        fn find_by_requirement(&self, requirement: &str) -> Result<Vec<Node>> {
            GraphRead::find_by_requirement(&self.0, requirement)
        }

        fn annotations(&self, symbol: &SymbolId) -> Result<Vec<Annotation>> {
            GraphRead::annotations(&self.0, symbol)
        }

        fn annotations_by_type(&self, ty: &str) -> Result<Vec<(SymbolId, Annotation)>> {
            GraphRead::annotations_by_type(&self.0, ty)
        }

        fn annotations_stale_since(&self, cutoff: i64) -> Result<Vec<(SymbolId, Annotation)>> {
            GraphRead::annotations_stale_since(&self.0, cutoff)
        }

        fn symbol_epoch(&self, id: &SymbolId) -> Result<Option<u64>> {
            GraphRead::symbol_epoch(&self.0, id)
        }

        fn edge_supports(
            &self,
            source: &SymbolId,
            target: &SymbolId,
            kind: &EdgeKind,
        ) -> Result<Vec<EdgeSupport>> {
            GraphRead::edge_supports(&self.0, source, target, kind)
        }

        fn support_generation(&self, owner: &SupportOwner) -> Result<Option<u64>> {
            GraphRead::support_generation(&self.0, owner)
        }

        fn support_owners(&self) -> Result<Vec<SupportOwnerState>> {
            GraphRead::support_owners(&self.0)
        }

        fn stats(&self) -> Result<GraphStats> {
            GraphRead::stats(&self.0)
        }
    }

    impl GraphWrite for UnsupportedSemanticsStore {
        fn begin_batch(&mut self) -> Result<()> {
            GraphWrite::begin_batch(&mut self.0)
        }

        fn commit_batch(&mut self) -> Result<()> {
            GraphWrite::commit_batch(&mut self.0)
        }

        fn upsert_nodes(&mut self, nodes: &[Node]) -> Result<()> {
            GraphWrite::upsert_nodes(&mut self.0, nodes)
        }

        fn upsert_edges(&mut self, edges: &[Edge]) -> Result<()> {
            GraphWrite::upsert_edges(&mut self.0, edges)
        }

        fn upsert_unresolved_refs(&mut self, refs: &[UnresolvedRef]) -> Result<()> {
            GraphWrite::upsert_unresolved_refs(&mut self.0, refs)
        }

        fn record_search_miss(
            &mut self,
            query: &str,
            searched_at: i64,
            session_id: Option<&str>,
        ) -> Result<()> {
            GraphWrite::record_search_miss(&mut self.0, query, searched_at, session_id)
        }

        fn remove_file(&mut self, file: &str) -> Result<()> {
            GraphWrite::remove_file(&mut self.0, file)
        }

        fn set_file_digest(&mut self, file: &str, digest: &str) -> Result<()> {
            GraphWrite::set_file_digest(&mut self.0, file, digest)
        }

        fn set_repo_info(&mut self, info: &RepoInfo) -> Result<()> {
            GraphWrite::set_repo_info(&mut self.0, info)
        }

        fn set_file_content(&mut self, file: &str, text: &str) -> Result<()> {
            GraphWrite::set_file_content(&mut self.0, file, text)
        }

        fn set_file_call_refs(&mut self, file: &str, refs_json: &str) -> Result<()> {
            GraphWrite::set_file_call_refs(&mut self.0, file, refs_json)
        }

        fn prune_dangling_edges(&mut self) -> Result<usize> {
            GraphWrite::prune_dangling_edges(&mut self.0)
        }

        fn log_change(&mut self, op: ChangeOp, target: &str) -> Result<()> {
            GraphWrite::log_change(&mut self.0, op, target)
        }

        fn set_node_semantics(
            &mut self,
            symbol: &SymbolId,
            description: Option<&str>,
            requirement: Option<&str>,
            validation: Option<&ValidationClaim>,
        ) -> Result<()> {
            GraphWrite::set_node_semantics(
                &mut self.0,
                symbol,
                description,
                requirement,
                validation,
            )
        }

        fn annotate(&mut self, symbol: &SymbolId, annotation: Annotation) -> Result<()> {
            GraphWrite::annotate(&mut self.0, symbol, annotation)
        }

        fn delete_annotations(
            &mut self,
            symbol: &SymbolId,
            ty: Option<&str>,
            key: &str,
        ) -> Result<usize> {
            GraphWrite::delete_annotations(&mut self.0, symbol, ty, key)
        }

        fn replace_edge_supports(
            &mut self,
            owner: &SupportOwner,
            generation: u64,
            facts: &[SupportFact],
        ) -> Result<SupportReplacement> {
            GraphWrite::replace_edge_supports(&mut self.0, owner, generation, facts)
        }
    }

    impl GraphStoreMutExt for UnsupportedSemanticsStore {
        fn version_bump(&mut self) {
            GraphStoreMutExt::version_bump(&mut self.0)
        }

        fn meta_set_key(&mut self, key: &str, value: &str) {
            GraphStoreMutExt::meta_set_key(&mut self.0, key, value)
        }

        fn meta_get_key(&self, key: &str) -> Option<String> {
            GraphStoreMutExt::meta_get_key(&self.0, key)
        }

        fn cache_put_key(&mut self, key: &str, value: &str) {
            GraphStoreMutExt::cache_put_key(&mut self.0, key, value)
        }

        fn cache_get_key(&self, key: &str) -> Option<String> {
            GraphStoreMutExt::cache_get_key(&self.0, key)
        }

        fn upsert_nodes_skip_fts(&mut self, nodes: &[Node]) -> Result<()> {
            GraphStoreMutExt::upsert_nodes_skip_fts(&mut self.0, nodes)
        }

        fn bulk_rebuild_fts_for_files(&mut self, files: &[&str]) -> Result<()> {
            GraphStoreMutExt::bulk_rebuild_fts_for_files(&mut self.0, files)
        }

        fn parked_relative_import_refs(&self) -> Result<Vec<UnresolvedRef>> {
            GraphStoreMutExt::parked_relative_import_refs(&self.0)
        }

        fn parked_import_refs(&self, file_prefix: &str) -> Result<Vec<(String, String)>> {
            GraphStoreMutExt::parked_import_refs(&self.0, file_prefix)
        }

        fn delete_unresolved_refs(&mut self, refs: &[UnresolvedRef]) -> Result<usize> {
            GraphStoreMutExt::delete_unresolved_refs(&mut self.0, refs)
        }
    }

    /// #460: unsupported stores report one warning and each lost symbol without failing re-index.
    #[test]
    fn unsupported_semantics_store_reindex_reports_and_clears_pending() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("a.rs");
        std::fs::write(&file, "pub fn pay() {}\n").unwrap();
        let mut store = UnsupportedSemanticsStore(MemStore::new());
        index_path(&mut store, tmp.path()).unwrap();
        let pay = store
            .find_symbols(&SymbolQuery {
                exact_name: Some("pay".into()),
                ..Default::default()
            })
            .unwrap()[0]
            .symbol
            .clone();
        store
            .set_node_semantics(
                &pay,
                Some("pays"),
                Some("REQ-460"),
                Some(&ValidationClaim::new(true, "alice").unwrap()),
            )
            .unwrap();
        assert!(store.node_semantics(&pay).unwrap().is_some());

        SEMANTICS_UNSUPPORTED.with(|notes| notes.borrow_mut().clear());
        SEMANTICS_DROPS.with(|notes| notes.borrow_mut().clear());
        std::fs::write(&file, "pub fn pay() { let _amount = 1; }\n").unwrap();
        index_path(&mut store, tmp.path()).unwrap();

        assert!(store.get_node(&pay).unwrap().is_some());
        assert!(store.node_semantics(&pay).unwrap().is_none());
        SEMANTICS_UNSUPPORTED.with(|notes| {
            assert_eq!(
                &*notes.borrow(),
                &[
                    "SEMANTICS-UNSUPPORTED: this store cannot restore semantic fields; symbols in \
                   re-extracted files lose their description, requirement and validation"
                ]
            );
        });
        SEMANTICS_DROPS.with(|notes| {
            assert_eq!(
                &*notes.borrow(),
                &["SEMANTICS-DROPPED: 1 symbol(s) in a.rs"]
            );
        });
        assert_eq!(
            store.meta_get_key(PENDING_SEMANTICS_KEY).as_deref(),
            Some("")
        );
    }

    #[test]
    fn vb6_source_requires_a_version_header_on_the_first_nonblank_line() {
        // Red on main: the content discriminator does not exist; .ctl is extension-only.
        for bytes in [
            &b"VERSION 5.00\r\nBegin VB.UserControl Gauge\r\n"[..],
            &b"\xef\xbb\xbfVERSION 5.00\n"[..],
            &b"\r\n \t\r\nVERSION 5.00\r\n"[..],
            &b"version 5.00\n"[..],
            &b"\xef\xbb\xbf\r\n\t\r\nversion 5.00\r\n"[..],
        ] {
            assert!(is_vb6_source(bytes), "{bytes:?}");
        }
        for bytes in [
            &b"   REPRO INFILE(TRANFILE) OUTFILE(TRANVSAM)\n"[..],
            &b""[..],
            &b" \t\r\n"[..],
            &b"//STEP05 EXEC PGM=IDCAMS\n"[..],
            &b"REPRO\nVERSION 5.00\n"[..],
            &b"VERSIONED 5.00\n"[..],
            &b" VERSION 5.00\n"[..],
        ] {
            assert!(!is_vb6_source(bytes), "{bytes:?}");
        }
    }

    #[test]
    fn not_indexed_reasons_describe_export_formats() {
        // Red on main: no reason helper or NOT-INDEXED reporting exists.
        for (ext, reason) in [
            ("ddl", "Db2 DDL: no DDL extractor yet (wicked-estate#447)"),
            ("db2", "Db2 DDL: no DDL extractor yet (wicked-estate#447)"),
            (
                "csd",
                "CICS CSD resource definitions: no extractor yet (wicked-estate#447)",
            ),
            (
                "mac",
                "HLASM macro library member: no macro extractor yet (wicked-estate#447)",
            ),
            (
                "ca7",
                "scheduler definition: no extractor yet (wicked-estate#447)",
            ),
            (
                "controlm",
                "scheduler definition: no extractor yet (wicked-estate#447)",
            ),
            (
                "ctl",
                "mainframe control card (not VB6): no extractor yet (wicked-estate#447)",
            ),
            ("unknown", "no extractor for this file type"),
            ("", "no extractor for this file type"),
        ] {
            assert_eq!(not_indexed_reason(ext), reason, ".{ext}");
        }
    }

    #[test]
    fn not_indexed_records_namespaced_control_cards_secrets_and_unknown_files() {
        // Red on main: these exclusions are silent (and the control card is admitted as VB6).
        let tmp = tempfile::tempdir().unwrap();
        for (file, text) in [
            ("REPROCT.ctl", "   REPRO INFILE(A) OUTFILE(B)\n"),
            ("credentials.yaml", "token: synthetic\n"),
            ("member.unknown", "synthetic export\n"),
            ("LICENSE", "synthetic license\n"),
        ] {
            std::fs::write(tmp.path().join(file), text).unwrap();
        }
        assert!(wicked_estate_extract::redact::is_excluded(
            "credentials.yaml"
        ));
        let mut store = MemStore::new();
        for _ in 0..2 {
            NOT_INDEXED.with(|notes| notes.borrow_mut().clear());
            index_path_as(&mut store, tmp.path(), Some("export")).unwrap();
            let mut notes = NOT_INDEXED.with(|notes| notes.borrow().clone());
            notes.sort();
            assert_eq!(
                notes,
                vec![
                    (
                        "export/LICENSE".to_string(),
                        "no extractor for this file type".to_string(),
                    ),
                    (
                        "export/REPROCT.ctl".to_string(),
                        "mainframe control card (not VB6): no extractor yet (wicked-estate#447)"
                            .to_string(),
                    ),
                    (
                        "export/credentials.yaml".to_string(),
                        "matches the secret-file rule; never indexed".to_string(),
                    ),
                    (
                        "export/member.unknown".to_string(),
                        "no extractor for this file type".to_string(),
                    ),
                ]
            );
        }
        assert!(GraphRead::indexed_files(&store).unwrap().is_empty());
    }

    #[test]
    fn new_unreadable_files_are_reported_without_duplicating_old_output_notes() {
        // Red on main: the work-list read failure silently drops a new supported file.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("old.rs"), "pub fn old() {}\n").unwrap();
        let mut store = MemStore::new();
        index_path_as(&mut store, tmp.path(), Some("export")).unwrap();
        std::fs::write(tmp.path().join("new.rs"), "pub fn new() {}\n").unwrap();
        // Force the existing UNREADABLE retry branch for the previously indexed path.
        store.meta_set_key(
            &repo_scope::meta_key(Some("export"), "indexed_version"),
            "0.0.0-older",
        );
        let _old =
            crate::semantics_survive_reindex_tests::ReadFailGuard::new(tmp.path().join("old.rs"));
        let _new =
            crate::semantics_survive_reindex_tests::ReadFailGuard::new(tmp.path().join("new.rs"));
        NOT_INDEXED.with(|notes| notes.borrow_mut().clear());
        UNREADABLE_FILES.with(|notes| notes.borrow_mut().clear());
        index_path_as(&mut store, tmp.path(), Some("export")).unwrap();
        NOT_INDEXED.with(|notes| {
            assert_eq!(
                &*notes.borrow(),
                &[(
                    "export/new.rs".to_string(),
                    "could not be read inside the repository root".to_string(),
                )]
            );
        });
        UNREADABLE_FILES.with(|notes| {
            assert_eq!(
                &*notes.borrow(),
                &[("export/old.rs".to_string(), "read failed".to_string())]
            );
        });
    }

    #[test]
    fn a_previously_indexed_mainframe_control_card_is_swept() {
        // Red on main: extension-only admission keeps the old VB6 digest and File node.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("REPROCT.ctl"),
            "   REPRO INFILE(A) OUTFILE(B)\n",
        )
        .unwrap();
        let mut store = MemStore::new();
        ensure_redacted(&mut store).unwrap();
        store.set_file_digest("REPROCT.ctl", "old").unwrap();
        store
            .upsert_nodes(&[Node::new(
                Symbol::file("REPROCT.ctl").id(),
                NodeKind::File,
                "REPROCT.ctl".to_string(),
                Language::new("vb6"),
                Location::new("REPROCT.ctl", Span::ZERO),
            )])
            .unwrap();
        index_path(&mut store, tmp.path()).unwrap();
        assert!(GraphRead::indexed_files(&store).unwrap().is_empty());
        assert!(GraphRead::all_nodes(&store).unwrap().is_empty());
    }

    // #422: keep chunk overrides on the indexing thread, isolated from concurrent tests.
    fn index_with_chunk_limits(
        store: &mut dyn GraphStoreMutExt,
        root: &Path,
        limits: Option<(usize, usize)>,
    ) -> usize {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                TEST_EXTRACT_CHUNK_LIMITS.with(|v| v.set(None));
            }
        }
        let _reset = Reset;
        TEST_EXTRACT_CHUNK_LIMITS.with(|v| v.set(limits));
        TEST_MAX_EXTRACTIONS.with(|v| v.set(0));
        index_path(store, root).unwrap();
        TEST_MAX_EXTRACTIONS.with(|v| v.get())
    }

    /// #515: a separate writer must make progress during extraction and before final writes.
    /// With the old outer batch, remove_file takes the write lock before the first probe.
    #[test]
    fn chunked_index_releases_write_lock_during_extraction_and_resolution() {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                TEST_INDEX_WRITE_HOOK.with(|hook| *hook.borrow_mut() = None);
            }
        }
        let _reset = Reset;
        let src = tempfile::tempdir().unwrap();
        let total = 5;
        for i in 0..total {
            std::fs::write(
                src.path().join(format!("file{i}.rs")),
                format!("pub fn function{i}() {{}}\n"),
            )
            .unwrap();
        }
        let dbdir = tempfile::tempdir().unwrap();
        let dbpath = dbdir.path().join("graph.db");
        let mut store = SqliteStore::open(&dbpath).unwrap();
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let hook_calls = calls.clone();
        TEST_INDEX_WRITE_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                // SqliteStore's public constructor fixes its timeout at 30 s. Use its SQLite
                // meta write directly so a held lock fails this probe after only 200 ms.
                let side = rusqlite::Connection::open(&dbpath).unwrap();
                side.busy_timeout(std::time::Duration::from_millis(200))
                    .unwrap();
                side.execute(
                    "INSERT OR REPLACE INTO meta(k, v) VALUES('test.write_probe', 'ok')",
                    [],
                )
                .expect("another writer must succeed outside the short write batches");
                hook_calls.set(hook_calls.get() + 1);
            }));
        });

        index_with_chunk_limits(&mut store, src.path(), Some((usize::MAX, 1)));
        assert_eq!(
            calls.get(),
            total + 1,
            "one probe per chunk plus final write"
        );
        assert_eq!(store.indexed_files().unwrap().len(), total);

        // Exercise replacement plus deletion: the deletion batch must commit before extraction.
        let remaining = total - 1;
        std::fs::remove_file(src.path().join(format!("file{remaining}.rs"))).unwrap();
        for i in 0..remaining {
            std::fs::write(
                src.path().join(format!("file{i}.rs")),
                format!("pub fn function{i}() {{ let changed = {i}; }}\n"),
            )
            .unwrap();
        }
        calls.set(0);
        index_with_chunk_limits(&mut store, src.path(), Some((usize::MAX, 1)));
        assert_eq!(
            calls.get(),
            remaining + 1,
            "every replacement chunk must probe"
        );
        assert_eq!(store.indexed_files().unwrap().len(), remaining);
    }

    fn chunk_graph_snapshot(
        store: &wicked_estate_store::SqliteStore,
        db: &Path,
    ) -> serde_json::Value {
        fn sorted<T: serde::Serialize>(rows: Vec<T>) -> Vec<String> {
            let mut rows: Vec<String> = rows
                .iter()
                .map(|r| serde_json::to_string(r).unwrap())
                .collect();
            rows.sort();
            rows
        }
        // Enumerate every parked name, rather than assuming which refs the fixture leaves.
        let conn = rusqlite::Connection::open(db).unwrap();
        let mut stmt = conn
            .prepare("SELECT DISTINCT raw_name FROM unresolved_refs ORDER BY raw_name")
            .unwrap();
        let names: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        let refs: Vec<_> = names
            .iter()
            .flat_map(|name| store.unresolved_refs_for_name(name).unwrap())
            .collect();
        assert!(!refs.is_empty(), "fixture must exercise parked refs");
        let mut files = store.indexed_files().unwrap();
        files.sort();
        let content: Vec<_> = files
            .iter()
            .map(|file| {
                let text = store.file_content(file).unwrap().unwrap();
                (file, text)
            })
            .collect();
        let fts: Vec<_> = ["normalize", "consume", "rust_helper", "py_helper"]
            .iter()
            .map(|name| {
                let hits = store
                    .find_symbols(&wicked_estate_core::SymbolQuery {
                        text: Some((*name).to_string()),
                        ..Default::default()
                    })
                    .unwrap();
                assert!(!hits.is_empty(), "fixture must have FTS hits for {name}");
                (name, sorted(hits))
            })
            .collect();
        serde_json::json!({
            "nodes": sorted(GraphRead::all_nodes(store).unwrap()),
            "edges": sorted(store.all_edges().unwrap()),
            "refs": sorted(refs),
            "content": content,
            "fts": fts,
        })
    }

    #[test]
    fn chunked_index_matches_default_graph() {
        let src = tempfile::tempdir().unwrap();
        for (file, text) in [
            (
                "a.ts",
                "export function consume() { return normalize(missingInput); }\n",
            ),
            (
                "b.ts",
                "export function normalize(value: string) { return value; }\n",
            ),
            ("c.rs", "pub fn rust_helper() {}\n"),
            ("d.py", "def py_helper():\n    return missing_python()\n"),
        ] {
            std::fs::write(src.path().join(file), text).unwrap();
        }
        let dbdir = tempfile::tempdir().unwrap();
        let default_db = dbdir.path().join("default.db");
        let chunked_db = dbdir.path().join("chunked.db");
        let mut default = wicked_estate_store::SqliteStore::open(&default_db).unwrap();
        let mut chunked = wicked_estate_store::SqliteStore::open(&chunked_db).unwrap();
        assert_eq!(index_with_chunk_limits(&mut default, src.path(), None), 4);
        assert_eq!(
            index_with_chunk_limits(&mut chunked, src.path(), Some((usize::MAX, 1))),
            1
        );
        let nodes = GraphRead::all_nodes(&default).unwrap();
        assert!(
            default.all_edges().unwrap().iter().any(|edge| {
                edge.kind == EdgeKind::Calls
                    && nodes
                        .iter()
                        .any(|n| n.symbol == edge.source && n.location.file == "a.ts")
                    && nodes
                        .iter()
                        .any(|n| n.symbol == edge.target && n.location.file == "b.ts")
            }),
            "fixture must resolve a call across files"
        );
        assert_eq!(
            chunk_graph_snapshot(&chunked, &chunked_db),
            chunk_graph_snapshot(&default, &default_db)
        );

        // Exercise the lightweight leftovers too: rederive an unchanged TS caller's flows and
        // back-fill an unchanged Python caller when its missing target appears in another chunk.
        std::fs::write(
            src.path().join("b.ts"),
            "export function normalize(value: string) { const result = value; return result; }\n",
        )
        .unwrap();
        std::fs::write(
            src.path().join("e.py"),
            "def missing_python():\n    return never_bound()\n",
        )
        .unwrap();
        index_with_chunk_limits(&mut default, src.path(), None);
        index_with_chunk_limits(&mut chunked, src.path(), Some((usize::MAX, 1)));
        assert!(
            chunked
                .unresolved_refs_for_name("missing_python")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            chunk_graph_snapshot(&chunked, &chunked_db),
            chunk_graph_snapshot(&default, &default_db)
        );
    }

    #[test]
    fn chunked_index_bounds_live_extractions() {
        let src = tempfile::tempdir().unwrap();
        let text = "pub fn chunk_fixture() {}\n";
        let total = 9;
        for i in 0..total {
            std::fs::write(src.path().join(format!("file{i}.rs")), text).unwrap();
        }
        // Byte budget, oversized singleton progress, and file-count cap, respectively.
        for (bytes, files, expected) in [(text.len() * 2, 512, 2), (1, 512, 1), (usize::MAX, 3, 3)]
        {
            let mut store = wicked_estate_store::SqliteStore::in_memory().unwrap();
            let max = index_with_chunk_limits(&mut store, src.path(), Some((bytes, files)));
            assert!(
                max > 0,
                "the live-extraction counter must observe extraction"
            );
            assert!(
                max <= expected,
                "held {max} extractions; chunk allows {expected}"
            );
            assert!(max < total, "must not retain every file's extraction");
            assert_eq!(store.indexed_files().unwrap().len(), total);
        }
    }

    fn resource_node(sym: &str, name: &str, origin: &str) -> Node {
        let mut meta = serde_json::Map::new();
        meta.insert(
            "origin".to_string(),
            serde_json::Value::String(origin.to_string()),
        );
        let mut n = Node::new(
            SymbolId(sym.to_string()),
            NodeKind::Other("resource".to_string()),
            name,
            Language::new("tfstate"),
            Location::new(name, Span::ZERO),
        );
        n.metadata = meta;
        n
    }

    /// Admissibility F-A (§11 sibling class): the resolver pass's family table must carry the
    /// IaC logical languages (own-name families, from the single-source
    /// `wicked_estate_extract::LOGICAL_LANGUAGES`) and the json manifest row, while the
    /// deliberately family-None languages (tfstate, mainframe — D5/F7 "must keep resolving")
    /// stay absent.
    #[test]
    fn resolver_family_table_carries_iac_logical_languages_and_json() {
        use wicked_estate_core::SymbolIndex;
        let store = MemStore::new();
        let index = InMemoryIndex::build(&store, None).unwrap();
        for name in wicked_estate_extract::LOGICAL_LANGUAGES {
            assert_eq!(
                index.language_family(name).as_deref(),
                Some(*name),
                "logical language {name} must be its own family"
            );
        }
        assert_eq!(
            index.language_family("json").as_deref(),
            Some("json"),
            "json must carry its own-name family (manifest row)"
        );
        for name in ["tfstate", "cobol", "jcl", "hlasm", "synthetic"] {
            assert_eq!(
                index.language_family(name),
                None,
                "{name} must stay family-None (D5/F7: must keep resolving)"
            );
        }
    }

    fn call_flow_index() -> (InMemoryIndex, SymbolId, SymbolId, SymbolId, Span, String) {
        let caller = SymbolId("caller".to_string());
        let callee = SymbolId("callee".to_string());
        let other = SymbolId("other_callee".to_string());
        let mut callee_node = Node::new(
            callee.clone(),
            NodeKind::Function,
            "normalize",
            Language::new("typescript"),
            Location::new("callee.ts", Span::ZERO),
        );
        callee_node
            .metadata
            .insert("value_params".to_string(), serde_json::json!(["id"]));
        let mut other_node = Node::new(
            other.clone(),
            NodeKind::Function,
            "normalize",
            Language::new("typescript"),
            Location::new("other.ts", Span::ZERO),
        );
        other_node
            .metadata
            .insert("value_params".to_string(), serde_json::json!(["id"]));
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[
                Node::new(
                    caller.clone(),
                    NodeKind::Function,
                    "load",
                    Language::new("typescript"),
                    Location::new("caller.ts", Span::ZERO),
                ),
                callee_node,
                other_node,
                // The callee returns a value (`return <ident>`), so it has a return endpoint and a
                // `call_result` hop joins to it (#210: no endpoint, no hop).
                Node::new(
                    value_symbol(&callee, "return", "value"),
                    NodeKind::Synthetic,
                    "normalize.return",
                    Language::new("typescript"),
                    Location::new("callee.ts", Span::ZERO),
                )
                .with_value_role("Return"),
            ])
            .unwrap();
        store.commit_batch().unwrap();
        let index = InMemoryIndex::build(&store, None).unwrap();
        (
            index,
            caller,
            callee,
            other,
            Span {
                start_byte: 10,
                end_byte: 30,
                start_line: 1,
                start_col: 4,
                end_line: 1,
                end_col: 24,
            },
            "caller.ts".to_string(),
        )
    }

    fn call_flow_hints(
        caller: &SymbolId,
        file: &str,
        span: Span,
    ) -> HashMap<CallSiteKey, serde_json::Value> {
        HashMap::from([(
            call_site_key(&Location::new(file, span)),
            serde_json::json!({
                "args": [{
                    "name": "raw",
                    "kind": "local",
                    "symbol": value_symbol(caller, "local", "raw").0,
                    "slot": 0
                }],
                "result": {
                    "name": "out",
                    "kind": "local",
                    "symbol": value_symbol(caller, "local", "out").0
                }
            }),
        )])
    }

    #[test]
    fn value_flow_hint_endpoint_kinds_round_trip_stable_vocabulary() {
        let hint = serde_json::json!({
            "args": [
                {"name": "raw", "kind": "local", "slot": 0},
                {"name": "fieldValue", "kind": "field", "slot": 1},
                {"name": "id", "kind": "parameter", "slot": 2}
            ],
            "result": {"name": "out", "kind": "local"}
        });

        let args = value_flow_endpoints(&hint, "args").unwrap();
        let kinds: Vec<_> = args.iter().map(|endpoint| endpoint.kind).collect();
        assert_eq!(
            kinds,
            vec![
                ValueFlowHintKind::Local,
                ValueFlowHintKind::Field,
                ValueFlowHintKind::Parameter
            ]
        );
        assert_eq!(
            value_flow_result(&hint).unwrap().unwrap().kind,
            ValueFlowHintKind::Local
        );
    }

    #[test]
    fn value_flow_hint_rejects_unsupported_endpoint_kind() {
        let args_hint = serde_json::json!({
            "args": [{"name": "raw", "kind": "Local", "slot": 0}]
        });
        assert_eq!(
            value_flow_endpoints(&args_hint, "args").unwrap_err(),
            ValueFlowHintError {
                key: "args".to_string(),
                kind: "Local".to_string()
            }
        );

        let result_hint = serde_json::json!({
            "result": {"name": "out", "kind": "RouteParam"}
        });
        assert_eq!(
            value_flow_result(&result_hint).unwrap_err(),
            ValueFlowHintError {
                key: "result".to_string(),
                kind: "RouteParam".to_string()
            }
        );
    }

    #[test]
    fn call_value_flow_returns_error_for_unsupported_hint_kind() {
        let (index, caller, callee, _, span, file) = call_flow_index();
        let edge = Edge::new(
            caller.clone(),
            callee,
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test",
        )
        .with_location(Location::new(&file, span));
        let hints = HashMap::from([(
            call_site_key(&Location::new(&file, span)),
            serde_json::json!({
                "args": [{
                    "name": "raw",
                    "kind": "Local",
                    "symbol": value_symbol(&caller, "local", "raw").0,
                    "slot": 0
                }]
            }),
        )]);

        let error = call_value_flow(&[edge], &index, &hints).unwrap_err();
        let wicked_estate_core::Error::Invalid(message) = error else {
            panic!("expected invalid hint error, got {error:?}");
        };
        assert!(
            message.contains("caller.ts:1:10-30")
                && message.contains("key=args")
                && message.contains("kind=Local"),
            "invalid hint error should include site/key/kind context, got {message:?}"
        );
    }

    #[test]
    fn call_value_flow_allows_duplicate_same_target_at_one_site() {
        let (index, caller, callee, _, span, file) = call_flow_index();
        let edge = Edge::new(
            caller.clone(),
            callee.clone(),
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test",
        )
        .with_location(Location::new(&file, span));
        let hints = call_flow_hints(&caller, &file, span);

        let (_, flows) = call_value_flow(&[edge.clone(), edge], &index, &hints).unwrap();
        let pairs: std::collections::BTreeSet<_> = flows
            .into_iter()
            .map(|edge| (edge.target, edge.source))
            .collect();

        assert!(pairs.contains(&(
            value_symbol(&caller, "local", "raw"),
            value_symbol(&callee, "param", "id")
        )));
        assert!(pairs.contains(&(
            value_symbol(&callee, "return", "value"),
            value_symbol(&caller, "local", "out")
        )));
    }

    #[test]
    fn call_value_flow_suppresses_one_site_with_two_unique_targets() {
        let (index, caller, callee, other, span, file) = call_flow_index();
        let first = Edge::new(
            caller.clone(),
            callee,
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test",
        )
        .with_location(Location::new(&file, span));
        let second = Edge::new(
            caller.clone(),
            other,
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test",
        )
        .with_location(Location::new(&file, span));
        let hints = call_flow_hints(&caller, &file, span);

        let (_, flows) = call_value_flow(&[first, second], &index, &hints).unwrap();

        assert!(
            flows.is_empty(),
            "ambiguous exact-site call must not emit argument or return value flow"
        );
    }

    // ── Task C: estate_drift ─────────────────────────────────────────────────

    #[test]
    fn drift_empty_store_produces_empty_report() {
        let store = MemStore::new();
        let report = estate_drift(&store).unwrap();
        assert!(report.unmanaged.is_empty());
        assert!(report.undeployed.is_empty());
        assert!(report.managed.is_empty());
    }

    #[test]
    fn drift_live_only_is_unmanaged() {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[resource_node("live1", "aws_s3_bucket.logs", "live")])
            .unwrap();
        store.commit_batch().unwrap();

        let report = estate_drift(&store).unwrap();
        assert_eq!(
            report.unmanaged.len(),
            1,
            "live-only node must be unmanaged"
        );
        assert!(report.undeployed.is_empty(), "no iac-only nodes");
        assert!(report.managed.is_empty(), "no managed nodes");
    }

    #[test]
    fn drift_iac_only_is_undeployed() {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[resource_node("iac1", "aws_s3_bucket.logs", "iac")])
            .unwrap();
        store.commit_batch().unwrap();

        let report = estate_drift(&store).unwrap();
        assert_eq!(
            report.undeployed.len(),
            1,
            "iac-only node must be undeployed"
        );
        assert!(report.unmanaged.is_empty(), "no live-only nodes");
        assert!(report.managed.is_empty(), "no managed nodes");
    }

    #[test]
    fn drift_matching_iac_and_live_is_managed() {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[
                resource_node("iac1", "aws_s3_bucket.app", "iac"),
                resource_node("live1", "aws_s3_bucket.app", "live"),
            ])
            .unwrap();
        store.commit_batch().unwrap();

        let report = estate_drift(&store).unwrap();
        assert!(report.unmanaged.is_empty(), "no unmanaged");
        assert!(report.undeployed.is_empty(), "no undeployed");
        assert!(!report.managed.is_empty(), "both iac and live → managed");
    }

    #[test]
    fn drift_mixed_report() {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[
                // managed pair
                resource_node("iac_app", "aws_s3_bucket.app", "iac"),
                resource_node("live_app", "aws_s3_bucket.app", "live"),
                // iac-only (undeployed)
                resource_node("iac_new", "aws_sqs_queue.jobs", "iac"),
                // live-only (unmanaged)
                resource_node("live_shadow", "aws_instance.old", "live"),
            ])
            .unwrap();
        store.commit_batch().unwrap();

        let report = estate_drift(&store).unwrap();
        assert_eq!(report.undeployed.len(), 1, "one undeployed");
        assert_eq!(report.unmanaged.len(), 1, "one unmanaged");
        assert_eq!(
            report.managed.len(),
            2,
            "two managed (both iac+live for app)"
        );
    }

    // ── Task B: ingest_tfstate ───────────────────────────────────────────────

    #[test]
    fn ingest_tfstate_round_trips_resource_nodes() {
        // Minimal valid tfstate JSON with one resource.
        let tfstate_json = r#"{
            "version": 4,
            "terraform_version": "1.5.0",
            "resources": [
                {
                    "mode": "managed",
                    "type": "aws_s3_bucket",
                    "name": "test_bucket",
                    "provider": "provider[\"registry.terraform.io/hashicorp/aws\"]",
                    "instances": [
                        {
                            "attributes": {
                                "id": "my-bucket-id",
                                "bucket": "my-bucket-id"
                            }
                        }
                    ]
                }
            ]
        }"#;

        let mut store = MemStore::new();
        let n = ingest_tfstate(&mut store, tfstate_json).unwrap();
        assert!(
            n > 0,
            "ingest_tfstate must upsert at least one resource node"
        );

        // At least one resource node should exist.
        let nodes = wicked_estate_core::GraphRead::all_nodes(&store).unwrap();
        let resources: Vec<_> = nodes
            .iter()
            .filter(|n| matches!(&n.kind, NodeKind::Other(k) if k == "resource"))
            .collect();
        assert!(
            !resources.is_empty(),
            "at least one resource node must be in the store"
        );
    }

    // ── Task E: analytics cache ──────────────────────────────────────────────

    #[test]
    fn index_path_populates_pagerank_cache() {
        // Use a temp dir with a tiny Rust file so index_path has something to extract.
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("lib.rs");
        std::fs::write(&src, "pub fn hello() {}\npub fn world() { hello(); }\n").unwrap();

        let mut store = MemStore::new();
        index_path(&mut store, tmp.path()).unwrap();

        // After indexing, the pagerank.top cache entry should be populated.
        let cached = store.cache_get_key("pagerank.top");
        assert!(
            cached.is_some(),
            "pagerank.top cache must be set after index_path"
        );
        let json_str = cached.unwrap();
        // Should be a valid JSON array.
        let parsed: Vec<(String, f32)> = serde_json::from_str(&json_str)
            .expect("pagerank.top must be a JSON array of (symbol, score) pairs");
        // With two functions, we should have at least one ranked result.
        assert!(
            !parsed.is_empty(),
            "ranked list must not be empty for a non-empty codebase"
        );
    }

    #[test]
    fn important_symbols_reads_from_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("lib.rs");
        std::fs::write(
            &src,
            "pub fn a() {}\npub fn b() { a(); }\npub fn c() { b(); }\n",
        )
        .unwrap();

        let mut store = MemStore::new();
        index_path(&mut store, tmp.path()).unwrap();

        // important_symbols should return results (from cache or live compute).
        let top = important_symbols(&store, 10).unwrap();
        assert!(
            !top.is_empty(),
            "important_symbols must return at least one result"
        );
    }

    /// Lane relative-imports S5 (Decision H): after a real index, important_symbols never
    /// returns a File or Import node — the ranked seam filters live results and the cache
    /// write is already filtered.
    #[test]
    fn important_symbols_has_no_file_or_import_nodes() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("a.ts"),
            "import { f } from './b';\nexport function g() { return f(); }\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("b.ts"),
            "export function f() { return 1; }\n",
        )
        .unwrap();

        let mut store = MemStore::new();
        index_path(&mut store, tmp.path()).unwrap();

        let top = important_symbols(&store, 50).unwrap();
        assert!(!top.is_empty(), "ranked results expected");
        for (node, _) in &top {
            assert!(
                !matches!(node.kind, NodeKind::File | NodeKind::Import),
                "File/Import must never rank as a hotspot: {:?} {}",
                node.kind,
                node.symbol.as_str()
            );
        }
    }

    /// Lane relative-imports S5 (BR-1): a STALE pre-upgrade `pagerank.top` cache containing
    /// File/Import ids is cleaned at READ time — the ids are never returned and `top_n` is
    /// still filled from the remaining rows. This is the precondition for graph-view dropping
    /// its post-hoc exclusion.
    #[test]
    fn important_symbols_drops_file_import_from_stale_cache() {
        use wicked_estate_core::GraphWrite;
        let mut store = MemStore::new();
        let file_node = Node::new(
            wicked_estate_core::Symbol::file("a.ts").id(),
            NodeKind::File,
            "a.ts",
            Language::new("typescript"),
            Location::new("a.ts", Span::ZERO),
        );
        let import_node = Node::new(
            SymbolId("import:./hub".into()),
            NodeKind::Import,
            "./hub",
            Language::new("typescript"),
            Location::new("a.ts", Span::ZERO),
        );
        let fn_x = Node::new(
            SymbolId("fn_x".into()),
            NodeKind::Function,
            "fn_x",
            Language::new("typescript"),
            Location::new("a.ts", Span::ZERO),
        );
        let fn_y = Node::new(
            SymbolId("fn_y".into()),
            NodeKind::Function,
            "fn_y",
            Language::new("typescript"),
            Location::new("a.ts", Span::ZERO),
        );
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[file_node.clone(), import_node.clone(), fn_x, fn_y])
            .unwrap();
        store.commit_batch().unwrap();

        // Hand-seed the cache the way a PRE-upgrade binary wrote it: File/Import rows on top.
        let stale = serde_json::to_string(&vec![
            (file_node.symbol.0.clone(), 0.9_f32),
            (import_node.symbol.0.clone(), 0.8_f32),
            ("fn_x".to_string(), 0.5_f32),
            ("fn_y".to_string(), 0.4_f32),
        ])
        .unwrap();
        store.cache_put_key("pagerank.top", &stale);

        let top = important_symbols(&store, 2).unwrap();
        let ids: Vec<&str> = top.iter().map(|(n, _)| n.symbol.as_str()).collect();
        assert_eq!(
            ids,
            vec!["fn_x", "fn_y"],
            "stale File/Import cache rows skipped, top_n filled from the remaining rows"
        );
    }

    // ── W7.4: commits_behind returns None gracefully for non-git dir ─────────

    #[test]
    fn commits_behind_non_git_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        // Not a git repo + db doesn't exist = None.
        let result = commits_behind(tmp.path(), "/nonexistent/graph.db");
        assert!(
            result.is_none(),
            "commits_behind must return None for non-git dir"
        );
    }

    /// A git repo with one committed TypeScript file at a scratch path. Returns the root and
    /// HEAD's full SHA.
    fn committed_repo(tag: &str) -> (std::path::PathBuf, String) {
        let root = std::env::temp_dir().join(format!(
            "wicked-estate-freshness-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/index.ts"),
            format!("export function {tag}() {{ return 1; }}\n"),
        )
        .unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "t@example.invalid"]);
        git(&root, &["config", "user.name", "wicked-test"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-qm", "base"]);
        let head = git(&root, &["rev-parse", "HEAD"]);
        (root, head)
    }

    fn git(root: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn commit_one(root: &Path, name: &str) {
        std::fs::write(root.join(format!("src/{name}.ts")), "export const x = 1;\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-qm", name]);
    }

    /// #243: freshness is measured from the commit the graph was indexed at, not the db file's
    /// mtime. The server's own cache write moves the mtime, so the mtime measure answered
    /// "current" for a graph that was a commit behind; the baseline measure does not. With no
    /// baseline (a pre-0.21 graph) or one git does not know, the mtime measure is the fallback.
    #[test]
    fn commits_behind_uses_meta_repo_commit_not_mtime() {
        let (root, base) = committed_repo("baseline");
        let db = root.join("graph.db");
        std::fs::write(&db, b"not a real graph; only its mtime matters here").unwrap();
        commit_one(&root, "after_index");
        // The write that moves the mtime past the new commit — what a cache write does.
        let future = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&db)
            .unwrap()
            .set_modified(future)
            .unwrap();
        assert_eq!(
            commits_behind(&root, db.to_str().unwrap()),
            Some(0),
            "the mtime measure is blind to the commit once the file is touched"
        );
        assert_eq!(
            commits_behind_since(&root, Some(&base), db.to_str().unwrap()),
            Some(1),
            "the baseline measure counts the commit regardless of the file's mtime"
        );
        assert_eq!(
            commits_behind_since(&root, None, db.to_str().unwrap()),
            Some(0),
            "no recorded baseline: the mtime measure is the fallback"
        );
        assert_eq!(
            commits_behind_since(
                &root,
                Some("0123456789abcdef0123456789abcdef01234567"),
                db.to_str().unwrap()
            ),
            Some(0),
            "a baseline git does not know: the mtime measure is the fallback"
        );
        assert_eq!(
            commits_behind_since(&root, Some(&base), ":memory:"),
            Some(1),
            "the baseline needs no db file at all"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #245: a db indexed from two repos reports the WORST repo's staleness, each repo measured
    /// from its own recorded commit — not whichever root happened to be indexed last. #351: the
    /// measure lives in `freshness`, and a probe that outlives a commit sees it on the next call.
    #[test]
    fn graph_commits_behind_two_repos_returns_the_worst() {
        let (a, _) = committed_repo("two_a");
        let (b, _) = committed_repo("two_b");
        let dir =
            std::env::temp_dir().join(format!("wicked-estate-two-repos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("shared.db");
        let mut store = wicked_estate_store::SqliteStore::open(&db).unwrap();
        index_path_as(&mut store, &a, Some("a")).unwrap();
        index_path_as(&mut store, &b, Some("b")).unwrap();
        let db_str = db.to_str().unwrap();
        let worst =
            |s: &wicked_estate_store::SqliteStore| freshness::staleness_report(s, db_str).worst;
        assert_eq!(worst(&store), Some(0));
        // A long-lived probe (the MCP server's): measured per call, not at startup.
        let mut probe = freshness::FreshnessProbe::open(db_str);
        let behind = |probe: &mut freshness::FreshnessProbe| {
            let mut content = serde_json::json!({});
            probe.annotate(&mut content, &mut vec![]);
            content["freshness"]["commits_behind"].clone()
        };
        assert_eq!(behind(&mut probe), serde_json::json!(0));
        commit_one(&a, "a1");
        commit_one(&b, "b1");
        commit_one(&b, "b2");
        // `b` (indexed LAST, so the old single-root read would have seen only it) is 2 behind;
        // `a` is 1 behind; the graph is as stale as its stalest repo.
        assert_eq!(worst(&store), Some(2));
        assert_eq!(
            behind(&mut probe),
            serde_json::json!(2),
            "no restart needed"
        );
        // Re-index `b` alone: it is current, `a` is still 1 behind — the worst is now `a`'s.
        index_path_as(&mut store, &b, Some("b")).unwrap();
        assert_eq!(worst(&store), Some(1));
        assert_eq!(behind(&mut probe), serde_json::json!(1));

        // Drift is attributed to the repo the path belongs to: an uncommitted edit in `a`
        // marks `a/src/index.ts` — and only rows naming it.
        std::fs::write(
            a.join("src/index.ts"),
            "export function two_a() { return 2; }\n",
        )
        .unwrap();
        let mut content = serde_json::json!({"rows": [
            {"file": "a/src/index.ts"}, {"file": "b/src/index.ts"}
        ]});
        let mut diags = vec![];
        probe.annotate(&mut content, &mut diags);
        assert_eq!(
            content["rows"][0]["stale"],
            serde_json::json!("modified"),
            "{content}"
        );
        assert!(content["rows"][1].get("stale").is_none(), "{content}");
        assert_eq!(
            content["freshness"]["stale_files"],
            serde_json::json!([{"file": "a/src/index.ts", "status": "modified"}])
        );
        let roots = content["freshness"]["roots"].as_array().unwrap();
        assert_eq!(roots.len(), 2, "per-root counts: {content}");
        // A new, never-indexed source file in `b` is counted; a `.txt` is not.
        std::fs::write(b.join("src/new.ts"), "export const y = 2;\n").unwrap();
        std::fs::write(b.join("notes.txt"), "not source\n").unwrap();
        let mut content = serde_json::json!({});
        let mut diags = vec![];
        probe.annotate(&mut content, &mut diags);
        assert_eq!(
            content["freshness"]["unindexed_files"],
            serde_json::json!(1),
            "{content}"
        );
        assert!(
            diags
                .iter()
                .any(|d| d.contains("1 new source file(s) not in the graph")),
            "{diags:?}"
        );
        for d in [&a, &b, &dir] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// #248 on Windows (#360): `index <relative path>` records an ABSOLUTE root with a DRIVE
    /// LETTER (`D:\…`) — the canonical form a user, git and the freshness check can all read —
    /// never the relative spelling and never the verbatim `\\?\D:\…` form
    /// `std::fs::canonicalize` returns on Windows. The repo sits under the current directory, so
    /// the relative spelling is on the same drive.
    #[cfg(windows)]
    #[test]
    fn indexed_root_is_an_absolute_drive_letter_path_after_index_relative() {
        let cwd = std::env::current_dir().unwrap();
        let name = format!("relroot-win-{}", std::process::id());
        let root = cwd.join(&name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/index.ts"),
            "export function w() { return 1; }\n",
        )
        .unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "t@example.invalid"]);
        git(&root, &["config", "user.name", "wicked-test"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-qm", "base"]);
        let rel = PathBuf::from(&name);
        assert!(rel.is_relative() && rel.is_dir(), "{}", rel.display());
        let mut store = MemStore::new();
        index_path(&mut store, &rel).unwrap();
        let recorded = store.meta_get_key("indexed_root").unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            !recorded.starts_with(r"\\?\"),
            "the recorded root is not a verbatim path: {recorded}"
        );
        let bytes = recorded.as_bytes();
        assert!(
            bytes.len() > 3 && bytes[0].is_ascii_alphabetic() && &bytes[1..3] == br":\",
            "the recorded root starts with a drive letter: {recorded}"
        );
        assert!(Path::new(&recorded).is_absolute());
        assert!(recorded.ends_with(&name), "{recorded}");
        assert_eq!(
            indexed_root_path(&store, ":memory:").unwrap(),
            PathBuf::from(&recorded)
        );
    }

    /// #248: `index <relative path>` records the ABSOLUTE, symlink-resolved root (and when it
    /// was indexed), so a freshness check from any other working directory finds the repo. A
    /// pre-#248 relative root that is not visible from the current directory resolves against
    /// the db file's own directory.
    #[cfg(unix)]
    #[test]
    fn indexed_root_is_absolute_after_index_dot() {
        let (root, _) = committed_repo("relroot");
        // The repo spelled RELATIVE to the current directory, the way `index .` or
        // `index ../repo` is typed.
        let cwd = std::env::current_dir().unwrap();
        let ups = cwd.components().count() - 1;
        let rel = PathBuf::from("../".repeat(ups)).join(root.strip_prefix("/").unwrap());
        assert!(rel.is_relative() && rel.is_dir(), "{}", rel.display());
        let mut store = MemStore::new();
        index_path(&mut store, &rel).unwrap();
        let recorded = store.meta_get_key("indexed_root").unwrap();
        assert_eq!(
            PathBuf::from(&recorded),
            std::fs::canonicalize(&root).unwrap(),
            "the recorded root is canonical, not the relative spelling"
        );
        assert!(Path::new(&recorded).is_absolute());
        assert_eq!(
            indexed_root_path(&store, ":memory:").unwrap(),
            PathBuf::from(&recorded)
        );
        let at = store.meta_get_key("indexed_at").unwrap();
        assert!(
            at.len() == 20 && at.ends_with('Z') && &at[4..5] == "-" && &at[10..11] == "T",
            "indexed_at is RFC 3339 UTC: {at}"
        );
        // Legacy relative roots (written by an older binary as spelled). `"."` — always a
        // directory from ANY cwd — resolves against the db file's directory when that is a
        // directory (codex round 1 on #264), never the caller's cwd; one that resolves to no
        // directory there stays as spelled (codex round 2: a missing db directory must not
        // rewrite it into another missing path); one visible only from the cwd stays as spelled
        // (the pre-#248 behaviour).
        store.meta_set_key("indexed_root", ".");
        assert_eq!(
            indexed_root_path(&store, root.join("graph.db").to_str().unwrap()).unwrap(),
            root.join("."),
            "a legacy '.' is the db's directory, not the cwd"
        );
        store.meta_set_key("indexed_root", "no-such-dir-for-this-test");
        assert_eq!(
            indexed_root_path(&store, "/graphs/acme/graph.db").unwrap(),
            PathBuf::from("no-such-dir-for-this-test"),
            "nothing to resolve against: the recorded spelling is kept"
        );
        store.meta_set_key("indexed_root", rel.to_str().unwrap());
        assert_eq!(
            indexed_root_path(&store, "/graphs/acme/graph.db").unwrap(),
            rel,
            "visible only from the cwd: kept as spelled"
        );
        // A labelled record is written canonical too (codex round 1 on #264).
        let mut labelled = MemStore::new();
        index_path_as(&mut labelled, &rel, Some("rel")).unwrap();
        let rec = repo_scope::registry(&labelled)
            .into_iter()
            .find(|r| r.label == "rel")
            .expect("the record");
        assert_eq!(
            PathBuf::from(&rec.root),
            std::fs::canonicalize(&root).unwrap()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn indexed_at_rfc3339_matches_known_instants() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339_utc(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(rfc3339_utc(4_102_444_799), "2099-12-31T23:59:59Z");
    }

    // ── Task A: InfraResolver in resolver slice ──────────────────────────────

    #[test]
    fn index_path_uses_infra_resolver_for_resource_refs() {
        use wicked_estate_core::{EdgeKind, GraphWrite, NodeKind, UnresolvedRef};
        // Build a store with a resource node and an unresolved ref to it.
        // index_path is not called here (that needs a real dir); we verify the production
        // resolve path (resolve_all_with_coverage) with InfraResolver by calling it directly.
        let resource_name = "aws_instance.web";
        let resource_sym = wicked_estate_core::Symbol::synthetic("tfstate", resource_name).id();
        let resource_node = Node::new(
            resource_sym.clone(),
            NodeKind::Other("resource".to_string()),
            resource_name,
            Language::new("tfstate"),
            Location::new(resource_name, Span::ZERO),
        );

        let eip_sym = wicked_estate_core::Symbol::synthetic("tfstate", "aws_eip.ip").id();
        let eip_node = Node::new(
            eip_sym.clone(),
            NodeKind::Other("resource".to_string()),
            "aws_eip.ip",
            Language::new("tfstate"),
            Location::new("aws_eip.ip", Span::ZERO),
        );

        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store.upsert_nodes(&[resource_node, eip_node]).unwrap();
        store.commit_batch().unwrap();

        let index = InMemoryIndex::build(&store, None).unwrap();
        let refs = vec![UnresolvedRef::new(
            eip_sym,
            resource_name,
            EdgeKind::Other("depends_on".to_string()),
            Location::new("aws_eip.ip", Span::ZERO),
        )];
        let resolvers: &[&dyn Resolver] = &[
            &NameResolver,
            &ScopedNameResolver,
            &ImportMapResolver,
            &InfraResolver,
        ];
        let resolution = resolve_all_with_coverage(resolvers, &refs, &index).unwrap();
        assert_eq!(
            resolution.edges.len(),
            1,
            "InfraResolver must resolve the depends_on ref"
        );
        assert_eq!(resolution.edges[0].target, resource_sym);
        assert!(
            resolution.unresolved.is_empty(),
            "the bound depends_on ref must not be counted unresolved"
        );
    }

    /// The index slice composition resolves `rules-engine:*` bridge refs: a RuleSet node in the
    /// index + a bridge ref through the SAME resolver set `index_path` uses (built explicitly
    /// here, not by reproducing the anchored slice literal) yields an InvokedBy edge from
    /// `RulesBridgeResolver`. Before the wiring, those refs were never produced under `index`.
    #[test]
    fn index_slice_resolves_rules_engine_bridge_refs() {
        use wicked_estate_core::{EdgeKind, GraphWrite, NodeKind, UnresolvedRef};

        let ruleset_sym = wicked_estate_core::Symbol::synthetic("drl", "pricing").id();
        let ruleset_node = Node::new(
            ruleset_sym.clone(),
            NodeKind::RuleSet,
            "com.example.pricing",
            Language::new("drl"),
            Location::new("rules/pricing.drl", Span::ZERO),
        );
        let caller_sym = wicked_estate_core::Symbol::file("src/PricingService.java").id();

        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store.upsert_nodes(&[ruleset_node]).unwrap();
        store.commit_batch().unwrap();

        let index = InMemoryIndex::build(&store, None).unwrap();
        let refs = vec![UnresolvedRef::new(
            caller_sym.clone(),
            "rules-engine:ibm-odm",
            EdgeKind::InvokedBy,
            Location::new("src/PricingService.java", Span::ZERO),
        )];
        let resolvers: &[&dyn Resolver] = &[
            &NameResolver,
            &ScopedNameResolver,
            &ImportMapResolver,
            &InfraResolver,
            &RulesBridgeResolver,
        ];
        let edges = resolve_all_with_coverage(resolvers, &refs, &index)
            .unwrap()
            .edges;
        assert_eq!(edges.len(), 1, "the bridge ref must resolve");
        assert_eq!(edges[0].source, caller_sym);
        assert_eq!(edges[0].target, ruleset_sym);
        assert_eq!(edges[0].kind, EdgeKind::InvokedBy);
        assert_eq!(edges[0].resolved_by, "rules-bridge-resolver");
        assert!((edges[0].confidence.get() - 0.5).abs() < 1e-6);
    }

    // ── Task A: SKIPPED_MINIFIED notice ──────────────────────────────────────

    #[test]
    fn is_minified_or_huge_detects_long_lines() {
        // Verify the public API matches expectations used in index_path.
        let normal = "fn foo() {}\nfn bar() {}\n";
        assert!(
            !is_minified_or_huge(normal),
            "normal code must not be flagged"
        );

        // A >50,000-char single line is considered minified/huge.
        let long_line: String = "a".repeat(50_001);
        assert!(
            is_minified_or_huge(&long_line),
            ">50000-char line must be flagged"
        );
    }

    // ── W12 federation helpers ────────────────────────────────────────────────

    /// Build a SqliteStore at `path` with one symbol named `sym_name` + an optional caller.
    fn build_repo_db(
        path: &str,
        sym_id: &str,
        sym_name: &str,
        caller_id: Option<&str>,
        caller_name: Option<&str>,
    ) {
        use wicked_estate_core::{
            Edge, EdgeKind, GraphWrite, Language, Location, ResolutionTier, Span,
        };
        use wicked_estate_store::SqliteStore;

        let mut store = SqliteStore::open(path).expect("open temp store");
        store.begin_batch().unwrap();

        let sym = Node::new(
            SymbolId(sym_id.to_string()),
            NodeKind::Function,
            sym_name,
            Language::new("rust"),
            Location::new("src/lib.rs", Span::ZERO),
        );
        store.upsert_nodes(&[sym]).unwrap();

        if let (Some(cid), Some(cname)) = (caller_id, caller_name) {
            let caller = Node::new(
                SymbolId(cid.to_string()),
                NodeKind::Function,
                cname,
                Language::new("rust"),
                Location::new("src/lib.rs", Span::ZERO),
            );
            let edge = Edge::new(
                SymbolId(cid.to_string()),
                SymbolId(sym_id.to_string()),
                EdgeKind::Calls,
                ResolutionTier::Parsed,
                "test",
            );
            store.upsert_nodes(&[caller]).unwrap();
            store.upsert_edges(&[edge]).unwrap();
        }

        store.commit_batch().unwrap();
    }

    #[test]
    fn cross_graph_search_finds_symbol_in_both_repos() {
        let tmp = tempfile::tempdir().unwrap();
        let db_a = tmp.path().join("repo_a.db").to_string_lossy().to_string();
        let db_b = tmp.path().join("repo_b.db").to_string_lossy().to_string();

        // Both repos have a symbol named "shared_fn".
        build_repo_db(&db_a, "sym_a", "shared_fn", None, None);
        build_repo_db(&db_b, "sym_b", "shared_fn", None, None);

        let (results, errors) = cross_graph_search(&[db_a.clone(), db_b.clone()], "shared_fn")
            .expect("federation search must succeed");

        assert!(errors.is_empty(), "no DB errors expected: {errors:?}");
        assert_eq!(results.len(), 2, "one match per repo = 2 total");

        // Both repo paths are represented.
        let repos: Vec<&str> = results.iter().map(|(r, _)| r.as_str()).collect();
        assert!(repos.contains(&db_a.as_str()), "repo_a in results");
        assert!(repos.contains(&db_b.as_str()), "repo_b in results");

        // Both matches have the expected name.
        assert!(
            results.iter().all(|(_, n)| n.name == "shared_fn"),
            "all matches must be named 'shared_fn'"
        );
    }

    #[test]
    fn cross_graph_blast_radius_unions_dependents_across_repos() {
        let tmp = tempfile::tempdir().unwrap();
        let db_a = tmp.path().join("br_a.db").to_string_lossy().to_string();
        let db_b = tmp.path().join("br_b.db").to_string_lossy().to_string();

        // repo_a: "target_fn" called by "caller_a_fn"
        build_repo_db(
            &db_a,
            "target_a",
            "target_fn",
            Some("caller_a"),
            Some("caller_a_fn"),
        );
        // repo_b: "target_fn" also exists, called by "caller_b_fn"
        build_repo_db(
            &db_b,
            "target_b",
            "target_fn",
            Some("caller_b"),
            Some("caller_b_fn"),
        );

        let fed = cross_graph_blast_radius(&[db_a.clone(), db_b.clone()], "target_fn", 8)
            .expect("federation blast-radius must succeed");
        let (results, errors) = (&fed.dependents, &fed.errors);

        assert!(errors.is_empty(), "no DB errors expected: {errors:?}");
        // Each repo contributes its own caller to the union.
        assert_eq!(results.len(), 2, "one dependent per repo = 2 total");

        let caller_names: Vec<&str> = results.iter().map(|(_, n)| n.name.as_str()).collect();
        assert!(
            caller_names.contains(&"caller_a_fn"),
            "caller from repo_a must appear"
        );
        assert!(
            caller_names.contains(&"caller_b_fn"),
            "caller from repo_b must appear"
        );
        // wicked-estate#194: one envelope per repo, each over that repo's own admission edge.
        let repos: Vec<&str> = fed.confidence.iter().map(|(r, _)| r.as_str()).collect();
        assert_eq!(repos, [db_a.as_str(), db_b.as_str()]);
        for (repo, c) in &fed.confidence {
            assert_eq!(c.edge_count, 1, "{repo}: {c:?}");
        }
    }

    #[test]
    fn cross_graph_search_nonexistent_symbol_returns_empty_not_error() {
        let tmp = tempfile::tempdir().unwrap();
        let db_a = tmp.path().join("empty_a.db").to_string_lossy().to_string();
        build_repo_db(&db_a, "sym_a", "known_fn", None, None);

        let (results, errors) =
            cross_graph_search(&[db_a], "does_not_exist_xyz").expect("search must not error");

        assert!(errors.is_empty(), "no DB errors");
        assert!(results.is_empty(), "no results for unknown symbol");
    }

    #[test]
    fn cross_graph_search_bad_db_path_collects_error_not_panic() {
        let (results, errors) =
            cross_graph_search(&["/nonexistent/path/ghost.db".to_string()], "foo")
                .expect("federation must not propagate per-db errors");

        // Results may be empty; error list must name the bad path.
        assert!(results.is_empty(), "no results from an unopenable db");
        assert!(
            !errors.is_empty(),
            "error list must be non-empty for bad path"
        );
        assert!(
            errors[0].contains("ghost.db"),
            "error must mention the problematic db path"
        );
    }

    // ── W7: collect_repo_info graceful fallback ───────────────────────────────

    /// A temp dir that is NOT a git repo must produce an all-None / dirty=false RepoInfo
    /// without panicking.  This validates the git-absent / non-repo path of the function.
    #[test]
    fn collect_repo_info_non_git_dir_returns_all_none() {
        let tmp = tempfile::tempdir().unwrap();
        let info = collect_repo_info(tmp.path());
        assert!(info.commit.is_none(), "commit must be None for non-git dir");
        assert!(info.branch.is_none(), "branch must be None for non-git dir");
        assert!(info.remote.is_none(), "remote must be None for non-git dir");
        assert!(!info.dirty, "dirty must be false for non-git dir");
    }

    // ── W7.1: subscribe-after-index emits ≥1 change ──────────────────────────

    /// After indexing a directory with at least one source file, `changes_since(0)` must
    /// return at least one Upsert change entry.
    #[test]
    fn index_path_emits_change_log_entries() {
        use wicked_estate_core::ChangeOp;

        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("lib.rs");
        std::fs::write(&src, "pub fn alpha() {}\n").unwrap();

        let mut store = MemStore::new();
        index_path(&mut store, tmp.path()).unwrap();

        let changes = store.changes_since(0).unwrap();
        assert!(
            !changes.is_empty(),
            "at least one change must be emitted after indexing"
        );
        assert!(
            changes.iter().any(|c| c.op == ChangeOp::Upsert),
            "at least one Upsert change must be present"
        );
    }

    /// A second index run where nothing changed must produce no NEW change-log entries
    /// beyond those from the first run.
    #[test]
    fn index_path_no_extra_changes_when_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("lib.rs");
        std::fs::write(&src, "pub fn beta() {}\n").unwrap();

        let mut store = MemStore::new();
        index_path(&mut store, tmp.path()).unwrap();

        let after_first = store.changes_since(0).unwrap();
        let last_seq = after_first.iter().map(|c| c.seq).max().unwrap_or(0);

        // Second run — nothing changed.
        index_path(&mut store, tmp.path()).unwrap();

        let after_second = store.changes_since(last_seq).unwrap();
        assert!(
            after_second.is_empty(),
            "no new change-log entries when nothing has changed"
        );
    }

    /// Deleting a file then re-indexing must emit a Remove change.
    #[test]
    fn index_path_emits_remove_change_for_deleted_file() {
        use wicked_estate_core::ChangeOp;

        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("to_delete.rs");
        std::fs::write(&src, "pub fn gamma() {}\n").unwrap();

        let mut store = MemStore::new();
        index_path(&mut store, tmp.path()).unwrap();

        let after_first = store.changes_since(0).unwrap();
        let last_seq = after_first.iter().map(|c| c.seq).max().unwrap_or(0);

        // Delete the file, re-index.
        std::fs::remove_file(&src).unwrap();
        index_path(&mut store, tmp.path()).unwrap();

        let deltas = store.changes_since(last_seq).unwrap();
        assert!(
            deltas
                .iter()
                .any(|c| c.op == ChangeOp::Remove && c.target.contains("to_delete.rs")),
            "a Remove change for to_delete.rs must be emitted"
        );
    }

    /// P8 self-pollution. wicked's own coverage/requirements artifacts, if they land at an indexed
    /// root (a coverage run whose CWD is the repo, not a worktree), must never be collected as
    /// source — indexing `coverage-report.json` back in turns its JSON keys into behavior-bearing
    /// nodes that pin the NEXT coverage run below 1.0 forever. Tests the walk seam directly, so it
    /// is independent of any extractor: drop the artifact names from the `filter_entry` skip list
    /// and `coverage-report.json` reappears in the collected set, failing the second assert.
    #[test]
    fn collect_source_files_skips_wicked_measurement_artifacts() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("main.rs"), "pub fn foo() {}\n").unwrap();
        std::fs::write(
            tmp.path().join("coverage-report.json"),
            "{\"total\":1,\"coverage\":1.0}\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("requirements_graph.json"),
            "{\"reqs\":[]}\n",
        )
        .unwrap();

        let names: Vec<String> = collect_source_files(tmp.path())
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();

        assert!(
            names.iter().any(|n| n == "main.rs"),
            "real source must still be collected; got {names:?}"
        );
        assert!(
            !names
                .iter()
                .any(|n| n == "coverage-report.json" || n == "requirements_graph.json"),
            "wicked measurement artifacts must be skipped (self-pollution); got {names:?}"
        );
    }

    /// crew#620. A run worktree lives inside the repo checkout (`<repo>/wicked-worktrees/<run>/`)
    /// and holds a full repo copy plus the run's unreviewed output. Neither walk may collect it,
    /// nor the hidden `.wicked/worktrees/` root the extra-rule walk reaches. Drop the entries from
    /// the skip helper and the worktree copies reappear, failing the asserts.
    #[test]
    fn walks_skip_run_worktrees() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("main.rs"), "pub fn foo() {}\n").unwrap();
        for wt in ["wicked-worktrees/run-1", ".wicked/worktrees/run-2"] {
            let d = root.join(wt).join(".claude-plugin");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(root.join(wt).join("main.rs"), "pub fn foo() {}\n").unwrap();
            std::fs::write(d.join("archetypes.json"), "{}\n").unwrap();
        }
        std::fs::create_dir_all(root.join(".claude-plugin")).unwrap();
        std::fs::write(root.join(".claude-plugin/archetypes.json"), "{}\n").unwrap();

        let rels = |paths: Vec<PathBuf>| -> Vec<String> {
            let mut v: Vec<String> = paths.iter().map(|p| rel(root, p)).collect();
            v.sort();
            v
        };
        assert_eq!(
            rels(collect_source_files(root)),
            vec!["main.rs".to_string()]
        );

        let rules = ExtraEdgeExtractor::from_toml_named(&[(
            "r.toml".to_string(),
            r#"
[[rule]]
name = "archetypes"
file_glob = "**/archetypes.json"
pattern = "(?P<k>\\{)"
"#
            .to_string(),
        )])
        .expect("rule parses");
        assert_eq!(
            rels(collect_extra_rule_files(root, &rules)),
            vec![".claude-plugin/archetypes.json".to_string()]
        );
    }

    /// FINDING-067. The sibling of the test above, and the one that was missing: the sweep must
    /// remove deleted SOURCE files and nothing else. A store is not always exclusively the
    /// indexer's — an orchestrator sharing one keeps its domain objects as nodes whose
    /// `location.file` is a synthetic key (`agent_session/<id>`, `validator_vault/<pin>`), never a
    /// path on disk. Deriving "previously indexed" from `all_nodes()` made every one of those look
    /// like a file that had been deleted since the last run.
    ///
    /// Measured, not hypothesised: indexing one repo subdirectory into a live orchestrator's store
    /// removed 833 operational nodes in a single transaction — 27 sessions, 77 work units, 57
    /// phases, 53 work outputs, 23 workflows, 4 validator-vault entries, 3 policies and the repo
    /// registration — while upserting the 41 source files it had been asked to index. The session
    /// that issued the index was among the 27, and the run died with `run not found`.
    ///
    /// Revert `previously_indexed` to `all_nodes()` and this fails on the first assert.
    #[test]
    fn the_delete_sweep_leaves_foreign_nodes_alone() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("real.rs"), "pub fn kept() {}\n").unwrap();

        let mut store = MemStore::new();
        index_path(&mut store, tmp.path()).unwrap();

        // A node no indexer wrote: synthetic `location.file`, shaped exactly like the operational
        // rows that were destroyed. It is in the store when the index runs, and it is not on disk.
        let foreign_id = Symbol::synthetic("wicked-core", "agent_session/run-1").id();
        let foreign = Node::new(
            foreign_id.clone(),
            NodeKind::Other("agent_session".to_string()),
            "run-1".to_string(),
            Language::new("none"),
            Location::new("agent_session/run-1".to_string(), Span::ZERO),
        );
        store.upsert_nodes(&[foreign]).unwrap();

        // Re-index with a genuinely deleted source file in the same pass, so this pins the fix
        // rather than a sweep that simply stopped running.
        std::fs::write(tmp.path().join("doomed.rs"), "pub fn doomed() {}\n").unwrap();
        index_path(&mut store, tmp.path()).unwrap();
        std::fs::remove_file(tmp.path().join("doomed.rs")).unwrap();
        index_path(&mut store, tmp.path()).unwrap();

        assert!(
            store.get_node(&foreign_id).unwrap().is_some(),
            "indexing a directory must not delete a node it never indexed"
        );
        let files: HashSet<String> = GraphRead::all_nodes(&store)
            .unwrap()
            .into_iter()
            .map(|n| n.location.file)
            .collect();
        assert!(files.contains("real.rs"), "the live source file survives");
        assert!(
            !files.contains("doomed.rs"),
            "a source file that really was deleted is still swept — the sweep still works"
        );
    }

    /// wicked-estate#433: the secret-file name prefixes (`credentials*`, `id_*`, `kubeconfig*`)
    /// excluded ordinary modules, so `credentials_store.py` vanished from the graph. Source code is
    /// indexed; the secret files beside it are not. Restore `credentials*` / `id_*` to the
    /// always-excluded `names` list and the first assert fails.
    #[test]
    fn secret_name_prefixes_keep_source_and_drop_secret_files() {
        let tmp = tempfile::tempdir().unwrap();
        let w = |rel: &str, body: &str| {
            let p = tmp.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        w(
            "integrations/credentials_store.py",
            "class CredentialsStore:\n    def get(self):\n        return 1\n",
        );
        w(
            "integrations/credentials_store_test.py",
            "def test_get():\n    assert True\n",
        );
        w(
            "src/id_utils.ts",
            "export function newId(): string { return 'a'; }\n",
        );
        w(
            "cmd/kubeconfig_loader.go",
            "package cmd\n\nfunc Load() {}\n",
        );
        w("integrations/credentials.json", "{\"token\": \"x\"}\n");
        w("integrations/credentials.yaml", "token: x\n");
        w("deploy/kubeconfig.yaml", "apiVersion: v1\nkind: Config\n");
        w("keys/id_rsa", "-----BEGIN OPENSSH PRIVATE KEY-----\n");
        w("keys/id_ed25519", "-----BEGIN OPENSSH PRIVATE KEY-----\n");
        w("keys/id_ed25519.pub", "ssh-ed25519 AAAA\n");

        let mut store = MemStore::new();
        index_path(&mut store, tmp.path()).unwrap();
        let files: HashSet<String> = GraphRead::indexed_files(&store)
            .unwrap()
            .into_iter()
            .collect();
        for kept in [
            "integrations/credentials_store.py",
            "integrations/credentials_store_test.py",
            "src/id_utils.ts",
            "cmd/kubeconfig_loader.go",
        ] {
            assert!(
                files.contains(kept),
                "{kept} is source and is indexed: {files:?}"
            );
        }
        for gone in [
            "integrations/credentials.json",
            "integrations/credentials.yaml",
            "deploy/kubeconfig.yaml",
            "keys/id_rsa",
            "keys/id_ed25519",
            "keys/id_ed25519.pub",
        ] {
            assert!(!files.contains(gone), "{gone} is a secret file: {files:?}");
        }
        let node_files: HashSet<String> = GraphRead::all_nodes(&store)
            .unwrap()
            .into_iter()
            .map(|n| n.location.file)
            .collect();
        assert!(
            node_files.contains("integrations/credentials_store.py"),
            "the module's symbols are in the graph"
        );
    }

    /// wicked-estate#433 (b): files an older build indexed that the secret rule now excludes are
    /// swept, and the sweep reports them by name, never silently: a visible one, a hidden one the
    /// walk never visits, and one under a repo label. A genuinely deleted source file is not
    /// reported as a secret exclusion. Delete the `eprintln!` block in the sweep and this fails.
    /// Binary documents are converted to Markdown and indexed under their own paths: stored
    /// content, headings as nodes, the same redaction as a `.md`. A scanned PDF is skipped with
    /// "needs OCR" (never uploaded anywhere), and a second run neither re-converts nor retries it.
    /// Before the converter the walk dropped every one of these files.
    #[test]
    fn binary_documents_are_converted_indexed_and_a_scan_is_skipped_as_needs_ocr() {
        let fixtures = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../wicked-estate-extract/tests/fixtures/docs"
        );
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("docs")).unwrap();
        for (from, to) in [
            ("spec.docx", "docs/spec.docx"),
            ("deck.pptx", "docs/deck.pptx"),
            ("runbook.pdf", "docs/runbook.pdf"),
            ("scanned.pdf", "docs/scanned.pdf"),
            // One 70,000-character paragraph: the minified-file guard would drop it as code.
            ("long.docx", "docs/long.docx"),
        ] {
            std::fs::copy(format!("{fixtures}/{from}"), tmp.path().join(to)).unwrap();
        }
        // Over the input cap: skipped by its size, never read (a sparse file, so this is cheap).
        std::fs::File::create(tmp.path().join("docs/huge.pdf"))
            .unwrap()
            .set_len(doc_convert::MAX_INPUT_BYTES as u64 + 1)
            .unwrap();
        let mut store = MemStore::new();
        DOC_SKIPS.with(|n| n.borrow_mut().clear());
        index_path(&mut store, tmp.path()).unwrap();

        let files = GraphRead::indexed_files(&store).unwrap();
        for f in [
            "docs/spec.docx",
            "docs/deck.pptx",
            "docs/runbook.pdf",
            "docs/long.docx",
        ] {
            assert!(files.contains(&f.to_string()), "{f} indexed: {files:?}");
        }
        // A scan keeps its digest (so an unchanged one is not retried) but has no content or nodes.
        assert!(
            store.file_content("docs/scanned.pdf").unwrap().is_none(),
            "a scan has no text to index"
        );
        let spec = store
            .file_content("docs/spec.docx")
            .unwrap()
            .expect("stored");
        assert!(spec.contains("reconcile_ledger"), "{spec}");
        assert!(
            !spec.contains("AKIAABCDEFGHIJKLMNOP"),
            "redacted like any markdown: {spec}"
        );
        let deck = store
            .file_content("docs/deck.pptx")
            .unwrap()
            .expect("stored");
        assert!(
            deck.contains("settle_invoice") && deck.contains("retry_budget"),
            "{deck}"
        );
        let long = store
            .file_content("docs/long.docx")
            .unwrap()
            .expect("a long-line document keeps its content");
        assert_eq!(
            long.matches("longprose").count(),
            7000,
            "the whole paragraph is kept"
        );
        let pdf = store
            .file_content("docs/runbook.pdf")
            .unwrap()
            .expect("stored");
        assert!(pdf.contains("retry_budget"), "{pdf}");
        // The heading is a node, so the document is findable by name like a .md.
        let nodes = GraphRead::all_nodes(&store).unwrap();
        assert!(!nodes.iter().any(|n| n.location.file == "docs/scanned.pdf"));
        assert!(
            nodes.iter().any(|n| n.location.file == "docs/spec.docx"
                && n.name.contains("Ledger reconciliation spec")),
            "heading node on the docx"
        );

        let skips = DOC_SKIPS.with(|n| n.borrow().clone());
        assert_eq!(skips.len(), 2, "{skips:?}");
        assert_eq!(skips[0].0, "docs/huge.pdf");
        assert!(skips[0].1.starts_with("too large"), "{skips:?}");
        assert_eq!(skips[1].0, "docs/scanned.pdf");
        assert!(skips[1].1.starts_with("needs OCR"), "{skips:?}");
        assert!(!files.contains(&"docs/huge.pdf".to_string()));

        // Unchanged documents: no re-conversion, no retry of the scan. The oversized one is still
        // reported, on the no-change exit too.
        DOC_SKIPS.with(|n| n.borrow_mut().clear());
        index_path(&mut store, tmp.path()).unwrap();
        let skips = DOC_SKIPS.with(|n| n.borrow().clone());
        assert_eq!(skips.len(), 1, "{skips:?}");
        assert_eq!(skips[0].0, "docs/huge.pdf");
    }

    #[test]
    fn a_newly_secret_excluded_file_is_swept_and_reported() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("real.rs"), "pub fn kept() {}\n").unwrap();
        std::fs::write(tmp.path().join("gone.rs"), "pub fn gone() {}\n").unwrap();
        std::fs::write(tmp.path().join("credentials.yaml"), "token: x\n").unwrap();
        std::fs::write(tmp.path().join(".env"), "TOKEN=x\n").unwrap();
        let mut store = MemStore::new();
        index_path(&mut store, tmp.path()).unwrap();

        // What an older build left: the credential files indexed, each with a symbol on it.
        let seed = |store: &mut MemStore, file: &str| {
            let id = Symbol::synthetic("test", format!("{file}/token")).id();
            let node = Node::new(
                id.clone(),
                NodeKind::Other("key".to_string()),
                "token".to_string(),
                Language::new("yaml"),
                Location::new(file.to_string(), Span::ZERO),
            );
            store.upsert_nodes(&[node]).unwrap();
            store.set_file_digest(file, "old").unwrap();
            id
        };
        let ids = [
            seed(&mut store, "credentials.yaml"),
            seed(&mut store, ".env"),
        ];
        std::fs::remove_file(tmp.path().join("gone.rs")).unwrap();
        SECRET_NOTES.with(|n| n.borrow_mut().clear());

        index_path(&mut store, tmp.path()).unwrap();
        let files = GraphRead::indexed_files(&store).unwrap();
        for f in ["credentials.yaml", ".env", "gone.rs"] {
            assert!(!files.contains(&f.to_string()), "{f} is swept: {files:?}");
        }
        assert!(files.contains(&"real.rs".to_string()));
        for id in &ids {
            assert!(
                store.get_node(id).unwrap().is_none(),
                "its symbols go with it"
            );
        }
        let notes = SECRET_NOTES.with(|n| n.borrow().clone());
        assert_eq!(
            notes,
            vec![
                "SECRET-EXCLUDED: 2 previously indexed file(s) match the secret-file rule and were \
                 removed from the index, with their symbols and any requirement annotations on \
                 them: .env, credentials.yaml"
                    .to_string()
            ],
            "the sweep reports the secret files by name, and not the deleted source file"
        );

        // Under a repo label the stored path is `<label>/...`; the rule still reads the file name.
        assert_eq!(
            secret_exclusion_note(&["svc/keys/id_rsa".to_string(), "svc/a.rs".to_string()])
                .as_deref()
                .map(|n| n.ends_with(": svc/keys/id_rsa")),
            Some(true)
        );
        assert!(secret_exclusion_note(&["gone.rs".to_string()]).is_none());
    }

    // ── Task G: incremental scenario — removed symbol leaves no dangling edge ──────────────

    /// After a file is modified such that a symbol disappears, the next `index_path` must leave
    /// no dangling edges pointing at the removed symbol.
    ///
    /// Scenario:
    ///   - Round 1: file_a.rs defines `foo`; file_b.rs calls `foo` (resolved edge b→a).
    ///   - Round 2: file_a.rs is re-written and `foo` is gone. `index_path` is run again.
    ///   - Assert: no edge with target pointing to the (now absent) `foo` symbol.
    #[test]
    fn incremental_index_no_dangling_edge_after_symbol_removed() {
        let tmp = tempfile::tempdir().unwrap();

        // Round 1: both files present; file_a defines `foo`, file_b calls it.
        let file_a = tmp.path().join("file_a.rs");
        let file_b = tmp.path().join("file_b.rs");
        std::fs::write(&file_a, "pub fn foo() {}\n").unwrap();
        std::fs::write(&file_b, "use file_a::foo;\npub fn bar() { foo(); }\n").unwrap();

        let mut store = MemStore::new();
        index_path(&mut store, tmp.path()).unwrap();

        // Sanity: after round 1 we should have nodes and edges.
        let stats_r1 = store.stats().unwrap();
        assert!(stats_r1.node_count > 0, "round-1 must produce nodes");

        // Round 2: overwrite file_a to remove `foo`. The symbol is gone.
        std::fs::write(&file_a, "pub fn other() {}\n").unwrap();

        index_path(&mut store, tmp.path()).unwrap();

        // After round 2, no edge must have a target that is not in the node set.
        let all_nodes: std::collections::HashSet<SymbolId> =
            wicked_estate_core::GraphRead::all_nodes(&store)
                .unwrap()
                .into_iter()
                .map(|n| n.symbol)
                .collect();
        let dangling: Vec<_> = store
            .all_edges()
            .unwrap()
            .into_iter()
            .filter(|e| !all_nodes.contains(&e.source) || !all_nodes.contains(&e.target))
            .collect();
        assert!(
            dangling.is_empty(),
            "after incremental re-index, no dangling edges must remain; found: {dangling:?}"
        );
    }

    // ── W5.2: compute_embeddings ──────────────────────────────────────────────

    /// With `--embeddings` ON: after indexing a tiny fixture and running
    /// `compute_embeddings`, `nearest` must return at least one result.
    #[test]
    fn compute_embeddings_on_nearest_returns_results() {
        use wicked_estate_retrieve::HashEmbedder;
        use wicked_estate_store::SqliteStore;

        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("lib.rs");
        std::fs::write(&src, "pub fn hello() {}\npub fn world() { hello(); }\n").unwrap();
        let db_path = tmp.path().join("graph.db").to_string_lossy().to_string();

        // Index into a concrete SqliteStore.
        let mut store = SqliteStore::open(&db_path).expect("open temp store");
        index_path(&mut store, tmp.path()).unwrap();

        // Embeddings table must be empty before compute_embeddings.
        let embedder = HashEmbedder::default();
        let count = compute_embeddings(&mut store, &embedder).unwrap();
        assert!(count > 0, "compute_embeddings must embed at least one node");

        // nearest must return ≥1 result for a query that matches any token.
        let qvec = embedder.embed("hello");
        let hits = store.nearest(&qvec, 10).unwrap();
        assert!(
            !hits.is_empty(),
            "nearest must return results after embeddings are populated"
        );
    }

    /// With `--embeddings` OFF: after plain `index_path`, the embeddings table
    /// must remain empty (no embeddings written as a side effect of indexing).
    #[test]
    fn index_path_without_compute_embeddings_leaves_table_empty() {
        use wicked_estate_retrieve::HashEmbedder;
        use wicked_estate_store::SqliteStore;

        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("lib.rs");
        std::fs::write(&src, "pub fn alpha() {}\n").unwrap();
        let db_path = tmp.path().join("graph.db").to_string_lossy().to_string();

        let mut store = SqliteStore::open(&db_path).expect("open temp store");
        index_path(&mut store, tmp.path()).unwrap();

        // No compute_embeddings call — nearest must return nothing.
        let embedder = HashEmbedder::default();
        let qvec = embedder.embed("alpha");
        let hits = store.nearest(&qvec, 10).unwrap();
        assert!(
            hits.is_empty(),
            "embeddings table must be empty when --embeddings was not used"
        );
    }

    // ── clusters --summary JSON shape ────────────────────────────────────────

    /// Helper: build a MemStore with two Function nodes connected by a Calls edge.
    fn build_two_node_store() -> (MemStore, SymbolId, SymbolId) {
        use wicked_estate_core::{
            Edge, EdgeKind, GraphWrite, Language, Location, ResolutionTier, Span,
        };

        let id_a = SymbolId("fn::alpha".to_string());
        let id_b = SymbolId("fn::beta".to_string());

        let node_a = Node::new(
            id_a.clone(),
            NodeKind::Function,
            "alpha",
            Language::new("rust"),
            Location::new("src/alpha.rs", Span::ZERO),
        );
        let node_b = Node::new(
            id_b.clone(),
            NodeKind::Function,
            "beta",
            Language::new("rust"),
            Location::new("src/beta.rs", Span::ZERO),
        );
        let edge = Edge::new(
            id_b.clone(),
            id_a.clone(),
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test",
        );

        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store.upsert_nodes(&[node_a, node_b]).unwrap();
        store.upsert_edges(&[edge]).unwrap();
        store.commit_batch().unwrap();

        (store, id_a, id_b)
    }

    /// `summarize_communities` returns one entry per non-empty community with all
    /// required fields non-empty (size ≥ 1, top_symbols non-empty, dominant_files non-empty).
    ///
    /// This covers the JSON-shaping logic used by `clusters --summary --json`.
    #[test]
    fn clusters_summary_json_shape_has_required_fields() {
        let (store, id_a, id_b) = build_two_node_store();

        // A hand-crafted community partition: one community with both symbols.
        let communities: Vec<Vec<SymbolId>> = vec![vec![id_a.clone(), id_b.clone()]];

        let summaries =
            wicked_estate_rank::summarize_communities(&store, &communities, 1.0).unwrap();

        assert_eq!(summaries.len(), 1, "one summary per community");
        let s = &summaries[0];
        assert_eq!(s.size, 2, "size must match member count");
        assert!(!s.top_symbols.is_empty(), "top_symbols must not be empty");
        assert!(
            !s.dominant_files.is_empty(),
            "dominant_files must not be empty"
        );

        // Verify the JSON value we'd emit in the clusters arm has all expected keys.
        let members_json: Vec<String> = communities[0].iter().map(|id| id.to_string()).collect();
        let json_obj = serde_json::json!({
            "id": 0usize,
            "size": s.size,
            "members": members_json,
            "label_candidates": s.top_symbols,
            "dominant_files": s.dominant_files,
            "modularity_contribution": s.modularity_contribution,
        });
        for key in &[
            "id",
            "size",
            "members",
            "label_candidates",
            "dominant_files",
            "modularity_contribution",
        ] {
            assert!(
                json_obj.get(key).is_some(),
                "clusters --summary JSON must include key '{key}'"
            );
        }
    }

    /// Without `--summary`, `clusters --json` emits bare arrays (back-compat).
    ///
    /// The bare-array path produces `Vec<Vec<String>>` — a JSON array of string arrays.
    /// Verify it serialises correctly and does NOT include summary keys.
    #[test]
    fn clusters_bare_json_is_array_of_arrays() {
        let (_, id_a, id_b) = build_two_node_store();

        let communities: Vec<Vec<SymbolId>> = vec![vec![id_a.clone(), id_b.clone()]];

        // This is exactly the bare-array path in the clusters arm.
        let j: Vec<Vec<String>> = communities
            .iter()
            .map(|c| c.iter().map(|s| s.to_string()).collect())
            .collect();

        let json_val = serde_json::to_value(&j).unwrap();
        assert!(json_val.is_array(), "bare output must be a JSON array");
        let outer = json_val.as_array().unwrap();
        assert_eq!(outer.len(), 1, "one inner array per community");
        let inner = outer[0].as_array().unwrap();
        assert_eq!(inner.len(), 2, "two members in the community");
        // Must NOT have a 'size' key — that would indicate the summary path.
        assert!(
            outer[0].get("size").is_none(),
            "bare array must not have 'size' key"
        );
    }

    // ── nodes --json symbol_id field ─────────────────────────────────────────

    /// `nodes --json` objects must each carry a non-empty `symbol_id`.
    ///
    /// Exercise the JSON-shaping used by the nodes arm by calling the same
    /// `serde_json::json!` construction directly against real nodes from the store.
    #[test]
    fn nodes_json_includes_symbol_id() {
        let (store, id_a, _id_b) = build_two_node_store();

        // Retrieve the nodes as the nodes arm does (all_nodes is on GraphRead / GraphStoreMutExt).
        let nodes = wicked_estate_core::GraphRead::all_nodes(&store).unwrap();
        assert!(!nodes.is_empty(), "store must have nodes");

        // Reproduce the JSON shaping from the nodes arm for both code paths.
        let j: Vec<serde_json::Value> = nodes
            .iter()
            .map(|n| {
                serde_json::json!({
                    "symbol_id": n.symbol.to_string(),
                    "name": n.name,
                    "kind": format!("{:?}", n.kind),
                    "file": n.location.file,
                    "line": n.location.span.start_line + 1,
                    "signature": n.signature,
                })
            })
            .collect();

        for obj in &j {
            let sym_id = obj.get("symbol_id").and_then(|v| v.as_str()).unwrap_or("");
            assert!(
                !sym_id.is_empty(),
                "every node JSON object must have a non-empty symbol_id; got: {obj}"
            );
        }

        // The node we inserted with id "fn::alpha" must be findable by symbol_id.
        let has_alpha = j
            .iter()
            .any(|obj| obj.get("symbol_id").and_then(|v| v.as_str()) == Some(id_a.0.as_str()));
        assert!(
            has_alpha,
            "fn::alpha must appear in nodes JSON by symbol_id"
        );

        // There must be no object missing the symbol_id key.
        let missing: Vec<_> = j
            .iter()
            .filter(|obj| obj.get("symbol_id").is_none())
            .collect();
        assert!(
            missing.is_empty(),
            "{} node(s) are missing symbol_id",
            missing.len()
        );
    }

    // ── DoD-A6a: compute_embeddings tags the store with embedder id + dim (§3.1, §3.3) ──

    #[test]
    fn compute_embeddings_writes_embedder_meta_last() {
        use wicked_estate_retrieve::HashEmbedder;
        use wicked_estate_store::SqliteStore;

        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("g.db");
        let db = db.to_str().unwrap();

        let mut store = SqliteStore::open(db).unwrap();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[Node::new(
                SymbolId("fn::alpha".to_string()),
                NodeKind::Function,
                "alpha",
                Language::new("rust"),
                Location::new("src/lib.rs", Span::ZERO),
            )])
            .unwrap();
        store.commit_batch().unwrap();

        // Before embedding: no embedder meta — a store that predates tagging reads None → the MCP
        // guard fails closed (EMBED-META-MISSING). This is the §3.3 crash-safety prior state.
        assert_eq!(store.meta_get_key("embedder_id"), None);
        assert_eq!(store.meta_get_key("embedder_dim"), None);

        let embedder = HashEmbedder::default(); // dim 128, id "hash:v1"
        let n = compute_embeddings(&mut store, &embedder).unwrap();
        assert_eq!(n, 1, "one node embedded");

        // After embedding: meta carries the exact embedder identity + dim used at index time.
        assert_eq!(
            store.meta_get_key("embedder_id").as_deref(),
            Some("hash:v1")
        );
        assert_eq!(store.meta_get_key("embedder_dim").as_deref(), Some("128"));
    }

    // ── D10: activation-table drift guard ────────────────────────────────────

    /// The production resolver slice and ENGINE-CONTRACT §3.1's "yes (slice)" rows must be the
    /// same set. The slice literal is found by its anchor comment (built at runtime so this
    /// test's own source never matches); the doc is read via CARGO_MANIFEST_DIR (repo precedent:
    /// wicked-estate-knowledge/src/lib.rs, wicked-estate-mcp/tests/conformance_schemas.rs).
    /// When a lane adds a resolver to the slice, this test fails until the doc gains its row —
    /// that is the intended behaviour.
    #[test]
    fn slice_matches_engine_contract_table() {
        let src = include_str!("lib.rs");
        // Runtime-built anchor so the anchor comment matches exactly once in this file.
        let anchor = format!("{} Activation table: docs/ENGINE-CONTRACT.md §3.1", "//");
        let matches: Vec<usize> = src.match_indices(&anchor).map(|(i, _)| i).collect();
        assert_eq!(
            matches.len(),
            1,
            "the anchor comment must appear exactly once (found {})",
            matches.len()
        );

        // Parse the ONE slice literal immediately following the anchor comment.
        let after = &src[matches[0]..];
        let open = after
            .find("= &[")
            .expect("slice literal must follow the anchor comment");
        let body_start = matches[0] + open + 4;
        let body_end = body_start
            + src[body_start..]
                .find("];")
                .expect("slice literal must close");
        let body = &src[body_start..body_end];
        let slice_ids: std::collections::BTreeSet<String> = body
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| {
                let name = t.trim_start_matches('&');
                // Map struct name → resolver id via the real instances (no string duplication).
                let id: &str = match name {
                    "NameResolver" => NameResolver.id(),
                    "ScopedNameResolver" => ScopedNameResolver.id(),
                    "ImportMapResolver" => ImportMapResolver.id(),
                    "InfraResolver" => InfraResolver.id(),
                    "RulesBridgeResolver" => RulesBridgeResolver.id(),
                    "DynamicCallResolver" => DynamicCallResolver.id(),
                    "ApexlangRefResolver" => ApexlangRefResolver.id(),
                    // let-bound in the slice (needs the repo scope at construction time)
                    "relative" => {
                        return wicked_estate_resolve::RelativeImportResolver::new(None)
                            .id()
                            .to_string();
                    }
                    other => panic!(
                        "unknown resolver `{other}` in the slice — add its arm here AND its row to \
                         docs/ENGINE-CONTRACT.md §3.1"
                    ),
                };
                id.to_string()
            })
            .collect();

        let doc = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/ENGINE-CONTRACT.md"
        ))
        .expect("docs/ENGINE-CONTRACT.md must be readable from the crate dir");
        let doc_ids: std::collections::BTreeSet<String> = doc
            .lines()
            .filter(|l| l.starts_with('|') && l.contains("yes (slice)"))
            .map(|l| {
                let first_cell = l.trim_start_matches('|').split('|').next().unwrap().trim();
                first_cell.trim_matches('`').to_string()
            })
            .collect();

        assert_eq!(
            slice_ids, doc_ids,
            "the index_path resolver slice and ENGINE-CONTRACT §3.1's `yes (slice)` rows drifted apart"
        );
    }
}

#[cfg(test)]
mod blast_radius_confidence_tests {
    use super::*;
    use wicked_estate_core::{EdgeKind, GraphWrite, ResolutionTier};
    use wicked_estate_store::MemStore;

    fn node(id: &str, kind: NodeKind) -> Node {
        Node::new(
            SymbolId(id.into()),
            kind,
            id,
            Language::new("typescript"),
            Location::new("x.ts", Span::ZERO),
        )
    }

    fn edge(src: &str, tgt: &str, kind: EdgeKind, tier: ResolutionTier) -> Edge {
        Edge::new(
            SymbolId(src.into()),
            SymbolId(tgt.into()),
            kind,
            tier,
            "test",
        )
    }

    /// `s` reaches `f` only THROUGH an import-transit File `t` (it sources only `Imports`, so
    /// `code_dependents` drops it as a row). `s` is a row, and `s → t` is the edge that
    /// admitted it — the envelope must count it. `t → f` is sourced at a non-row and must not
    /// count; neither may the parse-certain `Contains` edge.
    #[test]
    fn envelope_counts_the_edge_into_a_dropped_transit_file() {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[
                node("f", NodeKind::Function),
                node("t", NodeKind::File),
                node("s", NodeKind::Function),
                node("home", NodeKind::File),
            ])
            .unwrap();
        store
            .upsert_edges(&[
                edge("t", "f", EdgeKind::Imports, ResolutionTier::ImportMap),
                edge("s", "t", EdgeKind::References, ResolutionTier::Tags),
                edge("home", "s", EdgeKind::Contains, ResolutionTier::Parsed),
            ])
            .unwrap();
        store.commit_batch().unwrap();

        let br = blast_radius_by_name(&store, "f", 12).unwrap();
        let rows: Vec<&str> = br.dependents.iter().map(|n| n.symbol.as_str()).collect();
        assert!(rows.contains(&"s") && !rows.contains(&"t"), "{rows:?}");
        let tags = ResolutionTier::Tags.default_confidence().get();
        assert_eq!(br.confidence.edge_count, 1, "{:?}", br.confidence);
        assert_eq!(br.confidence.min, Some(tags));
        assert_eq!(br.confidence.avg, Some(tags));
    }
}

#[cfg(test)]
mod semantics_survive_reindex_tests {
    use super::*;
    use wicked_estate_core::{GraphRead, GraphWrite, NodeSemantics, ValidationClaim};
    use wicked_estate_store::{GraphStoreMutExt, MemStore, SqliteStore};

    fn sym_of(store: &dyn GraphStoreMutExt, name: &str) -> SymbolId {
        let hits = store
            .find_symbols(&wicked_estate_core::SymbolQuery {
                exact_name: Some(name.to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(hits.len(), 1, "expected one `{name}`");
        hits[0].symbol.clone()
    }

    /// Annotate `symbol`, then SEED a past validation time through `restore_node_semantics` (no
    /// sleep): the clock cannot reproduce `1_000_000_000`, so a re-stamp by the re-index would
    /// change the time and the round-trip assertion would fail. Returns the seeded snapshot.
    fn annotate_with_claim(store: &mut dyn GraphStoreMutExt, symbol: &SymbolId) -> NodeSemantics {
        let claim = ValidationClaim::new(true, "alice").unwrap();
        store
            .set_node_semantics(symbol, Some("pays"), Some("REQ-1"), Some(&claim))
            .unwrap();
        let mut got = store.node_semantics(symbol).unwrap().unwrap();
        assert_eq!(got.requirement_validated_by.as_deref(), Some("alice"));
        got.requirement_validated_at = Some(1_000_000_000);
        store.restore_node_semantics(symbol, &got).unwrap();
        assert_eq!(
            store.node_semantics(symbol).unwrap(),
            Some(got.clone()),
            "the seeded past time must be stored before the re-index"
        );
        got
    }

    fn version_change_keeps_five_fields(store: &mut dyn GraphStoreMutExt) {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("a.rs"),
            "pub fn pay() {}\npub fn refund() {}\n",
        )
        .unwrap();
        index_path(store, tmp.path()).unwrap();
        let pay = sym_of(store, "pay");
        let before = annotate_with_claim(store, &pay);

        // An upgrade: the stored indexed_version differs, so every file is re-extracted.
        store.meta_set_key("indexed_version", "0.0.0-older");
        index_path(store, tmp.path()).unwrap();

        assert_eq!(
            store.node_semantics(&sym_of(store, "pay")).unwrap(),
            Some(before),
            "all five fields, validator and time included, must survive the forced re-extract"
        );
    }

    #[test]
    fn sqlite_version_change_keeps_node_semantics() {
        version_change_keeps_five_fields(&mut SqliteStore::in_memory().unwrap());
    }

    /// #470: this argument slot is created by call resolution, after extraction writes its nodes.
    #[test]
    fn sqlite_version_change_keeps_call_derived_node_semantics() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(
            src.path().join("a.ts"),
            "function identity(value: string) { return value; }\n\
             function consume() { identity(externalInput); }\n",
        )
        .unwrap();
        let mut store = SqliteStore::in_memory().unwrap();
        index_path(&mut store, src.path()).unwrap();
        let slot = GraphRead::all_nodes(&store)
            .unwrap()
            .into_iter()
            .find(|n| {
                n.symbol
                    .as_str()
                    .ends_with("consume().:local:externalInput:")
            })
            .expect("resolution must create the external argument slot")
            .symbol;
        let before = annotate_with_claim(&mut store, &slot);
        SEMANTICS_DROPS.with(|notes| notes.borrow_mut().clear());

        store.meta_set_key("indexed_version", "0.0.0-older");
        index_path(&mut store, src.path()).unwrap();

        assert_eq!(store.node_semantics(&slot).unwrap(), Some(before));
        SEMANTICS_DROPS.with(|notes| assert!(notes.borrow().is_empty(), "{:?}", notes.borrow()));
    }

    /// A rename `a.cpp` → `a.cc` keeps the free function's SymbolId `compute`, so the five semantic
    /// fields survive the single run that deletes a.cpp and adds a.cc (#438 item 1).
    fn rename_keeps_five_fields_under_same_id(store: &mut dyn GraphStoreMutExt) {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.cpp"), "int compute() { return 1; }\n").unwrap();
        index_path(store, tmp.path()).unwrap();
        let compute = sym_of(store, "compute");
        let claim = ValidationClaim::new(true, "alice").unwrap();
        store
            .set_node_semantics(&compute, Some("adds"), Some("REQ-9"), Some(&claim))
            .unwrap();
        let mut before = store.node_semantics(&compute).unwrap().unwrap();
        before.requirement_validated_at = Some(1_000_000_000);
        store.restore_node_semantics(&compute, &before).unwrap();

        // Rename: same content, new extension — a.cpp is deleted and a.cc is new in ONE run.
        std::fs::remove_file(tmp.path().join("a.cpp")).unwrap();
        std::fs::write(tmp.path().join("a.cc"), "int compute() { return 1; }\n").unwrap();
        index_path(store, tmp.path()).unwrap();

        let renamed = sym_of(store, "compute");
        assert_eq!(renamed, compute, "the rename must keep the same SymbolId");
        assert_eq!(
            store.node_semantics(&renamed).unwrap(),
            Some(before),
            "all five fields must survive a rename onto the same SymbolId"
        );
    }

    #[test]
    fn sqlite_rename_keeps_five_fields_under_same_id() {
        rename_keeps_five_fields_under_same_id(&mut SqliteStore::in_memory().unwrap());
    }

    #[test]
    fn mem_rename_keeps_five_fields_under_same_id() {
        rename_keeps_five_fields_under_same_id(&mut MemStore::new());
    }

    #[test]
    fn mem_version_change_keeps_node_semantics() {
        version_change_keeps_five_fields(&mut MemStore::new());
    }

    // Compare only graph identity and parked site identity; timestamps and ranking are unrelated.
    fn assert_pipeline_matches_fresh(actual: &SqliteStore, db: &Path, root: &Path) {
        let tmp = tempfile::tempdir().unwrap();
        let fresh_db = tmp.path().join("fresh.db");
        let mut fresh = SqliteStore::open(&fresh_db).unwrap();
        index_path(&mut fresh, root).unwrap();
        let nodes = |store: &SqliteStore| {
            let mut rows: Vec<_> = GraphRead::all_nodes(store)
                .unwrap()
                .into_iter()
                .map(|n| (n.symbol.0, format!("{:?}", n.kind), n.location.file))
                .collect();
            rows.sort();
            rows
        };
        let edges = |store: &SqliteStore| {
            let mut rows: Vec<_> = store
                .all_edges()
                .unwrap()
                .into_iter()
                .map(|e| (e.source.0, e.target.0, format!("{:?}", e.kind)))
                .collect();
            rows.sort();
            rows
        };
        let parked = |path: &Path| {
            let conn = rusqlite::Connection::open(path).unwrap();
            let mut query = conn
                .prepare(
                    "SELECT s.sym, u.raw_name, u.kind, u.file, u.line, u.start_byte, u.end_byte \
                     FROM unresolved_refs u JOIN symbols s ON s.sid = u.from_sym \
                     ORDER BY s.sym, u.raw_name, u.kind, u.file, u.line, u.start_byte, u.end_byte",
                )
                .unwrap();
            let rows = query
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, i64>(5)?,
                        r.get::<_, i64>(6)?,
                    ))
                })
                .unwrap();
            rows.collect::<std::result::Result<Vec<_>, _>>().unwrap()
        };
        assert_eq!(nodes(actual), nodes(&fresh), "nodes differ from full index");
        assert_eq!(edges(actual), edges(&fresh), "edges differ from full index");
        assert_eq!(
            parked(db),
            parked(&fresh_db),
            "parked rows differ from full index"
        );
    }

    fn write_pipeline_fixture(root: &Path, path: &str, text: &str) {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }

    #[test]
    fn killed_index_then_plain_reindex_restores_every_edge() {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                TEST_INDEX_FAIL_AFTER.with(|point| point.set(None));
            }
        }
        for point in ["digests", "deletions"] {
            let src = tempfile::tempdir().unwrap();
            let dbdir = tempfile::tempdir().unwrap();
            let db = dbdir.path().join("graph.db");
            if point == "digests" {
                write_pipeline_fixture(src.path(), "caller/a.rs", "fn caller() { target(); }\n");
            } else {
                write_pipeline_fixture(src.path(), "a.ts", "import './b';\n");
                write_pipeline_fixture(src.path(), "b.ts", "export function target() {}\n");
            }
            let mut store = SqliteStore::open(&db).unwrap();
            index_path(&mut store, src.path()).unwrap();
            if point == "digests" {
                assert_eq!(store.unresolved_refs_for_name("target").unwrap().len(), 1);
                write_pipeline_fixture(src.path(), "callee/b.rs", "fn target() {}\n");
            } else {
                std::fs::remove_file(src.path().join("b.ts")).unwrap();
            }
            let reset = Reset;
            TEST_INDEX_FAIL_AFTER.with(|hook| hook.set(Some(point)));
            let err = index_path(&mut store, src.path()).expect_err("injected interruption");
            assert!(err.to_string().contains(point));
            drop(reset);
            // A new handle models a restarted process after the committed batch.
            drop(store);
            let mut store = SqliteStore::open(&db).unwrap();
            index_path(&mut store, src.path()).unwrap();
            assert_pipeline_matches_fresh(&store, &db, src.path());
        }
    }

    #[test]
    fn homonym_added_unbinds_name_bound_edge() {
        let src = tempfile::tempdir().unwrap();
        let dbdir = tempfile::tempdir().unwrap();
        let db = dbdir.path().join("graph.db");
        write_pipeline_fixture(src.path(), "caller/a.rs", "fn caller() { target(); }\n");
        write_pipeline_fixture(src.path(), "first/b.rs", "fn target() {}\n");
        let mut store = SqliteStore::open(&db).unwrap();
        index_path(&mut store, src.path()).unwrap();
        assert!(store.unresolved_refs_for_name("target").unwrap().is_empty());
        write_pipeline_fixture(src.path(), "second/c.rs", "fn target() {}\n");
        index_path(&mut store, src.path()).unwrap();
        assert_pipeline_matches_fresh(&store, &db, src.path());
        assert_eq!(store.unresolved_refs_for_name("target").unwrap().len(), 1);
    }

    fn homonym_disappears_rebinds_parked_ref(delete: bool) {
        let src = tempfile::tempdir().unwrap();
        let dbdir = tempfile::tempdir().unwrap();
        let db = dbdir.path().join("graph.db");
        write_pipeline_fixture(src.path(), "caller/a.rs", "fn caller() { target(); }\n");
        write_pipeline_fixture(src.path(), "first/b.rs", "fn target() {}\n");
        write_pipeline_fixture(src.path(), "second/c.rs", "fn target() {}\n");
        let mut store = SqliteStore::open(&db).unwrap();
        index_path(&mut store, src.path()).unwrap();
        assert_eq!(store.unresolved_refs_for_name("target").unwrap().len(), 1);
        if delete {
            std::fs::remove_file(src.path().join("second/c.rs")).unwrap();
        } else {
            write_pipeline_fixture(src.path(), "second/c.rs", "fn renamed() {}\n");
        }
        index_path(&mut store, src.path()).unwrap();
        assert_pipeline_matches_fresh(&store, &db, src.path());
        assert!(store.unresolved_refs_for_name("target").unwrap().is_empty());
    }

    #[test]
    fn homonym_removed_rebinds_parked_ref() {
        homonym_disappears_rebinds_parked_ref(true);
    }

    #[test]
    fn homonym_renamed_away_rebinds_parked_ref() {
        homonym_disappears_rebinds_parked_ref(false);
    }

    #[test]
    fn same_file_bound_edge_does_not_force_reextract() {
        let src = tempfile::tempdir().unwrap();
        let dbdir = tempfile::tempdir().unwrap();
        let db = dbdir.path().join("graph.db");
        write_pipeline_fixture(
            src.path(),
            "caller/a.rs",
            "fn target() {}\nfn caller() { target(); }\n",
        );
        write_pipeline_fixture(src.path(), "client/c.rs", "fn distant() { target(); }\n");
        let mut store = SqliteStore::open(&db).unwrap();
        index_path(&mut store, src.path()).unwrap();
        let target = sym_of(&store, "target");
        let bound = store
            .neighbors(&target, wicked_estate_core::Direction::Dependents)
            .unwrap();
        assert!(bound.iter().any(|e| {
            e.kind == EdgeKind::Calls
                && e.resolved_by == "scoped-name-resolver"
                && e.metadata.get("scope").and_then(|v| v.as_str()) == Some("same-file")
        }));
        write_pipeline_fixture(src.path(), "other/b.rs", "fn target() {}\n");
        TEST_REBIND_FILES.with(|count| count.set(0));
        index_path(&mut store, src.path()).unwrap();
        TEST_REBIND_FILES.with(|count| {
            assert_eq!(
                count.get(),
                1,
                "only the distant caller needs re-extraction"
            );
        });
        assert_pipeline_matches_fresh(&store, &db, src.path());
    }

    #[test]
    fn new_parameter_name_does_not_trigger_rebind() {
        let src = tempfile::tempdir().unwrap();
        let dbdir = tempfile::tempdir().unwrap();
        let db = dbdir.path().join("graph.db");
        // Value-flow parameter nodes are minted for JS/TS (the flow.parameter captures).
        write_pipeline_fixture(
            src.path(),
            "first/a.ts",
            "export function a(value: number) { return value; }\n",
        );
        write_pipeline_fixture(
            src.path(),
            "caller/c.ts",
            "export function caller() { return a(1); }\n",
        );
        let mut store = SqliteStore::open(&db).unwrap();
        index_path(&mut store, src.path()).unwrap();
        assert!(
            GraphRead::all_nodes(&store)
                .unwrap()
                .iter()
                .any(|node| node.name == "value" && node.is_value_flow_node())
        );

        write_pipeline_fixture(
            src.path(),
            "second/b.ts",
            "export function b(value: number) { return value; }\n",
        );
        TEST_REBIND_FILES.with(|count| count.set(0));
        index_path(&mut store, src.path()).unwrap();
        TEST_REBIND_FILES.with(|count| assert_eq!(count.get(), 0));
        assert_pipeline_matches_fresh(&store, &db, src.path());
    }

    #[test]
    fn extractor_edges_do_not_trigger_rebind() {
        for (producer, kind) in [
            ("tree-sitter", EdgeKind::References),
            ("iac-cloudformation", EdgeKind::Contains),
            ("jcl", EdgeKind::Defines),
            ("extra-edge:test", EdgeKind::References),
            (
                "tree-sitter-convention",
                EdgeKind::Other(edge_tags::FLOWS_TO.into()),
            ),
        ] {
            let src = tempfile::tempdir().unwrap();
            write_pipeline_fixture(src.path(), "first/a.rs", "fn target() {}\n");
            write_pipeline_fixture(src.path(), "owner/c.rs", "fn owner() {}\n");
            let mut store = SqliteStore::in_memory().unwrap();
            index_path(&mut store, src.path()).unwrap();
            let edge = Edge::new(
                sym_of(&store, "owner"),
                sym_of(&store, "target"),
                kind,
                ResolutionTier::Parsed,
                producer,
            )
            .with_location(Location::new("owner/c.rs", Span::ZERO));
            store.upsert_edges(std::slice::from_ref(&edge)).unwrap();

            write_pipeline_fixture(src.path(), "second/b.rs", "fn target() {}\n");
            TEST_REBIND_FILES.with(|count| count.set(0));
            index_path(&mut store, src.path()).unwrap();
            TEST_REBIND_FILES.with(|count| assert_eq!(count.get(), 0, "{producer}"));
            assert!(
                store.all_edges().unwrap().iter().any(|stored| {
                    stored.dedup_key() == edge.dedup_key() && stored.resolved_by == producer
                }),
                "{producer}"
            );
        }
    }

    #[test]
    fn skipped_file_keeps_its_digest_and_is_not_reextracted() {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                TEST_INDEX_WRITE_HOOK.with(|hook| *hook.borrow_mut() = None);
            }
        }
        let src = tempfile::tempdir().unwrap();
        let minified = "x".repeat(50_001);
        assert!(is_minified_or_huge(&minified));
        write_pipeline_fixture(src.path(), "minified.rs", &minified);
        write_pipeline_fixture(src.path(), "empty.rs", "");
        write_pipeline_fixture(src.path(), "normal.rs", "fn normal() {}\n");
        let mut store = SqliteStore::in_memory().unwrap();
        index_path(&mut store, src.path()).unwrap();
        for (file, text) in [("minified.rs", minified.as_str()), ("empty.rs", "")] {
            assert_eq!(
                store.file_digest(file).unwrap(),
                Some(source_digest(text.as_bytes(), None)),
                "{file} must retain a digest even without an extraction"
            );
        }

        let writes = std::rc::Rc::new(std::cell::Cell::new(0));
        let hook_writes = writes.clone();
        let reset = Reset;
        TEST_INDEX_WRITE_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                hook_writes.set(hook_writes.get() + 1);
            }));
        });
        TEST_MAX_EXTRACTIONS.with(|count| count.set(0));
        index_path(&mut store, src.path()).unwrap();
        drop(reset);
        TEST_MAX_EXTRACTIONS.with(|count| assert_eq!(count.get(), 0));
        assert_eq!(
            writes.get(),
            0,
            "unchanged files must take the no-change exit"
        );
        for (file, text) in [("minified.rs", minified.as_str()), ("empty.rs", "")] {
            assert_eq!(
                store.file_digest(file).unwrap(),
                Some(source_digest(text.as_bytes(), None))
            );
        }
    }

    #[test]
    fn chunk_extracts_bytes_read_at_chunk_time() {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                TEST_INDEX_WRITE_HOOK.with(|hook| *hook.borrow_mut() = None);
            }
        }
        let src = tempfile::tempdir().unwrap();
        let file = src.path().join("a.rs");
        std::fs::write(&file, "fn old() {}\n").unwrap();
        let mut store = SqliteStore::in_memory().unwrap();
        let new_text = "fn current() {}\n";
        let reset = Reset;
        let hook_file = file.clone();
        TEST_INDEX_WRITE_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                std::fs::write(&hook_file, new_text).unwrap();
            }));
        });
        index_path(&mut store, src.path()).unwrap();
        drop(reset);
        assert_eq!(
            store.file_content("a.rs").unwrap().as_deref(),
            Some(new_text)
        );
        assert_eq!(
            store.file_digest("a.rs").unwrap(),
            Some(source_digest(new_text.as_bytes(), None))
        );
        assert!(
            GraphRead::all_nodes(&store)
                .unwrap()
                .iter()
                .any(|n| n.name == "current")
        );
        TEST_MAX_EXTRACTIONS.with(|count| count.set(0));
        index_path(&mut store, src.path()).unwrap();
        TEST_MAX_EXTRACTIONS.with(|count| assert_eq!(count.get(), 0));
    }

    /// #463: a claim committed by another connection during extraction must reach the snapshot.
    #[test]
    fn sqlite_claim_written_during_extraction_survives_reindex() {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                TEST_INDEX_WRITE_HOOK.with(|hook| *hook.borrow_mut() = None);
            }
        }
        let src = tempfile::tempdir().unwrap();
        let file = src.path().join("a.rs");
        std::fs::write(&file, "pub fn pay() {}\n").unwrap();
        let dbdir = tempfile::tempdir().unwrap();
        let dbpath = dbdir.path().join("graph.db");
        let mut store = SqliteStore::open(&dbpath).unwrap();
        index_path(&mut store, src.path()).unwrap();
        let symbol = sym_of(&store, "pay");
        store
            .set_node_semantics(
                &symbol,
                None,
                None,
                Some(&ValidationClaim::new(true, "alice").unwrap()),
            )
            .unwrap();
        std::fs::write(&file, "pub fn pay() { let _changed = 1; }\n").unwrap();

        let reset = Reset;
        let hook_symbol = symbol.clone();
        let mut called = false;
        TEST_INDEX_WRITE_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                if called {
                    return;
                }
                called = true;
                let mut side = SqliteStore::open(&dbpath).unwrap();
                side.set_node_semantics(
                    &hook_symbol,
                    None,
                    None,
                    Some(&ValidationClaim::new(false, "bob").unwrap()),
                )
                .unwrap();
            }));
        });
        index_path(&mut store, src.path()).unwrap();
        drop(reset);

        let claim = store.node_semantics(&symbol).unwrap().unwrap();
        assert!(
            !claim.requirement_validated,
            "Bob's retraction must survive"
        );
        assert_eq!(claim.requirement_validated_by.as_deref(), Some("bob"));
    }

    /// #515: a failure in chunk two rolls back only that chunk. Chunk one's annotation is
    /// already restored, and its missing digest forces a plain retry to re-extract it too.
    #[test]
    fn sqlite_chunk_write_failure_keeps_fields_and_retry_reextracts() {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                TEST_INDEX_WRITE_HOOK.with(|hook| *hook.borrow_mut() = None);
                TEST_EXTRACT_CHUNK_LIMITS.with(|limits| limits.set(None));
            }
        }
        let src = tempfile::tempdir().unwrap();
        for i in 0..4 {
            std::fs::write(
                src.path().join(format!("file{i}.rs")),
                format!("pub fn pay{i}() {{}}\n"),
            )
            .unwrap();
        }
        let dbdir = tempfile::tempdir().unwrap();
        let dbpath = dbdir.path().join("graph.db");
        let mut store = SqliteStore::open(&dbpath).unwrap();
        index_path(&mut store, src.path()).unwrap();
        let saved: Vec<_> = (0..4)
            .map(|i| {
                let symbol = sym_of(&store, &format!("pay{i}"));
                let fields = annotate_with_claim(&mut store, &symbol);
                (symbol, fields)
            })
            .collect();
        for i in 0..4 {
            std::fs::write(
                src.path().join(format!("file{i}.rs")),
                format!("pub fn pay{i}() {{ let _changed = {i}; }}\n"),
            )
            .unwrap();
        }
        SEMANTICS_DROPS.with(|notes| notes.borrow_mut().clear());
        let reset = Reset;
        TEST_EXTRACT_CHUNK_LIMITS.with(|limits| limits.set(Some((usize::MAX, 1))));
        let hook_db = dbpath.clone();
        let mut calls = 0;
        TEST_INDEX_WRITE_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                calls += 1;
                if calls == 2 {
                    let side = rusqlite::Connection::open(&hook_db).unwrap();
                    side.busy_timeout(std::time::Duration::from_millis(200))
                        .unwrap();
                    let incomplete: usize = side
                        .query_row("SELECT count(*) FROM files WHERE digest = ''", [], |r| {
                            r.get(0)
                        })
                        .unwrap();
                    assert_eq!(
                        incomplete, 1,
                        "the first chunk committed without its digest"
                    );
                    side.execute_batch(
                        "CREATE TRIGGER we515_poison BEFORE INSERT ON nodes \
                         BEGIN SELECT RAISE(ABORT, 'we515 injected write failure'); END;",
                    )
                    .unwrap();
                }
            }));
        });
        let err = index_path(&mut store, src.path()).expect_err("chunk two must fail");
        assert!(err.to_string().contains("we515 injected write failure"));
        drop(reset);
        for (symbol, fields) in &saved {
            assert_eq!(store.node_semantics(symbol).unwrap().as_ref(), Some(fields));
        }
        SEMANTICS_DROPS.with(|notes| assert!(notes.borrow().is_empty()));

        let side = rusqlite::Connection::open(&dbpath).unwrap();
        side.busy_timeout(std::time::Duration::from_millis(200))
            .unwrap();
        side.execute_batch("DROP TRIGGER we515_poison;").unwrap();
        let incomplete: String = side
            .query_row("SELECT path FROM files WHERE digest = ''", [], |r| r.get(0))
            .unwrap();
        // Corrupt the committed chunk's display name: retry must replace it despite unchanged
        // source bytes since the failed run, rather than trusting that chunk as fully indexed.
        assert_eq!(
            side.execute(
                "UPDATE nodes SET name = 'old_pay' WHERE file = ?1 AND name LIKE 'pay%'",
                [&incomplete],
            )
            .unwrap(),
            1
        );
        drop(side);
        index_path(&mut store, src.path()).unwrap();
        for (i, (symbol, fields)) in saved.iter().enumerate() {
            assert_eq!(&sym_of(&store, &format!("pay{i}")), symbol);
            assert_eq!(store.node_semantics(symbol).unwrap().as_ref(), Some(fields));
            let file = format!("file{i}.rs");
            let bytes = std::fs::read(src.path().join(&file)).unwrap();
            assert_eq!(
                store.file_digest(&file).unwrap(),
                Some(source_digest(&bytes, None))
            );
        }
    }

    /// #515: a symbol whose old rows a committed chunk removed, and that is not re-created
    /// until a later run, keeps its fields durably. The run dies in chunk two; a fresh run that
    /// re-creates the symbol restores them. Fails if the pending snapshot lives only in memory.
    #[test]
    fn sqlite_pending_fields_survive_a_killed_run() {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                TEST_INDEX_WRITE_HOOK.with(|hook| *hook.borrow_mut() = None);
                TEST_EXTRACT_CHUNK_LIMITS.with(|limits| limits.set(None));
            }
        }
        let src = tempfile::tempdir().unwrap();
        // `changed` has no stable order, so every file carries an annotated symbol that the
        // edit removes: whichever file chunk one takes loses one.
        let full = |i: usize| format!("pub fn pay{i}() {{}}\npub fn extra{i}() {{}}\n");
        for i in 0..4 {
            std::fs::write(src.path().join(format!("file{i}.rs")), full(i)).unwrap();
        }
        let dbdir = tempfile::tempdir().unwrap();
        let dbpath = dbdir.path().join("graph.db");
        let mut store = SqliteStore::open(&dbpath).unwrap();
        index_path(&mut store, src.path()).unwrap();
        let saved: Vec<_> = (0..4)
            .map(|i| {
                let symbol = sym_of(&store, &format!("extra{i}"));
                let fields = annotate_with_claim(&mut store, &symbol);
                (symbol, fields)
            })
            .collect();
        for i in 0..4 {
            std::fs::write(
                src.path().join(format!("file{i}.rs")),
                format!("pub fn pay{i}() {{ let _x = {i}; }}\n"),
            )
            .unwrap();
        }
        SEMANTICS_DROPS.with(|notes| notes.borrow_mut().clear());
        let reset = Reset;
        TEST_EXTRACT_CHUNK_LIMITS.with(|limits| limits.set(Some((usize::MAX, 1))));
        let hook_db = dbpath.clone();
        let mut calls = 0;
        TEST_INDEX_WRITE_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                calls += 1;
                if calls == 2 {
                    let side = rusqlite::Connection::open(&hook_db).unwrap();
                    side.busy_timeout(std::time::Duration::from_millis(200))
                        .unwrap();
                    side.execute_batch(
                        "CREATE TRIGGER we515_poison2 BEFORE INSERT ON nodes \
                         BEGIN SELECT RAISE(ABORT, 'we515 injected write failure'); END;",
                    )
                    .unwrap();
                }
            }));
        });
        index_path(&mut store, src.path()).expect_err("chunk two must fail");
        drop(reset);
        let gone = saved
            .iter()
            .filter(|(s, _)| store.get_node(s).unwrap().is_none())
            .count();
        assert_eq!(gone, 1, "chunk one removed its file's annotated symbol");

        let side = rusqlite::Connection::open(&dbpath).unwrap();
        side.execute_batch("DROP TRIGGER we515_poison2;").unwrap();
        drop(side);
        // A fresh store handle stands for the restarted process.
        drop(store);
        let mut store = SqliteStore::open(&dbpath).unwrap();
        for i in 0..4 {
            std::fs::write(src.path().join(format!("file{i}.rs")), full(i)).unwrap();
        }
        index_path(&mut store, src.path()).unwrap();
        for (symbol, fields) in &saved {
            assert_eq!(store.node_semantics(symbol).unwrap().as_ref(), Some(fields));
        }
        SEMANTICS_DROPS.with(|notes| assert!(notes.borrow().is_empty()));
        assert_eq!(
            store
                .meta_get(PENDING_SEMANTICS_KEY)
                .unwrap()
                .unwrap_or_default(),
            "",
            "the durable snapshot is cleared once restored"
        );
    }

    /// #438 item 2: remove + upsert + restore share one chunk batch, and a write failure mid-re-index
    /// rolls the removal back so the stored fields survive; a retry on the SAME store (no drop,
    /// no reconnect — as the `watch` loop keeps it) keeps them. The failure is a persistent
    /// `BEFORE INSERT` trigger installed over a side connection — it fires on the indexer's own
    /// connection during the node upsert, AFTER the batch has already removed the file. The fix
    /// rolls the open batch back on error at the `index_path_as` boundary; without it the next
    /// index on the same store would reuse the half-written transaction and commit the removal
    /// without the fields. On the pre-fix code the fields are gone for good.
    #[test]
    fn sqlite_version_write_failure_plain_retry_reextracts_and_keeps_fields() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("a.rs"), "pub fn pay() {}\n").unwrap();
        let dbdir = tempfile::tempdir().unwrap();
        let dbpath = dbdir.path().join("g.db");

        // Phase 1: index, annotate, seed a past validation time, commit to disk.
        let (pay_id, original);
        {
            let mut store = SqliteStore::open(&dbpath).unwrap();
            index_path(&mut store, src.path()).unwrap();
            let pay = sym_of(&store, "pay");
            pay_id = pay.clone();
            original = annotate_with_claim(&mut store, &pay);
        }

        // One long-lived store for BOTH the failed run and the retry — exactly what `watch` does.
        let mut store = SqliteStore::open(&dbpath).unwrap();

        // Simulate an older extractor's output without changing the source or its digest.
        // Only a full re-extraction can replace this stale name with `pay`.
        rusqlite::Connection::open(&dbpath)
            .unwrap()
            .execute("UPDATE nodes SET name = 'old_pay' WHERE name = 'pay'", [])
            .unwrap();

        // Phase 2: a version-triggered re-index whose first node INSERT is poisoned.
        {
            let side = rusqlite::Connection::open(&dbpath).unwrap();
            side.execute_batch(
                "CREATE TRIGGER we438_poison BEFORE INSERT ON nodes \
                 BEGIN SELECT RAISE(ABORT, 'we438 injected write failure'); END;",
            )
            .unwrap();
            drop(side);

            store.meta_set_key("indexed_version", "0.0.0-older"); // force full re-extract
            let err = index_path(&mut store, src.path())
                .expect_err("the poisoned node INSERT must abort the re-index");
            let msg = format!("{err}").to_lowercase();
            assert!(
                msg.contains("injected write failure") || msg.contains("abort"),
                "{err}"
            );
        }
        // The batch was rolled back at the index_path_as boundary, so `store`'s connection holds
        // no open transaction and the original five fields survive — read on the SAME store.
        assert_eq!(
            store.node_semantics(&pay_id).unwrap(),
            Some(original.clone()),
            "an interrupted re-index must not lose the semantic fields"
        );
        // Remove the poison; `store`'s connection is idle (no open batch), so the DDL is free.
        rusqlite::Connection::open(&dbpath)
            .unwrap()
            .execute_batch("DROP TRIGGER we438_poison;")
            .unwrap();

        // Phase 3 (#471): plain retry, with no metadata reset, must re-extract the old output.
        index_path(&mut store, src.path()).unwrap();
        assert_eq!(sym_of(&store, "pay"), pay_id);
        assert_eq!(
            store.meta_get_key("indexed_version").as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(
            store.node_semantics(&pay_id).unwrap(),
            Some(original),
            "a retry on the same store keeps the restored fields"
        );
    }

    /// Forces the work-list read of `abs` to fail until dropped (#471): a deterministic read
    /// failure on every platform and as any user, unlike file permissions (root reads mode 000).
    pub(super) struct ReadFailGuard(PathBuf);
    impl ReadFailGuard {
        pub(super) fn new(abs: PathBuf) -> Self {
            TEST_READ_FAILS.lock().unwrap().push(abs.clone());
            ReadFailGuard(abs)
        }
    }
    impl Drop for ReadFailGuard {
        fn drop(&mut self) {
            if let Ok(mut v) = TEST_READ_FAILS.lock() {
                v.retain(|p| p != &self.0);
            }
        }
    }

    fn assert_same_extracted_graph(actual: &SqliteStore, fresh: &SqliteStore) {
        let mut actual_nodes = GraphRead::all_nodes(actual).unwrap();
        let mut fresh_nodes = GraphRead::all_nodes(fresh).unwrap();
        actual_nodes.sort_by_key(|node| node.symbol.clone());
        fresh_nodes.sort_by_key(|node| node.symbol.clone());
        assert_eq!(actual_nodes, fresh_nodes);
        let sorted_edges = |store: &SqliteStore| {
            let mut edges: Vec<String> = store
                .all_edges()
                .unwrap()
                .iter()
                .map(|edge| serde_json::to_string(edge).unwrap())
                .collect();
            edges.sort();
            edges
        };
        assert_eq!(sorted_edges(actual), sorted_edges(fresh));
    }

    /// #477: a killed run has committed the new rule's output and the file digests, but has
    /// not stamped the markers. Rolling the rules back must still force full re-extraction.
    #[test]
    fn sqlite_interrupted_rules_change_then_rollback_retry_matches_fresh() {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                TEST_INDEX_STAMP_HOOK.with(|hook| *hook.borrow_mut() = None);
            }
        }
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("a.rs"), "pub fn keep() {}\n").unwrap();
        let dbdir = tempfile::tempdir().unwrap();
        let dbpath = dbdir.path().join("g.db");
        let mut store = SqliteStore::open(&dbpath).unwrap();
        index_path(&mut store, src.path()).unwrap();
        let rule_dir = src.path().join(".wicked-estate-extractors");
        std::fs::create_dir(&rule_dir).unwrap();
        let rule = rule_dir.join("interrupted.toml");
        std::fs::write(
            &rule,
            r#"[[rule]]
name = "reviewmarker"
file_glob = "a.rs"
pattern = '(?P<n>keep)'
[rule.emit_node]
id_template = "reviewmarker:{n}"
label_capture = "n"
kind = "synthetic"
node_scheme = "reviewmarker"
"#,
        )
        .unwrap();
        let reset = Reset;
        TEST_INDEX_STAMP_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(|| panic!("interrupted before marker stamp")));
        });
        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            index_path(&mut store, src.path())
        }));
        drop(reset);
        assert!(interrupted.is_err());
        drop(store);

        // Reopening proves the stale rule node AND the source digest were committed before
        // interruption; an ordinary digest-based retry alone cannot remove the node.
        let mut store = SqliteStore::open(&dbpath).unwrap();
        for key in ["indexed_version", "extra_rules_digest"] {
            assert!(
                store
                    .meta_get_key(key)
                    .is_some_and(|value| value.starts_with(PENDING_FULL_PREFIX)),
                "{key} must stay pending after interruption"
            );
        }
        assert!(
            GraphRead::all_nodes(&store)
                .unwrap()
                .iter()
                .any(|node| node.symbol.as_str().contains("reviewmarker"))
        );
        let bytes = std::fs::read(src.path().join("a.rs")).unwrap();
        assert_eq!(
            store.file_digest("a.rs").unwrap(),
            Some(source_digest(&bytes, None))
        );
        std::fs::remove_file(rule).unwrap();
        index_path(&mut store, src.path()).unwrap();
        let mut fresh = SqliteStore::in_memory().unwrap();
        index_path(&mut fresh, src.path()).unwrap();
        assert_same_extracted_graph(&store, &fresh);
    }

    /// #476: after the upgrade pass, a persistently unreadable file must not make unrelated
    /// readable files re-extract on each retry. Exercise each successful exit on run two.
    fn persistent_read_failure_retries_only_recorded_file(exit: &str) {
        let src = tempfile::tempdir().unwrap();
        for (file, name) in [("a.rs", "readable"), ("b.rs", "blocked"), ("c.rs", "gone")] {
            std::fs::write(src.path().join(file), format!("pub fn {name}() {{}}\n")).unwrap();
        }
        let dbdir = tempfile::tempdir().unwrap();
        let dbpath = dbdir.path().join("g.db");
        let mut store = SqliteStore::open(&dbpath).unwrap();
        index_path(&mut store, src.path()).unwrap();
        store.meta_set_key("indexed_version", "0.0.0-older");
        let blocked = ReadFailGuard::new(src.path().join("b.rs"));
        index_path(&mut store, src.path()).unwrap();
        let side = rusqlite::Connection::open(&dbpath).unwrap();
        assert_eq!(
            side.execute(
                "UPDATE nodes SET name = 'probe_readable' WHERE file = 'a.rs' AND name = 'readable'",
                [],
            )
            .unwrap(),
            1
        );
        match exit {
            "no-change" => {}
            "deletion-only" => std::fs::remove_file(src.path().join("c.rs")).unwrap(),
            "normal-end" => {
                std::fs::write(src.path().join("d.rs"), "pub fn added() {}\n").unwrap();
            }
            _ => panic!("unknown exit: {exit}"),
        }
        UNREADABLE_FILES.with(|notes| notes.borrow_mut().clear());
        index_path(&mut store, src.path()).unwrap();
        let untouched: usize = side
            .query_row(
                "SELECT count(*) FROM nodes WHERE name = 'probe_readable'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(untouched, 1, "run two must not re-extract a.rs ({exit})");
        assert_eq!(
            store.meta_get_key("indexed_version").as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(
            store.meta_get_key("unreadable_files").as_deref(),
            Some("b.rs\tread failed")
        );
        UNREADABLE_FILES.with(|notes| {
            assert_eq!(
                &*notes.borrow(),
                &[("b.rs".to_string(), "read failed".to_string())],
                "one diagnostic per recorded file per run"
            );
        });
        // Recovery must force just b.rs, even though its bytes/digest never changed.
        assert_eq!(
            side.execute(
                "UPDATE nodes SET name = 'old_blocked' WHERE name = 'blocked'",
                []
            )
            .unwrap(),
            1
        );
        drop(blocked);
        UNREADABLE_FILES.with(|notes| notes.borrow_mut().clear());
        index_path(&mut store, src.path()).unwrap();
        let count_blocked = |name: &str| -> usize {
            side.query_row("SELECT count(*) FROM nodes WHERE name = ?1", [name], |r| {
                r.get(0)
            })
            .unwrap()
        };
        assert_eq!(count_blocked("blocked"), 1, "recovery must re-extract b.rs");
        assert_eq!(count_blocked("old_blocked"), 0);
        let untouched: usize = side
            .query_row(
                "SELECT count(*) FROM nodes WHERE name = 'probe_readable'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(untouched, 1, "recovery must not re-extract a.rs");
        assert_eq!(store.meta_get_key("unreadable_files").as_deref(), Some(""));
        UNREADABLE_FILES.with(|notes| assert!(notes.borrow().is_empty()));
    }

    #[test]
    fn sqlite_persistent_read_failure_no_change_retry_is_incremental() {
        persistent_read_failure_retries_only_recorded_file("no-change");
    }

    #[test]
    fn sqlite_persistent_read_failure_deletion_only_retry_is_incremental() {
        persistent_read_failure_retries_only_recorded_file("deletion-only");
    }

    #[test]
    fn sqlite_persistent_read_failure_normal_end_retry_is_incremental() {
        persistent_read_failure_retries_only_recorded_file("normal-end");
    }

    #[test]
    fn sqlite_version_change_deletion_only_records_unreadable_file() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("a.rs"), "pub fn blocked() {}\n").unwrap();
        std::fs::write(src.path().join("b.rs"), "pub fn gone() {}\n").unwrap();
        let mut store = SqliteStore::in_memory().unwrap();
        index_path(&mut store, src.path()).unwrap();
        std::fs::remove_file(src.path().join("b.rs")).unwrap();
        store.meta_set_key("indexed_version", "0.0.0-older");
        let blocked = ReadFailGuard::new(src.path().join("a.rs"));
        index_path(&mut store, src.path()).unwrap();
        assert_eq!(
            store.meta_get_key("indexed_version").as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(
            store.meta_get_key("unreadable_files").as_deref(),
            Some("a.rs\tread failed")
        );
        assert!(store.file_digest("b.rs").unwrap().is_none());
        drop(blocked);
        index_path(&mut store, src.path()).unwrap();
        assert_eq!(store.meta_get_key("unreadable_files").as_deref(), Some(""));
    }

    #[test]
    fn sqlite_unreadable_records_are_sorted_and_repo_scoped() {
        let src = tempfile::tempdir().unwrap();
        for file in ["b.rs", "a.rs"] {
            std::fs::write(src.path().join(file), "pub fn kept() {}\n").unwrap();
        }
        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join("other.rs"), "pub fn other() {}\n").unwrap();
        let mut store = SqliteStore::in_memory().unwrap();
        index_path_as(&mut store, src.path(), Some("ledger")).unwrap();
        index_path_as(&mut store, other.path(), Some("other")).unwrap();
        let other_version = repo_scope::meta_key(Some("other"), "indexed_version");
        let key = repo_scope::meta_key(Some("ledger"), "unreadable_files");
        store.meta_set_key(
            &repo_scope::meta_key(Some("ledger"), "indexed_version"),
            "older",
        );
        let blocked_a = ReadFailGuard::new(src.path().join("a.rs"));
        let blocked_b = ReadFailGuard::new(src.path().join("b.rs"));
        index_path_as(&mut store, src.path(), Some("ledger")).unwrap();
        assert_eq!(
            store.meta_get_key(&key).as_deref(),
            Some("ledger/a.rs\tread failed\nledger/b.rs\tread failed")
        );
        index_path_as(&mut store, other.path(), Some("other")).unwrap();
        assert_eq!(
            store.meta_get_key(&key).as_deref(),
            Some("ledger/a.rs\tread failed\nledger/b.rs\tread failed")
        );
        assert_eq!(
            store.meta_get_key(&other_version).as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        // A deleted file leaves the retry record, while the still-unreadable file remains.
        std::fs::remove_file(src.path().join("b.rs")).unwrap();
        index_path_as(&mut store, src.path(), Some("ledger")).unwrap();
        assert_eq!(
            store.meta_get_key(&key).as_deref(),
            Some("ledger/a.rs\tread failed")
        );
        drop((blocked_a, blocked_b));
        index_path_as(&mut store, src.path(), Some("ledger")).unwrap();
        assert_eq!(store.meta_get_key(&key).as_deref(), Some(""));
    }

    /// #471 / #476: an upgrade records a skipped file while certifying current markers. A plain
    /// retry must re-extract that file once readable, keeping its old rows and reviewed fields
    /// until then.
    #[test]
    fn sqlite_version_change_read_failure_plain_retry_reextracts() {
        let src = tempfile::tempdir().unwrap();
        let file = src.path().join("a.rs");
        std::fs::write(&file, "pub fn pay() {}\n").unwrap();
        let dbdir = tempfile::tempdir().unwrap();
        let dbpath = dbdir.path().join("g.db");

        // One long-lived store for the blocked run and the retry — exactly what `watch` keeps.
        let mut store = SqliteStore::open(&dbpath).unwrap();
        index_path(&mut store, src.path()).unwrap();
        let pay = sym_of(&store, "pay");
        let fields = annotate_with_claim(&mut store, &pay);

        // Read the `nodes.name` column directly: `find_symbols` goes through an FTS index a raw
        // UPDATE does not refresh, so the raw column is the honest view of what extraction wrote.
        let count_name = |name: &str| -> i64 {
            rusqlite::Connection::open(&dbpath)
                .unwrap()
                .query_row("SELECT COUNT(*) FROM nodes WHERE name = ?1", [name], |r| {
                    r.get(0)
                })
                .unwrap()
        };
        assert_eq!(count_name("pay"), 1);

        // Simulate an older extractor's output without changing the source or its digest. Only a
        // full re-extraction can replace this stale name with `pay`.
        rusqlite::Connection::open(&dbpath)
            .unwrap()
            .execute("UPDATE nodes SET name = 'old_pay' WHERE name = 'pay'", [])
            .unwrap();

        // Phase 2: a version-triggered re-index during which the file cannot be read. The read
        // fails in the work list, the file stays in `current_rel_paths` (it is not deleted), and
        // the run takes the no-change exit.
        store.meta_set_key("indexed_version", "0.0.0-older"); // force full re-extract
        {
            let _blocked = ReadFailGuard::new(file.clone());
            index_path(&mut store, src.path()).unwrap();
        }
        assert_eq!(
            count_name("old_pay"),
            1,
            "the blocked run must keep the skipped file's old rows, not delete them"
        );
        assert_eq!(
            count_name("pay"),
            0,
            "the skipped file must not have been re-extracted during the blocked run"
        );
        assert_eq!(
            store.node_semantics(&pay).unwrap(),
            Some(fields.clone()),
            "the blocked run must keep the reviewed fields of the skipped file's node"
        );
        assert_eq!(
            store.meta_get_key("indexed_version").as_deref(),
            Some(env!("CARGO_PKG_VERSION")),
            "a successful run certifies the current version"
        );
        assert_eq!(
            store.meta_get_key("unreadable_files").as_deref(),
            Some("a.rs\tread failed"),
            "the skipped file must be recorded for a plain retry"
        );

        // Phase 3: a plain retry, with NO metadata reset, must re-extract the recorded file.
        index_path(&mut store, src.path()).unwrap();
        assert_eq!(
            count_name("pay"),
            1,
            "a plain retry must re-extract the file skipped by the read failure"
        );
        assert_eq!(
            count_name("old_pay"),
            0,
            "the stale output must be gone after the retry"
        );
        assert_eq!(
            store.node_semantics(&pay).unwrap(),
            Some(fields),
            "the reviewed fields survive the re-extraction"
        );
        assert_eq!(
            store.meta_get_key("indexed_version").as_deref(),
            Some(env!("CARGO_PKG_VERSION")),
            "a complete run certifies the current version"
        );
        assert_eq!(store.meta_get_key("unreadable_files").as_deref(), Some(""));
    }

    /// #471 (fix, rollback): change the extra-edge rules while a previously indexed file cannot be
    /// read, then roll the rules back before retrying. Keeping the OLD digest on the incomplete
    /// run let the rollback match it, so the retry skipped re-extraction and the readable file
    /// kept a node only the rolled-back rule creates. Stamping the current rules makes rollback
    /// mismatch them, so the retry re-extracts and equals a fresh index under the same rules.
    #[test]
    fn sqlite_rules_change_with_skipped_file_then_rollback_retry_reextracts() {
        fn markers(store: &SqliteStore) -> usize {
            GraphRead::all_nodes(store)
                .unwrap()
                .iter()
                .filter(|n| n.symbol.as_str().contains("reviewmarker"))
                .count()
        }
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("a.rs"), "pub fn keep() {}\n").unwrap();
        let b = src.path().join("b.rs");
        std::fs::write(&b, "pub fn blocked() {}\n").unwrap();
        let mut store = SqliteStore::in_memory().unwrap();
        index_path(&mut store, src.path()).unwrap();
        assert_eq!(markers(&store), 0);

        // Rules R1: one rule that emits a node for `keep` in a.rs.
        let rule_dir = src.path().join(".wicked-estate-extractors");
        std::fs::create_dir(&rule_dir).unwrap();
        let rule = rule_dir.join("review.toml");
        std::fs::write(
            &rule,
            r#"[[rule]]
name = "reviewmarker"
file_glob = "a.rs"
pattern = '(?P<n>keep)'
[rule.emit_node]
id_template = "reviewmarker:{n}"
label_capture = "n"
kind = "synthetic"
node_scheme = "reviewmarker"
"#,
        )
        .unwrap();
        let (_, current_digest) = load_extra_edge_rules(src.path());
        {
            let _blocked = ReadFailGuard::new(b.clone());
            index_path(&mut store, src.path()).unwrap();
        }
        assert_eq!(
            markers(&store),
            1,
            "the readable file is extracted under R1"
        );
        assert_eq!(
            store.meta_get_key("extra_rules_digest"),
            Some(current_digest),
            "a successful run stamps current rules even when a file was skipped"
        );
        assert_eq!(
            store.meta_get_key("unreadable_files").as_deref(),
            Some("b.rs\tread failed")
        );

        // Roll the rules back to R0 (no rules) and retry with b.rs readable again.
        std::fs::remove_file(&rule).unwrap();
        index_path(&mut store, src.path()).unwrap();

        let mut fresh = SqliteStore::in_memory().unwrap();
        index_path(&mut fresh, src.path()).unwrap();
        assert_eq!(markers(&fresh), 0);
        assert_same_extracted_graph(&store, &fresh);
        assert_eq!(
            markers(&store),
            markers(&fresh),
            "after the rollback the retry must re-extract: the graph equals a fresh index"
        );
        assert_eq!(
            store.meta_get_key("extra_rules_digest"),
            fresh.meta_get_key("extra_rules_digest"),
            "a complete run certifies the current rules"
        );
    }

    fn removed_symbol_is_dropped_and_kept_one_survives(store: &mut dyn GraphStoreMutExt) {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("a.rs");
        std::fs::write(&file, "pub fn keep() {}\npub fn gone() {}\n").unwrap();
        index_path(store, tmp.path()).unwrap();
        let (keep, gone) = (sym_of(store, "keep"), sym_of(store, "gone"));
        let kept = annotate_with_claim(store, &keep);
        annotate_with_claim(store, &gone);

        SEMANTICS_DROPS.with(|notes| notes.borrow_mut().clear());
        std::fs::write(&file, "pub fn keep() {}\n").unwrap();
        index_path(store, tmp.path()).unwrap();
        SEMANTICS_DROPS.with(|notes| {
            assert_eq!(
                &*notes.borrow(),
                &["SEMANTICS-DROPPED: 1 symbol(s) in a.rs"]
            );
        });

        assert_eq!(store.node_semantics(&keep).unwrap(), Some(kept));
        assert!(store.get_node(&gone).unwrap().is_none());
    }

    #[test]
    fn restore_semantics_counts_symbols_that_are_gone() {
        let mut store = MemStore::new();
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "pub fn keep() {}\n").unwrap();
        index_path(&mut store, tmp.path()).unwrap();
        let sem = NodeSemantics {
            requirement: Some("REQ-1".into()),
            ..Default::default()
        };
        let saved = vec![
            ("a.rs".to_string(), sym_of(&store, "keep"), sem.clone()),
            (
                "a.rs".to_string(),
                SymbolId("a.rs::gone".into()),
                sem.clone(),
            ),
            ("b.rs".to_string(), SymbolId("b.rs::x".into()), sem),
        ];
        let dropped = restore_semantics(&mut store, saved).unwrap();
        assert_eq!(dropped.get("a.rs"), Some(&1), "{dropped:?}");
        assert_eq!(dropped.get("b.rs"), Some(&1), "{dropped:?}");
    }

    /// A symbol whose file was removed reads back None BEFORE restore, and restore leaves it that
    /// way (counted dropped) because `get_node` finds it gone — on SQLite and MemStore.
    fn dropped_reads_none_after_remove_before_restore(store: &mut dyn GraphStoreMutExt) {
        let node = Node::new(
            SymbolId("a.rs::f".into()),
            NodeKind::Function,
            "f",
            Language::new("rust"),
            Location::new("a.rs", Span::ZERO),
        );
        store.upsert_nodes(std::slice::from_ref(&node)).unwrap();
        let claim = ValidationClaim::new(true, "alice").unwrap();
        store
            .set_node_semantics(&node.symbol, Some("d"), Some("REQ-1"), Some(&claim))
            .unwrap();
        let snapshot = store.semantics_in_files(&["a.rs"]).unwrap();
        assert_eq!(snapshot.len(), 1, "one annotated node in a.rs");
        store.remove_file("a.rs").unwrap();
        assert!(
            store.node_semantics(&node.symbol).unwrap().is_none(),
            "node_semantics must read None after remove_file and before restore"
        );
        let dropped = restore_semantics(store, snapshot).unwrap();
        assert_eq!(dropped.get("a.rs"), Some(&1), "{dropped:?}");
        assert!(
            store.node_semantics(&node.symbol).unwrap().is_none(),
            "restore must not resurrect a symbol whose file is gone"
        );
    }

    #[test]
    fn sqlite_dropped_reads_none_after_remove_before_restore() {
        dropped_reads_none_after_remove_before_restore(&mut SqliteStore::in_memory().unwrap());
    }

    #[test]
    fn mem_dropped_reads_none_after_remove_before_restore() {
        dropped_reads_none_after_remove_before_restore(&mut MemStore::new());
    }

    #[test]
    fn sqlite_dropped_symbol_loses_fields_kept_one_survives() {
        removed_symbol_is_dropped_and_kept_one_survives(&mut SqliteStore::in_memory().unwrap());
    }

    #[test]
    fn mem_dropped_symbol_loses_fields_kept_one_survives() {
        removed_symbol_is_dropped_and_kept_one_survives(&mut MemStore::new());
    }

    #[test]
    fn validation_only_claim_reads_back() {
        let mut store = SqliteStore::in_memory().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "pub fn pay() {}\n").unwrap();
        index_path(&mut store, tmp.path()).unwrap();
        let pay = sym_of(&store, "pay");
        let retracted = ValidationClaim::new(false, "bob").unwrap();
        store
            .set_node_semantics(&pay, None, None, Some(&retracted))
            .unwrap();
        let got = store
            .node_semantics(&pay)
            .unwrap()
            .expect("a retracted claim is still a claim");
        assert_eq!(got.requirement_validated_by.as_deref(), Some("bob"));
        assert!(!got.requirement_validated);
    }
}
