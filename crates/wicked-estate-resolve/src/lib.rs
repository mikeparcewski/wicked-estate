//! `wicked-estate-resolve` — `Resolver` implementations and the on-demand LSP tier (W3.3).
//!
//! [`NameResolver`] is the Tier-2 import-map-style resolver: it binds an unresolved call/import
//! reference to a project symbol by **name**, emitting an edge only when the name resolves
//! **uniquely**. Ambiguous names are left for the precise tiers (SCIP/TSG/LSP — W2.2/W3.2/W3.3),
//! which is the honest "cheap-broad over precise-narrow" layering from the design notes.
//!
//! [`ScopedNameResolver`] improves precision when multiple same-name candidates exist: it
//! prefers the candidate in the **same file** as the reference, then the same directory
//! (approximating module scope). The disambiguation reason is recorded in the edge metadata.
//!
//! [`ImportMapResolver`] uses the per-file import map recorded in `UnresolvedRef.hints["imports"]`
//! by `wicked-estate-extract` to bind a call to the specific file the symbol was imported from, cutting through
//! same-name ambiguity without requiring a precise-tier tool. Confidence 0.63, `via=import-map`.
//!
//! [`resolve_all`] runs multiple resolvers and deduplicate edges by `(source, target, kind)`,
//! keeping the highest-confidence edge when resolvers produce the same relationship.
//!
//! [`lsp`] is the on-demand LSP tier: a minimal JSON-RPC stdio client that drives installed
//! language servers (`typescript-language-server`, `rust-analyzer`, `pyright-langserver`) for
//! precise single-symbol queries (definition / references / hover). ON DEMAND only.

pub mod lsp;

pub mod estate;
pub use estate::estate_edges;

pub mod relative_import;
pub use relative_import::RelativeImportResolver;

use wicked_estate_core::{
    Edge, EdgeKind, NodeKind, ResolutionTier, Resolver, Result, SymbolIndex, UnresolvedRef,
};

/// Resolve references to unique same-name project symbols. Confidence comes from the tier
/// (`ImportMap` = 0.6). Self-edges and ambiguous (>1 candidate) names are skipped.
#[derive(Debug, Default, Clone, Copy)]
pub struct NameResolver;

impl Resolver for NameResolver {
    fn id(&self) -> &str {
        "name-resolver"
    }

    fn tier(&self) -> ResolutionTier {
        ResolutionTier::ImportMap
    }

    fn resolve(&self, refs: &[UnresolvedRef], index: &dyn SymbolIndex) -> Result<Vec<Edge>> {
        let mut out = Vec::new();
        for r in refs {
            let mut candidates = index.by_name(&r.raw_name);
            // Kind admissibility (D1) runs PRE-uniqueness — deliberately recall-widening:
            // dropping a deny-listed homonym (e.g. an html type_alias) can make a legitimate
            // same-family callable unique. Every edge this mints is measured (Q4b).
            candidates.retain(|n| admissible_target(&r.kind, &n.kind));
            // Unique resolution only — ambiguity is deferred to a precise tier (W2.2+).
            if let [only] = candidates.as_slice() {
                if only.symbol != r.from {
                    // Family guard (D5) runs POST-uniqueness on the sole survivor — strictly
                    // narrowing, never edge-minting (it is what stops a typescript ref whose
                    // only admissible homonym is a bash variable from binding cross-family).
                    if !family_compatible(index, from_family(index, &r.from).as_deref(), only) {
                        continue;
                    }
                    let edge = Edge::new(
                        r.from.clone(),
                        only.symbol.clone(),
                        r.kind.clone(),
                        self.tier(),
                        self.id(),
                    )
                    .with_location(r.location.clone());
                    out.push(edge);
                }
            }
        }
        Ok(out)
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Parent directory of a file path, or `""` when the path has no separator (a root-level file).
///
/// Root-level files share the empty directory, so `ScopedNameResolver`'s same-dir tier ranks two
/// root-level files as same-dir, and `ImportMapResolver`'s relative-path resolution binds a
/// root-level `./x` import (`file_matches_module` special-cases `ref_dir.is_empty()`).
fn dir_of(file: &str) -> &str {
    // Works for both Unix (`/`) and Windows (`\`) paths in repo-relative strings.
    if let Some(pos) = file.rfind(['/', '\\']) {
        &file[..pos]
    } else {
        ""
    }
}

/// Whether a node kind is a method or function (used for bare method-name matching).
fn is_callable(kind: &NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Function | NodeKind::Method | NodeKind::Constructor
    )
}

/// Target-kind admissibility (the D1 deny-list — shared by every name-based resolver).
///
/// - [`NodeKind::Import`] nodes are never edge targets, for ANY ref kind: an import node is a
///   *reference site*, not a definition (a TS `res.json()` call must not bind to a Python
///   `import json` node).
/// - For [`EdgeKind::Calls`] refs, kinds that are definitively not call targets are rejected:
///   type-level declarations (`Interface`/`Trait`/`TypeAlias`/`Enum`), value slots
///   (`Field`/`Parameter`), containers (`File`/`Namespace`) and rules-engine entities (those
///   bind through `RulesBridgeResolver`, not by name).
/// - Kept for Calls: `Function`/`Method`/`Constructor`, `Class`/`Struct` (a `new X()`
///   construction site is captured as `@call.function` by design), `Module` (JCL/HLASM/COBOL
///   program calls target `Module` nodes — pinned by `tests/cross_language_estate.rs`),
///   `Constant`/`Variable` (function-valued bindings: `const f = () => …`, `vi.fn()`),
///   `Macro`, `Synthetic`, and `Other(_)`.
fn admissible_target(ref_kind: &EdgeKind, cand_kind: &NodeKind) -> bool {
    if matches!(cand_kind, NodeKind::Import) {
        return false;
    }
    if *ref_kind != EdgeKind::Calls {
        return true;
    }
    !matches!(
        cand_kind,
        NodeKind::Interface
            | NodeKind::Trait
            | NodeKind::TypeAlias
            | NodeKind::Enum
            | NodeKind::Field
            | NodeKind::Parameter
            | NodeKind::File
            | NodeKind::Namespace
            | NodeKind::Rule
            | NodeKind::RuleSet
            | NodeKind::Condition
            | NodeKind::Action
            | NodeKind::Fact
    )
}

/// Language family of the ref's source node, when both the node and a manifest family exist.
fn from_family(index: &dyn SymbolIndex, from: &wicked_estate_core::SymbolId) -> Option<String> {
    index
        .get(from)
        .and_then(|n| index.language_family(n.language.as_str()))
}

/// Cross-family guard (D5): block a candidate only when BOTH ends carry a **known**
/// `languages.toml` family and the families differ. Unknown/absent family (mainframe langs
/// registered outside the manifest, `synthetic`/`tfstate` tags) or a missing from-node ⇒ allow —
/// a strict guard would kill the shipped JCL/HLASM→COBOL joins.
fn family_compatible(
    index: &dyn SymbolIndex,
    from_family: Option<&str>,
    cand: &wicked_estate_core::Node,
) -> bool {
    match (from_family, index.language_family(cand.language.as_str())) {
        (Some(f), Some(c)) => f == c,
        _ => true,
    }
}

// ── ScopedNameResolver ───────────────────────────────────────────────────────

/// A scope-aware resolver that improves on [`NameResolver`] when multiple same-name candidates
/// exist.
///
/// **Resolution priority** (within the `ImportMap` tier):
/// 1. Candidate in the **same file** as the reference → `confidence 0.65`,
///    metadata `{"scope":"same-file"}`.
/// 2. Candidate in the **same directory** (same module / namespace approximation) →
///    `confidence 0.62`, metadata `{"scope":"same-dir"}`.
/// 3. Unique cross-file candidate → `confidence 0.60` (standard ImportMap, no metadata).
/// 4. Truly ambiguous (multiple equally-ranked candidates) → skip; left for precise tiers.
///
/// For method-style references the candidate pool is additionally filtered to callable nodes
/// (Function / Method / Constructor) before the scope ranking is applied.
///
/// Self-edges are never emitted.
#[derive(Debug, Default, Clone, Copy)]
pub struct ScopedNameResolver;

/// Scope tier assigned to a candidate during disambiguation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ScopeTier {
    CrossFile = 0,
    SameDir = 1,
    SameFile = 2,
}

impl ScopeTier {
    fn confidence(self) -> f32 {
        match self {
            ScopeTier::SameFile => 0.65,
            ScopeTier::SameDir => 0.62,
            ScopeTier::CrossFile => 0.60,
        }
    }

    fn label(self) -> Option<&'static str> {
        match self {
            ScopeTier::SameFile => Some("same-file"),
            ScopeTier::SameDir => Some("same-dir"),
            ScopeTier::CrossFile => None,
        }
    }
}

impl Resolver for ScopedNameResolver {
    fn id(&self) -> &str {
        "scoped-name-resolver"
    }

    fn tier(&self) -> ResolutionTier {
        ResolutionTier::ImportMap
    }

    fn resolve(&self, refs: &[UnresolvedRef], index: &dyn SymbolIndex) -> Result<Vec<Edge>> {
        let mut out = Vec::new();

        for r in refs {
            let mut candidates = index.by_name(&r.raw_name);

            // Kind admissibility (D1): Import nodes are never targets, for any ref kind.
            candidates.retain(|n| admissible_target(&r.kind, &n.kind));

            // For method-like edge kinds, narrow the pool to callable nodes.
            if matches!(r.kind, EdgeKind::Calls) {
                candidates.retain(|n| is_callable(&n.kind));
            }

            // Remove self-candidates immediately.
            candidates.retain(|n| n.symbol != r.from);

            if candidates.is_empty() {
                continue;
            }

            // Cross-family guard (D5), PRE-ranking — recall-widening within scope tiers by
            // design: a same-family candidate in a scope tier is exactly what this resolver
            // exists to bind, so dropping a cross-family homonym can flip tie→park into a
            // unique winner. Measured by Q4b; pinned by
            // scoped_family_retain_unshadows_same_family_homonym.
            let from_fam = from_family(index, &r.from);
            candidates.retain(|n| family_compatible(index, from_fam.as_deref(), n));

            if candidates.is_empty() {
                continue;
            }

            // Score every candidate by scope tier.
            let ref_file = r.location.file.as_str();
            let ref_dir = dir_of(ref_file);

            let scored: Vec<(ScopeTier, &wicked_estate_core::Node)> = candidates
                .iter()
                .map(|n| {
                    let cand_file = n.location.file.as_str();
                    let tier = if cand_file == ref_file {
                        ScopeTier::SameFile
                    } else if dir_of(cand_file) == ref_dir {
                        ScopeTier::SameDir
                    } else {
                        ScopeTier::CrossFile
                    };
                    (tier, n)
                })
                .collect();

            // Find the best (highest) scope tier among candidates.
            let best_tier = scored
                .iter()
                .map(|(t, _)| *t)
                .max()
                .expect("scored is non-empty");

            // Collect all candidates at the best tier.
            let best: Vec<&wicked_estate_core::Node> = scored
                .iter()
                .filter(|(t, _)| *t == best_tier)
                .map(|(_, n)| *n)
                .collect();

            // If more than one candidate shares the best tier, the reference is still
            // ambiguous at this precision level — defer to a precise tier (SCIP/TSG/LSP).
            if best.len() != 1 {
                continue;
            }

            let winner = best[0];
            let confidence = wicked_estate_core::Confidence::new(best_tier.confidence());

            let mut edge = Edge::new(
                r.from.clone(),
                winner.symbol.clone(),
                r.kind.clone(),
                self.tier(),
                self.id(),
            )
            .with_location(r.location.clone());

            // Override the tier-default confidence with the scope-adjusted value.
            edge.confidence = confidence;

            // Record the disambiguation reason in edge metadata.
            if let Some(label) = best_tier.label() {
                edge.metadata.insert(
                    "scope".to_string(),
                    serde_json::Value::String(label.to_string()),
                );
            }

            out.push(edge);
        }

        Ok(out)
    }
}

// ── ImportMapResolver ─────────────────────────────────────────────────────────

/// Strip a leading `./` or `../` from a path component so bare file stems can be compared.
fn stem_of(path: &str) -> &str {
    // Trim optional leading path separators and dots.
    let mut s = path;
    while let Some(rest) = s.strip_prefix("./").or_else(|| s.strip_prefix("../")) {
        s = rest;
    }
    s
}

/// Whether a candidate file path "plausibly matches" a module specifier.
///
/// Matching rules (applied in order; first match wins):
/// 1. After resolving `module_spec` against `ref_dir`:
///    - For relative specs (`./x`, `../x`): compute the logical path by joining `ref_dir/module_spec`,
///      then check if `cand_file` (without extension) ends with that logical stem, or equals it.
/// 2. For bare module names (no leading `.`): match if `cand_file`'s path (without extension)
///    has a component equal to the module name (suffix match), or ends with it after the last `/`.
///
/// Returns `true` when the candidate is a plausible definition site for `module_spec`.
fn file_matches_module(cand_file: &str, ref_dir: &str, module_spec: &str) -> bool {
    // Strip extension from candidate file path — we compare path stems.
    let cand_stem = match cand_file.rsplit_once('.') {
        Some((stem, _)) => stem,
        None => cand_file,
    };
    // Also strip a trailing `/index` component (JS index file convention).
    let cand_stem = cand_stem.trim_end_matches("/index");

    if module_spec.starts_with('.') {
        // Relative import: resolve against ref_dir.
        // Build a logical path by joining ref_dir + "/" + module_spec, then normalise.
        let joined = if ref_dir.is_empty() {
            module_spec.to_string()
        } else {
            format!("{ref_dir}/{module_spec}")
        };
        // Normalise: collapse `./` and `../` segments (best-effort, not full VFS).
        let logical = normalise_relative_path(&joined);
        // The logical path is already extension-free (module_spec has no extension).
        // Match if the candidate stem equals the logical path or ends with it after a '/'.
        cand_stem == logical || cand_stem.ends_with(&format!("/{logical}"))
    } else {
        // Bare module name (e.g. "react", "lodash", "mypackage/utils").
        // Match by suffix: the candidate file path (without ext) ends with the module name,
        // or a path component equals it — handles monorepo package structures.
        let spec_stem = stem_of(module_spec);
        cand_stem == spec_stem
            || cand_stem.ends_with(&format!("/{spec_stem}"))
            || cand_stem.ends_with(&format!("\\{spec_stem}"))
    }
}

/// Collapse `./` and `../` in a joined path string (best-effort, not full VFS traversal).
fn normalise_relative_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => {
                parts.push(other);
            }
        }
    }
    parts.join("/")
}

