//! Embedded code (estate#314): extract code that a host file embeds in another language with that
//! language's built-in grammar, and map the result back into the host file.
//!
//! A host extractor (APEXlang today; Vue/Svelte blocks, Markdown fences and COBOL `EXEC SQL` are
//! the obvious next ones) hands over its embedded regions: the host component that owns each one, a
//! stable key, the language tag as written and the body's span in the host file. For each region:
//! - the tag is mapped to a built-in language through a data table (`embedded.toml`), never
//!   guessed;
//! - a mapped region is extracted by that language's [`TreeSitterExtractor`] on the body alone,
//!   and every resulting location is shifted into host coordinates (file = the host file);
//! - every region, parsed or not, becomes one `embedded_region` node, held by its owner through a
//!   `contains` edge, so no block is ever dropped silently. A tag with no built-in grammar
//!   (`plsql`), a missing tag, or a failed extraction is an unparsed region marker
//!   (`embedded_parsed = false`, with the reason);
//! - the embedded extractor's edges keep their own tier and confidence; their metadata names the
//!   host file and the region.
//!
//! Identity: a region is `embedded synthetic <host path>/<encoded key>:`, and the code inside it is
//! minted by its own extractor under that same virtual module, so nothing depends on a byte offset
//! and two regions never share an id (a repeated key gets `~2`). Refs of top-level embedded code
//! are owned by the region marker, whose language is the embedded one. Not handled: relative
//! imports from embedded code (the importer is the host file, which has no import conventions).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use serde::Deserialize;
use wicked_estate_core::{
    Edge, EdgeKind, Error, Extraction, Extractor, Language, Location, Node, NodeKind,
    ResolutionTier, Result, SourceFile, Span, Symbol, SymbolId,
};

use crate::treesitter::TreeSitterExtractor;

const TABLE_TOML: &str = include_str!("../embedded.toml");

/// Metadata keys of an `embedded_region` node.
pub const REGION_KIND: &str = "embedded_region";
pub const TAG_KEY: &str = "embedded_tag";
pub const LANGUAGE_KEY: &str = "embedded_language";
pub const PARSED_KEY: &str = "embedded_parsed";
pub const REASON_KEY: &str = "embedded_reason";

/// Tag -> built-in language, and the node kinds that are strings or comments per language.
#[derive(Debug, Clone, Deserialize)]
pub struct EmbedTable {
    #[serde(default)]
    tags: HashMap<String, String>,
    /// Node kinds whose text is a string literal or a comment, per language: a scan for names
    /// inside a parsed region skips them.
    #[serde(default)]
    literal_kinds: HashMap<String, Vec<String>>,
}

impl EmbedTable {
    /// Parse a table (the built-in one is `embedded.toml`).
    pub fn parse(toml_src: &str) -> Result<Self> {
        toml::from_str(toml_src).map_err(|e| Error::Extraction(format!("embedded.toml: {e}")))
    }

    /// The table compiled into the binary.
    pub fn builtin() -> &'static EmbedTable {
        static TABLE: LazyLock<EmbedTable> =
            LazyLock::new(|| EmbedTable::parse(TABLE_TOML).expect("built-in embedded.toml parses"));
        &TABLE
    }

    /// The built-in language a tag names; `None` for an unknown or missing tag.
    pub fn language_for(&self, tag: Option<&str>) -> Option<&str> {
        tag.and_then(|t| self.tags.get(&t.to_ascii_lowercase()))
            .map(String::as_str)
    }
}

/// One embedded region of a host file.
#[derive(Debug, Clone)]
pub struct EmbeddedRegion {
    /// The host component that owns the region.
    pub owner: SymbolId,
    /// Stable key, unique within the host file (APEXlang: the owner's path plus the property that
    /// holds the block). A repeated key is disambiguated with `~2`, `~3` in input order.
    pub key: String,
    /// The language tag as written (`None` when untagged).
    pub tag: Option<String>,
    /// The body's span in the host file (may be empty).
    pub span: Span,
    /// Collect the region's string-literal and comment ranges ([`RegionOutcome::literals`]); only
    /// a host that scans the region afterwards (bind variables) pays the extra parse.
    pub want_literals: bool,
}

