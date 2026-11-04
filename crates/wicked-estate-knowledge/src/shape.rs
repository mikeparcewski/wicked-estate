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
//! 1. a `kapply` journal records the full plan (the node and every edge row) and its attempt id;
//! 2. the typed edges are written, owned by the node's id;
//! 3. the knowledge node is written (node and vector in one transaction);
//! 4. the caller marks the proposal approved;
//! 5. the journal is deleted ([`KnowledgeEngine::finish_proposal`]): **the flip to visible**.
//!
//! Every read path hides a proposal-derived node while its journal exists, and keeps an edge only
//! when its owner is a live, complete node, so nothing of steps 1-4 is visible, even to a
//! `--readonly` server that never sweeps. Ids derive from the proposal id, so every step is an
//! idempotent upsert. A start-up sweep reads every journal: an approved proposal is completed
//! ([`KnowledgeEngine::complete_proposal`]); anything else is rolled back once its journal is past
//! the grace period ([`KnowledgeEngine::rollback_proposal`]: node, then edges, then the journal
//! last), and only the attempt the sweep inspected.
//!
//! [`KnowledgeEngine::neighbors`] walks the typed edges, filtered by the reader's `visible` set,
//! including the links a source's documents own (DES-PM-001 §3.4, [`crate::links`]): a link is kept
//! only while its owner document is live at the link's generation, and its `control` end (a crew
//! project, repo, PR or run) only when the end's own deps are visible.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{Value, json};
use wicked_estate_core::{
    GraphRead, GraphWrite, Language, Location, Node, NodeKind, Span, Symbol, SymbolId, SymbolQuery,
};
use wicked_estate_memory_core::Proposal;
use wicked_estate_overlay::{Endpoint, TYPED_RELS, TypedXEdge, is_typed_rel};

use crate::engine::{KClass, KNode, KnowledgeEngine};
use crate::links::parse_link_owner;
use crate::plane::{
    RUN_KIND, Visible, control_deps, effective_deps, is_untrusted, normalize_deps, scope_deps,
    valid_dep, valid_repo_id,
};

/// The approval journal's node kind (never a knowledge kind, so never read as knowledge).
pub const APPLY_JOURNAL_KIND: &str = "kapply";
/// The store names typed edges use for their endpoints.
pub const KNOWLEDGE_STORE: &str = "knowledge";
pub const ESTATE_STORE: &str = "estate";
/// Crew's control-plane endpoints (DES-PM-001 §3.4): `project:<id>`, `repo:<id>`,
/// `pr:<repo id>#<n>`, `run:<id>`. Never hydrated and never epoch-checked; their deps are derived
/// ([`crate::plane::control_deps`]).
pub const CONTROL_STORE: &str = "control";

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
pub(crate) const ADJACENCY_LIMIT: usize = 5_000;
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

