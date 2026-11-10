//! Format-aware secret redaction of a file's text BEFORE it is parsed (wicked-estate#405,
//! DES-S7 §5).
//!
//! [`redact_source`] returns text of the SAME byte length with secret values masked, so every
//! downstream product of the index (nodes, signatures, the full-text index, the stored
//! `content`, unresolved-ref names) derives from masked text and byte spans stay valid. It is a
//! pure function of `(path, text)`: the migration of an existing store re-runs it over stored
//! content and gets exactly what a fresh index writes.
//!
//! Policy (rules in `secret-rules.toml`):
//! - every file: known token shapes and URL credentials (C3, [`core_redact::token_shape_ranges`]);
//! - config formats (JSON, YAML, TOML, HCL/tfvars, properties, INI, XML, Dockerfile): DEFAULT-DENY.
//!   A value is kept only when it is a boolean/null, a reference (C5), or sits in a trusted field
//!   (C1) and passes its class's shape rules (C2) and C3; a C4-denied key masks it regardless;
//! - code: a string literal is masked when C3 flags it or when it is bound to a C4-denied name
//!   (`const password = "..."`, `{ apiKey: "..." }`, `process.env.TOKEN || "..."`). This is
//!   best-effort: a secret under an innocent name in code is NOT detected (the documented residual).

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use wicked_estate_core::redact as core_redact;

// ── Rules ──────────────────────────────────────────────────────────────────────────────────────

const RULES_SRC: &str = include_str!("../secret-rules.toml");

#[derive(Deserialize)]
struct Rules {
    exclude: Exclude,
    references: References,
    url: UrlRules,
    env_suffix: EnvSuffix,
    code: CodeRules,
    default_deny: DefaultDeny,
    unruled: Unruled,
    #[serde(default)]
    structural_deny: Vec<StructuralDeny>,
    #[serde(default)]
    format: Vec<Format>,
    #[serde(default)]
    trust: Vec<Trust>,
}

#[derive(Deserialize)]
struct Exclude {
    names: Vec<String>,
    allow: Vec<String>,
}

#[derive(Deserialize)]
struct References {
    patterns: Vec<String>,
}

#[derive(Deserialize)]
struct UrlRules {
    path_segments: Vec<String>,
}

#[derive(Deserialize)]
struct EnvSuffix {
    url: Vec<String>,
    host: Vec<String>,
    port: Vec<String>,
    topic: Vec<String>,
}

#[derive(Deserialize)]
struct CodeRules {
    module_fields: Vec<String>,
    module_parent_kinds: Vec<String>,
    module_callees: Vec<String>,
    string_kind_substrings: Vec<String>,
    fragment_kind_substrings: Vec<String>,
    number_kind_substrings: Vec<String>,
    comment_kind_substrings: Vec<String>,
}

#[derive(Deserialize)]
struct Unruled {
    not_secret_keys: Vec<String>,
}

#[derive(Deserialize)]
struct DefaultDeny {
    grammars: Vec<String>,
    #[cfg_attr(not(test), allow(dead_code))]
    prose: Vec<String>,
}

#[derive(Deserialize)]
struct StructuralDeny {
    when: DenyWhen,
    paths: Vec<String>,
}

#[derive(Deserialize)]
struct DenyWhen {
    #[serde(default)]
    root_key: Option<String>,
    #[serde(default)]
    equals: Vec<String>,
    #[serde(default)]
    file_glob: Vec<String>,
}

#[derive(Deserialize)]
struct Format {
    grammar: String,
    #[serde(default)]
    pair: Vec<PairRule>,
    #[serde(default)]
    section: Vec<SectionRule>,
    #[serde(default)]
    list: Vec<String>,
    #[serde(default)]
    scalar: Vec<String>,
    #[serde(default)]
    keep_leaf: Vec<String>,
    #[serde(default)]
    numeric: Vec<String>,
    unkeyed: Unkeyed,
    #[serde(default)]
    key_from: Option<KeyFrom>,
    /// YAML tags whose scalar is a reference, not a value (CloudFormation `!Ref`, `!GetAtt`).
    #[serde(default)]
    reference_tags: Vec<String>,
    /// `parent/child` node-kind pairs whose whole subtree is masked (Dockerfile `COPY <<EOF`
    /// heredoc data is file content, not a command).
    #[serde(default)]
    deny_subtree: Vec<String>,
    /// Boolean / null node kinds that are walked as leaves (others are `keep_leaf`).
    #[serde(default)]
    bool_kinds: Vec<String>,
}

#[derive(Deserialize)]
struct PairRule {
    kind: String,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    key_child: Vec<String>,
    #[serde(default)]
    value_child: Vec<String>,
    #[serde(default)]
    value_index: Option<i64>,
}

