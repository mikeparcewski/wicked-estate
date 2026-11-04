//! Data shapes and the combined view (DES-KNOWLEDGE-DATA-001 §4.3, §10.1).
//!
//! A **data shape** is an approved knowledge node (usually a `kconcept`) with typed edges to what
//! it connects: source relations and columns, code entities, rules, metrics. An agent proposes it
//! into the inert proposal queue with `relate[]` (knowledge endpoints) and `relate_code[]` (code
//! endpoints); a person approves it.
//!
//! **Approve derives the deps.** The node's `deps` are the union of every endpoint's effective
//! deps (a source catalog's `source:<id>`, a code endpoint's `repo:<id>`, another shape's deps),
//! plus its own scope's. A proposal that declares deps different from those is refused, and
//! nothing is written.
//!
//! **Invisible until complete.** An approve crosses three stores (knowledge, the xedge overlay,
//! and memory's proposal queue), so it is ordered so that nothing is visible half-done:
//!
//! 1. a `kapply` journal records the full plan (the node and every edge row). No read path reads
//!    that kind: it is the invisible record of the approval;
//! 2. the typed edges are written, owned by the node's id. A reader keeps an edge only when its
//!    owner is a live knowledge node, so these are hidden until step 3;
//! 3. the knowledge node is written (node and vector in one transaction): the flip to visible;
//! 4. the caller marks the proposal approved;
//! 5. the journal is deleted ([`KnowledgeEngine::finish_proposal`]).
//!
//! Ids derive from the proposal id, so every step is an idempotent upsert. A start-up sweep reads
//! every journal: an approved proposal is completed ([`KnowledgeEngine::complete_proposal`]),
//! anything else is rolled back ([`KnowledgeEngine::rollback_proposal`]: its edges, its node, its
//! journal).
//!
//! [`KnowledgeEngine::neighbors`] walks the typed edges, filtered by the reader's `visible` set.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{Value, json};
use wicked_estate_core::{
    GraphRead, GraphWrite, Language, Location, Node, NodeKind, Span, Symbol, SymbolId, SymbolQuery,
};
use wicked_estate_memory_core::Proposal;
use wicked_estate_overlay::{Endpoint, TYPED_RELS, TypedXEdge, is_typed_rel};

use crate::engine::{KClass, KNode, KnowledgeEngine};
use crate::plane::{
    Visible, effective_deps, is_untrusted, normalize_deps, scope_deps, valid_dep, valid_repo_id,
};

/// The approval journal's node kind (never a knowledge kind, so never read as knowledge).
pub const APPLY_JOURNAL_KIND: &str = "kapply";
/// The store names typed edges use for their endpoints.
pub const KNOWLEDGE_STORE: &str = "knowledge";
pub const ESTATE_STORE: &str = "estate";

/// At most this many `relate[]` + `relate_code[]` entries per proposal.
pub const MAX_LINKS: usize = 200;
/// An entry's `evidence` object is at most this many bytes of JSON.
pub const MAX_EVIDENCE_BYTES: usize = 1024;
/// Longest endpoint id: a source section id with two 512-byte keys percent-encoded (×3) plus its
/// prefix fits.
const MAX_ID_BYTES: usize = 4096;
/// `knowledge.neighbors` bounds: depth 1-3, at most this many nodes and edges, at most this many
/// adjacency rows read per node and side, and an output budget that keeps the answer inside an
/// agent's tool-output limit (R4, < 25K chars). Any bound that bites sets `truncated`.
pub const MAX_NEIGHBOR_DEPTH: usize = 3;
pub const MAX_NEIGHBOR_NODES: usize = 100;
pub const MAX_NEIGHBOR_EDGES: usize = 200;
const ADJACENCY_LIMIT: usize = 400;
const NEIGHBOR_OUTPUT_BYTES: usize = 22_000;
/// An approval journal younger than this is in flight in another process and is never rolled
/// back by a sweep (an approve takes milliseconds; crew's estate call deadline is 30 s).
pub const APPLY_GRACE_SECS: i64 = 120;
const DEFAULT_CONFIDENCE: f32 = 0.8;

/// A typed link between two knowledge nodes; `from: None` is the proposed node itself.
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeLink {
    pub rel: String,
    pub from: Option<String>,
    pub to: String,
    pub confidence: f32,
    pub evidence: Option<Value>,
}

/// A typed link from a knowledge node to a code symbol in `repo`; `from: None` is the proposed node.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeLink {
    pub rel: String,
    pub from: Option<String>,
    pub code_id: String,
    pub repo: String,
    pub confidence: f32,
    pub evidence: Option<Value>,
}

/// The links and optional declared deps of a knowledge proposal's payload.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShapeLinks {
    pub relate: Vec<KnowledgeLink>,
    pub relate_code: Vec<CodeLink>,
    /// `payload.deps` when the proposer declared it (it must equal the derived set).
    pub declared_deps: Option<Vec<String>>,
}

fn id_ok(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= MAX_ID_BYTES && !s.chars().any(char::is_control)
}

fn entry_str(
    entry: &Value,
    field: &str,
    at: &str,
    required: bool,
) -> Result<Option<String>, String> {
    match entry.get(field) {
        None | Some(Value::Null) if !required => Ok(None),
        Some(Value::String(s)) if id_ok(s) => Ok(Some(s.clone())),
        _ => Err(format!(
            "{at}.{field} must be a non-empty string of at most {MAX_ID_BYTES} bytes"
        )),
    }
}

fn entry_rel(entry: &Value, at: &str) -> Result<String, String> {
    match entry.get("rel").and_then(Value::as_str) {
        Some(r) if is_typed_rel(r) => Ok(r.to_string()),
        _ => Err(format!(
            "{at}.rel must be EXACTLY one of {}",
            TYPED_RELS.join("|")
        )),
    }
}

fn entry_confidence(entry: &Value, at: &str) -> Result<f32, String> {
    match entry.get("confidence") {
        None | Some(Value::Null) => Ok(DEFAULT_CONFIDENCE),
        Some(v) => match v.as_f64() {
            Some(c) if (0.0..=1.0).contains(&c) => Ok(c as f32),
            _ => Err(format!("{at}.confidence must be a number in [0, 1]")),
        },
    }
}

fn entry_evidence(entry: &Value, at: &str) -> Result<Option<Value>, String> {
    match entry.get("evidence") {
        None | Some(Value::Null) => Ok(None),
        Some(v @ Value::Object(_)) if v.to_string().len() <= MAX_EVIDENCE_BYTES => {
            Ok(Some(v.clone()))
        }
        Some(_) => Err(format!(
            "{at}.evidence must be a JSON object of at most {MAX_EVIDENCE_BYTES} bytes"
        )),
    }
}

fn entries<'a>(payload: &'a Value, field: &str) -> Result<&'a [Value], String> {
    match payload.get(field) {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(xs)) => Ok(xs),
        Some(_) => Err(format!("payload.{field} must be an array")),
    }
}

