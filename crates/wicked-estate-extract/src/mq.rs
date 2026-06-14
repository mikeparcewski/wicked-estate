//! IBM MQ Script Command (MQSC) extractor — **line-oriented, NO tree-sitter grammar**.
//!
//! No tree-sitter grammar exists for MQSC. The format is a sequence of line-oriented commands
//! that define an IBM MQ messaging *estate*: queues, channels, and topics. This is a first-class
//! language via a ~200-line line parser — the same pattern as [`super::jcl`] and
//! [`super::hlasm`]. Estate mapping per `docs/adr/ADR-004`.
//!
//! > **Scope:** this extractor covers the *queue/channel TOPOLOGY* from MQSC definition files.
//! > MQI CALL edges in COBOL (`CALL 'MQOPEN'`) are already captured by the COBOL extractor as
//! > call edges and are **not** duplicated here.
//!
//! # What it extracts
//!
//! | MQSC command                         | node kind        | edge kind          |
//! |--------------------------------------|------------------|--------------------|
//! | `DEFINE`/`ALTER QLOCAL(<q>)`         | `mq_queue`       | —                  |
//! | `DEFINE`/`ALTER QREMOTE(<q>)`        | `mq_queue`       | `resolves_to` ref  |
//! | `DEFINE`/`ALTER QALIAS(<q>)`         | `mq_queue`       | `resolves_to` ref  |
//! | `DEFINE`/`ALTER QMODEL(<q>)`         | `mq_queue`       | —                  |
//! | `DEFINE`/`ALTER CHANNEL(<c>)`        | `mq_channel`     | —                  |
//! | `DEFINE`/`ALTER TOPIC(<t>)`          | `mq_topic`       | —                  |
//!
//! `QREMOTE` additionally emits an [`UnresolvedRef`] with `EdgeKind::Other("resolves_to")` when
//! the command carries an `RNAME(<target>)` attribute (a remote queue points at a target queue).
//! `QALIAS` does the same for `TARGET(<t>)`. The ref is **unresolved** because the target queue
//! may live in a different MQSC file or even a different queue manager — the resolver decides.
//!
//! # MQSC syntax handled
//!
//! - **Comments:** lines whose first character is `*` are comment lines and are skipped.
//! - **Continuation:** a logical command may span multiple physical lines. A physical line that
//!   ends with a trailing `+` or `-` (optionally followed by whitespace) is continued on the next
//!   line. The continuation character is stripped and the logical command is assembled before
//!   parsing. This matters because real MQSC files routinely split long `DEFINE` commands.
//! - **Case-insensitive:** MQSC keywords are case-insensitive in practice; the regexes use `(?i)`.
//! - **Unknown verbs / types:** silently skipped — this extractor is additive, not exhaustive.
//!
//! # Regex safety
//!
//! All regexes are compiled exactly once via [`std::sync::LazyLock`] and are `Send + Sync`.

use regex::Regex;
use std::sync::LazyLock;
use wicked_estate_core::{
    EdgeKind, Extraction, Extractor, Language, Location, Node, NodeKind, Result, SourceFile, Span,
    Symbol, UnresolvedRef,
};

// ── Compiled regexes ──────────────────────────────────────────────────────────

/// Matches an MQSC object-type+name token, e.g. `QLOCAL(PAYROLL.IN)` or `CHANNEL(PAY.CHANNEL)`.
///
/// - Group 1: the object type keyword (e.g. `QLOCAL`, `CHANNEL`, `TOPIC`).
/// - Group 2: the object name inside the parentheses.
static RE_OBJECT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(QLOCAL|QREMOTE|QALIAS|QMODEL|CHANNEL|TOPIC)\s*\(\s*([^\s)]+)\s*\)")
        .expect("RE_OBJECT must compile")
});

/// Matches a keyword-value attribute like `RNAME(REMOTE.PAY)` or `TARGET(ALIAS.TARGET)`.
///
/// - Group 1: the keyword name (e.g. `RNAME`, `TARGET`).
/// - Group 2: the value inside the parentheses (unquoted).
static RE_KV: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b([A-Z]+)\s*\(\s*([^\s)]*)\s*\)").expect("RE_KV must compile")
});

// ── Extractor ─────────────────────────────────────────────────────────────────

/// Extractor for IBM MQ Script Command (MQSC) files. Grammar-less; parses the line format
/// directly, joining continuation lines before matching.
pub struct MqExtractor;