#[derive(Deserialize)]
struct SectionRule {
    kind: String,
    key_child: Vec<String>,
    #[serde(default)]
    list: bool,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
enum Unkeyed {
    Mask,
    Command,
    C3,
}

#[derive(Deserialize)]
struct KeyFrom {
    separator: String,
}

#[derive(Deserialize)]
struct Trust {
    grammar: String,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    detect: Vec<String>,
    rows: Vec<TrustRow>,
    #[serde(default)]
    known_hosts: Vec<String>,
}

#[derive(Deserialize)]
struct TrustRow {
    paths: Vec<String>,
    class: Class,
    #[serde(default)]
    key_sibling: Option<String>,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
enum Class {
    Plain,
    Identity,
    Integrity,
    Command,
    Env,
    Url,
    Host,
    Port,
    /// A topic / queue / subject name: kept only when equal to a channel known in the same label;
    /// X0 knows no channels, so it is masked.
    Topic,
}

struct Compiled {
    rules: Rules,
    references: Vec<Regex>,
    path_segments: Regex,
    integrity: Regex,
    exclude: Vec<Regex>,
    allow: Vec<Regex>,
}

static COMPILED: LazyLock<Compiled> = LazyLock::new(|| {
    let rules: Rules = toml::from_str(RULES_SRC).expect("secret-rules.toml parses");
    let references = rules
        .references
        .patterns
        .iter()
        .map(|p| Regex::new(p).expect("reference pattern compiles"))
        .collect();
    let path_segments = Regex::new(&format!("^(?:{})$", rules.url.path_segments.join("|")))
        .expect("path segment allow-list compiles");
    let exclude = rules.exclude.names.iter().map(|g| glob_regex(g)).collect();
    let allow = rules.exclude.allow.iter().map(|g| glob_regex(g)).collect();
    Compiled {
        rules,
        references,
        path_segments,
        integrity: Regex::new(r"^(?:sha1|sha256|sha384|sha512)-[A-Za-z0-9+/]+=*$")
            .expect("integrity"),
        exclude,
        allow,
    }
});

/// A file-name glob (`*` any run, `?` one char) as a case-insensitive anchored regex.
fn glob_regex(glob: &str) -> Regex {
    let mut re = String::from("(?i)^");
    for c in glob.chars() {
        match c {
            '*' => re.push_str(".*"),
            '?' => re.push('.'),
            c => re.push_str(&regex::escape(&c.to_string())),
        }
    }
    re.push('$');
    Regex::new(&re).expect("glob compiles")
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn name_matches(globs: &[String], name: &str) -> bool {
    globs.iter().any(|g| glob_regex(g).is_match(name))
}

/// A short digest of the redaction rules, recorded with the store's redaction epoch so a rules
/// change is visible (DES-S2 §6 rules digest).
pub fn rules_digest() -> String {
    format!("{:016x}", xxh3(RULES_SRC.as_bytes()))
}

fn xxh3(b: &[u8]) -> u64 {
    // FNV-1a: no new dependency for a 16-hex digest of a compile-time constant.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &x in b {
        h ^= u64::from(x);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// True when `path` must never be indexed even if committed (`.env`, `.npmrc`, private keys,
/// kubeconfigs, credential files). `.env.example` / `.env.sample` / `.env.template` are allowed.
pub fn is_excluded(path: &str) -> bool {
    let c = &*COMPILED;
    let name = file_name(path);
    if c.allow.iter().any(|r| r.is_match(name)) {
        return false;
    }
    c.exclude.iter().any(|r| r.is_match(name))
}

// ── Entry point ────────────────────────────────────────────────────────────────────────────────

/// Every non-whitespace byte of `text` masked (same length, line breaks and indentation kept):
/// for text of a file that must never have been stored (a credential file an older build indexed).
pub fn mask_all(text: &str) -> String {
    let mut buf = text.as_bytes().to_vec();
    let mut i = 0;
    while i < buf.len() {
        if buf[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let s = i;
        while i < buf.len() && !buf[i].is_ascii_whitespace() {
            i += 1;
        }
        core_redact::mask_text(&mut buf, s, i);
    }
    String::from_utf8(buf).expect("ASCII masking over whole runs keeps UTF-8 valid")
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MaskKind {
    Text,
    Numeric,
}

/// Redact `text` (the content of `path`) per the policy above. The result has exactly
/// `text.len()` bytes and the same line breaks. Returns `text` unchanged when nothing matched.
pub fn redact_source(path: &str, text: &str) -> String {
    let mut masks: Vec<(usize, usize, MaskKind)> = core_redact::token_shape_ranges(text)
        .into_iter()
        .map(|(s, e)| (s, e, MaskKind::Text))
        .collect();
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    if let Some(grammar) = crate::treesitter::builtin_owner_of_ext(&ext) {
        let c = &*COMPILED;
        if let Some(fmt) = c.rules.format.iter().find(|f| f.grammar == grammar) {
            config_masks(path, text, grammar, fmt, &mut masks);
        } else if c.rules.default_deny.grammars.iter().any(|g| g == grammar) {
            code_masks(text, grammar, true, &mut masks);
        } else if code_prefilter(text) {
            code_masks(text, grammar, false, &mut masks);
        }
    } else {
        // No grammar this redactor can read (grammar-less mainframe extractors, runtime plugin
        // grammars, files only a drop-in rule targets): token shapes (above) plus the value of
        // every `name=value` / `name: value` whose name C4 denies (`PASSWORD=...` in JCL).
        kv_deny_masks(text, &mut masks);
    }
    apply(text, &masks)
}

static KV: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"([A-Za-z_][\w.\-]*)[ \t]*[:=][ \t]*("[^"\n]*"|'[^'\n]*'|[^\s,;)'"]+)"#)
        .expect("kv compiles")
});

fn kv_deny_masks(text: &str, masks: &mut Vec<(usize, usize, MaskKind)>) {
    for c in KV.captures_iter(text) {
        let (Some(k), Some(v)) = (c.get(1), c.get(2)) else {
            continue;
        };
        let not_secret = COMPILED
            .rules
            .unruled
            .not_secret_keys
            .iter()
            .any(|n| n.eq_ignore_ascii_case(k.as_str()));
        if !not_secret && core_redact::deny_key(k.as_str()) {
            let raw = v.as_str();
            let inner = unquote(raw);
            let off = (raw.len() - inner.len()) / 2;
            masks.push((
                v.start() + off,
                v.start() + off + inner.len(),
                MaskKind::Text,
            ));
        }
    }
}

fn apply(text: &str, masks: &[(usize, usize, MaskKind)]) -> String {
    if masks.is_empty() {
        return text.to_string();
    }
    let mut buf = text.as_bytes().to_vec();
    for &(s, e, kind) in masks {
        let (s, e) = widen(text, s, e);
        match kind {
            MaskKind::Text => core_redact::mask_text(&mut buf, s, e),
            MaskKind::Numeric => core_redact::mask_numeric(&mut buf, s, e),
        }
    }
    // Masks write ASCII over whole chars (ranges are widened to char boundaries), so this holds.
    String::from_utf8(buf).expect("same-length ASCII masking keeps UTF-8 valid")
}

fn widen(text: &str, s: usize, e: usize) -> (usize, usize) {
    let (mut s, mut e) = (s.min(text.len()), e.min(text.len()));
    while s > 0 && !text.is_char_boundary(s) {
        s -= 1;
    }
    while e < text.len() && !text.is_char_boundary(e) {
        e += 1;
    }
    (s, e)
}

fn parse(text: &str, grammar: &str) -> Option<tree_sitter::Tree> {
    let language = crate::treesitter::builtin_language(grammar)?;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language).ok()?;
    parser.parse(text, None)
}

// ── Config: the normalized document walk ───────────────────────────────────────────────────────

#[derive(Debug)]
struct Leaf {
    doc: usize,
    path: Vec<String>,
    /// Byte range of the value material (quotes excluded).
    start: usize,
    end: usize,
    numeric: bool,
    /// Index into `Walk::maps`: the direct scalar pairs of the mapping holding this leaf.
    map: Option<usize>,
    /// Key taken from a `KEY=value` list item (compose environment list form).
    split_key: Option<String>,
    /// The grammar's boolean or null node (never a string spelling `false`).
    boolean: bool,
    /// A tagged reference (`!Ref X`): kept like C5 references.
    reference: bool,
    /// Masked whatever its path says (a denied subtree).
    forced: bool,
    /// Text of the previous item in the same sequence (argv context: `["--password", "x"]`).
    prev: Option<String>,
}

#[derive(Default)]
struct Walk {
    leaves: Vec<Leaf>,
    /// (doc, path) of every key, for known-host collection and format detection.
    keys: Vec<(usize, Vec<String>)>,
    /// Direct scalar children of each mapping: key -> value text.
    maps: Vec<HashMap<String, String>>,
    docs: usize,
}

fn config_masks(
    path: &str,
    text: &str,
    grammar: &str,
    fmt: &Format,
    masks: &mut Vec<(usize, usize, MaskKind)>,
) {
    let src = text.as_bytes();
    let Some(tree) = parse(text, grammar) else {
        // Unparseable config: fail closed, mask every non-space run after a separator.
        fallback_mask_all_values(text, masks);
        return;
    };
    let mut w = Walk::default();
    let root = tree.root_node();
    // YAML streams hold several documents; every other grammar has one.
    let mut cursor = root.walk();
    let docs: Vec<tree_sitter::Node> = if root.kind() == "stream" {
        root.named_children(&mut cursor)
            .filter(|n| n.kind() == "document")
            .collect()
    } else {
        vec![root]
    };
    for (i, d) in docs.iter().enumerate() {
        let mut p = Vec::new();
        walk(*d, src, fmt, i, &mut p, None, &mut w);
    }
    w.docs = docs.len().max(1);
    // A tree with recovered errors: the regions the parser could not place (ERROR / MISSING
    // nodes, and anything outside the walked documents) fail closed, line by line.
    if root.has_error() {
        let mut covered: Vec<(usize, usize)> = docs
            .iter()
            .map(|d| (d.start_byte(), d.end_byte()))
            .collect();
        if root.kind() != "stream" {
            covered.clear();
        }
        let mut ranges = Vec::new();
        error_ranges(root, &mut ranges);
        let mut pos = 0;
        for (s, e) in covered
            .iter()
            .copied()
            .chain(std::iter::once((text.len(), text.len())))
        {
            if s > pos {
                ranges.push((pos, s));
            }
            pos = pos.max(e);
        }
        for (s, e) in ranges {
            let (s, e) = widen(text, s, e);
            let mut sub = Vec::new();
            fallback_mask_all_values(&text[s..e], &mut sub);
            masks.extend(sub.into_iter().map(|(a, b, k)| (a + s, b + s, k)));
        }
    }

    let c = &*COMPILED;
    let name = file_name(path);
    let scalar_at = |doc: usize, p: &[&str]| -> Option<String> {
        w.leaves
            .iter()
            .find(|l| l.doc == doc && l.path.iter().map(String::as_str).eq(p.iter().copied()))
            .map(|l| text[l.start..l.end].to_string())
    };
    // Per-document: the trust tables that apply, the structural-deny patterns, known hosts.
    let mut doc_trust: Vec<Vec<&Trust>> = vec![Vec::new(); w.docs];
    let mut doc_deny: Vec<Vec<&str>> = vec![Vec::new(); w.docs];
    let mut known_hosts: HashSet<String> = HashSet::new();
    for doc in 0..w.docs {
        for t in c.rules.trust.iter().filter(|t| t.grammar == grammar) {
            let by_file = t.files.is_empty() || name_matches(&t.files, name);
            let by_detect = t.detect.iter().all(|d| match d.split_once('=') {
                Some((k, v)) => scalar_at(doc, &key_path(k)).is_some_and(|s| s == v),
                None => {
                    let kp = key_path(d);
                    w.keys.iter().any(|(kd, k)| {
                        *kd == doc && k.iter().map(String::as_str).eq(kp.iter().copied())
                    })
                }
            });
            if by_file && by_detect {
                doc_trust[doc].push(t);
            }
        }
        for sd in &c.rules.structural_deny {
            let hit = match &sd.when.root_key {
                Some(k) => {
                    scalar_at(doc, &[k.as_str()]).is_some_and(|v| sd.when.equals.contains(&v))
                }
                None => !sd.when.file_glob.is_empty() && name_matches(&sd.when.file_glob, name),
            };
            if hit {
                doc_deny[doc].extend(sd.paths.iter().map(String::as_str));
            }
        }
        for t in &doc_trust[doc] {
            for pat in &t.known_hosts {
                if let Some(parent) = pat.strip_suffix("/*") {
                    // A mapping's keys (compose service names).
                    let pp = key_path(parent);
                    for (kd, k) in &w.keys {
                        if *kd == doc && k.len() == pp.len() + 1 && path_match(&pp, &k[..pp.len()])
                        {
                            known_hosts.insert(k[pp.len()].clone());
                        }
                    }
                } else if let Some(v) = scalar_at(doc, &key_path(pat)) {
                    known_hosts.insert(v);
                }
            }
        }
    }

    for leaf in &w.leaves {
        let value = &text[leaf.start..leaf.end];
        let path: Vec<&str> = leaf.path.iter().map(String::as_str).collect();
        let decision = decide(
            value,
            &path,
            leaf,
            &doc_trust[leaf.doc],
            &doc_deny[leaf.doc],
            fmt,
            &known_hosts,
            &w,
        );
        for (s, e) in decision {
            let kind = if leaf.numeric && s == 0 && e == value.len() {
                MaskKind::Numeric
            } else {
                MaskKind::Text
            };
            masks.push((leaf.start + s, leaf.start + e, kind));
        }
    }
}

/// `/a/b/[]` -> ["a", "b", "[]"].
fn key_path(p: &str) -> Vec<&str> {
    p.split('/').filter(|s| !s.is_empty()).collect()
}

/// Match a concrete path against a pattern: `*` one segment, `**` any number (incl. zero),
/// `[]` a list item. Concrete segments compare exactly.
fn path_match(pat: &[&str], path: &[String]) -> bool {
    match pat.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|i| path_match(rest, &path[i..])),
        Some((seg, rest)) => match path.split_first() {
            None => false,
            Some((p, prest)) => {
                let ok = match *seg {
                    "*" => p != "[]",
                    s => s == p,
                };
                ok && path_match(rest, prest)
            }
        },
    }
}

fn pattern_match(pattern: &str, path: &[String]) -> bool {
    path_match(&key_path(pattern), path)
}

fn node_text<'a>(n: tree_sitter::Node, src: &'a [u8]) -> &'a str {
    n.utf8_text(src).unwrap_or("")
}

/// Strip surrounding quotes from a key's text.
fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ["\"\"\"", "'''", "\"", "'", "`"] {
        if s.len() >= 2 * q.len() && s.starts_with(q) && s.ends_with(q) {
            return &s[q.len()..s.len() - q.len()];
        }
    }
    s
}

fn find_child<'t>(n: tree_sitter::Node<'t>, kinds: &[String]) -> Option<tree_sitter::Node<'t>> {
    let mut c = n.walk();
    n.named_children(&mut c)
        .find(|ch| kinds.iter().any(|k| k == ch.kind()))
}

fn walk(
    node: tree_sitter::Node,
    src: &[u8],
    fmt: &Format,
    doc: usize,
    path: &mut Vec<String>,
    map: Option<usize>,
    w: &mut Walk,
) {
    let kind = node.kind();
    if let Some(rule) = fmt.pair.iter().find(|r| r.kind == kind) {
        pair(node, rule, src, fmt, doc, path, map, w);
        return;
    }
    if let Some(rule) = fmt.section.iter().find(|r| r.kind == kind) {
        let key_node = find_child(node, &rule.key_child);
        let segs: Vec<String> = key_node.map(|k| key_segments(k, src)).unwrap_or_default();
        let depth = path.len();
        path.extend(segs.iter().cloned());
        if rule.list {
            path.push("[]".into());
        }
        w.keys.push((doc, path.clone()));
        let m = new_map(node, src, fmt, w);
        let mut c = node.walk();
        for ch in node.named_children(&mut c) {
            if key_node.is_some_and(|k| k.id() == ch.id()) {
                continue;
            }
            walk(ch, src, fmt, doc, path, Some(m), w);
        }
        path.truncate(depth);
        return;
    }
    let parent_kind = node.parent().map(|p| p.kind()).unwrap_or("");
    if fmt.deny_subtree.iter().any(|d| {
        d.split_once('/')
            .is_some_and(|(p, k)| p == parent_kind && k == kind)
    }) {
        // Mask the data children; the delimiters (`keep_leaf`, e.g. `heredoc_end`) stay, so the
        // heredoc still closes and the instructions after it keep their structure.
        let mut c = node.walk();
        let kids: Vec<tree_sitter::Node> = node.named_children(&mut c).collect();
        let targets = if kids.is_empty() { vec![node] } else { kids };
        for k in targets {
            if fmt.keep_leaf.iter().any(|x| x == k.kind()) {
                continue;
            }
            if let Some(mut l) = make_leaf(k, src, fmt, doc, path, map, None, false) {
                l.forced = true;
                w.leaves.push(l);
            }
        }
        return;
    }
    if fmt.scalar.iter().any(|k| k == kind) {
        leaf(node, src, fmt, doc, path, map, None, w);
        return;
    }
    if fmt.list.iter().any(|l| l == kind) {
        let mut c = node.walk();
        for ch in node.named_children(&mut c) {
            if fmt.keep_leaf.iter().any(|k| k == ch.kind()) {
                continue;
            }
            path.push("[]".into());
            item(ch, src, fmt, doc, path, map, w);
            path.pop();
        }
        return;
    }
    let mut c = node.walk();
    let children: Vec<tree_sitter::Node> = node.named_children(&mut c).collect();
    let has_pairs = children
        .iter()
        .any(|ch| fmt.pair.iter().any(|r| r.kind == ch.kind()));
    let map = if has_pairs {
        Some(new_map(node, src, fmt, w))
    } else {
        map
    };
    if node.child_count() == 0 {
        leaf(node, src, fmt, doc, path, map, None, w);
        return;
    }
    if children.is_empty() {
        // An empty container (`{}`, `[]`) holds no value: its children are only delimiters, and
        // masking them broke the document (`"": {}` became `"": RE`, so a JSON manifest or
        // lockfile no longer parsed). Judged on the node's whole text, not its visible children:
        // a grammar's content token can be hidden (a Dockerfile exec-form `"x"` shows only its
        // quotes), and that is a value.
        let delimiters_only = node_text(node, src)
            .chars()
            .all(|c| c.is_whitespace() || "{}[]()\"'".contains(c));
        if !delimiters_only {
            leaf(node, src, fmt, doc, path, map, None, w);
        }
        return;
    }
    for ch in children {
        walk(ch, src, fmt, doc, path, map, w);
    }
}

