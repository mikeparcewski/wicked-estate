//! Language-neutral **semantic evidence** (TS-S2C): the versioned envelope every precise producer
//! (a SCIP indexer, a compiler, a database catalog) hands the engine, and the one projection that
//! turns accepted evidence into [`crate::support`] facts.
//!
//! # The contract
//!
//! * **One envelope, versioned.** [`SemanticEvidence`] carries `schema_version`
//!   ([`SEMANTIC_EVIDENCE_SCHEMA_VERSION`]); any other version is rejected, and every wire struct
//!   denies unknown fields, so a v1 reader never silently drops a later meaning.
//! * **A declared producer profile.** [`ProducerProfile`] names the producer, its version, its
//!   [`ProducerClass`] (an index or a compiler) and the [`Capability`] set it vouches for. A fact
//!   of a kind the profile does not declare is never projected; it is counted
//!   ([`EvidenceSkip::UndeclaredCapability`]).
//! * **Opaque identities.** `producer`, `snapshot`, `fact_id` and every producer `symbol` are
//!   compared byte for byte, never trimmed, case-folded or Unicode-normalized. The only rules are
//!   the support plane's: non-empty and NUL-free, and an owner part (`producer`, `snapshot`) is
//!   not whitespace-only.
//! * **References are references.** A [`EvidenceFact::Reference`] projects to
//!   [`EdgeKind::References`], whatever shape its target has. Only a [`EvidenceFact::Call`] —
//!   explicit, site-level call evidence from a producer that declares [`Capability::Calls`] — can
//!   project to [`EdgeKind::Calls`], and only when its target is [`CallTarget::Exact`] and
//!   correlates. SCIP has no call role, so the SCIP adapter never emits calls.
//! * **Deterministic correlation.** Producer symbols are tied to graph nodes through definition
//!   facts: the unique innermost *structural* node of the document (value slots, `File` and
//!   `Import` nodes excluded) whose span contains the whole definition site **and** whose `name`
//!   equals the fact's `name`. A reference or call site's source is the unique innermost
//!   structural node containing the site, or the document's `File` node for a module-level use.
//!   Ties, misses, ambiguous and dynamic targets are never guessed: each is counted in the
//!   [`EvidenceReport`] by [`EvidenceSkip`] reason. The input is a set; order never decides.
//! * **The support plane, not a second model.** Accepted facts are [`SupportFact`]s owned by
//!   `SupportOwner(producer, snapshot)` and written with
//!   [`GraphWrite::replace_edge_supports`](crate::traits::GraphWrite::replace_edge_supports):
//!   complete replacement, monotonic generations, producer isolation, last-support restoration.
//!   Tree-sitter output stays the base plane, untouched.
//!
//! Contract text: `docs/ENGINE-CONTRACT.md` §3.5.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::edge::{Edge, EdgeKind, ResolutionTier};
use crate::error::{Error, Result};
use crate::flow::is_structural_symbol;
use crate::node::{Location, Node, NodeKind, Span};
use crate::support::{MAX_SUPPORT_GENERATION, SupportFact, SupportOwner, check_opaque};
use crate::symbol::SymbolId;
use crate::traits::GraphStore;

/// The envelope version this build reads and writes.
pub const SEMANTIC_EVIDENCE_SCHEMA_VERSION: u32 = 1;

/// Edge metadata key: the producer version that asserted a projected fact.
pub const EVIDENCE_PRODUCER_VERSION_KEY: &str = "evidence_producer_version";
/// Edge metadata key: which fact kind (`reference` / `call`) a projected edge came from.
pub const EVIDENCE_FACT_KEY: &str = "evidence_fact";
/// Edge metadata key: the occurrence roles a reference carried (`import`, `read`, `write`, `test`).
pub const EVIDENCE_ROLES_KEY: &str = "evidence_roles";

/// One producer's complete evidence for one snapshot. Replaying it replaces everything the
/// `(producer.name, snapshot)` owner asserted before.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticEvidence {
    pub schema_version: u32,
    pub producer: ProducerProfile,
    /// The unit this producer re-emits whole (one index, one compilation unit). Opaque, and unique
    /// within one graph: two repositories ingesting under the same producer must not share it.
    pub snapshot: String,
    /// The producer's own monotonic generation, when it has one. Without it the engine applies the
    /// owner's next generation (stored + 1, or 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
    /// Every document a fact cites, with the unit its columns count in.
    pub documents: Vec<EvidenceDocument>,
    pub facts: Vec<EvidenceFact>,
}

/// Who asserts the evidence and what it vouches for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerProfile {
    /// Opaque producer name — the support owner's `producer` and every projected edge's
    /// `resolved_by` (`scip-typescript`, `plscope`, `angular-compiler`).
    pub name: String,
    pub version: String,
    pub class: ProducerClass,
    pub capabilities: BTreeSet<Capability>,
}

/// What kind of system produced the evidence; decides the projected edge's tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerClass {
    /// A precise code index (SCIP): [`ResolutionTier::Scip`].
    Index,
    /// A compiler or toolchain catalog (Angular compiler, PL/Scope): [`ResolutionTier::Compiler`].
    Compiler,
}

impl ProducerClass {
    pub fn tier(self) -> ResolutionTier {
        match self {
            ProducerClass::Index => ResolutionTier::Scip,
            ProducerClass::Compiler => ResolutionTier::Compiler,
        }
    }
}

/// A fact kind a producer vouches for. Facts of an undeclared kind are counted, never projected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Definitions,
    References,
    /// Explicit, site-level call evidence. Declaring it is necessary but not sufficient: each call
    /// still needs its own site and an exact target.
    Calls,
}

