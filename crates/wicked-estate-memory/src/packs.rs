//! The verified governance sync's library-only write path. No CLI or MCP exposes this bypass.

use std::collections::BTreeSet;
use wicked_estate_core::{Error, Node, Result, SymbolId};
use wicked_estate_memory_core::{Facets, MemKind, MemLock, Memory, Scope, Tier, memory_lock};

use crate::MemoryEngine;

/// A pack whose authenticity the caller has already verified.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PackRef {
    pub id: String,
    pub layer: String,
    pub version: String,
}

impl PackRef {
    pub fn new(
        id: impl Into<String>,
        layer: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            layer: layer.into(),
            version: version.into(),
        }
    }
}

/// The hashed pack projection. `id` is local to the pack, without the `<pack>:` prefix.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct PackMemory {
    pub id: String,
    pub kind: MemKind,
    pub tier: Tier,
    pub content: String,
    pub facets: Facets,
}

impl PackMemory {
    pub fn new(
        id: impl Into<String>,
        kind: MemKind,
        tier: Tier,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            tier,
            content: content.into(),
            facets: Facets::default(),
        }
    }

    pub fn with_facets(mut self, facets: Facets) -> Self {
        self.facets = facets;
        self
    }
}

/// Counts for a single reconciliation, including removals absent from the new version.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PackApplyReport {
    pub written: usize,
    pub unchanged: usize,
    pub erased: usize,
}

impl PackApplyReport {
    pub fn new() -> Self {
        Self::default()
    }
}

fn projection_hash(memory: &PackMemory) -> Result<String> {
    // A change detector over the canonical JSON projection (struct field order, facets sorted),
    // stored on the row and compared on re-apply. FNV-1a 64 is written out here because it must
    // be stable across Rust releases (std's DefaultHasher is not); core verifies the pack itself.
    // Ownership/version, timestamps and memory bookkeeping are deliberately outside this hash.
    let value = serde_json::to_value(memory).map_err(|e| Error::Invalid(e.to_string()))?;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.to_string().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Ok(format!("{hash:016x}"))
}

fn owned_nodes(engine: &MemoryEngine, pack: &str) -> Result<Vec<Node>> {
    let mut nodes = Vec::new();
    for kind in [crate::MEMORY_NODE_KIND, crate::RETIRED_MEMORY_NODE_KIND] {
        for node in engine.memory_nodes(kind)? {
            let Some(lock) = memory_lock(&node) else {
                continue;
            };
            if lock.layer == "?" {
                let owner = node.metadata.get("lock").and_then(|v| v.get("pack"));
                let prefix = format!("mem synthetic {pack}:");
                if owner.and_then(|v| v.as_str()) == Some(pack)
                    || node.symbol.as_str().starts_with(&prefix)
                {
                    return Err(Error::Invalid(format!(
                        "memory_locked: {} has a malformed lock for pack {pack}",
                        node.symbol
                    )));
                }
            } else if lock.pack == pack {
                nodes.push(node);
            }
        }
    }
    Ok(nodes)
}

/// Read a pack's stored projection for verification, with local ids and stable id ordering.
/// An owned row that cannot be decoded is an error, never a silently incomplete verification.
pub fn pack_memories(engine: &MemoryEngine, pack: &str) -> Result<Vec<PackMemory>> {
    let prefix = format!("{pack}:");
    let mut memories = Vec::new();
    for node in owned_nodes(engine, pack)? {
        let memory = Memory::from_node(&node).ok_or_else(|| {
            Error::Invalid(format!("pack {pack}: undecodable memory {}", node.symbol))
        })?;
        let id = memory.id.strip_prefix(&prefix).ok_or_else(|| {
            Error::Invalid(format!("pack {pack}: memory {} lacks pack prefix", memory.id))
        })?;
        memories.push(
            PackMemory::new(id, memory.kind, memory.tier, memory.content)
                .with_facets(memory.facets),
        );
    }
    memories.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(memories)
}