/// A list item: a `KEY=value` scalar (compose environment list form) is split at the separator.
fn item(
    node: tree_sitter::Node,
    src: &[u8],
    fmt: &Format,
    doc: usize,
    path: &mut Vec<String>,
    map: Option<usize>,
    w: &mut Walk,
) {
    if let Some(kf) = &fmt.key_from {
        // Unwrap single-child wrappers (YAML `block_sequence_item` > `flow_node` > scalar).
        let mut node = node;
        while !fmt.scalar.iter().any(|k| k == node.kind()) && node.named_child_count() == 1 {
            match node.named_child(0) {
                Some(ch) => node = ch,
                None => break,
            }
        }
        let t = node_text(node, src);
        let is_scalar = !t.contains('\n') && !t.trim_start().starts_with(['{', '[', '-']);
        if is_scalar {
            let inner = unquote(t);
            if let Some((k, _)) = inner.split_once(kf.separator.as_str()) {
                if !k.is_empty()
                    && k.chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-')
                {
                    let start = node.start_byte() + (t.len() - t.trim_start().len());
                    let off = t.trim_start().find(inner).unwrap_or(0);
                    let vstart = start + off + k.len() + kf.separator.len();
                    let vend = start + off + inner.len();
                    w.keys.push((doc, path.clone()));
                    w.leaves.push(Leaf {
                        doc,
                        path: path.clone(),
                        start: vstart,
                        end: vend.max(vstart),
                        numeric: false,
                        map,
                        split_key: Some(k.to_string()),
                        boolean: false,
                        reference: false,
                        forced: false,
                        prev: None,
                    });
                    return;
                }
            }
        }
    }
    walk(node, src, fmt, doc, path, map, w);
}

/// The path segments a key node names. A TOML `dotted_key` is one segment per component; a quoted
/// component is ONE segment even when it contains dots (`"project.name" = 1` is a single key,
/// never `/project/name`). Every other key node is one segment.
fn key_segments(k: tree_sitter::Node, src: &[u8]) -> Vec<String> {
    if k.kind() == "dotted_key" {
        let mut c = k.walk();
        let parts: Vec<tree_sitter::Node> = k.named_children(&mut c).collect();
        return parts
            .into_iter()
            .flat_map(|p| key_segments(p, src))
            .collect();
    }
    vec![unquote(node_text(k, src)).to_string()]
}

fn pair_parts<'t>(
    node: tree_sitter::Node<'t>,
    rule: &PairRule,
    src: &[u8],
) -> (Option<Vec<String>>, Vec<tree_sitter::Node<'t>>) {
    let key_node = if let Some(f) = &rule.key {
        node.child_by_field_name(f)
    } else {
        find_child(node, &rule.key_child)
    };
    let key = key_node.map(|k| key_segments(k, src));
    let values: Vec<tree_sitter::Node> = if let Some(f) = &rule.value {
        node.child_by_field_name(f).into_iter().collect()
    } else if !rule.value_child.is_empty() {
        find_child(node, &rule.value_child).into_iter().collect()
    } else if let Some(i) = rule.value_index {
        let mut c = node.walk();
        let named: Vec<tree_sitter::Node> = node.named_children(&mut c).collect();
        let idx = if i < 0 { named.len() as i64 + i } else { i };
        named
            .get(idx.max(0) as usize)
            .copied()
            .into_iter()
            .collect()
    } else {
        Vec::new()
    };
    (key, values)
}

#[allow(clippy::too_many_arguments)]
fn pair(
    node: tree_sitter::Node,
    rule: &PairRule,
    src: &[u8],
    fmt: &Format,
    doc: usize,
    path: &mut Vec<String>,
    map: Option<usize>,
    w: &mut Walk,
) {
    let (key, values) = pair_parts(node, rule, src);
    let depth = path.len();
    match key {
        Some(k) => {
            path.extend(k);
            w.keys.push((doc, path.clone()));
        }
        None => path.push("?".into()),
    }
    for v in values {
        // A key node never doubles as a value (TOML/HCL `value_index = -1` on a bare pair).
        walk(v, src, fmt, doc, path, map, w);
    }
    path.truncate(depth);
}

fn new_map(node: tree_sitter::Node, src: &[u8], fmt: &Format, w: &mut Walk) -> usize {
    let mut m = HashMap::new();
    let mut c = node.walk();
    for ch in node.named_children(&mut c) {
        if let Some(rule) = fmt.pair.iter().find(|r| r.kind == ch.kind()) {
            let (key, values) = pair_parts(ch, rule, src);
            if let (Some(k), Some(v)) = (key, values.first()) {
                m.insert(k.join("."), unquote(node_text(*v, src)).to_string());
            }
        }
    }
    w.maps.push(m);
    w.maps.len() - 1
}

#[allow(clippy::too_many_arguments)]
fn leaf(
    node: tree_sitter::Node,
    src: &[u8],
    fmt: &Format,
    doc: usize,
    path: &[String],
    map: Option<usize>,
    split_key: Option<String>,
    w: &mut Walk,
) {
    let kind = node.kind();
    if !node.is_named() || fmt.keep_leaf.iter().any(|k| k == kind) || node.is_extra() {
        return;
    }
    w.leaves
        .extend(make_leaf(node, src, fmt, doc, path, map, split_key, true));
}

/// The value material of `node` as a [`Leaf`] (quotes and surrounding whitespace excluded; a
/// YAML `block_scalar` keeps its `|` / `>` header line).
#[allow(clippy::too_many_arguments)]
fn make_leaf(
    node: tree_sitter::Node,
    src: &[u8],
    fmt: &Format,
    doc: usize,
    path: &[String],
    map: Option<usize>,
    split_key: Option<String>,
    trim_quotes: bool,
) -> Option<Leaf> {
    let kind = node.kind();
    let t = node_text(node, src);
    if t.trim().is_empty() {
        return None;
    }
    let lead = t.len() - t.trim_start().len();
    let trimmed = t.trim();
    let inner = if trim_quotes {
        unquote(trimmed)
    } else {
        trimmed
    };
    let q = if inner.len() == trimmed.len() {
        0
    } else {
        (trimmed.len() - inner.len()) / 2
    };
    let header = if kind == "block_scalar" {
        inner.find('\n').map(|i| i + 1).unwrap_or(inner.len())
    } else {
        0
    };
    let start = node.start_byte() + lead + q + header;
    let end = node.start_byte() + lead + q + inner.len();
    if start >= end {
        return None;
    }
    // A tag on the enclosing node (`!Ref Target`): the scalar is a reference.
    let reference = !fmt.reference_tags.is_empty()
        && node.parent().is_some_and(|p| {
            let mut c = p.walk();
            p.named_children(&mut c).any(|ch| {
                ch.kind() == "tag" && fmt.reference_tags.iter().any(|r| r == node_text(ch, src))
            })
        });
    Some(Leaf {
        doc,
        path: path.to_vec(),
        start,
        end,
        numeric: fmt.numeric.iter().any(|k| k == kind),
        map,
        split_key,
        // The node itself, or the single child a scalar wrapper holds (`plain_scalar` >
        // `boolean_scalar`), is the grammar's boolean / null.
        boolean: fmt.bool_kinds.iter().any(|k| {
            k == kind
                || (node.named_child_count() == 1
                    && node.named_child(0).is_some_and(|c| c.kind() == k.as_str()))
        }),
        reference,
        forced: false,
        prev: prev_item_text(node, src),
    })
}

/// Text of the item before `node` in its sequence: the previous named sibling of the nearest
/// ancestor (up to 4 levels) that has one. Gives argv context (`["--password", "x"]`).
fn prev_item_text(node: tree_sitter::Node, src: &[u8]) -> Option<String> {
    let mut n = node;
    for _ in 0..4 {
        if let Some(p) = n.prev_named_sibling() {
            if p.kind() == "comment" {
                return None;
            }
            return Some(unquote(node_text(p, src)).to_string());
        }
        n = n.parent()?;
    }
    None
}