/// One document a fact may cite. `path` is repository-relative with `/` separators.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceDocument {
    pub path: String,
    pub position_encoding: PositionEncoding,
}

/// The unit an [`EvidenceRange`] column counts in. The graph's own columns are UTF-8 bytes; other
/// encodings are converted against the document's source text when it is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PositionEncoding {
    Utf8,
    Utf16,
    Utf32,
    /// The producer did not say; columns are used as given (exact for ASCII lines).
    Unspecified,
}

/// A 0-based, half-open source range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRange {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

/// Where a fact occurs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceSite {
    pub document: String,
    pub range: EvidenceRange,
}

/// How a reference uses its target, when the producer says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceRole {
    Import,
    Read,
    Write,
    Test,
}

/// The target of an explicit call site.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "resolution", rename_all = "snake_case", deny_unknown_fields)]
pub enum CallTarget {
    /// The producer resolved the callee to one symbol.
    Exact { symbol: String },
    /// The producer saw several possible callees; none is projected.
    Ambiguous { candidates: Vec<String> },
    /// The callee is decided at run time; nothing is projected.
    Dynamic,
}

/// One fact. Its `fact_id` is producer-owned and opaque (the support fact id).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidenceFact {
    /// `symbol` is declared at `site`, and is spelled `name` in source.
    Definition {
        fact_id: String,
        symbol: String,
        name: String,
        site: EvidenceSite,
    },
    /// `symbol` is used at `site`. Never a call, whatever the target is.
    Reference {
        fact_id: String,
        symbol: String,
        site: EvidenceSite,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        roles: Vec<ReferenceRole>,
    },
    /// An explicit call at `site`.
    Call {
        fact_id: String,
        site: EvidenceSite,
        target: CallTarget,
    },
}

impl EvidenceFact {
    pub fn fact_id(&self) -> &str {
        match self {
            EvidenceFact::Definition { fact_id, .. }
            | EvidenceFact::Reference { fact_id, .. }
            | EvidenceFact::Call { fact_id, .. } => fact_id,
        }
    }

    pub fn site(&self) -> &EvidenceSite {
        match self {
            EvidenceFact::Definition { site, .. }
            | EvidenceFact::Reference { site, .. }
            | EvidenceFact::Call { site, .. } => site,
        }
    }

    fn capability(&self) -> Capability {
        match self {
            EvidenceFact::Definition { .. } => Capability::Definitions,
            EvidenceFact::Reference { .. } => Capability::References,
            EvidenceFact::Call { .. } => Capability::Calls,
        }
    }
}

/// Why a fact (or an adapter occurrence) was not projected. Serialized snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EvidenceSkip {
    /// The fact's kind is not in the producer profile's capabilities.
    UndeclaredCapability,
    /// The cited document has no nodes in the graph (not indexed, or another repository).
    DocumentNotInGraph,
    /// No structural node of that name contains the definition site.
    UnmappedDefinition,
    /// Two innermost nodes qualify for a definition.
    AmbiguousDefinition,
    /// Two innermost nodes contain a reference or call site.
    AmbiguousSource,
    /// The target symbol has no mapped definition (external, local, or unmapped).
    UnknownTarget,
    /// The target symbol maps to more than one node, or the producer listed candidates.
    AmbiguousTarget,
    /// The producer marked the call target dynamic.
    DynamicTarget,
    /// A reference from a node to itself.
    SelfReference,
    /// Adapter: the occurrence is generated, not user-authored.
    Generated,
    /// Adapter: a forward declaration, not the definition.
    ForwardDefinition,
    /// Adapter: a module/package-level symbol (the base plane's `Imports` already cover files).
    ModuleSymbol,
    /// The range is malformed: an adapter could not read it, or the document's text is known and
    /// the site cannot exist in it.
    MalformedRange,
}

/// What one ingestion did. `skipped` counts every fact that was not projected, by reason.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EvidenceReport {
    pub producer: String,
    pub snapshot: String,
    /// The generation applied (filled by [`ingest_evidence`]).
    pub generation: Option<u64>,
    /// `true` when the store saw an identical replay of its stored generation.
    pub replayed: bool,
    pub definitions_mapped: usize,
    pub references_projected: usize,
    pub calls_projected: usize,
    /// Distinct public edges `(source, target, kind)` those facts support. Several facts (sites)
    /// can support one edge, so this is at most `references_projected + calls_projected`.
    pub edges_projected: usize,
    /// Facts projected with columns used as given, because the producer's encoding is not UTF-8
    /// and the document's text was unavailable.
    pub positions_unconverted: usize,
    pub skipped: BTreeMap<EvidenceSkip, usize>,
}

impl EvidenceReport {
    /// Support facts written (references + calls).
    pub fn projected(&self) -> usize {
        self.references_projected + self.calls_projected
    }

    /// Add `n` skips for `reason` (adapters fold their own counts in this way).
    pub fn skip(&mut self, reason: EvidenceSkip, n: usize) {
        if n > 0 {
            *self.skipped.entry(reason).or_default() += n;
        }
    }
}

/// The accepted facts of one envelope, ready for `replace_edge_supports`, and the report.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EvidenceProjection {
    pub owner: SupportOwner,
    pub facts: Vec<SupportFact>,
    pub report: EvidenceReport,
}