/// One approval journal: the proposal, the attempt that wrote it, and when (unix seconds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyJournal {
    pub proposal_id: String,
    pub attempt: String,
    pub written_at: i64,
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

    /// May `node` be the knowledge end of an `about` link (`knowledge.relate_code`)? It must be a
    /// live, complete knowledge node, and not a source's catalog structure: those are tombstoned and
    /// re-added by `replace_source` and link to code only through a data-shape proposal's typed,
    /// epoch-stamped edges (an `about` row carries no knowledge epoch, so a re-added key would
    /// inherit it).
    pub fn check_about_link_target(&self, node: &SymbolId) -> anyhow::Result<()> {
        let Some((_, kn)) = self.live_knowledge(&node.0)? else {
            anyhow::bail!("no live knowledge node {}", node.0);
        };
        if !scope_deps(&kn.scope).is_empty() {
            anyhow::bail!(
                "{} is a source's catalog structure: link it to code through a data-shape proposal \
                 (relate_code[]), not knowledge.relate_code",
                node.0
            );
        }
        Ok(())
    }

    /// A live, complete knowledge node by its full id (`kchunk synthetic …:` as recall returns
    /// it). An approval in flight is not live.
    pub(crate) fn live_knowledge(
        &self,
        id: &str,
    ) -> wicked_estate_core::Result<Option<(Node, KNode)>> {
        let sym = SymbolId(id.to_string());
        if self.applying(&sym) {
            return Ok(None);
        }
        Ok(self
            .store
            .get_node(&sym)?
            .and_then(|n| KNode::from_node(&n).map(|kn| (n, kn))))
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
                let Some((node, _)) = self.live_knowledge(id)? else {
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
        if deps.len() > crate::plane::MAX_DEPS {
            anyhow::bail!(
                "knowledge proposal {}: it connects {} sources and repos; a shape may connect at most {}",
                proposal.id,
                deps.len(),
                crate::plane::MAX_DEPS
            );
        }
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
        crate::plane::stamp(&mut node, &deps, false);
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
        // Each attempt is its own: a rollback names the attempt it inspected, so it can never undo
        // a fresh retry that overwrote the journal in between.
        let attempt = uuid::Uuid::now_v7().to_string();
        m.insert("attempt".into(), attempt.clone().into());
        self.store.upsert_nodes(std::slice::from_ref(&journal))?;
        self.attempts.insert(proposal.id.clone(), attempt);
        // 2-3. Edges (hidden: their owner is not live yet), then the node.
        self.put_plan(&plan)?;
        Ok((plan.node.symbol.0.clone(), plan))
    }

    /// Step 5: the approval is complete; drop its journal.
    pub fn finish_proposal(&mut self, proposal_id: &str) -> anyhow::Result<()> {
        self.store.remove_nodes(&[journal_symbol(proposal_id)])?;
        Ok(())
    }

    /// This engine's latest approval attempt for `proposal_id` (what its own failure path rolls back).
    pub fn own_attempt(&self, proposal_id: &str) -> Option<String> {
        self.attempts.get(proposal_id).cloned()
    }

    /// Roll back a half-done approval — its node, its typed edges, then its journal — but only the
    /// `attempt` the caller inspected: `false` when there is no journal or a newer attempt has
    /// overwritten it (an approved node is never undone; a fresh retry is never undone).
    ///
    /// The check and the knowledge-store deletes run in one transaction: if another process commits
    /// a new attempt after the check, this transaction's first write fails (a stale WAL snapshot)
    /// and nothing is rolled back. The node goes first (hidden), the journal last, so a rollback
    /// that fails part way leaves the journal and the next sweep finishes it.
    pub fn rollback_proposal(&mut self, proposal_id: &str, attempt: &str) -> anyhow::Result<bool> {
        self.store.begin_batch()?;
        match self.rollback_in_batch(proposal_id, attempt) {
            Ok(true) => match self.store.commit_batch() {
                Ok(()) => {
                    self.attempts.remove(proposal_id);
                    Ok(true)
                }
                Err(e) => {
                    self.store.rollback_batch()?;
                    Err(e.into())
                }
            },
            Ok(false) => {
                self.store.rollback_batch()?;
                Ok(false)
            }
            Err(e) => {
                self.store.rollback_batch()?;
                Err(e)
            }
        }
    }

    fn rollback_in_batch(&mut self, proposal_id: &str, attempt: &str) -> anyhow::Result<bool> {
        let Some(journal) = self.store.get_node(&journal_symbol(proposal_id))? else {
            return Ok(false);
        };
        let m = &journal.metadata;
        if m.get("attempt").and_then(Value::as_str) != Some(attempt) {
            return Ok(false);
        }
        let owner = m
            .get("owner")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                Symbol::synthetic(KClass::Chunk.as_kind(), format!("proposal:{proposal_id}"))
                    .id()
                    .0
            });
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

    /// The approvals with a journal (in flight, or interrupted).
    pub fn applying_proposals(&self) -> anyhow::Result<Vec<ApplyJournal>> {
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
                let m = &n.metadata;
                Some(ApplyJournal {
                    proposal_id: m.get("proposal_id").and_then(Value::as_str)?.to_string(),
                    attempt: m
                        .get("attempt")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    written_at: m.get("created_at").and_then(Value::as_i64).unwrap_or(0),
                })
            })
            .collect())
    }
}

// ── knowledge.neighbors ─────────────────────────────────────────────────────

/// A `knowledge.neighbors` request.
#[derive(Debug, Clone)]
pub struct NeighborsQuery {
    /// `"knowledge"` (a knowledge node id), `"estate"` (a code symbol id) or `"control"` (a crew
    /// project, repo, PR or run).
    pub store: String,
    pub id: String,
    /// Typed rels to follow (empty = all of them).
    pub rels: Vec<String>,
    pub depth: usize,
    pub visible: Visible,
}

/// One node of a neighbourhood (its wire form is [`NeighborNode::to_json`]).
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

/// One edge of a neighbourhood (its wire form is [`NeighborEdge::to_json`]).
#[derive(Debug, Clone, PartialEq)]
pub struct NeighborEdge {
    pub rel: String,
    pub from: (String, String),
    pub to: (String, String),
    pub owner: String,
    pub confidence: f32,
    pub meta: Value,
}

impl NeighborNode {
    /// The wire record (what `knowledge.neighbors` returns, and what its output budget measures).
    pub fn to_json(&self) -> Value {
        json!({
            "store": self.store, "id": self.id, "class": self.class, "label": self.label,
            "scope": self.scope, "deps": self.deps, "untrusted": self.untrusted,
            "source": self.source, "hydrated": self.hydrated,
        })
    }
}

