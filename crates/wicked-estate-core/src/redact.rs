//! Secret-redaction primitives shared by the extractor and every store (wicked-estate#405,
//! DES-S7 §5).
//!
//! Two layers use these:
//!
//! - the **extractor** (`wicked_estate_extract::redact`) applies the format-aware default-deny
//!   policy to a file's text BEFORE it is parsed, so nodes, signatures, the full-text index and
//!   the stored `content` all derive from the masked text;
//! - every **store write** of a free-text column runs [`scrub`] (known token shapes, private-key
//!   blocks, URL credentials) as a backstop for ingress that never passes the extractor
//!   (telemetry imports, annotations, SCIP symbol strings, tfstate ids, meta values).
//!
//! Masking is **same-length**: every masked byte is replaced by one ASCII byte and line breaks
//! are kept, so byte spans and line numbers recorded against the original file stay valid. The
//! cost, stated in the docs' residuals: a masked value's length is still visible.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;

/// Version of the redaction policy a store was cleaned with. A code-graph store whose
/// [`REDACTION_EPOCH_KEY`] meta value is lower (or absent) holds text written before this policy
/// and must be migrated before its text is served.
///
/// 2 (estate#686): XML values under secret names are masked. A store cleaned at 1 is re-redacted
/// once on its first write open (text rewritten, every file re-extracted, the file vacuumed); the
/// bump also covers the JSON rules redaction change of #670, which shipped without one.
pub const REDACTION_EPOCH: u32 = 2;
/// The meta key recording [`REDACTION_EPOCH`] on a migrated or freshly indexed store.
pub const REDACTION_EPOCH_KEY: &str = "redaction_epoch";

/// Version of the free-text policy used by memory and knowledge stores (#406).
pub const TEXT_REDACTION_EPOCH: u32 = 1;
/// Meta key recording a fresh or explicitly cleaned text store.
pub const TEXT_REDACTION_EPOCH_KEY: &str = "text_redaction_epoch";

const TEXT_MASK: &[u8] = b"REDACTED";

/// What every answer that returns stored source text says about it (DES-S7 §5, operator decision
/// D1): the guarantee and the two residuals, stated plainly.
pub const PRIVACY_NOTE: &str = "PRIVACY: stored source text is secret-redacted (estate#405). \
Config values are default-deny (masked as REDACTED, same length, so a masked value's length shows); \
code is masked only where a secret is detected (a known token shape, or a literal bound to a \
secret-named variable); file types without a redaction rule get only those two checks. Not \
detected: a secret under an innocent name in code, and a secret placed inside a hostname or \
repository owner. Memory and knowledge text masks known token shapes and secret-named assignments \
(estate#406); secrets under innocent names in prose are not detected.";

/// Replace `buf[start..end]` with the text mask (`REDACTEDREDACTED...`), keeping `\n`, `\r` and
/// the indentation at the start of every line inside the range (a YAML block scalar or a heredoc
/// body stays inside its block). Out-of-range or inverted ranges are clamped, never a panic.
pub fn mask_text(buf: &mut [u8], start: usize, end: usize) {
    let end = end.min(buf.len());
    if start >= end {
        return;
    }
    let mut i = 0;
    let mut line_start = start == 0 || matches!(buf[start - 1], b'\n' | b'\r');
    for b in &mut buf[start..end] {
        if *b == b'\n' || *b == b'\r' {
            line_start = true;
            continue;
        }
        if line_start && (*b == b' ' || *b == b'\t') {
            continue;
        }
        line_start = false;
        *b = TEXT_MASK[i % TEXT_MASK.len()];
        i += 1;
    }
}