/// Whether `path` is a document path the envelope can carry: repository-relative, non-empty,
/// `/`-separated, with no `.`, `..` or empty segment, no drive letter and no NUL or `\\`.
pub fn is_document_path(path: &str) -> bool {
    !(path.is_empty()
        || path.contains('\0')
        || path.contains('\\')
        || path.starts_with('/')
        || path.as_bytes().get(1) == Some(&b':')
        || path
            .split('/')
            .any(|seg| seg.is_empty() || seg == "." || seg == ".."))
}

fn check_document_path(path: &str) -> Result<()> {
    if !is_document_path(path) {
        return Err(Error::Invalid(format!(
            "evidence document path must be repository-relative with '/' separators and no '.', \
             '..' or empty segments, got {path:?}"
        )));
    }
    Ok(())
}

fn check_range(fact_id: &str, r: &EvidenceRange) -> Result<()> {
    if (r.start_line, r.start_col) > (r.end_line, r.end_col) {
        return Err(Error::Invalid(format!(
            "evidence fact {fact_id:?} has a range that ends before it starts: {r:?}"
        )));
    }
    Ok(())
}

impl SemanticEvidence {
    /// Reject an envelope this build cannot read faithfully. Nothing is projected from an invalid
    /// envelope, and nothing is written.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SEMANTIC_EVIDENCE_SCHEMA_VERSION {
            return Err(Error::Invalid(format!(
                "unsupported semantic-evidence schema_version {}; this build reads {}",
                self.schema_version, SEMANTIC_EVIDENCE_SCHEMA_VERSION
            )));
        }
        check_opaque("evidence producer name", &self.producer.name)?;
        check_opaque("evidence producer version", &self.producer.version)?;
        check_opaque("evidence snapshot", &self.snapshot)?;
        SupportOwner::new(self.producer.name.clone(), self.snapshot.clone())?;
        if let Some(g) = self.generation.filter(|g| *g > MAX_SUPPORT_GENERATION) {
            return Err(Error::Invalid(format!(
                "evidence generation {g} exceeds {MAX_SUPPORT_GENERATION}"
            )));
        }
        let mut docs = BTreeSet::new();
        for d in &self.documents {
            check_document_path(&d.path)?;
            if !docs.insert(d.path.as_str()) {
                return Err(Error::Invalid(format!(
                    "evidence document {:?} is declared twice",
                    d.path
                )));
            }
        }
        let mut ids: BTreeMap<&str, &EvidenceFact> = BTreeMap::new();
        for f in &self.facts {
            let id = f.fact_id();
            check_opaque("evidence fact_id", id)?;
            match f {
                EvidenceFact::Definition { symbol, name, .. } => {
                    check_opaque("evidence symbol", symbol)?;
                    check_opaque("evidence definition name", name)?;
                }
                EvidenceFact::Reference { symbol, .. } => check_opaque("evidence symbol", symbol)?,
                EvidenceFact::Call { target, .. } => match target {
                    CallTarget::Exact { symbol } => check_opaque("evidence call target", symbol)?,
                    CallTarget::Ambiguous { candidates } => {
                        for c in candidates {
                            check_opaque("evidence call candidate", c)?;
                        }
                    }
                    CallTarget::Dynamic => {}
                },
            }
            let site = f.site();
            if !docs.contains(site.document.as_str()) {
                return Err(Error::Invalid(format!(
                    "evidence fact {id:?} cites document {:?}, which the envelope does not declare",
                    site.document
                )));
            }
            check_range(id, &site.range)?;
            if ids.insert(id, f).is_some_and(|prev| prev != f) {
                return Err(Error::Invalid(format!(
                    "evidence fact_id {id:?} is used for two different facts"
                )));
            }
        }
        Ok(())
    }

    /// The support owner this envelope replaces.
    pub fn owner(&self) -> Result<SupportOwner> {
        SupportOwner::new(self.producer.name.clone(), self.snapshot.clone())
    }
}

/// `(line, col)` pairs in the graph's coordinates (UTF-8 byte columns).
type Pos = (u32, u32);

fn contains(span: &Span, start: Pos, end: Pos) -> bool {
    (span.start_line, span.start_col) <= start && end <= (span.end_line, span.end_col)
}

fn span_contains_span(outer: &Span, inner: &Span) -> bool {
    contains(
        outer,
        (inner.start_line, inner.start_col),
        (inner.end_line, inner.end_col),
    )
}

/// The unique innermost node among `cands` (each already containing the site): the one no other
/// candidate nests inside. Two with an identical span, or two crossing ones, are ambiguous.
fn innermost<'a>(cands: &[&'a Node]) -> std::result::Result<Option<&'a Node>, ()> {
    let minimal: Vec<&Node> = cands
        .iter()
        .copied()
        .filter(|n| {
            !cands.iter().any(|m| {
                m.symbol != n.symbol
                    && span_contains_span(&n.location.span, &m.location.span)
                    && m.location.span != n.location.span
            })
        })
        .collect();
    match minimal.len() {
        0 => Ok(None),
        1 => Ok(Some(minimal[0])),
        _ => Err(()),
    }
}

/// Byte offsets of each line start, and the text, for column conversion.
struct DocText {
    text: String,
    line_starts: Vec<usize>,
}

impl DocText {
    fn new(text: String) -> Self {
        let mut line_starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self { text, line_starts }
    }

    fn line(&self, line: u32) -> Option<&str> {
        let start = *self.line_starts.get(line as usize)?;
        let end = self
            .line_starts
            .get(line as usize + 1)
            .map(|e| e - 1)
            .unwrap_or(self.text.len());
        self.text.get(start..end)
    }