/// Reconcile the complete set of memories of an already verified org/team pack.
/// Only this path may write locks or remove pack-owned memories. Invalid input and ownership
/// collisions are refused before any writes. Equal projection, version and layer are a no-op.
/// Changed rows receive the application time as their capture timestamp.
pub fn apply_pack_memories(
    engine: &mut MemoryEngine,
    pack: &PackRef,
    memories: &[PackMemory],
) -> Result<PackApplyReport> {
    if pack.id.trim().is_empty()
        || pack.id == "?"
        || pack.id.contains(['/', ':', '\0'])
        || pack.id.chars().any(char::is_whitespace)
        || !matches!(pack.layer.as_str(), "org" | "team")
        || pack.version.trim().is_empty()
    {
        return Err(Error::Invalid(
            "pack memories: require a non-empty single-segment pack id, org/team layer and version"
                .into(),
        ));
    }
    let scope = Scope::parse_strict(&format!("{}:{}", pack.layer, pack.id))
        .map_err(|e| Error::Invalid(e.to_string()))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| Error::Invalid(format!("pack memories: invalid system clock: {e}")))?
        .as_secs() as i64;
    let lock = MemLock::new(&pack.id, &pack.layer, &pack.version);
    let old = owned_nodes(engine, &pack.id)?;
    let mut wanted = BTreeSet::new();
    let mut writes = Vec::new();
    let mut report = PackApplyReport::new();
    for input in memories {
        if input.id.trim().is_empty()
            || input.id.ends_with(':')
            || input.id.contains('\0')
            || input.id.chars().any(char::is_whitespace)
        {
            return Err(Error::Invalid(
                "pack memory id must be non-empty, without whitespace/NUL or a trailing colon"
                    .into(),
            ));
        }
        if !matches!(input.tier, Tier::Semantic | Tier::Procedural) {
            return Err(Error::Invalid(format!(
                "pack memory {}: tier must be semantic or procedural",
                input.id
            )));
        }
        let memory = Memory::new(input.kind, input.tier, scope.clone(), &input.content, now)
            .with_id(format!("{}:{}", pack.id, input.id))
            .with_facets(input.facets.clone());
        let symbol = memory.symbol();
        if !wanted.insert(symbol.clone()) {
            return Err(Error::Invalid(format!(
                "pack memory {}: duplicate id",
                input.id
            )));
        }
        let hash = projection_hash(input)?;
        if let Some(existing) = engine.store.get_node(&symbol)? {
            let owner = memory_lock(&existing);
            if !matches!(&existing.kind, wicked_estate_core::NodeKind::Other(kind)
                if kind == crate::MEMORY_NODE_KIND || kind == crate::RETIRED_MEMORY_NODE_KIND)
                || owner.as_ref().is_none_or(|owner| owner.pack != pack.id)
            {
                return Err(Error::Invalid(format!(
                    "memory_locked: {} is not owned by pack {}",
                    memory.id, pack.id
                )));
            }
            if owner.as_ref() == Some(&lock)
                && existing
                    .metadata
                    .get("lock")
                    .and_then(|v| v.get("hash"))
                    .and_then(|v| v.as_str())
                    == Some(hash.as_str())
            {
                report.unchanged += 1;
                continue;
            }
        }
        let mut node = memory.to_node();
        let mut metadata = serde_json::to_value(&lock).map_err(|e| Error::Invalid(e.to_string()))?;
        metadata["hash"] = hash.into();
        node.metadata.insert("lock".into(), metadata);
        writes.push((memory, node));
    }
    let removed: Vec<SymbolId> = old
        .into_iter()
        .map(|node| node.symbol)
        .filter(|id| !wanted.contains(id))
        .collect();
    // Node mutation and index maintenance match ordinary capture/erase, with ownership checked
    // above instead of capture's lock refusal. There is no ordinary unlock operation.
    for (memory, node) in writes {
        engine.store.upsert_nodes(&[node])?;
        let embedding = engine.embed_for(&memory.content);
        engine.store.set_embedding(&memory.symbol(), &embedding)?;
        engine.ext.upsert(&memory)?;
        report.written += 1;
    }
    if !removed.is_empty() {
        engine.store.remove_nodes(&removed)?;
        let ids: Vec<String> = removed.iter().map(|id| id.0.clone()).collect();
        engine.ext.remove(&ids)?;
        if let Some(xedge) = &engine.xedge {
            for id in &ids {
                xedge.delete_by_src_id("memory", id)?;
            }
        }
    }
    report.erased = removed.len();
    Ok(report)
}