impl MqExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MqExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for MqExtractor {
    fn languages(&self) -> Vec<Language> {
        vec![Language::new("mq")]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        let mut nodes: Vec<Node> = Vec::new();
        let mut refs: Vec<UnresolvedRef> = Vec::new();

        // Join continuation lines before parsing. A physical line ending in `+` or `-`
        // (optionally followed by whitespace) is continued on the next physical line.
        let logical_commands = join_continuations(&file.text);

        for (logical_line, first_physical_line) in logical_commands {
            let trimmed = logical_line.trim();

            // Comment lines: first character `*` (in MQSC `*` is always col-1 for comments,
            // but after continuation joining we test on the trimmed form for robustness).
            if trimmed.starts_with('*') || trimmed.is_empty() {
                continue;
            }

            // The first token is the verb (DEFINE / ALTER / DISPLAY / DELETE / …).
            let mut toks = trimmed.split_whitespace();
            let Some(verb) = toks.next() else { continue };
            let verb_upper = verb.to_ascii_uppercase();

            // We only handle topology-defining verbs.
            if verb_upper != "DEFINE" && verb_upper != "ALTER" {
                continue;
            }

            // Find the object type+name pair, e.g. `QLOCAL(PAYROLL.IN)`.
            let Some(obj_caps) = RE_OBJECT.captures(trimmed) else {
                continue;
            };
            let obj_type = obj_caps[1].to_ascii_uppercase();
            // MQSC object names may be quoted (`QLOCAL('PAY.IN')`) or bare (`QLOCAL(PAY.IN)`); the
            // stored name is the clean identifier either way, so it is queryable by its real name
            // and so RACF MQQUEUE-profile matching (which sees unquoted profiles) lines up.
            let obj_name = obj_caps[2].trim().trim_matches('\'').to_string();

            if obj_name.is_empty() {
                continue;
            }

            let loc = Location::new(&file.path, line_span(first_physical_line));
            let sym = Symbol::synthetic("mq", obj_name.as_str()).id();

            // Map object type to node kind; use the type tag as the signature too.
            let node_kind = match obj_type.as_str() {
                "QLOCAL" | "QREMOTE" | "QALIAS" | "QMODEL" => {
                    NodeKind::Other("mq_queue".to_string())
                }
                "CHANNEL" => NodeKind::Other("mq_channel".to_string()),
                "TOPIC" => NodeKind::Other("mq_topic".to_string()),
                _ => continue, // unknown — skip (RE_OBJECT already restricts, but be safe)
            };

            // Only emit the node once per name (DEFINE wins; ALTER on an existing node does not
            // duplicate it — graph upsert semantics handle it at the store level, but we avoid
            // emitting duplicate nodes in a single file here too).
            if !nodes.iter().any(|n| n.symbol == sym) {
                let mut node = Node::new(
                    sym.clone(),
                    node_kind,
                    obj_name,
                    Language::new("mq"),
                    loc.clone(),
                );
                node.signature = Some(obj_type.clone());
                nodes.push(node);
            }

            // Emit a `resolves_to` ref for QREMOTE (RNAME) and QALIAS (TARGET).
            let target_attr = match obj_type.as_str() {
                "QREMOTE" => Some("RNAME"),
                "QALIAS" => Some("TARGET"),
                _ => None,
            };

            if let Some(attr) = target_attr {
                if let Some(target_name) = extract_kv(trimmed, attr) {
                    let target_name = target_name.trim().trim_matches('\'').to_string();
                    if !target_name.is_empty() {
                        refs.push(UnresolvedRef::new(
                            sym,
                            target_name,
                            EdgeKind::Other("resolves_to".to_string()),
                            loc,
                        ));
                    }
                }
            }
        }

        // No intra-file structural edges: a queue definition does not own another definition in
        // a containment sense. Cross-queue topology comes via UnresolvedRef (resolves_to).
        Ok(Extraction {
            nodes,
            local_edges: Vec::new(),
            refs,
        })
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// One-line span anchor. MQSC is line-oriented; byte offsets within a card are not tracked.
fn line_span(line: usize) -> Span {
    let l = line as u32;
    Span {
        start_byte: 0,
        end_byte: 0,
        start_line: l,
        start_col: 0,
        end_line: l,
        end_col: 0,
    }
}

/// Join continuation lines into logical commands. A physical line that ends with `+` or `-`
/// (modulo trailing whitespace) is continued. Returns `(logical_text, first_physical_line_index)`.
fn join_continuations(text: &str) -> Vec<(String, usize)> {
    let mut result = Vec::new();
    let mut current: Option<(String, usize)> = None;

    for (i, raw) in text.lines().enumerate() {
        // Strip the trailing newline artifact from `lines()` (already done), but preserve
        // interior whitespace so token splitting still works.
        let stripped = raw.trim_end();

        // Detect a continuation character: `+` or `-` at end of (stripped) line.
        let is_continuation = stripped
            .chars()
            .last()
            .is_some_and(|c| c == '+' || c == '-');

        // The body to accumulate: strip the trailing continuation character if present.
        let body = if is_continuation {
            stripped[..stripped.len() - 1].to_string()
        } else {
            stripped.to_string()
        };

        match current.as_mut() {
            None => {
                if is_continuation {
                    current = Some((body + " ", i));
                } else {
                    result.push((body, i));
                }
            }
            Some((acc, _)) => {
                // Append to the accumulated logical command.
                acc.push_str(&body);
                if !is_continuation {
                    let finished = current.take().unwrap();
                    result.push(finished);
                } else {
                    acc.push(' ');
                }
            }
        }
    }

    // Any unterminated continuation at EOF is still a valid (partial) command.
    if let Some(cmd) = current {
        result.push(cmd);
    }

    result
}

/// Extract the value for a given `keyword` from a logical MQSC command string.
/// `RNAME(REMOTE.PAY)` with keyword `"RNAME"` → `Some("REMOTE.PAY")`.
/// Matching is case-insensitive.
fn extract_kv(command: &str, keyword: &str) -> Option<String> {
    for caps in RE_KV.captures_iter(command) {
        if caps[1].eq_ignore_ascii_case(keyword) {
            let val = caps[2].trim().to_string();
            return Some(val);
        }
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn extract(text: &str) -> Extraction {
        MqExtractor::new()
            .extract(&SourceFile {
                path: "PAYROLL.mqsc".to_string(),
                language: Language::new("mq"),
                text: text.to_string(),
            })
            .expect("mq extract")
    }

    // ── node kinds ────────────────────────────────────────────────────────────

    #[test]
    fn qlocal_emits_mq_queue_node() {
        let ex = extract("DEFINE QLOCAL(PAYROLL.IN) MAXDEPTH(5000)\n");
        let node = ex
            .nodes
            .iter()
            .find(|n| n.name == "PAYROLL.IN")
            .expect("expected a PAYROLL.IN node");
        assert_eq!(node.kind, NodeKind::Other("mq_queue".to_string()));
        assert_eq!(node.signature.as_deref(), Some("QLOCAL"));
    }

    #[test]
    fn quoted_object_name_is_stored_clean() {
        // MQSC allows quoted names; the stored identifier must be the bare name so it is queryable
        // by its real name and matches RACF MQQUEUE profiles (which carry no quotes). RNAME quotes
        // are stripped too, so the resolves_to target lines up with the real queue.
        let ex = extract("DEFINE QREMOTE('PAYROLL.OUT') RNAME('HR.INBOUND') RQMNAME(QMHR)\n");
        assert!(
            ex.nodes.iter().any(|n| n.name == "PAYROLL.OUT"),
            "quoted QREMOTE name must store as PAYROLL.OUT, got {:?}",
            ex.nodes.iter().map(|n| &n.name).collect::<Vec<_>>()
        );
        assert!(
            ex.refs.iter().any(|r| r.raw_name == "HR.INBOUND"),
            "quoted RNAME must resolve_to HR.INBOUND, got {:?}",
            ex.refs.iter().map(|r| &r.raw_name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn qremote_emits_mq_queue_node() {
        let ex = extract("DEFINE QREMOTE(PAYROLL.OUT) RNAME(REMOTE.PAY) RQMNAME(QMGR2)\n");
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "PAYROLL.OUT"
                    && n.kind == NodeKind::Other("mq_queue".to_string())),
            "expected a PAYROLL.OUT mq_queue node"
        );
    }

    #[test]
    fn channel_emits_mq_channel_node() {
        let ex = extract("DEFINE CHANNEL(PAY.CHANNEL) CHLTYPE(SDR)\n");
        let node = ex
            .nodes
            .iter()
            .find(|n| n.name == "PAY.CHANNEL")
            .expect("expected a PAY.CHANNEL node");
        assert_eq!(node.kind, NodeKind::Other("mq_channel".to_string()));
        assert_eq!(node.signature.as_deref(), Some("CHANNEL"));
    }

    #[test]
    fn topic_emits_mq_topic_node() {
        let ex = extract("DEFINE TOPIC(PAYROLL.EVENTS) TOPICSTR('/payroll/events')\n");
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "PAYROLL.EVENTS"
                    && n.kind == NodeKind::Other("mq_topic".to_string())),
            "expected a PAYROLL.EVENTS mq_topic node"
        );
    }

    #[test]
    fn qmodel_emits_mq_queue_node() {
        let ex = extract("DEFINE QMODEL(PAYROLL.MODEL) MAXDEPTH(100)\n");
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "PAYROLL.MODEL"
                    && n.kind == NodeKind::Other("mq_queue".to_string())),
            "expected a PAYROLL.MODEL mq_queue node"
        );
    }