    /// Convert a column in `enc` units to a UTF-8 byte column. `None` past the line's end.
    fn byte_col(&self, line: u32, col: u32, enc: PositionEncoding) -> Option<u32> {
        let text = self.line(line)?;
        if matches!(enc, PositionEncoding::Utf8 | PositionEncoding::Unspecified) {
            return (col as usize <= text.len()).then_some(col);
        }
        let mut units = 0u32;
        for (byte, ch) in text.char_indices() {
            if units >= col {
                return (units == col).then_some(byte as u32);
            }
            units += match enc {
                PositionEncoding::Utf16 => ch.len_utf16() as u32,
                _ => 1,
            };
        }
        (units == col).then_some(text.len() as u32)
    }

    fn byte_offset(&self, line: u32, byte_col: u32) -> u32 {
        self.line_starts
            .get(line as usize)
            .map(|s| (*s as u32).saturating_add(byte_col))
            .unwrap_or(0)
    }
}

struct Docs<'a> {
    encodings: HashMap<&'a str, PositionEncoding>,
    texts: HashMap<&'a str, Option<DocText>>,
}

impl<'a> Docs<'a> {
    /// The site in graph coordinates, and whether its columns had to be used unconverted.
    /// `None` when the document's text is known and the site cannot exist in it (a column past
    /// its line, a line past its end): such a site is malformed, never used raw.
    fn span(&self, site: &EvidenceSite) -> Option<(Span, bool)> {
        let r = site.range;
        let enc = self
            .encodings
            .get(site.document.as_str())
            .copied()
            .unwrap_or(PositionEncoding::Unspecified);
        let raw = Span {
            start_byte: 0,
            end_byte: 0,
            start_line: r.start_line,
            start_col: r.start_col,
            end_line: r.end_line,
            end_col: r.end_col,
        };
        let Some(Some(doc)) = self.texts.get(site.document.as_str()) else {
            let unconverted = matches!(enc, PositionEncoding::Utf16 | PositionEncoding::Utf32);
            return Some((raw, unconverted));
        };
        match (
            doc.byte_col(r.start_line, r.start_col, enc),
            doc.byte_col(r.end_line, r.end_col, enc),
        ) {
            (Some(sc), Some(ec)) => Some((
                Span {
                    start_byte: doc.byte_offset(r.start_line, sc),
                    end_byte: doc.byte_offset(r.end_line, ec),
                    start_col: sc,
                    end_col: ec,
                    ..raw
                },
                false,
            )),
            _ => None,
        }
    }
}

