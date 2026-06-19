//! CLIPS/Jess `.clp` extractor — **depth-counting S-expression parser, NO tree-sitter grammar**.
//!
//! No usable tree-sitter grammar exists for CLIPS on crates.io. CLIPS is a Lisp-style
//! S-expression language regular enough for a depth-counting line parser — the same
//! grammar-less pattern already used by the JCL and HLASM extractors. Extracts:
//!
//! - `(defmodule NAME …)` → [`NodeKind::RuleSet`] node.
//! - `(defrule NAME …)`   → [`NodeKind::Rule`] node, with optional `Contains` edge from the
//!   current module, plus [`NodeKind::Condition`] nodes for each LHS pattern and
//!   [`NodeKind::Action`] nodes for each RHS expression.
//! - `(deftemplate NAME …)` → [`NodeKind::Fact`] node (fact template / working-memory type).
//!   Optionally emits a call ref so resolvers can wire rule actions to the templates they assert.
//!
//! The LHS/RHS split is detected by the `=>` separator at depth 1 inside a `defrule` body.
//!
//! # W15.7 — CLIPS/Jess `.clp` extractor (closes GitHub #20).

use wicked_estate_core::{
    Descriptor, Edge, EdgeKind, Extraction, Extractor, Language, Location, Node, NodeKind,
    ResolutionTier, Result, SourceFile, Span, Suffix, Symbol, SymbolId, UnresolvedRef,
};

/// Extractor for CLIPS/Jess rule files. Grammar-less; parses `(def…)` forms by depth counting.
pub struct ClipsExtractor;

impl ClipsExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClipsExtractor {
    fn default() -> Self {
        Self::new()
    }
}

// ── Internal parser state ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Scanning the top level for a `(def…` keyword.
    TopLevel,
    /// Inside a `defrule` body, collecting LHS conditions (before `=>`).
    RuleLhs,
    /// Inside a `defrule` body, collecting RHS actions (after `=>`).
    RuleRhs,
    /// Inside a `deftemplate` or `defmodule` body — we only want the name.
    Other,
}

/// Minimal per-`defrule` accumulation record.
struct RuleCtx {
    rule_sym: SymbolId,
    /// Conditions accumulated so far (line-index + first-token text).
    conditions: Vec<(usize, String)>,
    /// Actions accumulated so far (line-index + first-token text).
    actions: Vec<(usize, String)>,
}

// ── Symbol helpers ─────────────────────────────────────────────────────────────

/// Stable global SymbolId for a CLIPS construct:
/// `clips . . . <module_path>/<name><suffix_sigil>`
fn clips_sym(module: &str, name: &str, suffix: Suffix) -> SymbolId {
    Symbol::global(
        "clips",
        None,
        vec![
            Descriptor::new(module, Suffix::Namespace),
            Descriptor {
                name: name.to_string(),
                suffix,
                disambiguator: None,
            },
        ],
    )
    .id()
}

/// Derive the module path from the file path (strip extension, like the tree-sitter extractor).
fn module_path(path: &str) -> String {
    match path.rsplit_once('.') {
        Some((stem, _)) => stem.to_string(),
        None => path.to_string(),
    }
}

/// Build a zero-width span at the given 0-based line index.
fn line_span(line: usize) -> Span {
    let row = line as u32;
    Span {
        start_byte: 0,
        end_byte: 0,
        start_line: row,
        start_col: 0,
        end_line: row,
        end_col: 0,
    }
}

// ── Extractor impl ─────────────────────────────────────────────────────────────