/// What happened to one region.
#[derive(Debug, Clone)]
pub struct RegionOutcome {
    /// The region marker node's id.
    pub symbol: SymbolId,
    /// The built-in language the region was extracted with, when it was.
    pub language: Option<String>,
    /// Host byte ranges of string literals and comments inside a parsed region.
    pub literals: Vec<(usize, usize)>,
}

/// The extraction contributed by a host's embedded regions, plus one outcome per region (in input
/// order).
#[derive(Debug, Default)]
pub struct Embedded {
    pub extraction: Extraction,
    pub regions: Vec<RegionOutcome>,
}

/// Extract every region of `host`. Never fails: a region that cannot be extracted is a marker.
pub fn extract_regions(
    host: &SourceFile,
    regions: &[EmbeddedRegion],
    table: &EmbedTable,
) -> Embedded {
    let mut out = Embedded::default();
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for region in regions {
        // The region's identity is its virtual module, `<host path>/<encoded key>`: the host path
        // is unique per file and the encoded key holds no `/`, so ids are injective.
        let n = seen.entry(region.key.as_str()).or_insert(0);
        *n += 1;
        let key = if *n == 1 {
            encode(&region.key)
        } else {
            format!("{}~{}", encode(&region.key), n)
        };
        let module = format!("{}/{key}", host.path);
        let symbol = Symbol::synthetic("embedded", module.clone()).id();
        let tag = region.tag.as_deref().filter(|t| !t.is_empty());
        let mapped = table.language_for(tag);
        let body = host
            .text
            .get(region.span.start_byte as usize..region.span.end_byte as usize)
            .unwrap_or("");
        let mut parsed = None;
        let mut reason = match (tag, mapped) {
            (None, _) => Some("untagged block: no language to parse it with".to_string()),
            (Some(t), None) => Some(format!("no built-in grammar for tag `{t}`")),
            _ => None,
        };
        let mut literals = Vec::new();
        let mut region_language = host.language.clone();
        if let Some(lang) = mapped {
            match extract_body(host, region, &symbol, &module, lang, body) {
                Ok(ex) => {
                    if region.want_literals {
                        let base = region.span.start_byte as usize;
                        literals = literal_ranges(lang, body, table)
                            .into_iter()
                            .map(|(s, e)| (base + s, base + e))
                            .collect();
                    }
                    // A parsed region speaks its embedded language, so code at its top level
                    // (owned by the marker) passes the resolvers' family guard.
                    region_language = Language::new(lang);
                    out.extraction.nodes.extend(ex.nodes);
                    out.extraction.local_edges.extend(ex.local_edges);
                    out.extraction.refs.extend(ex.refs);
                    parsed = Some(lang.to_string());
                }
                Err(e) => reason = Some(format!("{lang} extraction failed: {e}")),
            }
        }

        let mut marker = Node::new(
            symbol.clone(),
            NodeKind::Other(REGION_KIND.to_string()),
            tag.unwrap_or("untagged"),
            region_language,
            Location::new(&host.path, region.span),
        );
        marker
            .metadata
            .insert(TAG_KEY.into(), tag.unwrap_or("").into());
        marker.metadata.insert(
            LANGUAGE_KEY.into(),
            mapped.map_or(serde_json::Value::Null, Into::into),
        );
        marker
            .metadata
            .insert(PARSED_KEY.into(), parsed.is_some().into());
        if let Some(r) = &reason {
            marker.metadata.insert(REASON_KEY.into(), r.clone().into());
        }
        out.extraction.nodes.push(marker);
        out.extraction.local_edges.push(
            Edge::new(
                region.owner.clone(),
                symbol.clone(),
                EdgeKind::Contains,
                ResolutionTier::Parsed,
                "embedded",
            )
            .with_location(Location::new(&host.path, region.span)),
        );
        out.regions.push(RegionOutcome {
            symbol,
            language: parsed,
            literals,
        });
    }
    out
}