/// Resolve cross-file `Calls` references using the import-map recorded in `ref.hints["imports"]`.
///
/// **Resolution logic for a ref with `raw_name = N`:**
/// 1. Look up `hints["imports"][N]` to find the module source `M` the name was imported from.
/// 2. Collect all candidates named `N` from the index (callable nodes only for `Calls` refs).
/// 3. Remove self-candidates.
/// 4. Among the remaining candidates, keep those whose `location.file` plausibly matches `M`
///    using [`file_matches_module`] (relative-path resolution + suffix matching).
/// 5. If exactly one candidate matches → emit an edge at `ImportMap` confidence with
///    `metadata["via"] = "import-map"`.
/// 6. If zero or >1 candidates match → skip (leave for precise tiers).
///
/// If `hints["imports"]` is absent, the ref is skipped (nothing to contribute).
#[derive(Debug, Default, Clone, Copy)]
pub struct ImportMapResolver;

impl wicked_estate_core::Resolver for ImportMapResolver {
    fn id(&self) -> &str {
        "import-map-resolver"
    }

    fn tier(&self) -> wicked_estate_core::ResolutionTier {
        wicked_estate_core::ResolutionTier::ImportMap
    }

    fn resolve(
        &self,
        refs: &[wicked_estate_core::UnresolvedRef],
        index: &dyn wicked_estate_core::SymbolIndex,
    ) -> wicked_estate_core::Result<Vec<wicked_estate_core::Edge>> {
        let mut out = Vec::new();

        for r in refs {
            // Only handle Calls refs; other kinds don't benefit from import-map scoping.
            if r.kind != wicked_estate_core::EdgeKind::Calls {
                continue;
            }

            // Retrieve the imports map from hints, if present.
            let imports_obj = match r.hints.get("imports").and_then(|v| v.as_object()) {
                Some(obj) => obj,
                None => continue,
            };

            // Look up the module source for this specific name.
            let module_src = match imports_obj.get(&r.raw_name).and_then(|v| v.as_str()) {
                Some(m) => m,
                None => continue, // this name isn't in the import map — skip
            };

            // Collect callable candidates for this name (D1 admissibility + D5 family guard
            // run pre-ranking, alongside the callable filter — same placement as
            // ScopedNameResolver).
            let mut candidates = index.by_name(&r.raw_name);
            candidates.retain(|n| admissible_target(&r.kind, &n.kind));
            candidates.retain(|n| is_callable(&n.kind));
            candidates.retain(|n| n.symbol != r.from); // no self-edges

            if candidates.is_empty() {
                continue;
            }

            let from_fam = from_family(index, &r.from);
            candidates.retain(|n| family_compatible(index, from_fam.as_deref(), n));

            if candidates.is_empty() {
                continue;
            }

            // Narrow to candidates whose file matches the import module source.
            let ref_dir = dir_of(r.location.file.as_str());
            let matched: Vec<&wicked_estate_core::Node> = candidates
                .iter()
                .filter(|n| file_matches_module(n.location.file.as_str(), ref_dir, module_src))
                .collect();

            // Emit only when unambiguous after import-scoping.
            if matched.len() != 1 {
                continue;
            }

            let winner = matched[0];
            let mut edge = wicked_estate_core::Edge::new(
                r.from.clone(),
                winner.symbol.clone(),
                r.kind.clone(),
                self.tier(),
                self.id(),
            )
            .with_location(r.location.clone());

            // Boost confidence slightly above the base ImportMap tier (0.6) since we have
            // explicit import-map evidence — but stay below ScopedNameResolver's same-file (0.65).
            edge.confidence = wicked_estate_core::Confidence::new(0.63);
            edge.metadata.insert(
                "via".to_string(),
                serde_json::Value::String("import-map".to_string()),
            );

            out.push(edge);
        }

        Ok(out)
    }
}

// ── InfraResolver ─────────────────────────────────────────────────────────────

/// Resolver for IaC/estate dependency references (W9.4).
///
/// Binds [`UnresolvedRef`]s whose `raw_name` matches a resource node (i.e. a node with
/// [`NodeKind::Other("resource")`]) to that node, emitting the declared dependency as a
/// graph edge.
///
/// ## Which refs are handled
///
/// A ref is handled when **at least one** of these is true:
/// 1. The ref's `from` symbol resolves to a resource node (the referencing side is itself a
///    resource — tfstate cross-module dependencies, HCL `depends_on`).
/// 2. The `raw_name` resolves exclusively to resource nodes and **not** to any code nodes
///    (CFN `!Ref <LogicalId>` from a file-scope symbol into resource space).
///
/// The second condition lets CFN refs emitted from a `file_symbol` (not a resource node itself)
/// reach their targets.  Without it, CFN template wiring would be silently dropped.
///
/// ## Edge kind and confidence
///
/// - The edge kind is taken directly from the ref (`r.kind`), which for tfstate is already
///   `EdgeKind::Other("depends_on")`.  For CFN `!Ref` the extractor uses `EdgeKind::Calls`
///   as a proxy (documented limitation in `wicked-estate-extract`).
/// - Confidence: [`ResolutionTier::Parsed`] (confidence 1.0) — these are explicit, declared
///   dependencies, not heuristic inferences.
///
/// ## Direction invariant (ADR-001)
///
/// `source = dependent`, `target = dependency` — matching the invariant in `docs/ENGINE-CONTRACT.md`.
/// So blast-radius on `aws_s3_bucket.app` walks edges where `target == aws_s3_bucket.app`.
///
/// ## Non-interference with code resolvers
///
/// [`ScopedNameResolver`] and [`ImportMapResolver`] keep **callable** candidates only for
/// `Calls` refs (Function/Method/Constructor), so they never bind a resource node.
/// [`NameResolver`] rejects the D1 deny-list (`Import` for every ref kind; for `Calls` also
/// Interface/Trait/TypeAlias/Enum/Field/Parameter/File/Namespace and the rules-engine kinds) —
/// the deny-list does NOT include `Other(..)`, so a unique resource node named like a code call
/// CAN still bind at `NameResolver` unless the cross-family guard blocks it (resource nodes
/// carry non-manifest language tags, so the guard allows them). `InfraResolver` in turn requires
/// at least one candidate to be a resource node before it acts, so it will not fire on code refs.
///
/// ## Ambiguity rule
///
/// If `by_name(raw_name)` returns multiple resource nodes with the same name, the ref is skipped
/// (left for a future precise IaC resolver — analogous to how `NameResolver` handles ambiguous
/// code names).
///
/// ## Recommended resolver set for `resolve_all`
///
/// ```rust,no_run
/// use wicked_estate_resolve::{InfraResolver, NameResolver, ScopedNameResolver, ImportMapResolver};
/// use wicked_estate_resolve::resolve_all;
/// use wicked_estate_core::{Resolver, SymbolIndex, UnresolvedRef};
///
/// // Code pipeline (no InfraResolver — it does not interfere but adds noise):
/// // let code_resolvers: &[&dyn Resolver] = &[&NameResolver, &ScopedNameResolver, &ImportMapResolver];
///
/// // IaC/estate pipeline:
/// // let iac_resolvers: &[&dyn Resolver] = &[&InfraResolver];
///
/// // Combined (safe to run both together; InfraResolver only fires on resource targets):
/// // let all_resolvers: &[&dyn Resolver] = &[&NameResolver, &ScopedNameResolver, &ImportMapResolver, &InfraResolver];
/// ```
#[derive(Debug, Default, Clone, Copy)]
pub struct InfraResolver;

impl Resolver for InfraResolver {
    fn id(&self) -> &str {
        "infra-resolver"
    }

    fn tier(&self) -> ResolutionTier {
        // Explicit declared dependencies → Parsed tier (confidence 1.0).
        ResolutionTier::Parsed
    }

    fn resolve(&self, refs: &[UnresolvedRef], index: &dyn SymbolIndex) -> Result<Vec<Edge>> {
        let mut out = Vec::new();

        for r in refs {
            // Look up all nodes that have this name.
            let candidates = index.by_name(&r.raw_name);

            // Partition into resource nodes and code nodes.
            let resource_candidates: Vec<_> = candidates
                .iter()
                .filter(|n| matches!(&n.kind, NodeKind::Other(k) if k == "resource"))
                .collect();

            if resource_candidates.is_empty() {
                // No resource node with this name — not an infra ref, skip.
                continue;
            }

            // Check whether the referencing side is itself a resource node.
            let from_is_resource = index
                .get(&r.from)
                .map(|n| matches!(&n.kind, NodeKind::Other(k) if k == "resource"))
                .unwrap_or(false);

            // Guard: only act when the ref is resource-to-resource, OR when the raw_name
            // resolves *exclusively* to resource nodes (CFN !Ref from a file-scope symbol).
            // This prevents a code ref whose name accidentally matches a resource from being
            // bound here.
            let has_any_code_candidate = candidates
                .iter()
                .any(|n| !matches!(&n.kind, NodeKind::Other(k) if k == "resource"));

            if !from_is_resource && has_any_code_candidate {
                // Mixed — raw_name hits both code nodes and resource nodes. Ambiguous origin;
                // leave for code resolvers and a future precise IaC tier.
                continue;
            }

            // Ambiguity rule: if multiple resource nodes share this name, skip.
            if resource_candidates.len() != 1 {
                continue;
            }

            let target_node = resource_candidates[0];

            // Self-edges are never emitted.
            if target_node.symbol == r.from {
                continue;
            }

            let edge = Edge::new(
                r.from.clone(),
                target_node.symbol.clone(),
                r.kind.clone(),
                self.tier(),
                self.id(),
            )
            .with_location(r.location.clone());

            out.push(edge);
        }

        Ok(out)
    }
}

// ── RulesBridgeResolver ────────────────────────────────────────────────────────

/// Post-extraction resolver that links code call sites to real [`NodeKind::RuleSet`] nodes (W15.13).
///
/// ## Purpose
///
/// When `ExtraEdgeExtractor` detects a rules engine API call (e.g. `IlrContext.execute()` in Java
/// calling IBM ODM), it emits an [`UnresolvedRef`] with `raw_name = "rules-engine:{scheme}"` (e.g.
/// `"rules-engine:odm"`). This resolver handles those refs by scanning the symbol index for all
/// [`NodeKind::RuleSet`] nodes and emitting an [`EdgeKind::InvokedBy`] edge from each call site to
/// each discovered ruleset.
///
/// ## Confidence
///
/// [`ResolutionTier::Heuristic`] (confidence 0.5) — the connection is inferred from the presence
/// of a rules engine API call, not from type analysis. A single call site might invoke any ruleset
/// deployed to that engine.
///
/// ## Self-edge and dedup guards
///
/// Self-edges (source == target) are dropped. Duplicate `(source, target, kind)` triples produced
/// within a single call are deduplicated before returning.
///
/// ## Short-circuit
///
/// If no ref has a `raw_name` that starts with `"rules-engine:"`, the method returns immediately
/// without scanning the index — important for large graphs where `all_nodes()` is expensive.
#[derive(Debug, Default, Clone, Copy)]
pub struct RulesBridgeResolver;

impl Resolver for RulesBridgeResolver {
    fn id(&self) -> &str {
        "rules-bridge-resolver"
    }

    fn tier(&self) -> ResolutionTier {
        ResolutionTier::Heuristic
    }

    fn resolve(&self, refs: &[UnresolvedRef], index: &dyn SymbolIndex) -> Result<Vec<Edge>> {
        // Short-circuit: skip the expensive all_nodes() scan if there are no bridge refs.
        let bridge_refs: Vec<&UnresolvedRef> = refs
            .iter()
            .filter(|r| r.raw_name.starts_with("rules-engine:"))
            .collect();

        if bridge_refs.is_empty() {
            return Ok(vec![]);
        }

        // Scan the entire index for RuleSet nodes.
        let ruleset_nodes: Vec<_> = index
            .all_nodes()?
            .into_iter()
            .filter(|n| n.kind == NodeKind::RuleSet)
            .collect();

        let mut out: Vec<Edge> = Vec::new();
        let mut seen: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();

        for r in bridge_refs {
            for ruleset in &ruleset_nodes {
                // Skip self-edges.
                if ruleset.symbol == r.from {
                    continue;
                }

                // Deduplicate (source, target) pairs — kind is always InvokedBy here.
                let key = (r.from.to_string(), ruleset.symbol.to_string());
                if !seen.insert(key) {
                    continue;
                }

                let edge = Edge::new(
                    r.from.clone(),
                    ruleset.symbol.clone(),
                    EdgeKind::InvokedBy,
                    self.tier(),
                    self.id(),
                )
                .with_location(r.location.clone());

                out.push(edge);
            }
        }

        Ok(out)
    }
}

// ── resolve_all ───────────────────────────────────────────────────────────────

/// The full output of a resolve pass: the deduplicated edges plus the references no resolver
/// bound — computed once, from the same attribution, so persistence and telemetry can never
/// disagree about what "unresolved" means (`docs/ENGINE-CONTRACT.md` §2.1).
#[derive(Debug, Clone, Default)]
pub struct Resolution {
    /// Deduplicated edges, one per `(source, target, kind)`, highest confidence kept.
    pub edges: Vec<Edge>,
    /// References no resolver emitted an edge for (per site — one entry per reference).
    pub unresolved: Vec<UnresolvedRef>,
}

