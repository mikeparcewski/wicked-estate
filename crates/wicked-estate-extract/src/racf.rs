//! RACF (z/OS Resource Access Control Facility) extractor — **line-oriented, NO tree-sitter grammar**.
//!
//! RACF security policy is administered via TSO/ISPF commands stored in batch command members
//! (e.g. SYS1.RACF.CMDMEM, RACF batch job streams). Like JCL and HLASM, the format is
//! line-oriented with no expression grammar, so a line parser on the generic [`Extractor`] seam
//! is the right tool. This is estate mapping per ADR-004: RACF commands describe *who can access
//! what* across datasets, transactions, and resources — exactly the security layer of the estate.
//!
//! # What it extracts
//! - `RDEFINE <class> <profile>`          → a resource-profile node (`racf_profile`)
//! - `ADDSD '<dataset.profile>'`          → a dataset-profile node (`racf_dataset_profile`)
//! - `ADDGROUP <grp>`                     → a group node (`racf_group`)
//! - `ADDUSER <user>`                     → a user node (`racf_user`)
//! - `PERMIT <profile> … ID(<id>) …`      → an unresolved ref id → profile, kind `Other("permits")`
//! - `CONNECT <user> GROUP(<grp>)`        → an unresolved ref user → group, kind `Other("member_of")`
//!
//! Operands in `PERMIT` (`CLASS(...)`, `ID(...)`, `ACCESS(...)`) can appear in any order and are
//! parsed by pre-compiled regex. Unknown verbs are silently skipped (lenient).
//!
//! Continuation lines (trailing `+` or `-`) and blank/comment lines are skipped without error.
//! Comments: `/*…*/`-style lines and lines whose first non-whitespace token starts with `*`.

use std::sync::LazyLock;

use regex::Regex;
use wicked_estate_core::{
    EdgeKind, Extraction, Extractor, Language, Location, Node, NodeKind, Result, SourceFile, Span,
    Symbol, UnresolvedRef,
};

// ---------------------------------------------------------------------------
// Pre-compiled operand patterns — `KEYWORD(value)` extraction, any order.
// ---------------------------------------------------------------------------

/// Matches `ID(value)` — the user or group being granted access.
static RE_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)ID\(([^)]+)\)").expect("RE_ID"));

/// Matches `GROUP(value)` — used in CONNECT commands.
static RE_GROUP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)GROUP\(([^)]+)\)").expect("RE_GROUP"));

// ---------------------------------------------------------------------------
// Extractor
// ---------------------------------------------------------------------------

/// Extractor for RACF command streams. Grammar-less; parses the line-oriented TSO/batch format.
pub struct RacfExtractor;