/// Mask a bare numeric literal so it stays a valid number in JSON/TOML/HCL: the first digit
/// becomes `1`, every later digit `0`; signs, dots and exponents are kept.
pub fn mask_numeric(buf: &mut [u8], start: usize, end: usize) {
    let end = end.min(buf.len());
    if start >= end {
        return;
    }
    // A radix prefix (`0x`, `0o`, `0b`, after an optional sign) is kept; its digits (hex letters
    // included) are masked like decimal ones.
    let mut i = start;
    if matches!(buf[i], b'+' | b'-') {
        i += 1;
    }
    let mut hex = false;
    if i + 1 < end
        && buf[i] == b'0'
        && matches!(buf[i + 1], b'x' | b'X' | b'o' | b'O' | b'b' | b'B')
    {
        hex = matches!(buf[i + 1], b'x' | b'X');
        i += 2;
    }
    let mut first = true;
    for b in &mut buf[i..end] {
        if b.is_ascii_digit() || (hex && b.is_ascii_hexdigit()) {
            *b = if first { b'1' } else { b'0' };
            first = false;
        }
    }
}

/// Apply byte ranges as text masks to a copy of `text`. Ranges must fall on char boundaries
/// (every producer in this crate and the extractor emits them from `&str` matches or tree-sitter
/// byte offsets); the result is valid UTF-8 because masking only writes ASCII over whole chars.
pub fn apply_masks(text: &str, ranges: &[(usize, usize)]) -> String {
    if ranges.is_empty() {
        return text.to_string();
    }
    let mut buf = text.as_bytes().to_vec();
    for &(s, e) in ranges {
        mask_text(&mut buf, s, e);
    }
    // Masking replaces whole UTF-8 sequences with ASCII only when the range covers the whole
    // sequence; a range that splits one is widened here rather than trusted.
    match String::from_utf8(buf) {
        Ok(s) => s,
        Err(_) => {
            let mut buf = text.as_bytes().to_vec();
            for &(s, e) in ranges {
                let (s, e) = widen_to_char_boundaries(text, s, e);
                mask_text(&mut buf, s, e);
            }
            String::from_utf8(buf).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into())
        }
    }
}

fn widen_to_char_boundaries(text: &str, mut s: usize, mut e: usize) -> (usize, usize) {
    s = s.min(text.len());
    e = e.min(text.len());
    while s > 0 && !text.is_char_boundary(s) {
        s -= 1;
    }
    while e < text.len() && !text.is_char_boundary(e) {
        e += 1;
    }
    (s, e)
}

// ── C3: known token shapes ─────────────────────────────────────────────────────────────────────

/// Known credential shapes (DES-S7 §5 C3). High precision: a match is a credential format, not a
/// guess. Strict prefixed shapes carry no word boundaries, so a token glued to an identifier on
/// either side (`call_ghp_..._x`, `x_AKIA..._suffix`) still matches; the loose `sk-` / `npm_`
/// shapes keep them. No shape can match a quote, brace or comma, so scrubbing serialized JSON
/// never crosses a field boundary. Each pattern's group 1, when present, is the part masked;
/// otherwise the whole match.
static SHAPES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        // AWS access key ids.
        r"(?:AKIA|ASIA|AGPA|AIDA|AROA|ANPA|ANVA|AIPA)[0-9A-Z]{16}",
        // GitHub tokens (classic + fine-grained).
        r"gh[pousr]_[A-Za-z0-9]{30,255}",
        r"github_pat_[A-Za-z0-9_]{22,255}",
        // GitLab personal access tokens.
        r"glpat-[A-Za-z0-9_\-]{20,}",
        // Slack tokens and webhooks.
        r"xox[abposr]-[A-Za-z0-9\-]{10,}",
        r"hooks\.slack\.com/services/([A-Za-z0-9/]{16,})",
        // Stripe secret and restricted keys.
        r"[sr]k_(?:live|test)_[A-Za-z0-9]{16,}",
        // Google API keys.
        r"AIza[0-9A-Za-z_\-]{35}",
        // OpenAI / Anthropic style keys.
        r"\bsk-(?:ant-|proj-)?[A-Za-z0-9_\-]{20,}\b",
        // npm tokens.
        r"\bnpm_[A-Za-z0-9]{36}\b",
        // JSON Web Tokens.
        r"\beyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}",
        // A private-key block: the body between the markers is masked, the markers kept.
        r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----([A-Za-z0-9+/=\s\\:,\-]*?)-----END [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----",
    ]
    .iter()
    .map(|p| Regex::new(p).expect("redaction shape compiles"))
    .collect()
});