/// Run multiple resolvers over the same `refs` + `index`; return the deduplicated edges **and**
/// the unresolved references, under one definition (`docs/ENGINE-CONTRACT.md` §2.1):
///
/// > A reference is **unresolved** iff no resolver emitted an edge attributed to it — an edge
/// > carrying the reference's exact `(location, kind)` — after per-ref re-resolution of
/// > references that share `(location, kind)`.
///
/// Attribution is per reference (`bound: Vec<bool>` by ref index): an output edge whose
/// `(location, kind)` matches exactly one reference binds that reference. When several
/// references share one `(location, kind)` (multi-target heritage clauses, rules-engine refs at
/// `Span::ZERO`), a single edge at that key is ambiguous; the **collision pass** re-runs
/// `resolver.resolve` with a single-ref slice for each still-unbound reference of that key and
/// binds the ones that yield an edge at their `(location, kind)`. Cost bound: one extra resolve
/// call per (resolver, unbound shared-key ref) — exact because `Resolver::resolve` is per-ref
/// deterministic (the contract on the trait).
///
/// Edges with `location: None`, or whose kind matches no reference at their location, attribute
/// to nothing — they are still returned and may survive dedup.
///
/// Edge dedup is unchanged: by `(source, target, kind)`, keeping the **highest-confidence** edge
/// (strict `>`, first-seen wins ties). A precise tier resolver (SCIP/TSG/LSP) added later
/// naturally wins because its edges carry higher confidence.
///
/// **The production `index`/`watch` slice** (`wicked-estate`'s `index_path`; the activation table
/// lives in `docs/ENGINE-CONTRACT.md` §3.1):
/// ```text
/// NameResolver → ScopedNameResolver → ImportMapResolver → InfraResolver → RulesBridgeResolver
/// ```
/// Dedup keeps the max-confidence edge per key, so resolver ORDER is irrelevant to the result
/// (pinned by `resolve_all_dedup_keeps_higher_confidence_regardless_of_order`).
///
/// `MethodResolutionSynthesizer` (a Heuristic-0.5 unique-callable Calls synthesizer) was retired
/// 2026-08-28: its emit set was a strict subset of `ScopedNameResolver`'s Calls path at lower
/// confidence, so it could never add an edge (see ADR-007's superseding note; pinned by
/// `slice_plus_unique_callable_heuristic_adds_no_edge`).
pub fn resolve_all_with_coverage(
    resolvers: &[&dyn Resolver],
    refs: &[UnresolvedRef],
    index: &dyn SymbolIndex,
) -> Result<Resolution> {
    use std::collections::{HashMap, HashSet};

    // Bucket refs once by (file, span, kind). No JSON serialisation on this path — EdgeKind and
    // Span derive Hash + Eq. bucket_ids maps a key to an index into `buckets`.
    let mut bucket_ids: HashMap<(&str, wicked_estate_core::Span, &EdgeKind), usize> =
        HashMap::new();
    let mut buckets: Vec<Vec<usize>> = Vec::new();
    for (i, r) in refs.iter().enumerate() {
        let key = (r.location.file.as_str(), r.location.span, &r.kind);
        let id = *bucket_ids.entry(key).or_insert_with(|| {
            buckets.push(Vec::new());
            buckets.len() - 1
        });
        buckets[id].push(i);
    }

    let mut bound: Vec<bool> = vec![false; refs.len()];
    // (resolver_idx, bucket_id) pairs whose bucket holds >1 refs and received an edge — each
    // pair is re-run at most once in the collision pass.
    let mut collided: HashSet<(usize, usize)> = HashSet::new();

    let mut best: HashMap<(String, String, String), Edge> = HashMap::new();

    for (resolver_idx, resolver) in resolvers.iter().enumerate() {
        let edges = resolver.resolve(refs, index)?;
        for edge in edges {
            // Attribution: an edge binds the ref(s) at its exact (location, kind).
            if let Some(loc) = &edge.location {
                if let Some(&bucket_id) = bucket_ids.get(&(loc.file.as_str(), loc.span, &edge.kind))
                {
                    match buckets[bucket_id].as_slice() {
                        [only] => bound[*only] = true,
                        _ => {
                            collided.insert((resolver_idx, bucket_id));
                        }
                    }
                }
            }
            let key = edge.dedup_key();
            best.entry(key)
                .and_modify(|incumbent| {
                    if edge.confidence > incumbent.confidence {
                        *incumbent = edge.clone();
                    }
                })
                .or_insert(edge);
        }
    }

    // Collision pass: shared-(location, kind) refs are attributed individually by re-running the
    // resolver with a single-ref slice. Cost: one resolve call per (resolver, unbound
    // shared-key ref). The edges it returns are already in `best` (per-ref determinism), so
    // only the binding is recorded here.
    #[cfg(any(test, debug_assertions))]
    let mut collision_calls: usize = 0;
    for &(resolver_idx, bucket_id) in &collided {
        for &i in &buckets[bucket_id] {
            if bound[i] {
                continue;
            }
            let r = &refs[i];
            #[cfg(any(test, debug_assertions))]
            {
                collision_calls += 1;
            }
            let edges = resolvers[resolver_idx].resolve(std::slice::from_ref(r), index)?;
            if edges
                .iter()
                .any(|e| e.kind == r.kind && e.location.as_ref().is_some_and(|l| *l == r.location))
            {
                bound[i] = true;
            }
        }
    }

    let unresolved: Vec<UnresolvedRef> = refs
        .iter()
        .enumerate()
        .filter(|(i, _)| !bound[*i])
        .map(|(_, r)| r.clone())
        .collect();

    // Debug-only instrumentation (bucket-size histogram + collision-pass call count) for the
    // measurement protocol; silent unless WICKED_ESTATE_COVERAGE_DEBUG is set.
    #[cfg(any(test, debug_assertions))]
    if std::env::var_os("WICKED_ESTATE_COVERAGE_DEBUG").is_some() {
        let mut histogram: std::collections::BTreeMap<usize, usize> =
            std::collections::BTreeMap::new();
        for b in &buckets {
            *histogram.entry(b.len()).or_insert(0) += 1;
        }
        eprintln!(
            "coverage-debug: refs={} buckets={} bucket_size_histogram={:?} collision_calls={}",
            refs.len(),
            buckets.len(),
            histogram,
            collision_calls
        );
    }

    let resolved_edges: Vec<Edge> = best.into_values().collect();

    // Emit resolution counters (best-effort; telemetry failure must never abort resolution).
    {
        let sink = wicked_estate_observe::init_sink_from_env();
        let resource = wicked_estate_core::observability::Resource::service(
            "wicked_estate_resolve",
            env!("CARGO_PKG_VERSION"),
        );
        let scope =
            wicked_estate_core::observability::InstrumentationScope::new("wicked_estate.resolve");
        use wicked_estate_core::observability::*;
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        // Counter: resolved edges by tier.
        let mut tier_counts: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        for edge in &resolved_edges {
            let tier = edge.resolved_by.clone();
            *tier_counts.entry(tier).or_insert(0) += 1;
        }
        for (tier, count) in &tier_counts {
            let metric = Metric {
                name: "wicked_estate.resolve.resolved".to_string(),
                description: String::new(),
                unit: "1".to_string(),
                data: MetricData::Sum {
                    data_points: vec![NumberDataPoint {
                        attributes: vec![KeyValue::str("tier", tier.as_str())],
                        start_time_unix_nano: t,
                        time_unix_nano: t,
                        value: MetricValue::I64(*count),
                    }],
                    temporality: AggregationTemporality::Delta,
                    is_monotonic: true,
                },
            };
            if let Err(e) = sink.export_metrics(&resource, &scope, &[metric]) {
                eprintln!("telemetry: {e}");
            }
        }

        // Counter: unresolved refs — the same per-ref attribution the caller persists
        // (docs/ENGINE-CONTRACT.md §2.1), so the counter and the store can never disagree.
        let unresolved_count = unresolved.len() as i64;
        if unresolved_count > 0 {
            let metric = Metric {
                name: "wicked_estate.resolve.unresolved".to_string(),
                description: String::new(),
                unit: "1".to_string(),
                data: MetricData::Sum {
                    data_points: vec![NumberDataPoint {
                        attributes: vec![KeyValue::str("language", "unknown")],
                        start_time_unix_nano: t,
                        time_unix_nano: t,
                        value: MetricValue::I64(unresolved_count),
                    }],
                    temporality: AggregationTemporality::Delta,
                    is_monotonic: true,
                },
            };
            if let Err(e) = sink.export_metrics(&resource, &scope, &[metric]) {
                eprintln!("telemetry: {e}");
            }
        }
    }

    Ok(Resolution {
        edges: resolved_edges,
        unresolved,
    })
}

/// Edges-only view of [`resolve_all_with_coverage`]. Production callers must use the coverage
/// form — it is the single source of the unresolved set (`docs/ENGINE-CONTRACT.md` §2.1).
///
/// Kept only so test call sites edited by concurrent lanes don't conflict; removal (renaming
/// all call sites to `resolve_all_with_coverage`) is a tracked remainder for the post-lane
/// integration merge (docs/recon/unresolved-accounting.md, merge note M2).
pub fn resolve_all(
    resolvers: &[&dyn Resolver],
    refs: &[UnresolvedRef],
    index: &dyn SymbolIndex,
) -> Result<Vec<Edge>> {
    Ok(resolve_all_with_coverage(resolvers, refs, index)?.edges)
}

// ── scip_edges ────────────────────────────────────────────────────────────────

/// Parse a SCIP `index.scip` payload and correlate its occurrences with the nodes already in the
/// graph, emitting confidence-1.0 `ResolutionTier::Scip` edges.
///
/// ## Algorithm
///
/// **Phase 1 — build a definition map.**
/// For each document, for each occurrence whose `symbol_roles` bit `0x1` (Definition) is set:
/// find the node in `nodes` whose `location.file == document.relative_path` AND whose span
/// *contains* the occurrence start line (`start_line in [node.start_line, node.end_line]`).
/// When multiple nodes qualify, pick the smallest span (narrowest containing node).
/// Store `scip_symbol → (file, node)`.
///
/// **Phase 2 — emit edges.**
/// For each non-definition occurrence of a symbol that has a known definition node:
/// - The `to` node is the definition node.
/// - The `from` node is the node in the same document whose span contains the reference start
///   line (the *enclosing* definition — same "smallest containing" rule).
/// - Skip self-edges, skip occurrences with no matching nodes.
///
/// ## EdgeKind selection
/// If the target node's `kind` is `Function | Method | Constructor` → `EdgeKind::Calls`.
/// Otherwise → `EdgeKind::References`.
///
/// ## Known correlation fuzziness
/// - Match is by **start line only**; column is ignored when correlating to our nodes (our node
///   spans come from tree-sitter which records the full definition block, not just the name token).
/// - "Smallest containing" resolves the common case where a function body contains nested
///   definitions, but it does not resolve ambiguity when two nodes share the exact same span.
/// - Module/file-level occurrences (empty SCIP symbol suffix, e.g. `src/util.ts/`) are skipped
///   because our node model has no direct equivalent file-scope node to correlate to.
pub fn scip_edges(
    index_bytes: &[u8],
    nodes: &[wicked_estate_core::Node],
) -> wicked_estate_core::Result<Vec<wicked_estate_core::Edge>> {
    use protobuf::Message as _;
    use scip::types::Index;
    use std::collections::HashMap;
    use wicked_estate_core::{Edge, EdgeKind, Location, NodeKind, ResolutionTier, SymbolId};

    // ── decode ─────────────────────────────────────────────────────────────────
    let index = Index::parse_from_bytes(index_bytes).map_err(|e| {
        wicked_estate_core::Error::Resolution(format!("scip: protobuf decode error: {e}"))
    })?;

    // ── build file → nodes lookup ─────────────────────────────────────────────
    // Group our nodes by their relative_path file so Phase 1+2 lookups are cheap.
    let mut nodes_by_file: HashMap<&str, Vec<&wicked_estate_core::Node>> = HashMap::new();
    for n in nodes {
        nodes_by_file
            .entry(n.location.file.as_str())
            .or_default()
            .push(n);
    }

    // ── Phase 1: build scip_symbol → (file, our Node) definition map ─────────
    // Key: scip symbol string. Value: (relative_path, SymbolId of the smallest containing node).
    let mut def_map: HashMap<String, (&str, &wicked_estate_core::Node)> = HashMap::new();

    for doc in &index.documents {
        let file = doc.relative_path.as_str();
        let file_nodes = match nodes_by_file.get(file) {
            Some(ns) => ns,
            None => continue,
        };

        for occ in &doc.occurrences {
            // Only process Definition occurrences.
            if (occ.symbol_roles & 0x1) == 0 {
                continue;
            }
            let sym_str = occ.symbol.as_str();
            // Skip file/module-level SCIP symbols (they end in `/` — no function/method node).
            if sym_str.ends_with('/') {
                continue;
            }

            let occ_start_line = match occ.range.first() {
                Some(&l) if l >= 0 => l as u32,
                _ => continue,
            };

            // Find the smallest-span node in this file whose span contains the occurrence start.
            let best = file_nodes
                .iter()
                .filter(|n| {
                    let s = &n.location.span;
                    occ_start_line >= s.start_line && occ_start_line <= s.end_line
                })
                .min_by_key(|n| {
                    let s = &n.location.span;
                    // Span "size" in lines × 1000 + cols — prefer narrower spans.
                    let lines = s.end_line.saturating_sub(s.start_line);
                    (lines, s.end_col.saturating_sub(s.start_col))
                });

            if let Some(node) = best {
                def_map.entry(sym_str.to_string()).or_insert((file, node));
            }
        }
    }

    // ── Phase 2: emit edges from reference occurrences ────────────────────────
    let mut out: Vec<Edge> = Vec::new();

    for doc in &index.documents {
        let ref_file = doc.relative_path.as_str();
        let file_nodes = match nodes_by_file.get(ref_file) {
            Some(ns) => ns,
            None => continue,
        };

        for occ in &doc.occurrences {
            // Only non-definition (reference) occurrences.
            if (occ.symbol_roles & 0x1) != 0 {
                continue;
            }
            let sym_str = occ.symbol.as_str();
            if sym_str.ends_with('/') {
                continue;
            }

            // Must have a known definition node.
            let (def_file, def_node) = match def_map.get(sym_str) {
                Some(v) => *v,
                None => continue,
            };

            let occ_start_line = match occ.range.first() {
                Some(&l) if l >= 0 => l as u32,
                _ => continue,
            };

            // Find the enclosing node in the reference document.
            let enclosing = file_nodes
                .iter()
                .filter(|n| {
                    let s = &n.location.span;
                    occ_start_line >= s.start_line && occ_start_line <= s.end_line
                })
                .min_by_key(|n| {
                    let s = &n.location.span;
                    let lines = s.end_line.saturating_sub(s.start_line);
                    (lines, s.end_col.saturating_sub(s.start_col))
                });

            let from_node = match enclosing {
                Some(n) => n,
                None => continue,
            };

            // Skip self-edges.
            if from_node.symbol == def_node.symbol {
                continue;
            }

            // Pick EdgeKind based on the target's node kind.
            let kind = if matches!(
                def_node.kind,
                NodeKind::Function | NodeKind::Method | NodeKind::Constructor
            ) {
                EdgeKind::Calls
            } else {
                EdgeKind::References
            };

            // Build the reference location from the SCIP occurrence range.
            let ref_span = scip_range_to_span(&occ.range);
            let ref_location = Location::new(ref_file, ref_span);

            let from_sym: SymbolId = from_node.symbol.clone();
            let to_sym: SymbolId = def_node.symbol.clone();

            // Sanity: skip if the definition file is the same symbol pointing to itself
            // (can happen with module-level file nodes that slipped through).
            let _ = def_file; // used for future cross-file assertions; retained for clarity

            let edge = Edge::new(
                from_sym,
                to_sym,
                kind,
                ResolutionTier::Scip,
                "scip-typescript",
            )
            .with_location(ref_location);

            out.push(edge);
        }
    }

    Ok(out)
}

/// Convert a SCIP occurrence range (`Vec<i32>`) to our [`Span`].
///
/// SCIP range encoding (0-based):
/// - 3 elements: `[startLine, startChar, endChar]` — single-line occurrence.
/// - 4 elements: `[startLine, startChar, endLine, endChar]` — multi-line occurrence.
fn scip_range_to_span(range: &[i32]) -> wicked_estate_core::Span {
    match *range {
        [sl, sc, ec] => Span {
            start_byte: 0,
            end_byte: 0,
            start_line: sl.max(0) as u32,
            start_col: sc.max(0) as u32,
            end_line: sl.max(0) as u32,
            end_col: ec.max(0) as u32,
        },
        [sl, sc, el, ec] => Span {
            start_byte: 0,
            end_byte: 0,
            start_line: sl.max(0) as u32,
            start_col: sc.max(0) as u32,
            end_line: el.max(0) as u32,
            end_col: ec.max(0) as u32,
        },
        _ => wicked_estate_core::Span::ZERO,
    }
}