impl RacfExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RacfExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for RacfExtractor {
    fn languages(&self) -> Vec<Language> {
        vec![Language::new("racf")]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        let mut nodes: Vec<Node> = Vec::new();
        let local_edges = Vec::new();
        let mut refs: Vec<UnresolvedRef> = Vec::new();

        for (i, raw) in file.text.lines().enumerate() {
            let trimmed = raw.trim();

            // Skip blank lines and comment lines.
            // `/*` block-comment syntax and `*`-prefixed line comments.
            if trimmed.is_empty() || trimmed.starts_with("/*") || trimmed.starts_with('*') {
                continue;
            }

            // Skip continuation-marker-only lines (a trailing `+` or `-` on the previous
            // line means the next line continues it; we treat each line independently and
            // simply skip lines that are pure continuation markers).
            if trimmed == "+" || trimmed == "-" {
                continue;
            }

            // Tokenise by whitespace. RACF verbs are always UPPERCASE; we upper-case the
            // first token defensively so mixed-case batch scripts are handled too.
            let mut toks = trimmed.split_whitespace();
            let Some(verb) = toks.next() else { continue };
            let verb_up = verb.to_ascii_uppercase();
            let loc = Location::new(&file.path, line_span(i));

            match verb_up.as_str() {
                // -------------------------------------------------------
                // RDEFINE <class> <profile>
                // Example: RDEFINE FACILITY BPX.SUPERUSER
                // -------------------------------------------------------
                "RDEFINE" => {
                    let Some(class) = toks.next() else { continue };
                    let Some(profile_tok) = toks.next() else {
                        continue;
                    };
                    let profile = strip_quotes(profile_tok);
                    let sym = Symbol::synthetic("racf", profile.as_str()).id();
                    let mut node = Node::new(
                        sym,
                        NodeKind::Other("racf_profile".to_string()),
                        profile,
                        Language::new("racf"),
                        loc,
                    );
                    node.signature = Some(class.to_ascii_uppercase());
                    nodes.push(node);
                }

                // -------------------------------------------------------
                // ADDSD '<dataset.profile>'   (or unquoted)
                // Example: ADDSD 'PROD.PAY.**'
                // -------------------------------------------------------
                "ADDSD" => {
                    let Some(raw_profile) = toks.next() else {
                        continue;
                    };
                    let profile = strip_quotes(raw_profile);
                    if profile.is_empty() {
                        continue;
                    }
                    let sym = Symbol::synthetic("racf", profile.as_str()).id();
                    let mut node = Node::new(
                        sym,
                        NodeKind::Other("racf_dataset_profile".to_string()),
                        profile,
                        Language::new("racf"),
                        loc,
                    );
                    node.signature = Some("DATASET".to_string());
                    nodes.push(node);
                }

                // -------------------------------------------------------
                // ADDGROUP <grp>
                // Example: ADDGROUP PAYROLL
                // -------------------------------------------------------
                "ADDGROUP" => {
                    let Some(grp) = toks.next() else { continue };
                    let grp = strip_quotes(grp);
                    if grp.is_empty() {
                        continue;
                    }
                    let sym = Symbol::synthetic("racf", grp.as_str()).id();
                    let node = Node::new(
                        sym,
                        NodeKind::Other("racf_group".to_string()),
                        grp,
                        Language::new("racf"),
                        loc,
                    );
                    nodes.push(node);
                }

                // -------------------------------------------------------
                // ADDUSER <user>
                // Example: ADDUSER JSMITH
                // -------------------------------------------------------
                "ADDUSER" => {
                    let Some(user) = toks.next() else { continue };
                    let user = strip_quotes(user);
                    if user.is_empty() {
                        continue;
                    }
                    let sym = Symbol::synthetic("racf", user.as_str()).id();
                    let node = Node::new(
                        sym,
                        NodeKind::Other("racf_user".to_string()),
                        user,
                        Language::new("racf"),
                        loc,
                    );
                    nodes.push(node);
                }

                // -------------------------------------------------------
                // PERMIT <profile> CLASS(<class>) ID(<id>) ACCESS(<level>)
                // Operands may appear in any order after the profile.
                // Example: PERMIT 'PROD.PAY.**' CLASS(DATASET) ID(PAYROLL) ACCESS(READ)
                //
                // Emits an UnresolvedRef:  id (dependent) → profile (dependency)
                // kind = Other("permits")
                // -------------------------------------------------------
                "PERMIT" => {
                    let Some(raw_profile) = toks.next() else {
                        continue;
                    };
                    let profile = strip_quotes(raw_profile);
                    if profile.is_empty() {
                        continue;
                    }
                    // `trimmed` starts with the verb; skip past it to get the operand region so
                    // that `find(raw_profile)` reliably locates the *profile* token, not the verb.
                    let after_verb = trimmed.trim_start_matches(verb).trim_start();
                    let rest = after_verb[after_verb.find(raw_profile).unwrap_or(0)..].to_string();
                    let id = RE_ID
                        .captures(&rest)
                        .and_then(|c| c.get(1))
                        .map(|m| m.as_str().trim().to_string());
                    let Some(id) = id else { continue };
                    if id.is_empty() {
                        continue;
                    }
                    let from = Symbol::synthetic("racf", id.as_str()).id();
                    refs.push(UnresolvedRef::new(
                        from,
                        profile,
                        EdgeKind::Other("permits".to_string()),
                        loc,
                    ));
                }

                // -------------------------------------------------------
                // CONNECT <user> GROUP(<grp>)
                // Example: CONNECT JSMITH GROUP(PAYROLL)
                //
                // Emits an UnresolvedRef:  user (dependent) → grp (dependency)
                // kind = Other("member_of")
                // -------------------------------------------------------
                "CONNECT" => {
                    let Some(raw_user) = toks.next() else {
                        continue;
                    };
                    let user = strip_quotes(raw_user);
                    if user.is_empty() {
                        continue;
                    }
                    // Skip past the verb in `trimmed`, then past the user token, to get the
                    // operand tail where GROUP(...) lives.
                    let after_verb = trimmed.trim_start_matches(verb).trim_start();
                    let after_user_start = after_verb.find(raw_user).unwrap_or(0) + raw_user.len();
                    let rest = &after_verb[after_user_start..];
                    let grp = RE_GROUP
                        .captures(rest)
                        .and_then(|c| c.get(1))
                        .map(|m| m.as_str().trim().to_string());
                    let Some(grp) = grp else { continue };
                    if grp.is_empty() {
                        continue;
                    }
                    let from = Symbol::synthetic("racf", user.as_str()).id();
                    refs.push(UnresolvedRef::new(
                        from,
                        grp,
                        EdgeKind::Other("member_of".to_string()),
                        loc,
                    ));
                }

                // Unknown verbs → skip.
                _ => {}
            }
        }

        Ok(Extraction {
            nodes,
            local_edges,
            refs,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// One-line span anchor (RACF is line-oriented; byte offsets within a card are not tracked).
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

/// Strip surrounding single quotes from an RACF operand value (e.g. `'PROD.PAY.**'` → `PROD.PAY.**`).
/// Returns the input unchanged if it is not quoted.
fn strip_quotes(s: &str) -> String {
    if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn extract(text: &str) -> Extraction {
        RacfExtractor::new()
            .extract(&SourceFile {
                path: "RACFCMDS.racf".to_string(),
                language: Language::new("racf"),
                text: text.to_string(),
            })
            .expect("racf extract")
    }

    // The canonical full-snippet test required by the task spec.
    #[test]
    fn canonical_snippet_nodes_and_refs() {
        let ex = extract(
            "ADDGROUP PAYROLL\n\
             ADDUSER JSMITH\n\
             RDEFINE FACILITY BPX.SUPERUSER\n\
             ADDSD 'PROD.PAY.**'\n\
             PERMIT 'PROD.PAY.**' CLASS(DATASET) ID(PAYROLL) ACCESS(READ)\n\
             CONNECT JSMITH GROUP(PAYROLL)\n",
        );

        // --- node assertions ---

        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "PAYROLL" && n.kind == NodeKind::Other("racf_group".to_string())),
            "expected a racf_group node for PAYROLL; nodes: {:?}",
            ex.nodes.iter().map(|n| (&n.name, &n.kind)).collect::<Vec<_>>()
        );

        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "JSMITH" && n.kind == NodeKind::Other("racf_user".to_string())),
            "expected a racf_user node for JSMITH"
        );

        assert!(
            ex.nodes.iter().any(|n| n.name == "BPX.SUPERUSER"
                && n.kind == NodeKind::Other("racf_profile".to_string())
                && n.signature.as_deref() == Some("FACILITY")),
            "expected a racf_profile node for BPX.SUPERUSER with signature=FACILITY"
        );

        assert!(
            ex.nodes.iter().any(|n| n.name == "PROD.PAY.**"
                && n.kind == NodeKind::Other("racf_dataset_profile".to_string())),
            "expected a racf_dataset_profile node for PROD.PAY.**"
        );

        // --- ref assertions ---

        let permits = ex
            .refs
            .iter()
            .find(|r| {
                r.kind == EdgeKind::Other("permits".to_string()) && r.raw_name == "PROD.PAY.**"
            })
            .expect("expected a 'permits' ref from PAYROLL → PROD.PAY.**");

        // The from should be the PAYROLL synthetic id.
        let payroll_id = Symbol::synthetic("racf", "PAYROLL").id();
        assert_eq!(
            permits.from, payroll_id,
            "permits ref 'from' must be the PAYROLL symbol"
        );

        let member_of = ex
            .refs
            .iter()
            .find(|r| r.kind == EdgeKind::Other("member_of".to_string()) && r.raw_name == "PAYROLL")
            .expect("expected a 'member_of' ref from JSMITH → PAYROLL");

        let jsmith_id = Symbol::synthetic("racf", "JSMITH").id();
        assert_eq!(
            member_of.from, jsmith_id,
            "member_of ref 'from' must be the JSMITH symbol"
        );
    }