/// URL userinfo (`scheme://user:pass@host`): group 1 is the userinfo.
static URL_USERINFO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\b[A-Za-z][A-Za-z0-9+.\-]*://([^\s/@'"<>()]+)@"#).expect("userinfo compiles")
});

/// Byte ranges of every known token shape and URL credential in `text`.
pub fn token_shape_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for re in SHAPES.iter() {
        for c in re.captures_iter(text) {
            let m = c
                .get(1)
                .or_else(|| c.get(0))
                .expect("group 0 always matches");
            out.push((m.start(), m.end()));
        }
    }
    for c in URL_USERINFO.captures_iter(text) {
        let m = c.get(1).expect("userinfo group");
        let info = m.as_str();
        // `git@` / `deploy@` style usernames carry no secret; a password part, or a long
        // username (a token used as the user, as GitHub accepts), does.
        if info.contains(':') || info.len() >= 12 {
            out.push((m.start(), m.end()));
        }
    }
    out
}

// C4 in free text: match the name and operator first, then inspect the value without crossing
// JSON escape boundaries. No entropy checks: ordinary prose and commit hashes must survive.
static TEXT_ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?P<key>"[A-Za-z_][\w.\-]*"|'[A-Za-z_][\w.\-]*'|[A-Za-z_][\w.\-]*)[ \t]*(?P<op>:=|=|:)"#,
    )
    .expect("free-text assignment compiles")
});
static TEXT_VALUE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^[^\s,;)'"\\<>]+"#).expect("free-text value compiles"));

/// Known token shapes, URL credentials and private keys, plus values assigned to secret-named
/// keys in prose. A colon needs whitespace or a quote, so scope paths and URLs are kept.
pub fn free_text_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = token_shape_ranges(text);
    for caps in TEXT_ASSIGNMENT.captures_iter(text) {
        let key = caps.name("key").expect("key group");
        if text[..key.start()]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '-'))
        {
            continue;
        }
        let name = key.as_str().trim_matches(['\'', '"']);
        if !deny_key(name) {
            continue;
        }
        let op = caps.name("op").expect("operator group");
        let tail = &text[op.end()..];
        if (op.as_str() == "=" && tail.starts_with(['=', '>', '~']))
            || (op.as_str() == ":" && !tail.starts_with([' ', '\t', '\'', '"']))
        {
            continue;
        }
        let value = tail.trim_start_matches([' ', '\t']);
        let start = text.len() - value.len();
        if let Some(quote) = value.chars().next().filter(|c| matches!(*c, '\'' | '"')) {
            // An escaped quote belongs to the value. Mask the entire interior, keeping the
            // delimiters, and stop at an unescaped closing quote. An unterminated quote (a
            // pasted fragment) masks to the end of its line.
            let mut escaped = false;
            let mut end = value[1..].find(['\n', '\r']).map_or(value.len(), |i| i + 1);
            for (i, c) in value[1..].char_indices() {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == quote || c == '\n' || c == '\r' {
                    end = 1 + i;
                    break;
                }
            }
            ranges.push((start + 1, start + end));
        } else if let Some(run) = TEXT_VALUE.find(value) {
            ranges.push((start, start + run.end()));
            if name.eq_ignore_ascii_case("authorization")
                && (run.as_str().eq_ignore_ascii_case("bearer")
                    || run.as_str().eq_ignore_ascii_case("basic"))
            {
                let rest = &value[run.end()..];
                let token = rest.trim_start_matches([' ', '\t']);
                if token.len() < rest.len() {
                    if let Some(tok) = TEXT_VALUE.find(token) {
                        let offset = text.len() - token.len();
                        ranges.push((offset, offset + tok.end()));
                    }
                }
            }
        }
    }
    // Merge overlaps so nested assignments and token shapes share one stable mask origin.
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut().filter(|last| start < last.1) {
            last.1 = last.1.max(end);
        } else if start < end {
            merged.push((start, end));
        }
    }
    merged
}

