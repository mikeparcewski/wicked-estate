//! Continuation paging for the row-array graph tools (`Lineage`, `BlastRadius`).
//!
//! The operator's ruling on #279 (2026-10-11), "I want the full graph": the R4 char budget bounds
//! ONE reply, never the answer.
//! - A reply that does not carry every row of its answer says so (`truncated: true`) and carries a
//!   `next_cursor`. Passing that cursor back as `cursor` returns the next rows of the SAME answer.
//! - The last page carries `next_cursor: null`.
//! - Rows are ordered deterministically, so pages from separate processes join into exactly the
//!   whole answer.
//!
//! A cursor is bound to its tool, its query and the exact answer it pages. It holds a fingerprint
//! over every row, so a graph that changed between pages, or a cursor replayed against another
//! query, is refused. The pages are never spliced from two different answers.

use serde_json::Value;
use std::hash::{Hash, Hasher};
use wicked_estate_core::{Error, Result};

/// The request key a caller passes a `next_cursor` back under.
pub(crate) const CURSOR_KEY: &str = "cursor";

/// Fingerprint of one complete answer: the tool, its effective query, and every row of every
/// paged array, in order. The crate version is included, so a cursor from another build reads as
/// stale rather than as a position in an answer this build may order differently.
pub(crate) fn fingerprint(tool: &str, query: &str, arrays: &[&[Value]]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    env!("CARGO_PKG_VERSION").hash(&mut h);
    tool.hash(&mut h);
    query.hash(&mut h);
    for rows in arrays {
        rows.len().hash(&mut h);
        for row in rows.iter() {
            row.to_string().hash(&mut h);
        }
    }
    h.finish()
}

/// The opaque cursor for the rows starting at `offsets` (one per paged array).
pub(crate) fn encode(offsets: &[usize], fingerprint: u64) -> String {
    let offsets: Vec<String> = offsets.iter().map(usize::to_string).collect();
    format!("p1.{}.{fingerprint:016x}", offsets.join("-"))
}

/// Where this page starts: all zeros without a cursor, else the cursor's offsets. A malformed
/// cursor, or one issued for another query or another state of the graph, is an error. It never
/// restarts silently, which would hand a caller page 1 twice and read as the rest of the answer.
pub(crate) fn start_offsets(
    tool: &str,
    request: &Value,
    lens: &[usize],
    fingerprint: u64,
) -> Result<Vec<usize>> {
    let raw = match request.get(CURSOR_KEY) {
        None | Some(Value::Null) => return Ok(vec![0; lens.len()]),
        Some(Value::String(s)) if s.is_empty() => return Ok(vec![0; lens.len()]),
        Some(Value::String(s)) => s.as_str(),
        Some(other) => {
            return Err(Error::Invalid(format!(
                "{tool}: `cursor` must be the string a previous reply returned as `next_cursor`, \
                 got {other}"
            )));
        }
    };
    let malformed = || {
        Error::Invalid(format!(
            "{tool}: malformed `cursor` {raw:?}; pass back the `next_cursor` of the previous \
             page unchanged, or omit `cursor` for the first page"
        ))
    };
    let mut parts = raw.split('.');
    let (Some("p1"), Some(offsets), Some(fp), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(malformed());
    };
    let offsets: Vec<usize> = offsets
        .split('-')
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| malformed())?;
    let fp = u64::from_str_radix(fp, 16).map_err(|_| malformed())?;
    if offsets.len() != lens.len() {
        return Err(malformed());
    }
    if fp != fingerprint || offsets.iter().zip(lens).any(|(o, l)| o > l) {
        return Err(Error::Invalid(format!(
            "{tool}: this `cursor` belongs to another query, or the graph changed since it was \
             issued; restart without `cursor` to page the current answer"
        )));
    }
    Ok(offsets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_cursor_round_trips_and_is_bound_to_its_answer() {
        let rows = vec![json!({"a": 1}), json!({"a": 2})];
        let fp = fingerprint("T", "q", &[&rows]);
        let c = encode(&[1], fp);
        assert_eq!(
            start_offsets("T", &json!({"cursor": c}), &[2], fp).unwrap(),
            vec![1]
        );
        assert_eq!(start_offsets("T", &json!({}), &[2], fp).unwrap(), vec![0]);
        // Another answer (a changed row), another query, another tool: refused, never re-read.
        let changed = vec![json!({"a": 1}), json!({"a": 3})];
        for other in [
            fingerprint("T", "q", &[&changed]),
            fingerprint("T", "q2", &[&rows]),
            fingerprint("U", "q", &[&rows]),
        ] {
            assert!(start_offsets("T", &json!({"cursor": c}), &[2], other).is_err());
        }
        // Malformed shapes and out-of-range offsets.
        for bad in [
            json!("x"),
            json!("p1.1"),
            json!("p2.1.00"),
            json!("p1.a.00"),
            json!("p1.1-1.00"),
            json!(7),
        ] {
            assert!(start_offsets("T", &json!({ "cursor": bad }), &[2], fp).is_err());
        }
        assert!(start_offsets("T", &json!({"cursor": encode(&[3], fp)}), &[2], fp).is_err());
    }
}