    // ── resolves_to refs ──────────────────────────────────────────────────────

    #[test]
    fn qremote_rname_emits_resolves_to_ref() {
        let ex = extract("DEFINE QREMOTE(PAYROLL.OUT) RNAME(REMOTE.PAY) RQMNAME(QMGR2)\n");
        let r = ex
            .refs
            .iter()
            .find(|r| r.raw_name == "REMOTE.PAY")
            .expect("expected a resolves_to ref PAYROLL.OUT → REMOTE.PAY");
        assert_eq!(r.kind, EdgeKind::Other("resolves_to".to_string()));
        // The ref originates from the PAYROLL.OUT symbol.
        let out_sym = Symbol::synthetic("mq", "PAYROLL.OUT").id();
        assert_eq!(r.from, out_sym);
    }

    #[test]
    fn qalias_target_emits_resolves_to_ref() {
        let ex = extract("DEFINE QALIAS(PAYROLL.ALIAS) TARGET(PAYROLL.IN)\n");
        let r = ex
            .refs
            .iter()
            .find(|r| r.raw_name == "PAYROLL.IN")
            .expect("expected a resolves_to ref for the QALIAS TARGET");
        assert_eq!(r.kind, EdgeKind::Other("resolves_to".to_string()));
    }

    #[test]
    fn qlocal_no_resolves_to_ref() {
        // A local queue has no remote target — no refs should be emitted.
        let ex = extract("DEFINE QLOCAL(PAYROLL.IN) MAXDEPTH(5000)\n");
        assert!(ex.refs.is_empty(), "QLOCAL must not emit any refs");
    }