/// Run `lang`'s extractor on one body and move the result into the host file.
fn extract_body(
    host: &SourceFile,
    region: &EmbeddedRegion,
    marker: &SymbolId,
    module: &str,
    lang: &str,
    body: &str,
) -> Result<Extraction> {
    let (extractor, ext) = embedded_extractor(lang)
        .ok_or_else(|| Error::Extraction(format!("language `{lang}` is not wired")))?;
    // The generic extractor returns nothing for a minified or huge text; say so, never claim the
    // block was parsed.
    if crate::treesitter::is_minified_or_huge(body) {
        return Err(Error::Extraction(
            "skipped as minified or huge (the same guard as a source file)".into(),
        ));
    }
    // A virtual module per region: ids stay stable (no offsets) and two regions never collide.
    let sub = extractor.extract(&SourceFile {
        path: format!("{module}.{ext}"),
        language: Language::new(lang),
        text: body.to_string(),
    })?;
    let sub_file = Symbol::file(format!("{module}.{ext}")).id();
    let base = region.span;
    let relocate = |loc: &mut Location| {
        loc.file = host.path.clone();
        loc.span = shift(loc.span, base);
    };
    let mut ex = Extraction::default();
    for mut n in sub.nodes {
        if n.kind == NodeKind::File {
            continue;
        }
        relocate(&mut n.location);
        ex.nodes.push(n);
    }
    for mut e in sub.local_edges {
        if e.source == sub_file {
            e.source = marker.clone();
        }
        if e.target == sub_file {
            e.target = marker.clone();
        }
        if let Some(loc) = e.location.as_mut() {
            relocate(loc);
        }
        e.metadata
            .insert("embedded_host".into(), host.path.clone().into());
        e.metadata
            .insert("embedded_region".into(), marker.0.clone().into());
        ex.local_edges.push(e);
    }
    for mut r in sub.refs {
        if r.from == sub_file {
            r.from = marker.clone();
        }
        relocate(&mut r.location);
        r.hints
            .insert("embedded_region".into(), marker.0.clone().into());
        ex.refs.push(r);
    }
    Ok(ex)
}

/// One extractor (and its file extension) per embedded language per process: compiling a
/// language's query is the expensive part (ADR-014), and a host can hold thousands of blocks.
fn embedded_extractor(lang: &str) -> Option<(Arc<TreeSitterExtractor>, String)> {
    type Cache = Mutex<HashMap<String, Option<(Arc<TreeSitterExtractor>, String)>>>;
    static CACHE: LazyLock<Cache> = LazyLock::new(Default::default);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .entry(lang.to_string())
        .or_insert_with(|| {
            let extractor = TreeSitterExtractor::for_language(lang)?;
            let ext = crate::registry()
                .into_iter()
                .find(|l| l.name == lang)
                .and_then(|l| l.ext.first().cloned())
                .unwrap_or_else(|| "txt".to_string());
            Some((Arc::new(extractor), ext))
        })
        .clone()
}

/// Percent-encode a region key (injective; the result holds no `/`).
pub fn encode(key: &str) -> String {
    wicked_estate_core::refs::apexlang_ref::encode(key)
}

/// A span measured inside a region body, moved into host coordinates. Columns shift only on the
/// body's first line.
fn shift(s: Span, base: Span) -> Span {
    let col = |line: u32, c: u32| if line == 0 { c + base.start_col } else { c };
    Span {
        start_byte: s.start_byte + base.start_byte,
        end_byte: s.end_byte + base.start_byte,
        start_line: s.start_line + base.start_line,
        start_col: col(s.start_line, s.start_col),
        end_line: s.end_line + base.start_line,
        end_col: col(s.end_line, s.end_col),
    }
}

/// Byte ranges (body coordinates) of the nodes the table lists as literals or comments for `lang`.
fn literal_ranges(lang: &str, body: &str, table: &EmbedTable) -> Vec<(usize, usize)> {
    let Some(kinds) = table.literal_kinds.get(lang) else {
        return Vec::new();
    };
    let Some(language) = crate::treesitter::builtin_language(lang) else {
        return Vec::new();
    };
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(body, None) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut cursor = tree.walk();
    let mut stack = vec![tree.root_node()];
    while let Some(n) = stack.pop() {
        if kinds.iter().any(|k| k == n.kind()) {
            out.push((n.start_byte(), n.end_byte()));
            continue;
        }
        stack.extend(n.children(&mut cursor));
    }
    out
}
