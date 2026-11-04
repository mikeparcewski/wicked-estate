//! A live source's structure as knowledge (DES-KNOWLEDGE-DATA-001 §4.1).
//!
//! A source (a database, warehouse, SaaS API, MCP server) keeps its rows; estate keeps its
//! **structure**: one document per relation or tool, under scope `source:<sourceId>`, plus the typed
//! catalog. Crew's describe submits one **complete generation** at a time
//! ([`KnowledgeEngine::replace_source`]):
//!
//! - ids are stable, derived from the source id, the document key and the section key, so edges to
//!   a relation survive a re-describe;
//! - the whole generation's knowledge is written in ONE store transaction, and every document and
//!   section of the source that is absent from it is tombstoned (hard-deleted) in that same
//!   transaction. A generation that fails validation or fails mid-write commits no knowledge and
//!   tombstones nothing (a linked generation may leave hidden staged link rows, which the source's
//!   next generation deletes: [`crate::links`]);
//! - typed edges another owner (a data shape) put on a tombstoned node are kept (a re-added key
//!   gets a new store epoch, so they stay hidden) and reported. The links the source's own
//!   documents own are not reported: the generation's cleanup deletes them.
//!
//! [`KnowledgeEngine::source_catalog`] reads the complete, paged, versioned structure back.
//!
//! A generation's documents may also own typed **links** to crew projects, repos, PRs and runs
//! ([`KnowledgeEngine::replace_source_linked`], DES-PM-001 §3.4; see [`crate::links`]). A source id
//! is the registry entry's immutable id, or a tracker id crew derives from one.

use std::collections::BTreeMap;

use crate::engine::{KClass, KNode, KnowledgeEngine};
use crate::links::{DocLink, LinkedReplaceReport, canonical_links, source_owner_prefix};
use crate::plane::{SOURCE_KIND, Visible, shown, valid_source_id};
use serde_json::{Value, json};
use wicked_estate_core::{
    GraphRead, GraphWrite, Language, Location, Node, NodeKind, Scope, Span, Symbol, SymbolId,
    SymbolQuery,
};

/// The manifest node's kind: one per source, never a recall candidate.
pub const SOURCE_MANIFEST_KIND: &str = "ksource";

/// Limits on one generation (refused beyond, never truncated: a cut catalog is a wrong catalog).
pub const MAX_DOCS: usize = 5_000;
/// The whole generation's document records together (one describe, one transaction, in memory).
pub const MAX_GENERATION_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_SECTIONS_PER_DOC: usize = 256;
pub const MAX_KEY_BYTES: usize = 512;
pub const MAX_TITLE_BYTES: usize = 512;
pub const MAX_LABEL_BYTES: usize = 200;
pub const MAX_SECTION_TEXT_BYTES: usize = 16 * 1024;
pub const MAX_ENTRY_BYTES: usize = 16 * 1024;
pub const MAX_CATALOG_BYTES: usize = 4 * 1024;
/// A catalog page holds at most this many bytes of document records (JSON), and one document's
/// record may not exceed it, so a page (documents, the source catalog of at most 4 KiB, and the
/// envelope) always stays inside an agent's tool-output budget (R4, < 25K chars).
pub const PAGE_BYTES: usize = 18_000;
/// The tombstoned keys and orphaned edges a report lists are cut to this many bytes each (R4); the
/// counts are always exact.
const REPORT_LIST_BYTES: usize = 8_000;

/// Metadata keys on source nodes (beside the knowledge keys).
pub(crate) mod meta {
    pub const SOURCE_ID: &str = "source_id";
    pub const SOURCE_KEY: &str = "source_key";
    pub const SECTION_KEY: &str = "section_key";
    pub const SECTION_KEYS: &str = "section_keys";
    pub const GENERATION: &str = "generation";
    pub const ENTRY: &str = "entry";
    pub const TITLE: &str = "title";
    pub const LABEL: &str = "label";
    pub const CATALOG: &str = "catalog";
    pub const DOC_KEYS: &str = "doc_keys";
    pub const PAGES: &str = "pages";
    pub const UPDATED_AT: &str = "updated_at";
    pub const FINGERPRINT: &str = "fingerprint";
}

/// One section of a source document (a relation's columns, keys, enum literals; a tool's inputs).
#[derive(Debug, Clone, PartialEq)]
pub struct SourceSection {
    pub key: String,
    pub text: String,
}

/// One source document: a relation or a tool.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceDoc {
    /// The relation or tool key, unique in the generation (e.g. `support.tickets`).
    pub key: String,
    pub title: Option<String>,
    /// The typed catalog record for this relation or tool (a JSON object; estate stores it as-is).
    pub entry: Option<Value>,
    pub sections: Vec<SourceSection>,
}

/// One complete describe of a source.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceGeneration {
    /// The registry entry's immutable id (never its name).
    pub source_id: String,
    /// Monotonic per source; an older generation is refused, the same one is an idempotent retry.
    pub generation: u64,
    /// The display name at describe time (not an identity).
    pub label: Option<String>,
    /// Source-level typed metadata (a JSON object).
    pub catalog: Option<Value>,
    pub docs: Vec<SourceDoc>,
}

/// An edge that touched a tombstoned node (kept, reported).
#[derive(Debug, Clone, PartialEq)]
pub struct OrphanedEdge {
    pub rel: String,
    pub from: String,
    pub to: String,
    pub owner: String,
}

/// What a `replace_source` did.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplaceReport {
    pub source_id: String,
    pub generation: u64,
    pub docs: usize,
    pub sections: usize,
    /// Keys of tombstoned documents (a byte-capped list), the exact count of tombstoned documents,
    /// and the exact count of tombstoned nodes (documents and sections).
    pub tombstoned_docs: Vec<String>,
    pub tombstoned_doc_count: usize,
    pub tombstoned_nodes: usize,
    pub orphaned_edges: Vec<OrphanedEdge>,
    pub orphaned_edge_count: usize,
}

/// One page of a source catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogPage {
    pub source_id: String,
    pub label: Option<String>,
    pub generation: u64,
    pub catalog: Value,
    pub page: usize,
    pub pages: usize,
    pub total_docs: usize,
    /// `{key, title, entry, sections: [{key, text}]}` per document, in key order.
    pub docs: Vec<Value>,
}

/// Why a catalog read failed.
#[derive(Debug, Clone, PartialEq)]
pub enum CatalogError {
    /// The reader pinned a generation and the source has moved on: restart the read.
    GenerationChanged {
        pinned: u64,
        current: u64,
    },
    /// A new generation landed while this page was being read: restart the read.
    ChangedDuringRead {
        generation: u64,
    },
    PageOutOfRange {
        page: usize,
        pages: usize,
    },
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CatalogError::GenerationChanged { pinned, current } => write!(
                f,
                "the source moved from generation {pinned} to {current}: restart the read at page 0"
            ),
            CatalogError::ChangedDuringRead { generation } => write!(
                f,
                "the source changed while generation {generation} was being read: restart the read at page 0"
            ),
            CatalogError::PageOutOfRange { page, pages } => {
                write!(f, "page {page} is out of range (the catalog has {pages})")
            }
        }
    }
}