    // ── comment + unknown verb skipping ───────────────────────────────────────

    #[test]
    fn comment_lines_skipped() {
        let ex = extract("* This is a comment\n* Another comment\n");
        assert!(ex.nodes.is_empty(), "comment lines must not produce nodes");
        assert!(ex.refs.is_empty());
    }

    #[test]
    fn display_verb_skipped() {
        // DISPLAY is a read-only verb — no topology change.
        let ex = extract("DISPLAY QLOCAL(PAYROLL.IN)\n");
        assert!(ex.nodes.is_empty(), "DISPLAY must not produce nodes");
    }

    #[test]
    fn alter_verb_still_emits_node() {
        // ALTER updates attributes on an existing object — we still want the node in the graph.
        let ex = extract("ALTER QLOCAL(PAYROLL.IN) MAXDEPTH(9999)\n");
        assert!(
            ex.nodes.iter().any(
                |n| n.name == "PAYROLL.IN" && n.kind == NodeKind::Other("mq_queue".to_string())
            ),
            "ALTER must produce a node"
        );
    }

    // ── continuation joining ──────────────────────────────────────────────────

    #[test]
    fn continuation_plus_joins_command() {
        // A command split with `+` continuation across two physical lines must parse as one.
        let text = "DEFINE QLOCAL(PAYROLL.IN) +\n  MAXDEPTH(5000)\n";
        let ex = extract(text);
        let node = ex
            .nodes
            .iter()
            .find(|n| n.name == "PAYROLL.IN")
            .expect("continuation-joined QLOCAL must produce a node");
        assert_eq!(node.kind, NodeKind::Other("mq_queue".to_string()));
    }

