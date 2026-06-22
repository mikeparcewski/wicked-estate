//! Watch coalescing decision (A-6) — the pure, testable core of the `watch` event loop's
//! "emit ONCE per change, not per event-storm" rule.
//!
//! The `notify-debouncer-full` watcher already folds a burst of raw filesystem events into one
//! debounced batch (`Vec<Event>`) over its 500ms window. This module isolates the *decision*
//! the loop makes per batch so it can be unit-tested without spinning up a real watcher (which
//! would be slow and timing-flaky):
//!
//! * is this batch relevant (does it contain a create / modify / remove)?
//! * given a relevant batch, how many coarse `wicked.estate.indexed` emits should fire?
//!
//! The answer to the second question is the whole point of A-6: **exactly one** per relevant
//! batch, regardless of how many raw events the batch folded in. An irrelevant batch
//! (access-only / other) fires zero.

use notify::EventKind;

/// True if `kind` is a change worth re-indexing for (create / modify / remove). Access and
/// Other events are ignored — they are not changes.
pub fn is_relevant(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    )
}

/// Whether a debounced batch (already coalesced by the watcher) warrants a re-index + a single
/// coarse emit. Returns `true` iff at least one event in the batch is relevant.
///
/// This is the predicate the `watch` loop uses to gate its single `index_path` + single
/// `emit_event` call per batch.
pub fn batch_is_relevant<'a, I>(kinds: I) -> bool
where
    I: IntoIterator<Item = &'a EventKind>,
{
    kinds.into_iter().any(is_relevant)
}

/// How many coarse `wicked.estate.indexed` emits a single debounced batch should produce: `1`
/// for a relevant batch (the storm is already coalesced into this one batch), `0` otherwise.
///
/// This makes the once-per-change contract a returnable, assertable value.
pub fn emits_for_batch<'a, I>(kinds: I) -> usize
where
    I: IntoIterator<Item = &'a EventKind>,
{
    if batch_is_relevant(kinds) { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind};

    /// A-6: a batch that folded in a STORM of raw events still yields exactly ONE emit. This is
    /// the falsifier for "emits once per event-storm" — if the rule were per-event, a 50-event
    /// batch would return 50.
    #[test]
    fn storm_of_events_coalesces_to_one_emit() {
        // 50 raw modify events, as the debouncer would hand us after a burst of saves.
        let storm: Vec<EventKind> = (0..50)
            .map(|_| EventKind::Modify(ModifyKind::Any))
            .collect();
        assert_eq!(
            emits_for_batch(storm.iter()),
            1,
            "a coalesced storm must emit exactly once, not once per raw event"
        );
    }

    /// A mixed batch (create + modify + remove) is one change → one emit.
    #[test]
    fn mixed_relevant_batch_emits_once() {
        let batch = [
            EventKind::Create(CreateKind::File),
            EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Content)),
            EventKind::Remove(RemoveKind::File),
        ];
        assert!(batch_is_relevant(batch.iter()));
        assert_eq!(emits_for_batch(batch.iter()), 1);
    }

    /// An access-only / other batch is not a change → zero emits (no spurious event).
    #[test]
    fn irrelevant_batch_emits_zero() {
        let batch = [EventKind::Access(AccessKind::Any), EventKind::Other];
        assert!(!batch_is_relevant(batch.iter()));
        assert_eq!(emits_for_batch(batch.iter()), 0);
    }

    /// An empty batch emits zero.
    #[test]
    fn empty_batch_emits_zero() {
        let batch: [EventKind; 0] = [];
        assert_eq!(emits_for_batch(batch.iter()), 0);
    }
}
