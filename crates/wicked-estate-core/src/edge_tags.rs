//! Canonical tag names for framework / non-structural relationships carried on
//! [`EdgeKind::Other`](crate::EdgeKind::Other).
//!
//! Modern frameworks connect code through mechanisms that `Calls`/`Imports` miss:
//! dependency injection, event pub/sub, route bindings, lifecycle hooks. We model these
//! as `EdgeKind::Other("<tag>")` rather than minting new enum variants, keeping the core
//! schema stable (a new framework relationship is *data*, not a core change — see CLAUDE.md
//! "Rules as DATA"). This module is the single source of truth for the tag strings so
//! extractors emit and consumers match against the **same** constants instead of stringly-typed
//! literals scattered across crates.
//!
//! # Convention
//!
//! `tag` is the bare string stored in `EdgeKind::Other(tag)`. Helper [`other`] builds the
//! [`EdgeKind`](crate::EdgeKind) directly. Tags are lowercase, hyphen-free, `snake`-free —
//! single hyphenated words (`di-wired`) — to read cleanly in CLI output and JSON.

use crate::EdgeKind;

/// Dependency-injection wiring: a class/component depends on an injected collaborator
/// (`@Autowired`, constructor injection, `@Inject`, NestJS `@Injectable` constructor params).
/// `source` = the injecting type (dependent), `target` = the injected type (dependency).
pub const DI_WIRED: &str = "di-wired";

/// A handler bound to a route/path by a framework annotation (`@GetMapping`, `@PostMapping`,
/// `@RequestMapping`, NestJS `@Get`/`@Post`). `source` = the route node, `target` = the handler.
pub const ROUTE_HANDLER: &str = "route-handler";

/// A symbol that emits/publishes an event or message
/// (`publishEvent`, `kafkaTemplate.send`, `eventBus.emit`).
/// `source` = the emitter, `target` = the event/topic node.
pub const EVENT_EMITS: &str = "event-emits";

/// A symbol that subscribes to / handles an event or message
/// (`@EventListener`, `@KafkaListener`, `@EventPattern`, `@MessagePattern`).
/// `source` = the listener, `target` = the event/topic node.
pub const EVENT_LISTENS: &str = "event-listens";

/// A framework lifecycle hook binding (`@PostConstruct`, `@PreDestroy`, `OnModuleInit`).
/// `source` = the hooked component, `target` = the lifecycle phase node.
pub const LIFECYCLE_HOOK: &str = "lifecycle-hook";

/// Synthetic package/module co-membership, derived at analysis time (not extracted).
/// Used by community detection to let package structure inform the partition. Never persisted
/// as a real graph edge by extractors.
pub const SAME_PACKAGE: &str = "same-package";

/// Build an [`EdgeKind::Other`] from a canonical tag constant.
///
/// ```
/// use wicked_estate_core::edge_tags::{self, other};
/// use wicked_estate_core::EdgeKind;
/// assert_eq!(other(edge_tags::DI_WIRED), EdgeKind::Other("di-wired".into()));
/// ```
pub fn other(tag: &str) -> EdgeKind {
    EdgeKind::Other(tag.to_string())
}

/// True if `kind` is an `Other` edge carrying exactly `tag`.
///
/// ```
/// use wicked_estate_core::edge_tags::{self, is_tag};
/// use wicked_estate_core::EdgeKind;
/// assert!(is_tag(&EdgeKind::Other("di-wired".into()), edge_tags::DI_WIRED));
/// assert!(!is_tag(&EdgeKind::Calls, edge_tags::DI_WIRED));
/// ```
pub fn is_tag(kind: &EdgeKind, tag: &str) -> bool {
    matches!(kind, EdgeKind::Other(t) if t == tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_builds_matching_kind() {
        assert_eq!(other(DI_WIRED), EdgeKind::Other("di-wired".into()));
        assert!(is_tag(&other(EVENT_EMITS), EVENT_EMITS));
        assert!(!is_tag(&other(EVENT_EMITS), EVENT_LISTENS));
    }

    #[test]
    fn tags_are_distinct() {
        let all = [
            DI_WIRED,
            ROUTE_HANDLER,
            EVENT_EMITS,
            EVENT_LISTENS,
            LIFECYCLE_HOOK,
            SAME_PACKAGE,
        ];
        let set: std::collections::HashSet<_> = all.iter().collect();
        assert_eq!(set.len(), all.len(), "edge tag constants must be unique");
    }
}