/// Parse and validate `payload.relate`, `payload.relate_code` and `payload.deps` (the shape check
/// the queue runs at submit, so a proposer hears a mistake while it can still fix it).
pub fn parse_shape_links(payload: &Value) -> Result<ShapeLinks, String> {
    let rel_entries = entries(payload, "relate")?;
    let code_entries = entries(payload, "relate_code")?;
    if rel_entries.len() + code_entries.len() > MAX_LINKS {
        return Err(format!(
            "at most {MAX_LINKS} relate + relate_code entries per proposal"
        ));
    }
    let mut out = ShapeLinks::default();
    for (i, e) in rel_entries.iter().enumerate() {
        let at = format!("payload.relate[{i}]");
        if !e.is_object() {
            return Err(format!("{at} must be an object"));
        }
        out.relate.push(KnowledgeLink {
            rel: entry_rel(e, &at)?,
            from: entry_str(e, "from", &at, false)?,
            to: entry_str(e, "to", &at, true)?.unwrap_or_default(),
            confidence: entry_confidence(e, &at)?,
            evidence: entry_evidence(e, &at)?,
        });
    }
    for (i, e) in code_entries.iter().enumerate() {
        let at = format!("payload.relate_code[{i}]");
        if !e.is_object() {
            return Err(format!("{at} must be an object"));
        }
        let repo = match e.get("repo").and_then(Value::as_str) {
            Some(r) if valid_repo_id(r) => r.to_string(),
            _ => {
                return Err(format!(
                    "{at}.repo must name the code symbol's repo (its dep is repo:<repo>)"
                ));
            }
        };
        out.relate_code.push(CodeLink {
            rel: entry_rel(e, &at)?,
            from: entry_str(e, "from", &at, false)?,
            code_id: entry_str(e, "code_id", &at, true)?.unwrap_or_default(),
            repo,
            confidence: entry_confidence(e, &at)?,
            evidence: entry_evidence(e, &at)?,
        });
    }
    out.declared_deps = match payload.get("deps") {
        None | Some(Value::Null) => None,
        Some(Value::Array(xs)) => {
            let mut deps = Vec::with_capacity(xs.len());
            for x in xs {
                match x.as_str() {
                    Some(d) if valid_dep(d) => deps.push(d.to_string()),
                    _ => {
                        return Err(
                            "payload.deps entries must be source:<id> or repo:<id>".to_string()
                        );
                    }
                }
            }
            Some(deps)
        }
        Some(_) => return Err("payload.deps must be an array".to_string()),
    };
    Ok(out)
}

/// A proposal filed with a `repo` facet (crew's relay sets it to the unit's own repo and strips any
/// other, `mcp/estate-relay.ts`) may only link code in THAT repo: an agent proposer cannot name
/// another repo's symbols as its own to widen who sees the shape. An operator proposal with no
/// repo facet is trusted as written (estate holds no symbol-to-repo map; the reviewer sees it).
pub fn check_repo_facet(links: &ShapeLinks, repo_facet: Option<&str>) -> Result<(), String> {
    let Some(repo) = repo_facet else {
        return Ok(());
    };
    match links.relate_code.iter().find(|l| l.repo != repo) {
        Some(l) => Err(format!(
            "relate_code repo {:?} is not this proposal's repo {repo:?}: a proposal links only its own repo's code",
            l.repo
        )),
        None => Ok(()),
    }
}

/// The full plan of one approval (what the journal holds).
#[derive(Debug, Clone)]
pub struct ApprovalPlan {
    /// The knowledge node to write (kind and id final).
    pub node: Node,
    /// The text its vector is computed from.
    pub embed_text: String,
    pub edges: Vec<TypedXEdge>,
    pub deps: Vec<String>,
    /// Code endpoints the attached graph could not confirm (epoch 0, `code_verified: false`).
    pub unverified_code: usize,
}

pub(crate) fn journal_symbol(proposal_id: &str) -> SymbolId {
    Symbol::synthetic(APPLY_JOURNAL_KIND, proposal_id).id()
}

fn edge_to_json(e: &TypedXEdge) -> Value {
    json!({
        "src_store": e.source.engine, "src_id": e.source.stable_id, "src_epoch": e.source.epoch,
        "tgt_store": e.target.engine, "tgt_id": e.target.stable_id, "tgt_epoch": e.target.epoch,
        "rel": e.rel, "owner": e.owner, "confidence": e.confidence, "meta": e.meta,
    })
}

fn edge_from_json(v: &Value) -> Option<TypedXEdge> {
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let n = |k: &str| v.get(k).and_then(Value::as_u64);
    Some(TypedXEdge {
        source: Endpoint::new(s("src_store")?, s("src_id")?, n("src_epoch")?),
        target: Endpoint::new(s("tgt_store")?, s("tgt_id")?, n("tgt_epoch")?),
        rel: s("rel")?,
        owner: s("owner")?,
        confidence: v.get("confidence")?.as_f64()? as f32,
        meta: s("meta")?,
    })
}

/// The class a knowledge proposal asks for (default chunk).
fn proposal_class(payload: &Value) -> anyhow::Result<KClass> {
    Ok(match payload.get("class") {
        None => KClass::Chunk,
        Some(v) => match v.as_str() {
            Some("doc") => KClass::Doc,
            Some("section") => KClass::Section,
            Some("chunk") => KClass::Chunk,
            Some("concept") => KClass::Concept,
            _ => anyhow::bail!("invalid knowledge class: use doc|section|chunk|concept"),
        },
    })
}

impl KnowledgeEngine {
    /// Is this node an approval still in flight (a proposal-derived node whose `kapply` journal
    /// exists)? Such a node is invisible on every read path until its journal is dropped, which
    /// happens only after the queue marked the proposal approved. A store error answers `true`
    /// (hidden): fail closed.
    pub(crate) fn applying(&self, node: &SymbolId) -> bool {
        let id = node
            .as_str()
            .rsplit(' ')
            .next()
            .unwrap_or("")
            .trim_end_matches(':');
        let Some(pid) = id.strip_prefix("proposal:") else {
            return false;
        };
        !matches!(self.store.get_node(&journal_symbol(pid)), Ok(None))
    }

    /// A live, complete knowledge node by its full id (`kchunk synthetic …:` as recall returns
    /// it). An approval in flight is not live.
    fn live_knowledge(&self, id: &str) -> wicked_estate_core::Result<Option<KNode>> {
        let sym = SymbolId(id.to_string());
        if self.applying(&sym) {
            return Ok(None);
        }
        Ok(self
            .store
            .get_node(&sym)?
            .as_ref()
            .and_then(KNode::from_node))
    }