/// Decide one config leaf. Returns the ranges (relative to `value`) to mask.
#[allow(clippy::too_many_arguments)]
fn decide(
    value: &str,
    path: &[&str],
    leaf: &Leaf,
    trust: &[&Trust],
    deny: &[&str],
    fmt: &Format,
    known_hosts: &HashSet<String>,
    w: &Walk,
) -> Vec<(usize, usize)> {
    let all = vec![(0, value.len())];
    let c = &*COMPILED;
    let concrete: Vec<String> = path.iter().map(|s| s.to_string()).collect();
    // C0: structural deny wins first (k8s Secret data, sealed secrets, secrets*.yaml), and a
    // denied subtree (Dockerfile COPY heredoc data).
    if leaf.forced || deny.iter().any(|p| pattern_match(p, &concrete)) {
        return all;
    }
    let v = value.trim();
    // Booleans and nulls carry no secret, but only as the grammar's boolean/null nodes: a string
    // spelled `false` (quoted, a `KEY=false` list item, a Dockerfile value) is a string.
    if leaf.boolean {
        return Vec::new();
    }
    // C5: references are kept (a reference with a literal default or expression is not one).
    if leaf.reference || c.references.iter().any(|r| r.is_match(v)) {
        return Vec::new();
    }
    // C1: the first trusted row matching the path decides the class.
    let row = trust
        .iter()
        .flat_map(|t| t.rows.iter())
        .find(|r| r.paths.iter().any(|p| pattern_match(p, &concrete)));
    // C4: a denied key (the value's own key, or the KEY of a KEY=value item) masks the value.
    // Only the leaf key: C4 exists to override trusted rows, and an ancestor such as
    // `secretKeyRef` names a reference structure, not a secret value (untrusted values are
    // masked by default-deny whatever their ancestors are called). Not in identity rows: there
    // the key is a package name (`"auth": "^1.2.3"`), and the spec is still inspected.
    let identity_row = row.is_some_and(|r| matches!(r.class, Class::Identity | Class::Integrity));
    let own_key = path.iter().rev().find(|k| **k != "[]");
    if !identity_row
        && (own_key.is_some_and(|k| core_redact::deny_key(k))
            || leaf.split_key.as_deref().is_some_and(core_redact::deny_key))
    {
        return all;
    }
    let Some(row) = row else {
        let keyed = path.iter().any(|k| *k != "[]");
        if keyed {
            return all;
        }
        return match fmt.unkeyed {
            Unkeyed::Mask => all,
            Unkeyed::C3 => core_redact::value_secret_ranges(value),
            Unkeyed::Command => argv_masks(value, leaf),
        };
    };
    match row.class {
        Class::Plain => {
            if core_redact::value_looks_secret(value) {
                all
            } else {
                Vec::new()
            }
        }
        Class::Identity => identity_masks(value),
        Class::Integrity => {
            if c.integrity.is_match(v) {
                Vec::new()
            } else {
                all
            }
        }
        Class::Command => argv_masks(value, leaf),
        Class::Url => url_masks(value, known_hosts),
        Class::Host => host_port_masks(value, known_hosts),
        Class::Port => port_masks(value),
        Class::Topic => all,
        Class::Env => {
            let key: Option<String> = if let Some(k) = &leaf.split_key {
                Some(k.clone())
            } else if let Some(sib) = &row.key_sibling {
                leaf.map.and_then(|m| w.maps[m].get(sib).cloned())
            } else {
                path.iter()
                    .rev()
                    .find(|k| **k != "[]")
                    .map(|k| k.to_string())
            };
            let Some(key) = key else { return all };
            if core_redact::deny_key(&key) {
                return all;
            }
            match env_class(&key) {
                Some(Class::Url) => url_masks(value, known_hosts),
                Some(Class::Host) => host_port_masks(value, known_hosts),
                Some(Class::Port) => port_masks(value),
                _ => all,
            }
        }
    }
}

fn env_class(key: &str) -> Option<Class> {
    let s = &COMPILED.rules.env_suffix;
    let up = key.to_ascii_uppercase();
    let has = |list: &[String]| list.iter().any(|suf| up.ends_with(suf.as_str()));
    if has(&s.url) {
        Some(Class::Url)
    } else if has(&s.host) {
        Some(Class::Host)
    } else if has(&s.port) {
        Some(Class::Port)
    } else if has(&s.topic) {
        Some(Class::Topic)
    } else {
        None
    }
}

fn port_masks(value: &str) -> Vec<(usize, usize)> {
    match value.trim().parse::<u32>() {
        Ok(p) if (1..=65535).contains(&p) => Vec::new(),
        _ => vec![(0, value.len())],
    }
}

/// C2 for a URL value: scheme and port kept; userinfo, query and fragment masked; the host kept
/// only when it is a service name known in the same file, an IP literal or `localhost`, or a
/// registrable domain whose labels pass C3 (left-most labels beyond the registrable domain are
/// masked unless known); each path segment kept only when it is in the base-path allow-list.
fn url_masks(value: &str, known_hosts: &HashSet<String>) -> Vec<(usize, usize)> {
    let Some(sep) = value.find("://") else {
        return host_port_masks(value, known_hosts);
    };
    let scheme = &value[..sep];
    if scheme.is_empty()
        || !scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
    {
        return vec![(0, value.len())];
    }
    let mut out = Vec::new();
    let rest_start = sep + 3;
    let rest = &value[rest_start..];
    let auth_end = rest
        .find(['/', '?', '#'])
        .map(|i| rest_start + i)
        .unwrap_or(value.len());
    let mut host_start = rest_start;
    if let Some(at) = value[rest_start..auth_end].rfind('@') {
        out.push((rest_start, rest_start + at));
        host_start = rest_start + at + 1;
    }
    for (s, e) in host_port_masks(&value[host_start..auth_end], known_hosts) {
        out.push((host_start + s, host_start + e));
    }
    let path_end = value[auth_end..]
        .find(['?', '#'])
        .map(|i| auth_end + i)
        .unwrap_or(value.len());
    let mut pos = auth_end;
    for seg in value[auth_end..path_end].split('/') {
        if !seg.is_empty() && !COMPILED.path_segments.is_match(seg) {
            out.push((pos, pos + seg.len()));
        }
        pos += seg.len() + 1;
    }
    if path_end < value.len() {
        // Query and fragment: everything after the first `?`/`#` except the delimiters.
        let mut i = path_end;
        for (j, ch) in value[path_end..].char_indices() {
            let at = path_end + j;
            if ch == '?' || ch == '#' {
                if at > i {
                    out.push((i, at));
                }
                i = at + 1;
            }
        }
        if i < value.len() {
            out.push((i, value.len()));
        }
    }
    out
}

/// `host[:port]`: the host per [`url_masks`]; a numeric port is kept, anything else masked.
fn host_port_masks(value: &str, known_hosts: &HashSet<String>) -> Vec<(usize, usize)> {
    if let Some(rest) = value.strip_prefix('[') {
        // An IPv6 literal is kept (an address) only when it parses as one; the port after `]:`
        // is checked like any port. Anything else in brackets is masked.
        let Some(close) = rest.find(']') else {
            return vec![(0, value.len())];
        };
        if rest[..close].parse::<std::net::Ipv6Addr>().is_err() {
            return vec![(0, value.len())];
        }
        let after = close + 2; // past `[` and `]`
        return match value[after..].strip_prefix(':') {
            Some(p) => port_masks(p)
                .into_iter()
                .map(|(s, e)| (s + after + 1, e + after + 1))
                .collect(),
            None if after == value.len() => Vec::new(),
            None => vec![(after, value.len())],
        };
    }
    let (host, port) = match value.rsplit_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (value, None),
    };
    let mut out = host_masks(host, known_hosts);
    if let Some(p) = port {
        for (s, e) in port_masks(p) {
            out.push((host.len() + 1 + s, host.len() + 1 + e));
        }
    }
    out
}

fn host_masks(host: &str, known_hosts: &HashSet<String>) -> Vec<(usize, usize)> {
    let all = vec![(0, host.len())];
    if host.is_empty() || known_hosts.contains(host) || host == "localhost" {
        return Vec::new();
    }
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Vec::new();
    }
    let lower = host.to_ascii_lowercase();
    let Some(reg) = psl::domain_str(&lower) else {
        return all;
    };
    if reg.split('.').count() < 2 || !lower.ends_with(reg) {
        return all;
    }
    let mut out = Vec::new();
    let reg_start = host.len() - reg.len();
    // Labels inside the registrable domain: kept unless C3 flags one.
    let mut pos = reg_start;
    for label in host[reg_start..].split('.') {
        if core_redact::value_looks_secret(label) {
            out.push((pos, pos + label.len()));
        }
        pos += label.len() + 1;
    }
    // Labels to the left of it: kept only when known in the same file.
    if reg_start > 0 {
        let left = &host[..reg_start - 1];
        let mut pos = 0;
        for label in left.split('.') {
            if !known_hosts.contains(label) {
                out.push((pos, pos + label.len()));
            }
            pos += label.len() + 1;
        }
    }
    out
}

/// A source identity or dependency spec (`^1.2.0`, `workspace:*`, `git+https://...`,
/// `github:owner/repo`): userinfo, query and fragment masked, and any C3-flagged part.
fn identity_masks(value: &str) -> Vec<(usize, usize)> {
    // Known credential shapes and URL credentials only: a git commit pin, an artifact hash or a
    // transport username (`git@`) is part of the identity the linker compares (DES-S1 §2.1,
    // review R4-02), so the entropy and hex-run rules do not apply here.
    let mut out = core_redact::token_shape_ranges(value);
    if let Some(sep) = value.find("://") {
        let rest_start = sep + 3;
        let auth_end = value[rest_start..]
            .find(['/', '?', '#'])
            .map(|i| rest_start + i)
            .unwrap_or(value.len());
        if let Some(at) = value[rest_start..auth_end].rfind('@') {
            let info = &value[rest_start..rest_start + at];
            if info.contains(':') || info.len() >= 12 {
                out.push((rest_start, rest_start + at));
            }
        }
    }
    // A query string is never identity. `#` is a git ref or a monorepo directory in npm specs
    // (`github:o/r#v1.2`) and is kept (credential shapes in it were masked above).
    if let Some(q) = value.find('?') {
        let end = value[q..].find('#').map(|h| q + h).unwrap_or(value.len());
        out.push((q + 1, end));
    }
    out
}

/// Command words with argv context: one element of an exec array or a k8s `args` list whose
/// previous element is a C4-named flag (`["--password", "x"]`) is masked whole.
fn argv_masks(value: &str, leaf: &Leaf) -> Vec<(usize, usize)> {
    let flag = leaf.prev.as_deref().map(|p| {
        p.trim_matches(|c: char| "\"'`-:=,;()[]".contains(c))
            .to_string()
    });
    if flag.is_some_and(|f| !f.contains(char::is_whitespace) && core_redact::deny_key(&f)) {
        return vec![(0, value.len())];
    }
    command_masks(value)
}

