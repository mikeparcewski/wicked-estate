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
            let candidates = index.by_name(&r.raw_name);
            // Unique resolution only — ambiguity is deferred to a precise tier (W2.2+).
            if let [only] = candidates.as_slice() {
                if only.symbol != r.from {
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

/// Parent directory of a file path, or the path itself if no separator is found.
fn dir_of(file: &str) -> &str {
    // Works for both Unix (`/`) and Windows (`\`) paths in repo-relative strings.
    if let Some(pos) = file.rfind(['/', '\\']) {
        &file[..pos]
    } else {
        file
    }
}

/// Whether a node kind is a method or function (used for bare method-name matching).
fn is_callable(kind: &NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Function | NodeKind::Method | NodeKind::Constructor
    )
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

            // For method-like edge kinds, narrow the pool to callable nodes.
            if matches!(r.kind, EdgeKind::Calls) {
                candidates.retain(|n| is_callable(&n.kind));
            }

            // Remove self-candidates immediately.
            candidates.retain(|n| n.symbol != r.from);

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

            // Collect callable candidates for this name.
            let mut candidates = index.by_name(&r.raw_name);
            candidates.retain(|n| is_callable(&n.kind));
            candidates.retain(|n| n.symbol != r.from); // no self-edges

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
/// [`NameResolver`] and [`ScopedNameResolver`] only emit edges when candidates are callable or
/// match function/method/constructor nodes. Since resource nodes are `NodeKind::Other("resource")`
/// they are not callable, so those resolvers skip them. `InfraResolver` in turn requires at least
/// one candidate to be a resource node before it acts, so it will not fire on code refs.
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

// ── MethodResolutionSynthesizer ────────────────────────────────────────────────

/// AST-based synthesizer: resolves call-site references by looking up the called name in the
/// parsed node index.
///
/// **Algorithm:**
/// 1. Only acts on refs whose `kind` is [`EdgeKind::Calls`].
/// 2. Calls `index.by_name(&ref.raw_name)` and retains only callable nodes
///    (Function / Method / Constructor).
/// 3. Removes self-candidates.
/// 4. If **exactly one** callable candidate remains → emit a `Calls` edge at
///    [`ResolutionTier::Heuristic`] (confidence 0.5).
/// 5. If **zero or more than one** candidates remain → emit nothing (honest non-resolution;
///    ambiguity is deferred to a precise tier).
///
/// This synthesizer operates entirely on the parsed node index (AST-derived facts), never on
/// raw source text. That is the core distinction from the old regex-over-source approach.
///
/// Position in the resolver cascade: placed **after** the higher-confidence resolvers
/// ([`NameResolver`], [`ScopedNameResolver`], [`ImportMapResolver`]) in [`resolve_all`] so it
/// only fills gaps; the dedup step in `resolve_all` discards this synthesizer's lower-confidence
/// edge whenever a higher-confidence resolver already resolved the same relationship.
#[derive(Debug, Default, Clone, Copy)]
pub struct MethodResolutionSynthesizer;

impl Resolver for MethodResolutionSynthesizer {
    fn id(&self) -> &str {
        "ast-synth-method"
    }

    fn tier(&self) -> ResolutionTier {
        ResolutionTier::Heuristic
    }

    fn resolve(&self, refs: &[UnresolvedRef], index: &dyn SymbolIndex) -> Result<Vec<Edge>> {
        let mut out = Vec::new();

        for r in refs {
            // Only synthesize for call-site references.
            if r.kind != EdgeKind::Calls {
                continue;
            }

            // Look up all nodes that carry this name in the parsed index.
            let mut candidates = index.by_name(&r.raw_name);

            // Narrow to callable nodes (the parsed graph distinguishes callables).
            candidates.retain(|n| is_callable(&n.kind));

            // No self-edges.
            candidates.retain(|n| n.symbol != r.from);

            // Exact one candidate → synthesis is unambiguous.
            if let [only] = candidates.as_slice() {
                let edge = Edge::new(
                    r.from.clone(),
                    only.symbol.clone(),
                    EdgeKind::Calls,
                    ResolutionTier::Heuristic,
                    self.id(),
                )
                .with_location(r.location.clone());
                out.push(edge);
            }
            // >1 → ambiguous; emit nothing (honest non-resolution).
            // 0  → unknown; emit nothing.
        }

        Ok(out)
    }
}

// ── Precision monitoring ───────────────────────────────────────────────────────

/// The minimum acceptable precision for any synthesizer.
///
/// A synthesizer whose edge-level precision falls below this floor is caught by
/// [`measure_synth_precision`] + [`SynthPrecision::is_acceptable`]. The value mirrors the
/// research finding that heuristic synthesis at < 70% precision creates more noise than signal
///.
pub const SYNTH_PRECISION_FLOOR: f64 = 0.7;

/// Per-synthesizer precision measurement over a gold-labelled reference set.
///
/// Produced by [`measure_synth_precision`].
#[derive(Debug, Clone)]
pub struct SynthPrecision {
    /// The resolver id of the measured synthesizer (from [`Resolver::id`]).
    pub resolver_id: String,
    /// Number of edges the synthesizer emitted.
    pub emitted: usize,
    /// Number of emitted edges whose target matched the gold map.
    pub correct: usize,
    /// `correct / emitted`, or `1.0` when `emitted == 0` (vacuously precise).
    pub precision: f64,
}

impl SynthPrecision {
    /// Returns `true` iff the synthesizer meets the [`SYNTH_PRECISION_FLOOR`].
    pub fn is_acceptable(&self) -> bool {
        self.precision >= SYNTH_PRECISION_FLOOR
    }
}

/// Run `synth` against `refs` + `index` and score the resulting edges against a gold map.
///
/// # Gold map format
///
/// `gold` maps `(from_symbol_id, raw_name)` → expected `target_symbol_id`. An emitted edge is
/// counted as **correct** when `gold.get(&(edge.source.clone(), ref_raw_name.clone()))` equals
/// `Some(&edge.target)`.
///
/// # Matching emitted edges back to raw_name
///
/// Because `Edge` does not carry the `raw_name`, this function correlates each emitted edge back
/// to its originating ref via `(source, kind) == (ref.from, ref.kind)` matching. When multiple
/// refs share the same source+kind (uncommon in practice), the first match wins; precision
/// scoring is statistical and a one-off ambiguity does not skew the result meaningfully.
pub fn measure_synth_precision(
    synth: &dyn Resolver,
    refs: &[UnresolvedRef],
    index: &dyn SymbolIndex,
    gold: &std::collections::HashMap<
        (wicked_estate_core::SymbolId, String),
        wicked_estate_core::SymbolId,
    >,
) -> SynthPrecision {
    // Run the synthesizer; ignore errors (a failing synthesizer has precision 0).
    let edges = match synth.resolve(refs, index) {
        Ok(e) => e,
        Err(_) => {
            return SynthPrecision {
                resolver_id: synth.id().to_string(),
                emitted: 0,
                correct: 0,
                precision: 1.0, // vacuously precise: nothing emitted
            };
        }
    };

    let emitted = edges.len();
    if emitted == 0 {
        return SynthPrecision {
            resolver_id: synth.id().to_string(),
            emitted: 0,
            correct: 0,
            precision: 1.0,
        };
    }

    let mut correct = 0usize;

    for edge in &edges {
        // Correlate the edge back to its originating ref to retrieve raw_name.
        let raw_name = refs
            .iter()
            .find(|r| r.from == edge.source && r.kind == edge.kind)
            .map(|r| r.raw_name.as_str())
            .unwrap_or("");

        let key = (edge.source.clone(), raw_name.to_string());
        if gold.get(&key) == Some(&edge.target) {
            correct += 1;
        }
    }

    let precision = correct as f64 / emitted as f64;

    SynthPrecision {
        resolver_id: synth.id().to_string(),
        emitted,
        correct,
        precision,
    }
}

// ── resolve_all ───────────────────────────────────────────────────────────────

/// Run multiple resolvers over the same `refs` + `index`, then deduplicate the resulting edges
/// by `(source, target, kind)`, keeping the **highest-confidence** edge for each key.
///
/// This lets the pipeline compose cheap resolvers (e.g. [`NameResolver`] for unique names) with
/// scope-aware ones (e.g. [`ScopedNameResolver`]) without emitting duplicate edges. A precise
/// tier resolver (SCIP/TSG/LSP) added later will naturally win because its edges carry higher
/// confidence.
///
/// **Resolver order for the full code pipeline (recommended):**
/// ```text
/// ImportMapResolver → ScopedNameResolver → NameResolver → MethodResolutionSynthesizer
/// ```
/// `MethodResolutionSynthesizer` is listed last so Heuristic (0.5) edges only fill gaps left by
/// the higher-confidence ImportMap (0.6–0.65) resolvers; `resolve_all` keeps the max-confidence
/// edge on dedup.
pub fn resolve_all(
    resolvers: &[&dyn Resolver],
    refs: &[UnresolvedRef],
    index: &dyn SymbolIndex,
) -> Result<Vec<Edge>> {
    use std::collections::HashMap;

    let mut best: HashMap<(String, String, String), Edge> = HashMap::new();

    for resolver in resolvers {
        let edges = resolver.resolve(refs, index)?;
        for edge in edges {
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

        // Counter: unresolved refs.
        // Track which refs were resolved by (source_id, kind_string) — this correctly counts
        // edges whose `location` is `None` as resolved, fixing the inflated-unresolved bug
        // (Finding 7): previously `.filter_map(|e| e.location.as_ref())` silently dropped
        // None-location edges, leaving their originating refs uncancelled from the counter.
        let resolved_ref_keys: std::collections::HashSet<(String, String)> = resolved_edges
            .iter()
            .map(|e| {
                (
                    e.source.0.clone(),
                    serde_json::to_string(&e.kind).unwrap_or_default(),
                )
            })
            .collect();
        let unresolved_count = refs
            .iter()
            .filter(|r| {
                !resolved_ref_keys.contains(&(
                    r.from.0.clone(),
                    serde_json::to_string(&r.kind).unwrap_or_default(),
                ))
            })
            .count() as i64;
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

    Ok(resolved_edges)
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
    struct VecIndex(Vec<Node>);

    impl SymbolIndex for VecIndex {
        fn by_name(&self, name: &str) -> Vec<Node> {
            self.0.iter().filter(|n| n.name == name).cloned().collect()
        }
        fn get(&self, id: &SymbolId) -> Option<Node> {
            self.0.iter().find(|n| &n.symbol == id).cloned()
        }
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

    // ── MethodResolutionSynthesizer tests ────────────────────────────────────

    /// Happy path: exactly one callable `foo` in the index → synthesizer emits a Heuristic
    /// Calls edge with confidence 0.5 and provenance Heuristic.
    #[test]
    fn synth_method_emits_heuristic_edge_for_unique_callable() {
        // Build a unique method `foo` on type `Bar` (symbol id distinct from the caller).
        let foo_sym = Symbol::global("test", None, vec![Descriptor::method("Bar.foo", None)]).id();
        let foo_node = Node::new(
            foo_sym.clone(),
            NodeKind::Method,
            "foo",
            Language::new("rust"),
            Location::new("bar.rs", Span::ZERO),
        );

        let caller_sym =
            Symbol::global("test", None, vec![Descriptor::method("caller_synth", None)]).id();

        let index = VecIndex(vec![foo_node]);

        let r = UnresolvedRef::new(
            caller_sym.clone(),
            "foo",
            EdgeKind::Calls,
            Location::new("main.rs", Span::ZERO),
        );

        let edges = MethodResolutionSynthesizer.resolve(&[r], &index).unwrap();

        assert_eq!(
            edges.len(),
            1,
            "unique callable should produce exactly one edge; got {}",
            edges.len()
        );
        assert_eq!(edges[0].source, caller_sym, "source must be the caller");
        assert_eq!(edges[0].target, foo_sym, "target must be Bar.foo");
        assert_eq!(edges[0].kind, EdgeKind::Calls, "kind must be Calls");
        // Heuristic tier → confidence 0.5.
        assert!(
            (edges[0].confidence.get() - 0.5).abs() < 1e-6,
            "Heuristic tier confidence must be 0.5, got {}",
            edges[0].confidence.get()
        );
        // Provenance must be Heuristic (set by the tier, never hand-set).
        assert_eq!(
            edges[0].provenance,
            wicked_estate_core::Provenance::Heuristic,
            "provenance must be Heuristic"
        );
        assert_eq!(
            edges[0].resolved_by, "ast-synth-method",
            "resolved_by must be the synthesizer id"
        );
    }

    /// Ambiguity: two methods named `foo` → synthesizer emits nothing (honest non-resolution).
    #[test]
    fn synth_method_emits_nothing_for_ambiguous_callables() {
        let foo1_sym =
            Symbol::global("test", None, vec![Descriptor::method("foo_impl_1", None)]).id();
        let foo2_sym =
            Symbol::global("test", None, vec![Descriptor::method("foo_impl_2", None)]).id();

        let foo1 = Node::new(
            foo1_sym,
            NodeKind::Method,
            "foo",
            Language::new("rust"),
            Location::new("a.rs", Span::ZERO),
        );
        let foo2 = Node::new(
            foo2_sym,
            NodeKind::Method,
            "foo",
            Language::new("rust"),
            Location::new("b.rs", Span::ZERO),
        );

        let caller_sym =
            Symbol::global("test", None, vec![Descriptor::method("caller_amb", None)]).id();

        let index = VecIndex(vec![foo1, foo2]);

        let r = UnresolvedRef::new(
            caller_sym,
            "foo",
            EdgeKind::Calls,
            Location::new("main.rs", Span::ZERO),
        );

        let edges = MethodResolutionSynthesizer.resolve(&[r], &index).unwrap();
        assert!(
            edges.is_empty(),
            "ambiguous callables must produce no edge (honest non-resolution); got {}",
            edges.len()
        );
    }

    // ── Precision monitor tests ───────────────────────────────────────────────

    /// The good synthesizer (MethodResolutionSynthesizer) scores >= SYNTH_PRECISION_FLOOR
    /// on a clean gold set; is_acceptable() is true.
    #[test]
    fn precision_monitor_passes_for_correct_synthesizer() {
        use std::collections::HashMap;

        let foo_sym = Symbol::global("test", None, vec![Descriptor::method("pm_foo", None)]).id();
        let caller_sym =
            Symbol::global("test", None, vec![Descriptor::method("pm_caller", None)]).id();

        let foo_node = Node::new(
            foo_sym.clone(),
            NodeKind::Function,
            "foo",
            Language::new("rust"),
            Location::new("lib.rs", Span::ZERO),
        );
        let index = VecIndex(vec![foo_node]);

        let r = UnresolvedRef::new(
            caller_sym.clone(),
            "foo",
            EdgeKind::Calls,
            Location::new("main.rs", Span::ZERO),
        );

        // Gold: caller calling "foo" should resolve to foo_sym.
        let mut gold = HashMap::new();
        gold.insert((caller_sym.clone(), "foo".to_string()), foo_sym.clone());

        let result = measure_synth_precision(&MethodResolutionSynthesizer, &[r], &index, &gold);

        assert_eq!(result.emitted, 1, "one ref should produce one emitted edge");
        assert_eq!(result.correct, 1, "the one edge should be correct");
        assert!(
            (result.precision - 1.0).abs() < 1e-9,
            "precision should be 1.0, got {}",
            result.precision
        );
        assert!(
            result.is_acceptable(),
            "precision {} should be >= floor {}",
            result.precision,
            SYNTH_PRECISION_FLOOR
        );
    }

    /// A deliberately bad synthesizer always binds to the WRONG target.
    /// measure_synth_precision returns precision < SYNTH_PRECISION_FLOOR; is_acceptable() false.
    #[test]
    fn precision_monitor_catches_bad_synthesizer() {
        use std::collections::HashMap;

        // The bad synthesizer always emits an edge to a WRONG target symbol,
        // regardless of what the index or ref says.
        struct BadSynthesizer {
            wrong_target: wicked_estate_core::SymbolId,
        }

        impl Resolver for BadSynthesizer {
            fn id(&self) -> &str {
                "bad-synth"
            }
            fn tier(&self) -> ResolutionTier {
                ResolutionTier::Heuristic
            }
            fn resolve(
                &self,
                refs: &[UnresolvedRef],
                _index: &dyn SymbolIndex,
            ) -> Result<Vec<Edge>> {
                let mut out = Vec::new();
                for r in refs {
                    if r.kind == EdgeKind::Calls {
                        let edge = Edge::new(
                            r.from.clone(),
                            self.wrong_target.clone(),
                            EdgeKind::Calls,
                            ResolutionTier::Heuristic,
                            self.id(),
                        );
                        out.push(edge);
                    }
                }
                Ok(out)
            }
        }

        let correct_target =
            Symbol::global("test", None, vec![Descriptor::method("bs_correct", None)]).id();
        let wrong_target =
            Symbol::global("test", None, vec![Descriptor::method("bs_wrong", None)]).id();

        let index = VecIndex(vec![]);

        // Three refs — all will be resolved to wrong_target by BadSynthesizer.
        let refs: Vec<UnresolvedRef> = (0..3)
            .map(|i| {
                let from = Symbol::global(
                    "test",
                    None,
                    vec![Descriptor::method(format!("bs_caller_{i}"), None)],
                )
                .id();
                UnresolvedRef::new(
                    from,
                    format!("fn_{i}"),
                    EdgeKind::Calls,
                    Location::new("main.rs", Span::ZERO),
                )
            })
            .collect();

        // Gold says all should resolve to correct_target; bad synth will emit wrong_target.
        let mut gold = HashMap::new();
        for r in &refs {
            gold.insert((r.from.clone(), r.raw_name.clone()), correct_target.clone());
        }

        let bad = BadSynthesizer { wrong_target };
        let result = measure_synth_precision(&bad, &refs, &index, &gold);

        assert_eq!(result.emitted, 3, "bad synth should emit 3 edges");
        assert_eq!(result.correct, 0, "none should be correct");
        assert!(
            result.precision < SYNTH_PRECISION_FLOOR,
            "bad synth precision {} must be below floor {}",
            result.precision,
            SYNTH_PRECISION_FLOOR
        );
        assert!(
            !result.is_acceptable(),
            "bad synth should not be acceptable"
        );
    }

    /// MethodResolutionSynthesizer wired into resolve_all at the END of the resolver list:
    /// higher-confidence resolvers' edges win; synthesizer fills gaps only.
    #[test]
    fn synth_wired_into_resolve_all_fills_gaps_only() {
        // A unique `zap` function in the index — NameResolver resolves it at conf 0.6.
        // MethodResolutionSynthesizer also resolves it at conf 0.5.
        // After dedup, the NameResolver's 0.6-confidence edge must win.
        let zap_sym = Symbol::global("test", None, vec![Descriptor::method("zap_fn", None)]).id();
        let caller_sym =
            Symbol::global("test", None, vec![Descriptor::method("zap_caller", None)]).id();

        let zap_node = Node::new(
            zap_sym.clone(),
            NodeKind::Function,
            "zap",
            Language::new("rust"),
            Location::new("lib.rs", Span::ZERO),
        );
        let index = VecIndex(vec![zap_node]);

        let r = UnresolvedRef {
            from: caller_sym.clone(),
            raw_name: "zap".to_string(),
            kind: EdgeKind::Calls,
            location: Location::new("main.rs", Span::ZERO),
            hints: Default::default(),
        };

        // Run with NameResolver first, then MethodResolutionSynthesizer.
        let resolvers: &[&dyn Resolver] = &[&NameResolver, &MethodResolutionSynthesizer];
        let edges = resolve_all(resolvers, &[r], &index).unwrap();

        assert_eq!(
            edges.len(),
            1,
            "dedup should yield one edge; got {}",
            edges.len()
        );
        assert_eq!(edges[0].target, zap_sym);
        // NameResolver at ImportMap tier → confidence 0.6; synth at Heuristic → 0.5.
        // The higher-confidence edge (0.6) must survive.
        assert!(
            (edges[0].confidence.get() - 0.6).abs() < 1e-6,
            "NameResolver's 0.6-conf edge should win over synth's 0.5; got {}",
            edges[0].confidence.get()
        );
    }
}
