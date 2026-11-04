//! A live source's structure as knowledge (DES-KNOWLEDGE-DATA-001 §4.1).
//!
//! A source (a database, warehouse, SaaS API, MCP server) keeps its rows; estate keeps its
//! **structure**: one document per relation or tool, under scope `source:<sourceId>`, plus the typed
//! catalog. Crew's describe submits one **complete generation** at a time
//! ([`KnowledgeEngine::replace_source`]):
//!
//! - ids are stable, derived from the source id, the document key and the section key, so edges to
//!   a relation survive a re-describe;
//! - the whole generation is written in ONE store transaction, and every document and section of
//!   the source that is absent from it is tombstoned (hard-deleted) in that same transaction. A
//!   generation that fails validation or fails mid-write writes nothing and tombstones nothing;
//! - typed edges that touch a tombstoned node are kept (a re-added key gets a new store epoch, so
//!   they stay hidden) and reported.
//!
//! [`KnowledgeEngine::source_catalog`] reads the complete, paged, versioned structure back.

use crate::engine::{KClass, KNode, KnowledgeEngine};
use crate::plane::{SOURCE_KIND, Visible, valid_source_id};
use serde_json::{Value, json};
use wicked_estate_core::{
    GraphRead, GraphWrite, Language, Location, Node, NodeKind, Scope, Span, Symbol, SymbolId,
    SymbolQuery,
};

/// The manifest node's kind: one per source, never a recall candidate.
pub const SOURCE_MANIFEST_KIND: &str = "ksource";

/// Limits on one generation (refused beyond, never truncated: a cut catalog is a wrong catalog).
pub const MAX_DOCS: usize = 20_000;
pub const MAX_SECTIONS_PER_DOC: usize = 256;
pub const MAX_KEY_BYTES: usize = 512;
pub const MAX_TITLE_BYTES: usize = 512;
pub const MAX_LABEL_BYTES: usize = 200;
pub const MAX_SECTION_TEXT_BYTES: usize = 32 * 1024;
pub const MAX_ENTRY_BYTES: usize = 64 * 1024;
pub const MAX_CATALOG_BYTES: usize = 256 * 1024;
/// A catalog page holds documents up to about this many bytes of JSON (at least one document), so
/// a page stays inside an agent's tool-output budget (R4, < 25K chars) for ordinary relations.
pub const PAGE_BYTES: usize = 20_000;
/// At most this many tombstoned keys / orphaned edges are listed (the counts are always exact).
const REPORT_LIST_CAP: usize = 50;

/// Metadata keys on source nodes (beside the knowledge keys).
mod meta {
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
    /// Keys of tombstoned documents (capped list) and the exact count of tombstoned nodes
    /// (documents and sections).
    pub tombstoned_docs: Vec<String>,
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
                self.source_id
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
        for doc in &self.docs {
            if !key_ok(&doc.key, MAX_KEY_BYTES) {
                return Err(bad(format!(
                    "doc key {:?} must be 1-{MAX_KEY_BYTES} bytes with no control characters",
                    doc.key
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
                        doc.key
                    )));
                }
                if entry.to_string().len() > MAX_ENTRY_BYTES {
                    return Err(bad(format!(
                        "doc {:?}: entry is over {MAX_ENTRY_BYTES} bytes",
                        doc.key
                    )));
                }
            }
            if doc.sections.is_empty() {
                return Err(bad(format!(
                    "doc {:?}: at least one section is required",
                    doc.key
                )));
            }
            if doc.sections.len() > MAX_SECTIONS_PER_DOC {
                return Err(bad(format!(
                    "doc {:?}: {} sections is over {MAX_SECTIONS_PER_DOC}",
                    doc.key,
                    doc.sections.len()
                )));
            }
            let mut skeys = std::collections::BTreeSet::new();
            for sec in &doc.sections {
                if !key_ok(&sec.key, MAX_KEY_BYTES) {
                    return Err(bad(format!(
                        "doc {:?}: section key {:?} must be 1-{MAX_KEY_BYTES} bytes",
                        doc.key, sec.key
                    )));
                }
                if !skeys.insert(sec.key.as_str()) {
                    return Err(bad(format!(
                        "doc {:?}: duplicate section key {:?}",
                        doc.key, sec.key
                    )));
                }
                if sec.text.trim().is_empty() || sec.text.len() > MAX_SECTION_TEXT_BYTES {
                    return Err(bad(format!(
                        "doc {:?} section {:?}: text must be 1-{MAX_SECTION_TEXT_BYTES} bytes",
                        doc.key, sec.key
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

    /// Replace a source's structure with one complete generation (see the module docs).
    pub fn replace_source(
        &mut self,
        g: &SourceGeneration,
        now: i64,
    ) -> wicked_estate_core::Result<ReplaceReport> {
        g.validate()?;
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
            kn.deps = deps.clone();
            kn.untrusted = true;
            let doc_sym = kn.symbol();
            let mut node = kn.to_node();
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
                skn.deps = deps.clone();
                skn.untrusted = true;
                let sym = skn.symbol();
                let mut snode = skn.to_node();
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
            m.insert(crate::engine::meta_keys::DEPS.into(), json!(deps));
        }

        // Everything below is ONE transaction: all of it lands, or none of it does.
        self.store.begin_batch()?;
        let written =
            self.replace_source_in_batch(g, &scope, &to_write, &derived, &new_ids, &manifest);
        let outcome = match written {
            Ok(report) => self.store.commit_batch().map(|_| report),
            Err(e) => Err(e),
        };
        match outcome {
            Ok(mut report) => {
                report.docs = docs.len();
                report.sections = section_count;
                Ok(report)
            }
            Err(e) => {
                self.store.rollback_batch()?;
                Err(e)
            }
        }
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
        // Generation order, read inside the transaction.
        if let Some(stored) = self.manifest(sid)? {
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
        }

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
        let mut orphaned = Vec::new();
        let mut orphaned_count = 0usize;
        if let Some(xedge) = self.xedge.as_ref() {
            let reader = xedge.reader();
            for n in &absent {
                let id = n.symbol.0.as_str();
                let mut edges = reader.typed_out("knowledge", id, &[])?;
                edges.extend(reader.typed_in("knowledge", id, &[])?);
                orphaned_count += edges.len();
                for e in edges {
                    if orphaned.len() < REPORT_LIST_CAP {
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
        let tombstoned_docs: Vec<String> = absent
            .iter()
            .filter(|n| matches!(&n.kind, NodeKind::Other(k) if k == KClass::Doc.as_kind()))
            .filter_map(|n| {
                n.metadata
                    .get(meta::SOURCE_KEY)
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .take(REPORT_LIST_CAP)
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
        self.store.upsert_nodes(std::slice::from_ref(manifest))?;

        Ok(ReplaceReport {
            source_id: sid.to_string(),
            generation: g.generation,
            docs: 0,
            sections: 0,
            tombstoned_docs,
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
        let mut docs = Vec::new();
        for key in &keys[range.0..range.1] {
            let Some(doc) = self.store.get_node(&doc_symbol(source_id, key))? else {
                continue;
            };
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
                if let Some(sec) = self
                    .store
                    .get_node(&section_symbol(source_id, key, sk))?
                    .as_ref()
                    .and_then(KNode::from_node)
                {
                    sections.push(json!({"key": sk, "text": sec.content}));
                }
            }
            docs.push(json!({
                "key": key,
                "title": dm.get(meta::TITLE).cloned().unwrap_or(Value::Null),
                "entry": dm.get(meta::ENTRY).cloned().unwrap_or_else(|| json!({})),
                "sections": sections,
            }));
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