    #[test]
    fn blank_and_comment_lines_skipped() {
        let ex = extract(
            "\n\
             * This is a RACF comment\n\
             /* block-style comment */\n\
             \n",
        );
        assert!(
            ex.nodes.is_empty(),
            "comments and blanks must yield no nodes"
        );
        assert!(ex.refs.is_empty(), "comments and blanks must yield no refs");
    }

    #[test]
    fn rdefine_sets_signature_to_class() {
        let ex = extract("RDEFINE OPERCMDS MVS.MCSOPER.OPER1\n");
        let node = ex
            .nodes
            .iter()
            .find(|n| n.name == "MVS.MCSOPER.OPER1")
            .expect("expected RDEFINE node");
        assert_eq!(node.kind, NodeKind::Other("racf_profile".to_string()));
        assert_eq!(node.signature.as_deref(), Some("OPERCMDS"));
    }

    #[test]
    fn addsd_strips_quotes() {
        // Both quoted and unquoted forms must yield the same bare name.
        let ex_quoted = extract("ADDSD 'PROD.PAYROLL.**'\n");
        let ex_bare = extract("ADDSD PROD.PAYROLL.**\n");
        let name_q = &ex_quoted.nodes[0].name;
        let name_b = &ex_bare.nodes[0].name;
        assert_eq!(name_q, "PROD.PAYROLL.**");
        assert_eq!(name_b, "PROD.PAYROLL.**");
        assert_eq!(
            name_q, name_b,
            "quoted and unquoted ADDSD must yield the same name"
        );
    }