impl Extractor for ClipsExtractor {
    fn languages(&self) -> Vec<Language> {
        vec![Language::new("clips")]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        let mut nodes: Vec<Node> = Vec::new();
        let mut local_edges: Vec<Edge> = Vec::new();
        let mut refs: Vec<UnresolvedRef> = Vec::new();

        let module = module_path(&file.path);
        let lang = Language::new("clips");

        // Current defmodule symbol (for Contains edges to defrule nodes).
        let mut current_module: Option<SymbolId> = None;

        // Parser state machine.
        let mut depth: i32 = 0; // paren nesting depth
        let mut phase = Phase::TopLevel;
        let mut rule_ctx: Option<RuleCtx> = None;
        // depth at which the current top-level form opened (always 1 for top-level forms).
        let mut form_open_depth: i32 = 0;

        // We also need to track per-condition/action depth so we can tell when a nested
        // S-expression closes back to the `defrule` body level (depth == form_open_depth + 1).
        let mut inner_depth: i32 = 0; // depth relative to form_open_depth when inside a form

        // Pending keyword + name extraction: after seeing `(defXXX` we want the next token.
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Expect {
            Nothing,
            ModuleName,
            RuleName,
            TemplateName,
        }
        let mut expect = Expect::Nothing;
        let mut pending_line: usize = 0; // line where the keyword was seen

        for (line_idx, raw_line) in file.text.lines().enumerate() {
            // Strip line comments: `;` outside a string starts a comment.
            let line = strip_comment(raw_line);

            let mut chars = line.chars().peekable();
            while let Some(ch) = chars.next() {
                match ch {
                    '(' => {
                        depth += 1;

                        // Check for a `=>` that is NOT inside parens at rule body level.
                        // (handled in the token branch below — `=>` cannot follow `(`)

                        if phase == Phase::TopLevel && depth == 1 {
                            // Start of a new top-level form.
                            form_open_depth = depth;
                            // Consume any whitespace and peek at the keyword.
                            let keyword = collect_token(&mut chars);
                            pending_line = line_idx;
                            match keyword.as_str() {
                                "defmodule" => {
                                    expect = Expect::ModuleName;
                                    phase = Phase::Other;
                                    inner_depth = 0;
                                }
                                "defrule" => {
                                    expect = Expect::RuleName;
                                    phase = Phase::RuleLhs;
                                    inner_depth = 0;
                                }
                                "deftemplate" => {
                                    expect = Expect::TemplateName;
                                    phase = Phase::Other;
                                    inner_depth = 0;
                                }
                                _ => {
                                    phase = Phase::Other;
                                    inner_depth = 0;
                                }
                            }
                        } else if phase != Phase::TopLevel {
                            inner_depth += 1;

                            // A `(` at depth == form_open_depth + 1 (inner_depth == 1) inside a
                            // defrule starts a new condition (LHS) or action (RHS) form.
                            if inner_depth == 1 {
                                if phase == Phase::RuleLhs {
                                    // Peek at the first token: if it looks like a fact pattern or
                                    // test, record it as a condition.  We collect the name lazily.
                                    let cond_text = collect_token(&mut chars);
                                    if let Some(ctx) = &mut rule_ctx {
                                        ctx.conditions.push((line_idx, cond_text));
                                    }
                                } else if phase == Phase::RuleRhs {
                                    let act_text = collect_token(&mut chars);
                                    if let Some(ctx) = &mut rule_ctx {
                                        ctx.actions.push((line_idx, act_text));
                                    }
                                }
                            }
                        }
                    }
                    ')' => {
                        depth -= 1;

                        if depth < form_open_depth && phase != Phase::TopLevel {
                            // The top-level form closed — flush the rule context if any.
                            if let Some(ctx) = rule_ctx.take() {
                                flush_rule(
                                    ctx,
                                    &module,
                                    &lang,
                                    &file.path,
                                    &current_module,
                                    &mut nodes,
                                    &mut local_edges,
                                );
                            }
                            phase = Phase::TopLevel;
                            inner_depth = 0;
                        } else if phase != Phase::TopLevel {
                            inner_depth -= 1;
                        }
                    }
                    _ => {
                        // Collect a non-paren token (name, `=>`, etc.) for keyword/name handling.
                        if ch.is_whitespace() {
                            continue;
                        }
                        // Build the full token starting with `ch`.
                        let mut tok = String::new();
                        tok.push(ch);
                        while let Some(&nc) = chars.peek() {
                            if nc.is_whitespace() || nc == '(' || nc == ')' {
                                break;
                            }
                            tok.push(nc);
                            chars.next();
                        }

                        // Expect a name token after a def keyword.
                        if expect != Expect::Nothing {
                            let name = tok.trim_matches('"').to_string();
                            match expect {
                                Expect::ModuleName => {
                                    let sym = clips_sym(&module, &name, Suffix::Type);
                                    let node = Node::new(
                                        sym.clone(),
                                        NodeKind::RuleSet,
                                        name.clone(),
                                        lang.clone(),
                                        Location::new(&file.path, line_span(pending_line)),
                                    );
                                    nodes.push(node);
                                    current_module = Some(sym);
                                }
                                Expect::RuleName => {
                                    let sym = clips_sym(&module, &name, Suffix::Term);
                                    let node = Node::new(
                                        sym.clone(),
                                        NodeKind::Rule,
                                        name.clone(),
                                        lang.clone(),
                                        Location::new(&file.path, line_span(pending_line)),
                                    );
                                    nodes.push(node);
                                    // Contains edge: module → rule (if a module is in scope).
                                    if let Some(mod_sym) = &current_module {
                                        local_edges.push(Edge::new(
                                            mod_sym.clone(),
                                            sym.clone(),
                                            EdgeKind::Governs,
                                            ResolutionTier::Parsed,
                                            "clips-extractor",
                                        ));
                                    }
                                    rule_ctx = Some(RuleCtx {
                                        rule_sym: sym,
                                        conditions: Vec::new(),
                                        actions: Vec::new(),
                                    });
                                }
                                Expect::TemplateName => {
                                    let sym = clips_sym(&module, &name, Suffix::Type);
                                    let node = Node::new(
                                        sym.clone(),
                                        NodeKind::Fact,
                                        name.clone(),
                                        lang.clone(),
                                        Location::new(&file.path, line_span(pending_line)),
                                    );
                                    nodes.push(node);
                                    // Emit an unresolved ref so resolvers can wire rule actions
                                    // to this template when they appear in assert/retract calls.
                                    refs.push(UnresolvedRef::new(
                                        sym,
                                        name,
                                        EdgeKind::InvokedBy,
                                        Location::new(&file.path, line_span(pending_line)),
                                    ));
                                }
                                Expect::Nothing => {}
                            }
                            expect = Expect::Nothing;
                        } else if tok == "=>" && phase == Phase::RuleLhs && inner_depth == 0 {
                            // LHS → RHS separator at rule body level (depth == form_open_depth + 0
                            // means we're directly inside the defrule form, not in a nested sexp).
                            // inner_depth tracks nesting *inside* the form; at 0 we're at the
                            // defrule body level where `=>` is valid.
                            phase = Phase::RuleRhs;
                        }
                    }
                }
            }
        }

        // Flush any open rule context (file ended without closing paren — unlikely but defensive).
        if let Some(ctx) = rule_ctx.take() {
            flush_rule(
                ctx,
                &module,
                &lang,
                &file.path,
                &current_module,
                &mut nodes,
                &mut local_edges,
            );
        }

        Ok(Extraction {
            nodes,
            local_edges,
            refs,
        })
    }
}