    #[test]
    fn continuation_minus_joins_command() {
        let text = "DEFINE CHANNEL(PAY.CHANNEL)-\n  CHLTYPE(SDR)\n";
        let ex = extract(text);
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "PAY.CHANNEL"
                    && n.kind == NodeKind::Other("mq_channel".to_string())),
            "continuation with `-` must join and produce a channel node"
        );
    }

    #[test]
    fn qremote_rname_on_continued_line() {
        // The RNAME attribute is on the second physical line after a `+` continuation.
        let text = "DEFINE QREMOTE(PAYROLL.OUT) +\n  RNAME(REMOTE.PAY) RQMNAME(QMGR2)\n";
        let ex = extract(text);
        assert!(
            ex.refs.iter().any(|r| r.raw_name == "REMOTE.PAY"),
            "RNAME on a continuation line must still produce a resolves_to ref"
        );
    }

    // ── full scenario: the spec's representative snippet ─────────────────────

    #[test]
    fn spec_snippet_full() {
        let text = concat!(
            "* PAYROLL queue definitions\n",
            "DEFINE QLOCAL(PAYROLL.IN) MAXDEPTH(5000)\n",
            "DEFINE QREMOTE(PAYROLL.OUT) RNAME(REMOTE.PAY) RQMNAME(QMGR2)\n",
            "DEFINE CHANNEL(PAY.CHANNEL) CHLTYPE(SDR)\n",
            "DEFINE QLOCAL(PAYROLL.DEAD) +\n",
            "  MAXDEPTH(100) DESCR('Dead-letter queue')\n",
        );
        let ex = extract(text);

        // Nodes: PAYROLL.IN, PAYROLL.OUT, PAY.CHANNEL, PAYROLL.DEAD
        let queue_names: Vec<&str> = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Other("mq_queue".to_string()))
            .map(|n| n.name.as_str())
            .collect();
        assert!(
            queue_names.contains(&"PAYROLL.IN"),
            "PAYROLL.IN missing; got: {queue_names:?}"
        );
        assert!(
            queue_names.contains(&"PAYROLL.OUT"),
            "PAYROLL.OUT missing; got: {queue_names:?}"
        );
        assert!(
            queue_names.contains(&"PAYROLL.DEAD"),
            "PAYROLL.DEAD (continuation) missing; got: {queue_names:?}"
        );

        let channel_names: Vec<&str> = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Other("mq_channel".to_string()))
            .map(|n| n.name.as_str())
            .collect();
        assert!(
            channel_names.contains(&"PAY.CHANNEL"),
            "PAY.CHANNEL missing; got: {channel_names:?}"
        );

        // resolves_to ref: PAYROLL.OUT → REMOTE.PAY
        assert!(
            ex.refs.iter().any(|r| r.raw_name == "REMOTE.PAY"
                && r.kind == EdgeKind::Other("resolves_to".to_string())),
            "expected resolves_to ref PAYROLL.OUT → REMOTE.PAY"
        );
    }

    // ── extract_kv helper ─────────────────────────────────────────────────────

    #[test]
    fn extract_kv_finds_value() {
        assert_eq!(
            extract_kv(
                "DEFINE QREMOTE(Q) RNAME(REMOTE.PAY) RQMNAME(QMGR2)",
                "RNAME"
            ),
            Some("REMOTE.PAY".to_string())
        );
        assert_eq!(
            extract_kv("DEFINE QALIAS(A) TARGET(PAYROLL.IN)", "TARGET"),
            Some("PAYROLL.IN".to_string())
        );
    }

    #[test]
    fn extract_kv_case_insensitive() {
        assert_eq!(
            extract_kv("define qremote(Q) rname(REMOTE.Q)", "RNAME"),
            Some("REMOTE.Q".to_string())
        );
    }

    #[test]
    fn extract_kv_missing_returns_none() {
        assert_eq!(extract_kv("DEFINE QLOCAL(Q) MAXDEPTH(5000)", "RNAME"), None);
    }

    // ── join_continuations helper ─────────────────────────────────────────────

    #[test]
    fn join_continuations_single_line() {
        let cmds = join_continuations("DEFINE QLOCAL(Q)\n");
        assert_eq!(cmds.len(), 1);
        assert!(cmds[0].0.contains("QLOCAL(Q)"));
        assert_eq!(cmds[0].1, 0); // first physical line index
    }

    #[test]
    fn join_continuations_two_physical_lines() {
        let cmds = join_continuations("DEFINE QLOCAL(Q) +\n  MAXDEPTH(5000)\n");
        assert_eq!(cmds.len(), 1, "two physical lines → one logical command");
        assert!(cmds[0].0.contains("QLOCAL(Q)"));
        assert!(cmds[0].0.contains("MAXDEPTH(5000)"));
    }

    #[test]
    fn join_continuations_records_first_physical_line() {
        // The first logical command starts at physical line 0; the second at 2.
        let text = "* comment\nDEFINE QLOCAL(A)\nDEFINE CHANNEL(C)\n";
        let cmds = join_continuations(text);
        // All three lines produce entries (comment + two commands).
        let define_cmds: Vec<_> = cmds.iter().filter(|(t, _)| t.contains("DEFINE")).collect();
        assert_eq!(define_cmds.len(), 2);
        assert_eq!(define_cmds[0].1, 1); // DEFINE QLOCAL at physical line 1
        assert_eq!(define_cmds[1].1, 2); // DEFINE CHANNEL at physical line 2
    }
}