impl std::error::Error for CatalogError {}

fn bad(msg: String) -> wicked_estate_core::Error {
    wicked_estate_core::Error::Invalid(msg)
}

fn key_ok(key: &str, max: usize) -> bool {
    !key.trim().is_empty() && key.len() <= max && !key.chars().any(char::is_control)
}

impl SourceGeneration {
    /// Validate the whole generation before anything is written (pure).
    pub fn validate(&self) -> wicked_estate_core::Result<()> {
        if !valid_source_id(&self.source_id) {
            return Err(bad(format!(
                "invalid sourceId {:?}: expected the registry id, 1-128 of [A-Za-z0-9._-]",
                shown(&self.source_id)
            )));
        }
        if self.generation == 0 {
            return Err(bad("generation must be >= 1".into()));
        }
        if let Some(label) = &self.label {
            if !key_ok(label, MAX_LABEL_BYTES) {
                return Err(bad(format!(
                    "label must be 1-{MAX_LABEL_BYTES} bytes with no control characters"
                )));
            }
        }
        if let Some(catalog) = &self.catalog {
            if !catalog.is_object() {
                return Err(bad("catalog must be a JSON object".into()));
            }
            if catalog.to_string().len() > MAX_CATALOG_BYTES {
                return Err(bad(format!("catalog is over {MAX_CATALOG_BYTES} bytes")));
            }
        }
        if self.docs.len() > MAX_DOCS {
            return Err(bad(format!("{} docs is over {MAX_DOCS}", self.docs.len())));
        }
        let mut keys = std::collections::BTreeSet::new();
        let mut total = 0usize;
        for doc in &self.docs {
            if !key_ok(&doc.key, MAX_KEY_BYTES) {
                return Err(bad(format!(
                    "doc key {:?} must be 1-{MAX_KEY_BYTES} bytes with no control characters",
                    shown(&doc.key)
                )));
            }
            if !keys.insert(doc.key.as_str()) {
                return Err(bad(format!("duplicate doc key {:?}", doc.key)));
            }
            if let Some(title) = &doc.title {
                if !key_ok(title, MAX_TITLE_BYTES) {
                    return Err(bad(format!("doc {:?}: title too long or empty", doc.key)));
                }
            }
            if let Some(entry) = &doc.entry {
                if !entry.is_object() {
                    return Err(bad(format!(
                        "doc {:?}: entry must be a JSON object",
                        shown(&doc.key)
                    )));
                }
                if entry.to_string().len() > MAX_ENTRY_BYTES {
                    return Err(bad(format!(
                        "doc {:?}: entry is over {MAX_ENTRY_BYTES} bytes",
                        shown(&doc.key)
                    )));
                }
            }
            if doc.sections.is_empty() {
                return Err(bad(format!(
                    "doc {:?}: at least one section is required",
                    shown(&doc.key)
                )));
            }
            if doc.sections.len() > MAX_SECTIONS_PER_DOC {
                return Err(bad(format!(
                    "doc {:?}: {} sections is over {MAX_SECTIONS_PER_DOC}",
                    shown(&doc.key),
                    doc.sections.len()
                )));
            }
            let record = doc_record(doc).to_string().len();
            total += record;
            if total > MAX_GENERATION_BYTES {
                return Err(bad(format!(
                    "the generation's documents are over {MAX_GENERATION_BYTES} bytes"
                )));
            }
            if record > PAGE_BYTES {
                return Err(bad(format!(
                    "doc {:?}: its catalog record is {record} bytes, over the {PAGE_BYTES}-byte page \
                     (shorten its sections or split the relation's structure across documents)",
                    shown(&doc.key)
                )));
            }
            let mut skeys = std::collections::BTreeSet::new();
            for sec in &doc.sections {
                if !key_ok(&sec.key, MAX_KEY_BYTES) {
                    return Err(bad(format!(
                        "doc {:?}: section key {:?} must be 1-{MAX_KEY_BYTES} bytes",
                        shown(&doc.key),
                        shown(&sec.key)
                    )));
                }
                if !skeys.insert(sec.key.as_str()) {
                    return Err(bad(format!(
                        "doc {:?}: duplicate section key {:?}",
                        shown(&doc.key),
                        shown(&sec.key)
                    )));
                }
                if sec.text.trim().is_empty() || sec.text.len() > MAX_SECTION_TEXT_BYTES {
                    return Err(bad(format!(
                        "doc {:?} section {:?}: text must be 1-{MAX_SECTION_TEXT_BYTES} bytes",
                        shown(&doc.key),
                        shown(&sec.key)
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Percent-encode everything outside `[A-Za-z0-9._-]`, so a key with spaces, quotes or `:` makes a
/// well-formed, reversible id segment (a knowledge id may hold no space and end in no `:`).
pub fn encode_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for b in key.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The stable id (inside the synthetic symbol) of a source document.
pub fn doc_id(source_id: &str, key: &str) -> String {
    format!("{SOURCE_KIND}:{source_id}/{}", encode_key(key))
}

/// The stable id of a section of a source document.
pub fn section_id(source_id: &str, key: &str, section_key: &str) -> String {
    format!(
        "{SOURCE_KIND}:{source_id}/{}#{}",
        encode_key(key),
        encode_key(section_key)
    )
}

/// The node symbol of a source document.
pub fn doc_symbol(source_id: &str, key: &str) -> SymbolId {
    Symbol::synthetic(KClass::Doc.as_kind(), doc_id(source_id, key)).id()
}

/// The node symbol of a section.
pub fn section_symbol(source_id: &str, key: &str, section_key: &str) -> SymbolId {
    Symbol::synthetic(
        KClass::Section.as_kind(),
        section_id(source_id, key, section_key),
    )
    .id()
}

fn manifest_symbol(source_id: &str) -> SymbolId {
    Symbol::synthetic(SOURCE_MANIFEST_KIND, source_id).id()
}

fn source_scope(source_id: &str) -> String {
    format!("{SOURCE_KIND}:{source_id}")
}

/// A section's indexed text: the document key leads, so a relation name always matches a query.
fn section_content(doc_key: &str, text: &str) -> String {
    if text.starts_with(doc_key) {
        text.to_string()
    } else {
        format!("{doc_key}: {text}")
    }
}

/// The catalog record of one document (what a page returns).
fn doc_record(doc: &SourceDoc) -> Value {
    json!({
        "key": doc.key,
        "title": doc.title,
        "entry": doc.entry.clone().unwrap_or_else(|| json!({})),
        "sections": doc.sections.iter().map(|s| json!({
            "key": s.key,
            "text": section_content(&doc.key, &s.text),
        })).collect::<Vec<_>>(),
    })
}

/// A stable fingerprint of a generation's content (FNV-1a 64 over its canonical JSON, documents in
/// key order, and its links when it has any). Two submissions of one generation number must carry
/// the same content. A generation without links hashes exactly as it did before links existed.
fn fingerprint(g: &SourceGeneration, docs: &[&SourceDoc], links: Option<&Value>) -> String {
    let mut canonical = json!({
        "label": g.label,
        "catalog": g.catalog,
        "docs": docs.iter().map(|d| json!({
            "key": d.key, "title": d.title, "entry": d.entry,
            "sections": d.sections.iter().map(|s| json!([s.key, s.text])).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    });
    if let Some(links) = links {
        canonical["links"] = links.clone();
    }
    let canonical = canonical.to_string();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in canonical.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The generation order rule: refuse a generation older than the stored one, or the stored one with
/// different content (a pinned reader of generation N would otherwise read two structures as one).
/// Returns the committed generation (0 when the source has none).
fn check_order(
    stored: Option<&Node>,
    g: &SourceGeneration,
    manifest: &Node,
) -> wicked_estate_core::Result<u64> {
    let Some(stored) = stored else {
        return Ok(0);
    };
    let sid = g.source_id.as_str();
    let current = stored
        .metadata
        .get(meta::GENERATION)
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if g.generation < current {
        return Err(bad(format!(
            "generation {} is older than the stored generation {current} for source {sid}",
            g.generation
        )));
    }
    let fp = |n: &Node| {
        n.metadata
            .get(meta::FINGERPRINT)
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    if g.generation == current && fp(stored) != fp(manifest) {
        return Err(bad(format!(
            "generation {current} of source {sid} is already stored with different content: \
             a changed structure needs a new generation"
        )));
    }
    Ok(current)
}

/// Byte-bounded page boundaries over documents in key order: `[start, end)` pairs.
fn page_bounds(sizes: &[usize]) -> Vec<(usize, usize)> {
    let mut pages = Vec::new();
    let (mut start, mut used) = (0usize, 0usize);
    for (i, size) in sizes.iter().enumerate() {
        if i > start && used + size > PAGE_BYTES {
            pages.push((start, i));
            start = i;
            used = 0;
        }
        used += size;
    }
    if start < sizes.len() {
        pages.push((start, sizes.len()));
    }
    pages
}

impl KnowledgeEngine {
    fn manifest(&self, source_id: &str) -> wicked_estate_core::Result<Option<Node>> {
        self.store.get_node(&manifest_symbol(source_id))
    }

    /// Replace a source's structure with one complete generation (see the module docs). The
    /// generation owns no links, so any links the source's documents held are dropped with it.
    pub fn replace_source(
        &mut self,
        g: &SourceGeneration,
        now: i64,
    ) -> wicked_estate_core::Result<ReplaceReport> {
        Ok(self.replace_source_linked(g, &BTreeMap::new(), now)?.report)
    }

    /// [`Self::replace_source`] whose documents own typed links (`links`, by document key):
    /// DES-PM-001 §3.4, in the write order [`crate::links`] describes. A link is visible only once
    /// its generation is the source's committed one, and a crash at any step leaves only hidden
    /// rows, which the source's next generation deletes.
    pub fn replace_source_linked(
        &mut self,
        g: &SourceGeneration,
        links: &BTreeMap<String, Vec<DocLink>>,
        now: i64,
    ) -> wicked_estate_core::Result<LinkedReplaceReport> {
        // 1. Validate the whole generation and its links (pure).
        g.validate()?;
        crate::links::validate_links(g, links).map_err(bad)?;
        let link_count: usize = links.values().map(Vec::len).sum();
        if link_count > 0 && self.xedge.is_none() {
            return Err(bad(
                "xedge store not available: a generation with links cannot be written".into(),
            ));
        }
        let sid = g.source_id.as_str();
        let scope = source_scope(sid);
        let deps = vec![scope.clone()];

        // Sort docs by key: page boundaries and the manifest's key list are then deterministic.
        let mut docs: Vec<&SourceDoc> = g.docs.iter().collect();
        docs.sort_by(|a, b| a.key.cmp(&b.key));

        // Build every node first (pure).
        let mut to_write: Vec<(Node, String)> = Vec::new(); // (node, text to embed)
        let mut new_ids = std::collections::BTreeSet::new();
        let mut derived = Vec::new();
        let mut sizes = Vec::with_capacity(docs.len());
        let mut section_count = 0usize;
        for doc in &docs {
            let mut kn = KNode::new(
                KClass::Doc,
                doc.title.clone().unwrap_or_else(|| doc.key.clone()),
                scope.clone(),
                format!("{scope}/{}", doc.key),
                now,
            );
            kn.id = doc_id(sid, &doc.key);
            let doc_sym = kn.symbol();
            let mut node = kn.to_node();
            crate::plane::stamp(&mut node, &deps, true);
            let m = &mut node.metadata;
            m.insert(meta::SOURCE_ID.into(), sid.into());
            m.insert(meta::SOURCE_KEY.into(), doc.key.clone().into());
            m.insert(meta::GENERATION.into(), g.generation.into());
            m.insert(meta::TITLE.into(), json!(doc.title));
            m.insert(
                meta::ENTRY.into(),
                doc.entry.clone().unwrap_or_else(|| json!({})),
            );
            m.insert(
                meta::SECTION_KEYS.into(),
                json!(doc.sections.iter().map(|s| &s.key).collect::<Vec<_>>()),
            );
            new_ids.insert(doc_sym.clone());
            to_write.push((node, kn.content.clone()));
            for sec in &doc.sections {
                let mut skn = KNode::new(
                    KClass::Section,
                    section_content(&doc.key, &sec.text),
                    scope.clone(),
                    format!("{scope}/{}#{}", doc.key, sec.key),
                    now,
                );
                skn.id = section_id(sid, &doc.key, &sec.key);
                let sym = skn.symbol();
                let mut snode = skn.to_node();
                crate::plane::stamp(&mut snode, &deps, true);
                let m = &mut snode.metadata;
                m.insert(meta::SOURCE_ID.into(), sid.into());
                m.insert(meta::SOURCE_KEY.into(), doc.key.clone().into());
                m.insert(meta::SECTION_KEY.into(), sec.key.clone().into());
                m.insert(meta::GENERATION.into(), g.generation.into());
                derived.push(crate::engine::derived_from_edge(&sym, &doc_sym));
                new_ids.insert(sym);
                to_write.push((snode, skn.content.clone()));
                section_count += 1;
            }
            sizes.push(doc_record(doc).to_string().len());
        }
        let pages: Vec<Value> = page_bounds(&sizes)
            .into_iter()
            .map(|(a, b)| json!([a, b]))
            .collect();
        let mut manifest = Node::new(
            manifest_symbol(sid),
            NodeKind::Other(SOURCE_MANIFEST_KIND.into()),
            g.label.clone().unwrap_or_else(|| sid.to_string()),
            Language::new("knowledge"),
            Location::new("knowledge", Span::ZERO),
        )
        .with_scope(Scope::parse(&scope));
        {
            let m = &mut manifest.metadata;
            m.insert(meta::SOURCE_ID.into(), sid.into());
            m.insert(meta::GENERATION.into(), g.generation.into());
            m.insert(meta::LABEL.into(), json!(g.label));
            m.insert(
                meta::CATALOG.into(),
                g.catalog.clone().unwrap_or_else(|| json!({})),
            );
            m.insert(
                meta::DOC_KEYS.into(),
                json!(docs.iter().map(|d| &d.key).collect::<Vec<_>>()),
            );
            m.insert(meta::PAGES.into(), Value::Array(pages));
            m.insert(meta::UPDATED_AT.into(), now.into());
            m.insert(
                meta::FINGERPRINT.into(),
                fingerprint(g, &docs, canonical_links(links).as_ref()).into(),
            );
            m.insert(crate::engine::meta_keys::DEPS.into(), json!(deps));
        }

        // 2. Refuse an older generation, or the same one with other content, before any link row
        // is touched.
        let committed = check_order(self.manifest(sid)?.as_ref(), g, &manifest)?;
        // The committed generation again (same content, links included) is a retry: its rows are
        // already the live ones, so they are neither re-resolved nor rewritten (a re-stamp would
        // rebind a link to a knowledge end deleted and re-added since; review r1).
        let restage = g.generation > committed;
        // 1b. Resolve the links (reads only).
        let edges = if restage {
            self.plan_links(g, links)?
        } else {
            Vec::new()
        };
        let mut stale_removed = 0u64;
        if restage && self.xedge.is_some() {
            // 3. An interrupted attempt's rows above the committed generation can never be live:
            // delete them, so that generation number commits only its own links.
            stale_removed += self.delete_link_owners(sid, |n| n > committed)?;
            // 4. The new rows, hidden until the flip (their owner is still at the old generation).
            if let Some(xedge) = self.xedge.as_ref() {
                xedge.put_typed_batch(&edges)?;
            }
        }
        #[cfg(test)]
        if self.fail_links_before_flip {
            return Err(bad("injected crash before the flip".into()));
        }

        // 5. The flip. Everything below is ONE transaction: all of it lands, or none of it does.
        self.store.begin_batch()?;
        let written =
            self.replace_source_in_batch(g, &scope, &to_write, &derived, &new_ids, &manifest);
        let outcome = match written {
            Ok(report) => self.store.commit_batch().map(|_| report),
            Err(e) => Err(e),
        };
        let mut report = match outcome {
            Ok(report) => report,
            Err(e) => {
                self.store.rollback_batch()?;
                return Err(e);
            }
        };
        report.docs = docs.len();
        report.sections = section_count;
        #[cfg(test)]
        if self.fail_links_after_flip {
            return Err(bad("injected crash after the flip".into()));
        }

        // 6. Every other generation's rows are hidden now; delete them. The generation is
        // committed whatever happens here, so a failure is reported, never returned as one.
        let mut cleanup_error = None;
        if self.xedge.is_some() {
            #[cfg(test)]
            let injected = self.fail_links_cleanup;
            #[cfg(not(test))]
            let injected = false;
            let cleaned = if injected {
                Err(bad("injected cleanup failure".into()))
            } else {
                self.delete_link_owners(sid, |n| n != g.generation)
            };
            match cleaned {
                Ok(n) => stale_removed += n,
                Err(e) => cleanup_error = Some(e.to_string()),
            }
        }
        Ok(LinkedReplaceReport {
            report,
            links: link_count,
            stale_removed,
            cleanup_error,
        })
    }

    fn replace_source_in_batch(
        &mut self,
        g: &SourceGeneration,
        scope: &str,
        to_write: &[(Node, String)],
        derived: &[wicked_estate_core::Edge],
        new_ids: &std::collections::BTreeSet<SymbolId>,
        manifest: &Node,
    ) -> wicked_estate_core::Result<ReplaceReport> {
        let sid = g.source_id.as_str();
        // Generation order, read again inside the transaction.
        check_order(self.manifest(sid)?.as_ref(), g, manifest)?;

        // What this source holds now (by scope, not by the manifest, so nothing can be orphaned).
        let existing = self.store.find_symbols(&SymbolQuery {
            text: None,
            exact_name: None,
            kinds: vec![
                NodeKind::Other(KClass::Doc.as_kind().into()),
                NodeKind::Other(KClass::Section.as_kind().into()),
            ],
            language: None,
            limit: None,
            scope_prefix: Some(scope.to_string()),
        })?;
        let absent: Vec<&Node> = existing
            .iter()
            .filter(|n| !new_ids.contains(&n.symbol))
            .collect();

        // Report the typed edges that touch what is about to be tombstoned (they are kept).
        // Each edge once, however many of its ends are tombstoned.
        let mut orphaned = Vec::new();
        let mut orphaned_seen = std::collections::BTreeSet::new();
        let mut orphaned_bytes = 0usize;
        if let Some(xedge) = self.xedge.as_ref() {
            let reader = xedge.reader();
            let link_prefix = source_owner_prefix(sid);
            for n in &absent {
                let id = n.symbol.0.as_str();
                let mut edges = reader.typed_out("knowledge", id, &[], usize::MAX)?;
                edges.extend(reader.typed_in("knowledge", id, &[], usize::MAX)?);
                for e in edges {
                    // Links this source's documents own are this writer's own: the cleanup after
                    // the flip deletes them, so they are not reported as kept.
                    if e.owner.starts_with(&link_prefix) {
                        continue;
                    }
                    let key = (
                        e.rel.clone(),
                        e.source.engine.clone(),
                        e.source.stable_id.clone(),
                        e.target.engine.clone(),
                        e.target.stable_id.clone(),
                        e.owner.clone(),
                    );
                    let cost = 64
                        + e.rel.len()
                        + e.source.stable_id.len()
                        + e.target.stable_id.len()
                        + e.owner.len();
                    if orphaned_seen.insert(key) && orphaned_bytes + cost <= REPORT_LIST_BYTES {
                        orphaned_bytes += cost;
                        orphaned.push(OrphanedEdge {
                            rel: e.rel,
                            from: e.source.stable_id,
                            to: e.target.stable_id,
                            owner: e.owner,
                        });
                    }
                }
            }
        }
        let orphaned_count = orphaned_seen.len();
        let all_tombstoned: Vec<String> = absent
            .iter()
            .filter(|n| matches!(&n.kind, NodeKind::Other(k) if k == KClass::Doc.as_kind()))
            .filter_map(|n| {
                n.metadata
                    .get(meta::SOURCE_KEY)
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect();
        let tombstoned_doc_count = all_tombstoned.len();
        let mut tombstoned_bytes = 0usize;
        let tombstoned_docs: Vec<String> = all_tombstoned
            .into_iter()
            .take_while(|k| {
                tombstoned_bytes += k.len() * 2 + 4; // JSON-escaped worst case
                tombstoned_bytes <= REPORT_LIST_BYTES
            })
            .collect();

        for (node, text) in to_write {
            #[cfg(test)]
            if let Some(left) = self.fail_replace_after.as_mut() {
                if *left == 0 {
                    return Err(bad("injected failure mid-generation".into()));
                }
                *left -= 1;
            }
            self.store.upsert_nodes(std::slice::from_ref(node))?;
            let vec = self.embed(text);
            self.store.set_embedding(&node.symbol, &vec)?;
        }
        self.store.upsert_edges(derived)?;
        let absent_ids: Vec<SymbolId> = absent.iter().map(|n| n.symbol.clone()).collect();
        if !absent_ids.is_empty() {
            self.store.remove_nodes(&absent_ids)?;
        }
        #[cfg(test)]
        if self.fail_replace_before_manifest {
            return Err(bad("injected failure after the tombstones".into()));
        }
        self.store.upsert_nodes(std::slice::from_ref(manifest))?;

        Ok(ReplaceReport {
            source_id: sid.to_string(),
            generation: g.generation,
            docs: 0,
            sections: 0,
            tombstoned_docs,
            tombstoned_doc_count,
            tombstoned_nodes: absent_ids.len(),
            orphaned_edges: orphaned,
            orphaned_edge_count: orphaned_count,
        })
    }

    /// One page of a source's catalog, or `None` when the source is unknown OR not in `visible`
    /// (the two answer alike, so a hidden source's existence does not leak).
    pub fn source_catalog(
        &self,
        source_id: &str,
        page: usize,
        pinned_generation: Option<u64>,
        visible: &Visible,
    ) -> anyhow::Result<Option<CatalogPage>> {
        if !valid_source_id(source_id) || !visible.contains(&source_scope(source_id)) {
            return Ok(None);
        }
        let Some(manifest) = self.manifest(source_id)? else {
            return Ok(None);
        };
        let m = &manifest.metadata;
        let generation = m.get(meta::GENERATION).and_then(Value::as_u64).unwrap_or(0);
        if let Some(pinned) = pinned_generation {
            if pinned != generation {
                return Err(CatalogError::GenerationChanged {
                    pinned,
                    current: generation,
                }
                .into());
            }
        }
        let keys: Vec<String> = m
            .get(meta::DOC_KEYS)
            .and_then(Value::as_array)
            .map(|xs| {
                xs.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let bounds: Vec<(usize, usize)> = m
            .get(meta::PAGES)
            .and_then(Value::as_array)
            .map(|xs| {
                xs.iter()
                    .filter_map(|p| {
                        let a = p.get(0)?.as_u64()? as usize;
                        let b = p.get(1)?.as_u64()? as usize;
                        (a <= b && b <= keys.len()).then_some((a, b))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let pages = bounds.len();
        let range = match bounds.get(page) {
            Some(r) => *r,
            None if pages == 0 && page == 0 => (0, 0),
            None => return Err(CatalogError::PageOutOfRange { page, pages }.into()),
        };
        // Every record of the page must exist and carry the manifest's generation (each generation
        // rewrites all of them), so a page read while a new generation commits is detected, never
        // returned mixed or short.
        let changed = || -> anyhow::Error { CatalogError::ChangedDuringRead { generation }.into() };
        let gen_of = |n: &Node| n.metadata.get(meta::GENERATION).and_then(Value::as_u64);
        let mut docs = Vec::new();
        for key in &keys[range.0..range.1] {
            let Some(doc) = self.store.get_node(&doc_symbol(source_id, key))? else {
                return Err(changed());
            };
            if gen_of(&doc) != Some(generation) {
                return Err(changed());
            }
            let dm = &doc.metadata;
            let section_keys: Vec<String> = dm
                .get(meta::SECTION_KEYS)
                .and_then(Value::as_array)
                .map(|xs| {
                    xs.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let mut sections = Vec::new();
            for sk in &section_keys {
                let Some(node) = self.store.get_node(&section_symbol(source_id, key, sk))? else {
                    return Err(changed());
                };
                if gen_of(&node) != Some(generation) {
                    return Err(changed());
                }
                let Some(sec) = KNode::from_node(&node) else {
                    return Err(changed());
                };
                sections.push(json!({"key": sk, "text": sec.content}));
            }
            docs.push(json!({
                "key": key,
                "title": dm.get(meta::TITLE).cloned().unwrap_or(Value::Null),
                "entry": dm.get(meta::ENTRY).cloned().unwrap_or_else(|| json!({})),
                "sections": sections,
            }));
        }
        let now_gen = self
            .manifest(source_id)?
            .and_then(|m| m.metadata.get(meta::GENERATION).and_then(Value::as_u64));
        if now_gen != Some(generation) {
            return Err(changed());
        }
        Ok(Some(CatalogPage {
            source_id: source_id.to_string(),
            label: m
                .get(meta::LABEL)
                .and_then(Value::as_str)
                .map(str::to_string),
            generation,
            catalog: m.get(meta::CATALOG).cloned().unwrap_or_else(|| json!({})),
            page,
            pages,
            total_docs: keys.len(),
            docs,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use wicked_estate_overlay::{Endpoint, TypedXEdge, XedgeStore};

    fn sec(key: &str, text: &str) -> SourceSection {
        SourceSection {
            key: key.into(),
            text: text.into(),
        }
    }

    fn doc(key: &str, columns: &str) -> SourceDoc {
        SourceDoc {
            key: key.into(),
            title: Some(format!("{key} (table)")),
            entry: Some(json!({"kind": "table", "columns": columns})),
            sections: vec![sec("columns", columns), sec("keys", "primary key id")],
        }
    }

    fn generation(n: u64, docs: Vec<SourceDoc>) -> SourceGeneration {
        SourceGeneration {
            source_id: "s1".into(),
            generation: n,
            label: Some("support-pg".into()),
            catalog: Some(json!({"dialect": "postgres"})),
            docs,
        }
    }

    fn engine() -> KnowledgeEngine {
        KnowledgeEngine::in_memory()
            .unwrap()
            .with_xedge_store(Arc::new(XedgeStore::in_memory().unwrap()))
    }

    fn visible() -> Visible {
        Visible::from_deps(["source:s1"]).unwrap()
    }

    fn sources_in(e: &mut KnowledgeEngine, q: &str) -> Vec<String> {
        e.recall_visible(q, 8000, None, &visible(), 1)
            .unwrap()
            .into_iter()
            .map(|h| h.source)
            .collect()
    }

    /// r2 (codex 6): a link-free generation's fingerprint is exactly B3a's (e6b1f9a), so a
    /// same-number retry against a manifest an older estate wrote is still accepted after the
    /// upgrade. NEGATIVE CONTROL: links folded into every fingerprint (even an empty set).
    #[test]
    fn a_link_free_fingerprint_is_the_b3a_fingerprint() {
        // B3a's formula, verbatim (`fingerprint` in this file at e6b1f9a).
        fn b3a(g: &SourceGeneration) -> String {
            let mut docs: Vec<&SourceDoc> = g.docs.iter().collect();
            docs.sort_by(|a, b| a.key.cmp(&b.key));
            let canonical = json!({
                "label": g.label,
                "catalog": g.catalog,
                "docs": docs.iter().map(|d| json!({
                    "key": d.key, "title": d.title, "entry": d.entry,
                    "sections": d.sections.iter().map(|s| json!([s.key, s.text])).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })
            .to_string();
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for b in canonical.bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            format!("{h:016x}")
        }
        let mut e = engine();
        let g = generation(1, vec![doc("support.tickets", "id int8, status text")]);
        e.replace_source(&g, 10).unwrap();
        let stored = e.manifest("s1").unwrap().unwrap();
        assert_eq!(
            stored.metadata[meta::FINGERPRINT].as_str(),
            Some(b3a(&g).as_str())
        );
        // A linked call with no links is the same retry.
        let none = BTreeMap::from([("support.tickets".to_string(), Vec::new())]);
        e.replace_source_linked(&g, &none, 11).unwrap();
    }

    #[test]
    fn a_generation_is_recallable_cataloged_and_gated() {
        let mut e = engine();
        let r = e
            .replace_source(
                &generation(
                    1,
                    vec![
                        doc(
                            "support.tickets",
                            "id int8, status text enum(open,closed), region text",
                        ),
                        doc("crm.customers", "id int8, name text"),
                    ],
                ),
                10,
            )
            .unwrap();
        assert_eq!((r.docs, r.sections, r.tombstoned_nodes), (2, 4, 0));
        assert!(
            sources_in(&mut e, "tickets status region")
                .iter()
                .any(|s| s.starts_with("source:s1/support.tickets")),
            "a section is recallable under visible"
        );
        assert!(
            e.recall("tickets status region", 8000, None, 1)
                .unwrap()
                .is_empty(),
            "and hidden from a caller that declares nothing"
        );
        let page = e
            .source_catalog("s1", 0, None, &visible())
            .unwrap()
            .unwrap();
        assert_eq!((page.generation, page.total_docs, page.pages), (1, 2, 1));
        assert_eq!(page.label.as_deref(), Some("support-pg"));
        assert_eq!(page.catalog["dialect"], "postgres");
        assert_eq!(page.docs[0]["key"], "crm.customers", "docs in key order");
        assert_eq!(page.docs[1]["entry"]["kind"], "table");
        assert_eq!(page.docs[1]["sections"][0]["key"], "columns");
        // Unknown and hidden answer alike.
        assert!(
            e.source_catalog("s1", 0, None, &Visible::none())
                .unwrap()
                .is_none()
        );
        assert!(
            e.source_catalog("s2", 0, None, &visible())
                .unwrap()
                .is_none()
        );
    }

    /// NEGATIVE CONTROL: a second generation without a table tombstones it (document and
    /// sections), keeps the stable ids of what stayed, and reports the edge left on the dropped one.
    #[test]
    fn a_second_generation_without_a_table_tombstones_it() {
        let mut e = engine();
        e.replace_source(
            &generation(
                1,
                vec![
                    doc("support.tickets", "id int8, status text"),
                    doc("support.refunds", "id int8, amount numeric"),
                ],
            ),
            10,
        )
        .unwrap();
        let kept = section_symbol("s1", "support.tickets", "columns");
        let dropped = doc_symbol("s1", "support.refunds");
        assert!(e.store.get_node(&dropped).unwrap().is_some());
        // A shape's edge onto the table that is about to go.
        let xedge = Arc::clone(e.xedge.as_ref().unwrap());
        xedge
            .put_typed(&TypedXEdge {
                source: Endpoint::new("knowledge", "shape-1", 0),
                target: Endpoint::new("knowledge", dropped.0.clone(), 0),
                rel: "derived_from".into(),
                owner: "shape-1".into(),
                confidence: 1.0,
                meta: "{}".into(),
            })
            .unwrap();

        let r = e
            .replace_source(
                &generation(
                    2,
                    vec![doc("support.tickets", "id int8, status text, region text")],
                ),
                20,
            )
            .unwrap();
        assert_eq!(r.tombstoned_docs, vec!["support.refunds".to_string()]);
        assert_eq!(r.tombstoned_nodes, 3, "the document and its two sections");
        assert_eq!(r.orphaned_edge_count, 1);
        assert_eq!(r.orphaned_edges[0].owner, "shape-1");
        assert!(e.store.get_node(&dropped).unwrap().is_none());
        assert!(
            e.store
                .get_node(&section_symbol("s1", "support.refunds", "columns"))
                .unwrap()
                .is_none()
        );
        let still = e
            .store
            .get_node(&kept)
            .unwrap()
            .expect("the stable id stays");
        assert!(KNode::from_node(&still).unwrap().content.contains("region"));
        assert!(
            !sources_in(&mut e, "refunds amount")
                .iter()
                .any(|s| s.contains("support.refunds")),
            "a tombstoned table is never recalled"
        );
        assert_eq!(
            xedge.typed_len().unwrap(),
            1,
            "the edge is kept, and was reported"
        );
        let page = e
            .source_catalog("s1", 0, None, &visible())
            .unwrap()
            .unwrap();
        assert_eq!((page.generation, page.total_docs), (2, 1));
    }

    /// NEGATIVE CONTROL: a failed generation writes nothing and tombstones nothing — whether it
    /// fails validation, or fails part-way through the write (an injected store error).
    #[test]
    fn a_failed_generation_writes_nothing_and_tombstones_nothing() {
        let mut e = engine();
        e.replace_source(
            &generation(
                1,
                vec![
                    doc("support.tickets", "id int8"),
                    doc("support.refunds", "id int8"),
                ],
            ),
            10,
        )
        .unwrap();
        let before = e.all_nodes(None, None).unwrap().len();

        // (a) Invalid: a duplicate key. Refused before any write.
        let mut dup = generation(2, vec![doc("support.tickets", "id int8, region text")]);
        dup.docs.push(doc("support.tickets", "x"));
        assert!(e.replace_source(&dup, 20).is_err());

        // (b) Valid, but the store fails after the first node of the generation is written.
        e.fail_replace_after = Some(1);
        let next = generation(
            2,
            vec![
                doc("support.tickets", "id int8, region text"),
                doc("crm.accounts", "id int8"),
            ],
        );
        assert!(e.replace_source(&next, 20).is_err());
        e.fail_replace_after = None;

        assert_eq!(
            e.all_nodes(None, None).unwrap().len(),
            before,
            "nothing written"
        );
        assert!(
            e.store
                .get_node(&doc_symbol("s1", "support.refunds"))
                .unwrap()
                .is_some(),
            "nothing tombstoned: the table absent from the failed generation is still there"
        );
        assert!(
            e.store
                .get_node(&doc_symbol("s1", "crm.accounts"))
                .unwrap()
                .is_none()
        );
        let tickets = e
            .store
            .get_node(&doc_symbol("s1", "support.tickets"))
            .unwrap()
            .unwrap();
        assert_eq!(
            tickets.metadata[meta::GENERATION],
            1,
            "the first write was rolled back"
        );
        let page = e
            .source_catalog("s1", 0, None, &visible())
            .unwrap()
            .unwrap();
        assert_eq!(page.generation, 1);

        // The engine is usable afterwards (no transaction left open).
        e.replace_source(&next, 30).unwrap();
        assert!(
            e.store
                .get_node(&doc_symbol("s1", "support.refunds"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn an_older_generation_is_refused_and_the_same_one_is_a_retry() {
        let mut e = engine();
        e.replace_source(&generation(3, vec![doc("t", "id int8")]), 1)
            .unwrap();
        assert!(e.replace_source(&generation(2, vec![]), 2).is_err());
        assert!(e.store.get_node(&doc_symbol("s1", "t")).unwrap().is_some());
        let again = e
            .replace_source(&generation(3, vec![doc("t", "id int8")]), 3)
            .unwrap();
        assert_eq!(again.tombstoned_nodes, 0);
        // An empty generation is a real one: the source has no relations left.
        let empty = e.replace_source(&generation(4, vec![]), 4).unwrap();
        assert_eq!(empty.tombstoned_nodes, 3);
        let page = e
            .source_catalog("s1", 0, None, &visible())
            .unwrap()
            .unwrap();
        assert_eq!((page.pages, page.total_docs), (0, 0));
    }

    #[test]
    fn pages_are_byte_bounded_and_a_pinned_reader_hears_a_new_generation() {
        let mut e = engine();
        let wide = "c text, ".repeat(600); // ~4.8 KB per section
        let docs: Vec<SourceDoc> = (0..10).map(|i| doc(&format!("t{i:02}"), &wide)).collect();
        e.replace_source(&generation(1, docs), 1).unwrap();
        let first = e
            .source_catalog("s1", 0, Some(1), &visible())
            .unwrap()
            .unwrap();
        assert!(first.pages > 1, "10 wide tables do not fit one page");
        let mut seen = 0;
        for p in 0..first.pages {
            let page = e
                .source_catalog("s1", p, Some(1), &visible())
                .unwrap()
                .unwrap();
            assert!(!page.docs.is_empty());
            seen += page.docs.len();
        }
        assert_eq!(seen, 10, "every doc on exactly one page");
        assert!(
            e.source_catalog("s1", first.pages, None, &visible())
                .is_err()
        );
        e.replace_source(&generation(2, vec![doc("t00", "id int8")]), 2)
            .unwrap();
        let err = e
            .source_catalog("s1", 1, Some(1), &visible())
            .unwrap_err()
            .to_string();
        assert!(err.contains("generation 1 to 2"), "{err}");
    }

    #[test]
    fn keys_with_spaces_and_colons_make_well_formed_ids() {
        let mut e = engine();
        let d = SourceDoc {
            key: "Public.\"Order Items\":v2".into(),
            title: None,
            entry: None,
            sections: vec![sec("cols: all", "qty int4")],
        };
        e.replace_source(&generation(1, vec![d]), 1).unwrap();
        let sym = section_symbol("s1", "Public.\"Order Items\":v2", "cols: all");
        let node = e.store.get_node(&sym).unwrap().expect("written");
        let kn = KNode::from_node(&node).unwrap();
        assert_eq!(
            kn.id,
            section_id("s1", "Public.\"Order Items\":v2", "cols: all")
        );
        assert!(!kn.id.contains(' '));
        assert!(kn.content.starts_with("Public.\"Order Items\":v2: "));
    }

    #[test]
    fn validation_refuses_what_would_not_round_trip() {
        let mut g = generation(1, vec![doc("t", "id")]);
        g.source_id = "a/b".into();
        assert!(g.validate().is_err(), "a source id is one scope segment");
        let mut g = generation(0, vec![]);
        assert!(g.validate().is_err(), "generation >= 1");
        g.generation = 1;
        g.docs = vec![SourceDoc {
            key: "t".into(),
            title: None,
            entry: None,
            sections: vec![],
        }];
        assert!(g.validate().is_err(), "a doc needs a section");
        g.docs = vec![doc("t", "id")];
        g.docs[0].sections.push(sec("columns", "dup"));
        assert!(g.validate().is_err(), "duplicate section key");
        g.docs = vec![doc("t", "id")];
        g.docs[0].entry = Some(json!([1]));
        assert!(g.validate().is_err(), "entry must be an object");
    }
}

#[cfg(test)]
mod r1_tests {
    use super::*;
    use std::sync::Arc;
    use wicked_estate_overlay::{Endpoint, TypedXEdge, XedgeStore};

    fn doc(key: &str, columns: &str) -> SourceDoc {
        SourceDoc {
            key: key.into(),
            title: None,
            entry: Some(json!({"kind": "table"})),
            sections: vec![
                SourceSection {
                    key: "columns".into(),
                    text: columns.into(),
                },
                SourceSection {
                    key: "keys".into(),
                    text: "primary key id".into(),
                },
            ],
        }
    }

    fn generation(n: u64, docs: Vec<SourceDoc>) -> SourceGeneration {
        SourceGeneration {
            source_id: "s1".into(),
            generation: n,
            label: Some("support-pg".into()),
            catalog: Some(json!({"dialect": "postgres"})),
            docs,
        }
    }

    fn visible() -> Visible {
        Visible::from_deps(["source:s1"]).unwrap()
    }

    fn open(path: &std::path::Path) -> KnowledgeEngine {
        KnowledgeEngine::open(path.to_str().unwrap()).unwrap()
    }

    /// C3: the same generation number with different content is refused; the same content is an
    /// idempotent retry.
    #[test]
    fn the_same_generation_must_be_the_same_content() {
        let mut e = KnowledgeEngine::in_memory().unwrap();
        e.replace_source(&generation(3, vec![doc("t", "id int8")]), 1)
            .unwrap();
        let err = e
            .replace_source(&generation(3, vec![doc("t", "id int8, x text")]), 2)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("already stored with different content"),
            "{err}"
        );
        e.replace_source(&generation(3, vec![doc("t", "id int8")]), 3)
            .unwrap();
        let page = e
            .source_catalog("s1", 0, None, &visible())
            .unwrap()
            .unwrap();
        assert!(
            page.docs[0]["sections"][0]["text"]
                .as_str()
                .unwrap()
                .ends_with("id int8")
        );
    }

    /// C4 + pi-5: an edge between two tombstoned nodes is reported once; a re-added key gets a new
    /// epoch, so the old edge stays hidden in neighbors.
    #[test]
    fn orphaned_edges_count_once_and_a_re_added_key_does_not_inherit_them() {
        let mut e = KnowledgeEngine::in_memory()
            .unwrap()
            .with_xedge_store(Arc::new(XedgeStore::in_memory().unwrap()));
        e.replace_source(
            &generation(1, vec![doc("a", "id int8"), doc("b", "id int8")]),
            1,
        )
        .unwrap();
        let owner = e
            .write(&KNode::new(KClass::Concept, "a joins b", "", "shapes", 1))
            .unwrap()
            .0;
        let (a, b) = (doc_symbol("s1", "a").0, doc_symbol("s1", "b").0);
        let xedge = Arc::clone(e.xedge.as_ref().unwrap());
        xedge
            .put_typed(&TypedXEdge {
                source: Endpoint::new("knowledge", a.clone(), 0),
                target: Endpoint::new("knowledge", b.clone(), 0),
                rel: "joins".into(),
                owner: owner.clone(),
                confidence: 1.0,
                meta: json!({"deps": ["source:s1"]}).to_string(),
            })
            .unwrap();
        let graph = wicked_estate_store::SqliteStore::in_memory().unwrap();
        let q = crate::NeighborsQuery {
            store: "knowledge".into(),
            id: a.clone(),
            rels: vec![],
            depth: 1,
            visible: visible(),
        };
        assert_eq!(e.neighbors(&q, &graph).unwrap().unwrap().edges.len(), 1);
        // A routine re-describe that keeps both tables (content changed) keeps the edge: an
        // in-place upsert does not bump the epoch.
        e.replace_source(
            &generation(
                2,
                vec![
                    doc("a", "id int8, note text"),
                    doc("b", "id int8, note text"),
                ],
            ),
            2,
        )
        .unwrap();
        assert_eq!(
            e.neighbors(&q, &graph).unwrap().unwrap().edges.len(),
            1,
            "a kept relation keeps its shape edges across re-describes"
        );

        let r = e.replace_source(&generation(3, vec![]), 3).unwrap();
        assert_eq!(
            r.orphaned_edge_count, 1,
            "one edge, though both its ends went"
        );
        assert_eq!(r.orphaned_edges.len(), 1);

        e.replace_source(
            &generation(4, vec![doc("a", "id int8"), doc("b", "id int8")]),
            4,
        )
        .unwrap();
        let n = e.neighbors(&q, &graph).unwrap().unwrap();
        assert!(
            n.edges.is_empty(),
            "a re-added key never inherits the old edge: {n:?}"
        );
        assert_eq!(xedge.typed_len().unwrap(), 1, "the edge is still kept");
    }

    /// C6: a page whose records do not all carry the manifest's generation (a new generation
    /// committing mid-read) is an error, never a mixed page.
    #[test]
    fn a_page_read_across_a_commit_is_refused() {
        let mut e = KnowledgeEngine::in_memory().unwrap();
        e.replace_source(&generation(1, vec![doc("t", "id int8")]), 1)
            .unwrap();
        // Simulate the interleaving: a section already rewritten by generation 2.
        let sym = section_symbol("s1", "t", "keys");
        let mut node = e.store.get_node(&sym).unwrap().unwrap();
        node.metadata.insert(meta::GENERATION.into(), 2.into());
        e.store.upsert_nodes(&[node]).unwrap();
        let err = e
            .source_catalog("s1", 0, None, &visible())
            .unwrap_err()
            .to_string();
        assert!(err.contains("changed while generation 1"), "{err}");
    }

    /// C12 + pi-6: a doc whose record would not fit a page is refused, and a full page of the
    /// largest accepted docs stays inside R4.
    #[test]
    fn pages_stay_inside_r4() {
        let mut e = KnowledgeEngine::in_memory().unwrap();
        let huge = doc("wide", &"c text, ".repeat(2_000)); // 16 KB of columns
        let mut too_big = huge.clone();
        too_big.sections[1].text = "k ".repeat(2_000);
        assert!(
            generation(1, vec![too_big]).validate().is_err(),
            "over the page"
        );
        let docs: Vec<SourceDoc> = (0..6)
            .map(|i| doc(&format!("w{i}"), &"c text, ".repeat(2_000)))
            .collect();
        let mut g = generation(1, docs);
        g.catalog = Some(json!({"schemas": "x".repeat(3_500)}));
        e.replace_source(&g, 1).unwrap();
        let pages = e
            .source_catalog("s1", 0, None, &visible())
            .unwrap()
            .unwrap()
            .pages;
        for p in 0..pages {
            let page = e
                .source_catalog("s1", p, None, &visible())
                .unwrap()
                .unwrap();
            let wire = json!({
                "sourceId": page.source_id, "label": page.label, "generation": page.generation,
                "page": page.page, "pages": page.pages, "total_docs": page.total_docs,
                "untrusted": true, "catalog": page.catalog, "docs": page.docs,
            })
            .to_string();
            assert!(wire.len() < 25_000, "page {p} is {} chars", wire.len());
        }
    }

    /// C16: a generation that fails AFTER updating an existing doc, or AFTER its tombstones, rolls
    /// all of it back — content, FTS, generation and the tombstoned doc — durably (reopened).
    #[test]
    fn a_failure_after_an_update_or_a_tombstone_rolls_everything_back_durably() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("knowledge.db");
        {
            let mut e = open(&path);
            e.replace_source(
                &generation(
                    1,
                    vec![
                        doc("support.refunds", "id int8"),
                        doc("support.tickets", "id int8"),
                    ],
                ),
                1,
            )
            .unwrap();
        }
        let next = generation(
            2,
            vec![
                doc("crm.accounts", "id int8"),
                doc("support.tickets", "id int8, zanzibar text"),
            ],
        );
        // Node order: crm.accounts (doc + 2 sections), then support.tickets (doc...). Fail after
        // the 4th write: the EXISTING support.tickets doc has been rewritten.
        {
            let mut e = open(&path);
            e.fail_replace_after = Some(4);
            assert!(e.replace_source(&next, 2).is_err());
        }
        // Fail after the tombstones (support.refunds is gone inside the transaction).
        {
            let mut e = open(&path);
            e.fail_replace_before_manifest = true;
            assert!(e.replace_source(&next, 2).is_err());
        }
        let mut e = open(&path);
        let tickets = e
            .store
            .get_node(&doc_symbol("s1", "support.tickets"))
            .unwrap()
            .unwrap();
        assert_eq!(tickets.metadata[meta::GENERATION], 1);
        assert!(
            e.store
                .get_node(&doc_symbol("s1", "support.refunds"))
                .unwrap()
                .is_some(),
            "the tombstone was rolled back"
        );
        assert!(
            e.store
                .get_node(&doc_symbol("s1", "crm.accounts"))
                .unwrap()
                .is_none()
        );
        assert!(
            e.recall_visible("zanzibar", 4000, None, &visible(), 3)
                .unwrap()
                .is_empty(),
            "the rolled-back text is not in FTS"
        );
        let page = e
            .source_catalog("s1", 0, None, &visible())
            .unwrap()
            .unwrap();
        assert_eq!((page.generation, page.total_docs), (1, 2));
    }
}