/// Project one envelope against the graph's `nodes` (repository-relative paths, as the documents
/// cite them). `source` returns a document's text, used only to convert non-UTF-8 columns; return
/// `None` when it is unavailable. Validates the envelope first; writes nothing.
pub fn project_evidence(
    evidence: &SemanticEvidence,
    nodes: &[Node],
    source: &dyn Fn(&str) -> Option<String>,
) -> Result<EvidenceProjection> {
    evidence.validate()?;
    let owner = evidence.owner()?;
    let mut report = EvidenceReport {
        producer: evidence.producer.name.clone(),
        snapshot: evidence.snapshot.clone(),
        ..EvidenceReport::default()
    };

    let mut candidates: HashMap<&str, Vec<&Node>> = HashMap::new();
    let mut files: HashMap<&str, Vec<&Node>> = HashMap::new();
    for n in nodes {
        let file = n.location.file.as_str();
        match n.kind {
            NodeKind::File => files.entry(file).or_default().push(n),
            NodeKind::Import => {}
            _ if is_structural_symbol(n) => candidates.entry(file).or_default().push(n),
            _ => {}
        }
    }
    let in_graph = |doc: &str| candidates.contains_key(doc) || files.contains_key(doc);

    let docs = Docs {
        encodings: evidence
            .documents
            .iter()
            .map(|d| (d.path.as_str(), d.position_encoding))
            .collect(),
        texts: evidence
            .documents
            .iter()
            .map(|d| {
                let needs = matches!(
                    d.position_encoding,
                    PositionEncoding::Utf16 | PositionEncoding::Utf32
                );
                let text = if needs && in_graph(&d.path) {
                    source(&d.path).map(DocText::new)
                } else {
                    None
                };
                (d.path.as_str(), text)
            })
            .collect(),
    };

    // The input is a set: dedupe and order by content so nothing depends on submission order.
    let facts: BTreeSet<&EvidenceFact> = evidence.facts.iter().collect();
    let caps = &evidence.producer.capabilities;

    // Pass 1: definitions → symbol → the nodes it maps to.
    let mut defs: BTreeMap<&str, BTreeSet<&SymbolId>> = BTreeMap::new();
    for f in &facts {
        let EvidenceFact::Definition {
            symbol, name, site, ..
        } = f
        else {
            continue;
        };
        if !caps.contains(&Capability::Definitions) {
            report.skip(EvidenceSkip::UndeclaredCapability, 1);
            continue;
        }
        if !in_graph(&site.document) {
            report.skip(EvidenceSkip::DocumentNotInGraph, 1);
            continue;
        }
        let Some((span, unconverted)) = docs.span(site) else {
            report.skip(EvidenceSkip::MalformedRange, 1);
            continue;
        };
        let start = (span.start_line, span.start_col);
        let end = (span.end_line, span.end_col);
        let named: Vec<&Node> = candidates
            .get(site.document.as_str())
            .into_iter()
            .flatten()
            .copied()
            .filter(|n| n.name == *name && contains(&n.location.span, start, end))
            .collect();
        match innermost(&named) {
            Ok(Some(node)) => {
                defs.entry(symbol.as_str())
                    .or_default()
                    .insert(&node.symbol);
                report.definitions_mapped += 1;
                if unconverted {
                    report.positions_unconverted += 1;
                }
            }
            Ok(None) => report.skip(EvidenceSkip::UnmappedDefinition, 1),
            Err(()) => report.skip(EvidenceSkip::AmbiguousDefinition, 1),
        }
    }

    let target_of = |symbol: &str| -> std::result::Result<&SymbolId, EvidenceSkip> {
        match defs.get(symbol) {
            None => Err(EvidenceSkip::UnknownTarget),
            Some(set) if set.len() == 1 => Ok(set.iter().next().copied().expect("one")),
            Some(_) => Err(EvidenceSkip::AmbiguousTarget),
        }
    };
    let tier = evidence.producer.class.tier();

    // Pass 2: references and calls → support facts.
    let mut out = Vec::new();
    for f in &facts {
        let (fact_id, site, kind, target, roles) = match f {
            EvidenceFact::Definition { .. } => continue,
            EvidenceFact::Reference {
                fact_id,
                symbol,
                site,
                roles,
            } => (
                fact_id,
                site,
                EdgeKind::References,
                Ok(symbol.as_str()),
                roles.as_slice(),
            ),
            EvidenceFact::Call {
                fact_id,
                site,
                target,
            } => {
                let t = match target {
                    CallTarget::Exact { symbol } => Ok(symbol.as_str()),
                    CallTarget::Ambiguous { .. } => Err(EvidenceSkip::AmbiguousTarget),
                    CallTarget::Dynamic => Err(EvidenceSkip::DynamicTarget),
                };
                (fact_id, site, EdgeKind::Calls, t, &[][..])
            }
        };
        if !caps.contains(&f.capability()) {
            report.skip(EvidenceSkip::UndeclaredCapability, 1);
            continue;
        }
        if !in_graph(&site.document) {
            report.skip(EvidenceSkip::DocumentNotInGraph, 1);
            continue;
        }
        let target = match target.and_then(target_of) {
            Ok(t) => t,
            Err(reason) => {
                report.skip(reason, 1);
                continue;
            }
        };
        let Some((span, unconverted)) = docs.span(site) else {
            report.skip(EvidenceSkip::MalformedRange, 1);
            continue;
        };
        let start = (span.start_line, span.start_col);
        let end = (span.end_line, span.end_col);
        let around: Vec<&Node> = candidates
            .get(site.document.as_str())
            .into_iter()
            .flatten()
            .copied()
            .filter(|n| contains(&n.location.span, start, end))
            .collect();
        let source_node = match innermost(&around) {
            Ok(Some(n)) => n,
            Ok(None) => match files.get(site.document.as_str()).map(Vec::as_slice) {
                Some([file]) => *file,
                Some([_, _, ..]) => {
                    report.skip(EvidenceSkip::AmbiguousSource, 1);
                    continue;
                }
                _ => {
                    report.skip(EvidenceSkip::DocumentNotInGraph, 1);
                    continue;
                }
            },
            Err(()) => {
                report.skip(EvidenceSkip::AmbiguousSource, 1);
                continue;
            }
        };
        if kind == EdgeKind::References && source_node.symbol == *target {
            report.skip(EvidenceSkip::SelfReference, 1);
            continue;
        }
        let mut edge = Edge::new(
            source_node.symbol.clone(),
            target.clone(),
            kind.clone(),
            tier,
            evidence.producer.name.clone(),
        )
        .with_location(Location::new(site.document.clone(), span));
        edge.metadata.insert(
            EVIDENCE_PRODUCER_VERSION_KEY.into(),
            evidence.producer.version.clone().into(),
        );
        let fact_word = if kind == EdgeKind::Calls {
            "call"
        } else {
            "reference"
        };
        edge.metadata
            .insert(EVIDENCE_FACT_KEY.into(), fact_word.into());
        if !roles.is_empty() {
            let roles: BTreeSet<&ReferenceRole> = roles.iter().collect();
            edge.metadata.insert(
                EVIDENCE_ROLES_KEY.into(),
                serde_json::to_value(roles).expect("roles serialize"),
            );
        }
        out.push(SupportFact::new(fact_id.clone(), edge)?);
        if kind == EdgeKind::Calls {
            report.calls_projected += 1;
        } else {
            report.references_projected += 1;
        }
        if unconverted {
            report.positions_unconverted += 1;
        }
    }

    report.edges_projected = out.iter().map(|f| &f.key).collect::<BTreeSet<_>>().len();
    Ok(EvidenceProjection {
        owner,
        facts: out,
        report,
    })
}

/// Project `evidence` against `nodes` and make the result its owner's complete support set
/// ([`project_evidence`] then [`apply_projection`]).
pub fn ingest_evidence<S: GraphStore + ?Sized>(
    store: &mut S,
    evidence: &SemanticEvidence,
    nodes: &[Node],
    source: &dyn Fn(&str) -> Option<String>,
) -> Result<EvidenceReport> {
    let projection = project_evidence(evidence, nodes, source)?;
    apply_projection(store, projection, evidence.generation)
}