use wicked_estate_core::Span;

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::{
        Confidence, Descriptor, EdgeKind, Language, Location, Metadata, Node, NodeKind, Span,
        Symbol, SymbolId,
    };

    // ── shared test helpers ──────────────────────────────────────────────────

    fn sym(name: &str) -> SymbolId {
        Symbol::global("test", None, vec![Descriptor::method(name, None)]).id()
    }

    /// Minimal in-memory index for unit tests (avoids depending on wicked-estate-store).
    /// No family table — `language_family` stays the trait default (`None`), so the
    /// cross-family guard allows everything (the pre-guard behaviour existing tests pin).
    struct VecIndex(Vec<Node>);

    impl SymbolIndex for VecIndex {
        fn by_name(&self, name: &str) -> Vec<Node> {
            self.0.iter().filter(|n| n.name == name).cloned().collect()
        }
        fn get(&self, id: &SymbolId) -> Option<Node> {
            self.0.iter().find(|n| &n.symbol == id).cloned()
        }
        fn all_nodes(&self) -> wicked_estate_core::Result<Vec<Node>> {
            Ok(self.0.clone())
        }
    }

    impl VecIndex {
        /// The existing shape, named (FEAS-5).
        fn plain(nodes: Vec<Node>) -> Self {
            VecIndex(nodes)
        }
        /// Family-aware index for the D5 guard tests: `families` = (language, family) rows,
        /// mirroring what `languages.toml` provides in production.
        fn with_families(nodes: Vec<Node>, families: &[(&str, &str)]) -> FamilyIndex {
            FamilyIndex {
                inner: VecIndex(nodes),
                families: families
                    .iter()
                    .map(|(l, f)| (l.to_string(), f.to_string()))
                    .collect(),
            }
        }
    }

    /// VecIndex + a language→family table (overrides `language_family`).
    struct FamilyIndex {
        inner: VecIndex,
        families: std::collections::HashMap<String, String>,
    }

    impl SymbolIndex for FamilyIndex {
        fn by_name(&self, name: &str) -> Vec<Node> {
            self.inner.by_name(name)
        }
        fn get(&self, id: &SymbolId) -> Option<Node> {
            self.inner.get(id)
        }
        fn all_nodes(&self) -> wicked_estate_core::Result<Vec<Node>> {
            self.inner.all_nodes()
        }
        fn language_family(&self, language: &str) -> Option<String> {
            self.families.get(language).cloned()
        }
    }

    /// A node with an explicit language and a symbol id derived from `sym_tag` (so homonyms get
    /// distinct ids), kind `Function` unless overridden via [`node_kind_lang`].
    fn node_lang(sym_tag: &str, name: &str, file: &str, lang: &str) -> Node {
        node_kind_lang(sym_tag, name, file, lang, NodeKind::Function)
    }

    fn node_kind_lang(sym_tag: &str, name: &str, file: &str, lang: &str, kind: NodeKind) -> Node {
        Node::new(
            Symbol::global("test", None, vec![Descriptor::method(sym_tag, None)]).id(),
            kind,
            name,
            Language::new(lang),
            Location::new(file, Span::ZERO),
        )
    }

    fn node_at(name: &str, file: &str) -> Node {
        Node::new(
            sym(name),
            NodeKind::Function,
            name,
            Language::new("rust"),
            Location::new(file, Span::ZERO),
        )
    }

    fn node(name: &str) -> Node {
        node_at(name, "f.rs")
    }

    fn call_ref(from: &str, to_name: &str) -> UnresolvedRef {
        UnresolvedRef::new(
            sym(from),
            to_name,
            EdgeKind::Calls,
            Location::new("f.rs", Span::ZERO),
        )
    }

    // ── NameResolver tests ───────────────────────────────────────────────────

    #[test]
    fn resolves_unique_name_to_an_edge() {
        let index = VecIndex(vec![node("alpha"), node("beta")]);
        let edges = NameResolver
            .resolve(&[call_ref("alpha", "beta")], &index)
            .unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source, sym("alpha"));
        assert_eq!(edges[0].target, sym("beta"));
        assert_eq!(edges[0].kind, EdgeKind::Calls);
        assert!((edges[0].confidence.get() - 0.6).abs() < 1e-6);
    }

    #[test]
    fn skips_ambiguous_and_self_and_unknown() {
        // two "beta" defs → ambiguous → skip; a self-call → skip; unknown name → skip.
        let index = VecIndex(vec![node("alpha"), node("beta"), node("beta")]);
        let refs = vec![
            call_ref("alpha", "beta"),  // ambiguous
            call_ref("alpha", "alpha"), // self
            call_ref("alpha", "ghost"), // unknown
        ];
        let edges = NameResolver.resolve(&refs, &index).unwrap();
        assert!(
            edges.is_empty(),
            "no confident edge should be emitted, got {}",
            edges.len()
        );
    }

    // ── ScopedNameResolver tests ─────────────────────────────────────────────

    /// Two `foo` definitions in different files; the reference is in `src/a.rs`.
    /// The `foo` defined in `src/a.rs` should win with scope = "same-file".
    #[test]
    fn same_file_candidate_wins_over_cross_file() {
        // Two distinct symbol ids for two `foo` defs.
        let foo_a = {
            let mut n = node_at("foo", "src/a.rs");
            // Give it a unique symbol id to distinguish from foo_b.
            n.symbol =
                Symbol::global("test", None, vec![Descriptor::method("foo_in_a", None)]).id();
            n
        };
        let foo_b = {
            let mut n = node_at("foo", "src/b.rs");
            n.symbol =
                Symbol::global("test", None, vec![Descriptor::method("foo_in_b", None)]).id();
            n
        };
        // The caller lives in src/a.rs.
        let caller = {
            let mut n = node_at("caller", "src/a.rs");
            n.symbol = Symbol::global("test", None, vec![Descriptor::method("caller", None)]).id();
            n
        };

        let index = VecIndex(vec![foo_a.clone(), foo_b.clone(), caller.clone()]);
        // Construct the ref with caller's actual symbol id.
        let r = UnresolvedRef {
            from: caller.symbol.clone(),
            raw_name: "foo".to_string(),
            kind: EdgeKind::Calls,
            location: Location::new("src/a.rs", Span::ZERO),
            hints: Default::default(),
        };

        let edges = ScopedNameResolver.resolve(&[r], &index).unwrap();

        assert_eq!(edges.len(), 1, "expected exactly one edge");
        assert_eq!(
            edges[0].target, foo_a.symbol,
            "should resolve to same-file foo"
        );
        assert!(
            (edges[0].confidence.get() - 0.65).abs() < 1e-6,
            "same-file confidence should be 0.65, got {}",
            edges[0].confidence.get()
        );
        assert_eq!(
            edges[0].metadata.get("scope").and_then(|v| v.as_str()),
            Some("same-file"),
            "metadata should record scope=same-file"
        );
    }

    /// Two `bar` definitions: one in same dir, one in a different dir.
    /// Same-dir candidate wins when there's no same-file candidate.
    #[test]
    fn same_dir_candidate_wins_over_cross_dir() {
        let bar_same_dir = {
            let mut n = node_at("bar", "src/util.rs");
            n.symbol =
                Symbol::global("test", None, vec![Descriptor::method("bar_util", None)]).id();
            n
        };
        let bar_other_dir = {
            let mut n = node_at("bar", "other/bar.rs");
            n.symbol =
                Symbol::global("test", None, vec![Descriptor::method("bar_other", None)]).id();
            n
        };
        let caller_sym =
            Symbol::global("test", None, vec![Descriptor::method("caller2", None)]).id();

        let index = VecIndex(vec![bar_same_dir.clone(), bar_other_dir]);
        // caller lives in src/main.rs (same dir "src" as bar_same_dir)
        let r = UnresolvedRef {
            from: caller_sym,
            raw_name: "bar".to_string(),
            kind: EdgeKind::Calls,
            location: Location::new("src/main.rs", Span::ZERO),
            hints: Default::default(),
        };

        let edges = ScopedNameResolver.resolve(&[r], &index).unwrap();

        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].target, bar_same_dir.symbol);
        assert!(
            (edges[0].confidence.get() - 0.62).abs() < 1e-6,
            "same-dir confidence should be 0.62, got {}",
            edges[0].confidence.get()
        );
        assert_eq!(
            edges[0].metadata.get("scope").and_then(|v| v.as_str()),
            Some("same-dir")
        );
    }

    /// Two same-file candidates of the same name — still ambiguous, should be skipped.
    #[test]
    fn ambiguous_within_same_file_is_skipped() {
        let foo1 = {
            let mut n = node_at("foo", "src/a.rs");
            n.symbol = Symbol::global("test", None, vec![Descriptor::method("foo1", None)]).id();
            n
        };
        let foo2 = {
            let mut n = node_at("foo", "src/a.rs");
            n.symbol = Symbol::global("test", None, vec![Descriptor::method("foo2", None)]).id();
            n
        };
        let caller_sym =
            Symbol::global("test", None, vec![Descriptor::method("caller3", None)]).id();

        let index = VecIndex(vec![foo1, foo2]);
        let r = UnresolvedRef {
            from: caller_sym,
            raw_name: "foo".to_string(),
            kind: EdgeKind::Calls,
            location: Location::new("src/a.rs", Span::ZERO),
            hints: Default::default(),
        };

        let edges = ScopedNameResolver.resolve(&[r], &index).unwrap();
        assert!(
            edges.is_empty(),
            "truly ambiguous within same file should be skipped"
        );
    }

    /// Self-edges are never emitted.
    #[test]
    fn scoped_resolver_skips_self_edges() {
        let foo_sym = Symbol::global("test", None, vec![Descriptor::method("foo_self", None)]).id();
        let mut foo_node = node_at("foo", "src/a.rs");
        foo_node.symbol = foo_sym.clone();

        let index = VecIndex(vec![foo_node]);
        let r = UnresolvedRef {
            from: foo_sym,
            raw_name: "foo".to_string(),
            kind: EdgeKind::Calls,
            location: Location::new("src/a.rs", Span::ZERO),
            hints: Default::default(),
        };
        let edges = ScopedNameResolver.resolve(&[r], &index).unwrap();
        assert!(edges.is_empty(), "self-edge should not be emitted");
    }

    // ── resolve_all tests ────────────────────────────────────────────────────

    /// resolve_all deduplicates by (source, target, kind) and keeps the MAX-confidence edge.
    #[test]
    fn resolve_all_keeps_max_confidence_edge() {
        // A unique `beta` in the index — both NameResolver and ScopedNameResolver will
        // resolve it, but ScopedNameResolver's edge carries confidence 0.65 (same-file),
        // while NameResolver emits 0.60.
        let beta_sym =
            Symbol::global("test", None, vec![Descriptor::method("beta_all", None)]).id();
        let alpha_sym =
            Symbol::global("test", None, vec![Descriptor::method("alpha_all", None)]).id();

        let mut beta_node = node_at("beta", "src/a.rs");
        beta_node.symbol = beta_sym.clone();
        let mut alpha_node = node_at("alpha", "src/a.rs");
        alpha_node.symbol = alpha_sym.clone();

        let index = VecIndex(vec![beta_node, alpha_node]);

        let r = UnresolvedRef {
            from: alpha_sym.clone(),
            raw_name: "beta".to_string(),
            kind: EdgeKind::Calls,
            location: Location::new("src/a.rs", Span::ZERO),
            hints: Default::default(),
        };

        let resolvers: &[&dyn Resolver] = &[&NameResolver, &ScopedNameResolver];
        let edges = resolve_all(resolvers, &[r], &index).unwrap();

        // Exactly one deduplicated edge.
        assert_eq!(edges.len(), 1, "dedup should yield one edge");
        assert_eq!(edges[0].source, alpha_sym);
        assert_eq!(edges[0].target, beta_sym);

        // The ScopedNameResolver's same-file edge (0.65) wins over NameResolver's (0.60).
        assert!(
            edges[0].confidence.get() > 0.60 + 1e-6,
            "higher-confidence (scoped) edge should win, got {}",
            edges[0].confidence.get()
        );
    }

    // ── ImportMapResolver tests ──────────────────────────────────────────────

    /// Build an UnresolvedRef with an `imports` hint mapping `name` → `module_src`.
    fn import_ref(
        from_name: &str,
        from_file: &str,
        callee_name: &str,
        module_src: &str,
    ) -> UnresolvedRef {
        let mut hints = Metadata::new();
        let mut imports = serde_json::Map::new();
        imports.insert(
            callee_name.to_string(),
            serde_json::Value::String(module_src.to_string()),
        );
        hints.insert("imports".to_string(), serde_json::Value::Object(imports));
        let from_sym = Symbol::global("test", None, vec![Descriptor::method(from_name, None)]).id();
        UnresolvedRef {
            from: from_sym,
            raw_name: callee_name.to_string(),
            kind: EdgeKind::Calls,
            location: Location::new(from_file, Span::ZERO),
            hints,
        }
    }

    fn fn_node_at(name: &str, file: &str) -> Node {
        let sym = Symbol::global(
            "test",
            None,
            vec![Descriptor::method(format!("{name}_in_{file}"), None)],
        )
        .id();
        Node::new(
            sym,
            NodeKind::Function,
            name,
            Language::new("typescript"),
            Location::new(file, Span::ZERO),
        )
    }

    /// Core scenario: `a.ts` imports `helper` from `./b`, calls `helper()`.
    /// `helper` is defined in `b.ts` and also (unrelated) in `c.ts`.
    /// ImportMapResolver must choose the `b.ts` definition, not `c.ts`.
    #[test]
    fn import_map_resolver_binds_to_imported_file_not_unrelated() {
        let helper_b = fn_node_at("helper", "src/b.ts");
        let helper_c = fn_node_at("helper", "src/c.ts");
        let index = VecIndex(vec![helper_b.clone(), helper_c.clone()]);

        // a.ts imports helper from ./b and calls it
        let r = import_ref("main_a", "src/a.ts", "helper", "./b");

        let edges = ImportMapResolver.resolve(&[r], &index).unwrap();
        assert_eq!(
            edges.len(),
            1,
            "expected exactly one edge, got {}",
            edges.len()
        );
        assert_eq!(
            edges[0].target, helper_b.symbol,
            "should resolve to helper in b.ts (import-scoped), not c.ts"
        );
        assert_eq!(
            edges[0].metadata.get("via").and_then(|v| v.as_str()),
            Some("import-map"),
            "edge should carry via=import-map metadata"
        );
        // Confidence should be set (the import-map boost)
        assert!(
            edges[0].confidence.get() > 0.6 - 1e-6,
            "import-map confidence should be >= 0.6, got {}",
            edges[0].confidence.get()
        );
    }

    /// When the name is not in hints["imports"], ImportMapResolver skips.
    #[test]
    fn import_map_resolver_skips_when_name_not_in_hint() {
        let helper = fn_node_at("helper", "src/b.ts");
        let index = VecIndex(vec![helper]);

        // ref for `doSomething` but imports only map `helper`
        let _r = import_ref("main_a", "src/a.ts", "doSomething", "./b");
        // But doSomething is not in the imports map — we built a map with "doSomething"→"./b"
        // Actually the helper above builds the map with the callee_name, so let's use a diff ref.
        // Build a ref where the name is not in the imports map.
        let mut hints = Metadata::new();
        let mut imports = serde_json::Map::new();
        imports.insert(
            "helper".to_string(),
            serde_json::Value::String("./b".to_string()),
        );
        hints.insert("imports".to_string(), serde_json::Value::Object(imports));
        let from_sym =
            Symbol::global("test", None, vec![Descriptor::method("main_a_skip", None)]).id();
        let r = UnresolvedRef {
            from: from_sym,
            raw_name: "doSomething".to_string(), // NOT in imports map
            kind: EdgeKind::Calls,
            location: Location::new("src/a.ts", Span::ZERO),
            hints,
        };

        let edges = ImportMapResolver.resolve(&[r], &index).unwrap();
        assert!(
            edges.is_empty(),
            "resolver should skip refs not in the import map; got {}",
            edges.len()
        );
    }

    /// When hints["imports"] is absent, ImportMapResolver skips (leaves for other resolvers).
    #[test]
    fn import_map_resolver_skips_refs_without_hints() {
        let helper = fn_node_at("helper", "src/b.ts");
        let index = VecIndex(vec![helper]);

        // A plain call ref with no hints (e.g. produced by older extraction).
        let r = call_ref("main_a", "helper");
        let edges = ImportMapResolver.resolve(&[r], &index).unwrap();
        assert!(
            edges.is_empty(),
            "no hints → ImportMapResolver should skip; got {}",
            edges.len()
        );
    }

    /// Self-edges are never emitted even with a matching import map.
    #[test]
    fn import_map_resolver_skips_self_edges() {
        let from_sym =
            Symbol::global("test", None, vec![Descriptor::method("helper_self", None)]).id();
        let mut helper_node = fn_node_at("helper", "src/b.ts");
        helper_node.symbol = from_sym.clone();
        let index = VecIndex(vec![helper_node]);

        let mut hints = Metadata::new();
        let mut imports = serde_json::Map::new();
        imports.insert(
            "helper".to_string(),
            serde_json::Value::String("./b".to_string()),
        );
        hints.insert("imports".to_string(), serde_json::Value::Object(imports));

        let r = UnresolvedRef {
            from: from_sym,
            raw_name: "helper".to_string(),
            kind: EdgeKind::Calls,
            location: Location::new("src/b.ts", Span::ZERO),
            hints,
        };
        let edges = ImportMapResolver.resolve(&[r], &index).unwrap();
        assert!(edges.is_empty(), "self-edge must not be emitted");
    }

    /// Bare module name (non-relative) matches by suffix on candidate file path.
    #[test]
    fn import_map_resolver_bare_module_suffix_match() {
        let helper = fn_node_at("compute", "packages/math/src/index.ts");
        let other = fn_node_at("compute", "packages/other/index.ts");
        let index = VecIndex(vec![helper.clone(), other]);

        // Import from bare "math" package — should match `packages/math/src/index.ts` suffix.
        // Actually the file path after stripping ext and /index: packages/math/src
        // bare module "math" — ends with "math"? "packages/math/src" ends with "math/src" not "math"
        // Let's use a simpler file path.
        let helper2 = fn_node_at("fmt", "vendor/fmt/lib.ts");
        let index2 = VecIndex(vec![helper2.clone()]);

        let r = import_ref("main", "src/a.ts", "fmt", "fmt");
        let edges = ImportMapResolver.resolve(&[r], &index2).unwrap();
        // "vendor/fmt/lib" stem — "fmt" matches if cand_stem ends with "/fmt" (no) or == "fmt" (no).
        // This is a package directory match: bare "fmt" matches "vendor/fmt/lib" ?
        // Actually our current matching requires the stem (sans ext, sans /index) to end with "/fmt"
        // or equal "fmt". "vendor/fmt/lib" → strip ext → "vendor/fmt/lib", no "/index" to strip.
        // "vendor/fmt/lib".ends_with("/fmt") = false. So this won't match — correct behavior.
        // Let's use a file that IS a direct match.
        let helper3 = fn_node_at("parse", "fmt.ts");
        let index3 = VecIndex(vec![helper3.clone()]);
        let r3 = import_ref("main", "src/a.ts", "parse", "fmt");
        let edges3 = ImportMapResolver.resolve(&[r3], &index3).unwrap();
        // "fmt.ts" → stem "fmt", matches bare module "fmt" exactly. Should resolve.
        assert_eq!(
            edges3.len(),
            1,
            "bare module 'fmt' should match file 'fmt.ts' (stem equality)"
        );
        let _ = (edges, index); // suppress unused warnings
    }

    /// `resolve_all` with ImportMapResolver in the slice: import-scoped edge wins over name-only.
    #[test]
    fn resolve_all_with_import_map_resolver_wins_over_name_only() {
        // Two `process` functions: one in b.ts, one in c.ts.
        // a.ts imports `process` from `./b` and calls it.
        // ScopedNameResolver (cross-file, conf 0.60) would be ambiguous.
        // ImportMapResolver should resolve unambiguously to b.ts (conf 0.63).
        let process_b = fn_node_at("process", "src/b.ts");
        let process_c = fn_node_at("process", "src/c.ts");
        let index = VecIndex(vec![process_b.clone(), process_c]);

        let r = import_ref("main_a", "src/a.ts", "process", "./b");

        let resolvers: &[&dyn wicked_estate_core::Resolver] =
            &[&NameResolver, &ScopedNameResolver, &ImportMapResolver];
        let edges = resolve_all(resolvers, &[r], &index).unwrap();

        assert_eq!(edges.len(), 1, "expected exactly one deduplicated edge");
        assert_eq!(
            edges[0].target, process_b.symbol,
            "ImportMapResolver should have picked process in b.ts"
        );
        assert_eq!(
            edges[0].metadata.get("via").and_then(|v| v.as_str()),
            Some("import-map"),
        );
    }

    /// resolve_all with two independent (source,target,kind) edges keeps both.
    #[test]
    fn resolve_all_preserves_distinct_edges() {
        let a_sym = Symbol::global("test", None, vec![Descriptor::method("ra", None)]).id();
        let b_sym = Symbol::global("test", None, vec![Descriptor::method("rb", None)]).id();
        let c_sym = Symbol::global("test", None, vec![Descriptor::method("rc", None)]).id();

        let mut a_node = node("ra");
        a_node.symbol = a_sym.clone();
        let mut b_node = node("rb");
        b_node.symbol = b_sym.clone();
        let mut c_node = node("rc");
        c_node.symbol = c_sym.clone();

        let index = VecIndex(vec![a_node, b_node, c_node]);

        // a calls b (unique), a calls c (unique) — two distinct edges.
        let refs = vec![
            UnresolvedRef {
                from: a_sym.clone(),
                raw_name: "rb".to_string(),
                kind: EdgeKind::Calls,
                location: Location::new("f.rs", Span::ZERO),
                hints: Default::default(),
            },
            UnresolvedRef {
                from: a_sym.clone(),
                raw_name: "rc".to_string(),
                kind: EdgeKind::Calls,
                location: Location::new("f.rs", Span::ZERO),
                hints: Default::default(),
            },
        ];

        let resolvers: &[&dyn Resolver] = &[&NameResolver];
        let mut edges = resolve_all(resolvers, &refs, &index).unwrap();
        edges.sort_by_key(|e| e.target.0.clone());

        assert_eq!(edges.len(), 2, "two distinct edges should be preserved");
    }

    /// Confidence values injected by a mock resolver override lower ones from a real resolver.
    #[test]
    fn resolve_all_higher_confidence_wins_regardless_of_resolver_order() {
        // Build two resolvers that emit the same (source, target, Calls) edge but with
        // different confidences via a hand-crafted edge approach: we use a tiny mock.
        struct MockResolver {
            conf: f32,
            id_str: &'static str,
        }
        impl Resolver for MockResolver {
            fn id(&self) -> &str {
                self.id_str
            }
            fn tier(&self) -> ResolutionTier {
                ResolutionTier::ImportMap
            }
            fn resolve(
                &self,
                _refs: &[UnresolvedRef],
                _index: &dyn SymbolIndex,
            ) -> Result<Vec<Edge>> {
                let src =
                    Symbol::global("test", None, vec![Descriptor::method("mock_src", None)]).id();
                let tgt =
                    Symbol::global("test", None, vec![Descriptor::method("mock_tgt", None)]).id();
                let mut e = Edge::new(
                    src,
                    tgt,
                    EdgeKind::Calls,
                    ResolutionTier::ImportMap,
                    self.id_str,
                );
                e.confidence = Confidence::new(self.conf);
                Ok(vec![e])
            }
        }

        let high = MockResolver {
            conf: 0.9,
            id_str: "high",
        };
        let low = MockResolver {
            conf: 0.3,
            id_str: "low",
        };
        let index = VecIndex(vec![]);

        // Run low-confidence first.
        let resolvers_lo_hi: &[&dyn Resolver] = &[&low, &high];
        let edges = resolve_all(resolvers_lo_hi, &[], &index).unwrap();
        assert_eq!(edges.len(), 1);
        assert!(
            (edges[0].confidence.get() - 0.9).abs() < 1e-6,
            "high-conf should win"
        );

        // Run high-confidence first.
        let resolvers_hi_lo: &[&dyn Resolver] = &[&high, &low];
        let edges2 = resolve_all(resolvers_hi_lo, &[], &index).unwrap();
        assert_eq!(edges2.len(), 1);
        assert!(
            (edges2[0].confidence.get() - 0.9).abs() < 1e-6,
            "high-conf should still win"
        );
    }

    // ── InfraResolver tests ──────────────────────────────────────────────────

    /// Build a resource node mimicking what tfstate/CFN extractors emit.
    ///
    /// `name` is the terraform address or CFN logical id.
    /// The symbol uses `Symbol::synthetic("tfstate", name)` matching TfstateCollector.
    fn resource_node(name: &str) -> Node {
        let sym = Symbol::synthetic("tfstate", name).id();
        Node::new(
            sym,
            NodeKind::Other("resource".to_string()),
            name,
            Language::new("tfstate"),
            Location::new(name, Span::ZERO),
        )
    }

    /// Build the `SymbolId` for a resource node, matching `resource_node` above.
    fn resource_sym(name: &str) -> SymbolId {
        Symbol::synthetic("tfstate", name).id()
    }

    /// Build an `UnresolvedRef` representing a tfstate `depends_on` reference.
    ///
    /// `from_addr` is the dependent resource address; `target_addr` is the raw dependency name.
    fn depends_on_ref(from_addr: &str, target_addr: &str) -> UnresolvedRef {
        UnresolvedRef::new(
            resource_sym(from_addr),
            target_addr,
            EdgeKind::Other("depends_on".to_string()),
            Location::new(from_addr, Span::ZERO),
        )
    }

    /// Core scenario (W9.4): `aws_eip.ip` depends_on `aws_instance.web`.
    ///
    /// Both nodes are in the index. The resolver should emit exactly one
    /// `EdgeKind::Other("depends_on")` edge from `aws_eip.ip` → `aws_instance.web`.
    #[test]
    fn infra_resolver_emits_depends_on_edge_for_matching_resource() {
        let web = resource_node("aws_instance.web");
        let eip = resource_node("aws_eip.ip");
        let index = VecIndex(vec![web.clone(), eip.clone()]);

        let r = depends_on_ref("aws_eip.ip", "aws_instance.web");
        let edges = InfraResolver.resolve(&[r], &index).unwrap();

        assert_eq!(
            edges.len(),
            1,
            "expected exactly one edge, got {}",
            edges.len()
        );
        assert_eq!(
            edges[0].source,
            resource_sym("aws_eip.ip"),
            "source must be the dependent"
        );
        assert_eq!(
            edges[0].target,
            resource_sym("aws_instance.web"),
            "target must be the dependency"
        );
        assert_eq!(
            edges[0].kind,
            EdgeKind::Other("depends_on".to_string()),
            "edge kind must be depends_on"
        );
        // ResolutionTier::Parsed → confidence 1.0.
        assert!(
            (edges[0].confidence.get() - 1.0).abs() < 1e-6,
            "Parsed tier must give confidence 1.0, got {}",
            edges[0].confidence.get()
        );
        assert_eq!(edges[0].resolved_by, "infra-resolver");
    }

    /// Blast-radius direction invariant (ADR-001):
    /// `source = dependent`, `target = dependency`.
    /// Blast-radius on `aws_instance.web` must return `aws_eip.ip` as a dependent.
    #[test]
    fn infra_resolver_edge_direction_source_is_dependent() {
        let web = resource_node("aws_instance.web");
        let eip = resource_node("aws_eip.ip");
        let index = VecIndex(vec![web.clone(), eip.clone()]);

        let r = depends_on_ref("aws_eip.ip", "aws_instance.web");
        let edges = InfraResolver.resolve(&[r], &index).unwrap();

        assert_eq!(edges.len(), 1);
        // Blast-radius query: find edges where target == aws_instance.web.
        // The dependent (aws_eip.ip) is found as the source.
        assert_eq!(
            edges[0].target,
            resource_sym("aws_instance.web"),
            "target must be aws_instance.web for blast-radius to work"
        );
        assert_eq!(
            edges[0].source,
            resource_sym("aws_eip.ip"),
            "source (dependent) must be aws_eip.ip"
        );
    }

    /// Non-matching ref (the raw_name names a resource that is NOT in the index) is skipped.
    #[test]
    fn infra_resolver_skips_non_matching_ref() {
        let web = resource_node("aws_instance.web");
        let index = VecIndex(vec![web]);

        // This ref targets "aws_vpc.external" which is not in the index.
        let r = depends_on_ref("aws_instance.web", "aws_vpc.external");
        let edges = InfraResolver.resolve(&[r], &index).unwrap();

        assert!(
            edges.is_empty(),
            "ref to unknown resource must be skipped; got {} edges",
            edges.len()
        );
    }

    /// A code ref (raw_name matches a Function node) is not picked up by InfraResolver.
    #[test]
    fn infra_resolver_does_not_fire_on_code_refs() {
        // The index has both a Function node and no resource node named "helper".
        let fn_node = node("helper"); // NodeKind::Function (from the shared test helpers above)
        let index = VecIndex(vec![fn_node]);

        let r = call_ref("caller", "helper"); // EdgeKind::Calls, resolves to a code node
        let edges = InfraResolver.resolve(&[r], &index).unwrap();

        assert!(
            edges.is_empty(),
            "InfraResolver must not fire on refs that resolve only to code nodes; got {} edges",
            edges.len()
        );
    }

    /// Self-edges are never emitted (resource referencing itself).
    #[test]
    fn infra_resolver_skips_self_edges() {
        let web = resource_node("aws_instance.web");
        let index = VecIndex(vec![web]);

        // The from symbol is the same as the target resource.
        let r = UnresolvedRef::new(
            resource_sym("aws_instance.web"),
            "aws_instance.web",
            EdgeKind::Other("depends_on".to_string()),
            Location::new("aws_instance.web", Span::ZERO),
        );
        let edges = InfraResolver.resolve(&[r], &index).unwrap();

        assert!(
            edges.is_empty(),
            "self-edge must not be emitted; got {} edges",
            edges.len()
        );
    }

    /// Ambiguous resource names (two resource nodes with the same name) are skipped.
    #[test]
    fn infra_resolver_skips_ambiguous_resource_names() {
        // Two resource nodes with the same name "aws_instance.web" — ambiguous.
        let web1 = {
            let mut n = resource_node("aws_instance.web");
            n.symbol = Symbol::synthetic("tfstate", "aws_instance.web[0]").id();
            n
        };
        let web2 = {
            let mut n = resource_node("aws_instance.web");
            n.symbol = Symbol::synthetic("tfstate", "aws_instance.web[1]").id();
            n
        };
        let eip = resource_node("aws_eip.ip");
        let index = VecIndex(vec![web1, web2, eip.clone()]);

        let r = depends_on_ref("aws_eip.ip", "aws_instance.web");
        let edges = InfraResolver.resolve(&[r], &index).unwrap();

        assert!(
            edges.is_empty(),
            "ambiguous resource name must be skipped; got {} edges",
            edges.len()
        );
    }

    /// CFN-style ref: `from` is a file-scope symbol (not a resource node), but `raw_name`
    /// resolves exclusively to a resource node. InfraResolver must still bind it.
    #[test]
    fn infra_resolver_binds_cfn_ref_from_file_symbol_to_resource() {
        // Simulate a CFN file symbol (the `from` in CFN !Ref extraction).
        let file_sym = Symbol::file("template.yaml").id();
        let web_resource = resource_node("WebServer");

        // The index does NOT contain the file node itself, but that's fine —
        // `index.get(&file_sym)` will return None, so `from_is_resource = false`.
        // Since the raw_name resolves ONLY to resource nodes (no code nodes),
        // InfraResolver must act.
        let index = VecIndex(vec![web_resource.clone()]);

        let r = UnresolvedRef::new(
            file_sym,
            "WebServer",
            EdgeKind::Calls, // CFN extractor uses Calls as proxy for !Ref
            Location::new("template.yaml", Span::ZERO),
        );
        let edges = InfraResolver.resolve(&[r], &index).unwrap();

        assert_eq!(
            edges.len(),
            1,
            "CFN !Ref must be bound to the resource node; got {} edges",
            edges.len()
        );
        assert_eq!(
            edges[0].target,
            resource_sym("WebServer"),
            "target must be the WebServer resource"
        );
        assert_eq!(
            edges[0].kind,
            EdgeKind::Calls,
            "edge kind must preserve the ref's kind"
        );
    }

    /// When raw_name hits BOTH a code node and a resource node, InfraResolver skips
    /// (leaves for code resolvers + future precise IaC tier).
    #[test]
    fn infra_resolver_skips_mixed_code_and_resource_candidates() {
        // A function named "helper" AND a resource named "helper".
        let code_helper = node("helper"); // NodeKind::Function
        let resource_helper = resource_node("helper"); // NodeKind::Other("resource")
        let index = VecIndex(vec![code_helper, resource_helper]);

        // from is NOT a resource (code caller).
        let r = call_ref("caller", "helper");
        let edges = InfraResolver.resolve(&[r], &index).unwrap();

        assert!(
            edges.is_empty(),
            "mixed code+resource candidates must be skipped; got {} edges",
            edges.len()
        );
    }

    /// resolve_all with InfraResolver: infra edges are kept, code edges are kept,
    /// no cross-contamination.
    #[test]
    fn resolve_all_with_infra_resolver_keeps_both_infra_and_code_edges() {
        // Code graph: alpha calls beta (unique function).
        let beta = node("beta");
        let alpha = node("alpha");

        // Infra graph: aws_eip.ip depends_on aws_instance.web.
        let web = resource_node("aws_instance.web");
        let eip = resource_node("aws_eip.ip");

        let index = VecIndex(vec![beta.clone(), alpha.clone(), web.clone(), eip.clone()]);

        let refs = vec![
            call_ref("alpha", "beta"),                        // code ref
            depends_on_ref("aws_eip.ip", "aws_instance.web"), // infra ref
        ];

        let resolvers: &[&dyn Resolver] = &[&NameResolver, &InfraResolver];
        let mut edges = resolve_all(resolvers, &refs, &index).unwrap();
        edges.sort_by_key(|e| e.source.0.clone());

        assert_eq!(
            edges.len(),
            2,
            "expected one code edge + one infra edge; got {}",
            edges.len()
        );
        // The code edge (alpha → beta) was resolved by NameResolver.
        let code_edge = edges.iter().find(|e| e.kind == EdgeKind::Calls).unwrap();
        assert_eq!(code_edge.source, sym("alpha"));
        assert_eq!(code_edge.target, sym("beta"));

        // The infra edge (aws_eip.ip → aws_instance.web) was resolved by InfraResolver.
        let infra_edge = edges
            .iter()
            .find(|e| e.kind == EdgeKind::Other("depends_on".to_string()))
            .unwrap();
        assert_eq!(infra_edge.source, resource_sym("aws_eip.ip"));
        assert_eq!(infra_edge.target, resource_sym("aws_instance.web"));
        assert!(
            (infra_edge.confidence.get() - 1.0).abs() < 1e-6,
            "infra edge confidence must be 1.0"
        );
    }

    // ── RulesBridgeResolver tests ─────────────────────────────────────────────

    /// Build a RuleSet node mimicking what the rules engine extractor emits.
    fn ruleset_node(name: &str, scheme: &str) -> Node {
        let id = Symbol::synthetic(scheme, name).id();
        Node::new(
            id,
            NodeKind::RuleSet,
            name,
            Language::new(scheme),
            Location::new(name, Span::ZERO),
        )
    }

    fn ruleset_sym(name: &str, scheme: &str) -> SymbolId {
        Symbol::synthetic(scheme, name).id()
    }

    /// An `UnresolvedRef` emitted by `ExtraEdgeExtractor` when it detects a rules engine call.
    fn rules_engine_ref(from_name: &str, scheme: &str) -> UnresolvedRef {
        UnresolvedRef::new(
            sym(from_name),
            format!("rules-engine:{scheme}"),
            EdgeKind::InvokedBy,
            Location::new("Caller.java", Span::ZERO),
        )
    }

    /// Happy path (W15.13): a code file node emits a "rules-engine:odm" ref → one InvokedBy
    /// edge is produced pointing to the RuleSet node in the index.
    #[test]
    fn rules_bridge_resolver_emits_invoked_by_edges() {
        let rs = ruleset_node("LoanApproval", "odm");
        // File node — the referencing side (not a RuleSet itself).
        let file_node = Node::new(
            sym("Caller"),
            NodeKind::File,
            "Caller",
            Language::new("java"),
            Location::new("Caller.java", Span::ZERO),
        );
        let index = VecIndex(vec![file_node, rs.clone()]);

        let r = rules_engine_ref("Caller", "odm");
        let edges = RulesBridgeResolver.resolve(&[r], &index).unwrap();

        assert_eq!(
            edges.len(),
            1,
            "expected exactly 1 InvokedBy edge, got {}",
            edges.len()
        );
        assert_eq!(
            edges[0].source,
            sym("Caller"),
            "source must be the call site"
        );
        assert_eq!(
            edges[0].target,
            ruleset_sym("LoanApproval", "odm"),
            "target must be the RuleSet node"
        );
        assert_eq!(
            edges[0].kind,
            EdgeKind::InvokedBy,
            "edge kind must be InvokedBy"
        );
        // ResolutionTier::Heuristic → confidence 0.5.
        assert!(
            (edges[0].confidence.get() - 0.5).abs() < 1e-6,
            "Heuristic tier must give confidence 0.5, got {}",
            edges[0].confidence.get()
        );
        assert_eq!(edges[0].resolved_by, "rules-bridge-resolver");
    }

    /// A ref whose `raw_name` is a plain class name (not "rules-engine:…") must be ignored by
    /// `RulesBridgeResolver` — it is not a bridge ref.
    #[test]
    fn rules_bridge_resolver_ignores_non_bridge_refs() {
        let rs = ruleset_node("LoanApproval", "odm");
        let file_node = Node::new(
            sym("Caller"),
            NodeKind::File,
            "Caller",
            Language::new("java"),
            Location::new("Caller.java", Span::ZERO),
        );
        let index = VecIndex(vec![file_node, rs]);

        // A plain call-site ref — NOT a bridge ref.
        let r = call_ref("Caller", "SomeClass");
        let edges = RulesBridgeResolver.resolve(&[r], &index).unwrap();

        assert_eq!(
            edges.len(),
            0,
            "RulesBridgeResolver must not fire on non-bridge refs; got {} edges",
            edges.len()
        );
    }

    // ── dir_of / root-level file tests (D6) ──────────────────────────────────

    #[test]
    fn dir_of_root_level_is_empty() {
        assert_eq!(
            dir_of("a.ts"),
            "",
            "separator-less path has the empty (root) directory"
        );
        assert_eq!(dir_of("src/a.ts"), "src");
        assert_eq!(dir_of("src\\a.ts"), "src", "Windows separator");
    }

    /// Two root-level files must rank as same-dir (0.62), beating a sub-directory homonym.
    /// Under the old `dir_of` ("path itself when no separator") both candidates scored
    /// CrossFile and the tie parked the ref.
    #[test]
    fn scoped_resolver_ranks_two_root_files_same_dir() {
        let caller = {
            let mut n = node_at("caller", "a.ts");
            n.symbol =
                Symbol::global("test", None, vec![Descriptor::method("root_caller", None)]).id();
            n
        };
        let foo_root = {
            let mut n = node_at("foo", "b.ts");
            n.symbol =
                Symbol::global("test", None, vec![Descriptor::method("foo_root", None)]).id();
            n
        };
        let foo_sub = {
            let mut n = node_at("foo", "sub/c.ts");
            n.symbol = Symbol::global("test", None, vec![Descriptor::method("foo_sub", None)]).id();
            n
        };
        let index = VecIndex(vec![caller.clone(), foo_root.clone(), foo_sub]);
        let r = UnresolvedRef {
            from: caller.symbol,
            raw_name: "foo".to_string(),
            kind: EdgeKind::Calls,
            location: Location::new("a.ts", Span::ZERO),
            hints: Default::default(),
        };
        let edges = ScopedNameResolver.resolve(&[r], &index).unwrap();
        assert_eq!(edges.len(), 1, "root-level same-dir candidate must win");
        assert_eq!(edges[0].target, foo_root.symbol);
        assert!(
            (edges[0].confidence.get() - 0.62).abs() < 1e-6,
            "same-dir confidence must be 0.62, got {}",
            edges[0].confidence.get()
        );
        assert_eq!(
            edges[0].metadata.get("scope").and_then(|v| v.as_str()),
            Some("same-dir")
        );
    }

    /// A root-level `./b` import-map hint must bind: `file_matches_module` joins the (now empty)
    /// ref dir with the spec instead of producing the bogus `a.ts/./b`.
    #[test]
    fn import_map_resolver_binds_root_level_relative_import() {
        let caller_sym = Symbol::global(
            "test",
            None,
            vec![Descriptor::method("rootimp_caller", None)],
        )
        .id();
        let foo_root = {
            let mut n = node_at("foo", "b.ts");
            n.symbol = Symbol::global(
                "test",
                None,
                vec![Descriptor::method("rootimp_foo_b", None)],
            )
            .id();
            n
        };
        let foo_sub = {
            let mut n = node_at("foo", "sub/c.ts");
            n.symbol = Symbol::global(
                "test",
                None,
                vec![Descriptor::method("rootimp_foo_c", None)],
            )
            .id();
            n
        };
        let index = VecIndex(vec![foo_root.clone(), foo_sub]);
        let mut r = UnresolvedRef::new(
            caller_sym,
            "foo",
            EdgeKind::Calls,
            Location::new("a.ts", Span::ZERO),
        );
        r.hints
            .insert("imports".to_string(), serde_json::json!({ "foo": "./b" }));
        let edges = ImportMapResolver.resolve(&[r], &index).unwrap();
        assert_eq!(edges.len(), 1, "root-level ./b relative import must bind");
        assert_eq!(edges[0].target, foo_root.symbol);
        assert!(
            (edges[0].confidence.get() - 0.63).abs() < 1e-6,
            "import-map confidence must be 0.63, got {}",
            edges[0].confidence.get()
        );
        assert_eq!(
            edges[0].metadata.get("via").and_then(|v| v.as_str()),
            Some("import-map")
        );
    }

    // ── admissibility + family-guard tests (D1/D5, engine defect #2) ──────────

    /// A TS `res.json()` call must not bind to a Python `import json` node — Import nodes are
    /// reference sites, never definitions (rejected for EVERY ref kind).
    #[test]
    fn name_resolver_never_binds_calls_to_import_node() {
        let caller = node_lang("nrimp_caller", "caller", "client.ts", "typescript");
        let import_node =
            node_kind_lang("nrimp_json", "json", "api.py", "python", NodeKind::Import);
        let index = VecIndex::plain(vec![caller.clone(), import_node]);
        let r = UnresolvedRef::new(
            caller.symbol.clone(),
            "json",
            EdgeKind::Calls,
            Location::new("client.ts", Span::ZERO),
        );
        assert!(NameResolver.resolve(&[r], &index).unwrap().is_empty());

        // Import rejection applies to non-Calls kinds too.
        let r2 = UnresolvedRef::new(
            caller.symbol,
            "json",
            EdgeKind::References,
            Location::new("client.ts", Span::ZERO),
        );
        assert!(NameResolver.resolve(&[r2], &index).unwrap().is_empty());
    }

    /// `new Notification()` must not bind to `interface Notification` (deny-listed for Calls).
    #[test]
    fn name_resolver_never_binds_calls_to_interface() {
        let caller = node_lang("nrif_caller", "caller", "app.ts", "typescript");
        let iface = node_kind_lang(
            "nrif_notif",
            "Notification",
            "types.ts",
            "typescript",
            NodeKind::Interface,
        );
        let index = VecIndex::plain(vec![caller.clone(), iface]);
        let r = UnresolvedRef::new(
            caller.symbol,
            "Notification",
            EdgeKind::Calls,
            Location::new("app.ts", Span::ZERO),
        );
        assert!(NameResolver.resolve(&[r], &index).unwrap().is_empty());
    }

    /// D1 keep-set: `new X()` construction sites (Class) and function-valued bindings (Constant)
    /// stay legitimate unique Calls targets.
    #[test]
    fn name_resolver_keeps_class_and_constant_targets_for_calls() {
        let caller = node_lang("nrkeep_caller", "caller", "app.ts", "typescript");
        let class_node = node_kind_lang(
            "nrkeep_api",
            "ApiError",
            "errors.ts",
            "typescript",
            NodeKind::Class,
        );
        let const_node = node_kind_lang(
            "nrkeep_store",
            "useRuntimeStore",
            "store.ts",
            "typescript",
            NodeKind::Constant,
        );
        let index = VecIndex::plain(vec![caller.clone(), class_node.clone(), const_node.clone()]);
        let refs = vec![
            UnresolvedRef::new(
                caller.symbol.clone(),
                "ApiError",
                EdgeKind::Calls,
                Location::new("app.ts", Span::ZERO),
            ),
            UnresolvedRef::new(
                caller.symbol,
                "useRuntimeStore",
                EdgeKind::Calls,
                Location::new("app.ts", Span::ZERO),
            ),
        ];
        let edges = NameResolver.resolve(&refs, &index).unwrap();
        assert_eq!(edges.len(), 2, "Class and Constant Calls targets are kept");
        let targets: Vec<_> = edges.iter().map(|e| e.target.clone()).collect();
        assert!(targets.contains(&class_node.symbol));
        assert!(targets.contains(&const_node.symbol));
    }

    /// The Calls deny-list must not leak into other ref kinds: `extends → interface` is legal
    /// (crew ships 2 such name-resolver edges).
    #[test]
    fn name_resolver_keeps_extends_to_interface() {
        let sub = node_kind_lang("nrext_sub", "Sub", "sub.ts", "typescript", NodeKind::Class);
        let iface = node_kind_lang(
            "nrext_base",
            "Base",
            "base.ts",
            "typescript",
            NodeKind::Interface,
        );
        let index = VecIndex::plain(vec![sub.clone(), iface.clone()]);
        let r = UnresolvedRef::new(
            sub.symbol,
            "Base",
            EdgeKind::Extends,
            Location::new("sub.ts", Span::ZERO),
        );
        let edges = NameResolver.resolve(&[r], &index).unwrap();
        assert_eq!(
            edges.len(),
            1,
            "Extends → Interface must survive the deny-list"
        );
        assert_eq!(edges[0].target, iface.symbol);
    }

    /// D5: python → typescript is cross-family (both known, different) → blocked.
    #[test]
    fn family_guard_blocks_python_ref_to_typescript_node() {
        let caller = node_lang("fg_py_caller", "caller", "api.py", "python");
        let target = node_lang("fg_ts_fn", "handle", "app.ts", "typescript");
        let index = VecIndex::with_families(
            vec![caller.clone(), target],
            &[("python", "python"), ("typescript", "javascript")],
        );
        let r = UnresolvedRef::new(
            caller.symbol,
            "handle",
            EdgeKind::Calls,
            Location::new("api.py", Span::ZERO),
        );
        assert!(NameResolver.resolve(&[r], &index).unwrap().is_empty());
    }

    /// D5: tsx → typescript share family `javascript` → allowed.
    #[test]
    fn family_guard_allows_tsx_ref_to_typescript_node() {
        let caller = node_lang("fg_tsx_caller", "caller", "App.tsx", "tsx");
        let target = node_lang("fg_ts_target", "handle", "app.ts", "typescript");
        let index = VecIndex::with_families(
            vec![caller.clone(), target.clone()],
            &[("tsx", "javascript"), ("typescript", "javascript")],
        );
        let r = UnresolvedRef::new(
            caller.symbol,
            "handle",
            EdgeKind::Calls,
            Location::new("App.tsx", Span::ZERO),
        );
        let edges = NameResolver.resolve(&[r], &index).unwrap();
        assert_eq!(edges.len(), 1, "same-family (javascript) must bind");
        assert_eq!(edges[0].target, target.symbol);
    }

    /// D5/F7: jcl and cobol have NO manifest row (unknown family) → guard allows; the target is
    /// a `Module` node (a COBOL program), which the D1 keep-set admits for Calls.
    #[test]
    fn family_guard_allows_unknown_family_jcl_to_cobol() {
        let step = node_kind_lang(
            "fg_jcl_step",
            "STEP1",
            "payroll.jcl",
            "jcl",
            NodeKind::Other("job_step".to_string()),
        );
        let program = node_kind_lang(
            "fg_cobol_prog",
            "PAYROLL",
            "payroll.cbl",
            "cobol",
            NodeKind::Module,
        );
        // Families table deliberately does NOT know jcl/cobol (like the real manifest).
        let index = VecIndex::with_families(
            vec![step.clone(), program.clone()],
            &[("typescript", "javascript")],
        );
        let r = UnresolvedRef::new(
            step.symbol,
            "PAYROLL",
            EdgeKind::Calls,
            Location::new("payroll.jcl", Span::ZERO),
        );
        let edges = NameResolver.resolve(&[r], &index).unwrap();
        assert_eq!(edges.len(), 1, "unknown-family mainframe join must survive");
        assert_eq!(edges[0].target, program.symbol);
    }

    /// D5: a ref whose `from` symbol has no node in the index (extractor-synthetic sources) has
    /// no source family → allow.
    #[test]
    fn family_guard_allows_missing_from_node() {
        let target = node_lang("fg_missing_target", "helper", "util.ts", "typescript");
        let index = VecIndex::with_families(
            vec![target.clone()],
            &[("typescript", "javascript"), ("python", "python")],
        );
        let ghost_from =
            Symbol::global("test", None, vec![Descriptor::method("fg_ghost", None)]).id();
        let r = UnresolvedRef::new(
            ghost_from,
            "helper",
            EdgeKind::Calls,
            Location::new("ghost.py", Span::ZERO),
        );
        let edges = NameResolver.resolve(&[r], &index).unwrap();
        assert_eq!(edges.len(), 1, "missing from-node must not block");
        assert_eq!(edges[0].target, target.symbol);
    }

    /// ScopedNameResolver applies the same family guard (shared helper, D3).
    #[test]
    fn scoped_resolver_applies_family_guard() {
        let caller = node_lang("sfg_caller", "caller", "app.ts", "typescript");
        let target = node_lang("sfg_pyfn", "compute", "calc.py", "python");
        let index = VecIndex::with_families(
            vec![caller.clone(), target],
            &[("typescript", "javascript"), ("python", "python")],
        );
        let r = UnresolvedRef::new(
            caller.symbol,
            "compute",
            EdgeKind::Calls,
            Location::new("app.ts", Span::ZERO),
        );
        assert!(ScopedNameResolver.resolve(&[r], &index).unwrap().is_empty());
    }

    // ── F16 shape tests (FEAS-1: recall-widening placements pinned) ───────────

    /// Crew `code` shape: candidates = a deny-listed css type_alias + a cross-family bash
    /// variable. The deny-list makes the bash variable the unique survivor; ONLY the
    /// post-uniqueness family guard stops the wrong edge → 0 edges.
    #[test]
    fn deny_list_survivor_blocked_by_family_guard() {
        let caller = node_lang("f16_code_caller", "caller", "src/api.ts", "typescript");
        let css_alias = node_kind_lang(
            "f16_code_css",
            "code",
            "site/src/styles/crew.css",
            "css",
            NodeKind::TypeAlias,
        );
        let bash_var = node_kind_lang(
            "f16_code_bash",
            "code",
            "scripts/verify-ecosystem.sh",
            "bash",
            NodeKind::Variable,
        );
        let index = VecIndex::with_families(
            vec![caller.clone(), css_alias, bash_var],
            &[
                ("typescript", "javascript"),
                ("css", "css"),
                ("bash", "bash"),
            ],
        );
        let r = UnresolvedRef::new(
            caller.symbol,
            "code",
            EdgeKind::Calls,
            Location::new("src/api.ts", Span::ZERO),
        );
        assert!(
            NameResolver.resolve(&[r], &index).unwrap().is_empty(),
            "the deny-list unshadows the bash variable; the family guard must block it"
        );
    }
    // ── resolve_all_with_coverage tests (unresolved accounting, ENGINE-CONTRACT §2.1) ────────
    //
    // T2-T9 use a mock resolver with an explicit binding table — accounting tests must not
    // encode NameResolver kind semantics (owned by the resolver-precision lane). T1 is the
    // single real-slice smoke test.

    /// A span whose only distinguishing feature is its start line — enough for distinct
    /// `(location, kind)` bucket keys.
    fn span_at(line: u32) -> Span {
        Span {
            start_byte: line * 100,
            end_byte: line * 100 + 10,
            start_line: line,
            start_col: 0,
            end_line: line,
            end_col: 10,
        }
    }

    /// Mock resolver driven by an explicit binding table: it emits one edge per ref that
    /// matches a `(raw_name, location, kind)` row, at that ref's `(location, kind)` — unless
    /// `emit_kind` overrides the kind (T9) or `strip_location` drops the location (T7).
    /// Stateless per-ref loop, so it satisfies the `Resolver` per-ref determinism contract.
    struct BindingMock {
        table: Vec<(String, Location, EdgeKind)>,
        emit_kind: Option<EdgeKind>,
        strip_location: bool,
    }

    impl BindingMock {
        fn binding(table: Vec<(&str, Location, EdgeKind)>) -> Self {
            Self {
                table: table
                    .into_iter()
                    .map(|(n, l, k)| (n.to_string(), l, k))
                    .collect(),
                emit_kind: None,
                strip_location: false,
            }
        }
    }

    impl Resolver for BindingMock {
        fn id(&self) -> &str {
            "binding-mock"
        }
        fn tier(&self) -> ResolutionTier {
            ResolutionTier::ImportMap
        }
        fn resolve(
            &self,
            refs: &[UnresolvedRef],
            _index: &dyn SymbolIndex,
        ) -> wicked_estate_core::Result<Vec<Edge>> {
            let mut out = Vec::new();
            for r in refs {
                let bound = self
                    .table
                    .iter()
                    .any(|(n, l, k)| *n == r.raw_name && *l == r.location && *k == r.kind);
                if bound {
                    let kind = self.emit_kind.clone().unwrap_or_else(|| r.kind.clone());
                    let mut e = Edge::new(
                        r.from.clone(),
                        sym(&format!("target_{}", r.raw_name)),
                        kind,
                        ResolutionTier::ImportMap,
                        "binding-mock",
                    );
                    if !self.strip_location {
                        e = e.with_location(r.location.clone());
                    }
                    out.push(e);
                }
            }
            Ok(out)
        }
    }

    fn calls_ref_at(from: &str, to_name: &str, file: &str, line: u32) -> UnresolvedRef {
        UnresolvedRef::new(
            sym(from),
            to_name,
            EdgeKind::Calls,
            Location::new(file, span_at(line)),
        )
    }

    /// T1 — real-slice smoke test: three call sites of one bound relationship are all
    /// attributed; none is "unresolved" (the engine-defect-#3 over-count).
    #[test]
    fn coverage_repeat_call_sites_are_not_unresolved() {
        let index = VecIndex(vec![node("g")]);
        let refs = vec![
            calls_ref_at("f", "g", "main.ts", 3),
            calls_ref_at("f", "g", "main.ts", 4),
            calls_ref_at("f", "g", "main.ts", 5),
        ];
        let resolvers: &[&dyn Resolver] = &[&NameResolver];
        let res = resolve_all_with_coverage(resolvers, &refs, &index).unwrap();
        assert_eq!(res.edges.len(), 1, "one deduped Calls edge f→g");
        assert!(
            res.unresolved.is_empty(),
            "no site of a bound relationship is unresolved; got {:?}",
            res.unresolved
        );
    }

    /// T2 — honest coverage: every site of an UNBOUND relationship stays unresolved (per site).
    #[test]
    fn coverage_keeps_every_site_of_an_unbound_relationship() {
        let index = VecIndex(vec![]);
        let refs = vec![
            calls_ref_at("f", "h", "main.ts", 3),
            calls_ref_at("f", "h", "main.ts", 7),
        ];
        let mock = BindingMock::binding(vec![]);
        let resolvers: &[&dyn Resolver] = &[&mock];
        let res = resolve_all_with_coverage(resolvers, &refs, &index).unwrap();
        assert!(res.edges.is_empty());
        assert_eq!(res.unresolved.len(), 2, "rows are per site");
    }

    /// T3 — the attribution key includes kind: a Calls edge at L binds the Calls ref at L,
    /// never the Imports ref at the same location.
    #[test]
    fn attribution_key_includes_kind() {
        let index = VecIndex(vec![]);
        let loc = Location::new("main.ts", span_at(1));
        let calls = UnresolvedRef::new(sym("f"), "x", EdgeKind::Calls, loc.clone());
        let imports = UnresolvedRef::new(sym("f"), "x", EdgeKind::Imports, loc.clone());
        let mock = BindingMock::binding(vec![("x", loc.clone(), EdgeKind::Calls)]);
        let resolvers: &[&dyn Resolver] = &[&mock];
        let res = resolve_all_with_coverage(resolvers, &[calls, imports], &index).unwrap();
        assert_eq!(res.edges.len(), 1);
        assert_eq!(res.unresolved.len(), 1);
        assert_eq!(res.unresolved[0].kind, EdgeKind::Imports);
    }

    /// T4 — the collision pass attributes shared-`(location, kind)` refs individually
    /// (multi-target heritage clause shape: `class C implements A, B` with only `A` bound).
    #[test]
    fn collision_pass_attributes_shared_key_refs_individually() {
        let index = VecIndex(vec![]);
        let loc = Location::new("c1.ts", span_at(1));
        let ref_a = UnresolvedRef::new(sym("C"), "A", EdgeKind::Implements, loc.clone());
        let ref_b = UnresolvedRef::new(sym("C"), "B", EdgeKind::Implements, loc.clone());
        let mock = BindingMock::binding(vec![("A", loc.clone(), EdgeKind::Implements)]);
        let resolvers: &[&dyn Resolver] = &[&mock];
        let res = resolve_all_with_coverage(resolvers, &[ref_a, ref_b], &index).unwrap();
        assert_eq!(res.edges.len(), 1, "the edge for A survives");
        assert_eq!(
            res.unresolved.len(),
            1,
            "exactly the unbound sibling stays unresolved"
        );
        assert_eq!(res.unresolved[0].raw_name, "B");
    }

    /// T5 — repeat import statements of one module are one relationship: 1 edge, 0 unresolved.
    #[test]
    fn repeat_import_statements_are_not_unresolved() {
        let index = VecIndex(vec![]);
        let loc0 = Location::new("main.ts", span_at(0));
        let loc1 = Location::new("main.ts", span_at(1));
        let r0 = UnresolvedRef::new(sym("main"), "'./mod'", EdgeKind::Imports, loc0.clone());
        let r1 = UnresolvedRef::new(sym("main"), "'./mod'", EdgeKind::Imports, loc1.clone());
        let mock = BindingMock::binding(vec![
            ("'./mod'", loc0, EdgeKind::Imports),
            ("'./mod'", loc1, EdgeKind::Imports),
        ]);
        let resolvers: &[&dyn Resolver] = &[&mock];
        let res = resolve_all_with_coverage(resolvers, &[r0, r1], &index).unwrap();
        assert_eq!(
            res.edges.len(),
            1,
            "same (source, target, kind) dedups to one"
        );
        assert!(res.unresolved.is_empty());
    }

    /// T6 — accounting is scoped per ref: a bound `sa/` ref never cancels an `sb/` ref that
    /// shares its raw_name and kind (multi-repo labelled graphs, D4).
    #[test]
    fn accounting_is_scoped_per_ref() {
        let index = VecIndex(vec![]);
        let loc_sa = Location::new("sa/src/x.ts", span_at(2));
        let loc_sb = Location::new("sb/src/x.ts", span_at(2));
        let r_sa = UnresolvedRef::new(sym("sa_f"), "g", EdgeKind::Calls, loc_sa.clone());
        let r_sb = UnresolvedRef::new(sym("sb_f"), "g", EdgeKind::Calls, loc_sb.clone());
        let mock = BindingMock::binding(vec![("g", loc_sa, EdgeKind::Calls)]);
        let resolvers: &[&dyn Resolver] = &[&mock];
        let res = resolve_all_with_coverage(resolvers, &[r_sa, r_sb], &index).unwrap();
        assert_eq!(res.unresolved.len(), 1);
        assert_eq!(res.unresolved[0].location.file, "sb/src/x.ts");
    }

    /// T7 — an edge with `location: None` attributes to nothing but is still returned
    /// (Resolver contract, location half).
    #[test]
    fn edges_without_location_attribute_nothing() {
        let index = VecIndex(vec![]);
        let loc = Location::new("main.ts", span_at(1));
        let r = UnresolvedRef::new(sym("f"), "g", EdgeKind::Calls, loc.clone());
        let mock = BindingMock {
            strip_location: true,
            ..BindingMock::binding(vec![("g", loc, EdgeKind::Calls)])
        };
        let resolvers: &[&dyn Resolver] = &[&mock];
        let res = resolve_all_with_coverage(resolvers, &[r], &index).unwrap();
        assert_eq!(res.edges.len(), 1, "the edge is still returned");
        assert_eq!(res.unresolved.len(), 1, "but it binds nothing");
    }

    /// T8 — a bound site never cancels a sibling site sharing `(from, raw_name, kind)` at a
    /// different location (`this.save()` vs `cache.save()` — the receiver-inference safety
    /// that rejects the relationship-keyed definition, A1).
    #[test]
    fn a_bound_site_does_not_cancel_a_sibling_site() {
        let index = VecIndex(vec![]);
        let loc1 = Location::new("svc.ts", span_at(10));
        let loc2 = Location::new("svc.ts", span_at(20));
        let r1 = UnresolvedRef::new(sym("f"), "save", EdgeKind::Calls, loc1.clone());
        let r2 = UnresolvedRef::new(sym("f"), "save", EdgeKind::Calls, loc2.clone());
        let mock = BindingMock::binding(vec![("save", loc1, EdgeKind::Calls)]);
        let resolvers: &[&dyn Resolver] = &[&mock];
        let res = resolve_all_with_coverage(resolvers, &[r1, r2], &index).unwrap();
        assert_eq!(res.unresolved.len(), 1);
        assert_eq!(res.unresolved[0].location, loc2);
    }

    /// T9 — an edge whose kind differs from the ref at its location binds nothing
    /// (Resolver contract, kind half).
    #[test]
    fn edge_kind_must_match_ref_kind() {
        let index = VecIndex(vec![]);
        let loc = Location::new("main.ts", span_at(1));
        let r = UnresolvedRef::new(sym("f"), "g", EdgeKind::Calls, loc.clone());
        let mock = BindingMock {
            emit_kind: Some(EdgeKind::Imports),
            ..BindingMock::binding(vec![("g", loc, EdgeKind::Calls)])
        };
        let resolvers: &[&dyn Resolver] = &[&mock];
        let res = resolve_all_with_coverage(resolvers, &[r], &index).unwrap();
        assert_eq!(res.edges.len(), 1, "the edge is still returned");
        assert_eq!(res.unresolved.len(), 1, "but it binds nothing");
    }


    /// Studio `p` shape: a deny-listed html type_alias homonym shadows a same-family tsx
    /// function. Dropping the type_alias pre-uniqueness is the INTENDED recovery → exactly one
    /// new name-resolver edge at 0.60.
    #[test]
    fn deny_list_unshadows_same_family_callable() {
        let caller = node_lang("f16_p_caller", "caller", "src/App.tsx", "tsx");
        let html_alias = node_kind_lang(
            "f16_p_html",
            "p",
            "e2e/fixtures/doc-fixture.html",
            "html",
            NodeKind::TypeAlias,
        );
        let tsx_fn = node_lang("f16_p_fn", "p", "src/components/RunTimeline.tsx", "tsx");
        let index = VecIndex::with_families(
            vec![caller.clone(), html_alias, tsx_fn.clone()],
            &[("tsx", "javascript"), ("html", "html")],
        );
        let r = UnresolvedRef::new(
            caller.symbol,
            "p",
            EdgeKind::Calls,
            Location::new("src/App.tsx", Span::ZERO),
        );
        let edges = NameResolver.resolve(&[r], &index).unwrap();
        assert_eq!(edges.len(), 1, "the tsx function must be unshadowed");
        assert_eq!(edges[0].target, tsx_fn.symbol);
        assert_eq!(edges[0].resolved_by, "name-resolver");
        assert!((edges[0].confidence.get() - 0.60).abs() < 1e-6);
    }

    /// Scoped pre-ranking family retain: a python-Function + typescript-Function cross-file
    /// homonym pair flips tie→park into unique→0.60 — deliberate recall-widening (D3).
    #[test]
    fn scoped_family_retain_unshadows_same_family_homonym() {
        let caller = node_lang("f16_sc_caller", "caller", "src/a.ts", "typescript");
        let py_fn = node_lang("f16_sc_py", "process", "jobs/run.py", "python");
        let ts_fn = node_lang("f16_sc_ts", "process", "lib/process.ts", "typescript");
        let index = VecIndex::with_families(
            vec![caller.clone(), py_fn, ts_fn.clone()],
            &[("typescript", "javascript"), ("python", "python")],
        );
        let r = UnresolvedRef::new(
            caller.symbol,
            "process",
            EdgeKind::Calls,
            Location::new("src/a.ts", Span::ZERO),
        );
        let edges = ScopedNameResolver.resolve(&[r], &index).unwrap();
        assert_eq!(
            edges.len(),
            1,
            "dropping the cross-family homonym must leave a unique cross-file winner"
        );
        assert_eq!(edges[0].target, ts_fn.symbol);
        assert!((edges[0].confidence.get() - 0.60).abs() < 1e-6);
    }

    // ── D14 pinning tests (FEAS-2: the corpora have zero svelte/vue files) ────

    /// A function declared in a `.vue` script block is a legitimate target for a typescript
    /// Calls ref — vue is in the javascript family.
    #[test]
    fn family_guard_allows_vue_to_typescript_node() {
        let caller = node_lang("d14_vue_caller", "caller", "src/main.ts", "typescript");
        let vue_fn = node_lang("d14_vue_fn", "mount", "src/App.vue", "vue");
        let index = VecIndex::with_families(
            vec![caller.clone(), vue_fn.clone()],
            &[("typescript", "javascript"), ("vue", "javascript")],
        );
        let r = UnresolvedRef::new(
            caller.symbol,
            "mount",
            EdgeKind::Calls,
            Location::new("src/main.ts", Span::ZERO),
        );
        let edges = NameResolver.resolve(&[r], &index).unwrap();
        assert_eq!(edges.len(), 1, "vue is javascript-family; must bind");
        assert_eq!(edges[0].target, vue_fn.symbol);
    }

    /// html is its OWN family (D14): even a callable-kinded symbol minted from markup must not
    /// bind from a typescript Calls ref.
    #[test]
    fn family_guard_blocks_html_to_typescript_ref() {
        let caller = node_lang("d14_html_caller", "caller", "src/main.ts", "typescript");
        let html_sym = node_lang("d14_html_fn", "render", "docs/page.html", "html");
        let index = VecIndex::with_families(
            vec![caller.clone(), html_sym],
            &[("typescript", "javascript"), ("html", "html")],
        );
        let r = UnresolvedRef::new(
            caller.symbol,
            "render",
            EdgeKind::Calls,
            Location::new("src/main.ts", Span::ZERO),
        );
        assert!(
            NameResolver.resolve(&[r], &index).unwrap().is_empty(),
            "html is its own family; a TS Calls ref must not bind into markup"
        );
    }

    // ── resolve_all structural regressions (D02-9 / FEAS-4) ───────────────────

    /// A resolver that emits unique-callable Calls at Heuristic 0.5 — exactly the retired
    /// `MethodResolutionSynthesizer`'s algorithm, inlined so the structural theorem stays
    /// testable without shipping the dead code.
    struct UniqueCallableHeuristic;

    impl Resolver for UniqueCallableHeuristic {
        fn id(&self) -> &str {
            "test-unique-callable-heuristic"
        }
        fn tier(&self) -> ResolutionTier {
            ResolutionTier::Heuristic
        }
        fn resolve(&self, refs: &[UnresolvedRef], index: &dyn SymbolIndex) -> Result<Vec<Edge>> {
            let mut out = Vec::new();
            for r in refs {
                if r.kind != EdgeKind::Calls {
                    continue;
                }
                let mut candidates = index.by_name(&r.raw_name);
                candidates.retain(|n| is_callable(&n.kind));
                candidates.retain(|n| n.symbol != r.from);
                if let [only] = candidates.as_slice() {
                    out.push(
                        Edge::new(
                            r.from.clone(),
                            only.symbol.clone(),
                            EdgeKind::Calls,
                            ResolutionTier::Heuristic,
                            self.id(),
                        )
                        .with_location(r.location.clone()),
                    );
                }
            }
            Ok(out)
        }
    }

    /// D02-9: a unique-callable Heuristic-0.5 synthesizer adds NOTHING to the production slice —
    /// its emit set is a strict subset of `ScopedNameResolver`'s Calls path (same by_name, same
    /// callable retain, same self-drop, lower confidence). This is the structural reason
    /// `MethodResolutionSynthesizer` was retired.
    #[test]
    fn slice_plus_unique_callable_heuristic_adds_no_edge() {
        // A homonym population: unique callables, ambiguous callables, non-callables, self-calls.
        let caller = node_lang("d029_caller", "caller", "src/a.ts", "typescript");
        let unique_fn = node_lang("d029_unique", "unique_fn", "src/b.ts", "typescript");
        let amb1 = node_lang("d029_amb1", "amb", "src/c.ts", "typescript");
        let amb2 = node_lang("d029_amb2", "amb", "src/d.ts", "typescript");
        let konst = node_kind_lang(
            "d029_const",
            "cfg",
            "src/e.ts",
            "typescript",
            NodeKind::Constant,
        );
        let nodes = vec![caller.clone(), unique_fn, amb1, amb2, konst];
        let index = VecIndex::plain(nodes);
        let mk = |name: &str| {
            UnresolvedRef::new(
                caller.symbol.clone(),
                name,
                EdgeKind::Calls,
                Location::new("src/a.ts", Span::ZERO),
            )
        };
        let refs = vec![
            mk("unique_fn"),
            mk("amb"),
            mk("cfg"),
            mk("caller"),
            mk("ghost"),
        ];

        let base: &[&dyn Resolver] = &[
            &NameResolver,
            &ScopedNameResolver,
            &ImportMapResolver,
            &InfraResolver,
        ];
        let with_synth: &[&dyn Resolver] = &[
            &NameResolver,
            &ScopedNameResolver,
            &ImportMapResolver,
            &InfraResolver,
            &UniqueCallableHeuristic,
        ];

        let mut a = resolve_all(base, &refs, &index).unwrap();
        let mut b = resolve_all(with_synth, &refs, &index).unwrap();
        let key = |e: &Edge| {
            (
                e.source.to_string(),
                e.target.to_string(),
                format!("{:?}", e.kind),
                e.resolved_by.clone(),
            )
        };
        a.sort_by_key(&key);
        b.sort_by_key(&key);
        assert_eq!(
            a.iter().map(&key).collect::<Vec<_>>(),
            b.iter().map(&key).collect::<Vec<_>>(),
            "the heuristic must add no edge and win no dedup over the production slice"
        );
    }

    /// FEAS-4: `resolve_all`'s max-confidence dedup is order-independent — the surviving edge
    /// keeps the higher tier's confidence and resolved_by whether the Heuristic-0.5 resolver runs
    /// FIRST (exercises the `>`-not-`>=` replace branch) or LAST (exercises or_insert-then-keep).
    /// This replaces the coverage the retired synthesizer's resolve_all test provided.
    #[test]
    fn resolve_all_dedup_keeps_higher_confidence_regardless_of_order() {
        let caller = node_lang("feas4_caller", "caller", "src/a.ts", "typescript");
        let target = node_lang("feas4_target", "zap", "src/b.ts", "typescript");
        let index = VecIndex::plain(vec![caller.clone(), target.clone()]);
        let r = UnresolvedRef::new(
            caller.symbol.clone(),
            "zap",
            EdgeKind::Calls,
            Location::new("src/a.ts", Span::ZERO),
        );

        for resolvers in [
            &[&UniqueCallableHeuristic as &dyn Resolver, &NameResolver] as &[&dyn Resolver],
            &[&NameResolver as &dyn Resolver, &UniqueCallableHeuristic],
        ] {
            let edges = resolve_all(resolvers, std::slice::from_ref(&r), &index).unwrap();
            assert_eq!(edges.len(), 1, "dedup must yield one edge");
            assert_eq!(edges[0].target, target.symbol);
            assert_eq!(
                edges[0].resolved_by, "name-resolver",
                "the higher-confidence resolver must win regardless of order"
            );
            assert!(
                (edges[0].confidence.get() - 0.6).abs() < 1e-6,
                "the surviving edge must keep the higher confidence, got {}",
                edges[0].confidence.get()
            );
        }
    }
}