/// Same-length, idempotent free-text masking. Borrows text when there is no match or change.
pub fn redact_free_text(text: &str) -> Cow<'_, str> {
    let ranges = free_text_ranges(text);
    if ranges.is_empty() {
        return Cow::Borrowed(text);
    }
    let redacted = apply_masks(text, &ranges);
    if redacted == text {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(redacted)
    }
}

/// Redact every string inside a JSON value, keeping keys. Returns whether anything changed.
pub fn redact_free_json(value: &mut serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(s) => {
            if let Cow::Owned(redacted) = redact_free_text(s) {
                *s = redacted;
                true
            } else {
                false
            }
        }
        serde_json::Value::Array(a) => a.iter_mut().fold(false, |acc, v| redact_free_json(v) | acc),
        serde_json::Value::Object(m) => m
            .values_mut()
            .fold(false, |acc, v| redact_free_json(v) | acc),
        _ => false,
    }
}

/// Redact a text node's name, signature, doc and metadata strings. Stable identity, kind and
/// location are kept. `None` means the node was already safe under the free-text policy.
pub fn redact_text_node(node: &crate::Node) -> Option<crate::Node> {
    let mut out = node.clone();
    let mut changed = false;
    for s in std::iter::once(&mut out.name)
        .chain(out.signature.iter_mut())
        .chain(out.doc.iter_mut())
    {
        if let Cow::Owned(redacted) = redact_free_text(s) {
            *s = redacted;
            changed = true;
        }
    }
    for value in out.metadata.values_mut() {
        // Proposals and approval journals carry opaque JSON as a metadata string. Decode
        // values before masking their strings: escaped quotes must not hide assignments,
        // and masking prose ranges over JSON punctuation must not corrupt the payload.
        if let serde_json::Value::String(text) = value {
            if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(text) {
                let strings_changed = redact_free_json(&mut json);
                // Content can itself be JSON prose, so also apply the assignment policy to
                // the compact representation (where numeric/boolean colons stay glued).
                let compact = json.to_string();
                let redacted = redact_free_text(&compact);
                if strings_changed || redacted.as_ref() != compact.as_str() {
                    *text = redacted.into_owned();
                    changed = true;
                }
                continue;
            }
        }
        changed |= redact_free_json(value);
    }
    changed.then_some(out)
}

/// The store-boundary backstop: mask known token shapes and URL credentials in `text`.
/// Returns the input unchanged (borrowed) when nothing matched, so the common path allocates
/// nothing.
pub fn scrub(text: &str) -> Cow<'_, str> {
    let ranges = token_shape_ranges(text);
    if ranges.is_empty() {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(apply_masks(text, &ranges))
    }
}

/// [`scrub`] for an optional column.
pub fn scrub_opt(text: Option<&str>) -> Option<Cow<'_, str>> {
    text.map(scrub)
}

/// [`scrub`] in place; true when anything changed.
pub fn scrub_string(s: &mut String) -> bool {
    match scrub(s) {
        Cow::Borrowed(_) => false,
        Cow::Owned(o) => {
            *s = o;
            true
        }
    }
}

/// [`scrub`] every string inside a JSON value (keys are names, not values, and are kept).
pub fn scrub_json(v: &mut serde_json::Value) -> bool {
    match v {
        serde_json::Value::String(s) => scrub_string(s),
        serde_json::Value::Array(a) => a.iter_mut().fold(false, |acc, x| scrub_json(x) | acc),
        serde_json::Value::Object(m) => m.values_mut().fold(false, |acc, x| scrub_json(x) | acc),
        _ => false,
    }
}

/// The store-boundary backstop for nodes: signature, doc and metadata strings. Identity fields
/// (symbol, name, location) are never rewritten (ADR-002). `None` when nothing changed.
pub fn scrub_node(n: &crate::Node) -> Option<crate::Node> {
    let mut out = n.clone();
    let mut changed = false;
    if let Some(s) = out.signature.as_mut() {
        changed |= scrub_string(s);
    }
    if let Some(d) = out.doc.as_mut() {
        changed |= scrub_string(d);
    }
    for v in out.metadata.values_mut() {
        changed |= scrub_json(v);
    }
    changed.then_some(out)
}