impl NeighborEdge {
    /// The wire record (what `knowledge.neighbors` returns, and what its output budget measures).
    pub fn to_json(&self) -> Value {
        json!({
            "rel": self.rel,
            "from": {"store": self.from.0, "id": self.from.1},
            "to": {"store": self.to.0, "id": self.to.1},
            "owner": self.owner, "confidence": self.confidence, "meta": self.meta,
        })
    }
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

/// How often `neighbors` reads again when a writer committed during the walk (review r2).
const NEIGHBOR_ATTEMPTS: usize = 3;
/// At most this many adjacency rows are read to resolve runs' filings in one walk, including runs
/// that end up hidden (review r2); a run past the budget is hidden and the answer `truncated`.
pub(crate) const RUN_RESOLVE_ROWS: usize = 20_000;

/// One walk's observations (review r1): each link document's generation as first seen, and each
/// run's resolved deps, so every edge and endpoint of one answer is judged against the same view.
struct WalkPins {
    doc_gens: HashMap<String, Option<u64>>,
    runs: HashMap<String, Option<Vec<String>>>,
    run_rows_left: usize,
    run_budget_hit: bool,
}

impl WalkPins {
    fn new() -> Self {
        Self {
            doc_gens: HashMap::new(),
            runs: HashMap::new(),
            run_rows_left: RUN_RESOLVE_ROWS,
            run_budget_hit: false,
        }
    }
}
/// A neighbourhood node's citation is cut to this many characters (an approved node's `source` is
/// free text), so one node can never take the output budget.
const SOURCE_CHARS: usize = 300;

fn cut(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n).collect();
        out.push('…');
        out
    }
}