// ── Flush helpers ──────────────────────────────────────────────────────────────

/// Emit Condition + Action child nodes for a completed `defrule` context.
fn flush_rule(
    ctx: RuleCtx,
    module: &str,
    lang: &Language,
    path: &str,
    _current_module: &Option<SymbolId>,
    nodes: &mut Vec<Node>,
    local_edges: &mut Vec<Edge>,
) {
    let rule_sym = &ctx.rule_sym;

    // Emit one Condition node per LHS pattern.
    for (i, (line_idx, cond_name)) in ctx.conditions.into_iter().enumerate() {
        let name = if cond_name.is_empty() {
            format!("cond_{i}")
        } else {
            cond_name.clone()
        };
        let sym = Symbol::global(
            "clips",
            None,
            vec![
                Descriptor::new(module, Suffix::Namespace),
                Descriptor {
                    name: format!(
                        "{}::cond_{i}",
                        rule_sym
                            .as_str()
                            .rsplit_once('/')
                            .map(|(_, n)| n)
                            .unwrap_or(rule_sym.as_str())
                    ),
                    suffix: Suffix::Term,
                    disambiguator: None,
                },
            ],
        )
        .id();
        let node = Node::new(
            sym.clone(),
            NodeKind::Condition,
            name,
            lang.clone(),
            Location::new(path, line_span(line_idx)),
        );
        nodes.push(node);
        local_edges.push(Edge::new(
            rule_sym.clone(),
            sym,
            EdgeKind::Evaluates,
            ResolutionTier::Parsed,
            "clips-extractor",
        ));
    }

    // Emit one Action node per RHS expression.
    for (i, (line_idx, act_name)) in ctx.actions.into_iter().enumerate() {
        let name = if act_name.is_empty() {
            format!("action_{i}")
        } else {
            act_name.clone()
        };
        let sym = Symbol::global(
            "clips",
            None,
            vec![
                Descriptor::new(module, Suffix::Namespace),
                Descriptor {
                    name: format!(
                        "{}::act_{i}",
                        rule_sym
                            .as_str()
                            .rsplit_once('/')
                            .map(|(_, n)| n)
                            .unwrap_or(rule_sym.as_str())
                    ),
                    suffix: Suffix::Term,
                    disambiguator: None,
                },
            ],
        )
        .id();
        let node = Node::new(
            sym.clone(),
            NodeKind::Action,
            name,
            lang.clone(),
            Location::new(path, line_span(line_idx)),
        );
        nodes.push(node);
        local_edges.push(Edge::new(
            rule_sym.clone(),
            sym,
            EdgeKind::Produces,
            ResolutionTier::Parsed,
            "clips-extractor",
        ));
    }
}

// ── Lexer helpers ──────────────────────────────────────────────────────────────

/// Strip a line comment (everything from `;` onward, outside strings).
fn strip_comment(line: &str) -> &str {
    // Simple heuristic: find the first `;` not inside a double-quoted string.
    let mut in_string = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => in_string = !in_string,
            ';' if !in_string => return &line[..i],
            _ => {}
        }
    }
    line
}

/// Consume whitespace from `chars` then collect a token until the next whitespace or paren.
fn collect_token(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    // Skip leading whitespace.
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    // Collect until whitespace or paren.
    let mut tok = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() || c == '(' || c == ')' {
            break;
        }
        tok.push(c);
        chars.next();
    }
    tok
}
