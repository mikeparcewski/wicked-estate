//! Length-preserving text maskers shared by the heuristic rules extractors (DRL, Blaze, Rego).
//!
//! Regex/brace scanning over raw source is fooled by keywords and braces that live inside comments
//! or string literals (e.g. a `rule "x"` in a block comment, or a `}` inside a quoted value). These
//! helpers blank out those spans **while preserving byte length and newlines**, so match offsets
//! computed on the masked text map 1:1 back onto the original — callers find structural positions on
//! the masked copy and slice human-readable content from the original (or the comment-blanked) copy.

/// Replace the bytes of `//` line comments and `/* … */` block comments with spaces, leaving string
/// literals intact. For C-style rule languages (Drools DRL, FICO Blaze SRL).
pub(crate) fn blank_c_comments(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                out[i] = b' ';
                i += 1;
            }
        } else if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            out[i] = b' ';
            out[i + 1] = b' ';
            i += 2;
            while i < b.len() && !(b[i] == b'*' && i + 1 < b.len() && b[i + 1] == b'/') {
                if b[i] != b'\n' {
                    out[i] = b' ';
                }
                i += 1;
            }
            if i < b.len() {
                out[i] = b' ';
            }
            if i + 1 < b.len() {
                out[i + 1] = b' ';
            }
            i += 2;
        } else if b[i] == b'"' || b[i] == b'\'' {
            i = skip_string(b, i);
        } else {
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

/// Replace the bytes of `#` line comments with spaces, leaving string literals intact. For Rego.
pub(crate) fn blank_hash_comments(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'#' {
            while i < b.len() && b[i] != b'\n' {
                out[i] = b' ';
                i += 1;
            }
        } else if b[i] == b'"' || b[i] == b'`' {
            i = skip_string(b, i);
        } else {
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

/// Replace the *contents* of `"…"`, `'…'`, and `` `…` `` string literals with spaces (keeping the
/// delimiters), so structural scanning (braces, keywords) ignores text inside strings.
pub(crate) fn mask_strings(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'"' || b[i] == b'\'' || b[i] == b'`' {
            let quote = b[i];
            let raw = quote == b'`';
            i += 1;
            while i < b.len() && b[i] != quote {
                if b[i] == b'\\' && !raw {
                    if i + 1 < b.len() && b[i + 1] < 0x80 && b[i + 1] != b'\n' {
                        out[i + 1] = b' ';
                    }
                    out[i] = b' ';
                    i += 2;
                    continue;
                }
                if b[i] != b'\n' {
                    out[i] = b' ';
                }
                i += 1;
            }
            i += 1; // step past the closing quote (or EOF)
        } else {
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

/// Index just past the closing quote of a string literal whose opening quote is at `i` (escapes
/// honored for non-raw quotes). Used by the comment maskers to skip over `//`/`#` inside strings.
fn skip_string(b: &[u8], mut i: usize) -> usize {
    let quote = b[i];
    let raw = quote == b'`';
    i += 1;
    while i < b.len() && b[i] != quote {
        if b[i] == b'\\' && !raw {
            i += 2;
        } else {
            i += 1;
        }
    }
    i + 1
}

/// Index of the `}` matching the `{` at `open` (depth-counted over `scan`, which should already have
/// strings/comments masked). Returns `scan.len()` if unbalanced.
pub(crate) fn match_brace_end(scan: &str, open: usize) -> usize {
    let b = scan.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
        i += 1;
    }
    scan.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_are_preserved() {
        let s = "rule \"a } then\" /* end */ x // y\n# z";
        assert_eq!(blank_c_comments(s).len(), s.len());
        assert_eq!(blank_hash_comments(s).len(), s.len());
        assert_eq!(mask_strings(s).len(), s.len());
    }

    #[test]
    fn c_comments_blanked_strings_kept() {
        let s = "a /* rule \"x\" */ b // rule y\nc";
        let out = blank_c_comments(s);
        assert!(
            !out.contains("rule"),
            "commented `rule` keywords blanked: {out:?}"
        );
        assert!(out.starts_with("a "));
        assert!(out.trim_end().ends_with("c"));
    }

    #[test]
    fn hash_comments_blanked() {
        let s = "allow if {\n  input.x == 1  # deny here\n}";
        let out = blank_hash_comments(s);
        assert!(!out.contains("deny"), "commented text blanked: {out:?}");
        assert!(out.contains("input.x == 1"));
    }

    #[test]
    fn masked_strings_hide_braces_and_keywords() {
        let s = r#"if note = "has } and then here" then act"#;
        let m = mask_strings(s);
        // The `}` and `then` inside the literal are gone; the trailing real `then` survives.
        let first_quote = m.find('"').unwrap();
        let masked_region = &m[first_quote..];
        assert!(!masked_region.contains('}'));
        assert!(
            m.matches("then").count() == 1,
            "only the real `then` remains: {m:?}"
        );
    }

    #[test]
    fn match_brace_end_respects_masking() {
        let s = r#"{ a "}" b }"#;
        let scan = mask_strings(s);
        let end = match_brace_end(&scan, 0);
        assert_eq!(&s[end..end + 1], "}");
        assert_eq!(
            end,
            s.len() - 1,
            "matched the real closing brace, not the one in the string"
        );
    }
}