/// The store-boundary backstop for edges: `resolved_by` and the metadata strings.
pub fn scrub_edge(e: &crate::Edge) -> Option<crate::Edge> {
    let mut out = e.clone();
    let mut changed = scrub_string(&mut out.resolved_by);
    for v in out.metadata.values_mut() {
        changed |= scrub_json(v);
    }
    changed.then_some(out)
}

/// The store-boundary backstop for unresolved references: the written name and the hints.
pub fn scrub_ref(r: &crate::UnresolvedRef) -> Option<crate::UnresolvedRef> {
    let mut out = r.clone();
    let mut changed = scrub_string(&mut out.raw_name);
    for v in out.hints.values_mut() {
        changed |= scrub_json(v);
    }
    changed.then_some(out)
}

/// Apply `f` (one of the `scrub_*` functions) to a slice, borrowing it when nothing changed.
pub fn scrub_all<'a, T: Clone>(items: &'a [T], f: impl Fn(&T) -> Option<T>) -> Cow<'a, [T]> {
    let mut out: Option<Vec<T>> = None;
    for (i, it) in items.iter().enumerate() {
        if let Some(s) = f(it) {
            out.get_or_insert_with(|| items[..i].to_vec()).push(s);
        } else if let Some(o) = out.as_mut() {
            o.push(it.clone());
        }
    }
    match out {
        Some(v) => Cow::Owned(v),
        None => Cow::Borrowed(items),
    }
}

/// The store-boundary backstop for annotations: every free-text field.
pub fn scrub_annotation(a: &mut crate::annotation::Annotation) {
    for s in [
        &mut a.key,
        &mut a.value,
        &mut a.provenance,
        &mut a.author,
        &mut a.r#type,
        &mut a.source_type,
        &mut a.extraction_method,
    ] {
        scrub_string(s);
    }
}

// ── C3: entropy and encoded runs (used on literal and config VALUES, never on prose) ──────────

/// Shannon entropy of `s` in bits per byte.
pub fn shannon_entropy(s: &str) -> f64 {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return 0.0;
    }
    let mut counts = [0usize; 256];
    for &b in bytes {
        counts[b as usize] += 1;
    }
    let n = bytes.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / n;
            -p * p.log2()
        })
        .sum()
}

static ENCODED_RUN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9+_\-]{20,}={0,2}").expect("encoded run compiles"));
static HEX_RUN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[0-9a-fA-F]{32,}\b").expect("hex run compiles"));

/// Byte ranges inside a literal VALUE that C3 flags: known shapes, URL credentials, hex runs of
/// at least 32 chars, base64-like runs (at least 24 chars, all three character classes, entropy
/// of at least 4.0 bits) and random segments (at least 16 chars between separators, two or more
/// digits, entropy near the maximum for the length). Used on string literals in code and on
/// config values; prose and comments get [`token_shape_ranges`] only, because English text and
/// paths trip entropy rules.
pub fn value_secret_ranges(value: &str) -> Vec<(usize, usize)> {
    let mut out = token_shape_ranges(value);
    for m in HEX_RUN.find_iter(value) {
        out.push((m.start(), m.end()));
    }
    for m in ENCODED_RUN.find_iter(value) {
        let t = m.as_str();
        let (digits, upper, lower) = classes(t);
        // A base64-like run: all three character classes and the entropy of random data (a
        // 24-char random base64 string sits near 4.2 bits; mixed-case identifiers near 3.8).
        let base64_like = t.len() >= 24 && digits && upper && lower && shannon_entropy(t) >= 4.0;
        // One long random segment between separators (`-`, `_`, `.`): package names and
        // snake/kebab identifiers are made of short segments and never trip this.
        // "Random" = at least two digits and an entropy close to the maximum a string of that
        // length can have (identifiers repeat letters; random tokens do not).
        let random_segment = t.split(['-', '_', '.']).any(|seg| {
            let (_, u, l) = classes(seg);
            let digits = seg.bytes().filter(u8::is_ascii_digit).count();
            seg.len() >= 16
                && digits >= 2
                && (u || l)
                && shannon_entropy(seg) >= 0.8 * (seg.len() as f64).log2()
        });
        if base64_like || random_segment {
            out.push((m.start(), m.end()));
        }
    }
    out
}