/// Command words (package scripts, Dockerfile RUN): a C3-flagged token is masked; the value of a
/// `KEY=value` token with a C4-denied key is masked; the token after a C4-denied flag or header
/// name (`--password x`, `-H "Authorization: Bearer x"`) is masked, and one more after a scheme
/// word (`Bearer`, `Basic`, `token`).
fn command_masks(value: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut tokens: Vec<(usize, &str)> = Vec::new();
    let mut start = None;
    for (i, ch) in value.char_indices() {
        if ch.is_whitespace() {
            if let Some(s) = start.take() {
                tokens.push((s, &value[s..i]));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        tokens.push((s, &value[s..]));
    }
    let strip = |t: &str| {
        t.trim_matches(|c: char| "\"'`-:=,;()".contains(c))
            .to_string()
    };
    // A value quoted in the command; an escaped quote inside a JSON string counts (`\"a b\"`).
    let quote_of = |t: &str| {
        t.trim_start_matches('\\')
            .chars()
            .next()
            .filter(|c| matches!(c, '"' | '\''))
    };
    let opens = |t: &str, q: char| !(t.len() > 1 && t.ends_with(q));
    let mut mask_next = 0usize;
    let mut in_quote: Option<char> = None;
    for (pos, tok) in tokens {
        // Inside a masked quoted value that spans words (`--password "two words"`).
        if let Some(q) = in_quote {
            out.push((pos, pos + tok.len()));
            if tok.ends_with(q) {
                in_quote = None;
            }
            continue;
        }
        if mask_next > 0 {
            mask_next -= 1;
            let word = strip(tok).to_ascii_lowercase();
            out.push((pos, pos + tok.len()));
            if let Some(q) = quote_of(tok).filter(|q| opens(tok, *q)) {
                in_quote = Some(q);
            }
            if matches!(word.as_str(), "bearer" | "basic" | "token" | "digest") {
                mask_next += 1;
            }
            continue;
        }
        if core_redact::value_looks_secret(tok) {
            out.push((pos, pos + tok.len()));
            continue;
        }
        // `KEY=value`, `--password=value`, `Authorization:value`: the value after a denied name.
        if let Some(i) = tok.find(['=', ':']) {
            let (k, v) = (&tok[..i], &tok[i + 1..]);
            if core_redact::deny_key(&strip(k)) {
                if v.trim_matches(|c: char| "\"'`".contains(c)).is_empty() {
                    mask_next = 1; // `Authorization:` then the value token(s)
                } else {
                    out.push((pos + i + 1, pos + tok.len()));
                    // `'Authorization:Bearer x'`: the scheme word is not the credential; the
                    // next token is. A quote opened at the start of the token (or of the value)
                    // keeps masking until it closes.
                    if let Some(q) = quote_of(tok)
                        .filter(|q| opens(tok, *q))
                        .or_else(|| quote_of(v).filter(|q| opens(v, *q)))
                    {
                        // The open quote masks through its close, credential included.
                        in_quote = Some(q);
                    } else if matches!(
                        strip(v).to_ascii_lowercase().as_str(),
                        "bearer" | "basic" | "token" | "digest"
                    ) {
                        mask_next = 1;
                    }
                }
                continue;
            }
        }
        if core_redact::deny_key(&strip(tok)) {
            mask_next = 1;
        }
    }
    out
}

/// Byte ranges of every ERROR / MISSING node under `n`.
fn error_ranges(n: tree_sitter::Node, out: &mut Vec<(usize, usize)>) {
    if n.is_error() || n.is_missing() {
        out.push((n.start_byte(), n.end_byte()));
        return;
    }
    if !n.has_error() {
        return;
    }
    let mut c = n.walk();
    for ch in n.children(&mut c) {
        error_ranges(ch, out);
    }
}

/// Text the parser could not place: fail closed. Per line, everything after the first `=` or
/// `:` is masked, and a list item (`- value`) or a bare value line is masked whole; comment lines
/// and keys are kept.
fn fallback_mask_all_values(text: &str, masks: &mut Vec<(usize, usize, MaskKind)>) {
    let mut pos = 0;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let lead = body.len() - body.trim_start().len();
        let t = body.trim_start();
        let comment = t.starts_with('#') || t.starts_with(';') || t.starts_with("//");
        if !t.is_empty() && !comment {
            let item = t.strip_prefix("- ").map(|r| (lead + 2, r));
            let (from, rest) = match (t.find(['=', ':']), item) {
                (Some(i), _) => (lead + i + 1, &t[i + 1..]),
                (None, Some((off, r))) => (off, r),
                (None, None) => (lead, t),
            };
            let skip = rest.len() - rest.trim_start().len();
            if !rest.trim().is_empty() {
                masks.push((pos + from + skip, pos + body.len(), MaskKind::Text));
            }
        }
        pos += line.len();
    }
}

// ── Code: string literals ──────────────────────────────────────────────────────────────────────

/// A word that may name a secret, case-insensitively: the fast gate before [`deny_key`] runs on
/// each identifier.
static DENY_HINT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)pass|pwd|secret|token|credential|creds|auth|cert|signature|salt|session|cookie|dsn|private|key|bearer|connection")
        .expect("deny hint compiles")
});
static IDENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z_$][\w$]*").expect("identifier compiles"));

/// Cheap check before parsing a code file a second time: any identifier that C4 denies (the same
/// vocabulary as [`core_redact::deny_key`]), or any C3 value hit in the raw text.
fn code_prefilter(text: &str) -> bool {
    if !core_redact::value_secret_ranges(text).is_empty() {
        return true;
    }
    DENY_HINT.is_match(text)
        && IDENT
            .find_iter(text)
            .any(|m| DENY_HINT.is_match(m.as_str()) && core_redact::deny_key(m.as_str()))
}

static LEFT_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?:["']([A-Za-z_][\w.\-]*)["']|([A-Za-z_$][\w$]*))\s*\]?\s*(?:===?|!==?|:=|=>|\?\?=|\|\|=|\?\?|\|\||=|:|,|\(|\bor\b)\s*$"#,
    )
    .expect("left-name compiles")
});
/// The leading `name:` of a typed binding segment (after an optional declaration keyword).
static SEGMENT_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:(?:export|declare|const|let|var|val|final|static|readonly|pub(?:\([^)]*\))?|public|private|protected|internal|abstract|override|mut)\s+)*([A-Za-z_]\w*)\s*:[^:]")
        .expect("segment name compiles")
});

/// The name of a typed binding whose `=` ends `left` (`const TOKEN: &str = `, `password:
/// Optional[Optional[str]] = `, `{ label: t, password: value = `). Scans back from the `=` with
/// bracket depth over `()[]{}<>`, stopping at a top-level `,` / `;` or an unbalanced opener, so a
/// type of any nesting stays inside its binding and a destructuring comma ends it; the binding is
/// the last line-anchored `name:` in that segment.
fn typed_binding_name(left: &str) -> Option<&str> {
    let t = left.trim_end();
    let eq = t.strip_suffix('=')?;
    if eq.ends_with(['=', '!', ':', '+', '-', '*', '/', '|', '&', '?']) {
        // `==`, `!=`, `:=`, `+=`, ... are not declarations. (`<` / `>` are allowed: a generic
        // type closes with `>`, and a comparison has no `name:` binding to find.)
        return None;
    }
    let bytes = eq.as_bytes();
    let mut depth = 0i32;
    let mut start = 0;
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' | b']' | b'}' => depth += 1,
            b'>' if i == 0 || !matches!(bytes[i - 1], b'-' | b'=') => depth += 1,
            b'(' | b'[' | b'{' | b'<' => {
                if depth == 0 {
                    start = i + 1;
                    break;
                }
                depth -= 1;
            }
            b',' | b';' if depth == 0 => {
                start = i + 1;
                break;
            }
            _ => {}
        }
    }
    // The LAST line-anchored `name:` in the segment: a type annotation may continue on the next
    // line (`const password:\n  string =`), and earlier lines (`from typing import ...`) are not
    // the binding.
    SEGMENT_NAME
        .captures_iter(&eq[start..])
        .last()
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
}
/// `name type = "..."` (Bicep `param adminPassword string = '...'`).
static LEFT_NAME_TYPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b([A-Za-z_]\w*)\s+[A-Za-z_][\w.<>\[\]]*\s*=\s*$")
        .expect("name-type decl compiles")
});

fn code_masks(
    text: &str,
    grammar: &str,
    deny_all: bool,
    masks: &mut Vec<(usize, usize, MaskKind)>,
) {
    let Some(tree) = parse(text, grammar) else {
        return;
    };
    let rules = &COMPILED.rules.code;
    let src = text.as_bytes();
    let is_string = |k: &str| {
        rules
            .string_kind_substrings
            .iter()
            .any(|s| k.contains(s.as_str()))
    };
    let is_comment = |k: &str| {
        rules
            .comment_kind_substrings
            .iter()
            .any(|s| k.contains(s.as_str()))
    };
    // Comment spans from the tree (never a regex over raw text: `//` in a URL or a string is not
    // a comment), so a comment between a binding and its literal can be blanked out of the C4
    // left context.
    let mut comments: Vec<(usize, usize)> = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(n) = stack.pop() {
        if is_comment(n.kind()) {
            comments.push((n.start_byte(), n.end_byte()));
            continue;
        }
        let mut c = n.walk();
        stack.extend(n.children(&mut c));
    }
    let mut stack = vec![tree.root_node()];
    while let Some(n) = stack.pop() {
        let k = n.kind();
        if is_comment(k) {
            continue; // comments: token shapes only (already applied to the whole text)
        }
        if n.is_named() && is_string(k) {
            literal(n, text, src, rules, deny_all, &comments, masks);
            continue; // the outermost string node decides; never its fragments again
        }
        // Default-deny grammars (KDL, RON): numeric values are values too.
        if deny_all
            && n.is_named()
            && n.child_count() == 0
            && rules
                .number_kind_substrings
                .iter()
                .any(|s| k.contains(s.as_str()))
        {
            masks.push((n.start_byte(), n.end_byte(), MaskKind::Numeric));
            continue;
        }
        let mut c = n.walk();
        stack.extend(n.children(&mut c));
    }
}

fn literal(
    n: tree_sitter::Node,
    text: &str,
    src: &[u8],
    rules: &CodeRules,
    deny_all: bool,
    comments: &[(usize, usize)],
    masks: &mut Vec<(usize, usize, MaskKind)>,
) {
    // Module specifiers are identities (`import x from "pkg"`, `require("pkg")`, `#include`).
    if is_module_specifier(n, src, rules) {
        return;
    }
    let t = node_text(n, src);
    let (is, ie) = literal_interior(t);
    if is >= ie {
        return;
    }
    let start = n.start_byte() + is;
    let end = n.start_byte() + ie;
    let inner = &text[start..end];
    if inner.trim().is_empty() || COMPILED.references.iter().any(|r| r.is_match(inner.trim())) {
        return;
    }
    // An interpolated literal (`\`a${x}b\``, `f"a{x}"`): only its literal fragments are value
    // text; the embedded expressions are code and stay.
    let whole = |masks: &mut Vec<(usize, usize, MaskKind)>| {
        let mut c = n.walk();
        let kids: Vec<tree_sitter::Node> = n.named_children(&mut c).collect();
        if kids
            .iter()
            .any(|k| k.kind().contains("substitution") || k.kind().contains("interpolat"))
        {
            for k in kids {
                if rules
                    .fragment_kind_substrings
                    .iter()
                    .any(|f| k.kind().contains(f.as_str()))
                {
                    // Clipped to the literal's interior: a fragment node can include the opening
                    // or closing delimiter (Swift `#"` in `raw_str_part`), which must stay.
                    let (s, e) = (k.start_byte().max(start), k.end_byte().min(end));
                    if s < e {
                        masks.push((s, e, MaskKind::Text));
                    }
                }
            }
        } else {
            masks.push((start, end, MaskKind::Text));
        }
    };
    if deny_all {
        whole(masks);
        return;
    }
    // C4-context: the literal is bound to a denied name. The window may span lines
    // (`const password =\n  "..."`).
    let left_from = n.start_byte().saturating_sub(200);
    let (left_from, _) = widen(text, left_from, left_from);
    // Comments between the binding and the literal (`const password = /* prod */ "..."`) do not
    // break the binding.
    // Each comment span collapses to ONE space (not same-length blanks, which would push a typed
    // binding past the declaration patterns' length limits).
    let mut left_bytes: Vec<u8> = Vec::with_capacity(n.start_byte() - left_from);
    let mut pos = left_from;
    let mut spans: Vec<(usize, usize)> = comments
        .iter()
        .map(|&(cs, ce)| (cs.max(left_from), ce.min(n.start_byte())))
        .filter(|(s, e)| s < e)
        .collect();
    spans.sort_unstable();
    for (cs, ce) in spans {
        if cs < pos {
            continue;
        }
        left_bytes.extend_from_slice(&text.as_bytes()[pos..cs]);
        left_bytes.push(b' ');
        pos = ce;
    }
    left_bytes.extend_from_slice(&text.as_bytes()[pos..n.start_byte()]);
    let left_owned = String::from_utf8_lossy(&left_bytes);
    let left = left_owned.as_ref();
    let named = LEFT_NAME
        .captures(left)
        .and_then(|c| c.get(1).or_else(|| c.get(2)))
        .is_some_and(|m| core_redact::deny_key(m.as_str()))
        || typed_binding_name(left).is_some_and(core_redact::deny_key)
        || LEFT_NAME_TYPE
            .captures(left)
            .and_then(|c| c.get(1))
            .is_some_and(|m| core_redact::deny_key(m.as_str()));
    if named {
        whole(masks);
        return;
    }
    // C3 on the literal's own text.
    for (s, e) in core_redact::value_secret_ranges(inner) {
        masks.push((start + s, start + e, MaskKind::Text));
    }
}

