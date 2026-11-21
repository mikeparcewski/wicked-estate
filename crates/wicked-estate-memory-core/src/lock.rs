//! Pack-owned memory locks live on nodes, outside the exhaustive `Memory` value type.

use serde::{Deserialize, Serialize};
use wicked_estate_core::Node;

/// A verified pack's ownership of an org or team memory. Only the pack application path writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MemLock {
    pub pack: String,
    pub layer: String,
    pub version: String,
}

impl MemLock {
    pub fn new(
        pack: impl Into<String>,
        layer: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            pack: pack.into(),
            layer: layer.into(),
            version: version.into(),
        }
    }
}

/// Absent means unlocked. Any present but malformed lock fails closed, with unknown ownership.
/// The pack path also stores a `hash` member; it is not part of the ownership descriptor.
pub fn memory_lock(node: &Node) -> Option<MemLock> {
    let value = node.metadata.get("lock")?;
    let lock = serde_json::from_value::<MemLock>(value.clone()).ok();
    Some(match lock {
        Some(lock)
            if !lock.pack.trim().is_empty()
                && !lock.version.trim().is_empty()
                && matches!(lock.layer.as_str(), "org" | "team") =>
        {
            lock
        }
        _ => MemLock::new("?", "?", "?"),
    })
}