fn classes(t: &str) -> (bool, bool, bool) {
    (
        t.bytes().any(|b| b.is_ascii_digit()),
        t.bytes().any(|b| b.is_ascii_uppercase()),
        t.bytes().any(|b| b.is_ascii_lowercase()),
    )
}

/// True when C3 flags anything in `value`.
pub fn value_looks_secret(value: &str) -> bool {
    !value_secret_ranges(value).is_empty()
}

// ── C4: key-name deny ──────────────────────────────────────────────────────────────────────────

const DENY_WORDS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "pass",
    "passphrase",
    "secret",
    "secrets",
    "token",
    "tokens",
    "credential",
    "credentials",
    "creds",
    "auth",
    "oauth",
    "authorization",
    "cert",
    "signature",
    "salt",
    "session",
    "cookie",
    "dsn",
    "private",
    "apikey",
    "accesskey",
    "secretkey",
    "privatekey",
    "connectionstring",
    "bearer",
];
const DENY_PAIRS: &[(&str, &str)] = &[
    ("api", "key"),
    ("access", "key"),
    ("secret", "key"),
    ("private", "key"),
    ("signing", "key"),
    ("encryption", "key"),
    ("master", "key"),
    ("client", "secret"),
    ("connection", "string"),
];

/// Split an identifier or config key into lower-case words: on every non-alphanumeric byte and
/// on camelCase boundaries (`apiKey` -> `api`, `key`; `DB_PASSWORD` -> `db`, `password`).
pub fn key_words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            continue;
        }
        let boundary = c.is_uppercase()
            && !cur.is_empty()
            && (chars[i - 1].is_lowercase()
                || chars.get(i + 1).is_some_and(|n| n.is_lowercase())
                    && chars[i - 1].is_uppercase());
        if boundary {
            words.push(std::mem::take(&mut cur));
        }
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