    /// Plan an approval: resolve every endpoint, derive the deps, refuse a mismatch. Pure (reads only).
    pub fn plan_proposal(
        &self,
        proposal: &Proposal,
        code_epochs: &HashMap<String, u64>,
        now: i64,
    ) -> anyhow::Result<ApprovalPlan> {
        let payload = &proposal.payload;
        let required = |field: &str| -> anyhow::Result<&str> {
            payload
                .get(field)
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!("knowledge payload.{field} (non-empty string) required")
                })
        };
        let links = parse_shape_links(payload).map_err(|e| anyhow::anyhow!(e))?;
        check_repo_facet(&links, proposal.facets.get("repo")).map_err(|e| anyhow::anyhow!(e))?;
        let mut kn = KNode::new(
            proposal_class(payload)?,
            required("content")?,
            payload.get("scope").and_then(Value::as_str).unwrap_or(""),
            required("source")?,
            now,
        );
        kn.id = format!("proposal:{}", proposal.id);
        let self_id = kn.symbol().0;

        // Resolve every knowledge endpoint: live, a knowledge kind (a pending or tombstoned node is
        // neither), and record its deps and epoch.
        let mut derived: BTreeSet<String> = scope_deps(&kn.scope).into_iter().collect();
        let mut epochs: BTreeMap<String, u64> = BTreeMap::new();
        let mut resolve = |id: &Option<String>,
                           derived: &mut BTreeSet<String>|
         -> anyhow::Result<String> {
            let Some(id) = id else {
                return Ok(self_id.clone());
            };
            if *id == self_id {
                return Ok(self_id.clone());
            }
            if !epochs.contains_key(id) {
                let Some(node) = self.live_knowledge(id)? else {
                    anyhow::bail!(
                        "relate endpoint {id:?} is not a live knowledge node (use the node_id recall returns)"
                    );
                };
                derived.extend(effective_deps(&node));
                let epoch = self.store.symbol_epoch(&SymbolId(id.clone()))?.unwrap_or(0);
                epochs.insert(id.clone(), epoch);
            }
            Ok(id.clone())
        };
        let mut resolved_rel = Vec::new();
        for l in &links.relate {
            let from = resolve(&l.from, &mut derived)?;
            let to = resolve(&Some(l.to.clone()), &mut derived)?;
            if from == to {
                anyhow::bail!("relate {:?}: an edge cannot loop to itself", l.rel);
            }
            resolved_rel.push((l, from, to));
        }
        let mut resolved_code = Vec::new();
        for l in &links.relate_code {
            let from = resolve(&l.from, &mut derived)?;
            derived.insert(format!("repo:{}", l.repo));
            resolved_code.push((l, from));
        }
        let deps: Vec<String> = derived.into_iter().collect();
        if let Some(declared) = &links.declared_deps {
            let declared = normalize_deps(declared.iter().cloned());
            if declared != deps {
                anyhow::bail!(
                    "knowledge proposal {}: declared deps {declared:?} differ from the deps its endpoints derive {deps:?}; \
                     estate derives deps and they can never be narrowed",
                    proposal.id
                );
            }
        }
        kn.deps = deps.clone();

        let epoch_of = |id: &str| epochs.get(id).copied().unwrap_or(0);
        let meta = |extra: Value| -> String {
            let mut m = json!({ "deps": deps });
            if let (Some(obj), Value::Object(more)) = (m.as_object_mut(), extra) {
                obj.extend(more);
            }
            m.to_string()
        };
        let mut edges: Vec<TypedXEdge> = Vec::new();
        let mut seen: BTreeSet<(String, String, String, String, String)> = BTreeSet::new();
        let mut push = |e: TypedXEdge, edges: &mut Vec<TypedXEdge>| {
            let key = (
                e.rel.clone(),
                e.source.engine.clone(),
                e.source.stable_id.clone(),
                e.target.engine.clone(),
                e.target.stable_id.clone(),
            );
            if seen.insert(key) {
                edges.push(e);
            }
        };
        let mut linked_from_self: BTreeSet<(String, String)> = BTreeSet::new();
        let mut endpoints: BTreeSet<(String, String)> = BTreeSet::new();
        for (l, from, to) in resolved_rel {
            for id in [&from, &to] {
                if *id != self_id {
                    endpoints.insert((KNOWLEDGE_STORE.into(), id.clone()));
                }
            }
            if from == self_id {
                linked_from_self.insert((KNOWLEDGE_STORE.into(), to.clone()));
            }
            let mut extra = json!({});
            if let Some(ev) = &l.evidence {
                extra["evidence"] = ev.clone();
            }
            push(
                TypedXEdge {
                    source: Endpoint::new(KNOWLEDGE_STORE, from.clone(), epoch_of(&from)),
                    target: Endpoint::new(KNOWLEDGE_STORE, to.clone(), epoch_of(&to)),
                    rel: l.rel.clone(),
                    owner: self_id.clone(),
                    confidence: l.confidence,
                    meta: meta(extra),
                },
                &mut edges,
            );
        }
        let mut unverified_code = 0usize;
        for (l, from) in resolved_code {
            if from != self_id {
                endpoints.insert((KNOWLEDGE_STORE.into(), from.clone()));
            } else {
                linked_from_self.insert((ESTATE_STORE.into(), l.code_id.clone()));
            }
            endpoints.insert((ESTATE_STORE.into(), l.code_id.clone()));
            let verified = code_epochs.contains_key(&l.code_id);
            if !verified {
                unverified_code += 1;
            }
            let mut extra = json!({ "repo": l.repo, "code_verified": verified });
            if let Some(ev) = &l.evidence {
                extra["evidence"] = ev.clone();
            }
            push(
                TypedXEdge {
                    source: Endpoint::new(KNOWLEDGE_STORE, from.clone(), epoch_of(&from)),
                    target: Endpoint::new(
                        ESTATE_STORE,
                        l.code_id.clone(),
                        code_epochs.get(&l.code_id).copied().unwrap_or(0),
                    ),
                    rel: l.rel.clone(),
                    owner: self_id.clone(),
                    confidence: l.confidence,
                    meta: meta(extra),
                },
                &mut edges,
            );
        }
        // Hub edges: the shape is derived_from everything it touches and is not already linked to,
        // so `neighbors(shape, 1)` shows all of it.
        let repo_of: HashMap<&str, &str> = links
            .relate_code
            .iter()
            .map(|l| (l.code_id.as_str(), l.repo.as_str()))
            .collect();
        for (store, id) in &endpoints {
            if linked_from_self.contains(&(store.clone(), id.clone())) {
                continue;
            }
            let (epoch, extra) = if store == ESTATE_STORE {
                (
                    code_epochs.get(id).copied().unwrap_or(0),
                    json!({
                        "repo": repo_of.get(id.as_str()).copied().unwrap_or_default(),
                        "code_verified": code_epochs.contains_key(id),
                    }),
                )
            } else {
                (epoch_of(id), json!({}))
            };
            push(
                TypedXEdge {
                    source: Endpoint::new(KNOWLEDGE_STORE, self_id.clone(), 0),
                    target: Endpoint::new(store.clone(), id.clone(), epoch),
                    rel: "derived_from".into(),
                    owner: self_id.clone(),
                    confidence: 1.0,
                    meta: meta(extra),
                },
                &mut edges,
            );
        }

        let mut node = kn.to_node();
        if !proposal.facets.is_empty() {
            node.metadata.insert(
                wicked_estate_memory_core::meta_keys::FACETS.into(),
                serde_json::to_value(&proposal.facets)?,
            );
        }
        Ok(ApprovalPlan {
            embed_text: kn.content.clone(),
            node,
            edges,
            deps,
            unverified_code,
        })
    }

    /// Write a plan's edges, then its node (the flip). Edges first: until the node exists their
    /// owner is not live, so no reader keeps them.
    fn put_plan(&mut self, plan: &ApprovalPlan) -> anyhow::Result<()> {
        if !plan.edges.is_empty() {
            let xedge = self
                .xedge
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("xedge store not available"))?;
            for e in &plan.edges {
                xedge.put_typed(e)?;
            }
        }
        #[cfg(test)]
        if self.fail_apply_before_node {
            anyhow::bail!("injected crash between the edges and the node");
        }
        // The flip: node and vector together.
        let vec = self.embed(&plan.embed_text);
        self.store.begin_batch()?;
        let written = self
            .store
            .upsert_nodes(std::slice::from_ref(&plan.node))
            .and_then(|_| self.store.set_embedding(&plan.node.symbol, &vec));
        match written.and_then(|_| self.store.commit_batch()) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.store.rollback_batch()?;
                Err(e.into())
            }
        }
    }

    /// Apply an approval up to the flip (steps 1-3 of the module protocol); returns the node id.
    /// The caller marks the proposal approved, then calls [`Self::finish_proposal`].
    pub fn apply_proposal(
        &mut self,
        proposal: &Proposal,
        code_epochs: &HashMap<String, u64>,
        now: i64,
    ) -> anyhow::Result<(String, ApprovalPlan)> {
        let plan = self.plan_proposal(proposal, code_epochs, now)?;
        if !plan.edges.is_empty() && self.xedge.is_none() {
            anyhow::bail!("xedge store not available: a proposal with relate[] cannot be approved");
        }
        // 1. The journal: the invisible record of the whole plan.
        let mut journal = Node::new(
            journal_symbol(&proposal.id),
            NodeKind::Other(APPLY_JOURNAL_KIND.into()),
            format!("apply:{}", proposal.id),
            Language::new("knowledge"),
            Location::new("knowledge", Span::ZERO),
        );
        let m = &mut journal.metadata;
        m.insert("proposal_id".into(), proposal.id.clone().into());
        m.insert("owner".into(), plan.node.symbol.0.clone().into());
        m.insert("node".into(), serde_json::to_string(&plan.node)?.into());
        m.insert("embed_text".into(), plan.embed_text.clone().into());
        m.insert(
            "edges".into(),
            Value::Array(plan.edges.iter().map(edge_to_json).collect()),
        );
        m.insert("created_at".into(), now.into());
        self.store.upsert_nodes(std::slice::from_ref(&journal))?;
        // 2-3. Edges (hidden: their owner is not live yet), then the node.
        self.put_plan(&plan)?;
        Ok((plan.node.symbol.0.clone(), plan))
    }

    /// Step 5: the approval is complete; drop its journal.
    pub fn finish_proposal(&mut self, proposal_id: &str) -> anyhow::Result<()> {
        self.store.remove_nodes(&[journal_symbol(proposal_id)])?;
        Ok(())
    }

    /// Roll back a half-done approval: its typed edges, its node, its journal. `false` when there
    /// is no journal (nothing in flight, so nothing is touched — an approved node is never undone).
    pub fn rollback_proposal(&mut self, proposal_id: &str) -> anyhow::Result<bool> {
        let Some(journal) = self.store.get_node(&journal_symbol(proposal_id))? else {
            return Ok(false);
        };
        let owner = journal
            .metadata
            .get("owner")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                Symbol::synthetic(KClass::Chunk.as_kind(), format!("proposal:{proposal_id}"))
                    .id()
                    .0
            });
        // Hide first (the node), then the edges, then the journal last: a rollback that fails part
        // way leaves the journal, so the node stays hidden and the next sweep finishes the job.
        self.store.remove_nodes(&[SymbolId(owner.clone())])?;
        if let Some(xedge) = self.xedge.as_ref() {
            xedge.delete_typed_by_owner(&owner)?;
        }
        self.store.remove_nodes(&[journal_symbol(proposal_id)])?;
        Ok(true)
    }

    /// Complete an approval whose proposal is already approved: re-put the plan from the journal if
    /// the node is missing, then drop the journal.
    pub fn complete_proposal(&mut self, proposal_id: &str) -> anyhow::Result<()> {
        let Some(journal) = self.store.get_node(&journal_symbol(proposal_id))? else {
            return Ok(());
        };
        let m = &journal.metadata;
        let node: Option<Node> = m
            .get("node")
            .and_then(Value::as_str)
            .and_then(|s| serde_json::from_str(s).ok());
        if let Some(node) = node {
            if self.store.get_node(&node.symbol)?.is_none() {
                let edges: Vec<TypedXEdge> = m
                    .get("edges")
                    .and_then(Value::as_array)
                    .map(|xs| xs.iter().filter_map(edge_from_json).collect())
                    .unwrap_or_default();
                let plan = ApprovalPlan {
                    embed_text: m
                        .get("embed_text")
                        .and_then(Value::as_str)
                        .unwrap_or(&node.name)
                        .to_string(),
                    node,
                    edges,
                    deps: Vec::new(),
                    unverified_code: 0,
                };
                self.put_plan(&plan)?;
            }
        }
        self.finish_proposal(proposal_id)
    }

    /// The approvals with a journal (in flight, or interrupted): `(proposal id, journal written at)`.
    pub fn applying_proposals(&self) -> anyhow::Result<Vec<(String, i64)>> {
        Ok(self
            .store
            .find_symbols(&SymbolQuery {
                text: None,
                exact_name: None,
                kinds: vec![NodeKind::Other(APPLY_JOURNAL_KIND.into())],
                language: None,
                limit: None,
                scope_prefix: None,
            })?
            .iter()
            .filter_map(|n| {
                let pid = n.metadata.get("proposal_id").and_then(Value::as_str)?;
                let at = n
                    .metadata
                    .get("created_at")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                Some((pid.to_string(), at))
            })
            .collect())
    }
}