impl KnowledgeEngine {
    /// A visible knowledge node as a neighbourhood node, else `None`.
    fn knowledge_neighbor(
        &self,
        id: &str,
        visible: &Visible,
    ) -> wicked_estate_core::Result<Option<NeighborNode>> {
        let Some((node, kn)) = self.live_knowledge(id)? else {
            return Ok(None);
        };
        let deps = effective_deps(&node);
        if !visible.allows(&deps) {
            return Ok(None);
        }
        Ok(Some(NeighborNode {
            store: KNOWLEDGE_STORE.into(),
            id: id.to_string(),
            class: Some(kn.class.as_kind().to_string()),
            label: kn.content.chars().take(LABEL_CHARS).collect(),
            scope: kn.scope.clone(),
            untrusted: is_untrusted(&node),
            deps,
            source: cut(&kn.source, SOURCE_CHARS),
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
                source: cut(&n.location.file, SOURCE_CHARS),
                hydrated: true,
            },
            None => NeighborNode {
                store: ESTATE_STORE.into(),
                id: id.to_string(),
                class: None,
                label: cut(id, LABEL_CHARS),
                scope: String::new(),
                deps,
                untrusted: false,
                source: String::new(),
                hydrated: false,
            },
        }
    }

    /// A document's live generation as THIS walk first observed it (`None`: not live). Every link
    /// of that document is judged against the one observation, so an answer read while the
    /// document's next generation commits never holds links of two generations (review r1).
    fn pinned_generation(&self, doc: &str, pins: &mut WalkPins) -> anyhow::Result<Option<u64>> {
        if let Some(g) = pins.doc_gens.get(doc) {
            return Ok(*g);
        }
        let g = self.live_knowledge(doc)?.and_then(|(node, _)| {
            node.metadata
                .get(crate::source::meta::GENERATION)
                .and_then(Value::as_u64)
        });
        pins.doc_gens.insert(doc.to_string(), g);
        Ok(g)
    }

    /// Is `owner` a link owner whose document is live AT the owner's generation (as pinned)?
    fn link_live(&self, owner: &str, pins: &mut WalkPins) -> anyhow::Result<bool> {
        let Some((doc, generation)) = parse_link_owner(owner) else {
            return Ok(false);
        };
        Ok(self.pinned_generation(&doc.0, pins)? == Some(generation))
    }

    /// A run's deps: the union of the filings its live links record (the narrowest set), the same
    /// for a start and for a far end, resolved once per walk. `None` (hidden) when it has no live
    /// link, when a live link's filing cannot be read, or when its adjacency is over the read bound
    /// (an incomplete union is never an authorisation).
    fn run_deps(
        &self,
        id: &str,
        reader: &wicked_estate_overlay::XedgeReader,
        pins: &mut WalkPins,
    ) -> anyhow::Result<Option<Vec<String>>> {
        if let Some(d) = pins.runs.get(id) {
            return Ok(d.clone());
        }
        if pins.run_rows_left == 0 {
            pins.run_budget_hit = true;
            return Ok(None);
        }
        let resolved = (|| -> anyhow::Result<Option<Vec<String>>> {
            let out = reader.typed_out(CONTROL_STORE, id, &[], ADJACENCY_LIMIT)?;
            let inn = reader.typed_in(CONTROL_STORE, id, &[], ADJACENCY_LIMIT)?;
            self.walk_hook("run_rows");
            if out.len() >= ADJACENCY_LIMIT || inn.len() >= ADJACENCY_LIMIT {
                return Ok(None);
            }
            let read = out.len() + inn.len();
            if read > pins.run_rows_left {
                pins.run_rows_left = 0;
                pins.run_budget_hit = true;
                return Ok(None);
            }
            pins.run_rows_left -= read;
            let mut union: BTreeSet<String> = BTreeSet::new();
            let mut live = false;
            for e in out.into_iter().chain(inn) {
                if !self.link_live(&e.owner, pins)? {
                    continue;
                }
                let meta: Value = serde_json::from_str(&e.meta).unwrap_or_else(|_| json!({}));
                let given: Option<Vec<String>> = meta
                    .get("run_deps")
                    .and_then(Value::as_array)
                    .and_then(|xs| xs.iter().map(|x| x.as_str().map(str::to_string)).collect());
                let Some(deps) = given.and_then(|g| control_deps(id, Some(&g)).ok()) else {
                    return Ok(None); // a live link whose filing cannot be read: fail closed
                };
                live = true;
                union.extend(deps);
            }
            // A run filed under more projects and repos than an item may carry is hidden: its
            // deps are never cut (they authorise), and the record stays inside R4.
            if union.len() > crate::plane::MAX_DEPS {
                return Ok(None);
            }
            Ok(live.then(|| union.into_iter().collect()))
        })()?;
        pins.runs.insert(id.to_string(), resolved.clone());
        Ok(resolved)
    }

    /// A `control` endpoint as a neighbourhood node, or `None` when it is malformed or (a run) has
    /// no resolvable filing. Its deps are derived from the id, or for a run by [`Self::run_deps`];
    /// never from one row's `meta`.
    fn control_node(
        &self,
        id: &str,
        reader: &wicked_estate_overlay::XedgeReader,
        pins: &mut WalkPins,
    ) -> anyhow::Result<Option<NeighborNode>> {
        let Some((kind, _)) = id.split_once(':') else {
            return Ok(None);
        };
        let deps = if kind == RUN_KIND {
            match self.run_deps(id, reader, pins)? {
                Some(d) => d,
                None => return Ok(None),
            }
        } else {
            match control_deps(id, None) {
                Ok(d) => d,
                Err(_) => return Ok(None),
            }
        };
        Ok(Some(NeighborNode {
            store: CONTROL_STORE.into(),
            id: id.to_string(),
            class: Some(kind.to_string()),
            label: cut(id, LABEL_CHARS),
            scope: String::new(),
            deps,
            untrusted: false,
            source: String::new(),
            hydrated: false,
        }))
    }

    /// May the reader keep an edge `owner` asserted? A link owner (`<doc>@<generation>`) needs its
    /// document live, visible, and AT that generation (as this walk pinned it); any other owner (a
    /// data shape) must be a live, visible knowledge node.
    fn owner_visible(
        &self,
        owner: &str,
        visible: &Visible,
        pins: &mut WalkPins,
    ) -> anyhow::Result<bool> {
        match parse_link_owner(owner) {
            Some((doc, _)) => {
                Ok(self.link_live(owner, pins)?
                    && self.knowledge_neighbor(&doc.0, visible)?.is_some())
            }
            None => Ok(self.knowledge_neighbor(owner, visible)?.is_some()),
        }
    }

    /// Is the endpoint's stamped epoch its live one? An owner endpoint (a shape's node, or the
    /// document that owns a link) is checked by the owner's liveness instead (its own epoch is not
    /// known before the flip). A `control` endpoint has no epoch in estate.
    fn epoch_ok(&self, ep: &Endpoint, owner: &str, graph: &dyn GraphRead) -> bool {
        if ep.engine == CONTROL_STORE {
            return true;
        }
        if ep.engine == KNOWLEDGE_STORE
            && (ep.stable_id == owner
                || parse_link_owner(owner).is_some_and(|(doc, _)| doc.0 == ep.stable_id))
        {
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
        // A walk reads rows and documents at different moments. If another connection commits to
        // either store meanwhile (crew's projector flipping a tracker's generation), the answer
        // could mix two states, so it is read again (review r2); a walk that never settles is
        // refused rather than returned mixed.
        for _ in 0..NEIGHBOR_ATTEMPTS {
            let before = (self.store.data_version()?, reader.data_version()?);
            let answer = self.neighbors_once(q, graph, &reader)?;
            if (self.store.data_version()?, reader.data_version()?) == before {
                return Ok(answer);
            }
        }
        anyhow::bail!(
            "the knowledge changed while it was being read, {NEIGHBOR_ATTEMPTS} times: read it again"
        )
    }

    /// One attempt of [`Self::neighbors`].
    fn neighbors_once(
        &self,
        q: &NeighborsQuery,
        graph: &dyn GraphRead,
        reader: &wicked_estate_overlay::XedgeReader,
    ) -> anyhow::Result<Option<Neighbors>> {
        let mut pins = WalkPins::new();
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
            CONTROL_STORE => match self.control_node(&q.id, reader, &mut pins)? {
                Some(n) if q.visible.allows(&n.deps) => n,
                _ => return Ok(None), // malformed, hidden, or a run with no live link: unknown
            },
            other => {
                anyhow::bail!("unknown store {other:?}: expected knowledge, estate or control")
            }
        };
        let rels: Vec<&str> = q.rels.iter().map(String::as_str).collect();
        let depth = q.depth.clamp(1, MAX_NEIGHBOR_DEPTH);

        // The budget measures the exact wire records (escaping included), plus a separator each.
        let node_bytes = |n: &NeighborNode| n.to_json().to_string().len() + 1;
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
                            let ok = self.owner_visible(&e.owner, &q.visible, &mut pins)?;
                            self.walk_hook("owner");
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
                        } else if other.0 == CONTROL_STORE {
                            // The far end's own deps (from its id, or a run's resolved filing),
                            // never from this row's `deps` alone.
                            self.control_node(&other.1, reader, &mut pins)?
                                .filter(|n| q.visible.allows(&n.deps))
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
                    let edge = NeighborEdge {
                        rel: e.rel,
                        from: src,
                        to: tgt,
                        owner: e.owner,
                        confidence: e.confidence,
                        meta,
                    };
                    let cost = edge.to_json().to_string().len() + 1;
                    if edges.len() >= MAX_NEIGHBOR_EDGES || used + cost > NEIGHBOR_OUTPUT_BYTES {
                        truncated = true;
                        break 'walk;
                    }
                    used += cost;
                    edge_keys.insert(key);
                    edges.push(edge);
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
            truncated: truncated || pins.run_budget_hit,
        }))
    }

    /// Test hook: lets a test commit from another connection at a named point of a walk.
    #[cfg(test)]
    fn walk_hook(&self, point: &str) {
        if let Some(h) = self.walk_hook.borrow_mut().as_mut() {
            h(point);
        }
    }

    #[cfg(not(test))]
    fn walk_hook(&self, _point: &str) {}
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
        let (node, _) = e
            .live_knowledge(&id)
            .unwrap()
            .expect("visible after approve");
        assert_eq!(crate::plane::stored_deps(&node), plan.deps);
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
        let journals = e.applying_proposals().unwrap();
        assert_eq!(journals.len(), 1);
        assert_eq!(
            (journals[0].proposal_id.as_str(), journals[0].written_at),
            ("p1", 2)
        );
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
        assert!(
            e.rollback_proposal("p1", &e.own_attempt("p1").unwrap())
                .unwrap()
        );
        assert!(e.applying_proposals().unwrap().is_empty());
        assert_eq!(e.xedge.as_ref().unwrap().typed_len().unwrap(), 0);
        assert!(
            !e.rollback_proposal("p1", "any").unwrap(),
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
        assert!(
            e.rollback_proposal("p1", &e.own_attempt("p1").unwrap())
                .unwrap()
        );
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
        assert!(!e.rollback_proposal("p1", "any").unwrap());
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

#[cfg(test)]
mod r2_tests {
    use super::*;
    use std::collections::BTreeMap as Map;
    use std::sync::Arc;
    use wicked_estate_memory_core::{Facets, ProposalState};
    use wicked_estate_overlay::XedgeStore;

    /// r2 C2: a rollback names the attempt it inspected; a retry that overwrote the journal in
    /// between is never undone (the sweep-versus-retry interleaving, made deterministic).
    #[test]
    fn a_stale_attempt_never_rolls_back_a_fresh_retry() {
        let mut e = KnowledgeEngine::in_memory()
            .unwrap()
            .with_xedge_store(Arc::new(XedgeStore::in_memory().unwrap()));
        let target = e
            .write(&KNode::new(
                KClass::Concept,
                "a target concept",
                "",
                "t.md",
                1,
            ))
            .unwrap()
            .0;
        let p = Proposal {
            id: "p7".into(),
            kind_type: "knowledge".into(),
            payload: json!({"content": "a shape", "source": "s", "class": "concept",
                "relate": [{"rel": "derived_from", "to": target}]}),
            facets: Facets::default(),
            provenance: Map::new(),
            state: ProposalState::Pending,
            created_at: 1,
        };
        // Attempt A crashes between its edges and its node; a sweeper inspects it.
        e.fail_apply_before_node = true;
        assert!(e.apply_proposal(&p, &HashMap::new(), 1).is_err());
        e.fail_apply_before_node = false;
        let seen = e.applying_proposals().unwrap().remove(0);
        // Attempt B (a retry) overwrites the journal and lands the node and edges.
        e.apply_proposal(&p, &HashMap::new(), 500).unwrap();
        // The sweeper now acts on what it saw: attempt A. Nothing of B is touched.
        assert!(!e.rollback_proposal("p7", &seen.attempt).unwrap());
        let node = Symbol::synthetic("kconcept", "proposal:p7").id();
        assert!(e.store.get_node(&node).unwrap().is_some());
        assert_eq!(e.xedge.as_ref().unwrap().typed_len().unwrap(), 1);
        assert_eq!(
            e.applying_proposals().unwrap().len(),
            1,
            "B's journal stays"
        );
    }

    /// r2 C8: a shape connecting more sources and repos than an item can carry is refused.
    #[test]
    fn a_shape_with_too_many_deps_is_refused() {
        let mut e = KnowledgeEngine::in_memory()
            .unwrap()
            .with_xedge_store(Arc::new(XedgeStore::in_memory().unwrap()));
        let links: Vec<Value> = (0..40)
            .map(|i| json!({"rel": "describes", "code_id": format!("c{i}"), "repo": format!("r{i}")}))
            .collect();
        let p = Proposal {
            id: "p8".into(),
            kind_type: "knowledge".into(),
            payload: json!({"content": "wide", "source": "s", "relate_code": links}),
            facets: Facets::default(),
            provenance: Map::new(),
            state: ProposalState::Pending,
            created_at: 1,
        };
        let err = e
            .apply_proposal(&p, &HashMap::new(), 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("at most 32"), "{err}");
    }

    /// r2 C1/C5: a source's catalog structure, and an approval in flight, cannot take an `about`
    /// link (it would carry no knowledge epoch, or surface a half-done approval).
    #[test]
    fn about_links_refuse_catalog_structure_and_approvals_in_flight() {
        use crate::source::{SourceDoc, SourceGeneration, SourceSection, doc_symbol};
        let mut e = KnowledgeEngine::in_memory()
            .unwrap()
            .with_xedge_store(Arc::new(XedgeStore::in_memory().unwrap()));
        e.replace_source(
            &SourceGeneration {
                source_id: "s1".into(),
                generation: 1,
                label: None,
                catalog: None,
                docs: vec![SourceDoc {
                    key: "t".into(),
                    title: None,
                    entry: None,
                    sections: vec![SourceSection {
                        key: "c".into(),
                        text: "id int8".into(),
                    }],
                }],
            },
            1,
        )
        .unwrap();
        let err = e
            .check_about_link_target(&doc_symbol("s1", "t"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("catalog structure"), "{err}");
        let p = Proposal {
            id: "p6".into(),
            kind_type: "knowledge".into(),
            payload: json!({"content": "x", "source": "s"}),
            facets: Facets::default(),
            provenance: Map::new(),
            state: ProposalState::Pending,
            created_at: 1,
        };
        e.apply_proposal(&p, &HashMap::new(), 1).unwrap(); // node written, journal kept
        let sym = Symbol::synthetic("kchunk", "proposal:p6").id();
        assert!(e.check_about_link_target(&sym).is_err(), "in flight");
        e.finish_proposal("p6").unwrap();
        e.check_about_link_target(&sym).unwrap();
    }
}

#[cfg(test)]
mod pm_r1_tests {
    use super::*;
    use crate::links::{DocLink, LinkDir, LinkEnd, link_owner};
    use crate::source::{SourceDoc, SourceGeneration, SourceSection, doc_symbol};
    use std::sync::Arc;
    use wicked_estate_overlay::XedgeStore;
    use wicked_estate_store::SqliteStore;

    fn generation(n: u64) -> SourceGeneration {
        SourceGeneration {
            source_id: "trk_A".into(),
            generation: n,
            label: None,
            catalog: None,
            docs: vec![SourceDoc {
                key: "tracker".into(),
                title: None,
                entry: None,
                sections: vec![SourceSection {
                    key: "workflow".into(),
                    text: "PROJA workflow".into(),
                }],
            }],
        }
    }

    fn to(id: &str) -> BTreeMap<String, Vec<DocLink>> {
        BTreeMap::from([(
            "tracker".to_string(),
            vec![DocLink {
                rel: "tracked_in".into(),
                dir: LinkDir::Out,
                end: LinkEnd {
                    store: CONTROL_STORE.into(),
                    id: id.into(),
                },
                deps: None,
            }],
        )])
    }

    /// r1 (codex 3): one walk judges every link of a document against the generation it first
    /// observed, so an answer read across the flip never mixes two generations' links.
    /// NEGATIVE CONTROL: the pin not kept (each owner re-reads the live generation).
    #[test]
    fn a_walk_pins_each_documents_generation() {
        let mut e = KnowledgeEngine::in_memory()
            .unwrap()
            .with_xedge_store(Arc::new(XedgeStore::in_memory().unwrap()));
        e.replace_source_linked(&generation(1), &to("project:proj_P"), 1)
            .unwrap();
        let doc = doc_symbol("trk_A", "tracker");
        let v = Visible::from_deps(["source:trk_A", "project:proj_P", "project:proj_Q"]).unwrap();
        let mut pins = WalkPins::new();
        assert!(
            e.owner_visible(&link_owner(&doc, 1), &v, &mut pins)
                .unwrap()
        );
        // The next generation commits while the walk is in flight (its cleanup not yet run).
        e.fail_links_after_flip = true;
        assert!(
            e.replace_source_linked(&generation(2), &to("project:proj_Q"), 2)
                .is_err()
        );
        e.fail_links_after_flip = false;
        assert!(
            !e.owner_visible(&link_owner(&doc, 2), &v, &mut pins)
                .unwrap(),
            "the same walk keeps judging by generation 1"
        );
        assert!(
            e.owner_visible(&link_owner(&doc, 1), &v, &mut pins)
                .unwrap()
        );
        // A fresh walk sees generation 2 only.
        let mut fresh = WalkPins::new();
        assert!(
            e.owner_visible(&link_owner(&doc, 2), &v, &mut fresh)
                .unwrap()
        );
        assert!(
            !e.owner_visible(&link_owner(&doc, 1), &v, &mut fresh)
                .unwrap()
        );
    }

    /// Two engines on one pair of files: a writer and a reader, as crew's projector and a
    /// per-call reader process are.
    fn on_disk() -> (tempfile::TempDir, impl Fn() -> KnowledgeEngine) {
        let dir = tempfile::tempdir().unwrap();
        let k = dir
            .path()
            .join("knowledge.db")
            .to_string_lossy()
            .to_string();
        let x = dir.path().join("xedge.db").to_string_lossy().to_string();
        let open = move || {
            KnowledgeEngine::open(&k)
                .unwrap()
                .with_xedge_store(Arc::new(XedgeStore::open(&x).unwrap()))
        };
        (dir, open)
    }

    fn two_docs(sid: &str, n: u64, docs: &[&str]) -> SourceGeneration {
        SourceGeneration {
            source_id: sid.into(),
            generation: n,
            label: None,
            catalog: None,
            docs: docs
                .iter()
                .map(|k| SourceDoc {
                    key: (*k).into(),
                    title: None,
                    entry: None,
                    sections: vec![SourceSection {
                        key: "s".into(),
                        text: format!("{k} text"),
                    }],
                })
                .collect(),
        }
    }

    fn links(xs: &[(&str, &str, &str, Option<&[&str]>)]) -> BTreeMap<String, Vec<DocLink>> {
        let mut out: BTreeMap<String, Vec<DocLink>> = BTreeMap::new();
        for (key, rel, end, deps) in xs {
            out.entry((*key).to_string()).or_default().push(DocLink {
                rel: (*rel).into(),
                dir: if *rel == "tracked_in" {
                    LinkDir::Out
                } else {
                    LinkDir::In
                },
                end: LinkEnd {
                    store: CONTROL_STORE.into(),
                    id: (*end).into(),
                },
                deps: deps.map(|d| d.iter().map(|x| x.to_string()).collect()),
            });
        }
        out
    }

    fn q(store: &str, id: &str, visible: &[&str]) -> NeighborsQuery {
        NeighborsQuery {
            store: store.into(),
            id: id.into(),
            rels: vec![],
            depth: 1,
            visible: Visible::from_deps(visible).unwrap(),
        }
    }

    /// r2 (codex 1): generation 2 moves r's link from document a to document b. A reader that
    /// judged a@1 before the flip and b@2 after it would hold both; the walk is read again when a
    /// writer committed meanwhile, so the answer is generation 2's alone.
    /// NEGATIVE CONTROL: the answer returned without the change check.
    #[test]
    fn r2_a_walk_across_a_flip_is_read_again_never_returned_mixed() {
        let (_dir, open) = on_disk();
        let w = std::rc::Rc::new(std::cell::RefCell::new(open()));
        let g = |n| two_docs("trk_A", n, &["a", "b"]);
        w.borrow_mut()
            .replace_source_linked(&g(1), &links(&[("a", "tracked_in", "repo:r", None)]), 1)
            .unwrap();
        // Generation 2's rows are staged (a crash before its flip), then it lands mid-walk.
        w.borrow_mut().fail_links_before_flip = true;
        let to_b = links(&[("b", "tracked_in", "repo:r", None)]);
        assert!(
            w.borrow_mut()
                .replace_source_linked(&g(2), &to_b, 2)
                .is_err()
        );
        w.borrow_mut().fail_links_before_flip = false;
        let r = open();
        let (writer, mut fired) = (std::rc::Rc::clone(&w), false);
        *r.walk_hook.borrow_mut() = Some(Box::new(move |point: &str| {
            if point == "owner" && !fired {
                fired = true;
                writer
                    .borrow_mut()
                    .replace_source_linked(&g(2), &to_b, 3)
                    .unwrap();
            }
        }));
        let n = r
            .neighbors(
                &q(CONTROL_STORE, "repo:r", &["source:trk_A", "repo:r"]),
                &SqliteStore::in_memory().unwrap(),
            )
            .unwrap()
            .unwrap();
        let docs: Vec<&str> = n
            .nodes
            .iter()
            .filter(|x| x.store == KNOWLEDGE_STORE)
            .map(|x| x.id.as_str())
            .collect();
        assert_eq!(docs, vec![doc_symbol("trk_A", "b").0.as_str()], "{n:?}");
    }

    /// r2 (codex 2): a run's filings are read across a flip of one of its trackers. Tracker A files
    /// run:x under project P, tracker B under repo r; a reader holding B and r (not P) must not see
    /// the run, even when A re-commits the same link while the run is being resolved.
    /// NEGATIVE CONTROL: the answer returned without the change check.
    #[test]
    fn r2_a_run_resolved_across_a_flip_never_drops_a_live_filing() {
        let (_dir, open) = on_disk();
        let w = std::rc::Rc::new(std::cell::RefCell::new(open()));
        let a = |n| two_docs("trk_A", n, &["PROJA-1"]);
        let a_links = links(&[("PROJA-1", "addresses", "run:x", Some(&["project:proj_P"]))]);
        w.borrow_mut()
            .replace_source_linked(&a(1), &a_links, 1)
            .unwrap();
        w.borrow_mut()
            .replace_source_linked(
                &two_docs("trk_B", 1, &["PROJB-1"]),
                &links(&[("PROJB-1", "addresses", "run:x", Some(&["repo:r"]))]),
                1,
            )
            .unwrap();
        let r = open();
        let (writer, mut fired) = (std::rc::Rc::clone(&w), false);
        *r.walk_hook.borrow_mut() = Some(Box::new(move |point: &str| {
            if point == "run_rows" && !fired {
                fired = true;
                writer
                    .borrow_mut()
                    .replace_source_linked(&a(2), &a_links, 2)
                    .unwrap();
            }
        }));
        let graph = SqliteStore::in_memory().unwrap();
        assert!(
            r.neighbors(
                &q(CONTROL_STORE, "run:x", &["source:trk_B", "repo:r"]),
                &graph
            )
            .unwrap()
            .is_none(),
            "the run's live filing under P is never dropped"
        );
        assert!(
            r.neighbors(
                &q(
                    CONTROL_STORE,
                    "run:x",
                    &["source:trk_B", "repo:r", "project:proj_P"]
                ),
                &graph
            )
            .unwrap()
            .is_some()
        );
    }

    /// pi r2: a proposal can never name a control endpoint (relate[] resolves knowledge nodes only).
    #[test]
    fn a_proposal_cannot_name_a_control_endpoint() {
        use std::collections::BTreeMap as Map;
        use wicked_estate_memory_core::{Facets, ProposalState};
        let mut e = KnowledgeEngine::in_memory()
            .unwrap()
            .with_xedge_store(Arc::new(XedgeStore::in_memory().unwrap()));
        let p = Proposal {
            id: "p1".into(),
            kind_type: "knowledge".into(),
            payload: json!({"content": "a shape", "source": "s", "class": "concept",
                "relate": [{"rel": "tracked_in", "to": "project:proj_P"}]}),
            facets: Facets::default(),
            provenance: Map::new(),
            state: ProposalState::Pending,
            created_at: 1,
        };
        let err = e
            .apply_proposal(&p, &HashMap::new(), 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a live knowledge node"), "{err}");
        assert_eq!(e.xedge.as_ref().unwrap().typed_len().unwrap(), 0);
    }
}