    #[test]
    fn permit_operands_any_order() {
        // ACCESS comes before ID — operand parser must be order-independent.
        let ex = extract("PERMIT MYRES ACCESS(UPDATE) CLASS(FACILITY) ID(GRPA)\n");
        let r = ex
            .refs
            .iter()
            .find(|r| r.kind == EdgeKind::Other("permits".to_string()))
            .expect("expected a permits ref");
        assert_eq!(r.raw_name, "MYRES");
        let grpa_id = Symbol::synthetic("racf", "GRPA").id();
        assert_eq!(r.from, grpa_id);
    }

    #[test]
    fn connect_without_group_keyword_skipped() {
        // A CONNECT with no GROUP(...) operand produces no ref (graceful skip).
        let ex = extract("CONNECT JSMITH OWNER(ADMGRP)\n");
        assert!(
            ex.refs
                .iter()
                .all(|r| r.kind != EdgeKind::Other("member_of".to_string())),
            "CONNECT without GROUP(...) must not emit a member_of ref"
        );
    }

    #[test]
    fn languages_returns_racf() {
        assert_eq!(
            RacfExtractor::new().languages(),
            vec![Language::new("racf")]
        );
    }

    #[test]
    fn unknown_verbs_skipped() {
        let ex = extract(
            "SETROPTS AUDIT(DATASET)\n\
             RALTER FACILITY BPX.SUPERUSER AUDIT(SUCCESS(READ))\n\
             ADDGROUP FINANCE\n",
        );
        // Only ADDGROUP should produce a node; the two unknown verbs are skipped.
        assert_eq!(ex.nodes.len(), 1, "only ADDGROUP should produce a node");
        assert_eq!(ex.nodes[0].name, "FINANCE");
    }

    #[test]
    fn strip_quotes_helper() {
        assert_eq!(strip_quotes("'PROD.LIB'"), "PROD.LIB");
        assert_eq!(strip_quotes("PROD.LIB"), "PROD.LIB");
        assert_eq!(strip_quotes("''"), "");
        assert_eq!(strip_quotes("'"), "'"); // lone quote — returned as-is
    }
}