// ── knowledge.neighbors ─────────────────────────────────────────────────────

/// A `knowledge.neighbors` request.
#[derive(Debug, Clone)]
pub struct NeighborsQuery {
    /// `"knowledge"` (a knowledge node id) or `"estate"` (a code symbol id).
    pub store: String,
    pub id: String,
    /// Typed rels to follow (empty = all of them).
    pub rels: Vec<String>,
    pub depth: usize,
    pub visible: Visible,
}

/// One node of a neighbourhood.
#[derive(Debug, Clone, PartialEq)]
pub struct NeighborNode {
    pub store: String,
    pub id: String,
    /// The knowledge class (`kdoc`…) or the code node's kind; `None` for an unhydrated code id.
    pub class: Option<String>,
    pub label: String,
    pub scope: String,
    pub deps: Vec<String>,
    pub untrusted: bool,
    pub source: String,
    /// `false` for a code endpoint the attached graph does not hold.
    pub hydrated: bool,
}

/// One edge of a neighbourhood.
#[derive(Debug, Clone, PartialEq)]
pub struct NeighborEdge {
    pub rel: String,
    pub from: (String, String),
    pub to: (String, String),
    pub owner: String,
    pub confidence: f32,
    pub meta: Value,
}

/// A bounded, visibility-filtered neighbourhood.
#[derive(Debug, Clone, PartialEq)]
pub struct Neighbors {
    pub nodes: Vec<NeighborNode>,
    pub edges: Vec<NeighborEdge>,
    /// The node cap cut the walk.
    pub truncated: bool,
}