/// Write a projection as its owner's complete support set.
///
/// The generation is `generation` (the producer's own), or the owner's stored generation + 1 (1
/// for a new owner). The support-plane rules then apply unchanged: an identical replay of the
/// stored generation is a no-op (`replayed`); a different set at the stored generation, or any
/// older one, is rejected and writes nothing.
pub fn apply_projection<S: GraphStore + ?Sized>(
    store: &mut S,
    projection: EvidenceProjection,
    generation: Option<u64>,
) -> Result<EvidenceReport> {
    let generation = match generation {
        Some(g) => g,
        None => match store.support_generation(&projection.owner)? {
            None => 1,
            Some(g) if g >= MAX_SUPPORT_GENERATION => {
                return Err(Error::Invalid(format!(
                    "support owner {:?} is at the largest generation; it cannot advance",
                    projection.owner
                )));
            }
            Some(g) => g + 1,
        },
    };
    let applied = store.replace_edge_supports(&projection.owner, generation, &projection.facts)?;
    let mut report = projection.report;
    report.generation = Some(applied.generation);
    report.replayed = applied.replayed;
    Ok(report)
}

/// An injective id from parts: each part length-prefixed, so no two part lists collide.
pub fn opaque_tuple_id(parts: &[&str]) -> String {
    let mut s = String::new();
    for p in parts {
        s.push_str(&p.len().to_string());
        s.push(':');
        s.push_str(p);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Language;

    fn node(name: &str, kind: NodeKind, file: &str, span: (u32, u32, u32, u32)) -> Node {
        Node::new(
            SymbolId(format!("{file}#{name}@{}", span.0)),
            kind,
            name,
            Language::new("typescript"),
            Location::new(
                file,
                Span {
                    start_byte: 0,
                    end_byte: 0,
                    start_line: span.0,
                    start_col: span.1,
                    end_line: span.2,
                    end_col: span.3,
                },
            ),
        )
    }

    fn site(doc: &str, r: (u32, u32, u32, u32)) -> EvidenceSite {
        EvidenceSite {
            document: doc.into(),
            range: EvidenceRange {
                start_line: r.0,
                start_col: r.1,
                end_line: r.2,
                end_col: r.3,
            },
        }
    }

    fn envelope(caps: &[Capability], facts: Vec<EvidenceFact>) -> SemanticEvidence {
        SemanticEvidence {
            schema_version: SEMANTIC_EVIDENCE_SCHEMA_VERSION,
            producer: ProducerProfile {
                name: "p".into(),
                version: "1".into(),
                class: ProducerClass::Index,
                capabilities: caps.iter().copied().collect(),
            },
            snapshot: "s".into(),
            generation: None,
            documents: vec![EvidenceDocument {
                path: "a.ts".into(),
                position_encoding: PositionEncoding::Utf8,
            }],
            facts,
        }
    }

    fn def(id: &str, symbol: &str, name: &str, r: (u32, u32, u32, u32)) -> EvidenceFact {
        EvidenceFact::Definition {
            fact_id: id.into(),
            symbol: symbol.into(),
            name: name.into(),
            site: site("a.ts", r),
        }
    }

    fn reference(id: &str, symbol: &str, r: (u32, u32, u32, u32)) -> EvidenceFact {
        EvidenceFact::Reference {
            fact_id: id.into(),
            symbol: symbol.into(),
            site: site("a.ts", r),
            roles: vec![],
        }
    }

    fn graph() -> Vec<Node> {
        vec![
            node("a.ts", NodeKind::File, "a.ts", (0, 0, 0, 0)),
            node("helper", NodeKind::Function, "a.ts", (0, 7, 2, 1)),
            node("run", NodeKind::Function, "a.ts", (4, 7, 8, 1)),
        ]
    }

    fn no_source(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn a_function_shaped_reference_never_becomes_a_call() {
        let ev = envelope(
            &[Capability::Definitions, Capability::References],
            vec![
                def("d1", "helper().", "helper", (0, 16, 0, 22)),
                reference("r1", "helper().", (5, 12, 5, 18)),
            ],
        );
        let p = project_evidence(&ev, &graph(), &no_source).unwrap();
        assert_eq!(p.facts.len(), 1);
        assert_eq!(p.facts[0].edge.kind, EdgeKind::References);
        assert_eq!(p.facts[0].edge.source.0, "a.ts#run@4");
        assert_eq!(p.facts[0].edge.target.0, "a.ts#helper@0");
        assert_eq!(p.facts[0].edge.resolved_by, "p");
        assert_eq!(p.report.references_projected, 1);
        assert_eq!(p.report.calls_projected, 0);
    }

    #[test]
    fn trusted_calls_need_the_capability_site_and_an_exact_target() {
        let call = |id: &str, target: CallTarget| EvidenceFact::Call {
            fact_id: id.into(),
            site: site("a.ts", (5, 12, 5, 18)),
            target,
        };
        let facts = vec![
            def("d1", "helper().", "helper", (0, 16, 0, 22)),
            call(
                "c1",
                CallTarget::Exact {
                    symbol: "helper().".into(),
                },
            ),
            call(
                "c2",
                CallTarget::Ambiguous {
                    candidates: vec!["helper().".into(), "x".into()],
                },
            ),
            call("c3", CallTarget::Dynamic),
            call(
                "c4",
                CallTarget::Exact {
                    symbol: "missing".into(),
                },
            ),
        ];
        // Without the capability: nothing trusted.
        let undeclared = envelope(&[Capability::Definitions], facts.clone());
        let p = project_evidence(&undeclared, &graph(), &no_source).unwrap();
        assert!(p.facts.is_empty());
        assert_eq!(p.report.skipped[&EvidenceSkip::UndeclaredCapability], 4);
        // With it: only the exact, correlated call.
        let declared = envelope(&[Capability::Definitions, Capability::Calls], facts);
        let p = project_evidence(&declared, &graph(), &no_source).unwrap();
        assert_eq!(p.facts.len(), 1);
        assert_eq!(p.facts[0].edge.kind, EdgeKind::Calls);
        assert_eq!(p.facts[0].fact_id, "c1");
        assert_eq!(p.report.skipped[&EvidenceSkip::AmbiguousTarget], 1);
        assert_eq!(p.report.skipped[&EvidenceSkip::DynamicTarget], 1);
        assert_eq!(p.report.skipped[&EvidenceSkip::UnknownTarget], 1);
    }

    #[test]
    fn definitions_need_the_name_and_skip_value_slots() {
        let mut nodes = graph();
        let mut slot = node("x", NodeKind::Parameter, "a.ts", (0, 23, 0, 24));
        slot.metadata
            .insert(crate::node::VALUE_ROLE_METADATA_KEY.into(), "param".into());
        nodes.push(slot);
        let ev = envelope(
            &[Capability::Definitions],
            vec![
                def("d1", "helper().", "helper", (0, 16, 0, 22)),
                def("d2", "helper().(x)", "x", (0, 23, 0, 24)),
                def("d3", "other", "other", (0, 16, 0, 22)),
            ],
        );
        let p = project_evidence(&ev, &nodes, &no_source).unwrap();
        assert_eq!(p.report.definitions_mapped, 1);
        assert_eq!(p.report.skipped[&EvidenceSkip::UnmappedDefinition], 2);
    }

    #[test]
    fn ties_are_ambiguous_not_decided_by_order() {
        let mut nodes = graph();
        // A second node with the identical span and name as `helper`.
        let mut twin = node("helper", NodeKind::Function, "a.ts", (0, 7, 2, 1));
        twin.symbol = SymbolId("twin".into());
        nodes.push(twin);
        let ev = envelope(
            &[Capability::Definitions, Capability::References],
            vec![
                def("d1", "helper().", "helper", (0, 16, 0, 22)),
                reference("r1", "helper().", (5, 12, 5, 18)),
            ],
        );
        for order in [nodes.clone(), nodes.into_iter().rev().collect()] {
            let p = project_evidence(&ev, &order, &no_source).unwrap();
            assert!(p.facts.is_empty());
            assert_eq!(p.report.skipped[&EvidenceSkip::AmbiguousDefinition], 1);
            assert_eq!(p.report.skipped[&EvidenceSkip::UnknownTarget], 1);
        }
    }

    #[test]
    fn module_level_uses_come_from_the_file_and_self_references_are_dropped() {
        let ev = envelope(
            &[Capability::Definitions, Capability::References],
            vec![
                def("d1", "helper().", "helper", (0, 16, 0, 22)),
                reference("top", "helper().", (3, 0, 3, 6)),
                reference("rec", "helper().", (1, 2, 1, 8)),
            ],
        );
        let p = project_evidence(&ev, &graph(), &no_source).unwrap();
        assert_eq!(p.facts.len(), 1);
        assert_eq!(p.facts[0].edge.source.0, "a.ts#a.ts@0");
        assert_eq!(p.report.skipped[&EvidenceSkip::SelfReference], 1);
    }

    #[test]
    fn opaque_ids_stay_distinct_by_whitespace_case_and_normalization() {
        let nodes = vec![
            node("a.ts", NodeKind::File, "a.ts", (0, 0, 0, 0)),
            node("User", NodeKind::Class, "a.ts", (0, 0, 0, 20)),
            node("user", NodeKind::Function, "a.ts", (1, 0, 1, 20)),
            node("caf\u{e9}", NodeKind::Function, "a.ts", (2, 0, 2, 20)),
            node("cafe\u{301}", NodeKind::Function, "a.ts", (3, 0, 3, 20)),
            node("run", NodeKind::Function, "a.ts", (5, 0, 9, 1)),
        ];
        let ev = envelope(
            &[Capability::Definitions, Capability::References],
            vec![
                def("d1", "User", "User", (0, 6, 0, 10)),
                def("d2", "user", "user", (1, 9, 1, 13)),
                def("d3", "caf\u{e9}", "caf\u{e9}", (2, 9, 2, 13)),
                def("d4", "cafe\u{301}", "cafe\u{301}", (3, 9, 3, 14)),
                reference("r", "User", (6, 0, 6, 4)),
                reference("r ", "User", (6, 0, 6, 4)),
                reference("R", "user", (7, 0, 7, 4)),
                reference("n1", "caf\u{e9}", (8, 0, 8, 4)),
                reference("n2", "cafe\u{301}", (8, 6, 8, 10)),
            ],
        );
        let p = project_evidence(&ev, &nodes, &no_source).unwrap();
        let mut got: Vec<(String, String)> = p
            .facts
            .iter()
            .map(|f| (f.fact_id.clone(), f.edge.target.0.clone()))
            .collect();
        got.sort();
        assert_eq!(
            got,
            vec![
                ("R".into(), "a.ts#user@1".into()),
                ("n1".into(), "a.ts#caf\u{e9}@2".into()),
                ("n2".into(), "a.ts#cafe\u{301}@3".into()),
                ("r".into(), "a.ts#User@0".into()),
                ("r ".into(), "a.ts#User@0".into()),
            ]
        );
    }

    #[test]
    fn utf16_columns_are_converted_against_the_source() {
        // `const é = 1; function f() { g(); }` — the `é` is 2 bytes but 1 UTF-16 unit.
        let text = "/*\u{e9}*/ function f() { g(); }\n/*\u{1f600}*/ function g() {}\n";
        let nodes = vec![
            node("a.ts", NodeKind::File, "a.ts", (0, 0, 0, 0)),
            node("f", NodeKind::Function, "a.ts", (0, 7, 0, 30)),
            node("g", NodeKind::Function, "a.ts", (1, 9, 1, 25)),
        ];
        let mut ev = envelope(
            &[Capability::Definitions, Capability::References],
            vec![
                // `g` defined at UTF-16 col 17 on line 1 (the emoji is 2 units, 4 bytes).
                def("dg", "g", "g", (1, 17, 1, 18)),
                // `g()` used at UTF-16 col 21 on line 0 (byte 22).
                reference("rg", "g", (0, 21, 0, 22)),
            ],
        );
        ev.documents[0].position_encoding = PositionEncoding::Utf16;
        let with_text = |_: &str| Some(text.to_string());
        let p = project_evidence(&ev, &nodes, &with_text).unwrap();
        assert_eq!(p.facts.len(), 1, "{:?}", p.report);
        assert_eq!(p.facts[0].edge.source.0, "a.ts#f@0");
        let loc = p.facts[0].edge.location.clone().unwrap();
        assert_eq!((loc.span.start_col, loc.span.end_col), (22, 23));
        assert_eq!(p.report.positions_unconverted, 0);
        // Without the text the columns are used as given, and that is counted.
        let p = project_evidence(&ev, &nodes, &no_source).unwrap();
        assert_eq!(
            p.report.positions_unconverted,
            p.report.definitions_mapped + p.facts.len()
        );
    }

    #[test]
    fn a_column_past_the_line_is_malformed_when_the_text_is_known() {
        let text = "function f() {\n  g();\n}\nfunction g() {}\n";
        let nodes = vec![
            node("a.ts", NodeKind::File, "a.ts", (0, 0, 0, 0)),
            node("f", NodeKind::Function, "a.ts", (0, 0, 2, 1)),
            node("g", NodeKind::Function, "a.ts", (3, 0, 3, 15)),
        ];
        let mut ev = envelope(
            &[Capability::Definitions, Capability::References],
            vec![
                def("dg", "g", "g", (3, 9, 3, 10)),
                // Line 1 is `  g();` (6 units): column 10000 cannot exist.
                reference("bogus", "g", (1, 10000, 1, 10001)),
            ],
        );
        ev.documents[0].position_encoding = PositionEncoding::Utf16;
        let with_text = |_: &str| Some(text.to_string());
        let p = project_evidence(&ev, &nodes, &with_text).unwrap();
        assert!(p.facts.is_empty(), "{:?}", p.facts);
        assert_eq!(
            p.report.skipped.get(&EvidenceSkip::MalformedRange),
            Some(&1)
        );
    }

    #[test]
    fn the_envelope_is_strict() {
        let ok = envelope(&[Capability::References], vec![]);
        let json = serde_json::to_value(&ok).unwrap();
        assert!(serde_json::from_value::<SemanticEvidence>(json.clone()).is_ok());
        let mut extra = json.clone();
        extra["surprise"] = 1.into();
        assert!(serde_json::from_value::<SemanticEvidence>(extra).is_err());
        let mut fact_extra = json.clone();
        fact_extra["facts"] = serde_json::json!([{
            "kind": "reference", "fact_id": "f", "symbol": "s",
            "site": {"document": "a.ts", "range": {"start_line":0,"start_col":0,"end_line":0,"end_col":1}},
            "confidence": 0.9
        }]);
        assert!(serde_json::from_value::<SemanticEvidence>(fact_extra).is_err());
        let mut unknown_cap = json.clone();
        unknown_cap["producer"]["capabilities"] = serde_json::json!(["taint"]);
        assert!(serde_json::from_value::<SemanticEvidence>(unknown_cap).is_err());

        let mut v2 = ok.clone();
        v2.schema_version = 2;
        assert!(
            v2.validate()
                .unwrap_err()
                .to_string()
                .contains("schema_version 2")
        );
        let mut undeclared_doc = ok.clone();
        undeclared_doc.facts = vec![EvidenceFact::Reference {
            fact_id: "f".into(),
            symbol: "s".into(),
            site: site("b.ts", (0, 0, 0, 1)),
            roles: vec![],
        }];
        assert!(undeclared_doc.validate().is_err());
        let mut backwards = ok.clone();
        backwards.facts = vec![reference("f", "s", (2, 0, 1, 0))];
        assert!(backwards.validate().is_err());
        let mut clash = ok.clone();
        clash.facts = vec![
            reference("f", "s", (0, 0, 0, 1)),
            reference("f", "t", (0, 0, 0, 1)),
        ];
        assert!(clash.validate().is_err());
        let mut dup_ok = ok.clone();
        dup_ok.facts = vec![
            reference("f", "s", (0, 0, 0, 1)),
            reference("f", "s", (0, 0, 0, 1)),
        ];
        assert!(dup_ok.validate().is_ok());
        for bad in [
            "/abs.ts", "../up.ts", "a\\b.ts", "C:/x.ts", "a//b.ts", "./a.ts", "",
        ] {
            let mut e = ok.clone();
            e.documents[0].path = bad.into();
            assert!(e.validate().is_err(), "{bad:?} must be rejected");
        }
        let mut empty_id = ok.clone();
        empty_id.facts = vec![reference("", "s", (0, 0, 0, 1))];
        assert!(empty_id.validate().is_err());
    }

    #[test]
    fn opaque_tuple_ids_are_injective() {
        assert_ne!(opaque_tuple_id(&["ab", "c"]), opaque_tuple_id(&["a", "bc"]));
        assert_ne!(opaque_tuple_id(&["a:", "b"]), opaque_tuple_id(&["a", ":b"]));
    }
}