/// True when the literal `n` is a module specifier: the `source`/`path` operand of an import,
/// export-from or include, the argument of a `require`/`import`-like call, or a PHP
/// `require`/`include` expression. Decided from the AST, never from source spelling.
fn is_module_specifier(n: tree_sitter::Node, src: &[u8], rules: &CodeRules) -> bool {
    let Some(p) = n.parent() else { return false };
    let is_n = |c: Option<tree_sitter::Node>| c.is_some_and(|c| c.id() == n.id());
    if rules
        .module_fields
        .iter()
        .any(|f| is_n(p.child_by_field_name(f.as_str())))
    {
        return true;
    }
    if rules
        .module_parent_kinds
        .iter()
        .any(|k| p.kind() == k.as_str())
    {
        return true;
    }
    if p.kind().contains("argument") {
        if let Some(call) = p.parent().filter(|g| g.kind().contains("call")) {
            let callee = call
                .child_by_field_name("function")
                .or_else(|| call.child_by_field_name("method"))
                .or_else(|| call.named_child(0));
            if callee.is_some_and(|f| {
                rules
                    .module_callees
                    .iter()
                    .any(|m| m == node_text(f, src).trim())
            }) {
                return true;
            }
        }
    }
    false
}

/// The interior of a string literal's source text: after its prefix and opening quote run, before
/// the closing run (`"x"`, `'x'`, `` `x` ``, `"""x"""`, `r#"x"#`, `b"x"`, `@"x"`, `$"x"`).
fn literal_interior(t: &str) -> (usize, usize) {
    let bytes = t.as_bytes();
    let Some(q) = bytes
        .iter()
        .take(6)
        .position(|b| matches!(b, b'"' | b'\'' | b'`'))
    else {
        return (0, t.len());
    };
    let qc = bytes[q];
    let mut run = 1;
    while q + run < bytes.len() && bytes[q + run] == qc && run < 3 {
        run += 1;
    }
    if run == 2 {
        run = 1; // `""` is an empty literal, not an opening triple
    }
    let hashes = bytes[..q].iter().rev().take_while(|b| **b == b'#').count();
    let open = q + run;
    let mut close = t.len();
    let tail = &bytes[open.min(bytes.len())..];
    let trailing_hashes = tail
        .iter()
        .rev()
        .take_while(|b| **b == b'#')
        .count()
        .min(hashes);
    close -= trailing_hashes;
    let mut r = 0;
    while r < run && close > open && bytes[close - 1] == qc {
        close -= 1;
        r += 1;
    }
    (open.min(close), close)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Planted FAKE values are assembled at runtime so no secret-shaped assignment appears in the
    /// source text (secret scanners rightly flag those even when the value is a fake).
    fn fill(src: &str) -> String {
        src.replace("@DB_PW_KEY@", &["DB_", "PASS", "WORD"].concat())
            .replace("@PW_KEY@", &["pass", "word"].concat())
            .replace(
                "@COMPOSE_PW@",
                &["PLANTED", "compose", "Secret9f8e7d"].concat(),
            )
            .replace("@PG_USERINFO@", &["u:", "hunter2", "pw"].concat())
            .replace("@K8S_B64@", &["UExBTlRF", "RGs4c1Nl", "Y3JldA=="].concat())
            .replace("@HELM_PW@", &["PLANTED", "helm", "Secret1a2b3c"].concat())
            .replace("@STRING_DATA@", &["PLANTED", "string", "Data55aa"].concat())
            .replace("@ROSEBUD@", &["rose", "bud7"].concat())
    }

    fn same_len(path: &str, text: &str) -> String {
        let out = redact_source(path, text);
        assert_eq!(out.len(), text.len(), "length preserved for {path}");
        assert_eq!(
            out.matches('\n').count(),
            text.matches('\n').count(),
            "lines preserved"
        );
        out
    }

    #[test]
    fn rules_file_parses_and_every_grammar_exists() {
        let c = &*COMPILED;
        for f in &c.rules.format {
            assert!(
                crate::treesitter::builtin_language(&f.grammar).is_some(),
                "{}",
                f.grammar
            );
        }
        assert!(!rules_digest().is_empty());
    }

    /// A config-like grammar wired later must not ship unredacted: every document-tier grammar the
    /// extractor can parse has a [[format]] row, a default_deny entry, or is declared prose.
    #[test]
    fn every_document_tier_grammar_is_covered() {
        #[derive(Deserialize)]
        struct Manifest {
            language: Vec<Row>,
        }
        #[derive(Deserialize)]
        struct Row {
            name: String,
            #[serde(default)]
            tier: String,
        }
        let m: Manifest = toml::from_str(include_str!("../languages.toml")).expect("manifest");
        let c = &*COMPILED;
        for row in m.language.iter().filter(|r| r.tier == "document") {
            if crate::treesitter::builtin_language(&row.name).is_none() {
                continue; // no grammar wired: the file type is not indexed
            }
            let covered = c.rules.format.iter().any(|f| f.grammar == row.name)
                || c.rules.default_deny.grammars.contains(&row.name)
                || c.rules.default_deny.prose.contains(&row.name);
            assert!(
                covered,
                "document-tier grammar `{}` has no redaction rule",
                row.name
            );
        }
    }

    #[test]
    fn excluded_names() {
        for p in [
            ".env",
            "a/.env.local",
            "deploy/.env.production",
            ".npmrc",
            "k/id_rsa",
            "c/credentials.json",
            "kubeconfig.yaml",
            "x.pem",
        ] {
            assert!(is_excluded(p), "{p}");
        }
        for p in [
            ".env.example",
            "a/.env.sample",
            "src/env.ts",
            "package.json",
            "docker-compose.yml",
        ] {
            assert!(!is_excluded(p), "{p}");
        }
    }

    #[test]
    fn compose_env_default_deny_and_url_rules() {
        let src = &fill(
            "services:\n  api:\n    image: acme/api:1.2\n    environment:\n      @DB_PW_KEY@: @COMPOSE_PW@\n      DB_P2: rosebud7\n      API_BASE_URL: http://orders:8080/v1\n      ORDERS_URL: http://api:9000/v1/rosebud7\n      CALLBACK_URL: https://example.test/rosebud7/hook\n      HOOK_URL: https://rosebud7.example.test/api\n      ORDERS_TOPIC: rosebud7\n      PG_URL: postgres://@PG_USERINFO@@db.example.test:5432/app?sslmode=require#f\n  worker:\n    environment:\n      - LIST_SECRET=PLANTEDlist1\n      - WORKER_PORT=9001\n      - QUOTED=\"dq\\tPLANTEDesc\"\n",
        );
        let out = same_len("docker-compose.yml", src);
        for gone in [
            "PLANTEDcomposeSecret9f8e7d",
            "rosebud7",
            "hunter2pw",
            "sslmode",
            "orders:",
            "PLANTEDlist1",
        ] {
            assert!(!out.contains(gone), "{gone} leaked:\n{out}");
        }
        assert!(out.contains("image: acme/api:1.2"), "{out}");
        assert!(
            out.contains("http://api:9000/v1/"),
            "known service host kept: {out}"
        );
        assert!(out.contains("https://example.test/"), "{out}");
        assert!(out.contains(".example.test/api"), "{out}");
        assert!(
            out.contains("DB_PASSWORD:") && out.contains("ORDERS_TOPIC:"),
            "keys kept: {out}"
        );
        assert!(out.contains(":5432/"), "port kept: {out}");
        assert!(
            out.contains("WORKER_PORT=9001") && !out.contains("PLANTEDesc"),
            "{out}"
        );
    }

    #[test]
    fn k8s_secret_configmap_and_env() {
        let src = &fill(
            "apiVersion: v1\nkind: Secret\nmetadata:\n  name: db-creds\ndata:\n  @PW_KEY@: @K8S_B64@\nstringData:\n  note: plainPLANTED\n---\napiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: cfg\ndata:\n  TOKENISH: ghp_abcdefghijklmnopqrstuvwxyz0123456789\n  SOME_KEY: innocentPLANTED\n  ORDERS_PORT: \"8080\"\n---\napiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\nspec:\n  template:\n    spec:\n      containers:\n        - name: web\n          image: acme/web:2\n          env:\n            - name: DB_PASS\n              value: envPLANTED\n            - name: ORDERS_ADDR\n              value: \"cfg:9000\"\n            - name: SECRET_REF\n              valueFrom:\n                secretKeyRef: {name: db-creds, key: password}\n",
        );
        let out = same_len("k8s/all.yaml", src);
        for gone in [
            "UExBTlRFRGs4c1NlY3JldA==",
            "plainPLANTED",
            "ghp_abc",
            "innocentPLANTED",
            "envPLANTED",
        ] {
            assert!(!out.contains(gone), "{gone} leaked:\n{out}");
        }
        for kept in [
            "name: db-creds",
            "password:",
            "image: acme/web:2",
            "\"8080\"",
            "cfg:9000",
            "key: password",
            "name: DB_PASS",
        ] {
            assert!(out.contains(kept), "{kept} missing:\n{out}");
        }
    }

    #[test]
    fn helm_values_and_secrets_files() {
        let out = same_len(
            "chart/values.yaml",
            &fill(
                "db:\n  @PW_KEY@: @HELM_PW@\norders:\n  baseUrl: http://orders.svc:8080\nreplicas: 3\nenabled: true\n",
            ),
        );
        assert!(
            !out.contains("PLANTEDhelmSecret1a2b3c") && !out.contains("orders.svc"),
            "{out}"
        );
        assert!(out.contains("enabled: true"), "{out}");
        let s = same_len("chart/secrets.yaml", "apiKey: abc\nnested:\n  x: y\n");
        assert!(!s.contains("abc") && !s.contains(" y\n"), "{s}");
    }

    #[test]
    fn manifests_keep_linker_fields_and_mask_the_rest() {
        let pkg = r#"{"name":"acme-types","version":"1.0.0","types":"src/index.ts","dependencies":{"ky":"^1.2.0","x":"git+https://bob:tokPLANTED1x@example.test/o/r.git"},"repository":{"url":"git+https://github.com/acme/types.git","directory":"packages/t"},"scripts":{"build":"tsc -p tsconfig.json","pub":"curl -H \"Authorization: Bearer abcPLANTED\" x"},"config":{"apiHost":"innerPLANTED"},"private":true}"#;
        let out = same_len("packages/t/package.json", pkg);
        for kept in [
            "acme-types",
            "1.0.0",
            "src/index.ts",
            "^1.2.0",
            "github.com/acme/types.git",
            "packages/t",
            "tsc -p tsconfig.json",
            "\"private\":true",
        ] {
            assert!(out.contains(kept), "{kept} missing: {out}");
        }
        for gone in ["tokPLANTED1x", "bob", "abcPLANTED", "innerPLANTED"] {
            assert!(!out.contains(gone), "{gone} leaked: {out}");
        }
        let lock = r#"{"packages":{"node_modules/ky":{"version":"1.2.3","resolved":"https://registry.npmjs.org/ky/-/ky-1.2.3.tgz","integrity":"sha512-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789+/AbCdEfGhIjKlMnOpQrStUvWxYz0123456789=="}}}"#;
        let out = same_len("package-lock.json", lock);
        assert_eq!(out, lock, "lockfile identities and integrity kept");
        // Empty containers in untrusted positions keep the document parseable (X1 found a
        // lockfile root entry `"": {}` stored as `"": RE`).
        for (path, text) in [
            (
                "package-lock.json",
                r#"{"lockfileVersion":3,"packages":{"":{},"node_modules/a":{"version":"1.0.0"}}}"#,
            ),
            (
                "package.json",
                r#"{"name":"x","keywords":[],"scripts":{},"config":{}}"#,
            ),
            ("app.json", r#"{"a":{},"b":[],"c":{"d":{}}}"#),
            ("c.yaml", "a: {}\nb: []\n"),
        ] {
            let out = same_len(path, text);
            assert_eq!(out, text, "{path}");
        }
        // X1: pnpm workspace member patterns are identities the linker reads.
        let pnpm = "packages:\n  - 'packages/*'\n  - apps/**\n";
        assert_eq!(same_len("pnpm-workspace.yaml", pnpm), pnpm);
        let cargo = "[package]\nname = \"a\"\nversion = \"0.1.0\"\nrepository = \"https://github.com/o/a\"\n[lib]\nname = \"a_lib\"\npath = \"src/x.rs\"\n[dependencies]\nb = { path = \"../b\", package = \"real-b\" }\n[package.metadata.secretstuff]\nv = \"hiddenPLANTED\"\n";
        let out = same_len("Cargo.toml", cargo);
        for kept in [
            "\"a\"",
            "0.1.0",
            "github.com/o/a",
            "a_lib",
            "src/x.rs",
            "../b",
            "real-b",
        ] {
            assert!(out.contains(kept), "{kept} missing: {out}");
        }
        assert!(!out.contains("hiddenPLANTED"), "{out}");
        let py = "[project]\nname = \"p\"\ndependencies = [\"q>=1\"]\n[tool.uv.sources]\nq = { path = \"../q\" }\n[tool.other]\nk = \"pyPLANTED\"\n";
        let out = same_len("pyproject.toml", py);
        assert!(
            out.contains("../q") && out.contains("q>=1") && !out.contains("pyPLANTED"),
            "{out}"
        );
    }

    #[test]
    fn other_config_formats_default_deny() {
        let cases = [
            ("db.ini", "[db]\nhost=iniPLANTED\n", "iniPLANTED"),
            (
                "q.yaml",
                "k: \"esc\\nPLANTEDdq\"\nb: |\n  blockPLANTED\n",
                "PLANTEDdq",
            ),
            ("q2.yaml", "b: |\n  blockPLANTED\n", "blockPLANTED"),
            ("c.kdl", "node \"kdlPLANTED\" key=\"v\"\n", "kdlPLANTED"),
            (
                "prod.tfvars",
                "db_password = \"tfPLANTED\"\nregion = \"us-east-1\"\n",
                "tfPLANTED",
            ),
            (
                "main.tf",
                "resource \"aws_db_instance\" \"d\" {\n  password = \"hclPLANTED\"\n  engine = var.engine\n}\n",
                "hclPLANTED",
            ),
            (
                "app.json",
                "{\"k\": \"jsonPLANTED\", \"n\": 4242}",
                "jsonPLANTED",
            ),
            (
                "x.dockerfile",
                "FROM node:20\nENV TOKEN=dockPLANTED API_URL=http://api:1/v1\nRUN echo hi\n",
                "dockPLANTED",
            ),
        ];
        for (p, src, planted) in cases {
            let out = same_len(p, src);
            assert!(!out.contains(planted), "{p}: {out}");
        }
        let tf = same_len(
            "main.tf",
            "resource \"aws_db_instance\" \"d\" {\n  engine = var.engine\n}\n",
        );
        assert!(
            tf.contains("\"aws_db_instance\" \"d\"") && tf.contains("var.engine"),
            "{tf}"
        );
        let j = same_len("app.json", "{\"n\": 4242}");
        assert_eq!(j, "{\"n\": 1000}");
        let d = same_len("x.dockerfile", "FROM node:20\nRUN echo hi\n");
        assert!(d.contains("FROM node:20"), "{d}");
    }

    #[test]
    fn code_literals_by_shape_and_by_binding() {
        let js = "const t = process.env.TOKEN || \"PLANTEDcodeSecretXYZ\";\nconst o = { apiKey: \"k1PLANTED\", name: \"bob\" };\nimport x from \"some-long-package-name-v2-2024\";\nconst g = \"ghp_abcdefghijklmnopqrstuvwxyz0123456789\";\nlet password = \"\";\n// comment ghp_abcdefghijklmnopqrstuvwxyz0123456789\nconst msg = \"token refreshed\";\n";
        let out = same_len("src/app.js", js);
        for gone in ["PLANTEDcodeSecretXYZ", "k1PLANTED", "ghp_abc"] {
            assert!(!out.contains(gone), "{gone} leaked:\n{out}");
        }
        for kept in [
            "\"bob\"",
            "some-long-package-name-v2-2024",
            "\"token refreshed\"",
            "process.env.TOKEN",
        ] {
            assert!(out.contains(kept), "{kept} missing:\n{out}");
        }
        let py = same_len(
            "a.py",
            "x = os.getenv(\"API_TOKEN\", \"pyDefaultPLANTED\")\nclient_secret = 'csPLANTED'\nname = 'plain'\n",
        );
        assert!(
            !py.contains("pyDefaultPLANTED") && !py.contains("csPLANTED") && py.contains("'plain'"),
            "{py}"
        );
        let bicep = same_len(
            "m.bicep",
            "param adminPassword string = 'bicepPLANTED'\nparam location string = 'eastus'\n",
        );
        assert!(
            !bicep.contains("bicepPLANTED") && bicep.contains("'eastus'"),
            "{bicep}"
        );
        let rs = same_len(
            "a.rs",
            "const TOKEN: &str = \"rsPLANTED\";\nconst NAME: &str = \"ok\";\n",
        );
        assert!(!rs.contains("rsPLANTED") && rs.contains("\"ok\""), "{rs}");
    }

    #[test]
    fn prose_gets_token_shapes_only() {
        let md = "Use key AKIAABCDEFGHIJKLMNOP here. See commit 0123456789abcdef0123456789abcdef01234567.\n";
        let out = same_len("README.md", md);
        assert!(!out.contains("AKIAABCD"), "{out}");
        assert!(
            out.contains("0123456789abcdef0123456789abcdef01234567"),
            "hex in prose kept: {out}"
        );
    }

    #[test]
    fn redaction_is_idempotent() {
        let src = "services:\n  api:\n    environment:\n      DB_P2: rosebud7\n";
        let once = redact_source("docker-compose.yml", src);
        assert_eq!(redact_source("docker-compose.yml", &once), once);
    }

    /// Review x0-r1 fail-open findings: each input must not keep its planted value.
    #[test]
    fn review_r1_fail_open_cases_are_masked() {
        let cases: &[(&str, &str, &str)] = &[
            // F1: a quoted value starting with `|` / `>` is not a block header.
            (
                "a.json",
                r#"{"password":">rosebud7","x":"|rosebud7"}"#,
                "rosebud7",
            ),
            // F2: strings spelling booleans are strings.
            (
                "b.json",
                r#"{"password":"false","secret":"None","token":"yes"}"#,
                "\"false\"",
            ),
            ("b2.json", r#"{"token":"yes"}"#, "\"yes\""),
            // F3: references carrying literal defaults or expressions.
            (
                "docker-compose.yml",
                "services:\n  a:\n    environment:\n      DB_PASSWORD: ${PW:-rosebud7}\n",
                "rosebud7",
            ),
            ("v.yaml", "k: '{{ \"rosebud7\" }}'\n", "rosebud7"),
            // F4: a quoted TOML key with a dot is one key, not a trusted path.
            (
                "pyproject.toml",
                "\"project.name\" = \"rosebud7\"\n",
                "rosebud7",
            ),
            // F5: a malformed file fails closed.
            (
                "broken.yaml",
                "services:\n  api:\n    environment:\n      DB_PASSWORD: rosebud7\n      - x\n   bad: [\n",
                "rosebud7",
            ),
            (
                "broken.tf",
                "resource \"a\" \"b\" {\n  x = \"rosebud7\"\n  {\n",
                "rosebud7",
            ),
            // F7: numbers under a default-deny grammar.
            ("c.kdl", "credentials pin=123456\n", "123456"),
            // F8: deny words the old prefilter missed, raw-string prefixes.
            (
                "a.js",
                "const connectionString = \"rosebud7\";\n",
                "rosebud7",
            ),
            (
                "a.rs",
                "const PASSWORD: &str = r#\"rosebud7\"#;\n",
                "rosebud7",
            ),
            ("a.py", "password = rf\"rosebud7\"\n", "rosebud7"),
            // F9: a binding split over lines.
            ("m.js", "const password =\n    \"rosebud7\";\n", "rosebud7"),
            // F10: an exported secret declaration is not a module specifier.
            (
                "e.ts",
                "export const password = \"rosebud7\";\n",
                "rosebud7",
            ),
            // F11: compose command and k8s args keep no named credential.
            (
                "docker-compose.yml",
                "services:\n  a:\n    command: app --password @ROSEBUD@\n",
                "rosebud7",
            ),
            (
                "k.yaml",
                "apiVersion: v1\nkind: Pod\nmetadata:\n  name: p\nspec:\n  containers:\n    - name: c\n      image: i\n      args: [\"--password\", \"rosebud7\"]\n",
                "rosebud7",
            ),
            // F12: Dockerfile exec arrays carry argv context.
            (
                "x.dockerfile",
                "FROM a\nRUN [\"curl\", \"--password\", \"rosebud7\"]\n",
                "rosebud7",
            ),
            // F13: inline header and quoted multiword values.
            (
                "package.json",
                r#"{"scripts":{"a":"curl -H Authorization:rosebud7 https://example.com","b":"app --password \"rose bud7\" go"}}"#,
                "rosebud7",
            ),
            (
                "package.json",
                r#"{"scripts":{"b":"app --password \"rosebud7 bud8\" go"}}"#,
                "bud8",
            ),
            // F14: COPY heredoc data is file content.
            (
                "x.dockerfile",
                "FROM a\nCOPY <<EOF /etc/password\nrosebud7\nEOF\n",
                "rosebud7",
            ),
            // F15: brackets are not proof of an IPv6 address.
            (
                "docker-compose.yml",
                "services:\n  a:\n    environment:\n      API_HOST: \"[rosebud7]\"\n",
                "rosebud7",
            ),
            // F17: registry URLs are identities (query masked).
            (
                "package.json",
                r#"{"publishConfig":{"registry":"https://registry.example.com/?token=rosebud7"}}"#,
                "rosebud7",
            ),
            // F6: a format with no redaction rule still masks C4-named values.
            (
                "job.jcl",
                "//STEP EXEC PGM=X,PASSWORD=rosebud7\n",
                "rosebud7",
            ),
        ];
        for (path, src, planted) in cases {
            let out = same_len(path, &fill(src));
            assert!(!out.contains(planted), "{path}: `{planted}` kept:\n{out}");
        }
    }

    /// Review x0-r1 over-redaction findings: each identity survives.
    #[test]
    fn review_r1_identities_survive() {
        let cases: &[(&str, &str, &str)] = &[
            // O2: a dependency named like a deny word keeps its spec.
            (
                "package.json",
                r#"{"dependencies":{"auth":"^1.2.3","cookie":"0.6.0"}}"#,
                "^1.2.3",
            ),
            // O3: git pins and the ssh transport user are identity.
            (
                "package.json",
                r#"{"dependencies":{"lib":"git+ssh://git@github.com/acme/lib.git#0123456789abcdef0123456789abcdef01234567"}}"#,
                "git@github.com/acme/lib.git#0123456789abcdef0123456789abcdef01234567",
            ),
            (
                "Cargo.toml",
                "[dependencies]\nx = { git = \"https://github.com/o/x\", rev = \"0123456789abcdef0123456789abcdef01234567\" }\n",
                "0123456789abcdef0123456789abcdef01234567",
            ),
            // O5: route paths are not entropy hits.
            (
                "R.java",
                "class R { @GetMapping(\"/api/v1/customer/v2/profile\") void p() {} }\n",
                "/api/v1/customer/v2/profile",
            ),
            // O6: module specifiers, whatever the spacing or language.
            (
                "a.js",
                "const x = require (\"0123456789abcdef0123456789abcdef\");\n",
                "0123456789abcdef0123456789abcdef",
            ),
            (
                "a.rb",
                "require \"0123456789abcdef0123456789abcdef\"\n",
                "0123456789abcdef0123456789abcdef",
            ),
            // O7: the expression inside an interpolated secret literal stays.
            (
                "i.js",
                "const password = `prefix${getPassword()}`;\n",
                "${getPassword()}",
            ),
            // O10: CloudFormation types, dependencies and references.
            (
                "cfn.yaml",
                "AWSTemplateFormatVersion: '2010-09-09'\nResources:\n  B:\n    Type: AWS::S3::Bucket\n    DependsOn: Other\n    Properties:\n      BucketName: !Ref Name\n",
                "Type: AWS::S3::Bucket",
            ),
            (
                "cfn.yaml",
                "Resources:\n  B:\n    Type: AWS::S3::Bucket\n    DependsOn: Other\n    Properties:\n      BucketName: !Ref NameParam\n",
                "!Ref NameParam",
            ),
            (
                "cfn.yaml",
                "Resources:\n  B:\n    Type: AWS::S3::Bucket\n    DependsOn: Other\n",
                "DependsOn: Other",
            ),
            // JCL `DSN=` is a dataset name, not a database DSN (CI: cross_language_estate).
            (
                "job.jcl",
                "//CUSTFILE DD DSN=PAYROLL.MASTER.KSDS,DISP=SHR\n",
                "DSN=PAYROLL.MASTER.KSDS",
            ),
            // O11: target-specific build dependencies.
            (
                "Cargo.toml",
                "[target.'cfg(unix)'.build-dependencies]\nhelper = { path = \"../helper\" }\n",
                "../helper",
            ),
            // R4-02: lockfile integrity + resolved survive whole.
            (
                "package-lock.json",
                r#"{"packages":{"node_modules/x":{"resolved":"git+ssh://git@github.com/o/x.git#0123456789abcdef0123456789abcdef01234567","integrity":"sha512-abc"}}}"#,
                "0123456789abcdef0123456789abcdef01234567",
            ),
        ];
        for (path, src, kept) in cases {
            let out = same_len(path, src);
            assert!(out.contains(kept), "{path}: `{kept}` lost:\n{out}");
        }
        // O8: masked block scalars and heredocs stay parseable structure (indentation, delimiters).
        let y = same_len("v.yaml", "a: |\n  line one\n  line two\nb: x\n");
        assert!(
            y.starts_with("a: |\n  ") && y.contains("\n  REDACTED") && y.contains("\nb: "),
            "{y}"
        );
        let h = same_len("m.tf", "x = <<EOT\nhello\nEOT\n");
        assert!(
            h.starts_with("x = <<EOT\n") && h.ends_with("\nEOT\n"),
            "{h}"
        );
    }

    /// Review x0-r2 cases.
    #[test]
    fn review_r2_cases() {
        let gone: &[(&str, &str, &str)] = &[
            // F2: a `KEY=false` list item is a string, not a boolean.
            (
                "docker-compose.yml",
                "services:\n  app:\n    environment:\n      - 'PASSWORD=false'\n",
                "=false",
            ),
            ("x.dockerfile", "FROM a\nENV PASSWORD=false\n", "=false"),
            // F13: an inline authorization scheme inside a quoted argument.
            (
                "package.json",
                r#"{"scripts":{"send":"curl -H 'Authorization:Bearer rosebud7' https://example.com"}}"#,
                "rosebud7",
            ),
            // `//` in a URL or a string is not a comment (review x0-r3).
            (
                "u.js",
                "fetch(\"http://api.example.com\"); const password = \"hunter2brown\";\n",
                "hunter2brown",
            ),
            (
                "v.js",
                "let a = \"x//y\"; password = \"hunter2brown\";\n",
                "hunter2brown",
            ),
            (
                "w.js",
                "const password = // the db one\n  \"hunter2brown\";\n",
                "hunter2brown",
            ),
            // A long comment inside a typed declaration (review x0-r4).
            (
                "t.ts",
                "const password: string /* local development database password used by the integration test runner */ = \"rosebud7\";\n",
                "rosebud7",
            ),
            // Exported / visibility-qualified, split over lines, generic before `=` (review x0-r8).
            (
                "x.ts",
                "export const password: string = \"rosebud7\";\n",
                "rosebud7",
            ),
            (
                "x.rs",
                "pub(crate) const TOKEN: &str = \"rosebud7\";\n",
                "rosebud7",
            ),
            (
                "y.ts",
                "const password:\n  string = \"rosebud7\";\n",
                "rosebud7",
            ),
            (
                "z.ts",
                "const password: NonNullable<string>=\"rosebud7\";\n",
                "rosebud7",
            ),
            // Nested types (review x0-r7).
            (
                "n.py",
                "from typing import Optional\npassword: Optional[Optional[str]] = \"rosebud7\"\n",
                "rosebud7",
            ),
            (
                "n.ts",
                "const password: NonNullable<Readonly<string>> = \"rosebud7\";\n",
                "rosebud7",
            ),
            // Comma-bearing types still bind (review x0-r6).
            (
                "u.py",
                "from typing import Union\npassword: Union[str, bytes] = \"rosebud7\"\n",
                "rosebud7",
            ),
            (
                "t2.ts",
                "const password: string | [string, string] = \"rosebud7\";\n",
                "rosebud7",
            ),
            (
                "t3.ts",
                "const password: Map<string, string> = \"rosebud7\" as any;\n",
                "rosebud7",
            ),
            // Destructuring: the typed match stays inside one binding (review x0-r5).
            (
                "d.js",
                "const { label: title /* a display title used when the caller does not supply a title in the options object */, password: value = \"rosebud7\" } = options;\n",
                "rosebud7",
            ),
            // A comment between the denied binding and its literal.
            (
                "c.js",
                "const password = /* production */ \"rosebud7\";\n",
                "rosebud7",
            ),
            // Swift raw interpolated string: its literal fragments are masked.
            (
                "p.swift",
                "let password = #\"rosebud7\\#(suffix())\"#\n",
                "rosebud7",
            ),
            // ... and its trailing fragment (review x0-r3).
            (
                "p2.swift",
                "let password = #\"prefix\\#(suffix())rosebud7\"#\n",
                "rosebud7",
            ),
            // COPY heredoc data.
            (
                "b.dockerfile",
                "FROM alpine:3.20 AS build\nCOPY <<EOF /app/config\nrosebud7\nEOF\nFROM busybox:1.36 AS final\n",
                "rosebud7",
            ),
        ];
        for (path, src, planted) in gone {
            let out = same_len(path, src);
            assert!(!out.contains(planted), "{path}: `{planted}` kept:\n{out}");
        }
        // Swift raw delimiters survive masking.
        let raw = same_len(
            "r.swift",
            "let password = #\"prefix\\#(suffix())rosebud7\"#\n",
        );
        assert!(
            raw.contains("= #\"") && raw.contains("\"#\n") && raw.contains("suffix()"),
            "{raw}"
        );
        // Swift ordinary interpolation: the embedded call stays.
        let sw = same_len("q.swift", "let password = \"prefix\\(suffix())\"\n");
        assert!(
            sw.contains("\\(suffix())") && !sw.contains("prefix"),
            "{sw}"
        );
        // The heredoc still closes and the next stage keeps its identity.
        let d = same_len(
            "b.dockerfile",
            "FROM alpine:3.20 AS build\nCOPY <<EOF /app/config\nrosebud7\nEOF\nFROM busybox:1.36 AS final\n",
        );
        assert!(d.contains("\nEOF\nFROM busybox:1.36 AS final\n"), "{d}");
        // The quoted header masks through its close and no further (review x0-r3).
        let c = same_len(
            "package.json",
            r#"{"scripts":{"send":"curl -H 'Authorization:Bearer rosebud7' https://example.com"}}"#,
        );
        assert!(
            !c.contains("rosebud7") && c.contains("https://example.com"),
            "{c}"
        );
        // ... and does not reach a neighbouring binding's default either (review x0-r5).
        let d = same_len(
            "e.js",
            "const { password: supplied, title /* a display title used when the caller does not supply a title in the options object */ = \"Welcome\" } = options;\n",
        );
        assert!(d.contains("\"Welcome\""), "{d}");
        // HCL booleans stay booleans.
        let h = same_len("m.tf", "resource \"a\" \"b\" {\n  enabled = true\n}\n");
        assert!(h.contains("enabled = true"), "{h}");
    }

    #[test]
    fn mask_all_keeps_layout() {
        assert_eq!(mask_all("A=1 bb\n  cc"), "RED RE\n  RE");
    }

    #[test]
    fn literal_interior_shapes() {
        assert_eq!(literal_interior("\"abc\""), (1, 4));
        assert_eq!(literal_interior("'a'"), (1, 2));
        assert_eq!(literal_interior("\"\"\"doc\"\"\""), (3, 6));
        assert_eq!(literal_interior("r#\"raw\"#"), (3, 6));
        assert_eq!(literal_interior("\"\""), (1, 1));
    }
}