const LABEL_CHARS: usize = 120;

impl KnowledgeEngine {
    /// A visible knowledge node as a neighbourhood node, else `None`.
    fn knowledge_neighbor(
        &self,
        id: &str,
        visible: &Visible,
    ) -> wicked_estate_core::Result<Option<NeighborNode>> {
        let Some(kn) = self.live_knowledge(id)? else {
            return Ok(None);
        };
        let deps = effective_deps(&kn);
        if !visible.allows(&deps) {
            return Ok(None);
        }
        Ok(Some(NeighborNode {
            store: KNOWLEDGE_STORE.into(),
            id: id.to_string(),
            class: Some(kn.class.as_kind().to_string()),
            label: kn.content.chars().take(LABEL_CHARS).collect(),
            scope: kn.scope.clone(),
            untrusted: is_untrusted(&kn),
            deps,
            source: kn.source,
            hydrated: true,
        }))
    }

    fn code_neighbor(&self, id: &str, repo: Option<&str>, graph: &dyn GraphRead) -> NeighborNode {
        // A code endpoint always names its repo; one that does not is never visible (fail closed).
        let deps: Vec<String> = match repo.filter(|r| !r.is_empty()) {
            Some(r) => vec![format!("repo:{r}")],
            None => vec!["invalid:no-repo".to_string()],
        };
        match graph.get_node(&SymbolId(id.to_string())).ok().flatten() {
            Some(n) => NeighborNode {
                store: ESTATE_STORE.into(),
                id: id.to_string(),
                class: Some(match serde_json::to_value(&n.kind) {
                    Ok(Value::String(k)) => k,
                    Ok(other) => other.to_string(),
                    Err(_) => String::new(),
                }),
                label: n.name.chars().take(LABEL_CHARS).collect(),
                scope: n.scope.as_path(),
                deps,
                untrusted: false,
                source: n.location.file.clone(),
                hydrated: true,
            },
            None => NeighborNode {
                store: ESTATE_STORE.into(),
                id: id.to_string(),
                class: None,
                label: id.chars().take(LABEL_CHARS).collect(),
                scope: String::new(),
                deps,
                untrusted: false,
                source: String::new(),
                hydrated: false,
            },
        }
    }

    /// Is the endpoint's stamped epoch its live one? An owner endpoint is checked by the owner's
    /// liveness instead (its own epoch is not known before the flip).
    fn epoch_ok(&self, ep: &Endpoint, owner: &str, graph: &dyn GraphRead) -> bool {
        if ep.engine == KNOWLEDGE_STORE && ep.stable_id == owner {
            return true;
        }
        let sym = SymbolId(ep.stable_id.clone());
        let live = if ep.engine == KNOWLEDGE_STORE {
            self.store.symbol_epoch(&sym).ok().flatten()
        } else {
            match graph.get_node(&sym) {
                Ok(Some(_)) => graph.symbol_epoch(&sym).ok().flatten(),
                // A code id the attached graph does not hold cannot be checked: keep it, unhydrated.
                _ => return true,
            }
        };
        live.unwrap_or(0) == ep.epoch
    }