/// C4: true when a key or binding name names a secret (`password`, `apiKey`, `DB_TOKEN`,
/// `client_secret`, ...). Deny wins over every allow rule.
pub fn deny_key(name: &str) -> bool {
    let words = key_words(name);
    if words.iter().any(|w| DENY_WORDS.contains(&w.as_str())) {
        return true;
    }
    words
        .windows(2)
        .any(|p| DENY_PAIRS.iter().any(|(a, b)| p[0] == *a && p[1] == *b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_nodes_clean_serialized_payloads_without_corrupting_json() {
        let mut node = crate::Node::new(
            crate::Symbol::synthetic("proposal", "legacy").id(),
            crate::NodeKind::Other("proposal".into()),
            "legacy proposal",
            crate::Language::new("proposal"),
            crate::Location::new("proposal", crate::Span::ZERO),
        );
        let payload = serde_json::json!({"content": "password: \"hunter2-example\""});
        node.metadata
            .insert("payload".into(), payload.to_string().into());
        node.metadata.insert(
            "scalar".into(),
            serde_json::json!("password: \"hunter2-example\"")
                .to_string()
                .into(),
        );
        node.metadata
            .insert("numeric".into(), r#"{"token": 123}"#.into());
        node.metadata
            .insert("content".into(), r#"{"password":"hunter2-example"}"#.into());
        let clean = redact_text_node(&node).unwrap();
        let content: serde_json::Value =
            serde_json::from_str(clean.metadata["content"].as_str().unwrap()).unwrap();
        assert!(content.get("password").is_some());
        assert!(!content.to_string().contains("hunter2-example"));
        let payload: serde_json::Value =
            serde_json::from_str(clean.metadata["payload"].as_str().unwrap()).unwrap();
        assert!(payload.get("content").is_some());
        assert!(!payload.to_string().contains("hunter2-example"));
        let scalar: serde_json::Value =
            serde_json::from_str(clean.metadata["scalar"].as_str().unwrap()).unwrap();
        assert!(scalar.is_string());
        assert!(!scalar.as_str().unwrap().contains("hunter2-example"));
        let numeric: serde_json::Value =
            serde_json::from_str(clean.metadata["numeric"].as_str().unwrap()).unwrap();
        assert_eq!(numeric["token"], 123);
        assert!(redact_text_node(&clean).is_none());
    }

    #[test]
    fn free_text_policy_masks_assignments_and_preserves_prose() {
        for (input, value) in [
            ("key AKIAIOSFODNN7EXAMPLE", "AKIAIOSFODNN7EXAMPLE"),
            (
                "key ghp_EXAMPLEexampleEXAMPLEexampleEXAMPLE12",
                "ghp_EXAMPLEexampleEXAMPLEexampleEXAMPLE12",
            ),
            ("DB_PASSWORD=hunter2-example", "hunter2-example"),
            (r#"password: "hunter2-example""#, "hunter2-example"),
            ("password: \"hunter2-example\nnext line", "hunter2-example"),
            (r#"'password' := 'hunter2-example'"#, "hunter2-example"),
            (r#"password="token=hunter2-example""#, "hunter2-example"),
            ("Authorization: Bearer hunter2-example", "hunter2-example"),
            ("Authorization: Basic hunter2-example", "hunter2-example"),
            ("auth: done", "done"),
            (
                r#"{"content":"DB_PASSWORD=hunter2-example\nnext"}"#,
                "hunter2-example",
            ),
        ] {
            let red = redact_free_text(input);
            assert!(!red.contains(value), "value survived free-text redaction");
            assert_eq!(red.len(), input.len());
            assert!(!free_text_ranges(input).is_empty());
            assert_eq!(redact_free_text(&red), red, "redaction must be idempotent");
        }
        for input in [
            "session:abc123",
            "auth:x",
            "https://x.example.com/a",
            "token == other",
            "token => other",
            "token =~ other",
            "auth::done",
            "0123456789abcdef0123456789abcdef01234567",
            "ordinary prose with no assignment",
        ] {
            assert!(
                free_text_ranges(input).is_empty(),
                "unexpected mask: {input}"
            );
            assert_eq!(redact_free_text(input), input);
        }
        assert_eq!(
            redact_free_text(r#"password: "hunter2-example""#),
            r#"password: "REDACTEDREDACTE""#,
        );
        let mut json = serde_json::json!({"password": ["DB_PASSWORD=hunter2-example"]});
        assert!(redact_free_json(&mut json));
        assert!(json.get("password").is_some(), "keys must be kept");
        assert!(!json.to_string().contains("hunter2-example"));
        assert!(
            !redact_free_json(&mut json),
            "a second pass changes nothing"
        );
    }

    #[test]
    fn masks_keep_length_and_line_breaks() {
        let mut b = b"ab\ncdefghijk".to_vec();
        mask_text(&mut b, 1, 12);
        assert_eq!(b.len(), 12);
        assert_eq!(&b, b"aR\nEDACTEDRE");
        let mut n = b"port: 54321".to_vec();
        mask_numeric(&mut n, 6, 11);
        assert_eq!(&n, b"port: 10000");
    }

    #[test]
    fn scrub_masks_known_shapes_only() {
        let t = "key ghp_abcdefghijklmnopqrstuvwxyz0123456789 and AKIAABCDEFGHIJKLMNOP end";
        let s = scrub(t);
        assert_eq!(s.len(), t.len());
        assert!(!s.contains("ghp_abc"));
        assert!(!s.contains("AKIAABCD"));
        assert!(s.starts_with("key ") && s.ends_with(" end"));
        assert!(matches!(
            scrub("plain words and a_long_identifier_name"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn url_credentials_are_masked_but_plain_users_kept() {
        let s = scrub("git clone https://bob:hunter2pass@example.test/x.git");
        assert!(!s.contains("hunter2pass"));
        assert!(s.contains("@example.test/x.git"));
        assert_eq!(
            scrub("ssh://git@example.test/a/b.git"),
            "ssh://git@example.test/a/b.git"
        );
    }

    #[test]
    fn private_key_body_is_masked() {
        let t = "-----BEGIN RSA PRIVATE KEY-----\nMIIBOgIBAAJBAKj34\n-----END RSA PRIVATE KEY-----";
        let s = scrub(t);
        assert!(!s.contains("MIIBOgIBAAJBAKj34"));
        assert!(s.starts_with("-----BEGIN RSA PRIVATE KEY-----\n"));
        assert_eq!(s.lines().count(), 3);
    }

    #[test]
    fn value_rules_flag_encoded_and_entropy_runs() {
        // Fakes assembled at runtime (no secret-shaped literal in the source).
        assert!(value_looks_secret(
            &["UExBTlRF", "RGs4c1Nl", "Y3JldA=="].concat()
        ));
        assert!(value_looks_secret(
            &["a8f3k29d", "k3m4n5b6", "v7c8x9z0"].concat()
        ));
        assert!(value_looks_secret("0123456789abcdef0123456789abcdef"));
        assert!(!value_looks_secret("application/json"));
        assert!(!value_looks_secret("src/components/header"));
        assert!(!value_looks_secret("handleUserAuthenticationRequest"));
        assert!(!value_looks_secret("pkg-name-with-many-parts-2024"));
        assert!(!value_looks_secret("UserProfileV2SettingsPage"));
        assert!(!value_looks_secret("e2e-mcp-petstore-2024-11-09"));
    }

    #[test]
    fn shapes_glued_to_identifiers_both_sides() {
        for t in [
            "x_AKIAABCDEFGHIJKLMNOP_suffix",
            "call_ghp_abcdefghijklmnopqrstuvwxyz0123456789_x",
        ] {
            let s = scrub(t);
            assert!(
                !s.contains("AKIAABCD") && !s.contains("ghp_abc"),
                "{t} -> {s}"
            );
        }
    }

    #[test]
    fn scrubbing_json_never_crosses_a_field() {
        let j = r#"{"signature":"-----BEGIN PRIVATE KEY-----","metadata":{"note":"-----END PRIVATE KEY-----"}}"#;
        let s = scrub(j);
        assert_eq!(s, j, "markers in two fields are not one block");
        assert!(serde_json::from_str::<serde_json::Value>(&s).is_ok());
        let j2 = r#"{"k":"-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\nMIIBOg\n-----END RSA PRIVATE KEY-----"}"#;
        let s2 = scrub(j2);
        assert!(
            !s2.contains("MIIBOg") && serde_json::from_str::<serde_json::Value>(&s2).is_ok(),
            "{s2}"
        );
    }

    #[test]
    fn indentation_and_radix_survive_masking() {
        let mut b = b"x: |\n  line one\n  line two\n".to_vec();
        let n = b.len();
        mask_text(&mut b, 5, n - 1);
        assert_eq!(
            String::from_utf8(b).unwrap(),
            "x: |\n  REDACTED\n  REDACTED\n"
        );
        for (src, want) in [
            ("0x1F2e", "0x1000"),
            ("0o7654", "0o1000"),
            ("0b1101", "0b1000"),
            ("-42", "-10"),
        ] {
            let mut n = src.as_bytes().to_vec();
            let len = n.len();
            mask_numeric(&mut n, 0, len);
            assert_eq!(String::from_utf8(n).unwrap(), want);
        }
    }

    #[test]
    fn route_paths_are_not_entropy_hits() {
        assert!(!value_looks_secret("/api/v1/customer/v2/profile"));
        assert!(!value_looks_secret(
            "crates/wicked-estate-core/src/redact.rs"
        ));
    }

    #[test]
    fn deny_key_splits_words() {
        for k in [
            "DB_PASSWORD",
            "apiKey",
            "api-key",
            "client_secret",
            "authToken",
            "TOKEN",
            "x.auth",
        ] {
            assert!(deny_key(k), "{k}");
        }
        for k in [
            "author",
            "passenger",
            "keyboard",
            "API_BASE_URL",
            "tokenizer",
            "bypass",
        ] {
            assert!(!deny_key(k), "{k}");
        }
    }
}