    /// The typed neighbourhood of a node, filtered by `q.visible`, or `None` when the start node is
    /// unknown or hidden (alike). Bounded: depth ≤ 3, at most [`MAX_NEIGHBOR_NODES`] nodes and
    /// [`MAX_NEIGHBOR_EDGES`] edges, a bounded adjacency read per node, and an output budget;
    /// `truncated` says a bound bit. A code seed is echoed back as its id only: the walk returns
    /// nothing about it that the caller did not send.
    pub fn neighbors(
        &self,
        q: &NeighborsQuery,
        graph: &dyn GraphRead,
    ) -> anyhow::Result<Option<Neighbors>> {
        let Some(xedge) = self.xedge.as_ref() else {
            anyhow::bail!("xedge store not available");
        };
        let reader = xedge.reader();
        let start = match q.store.as_str() {
            KNOWLEDGE_STORE => match self.knowledge_neighbor(&q.id, &q.visible)? {
                Some(n) => n,
                None => return Ok(None),
            },
            ESTATE_STORE => NeighborNode {
                store: ESTATE_STORE.into(),
                id: q.id.clone(),
                class: None,
                label: String::new(),
                scope: String::new(),
                deps: Vec::new(),
                untrusted: false,
                source: String::new(),
                hydrated: false,
            },
            other => anyhow::bail!("unknown store {other:?}: expected knowledge or estate"),
        };
        let rels: Vec<&str> = q.rels.iter().map(String::as_str).collect();
        let depth = q.depth.clamp(1, MAX_NEIGHBOR_DEPTH);

        let node_bytes = |n: &NeighborNode| {
            96 + n.id.len()
                + n.label.len()
                + n.scope.len()
                + n.source.len()
                + n.deps.iter().map(|d| d.len() + 3).sum::<usize>()
        };
        let mut used = node_bytes(&start);
        let mut nodes: Vec<NeighborNode> = vec![start];
        let mut index: BTreeSet<(String, String)> = BTreeSet::new();
        index.insert((q.store.clone(), q.id.clone()));
        let mut edges: Vec<NeighborEdge> = Vec::new();
        let mut edge_keys: BTreeSet<(String, String, String, String, String, String)> =
            BTreeSet::new();
        let mut owners: HashMap<String, bool> = HashMap::new();
        let mut frontier: Vec<(String, String)> = vec![(q.store.clone(), q.id.clone())];
        let mut truncated = false;

        'walk: for _ in 0..depth {
            let mut next = Vec::new();
            for (store, id) in &frontier {
                let out = reader.typed_out(store, id, &rels, ADJACENCY_LIMIT)?;
                let inn = reader.typed_in(store, id, &rels, ADJACENCY_LIMIT)?;
                if out.len() >= ADJACENCY_LIMIT || inn.len() >= ADJACENCY_LIMIT {
                    truncated = true;
                }
                for e in out.into_iter().chain(inn) {
                    let meta: Value = serde_json::from_str(&e.meta).unwrap_or_else(|_| json!({}));
                    // The owner must be live (an approval in flight is hidden) and visible, and
                    // so must every dep the edge carries (a malformed one never is).
                    let owner_ok = match owners.get(&e.owner) {
                        Some(ok) => *ok,
                        None => {
                            let ok = self.knowledge_neighbor(&e.owner, &q.visible)?.is_some();
                            owners.insert(e.owner.clone(), ok);
                            ok
                        }
                    };
                    let deps_ok = match meta.get("deps") {
                        None => true,
                        Some(v) => v.as_array().is_some_and(|xs| {
                            xs.iter()
                                .all(|x| x.as_str().is_some_and(|d| q.visible.contains(d)))
                        }),
                    };
                    if !owner_ok || !deps_ok {
                        continue;
                    }
                    if !self.epoch_ok(&e.source, &e.owner, graph)
                        || !self.epoch_ok(&e.target, &e.owner, graph)
                    {
                        continue;
                    }
                    let here = (store.clone(), id.clone());
                    let src = (e.source.engine.clone(), e.source.stable_id.clone());
                    let tgt = (e.target.engine.clone(), e.target.stable_id.clone());
                    let other = if src == here {
                        tgt.clone()
                    } else {
                        src.clone()
                    };
                    if !index.contains(&other) {
                        let node = if other.0 == KNOWLEDGE_STORE {
                            self.knowledge_neighbor(&other.1, &q.visible)?
                        } else {
                            let repo = meta.get("repo").and_then(Value::as_str);
                            let n = self.code_neighbor(&other.1, repo, graph);
                            q.visible.allows(&n.deps).then_some(n)
                        };
                        let Some(node) = node else {
                            continue; // hidden or gone: the walk never passes through it
                        };
                        let cost = node_bytes(&node);
                        if nodes.len() >= MAX_NEIGHBOR_NODES || used + cost > NEIGHBOR_OUTPUT_BYTES
                        {
                            truncated = true;
                            break 'walk;
                        }
                        used += cost;
                        index.insert(other.clone());
                        nodes.push(node);
                        next.push(other.clone());
                    }
                    let key = (
                        e.rel.clone(),
                        src.0.clone(),
                        src.1.clone(),
                        tgt.0.clone(),
                        tgt.1.clone(),
                        e.owner.clone(),
                    );
                    if edge_keys.contains(&key) {
                        continue;
                    }
                    let cost = 96
                        + e.rel.len()
                        + src.1.len()
                        + tgt.1.len()
                        + e.owner.len()
                        + meta.to_string().len();
                    if edges.len() >= MAX_NEIGHBOR_EDGES || used + cost > NEIGHBOR_OUTPUT_BYTES {
                        truncated = true;
                        break 'walk;
                    }
                    used += cost;
                    edge_keys.insert(key);
                    edges.push(NeighborEdge {
                        rel: e.rel,
                        from: src,
                        to: tgt,
                        owner: e.owner,
                        confidence: e.confidence,
                        meta,
                    });
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        Ok(Some(Neighbors {
            nodes,
            edges,
            truncated,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{SourceDoc, SourceGeneration, SourceSection, doc_symbol};
    use std::collections::BTreeMap as Map;
    use std::sync::Arc;
    use wicked_estate_memory_core::{Facets, ProposalState};
    use wicked_estate_overlay::XedgeStore;
    use wicked_estate_store::SqliteStore;

    fn engine_with_catalog() -> KnowledgeEngine {
        let mut e = KnowledgeEngine::in_memory()
            .unwrap()
            .with_xedge_store(Arc::new(XedgeStore::in_memory().unwrap()));
        let doc = |key: &str| SourceDoc {
            key: key.into(),
            title: None,
            entry: None,
            sections: vec![SourceSection {
                key: "columns".into(),
                text: "id int8, customer_id int8".into(),
            }],
        };
        e.replace_source(
            &SourceGeneration {
                source_id: "s1".into(),
                generation: 1,
                label: None,
                catalog: None,
                docs: vec![doc("support.tickets")],
            },
            1,
        )
        .unwrap();
        e.replace_source(
            &SourceGeneration {
                source_id: "s2".into(),
                generation: 1,
                label: None,
                catalog: None,
                docs: vec![doc("crm.customers")],
            },
            1,
        )
        .unwrap();
        e
    }

    fn tickets() -> String {
        doc_symbol("s1", "support.tickets").0
    }
    fn customers() -> String {
        doc_symbol("s2", "crm.customers").0
    }

    fn proposal(payload: Value) -> Proposal {
        Proposal {
            id: "p1".into(),
            kind_type: "knowledge".into(),
            payload,
            facets: Facets::default(),
            provenance: Map::new(),
            state: ProposalState::Pending,
            created_at: 1,
        }
    }

    fn shape_payload(deps: Option<Value>) -> Value {
        let mut p = json!({
            "content": "support.tickets.customer_id joins crm.customers.id; a ticket is a Ticket",
            "source": "data-shapes:support",
            "class": "concept",
            "relate": [{
                "rel": "joins", "from": tickets(), "to": customers(),
                "evidence": {"columns": [["customer_id", "id"]], "kind": "fk"}
            }],
            "relate_code": [{"rel": "describes", "from": tickets(), "code_id": "rust::support::Ticket", "repo": "support-app"}],
        });
        if let Some(d) = deps {
            p["deps"] = d;
        }
        p
    }

    fn all_visible() -> Visible {
        Visible::from_deps(["source:s1", "source:s2", "repo:support-app"]).unwrap()
    }

    fn shape_id() -> String {
        Symbol::synthetic("kconcept", "proposal:p1").id().0
    }

    /// NEGATIVE CONTROL: a proposal declaring fewer deps than its endpoints derive is refused,
    /// and nothing is written (no journal, no node, no edge).
    #[test]
    fn declaring_fewer_deps_than_the_endpoints_is_refused() {
        let mut e = engine_with_catalog();
        let p = proposal(shape_payload(Some(json!(["source:s1"]))));
        let err = e
            .apply_proposal(&p, &HashMap::new(), 2)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("differ from the deps its endpoints derive"),
            "{err}"
        );
        assert!(e.applying_proposals().unwrap().is_empty());
        assert!(e.store.get_node(&SymbolId(shape_id())).unwrap().is_none());
        assert_eq!(e.xedge.as_ref().unwrap().typed_len().unwrap(), 0);
        // More deps than derived is refused too: deps are derived, not declared.
        let p = proposal(shape_payload(Some(json!([
            "source:s1",
            "source:s2",
            "repo:support-app",
            "repo:other"
        ]))));
        assert!(e.apply_proposal(&p, &HashMap::new(), 2).is_err());
    }

    #[test]
    fn approve_derives_deps_and_the_neighbourhood_respects_them() {
        let mut e = engine_with_catalog();
        let exact = json!(["repo:support-app", "source:s2", "source:s1"]);
        let p = proposal(shape_payload(Some(exact)));
        let mut epochs = HashMap::new();
        epochs.insert("rust::support::Ticket".to_string(), 0u64);
        let (id, plan) = e.apply_proposal(&p, &epochs, 2).unwrap();
        e.finish_proposal("p1").unwrap();
        assert_eq!(id, shape_id());
        assert_eq!(
            plan.deps,
            vec!["repo:support-app", "source:s1", "source:s2"],
            "derived: both catalogs' sources and the code's repo"
        );
        assert_eq!(plan.unverified_code, 0);
        let kn = e
            .live_knowledge(&id)
            .unwrap()
            .expect("visible after approve");
        assert_eq!(kn.deps, plan.deps);
        // joins + describes + hub derived_from to customers... tickets and customers are linked
        // by `joins` (not from the shape), so the shape hubs to both, and to the code.
        let rels: Vec<&str> = plan.edges.iter().map(|e| e.rel.as_str()).collect();
        assert_eq!(
            rels,
            vec![
                "joins",
                "describes",
                "derived_from",
                "derived_from",
                "derived_from"
            ]
        );

        let graph = SqliteStore::in_memory().unwrap();
        let q = |visible: Visible, depth: usize| NeighborsQuery {
            store: KNOWLEDGE_STORE.into(),
            id: tickets(),
            rels: vec![],
            depth,
            visible,
        };
        let full = e.neighbors(&q(all_visible(), 1), &graph).unwrap().unwrap();
        let ids: BTreeSet<&str> = full.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(customers().as_str()), "joins");
        assert!(ids.contains("rust::support::Ticket"), "describes");
        assert!(
            ids.contains(shape_id().as_str()),
            "the shape hubs to the table"
        );
        let joins = full.edges.iter().find(|x| x.rel == "joins").unwrap();
        assert_eq!(joins.meta["evidence"]["kind"], "fk");
        let code = full
            .nodes
            .iter()
            .find(|n| n.id == "rust::support::Ticket")
            .unwrap();
        assert!(!code.hydrated, "the attached graph does not hold it");
        assert_eq!(code.deps, vec!["repo:support-app".to_string()]);

        // A reader that cannot see the repo sees neither the code nor ANY edge the shape owns
        // (its deps include the repo), and the walk does not pass through the shape.
        let partial = Visible::from_deps(["source:s1", "source:s2"]).unwrap();
        let some = e.neighbors(&q(partial, 2), &graph).unwrap().unwrap();
        assert_eq!(some.nodes.len(), 1, "only the start: {:?}", some.nodes);
        assert!(some.edges.is_empty());
        // And recall hides the shape from that reader too.
        let partial = Visible::from_deps(["source:s1", "source:s2"]).unwrap();
        assert!(
            e.recall_visible("customer_id joins", 4000, None, &partial, 3)
                .unwrap()
                .iter()
                .all(|h| h.id.0 != shape_id())
        );
        assert!(
            e.recall_visible("customer_id joins", 4000, None, &all_visible(), 3)
                .unwrap()
                .iter()
                .any(|h| h.id.0 == shape_id())
        );
        // A hidden start node answers like an unknown one.
        assert!(
            e.neighbors(&q(Visible::none(), 1), &graph)
                .unwrap()
                .is_none()
        );
    }

    /// NEGATIVE CONTROL: a crash between the node and its edges leaves nothing visible, and after
    /// the sweep's rollback nothing is left at all.
    #[test]
    fn a_crash_between_the_edges_and_the_node_leaves_nothing_visible() {
        let mut e = engine_with_catalog();
        e.fail_apply_before_node = true;
        let p = proposal(shape_payload(None));
        assert!(e.apply_proposal(&p, &HashMap::new(), 2).is_err());
        e.fail_apply_before_node = false;
        // Mid-flight state: the journal and the edges exist, the node does not.
        assert_eq!(e.applying_proposals().unwrap(), vec![("p1".to_string(), 2)]);
        assert!(e.xedge.as_ref().unwrap().typed_len().unwrap() > 0);
        let graph = SqliteStore::in_memory().unwrap();
        let q = NeighborsQuery {
            store: KNOWLEDGE_STORE.into(),
            id: tickets(),
            rels: vec![],
            depth: 2,
            visible: all_visible(),
        };
        let n = e.neighbors(&q, &graph).unwrap().unwrap();
        assert_eq!(
            n.nodes.len(),
            1,
            "no edge of a half-done approval is visible"
        );
        assert!(
            e.recall_visible("customer_id joins", 4000, None, &all_visible(), 3)
                .unwrap()
                .iter()
                .all(|h| h.id.0 != shape_id())
        );
        // The sweep (proposal still pending) rolls it back.
        assert!(e.rollback_proposal("p1").unwrap());
        assert!(e.applying_proposals().unwrap().is_empty());
        assert_eq!(e.xedge.as_ref().unwrap().typed_len().unwrap(), 0);
        assert!(
            !e.rollback_proposal("p1").unwrap(),
            "nothing left in flight"
        );
    }

    #[test]
    fn a_crash_after_the_node_rolls_back_when_pending_and_completes_when_approved() {
        let mut e = engine_with_catalog();
        let p = proposal(shape_payload(None));
        e.apply_proposal(&p, &HashMap::new(), 2).unwrap();
        // Not finished: the node is written but its journal is still there, so every read path
        // hides it (a --readonly server, which never sweeps, sees nothing half-done).
        let sym = SymbolId(shape_id());
        assert!(e.store.get_node(&sym).unwrap().is_some());
        assert!(e.live_knowledge(&shape_id()).unwrap().is_none());
        assert!(
            e.recall_visible("customer_id joins", 4000, None, &all_visible(), 3)
                .unwrap()
                .iter()
                .all(|h| h.id.0 != shape_id()),
            "recall hides an approval in flight"
        );
        let graph = SqliteStore::in_memory().unwrap();
        let q = NeighborsQuery {
            store: KNOWLEDGE_STORE.into(),
            id: tickets(),
            rels: vec![],
            depth: 2,
            visible: all_visible(),
        };
        assert_eq!(e.neighbors(&q, &graph).unwrap().unwrap().nodes.len(), 1);
        // Pending ⇒ the rollback removes the node, the edges and (last) the journal.
        assert!(e.rollback_proposal("p1").unwrap());
        assert!(e.store.get_node(&sym).unwrap().is_none());
        assert_eq!(e.xedge.as_ref().unwrap().typed_len().unwrap(), 0);
        assert!(e.applying_proposals().unwrap().is_empty());

        // Approved but the node never landed (journal + edges only) ⇒ complete re-puts it.
        e.fail_apply_before_node = true;
        assert!(e.apply_proposal(&p, &HashMap::new(), 3).is_err());
        e.fail_apply_before_node = false;
        e.complete_proposal("p1").unwrap();
        assert!(e.live_knowledge(&shape_id()).unwrap().is_some());
        assert!(e.applying_proposals().unwrap().is_empty());
        // A completed approval is never rolled back (no journal).
        assert!(!e.rollback_proposal("p1").unwrap());
        assert!(e.live_knowledge(&shape_id()).unwrap().is_some());
    }

    #[test]
    fn endpoints_must_be_live_knowledge() {
        let mut e = engine_with_catalog();
        let mut payload = shape_payload(None);
        payload["relate"][0]["to"] = json!("kdoc synthetic source:s9/nope:");
        let err = e
            .apply_proposal(&proposal(payload), &HashMap::new(), 2)
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a live knowledge node"), "{err}");
        assert!(e.applying_proposals().unwrap().is_empty());
    }

    #[test]
    fn shape_links_are_validated_at_parse() {
        assert!(parse_shape_links(&json!({})).unwrap().relate.is_empty());
        for bad in [
            json!({"relate": {}}),
            json!({"relate": [{"rel": "about", "to": "x"}]}),
            json!({"relate": [{"rel": "joins"}]}),
            json!({"relate": [{"rel": "joins", "to": "x", "confidence": 2}]}),
            json!({"relate": [{"rel": "joins", "to": "x", "evidence": "fk"}]}),
            json!({"relate_code": [{"rel": "describes", "code_id": "c"}]}),
            json!({"relate_code": [{"rel": "describes", "code_id": "c", "repo": "has space"}]}),
            json!({"deps": ["table:x"]}),
            json!({"deps": "source:s1"}),
        ] {
            assert!(parse_shape_links(&bad).is_err(), "must refuse {bad}");
        }
        let ok = parse_shape_links(&shape_payload(Some(json!(["source:s1"])))).unwrap();
        assert_eq!(ok.relate.len(), 1);
        assert_eq!(ok.relate_code[0].repo, "support-app");
        assert_eq!(ok.declared_deps, Some(vec!["source:s1".to_string()]));
    }
}

#[cfg(test)]
mod r1_tests {
    use super::*;
    use std::collections::BTreeMap as Map;
    use std::sync::Arc;
    use wicked_estate_memory_core::{Facets, ProposalState};
    use wicked_estate_overlay::XedgeStore;
    use wicked_estate_store::SqliteStore;

    fn engine() -> KnowledgeEngine {
        KnowledgeEngine::in_memory()
            .unwrap()
            .with_xedge_store(Arc::new(XedgeStore::in_memory().unwrap()))
    }

    fn concept(e: &mut KnowledgeEngine, text: &str) -> String {
        e.write(&KNode::new(KClass::Concept, text, "", "t.md", 1))
            .unwrap()
            .0
    }

    /// C8: a proposal filed with a repo facet (crew's relay stamps the unit's own repo) may link
    /// only that repo's code; another repo is refused at approve (and at submit, in the MCP).
    #[test]
    fn a_repo_facet_binds_every_code_endpoint() {
        let mut e = engine();
        let facets = Facets::try_from_map([("repo", "support-app")]).unwrap();
        let mut p = Proposal {
            id: "p9".into(),
            kind_type: "knowledge".into(),
            payload: json!({
                "content": "a ticket is a Ticket", "source": "shapes", "class": "concept",
                "relate_code": [{"rel": "describes", "code_id": "rust::secret::Thing", "repo": "public-app"}],
            }),
            facets,
            provenance: Map::new(),
            state: ProposalState::Pending,
            created_at: 1,
        };
        let err = e
            .apply_proposal(&p, &HashMap::new(), 2)
            .unwrap_err()
            .to_string();
        assert!(err.contains("is not this proposal's repo"), "{err}");
        assert!(e.applying_proposals().unwrap().is_empty());
        p.payload["relate_code"][0]["repo"] = json!("support-app");
        e.apply_proposal(&p, &HashMap::new(), 2).unwrap();
    }

    /// C13: neighbors is bounded in nodes, edges and output, and says so.
    #[test]
    fn neighbors_is_bounded_and_says_so() {
        let mut e = engine();
        let hub = concept(&mut e, "hub concept");
        let xedge = Arc::clone(e.xedge.as_ref().unwrap());
        // The owner must be live and visible: one dep-free shape owns every edge.
        let owner = concept(&mut e, "the owning shape");
        for i in 0..150 {
            let leaf = concept(
                &mut e,
                &format!("leaf concept number {i:03} with some label text"),
            );
            xedge
                .put_typed(&TypedXEdge {
                    source: Endpoint::new(KNOWLEDGE_STORE, hub.clone(), 0),
                    target: Endpoint::new(KNOWLEDGE_STORE, leaf, 0),
                    rel: "joins".into(),
                    owner: owner.clone(),
                    confidence: 1.0,
                    meta: json!({"deps": [], "evidence": {"kind": "fk", "note": "x".repeat(200)}})
                        .to_string(),
                })
                .unwrap();
        }
        let graph = SqliteStore::in_memory().unwrap();
        let n = e
            .neighbors(
                &NeighborsQuery {
                    store: KNOWLEDGE_STORE.into(),
                    id: hub,
                    rels: vec![],
                    depth: 1,
                    visible: Visible::none(),
                },
                &graph,
            )
            .unwrap()
            .unwrap();
        assert!(n.truncated, "a bound bit");
        assert!(n.nodes.len() <= MAX_NEIGHBOR_NODES);
        assert!(n.edges.len() <= MAX_NEIGHBOR_EDGES);
        let wire = json!({
            "nodes": n.nodes.iter().map(|x| json!({"store": x.store, "id": x.id, "class": x.class,
                "label": x.label, "scope": x.scope, "deps": x.deps, "untrusted": x.untrusted,
                "source": x.source, "hydrated": x.hydrated})).collect::<Vec<_>>(),
            "edges": n.edges.iter().map(|x| json!({"rel": x.rel, "from": {"store": x.from.0, "id": x.from.1},
                "to": {"store": x.to.0, "id": x.to.1}, "owner": x.owner, "confidence": x.confidence,
                "meta": x.meta})).collect::<Vec<_>>(),
            "truncated": n.truncated,
        })
        .to_string();
        assert!(
            wire.len() < 25_000,
            "R4: the answer is {} chars",
            wire.len()
        );
    }

    /// C7: a code seed is echoed back as its id only, never hydrated from the graph.
    #[test]
    fn a_code_seed_is_echoed_not_hydrated() {
        use wicked_estate_core::{GraphWrite, Language, Location, Span};
        let e = engine();
        let mut graph = SqliteStore::in_memory().unwrap();
        let sym = SymbolId("rust . . . app/Secret#".into());
        graph
            .upsert_nodes(&[Node::new(
                sym.clone(),
                NodeKind::Function,
                "secret_fn",
                Language::new("rust"),
                Location::new("src/secret.rs", Span::ZERO),
            )])
            .unwrap();
        let n = e
            .neighbors(
                &NeighborsQuery {
                    store: ESTATE_STORE.into(),
                    id: sym.0.clone(),
                    rels: vec![],
                    depth: 1,
                    visible: Visible::none(),
                },
                &graph,
            )
            .unwrap()
            .unwrap();
        assert_eq!(n.nodes.len(), 1);
        assert!(!n.nodes[0].hydrated);
        assert!(n.nodes[0].label.is_empty() && n.nodes[0].source.is_empty());
    }
}
