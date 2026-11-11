//! Oracle APEXlang (`.apx`) extractor (estate#312, #313).
//!
//! An APEX application is a directory of `.apx` files (`application.apx`, `pages/p00004-x.apx`,
//! `shared-components/...`). The vendored tree-sitter grammar (`vendor/tree-sitter-apexlang`)
//! parses the core syntax; this module turns a parse into nodes, `contains` edges and references,
//! driven by the data in `apexlang.toml` (layout, identity, the property -> target table).
//!
//! **Identity (ADR-002).** A component's `SymbolId` is the path of `(kind, key)` pairs from its
//! application root: `apexlang . . . <root>/app#page:4#region:customer#`. The root is derived from
//! the file's path, so extraction stays a pure function of (path, text) and a page's ids never
//! depend on another file. Never a line, a generated number or a content hash.
//!
//! **References.** Each `@slug` property value becomes one [`UnresolvedRef`] from the owning
//! component, at the token's exact site, whose `raw_name` carries the target kind and scope (see
//! [`wicked_estate_core::refs::apexlang_ref`]). The APEXlang resolver in `wicked-estate-resolve`
//! binds them; no name-based tier ever sees them.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};
use wicked_estate_core::refs::apexlang_ref;
use wicked_estate_core::{
    Descriptor, Edge, EdgeKind, Error, Extraction, Extractor, Language, Location, Node, NodeKind,
    ResolutionTier, Result, SourceFile, Span, Suffix, Symbol, SymbolId, UnresolvedRef,
};

/// Language name (the `languages.toml` row and `LANG_TABLE` entry).
pub const LANGUAGE: &str = "apexlang";
/// SymbolId scheme of every APEXlang node.
pub const SCHEME: &str = "apexlang";
/// Metadata key holding the APEXlang component kind (`region`, `pageItem`, ...).
pub const KIND_METADATA_KEY: &str = "apexlang_kind";
/// Metadata key saying how a node's key was formed: `declared`, `properties` or `positional`.
pub const IDENTITY_METADATA_KEY: &str = "apexlang_identity";
/// Application-node metadata: the export's meta-metadata version (`26.2.0+3479`, or `unknown`).
pub const MMD_VERSION_KEY: &str = "apexlang_mmd_version";
/// Application-node metadata: `false` when the version is unknown or its major is not supported.
pub const MMD_SUPPORTED_KEY: &str = "apexlang_mmd_supported";

const DATA_TOML: &str = include_str!("../apexlang.toml");

unsafe extern "C" {
    fn tree_sitter_apexlang() -> *const ();
}

/// The vendored APEXlang grammar (`vendor/tree-sitter-apexlang`, compiled by `build.rs`).
pub const GRAMMAR: tree_sitter_language::LanguageFn =
    unsafe { tree_sitter_language::LanguageFn::from_raw(tree_sitter_apexlang) };

/// The extractor's data (`apexlang.toml`).
#[derive(Debug, Clone, Deserialize)]
pub struct ApexlangData {
    layout: Layout,
    #[serde(default)]
    node_kind: HashMap<String, String>,
    #[serde(default)]
    identity: Identity,
    #[serde(rename = "ref", default)]
    refs: Vec<RefRule>,
    bind_variable: BindVariable,
    export_metadata: ExportMetadata,
}

/// Page-item bind variables inside embedded regions (#315).
#[derive(Debug, Clone, Deserialize)]
struct BindVariable {
    pattern: String,
    /// Component kinds a bind variable can name (marked `apexlang_bindable`).
    targets: Vec<String>,
    tags: Vec<String>,
    text_scan: TextScan,
    #[serde(skip)]
    regex: Option<regex::Regex>,
}

/// Delimiters a text scan of an unparsed region skips.
#[derive(Debug, Clone, Deserialize)]
struct TextScan {
    quote: String,
    line_comment: String,
    block_comment: [String; 2],
    /// Quoted identifiers (`"..."`), never bind variables.
    identifier_quote: String,
    /// Alternative quoting `q'[...]'` / `nq'[...]'`: the prefixes (case-insensitive, longest
    /// first) and the bracket pairs; any other delimiter closes with itself.
    alternative_quote_prefixes: Vec<String>,
    alternative_quote_pairs: Vec<[String; 2]>,
}

/// Where the export metadata version lives (#316).
#[derive(Debug, Clone, Deserialize)]
struct ExportMetadata {
    host_file: String,
    file: String,
    version_key: String,
    supported_majors: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Layout {
    boundary: Vec<Vec<String>>,
    #[serde(default)]
    container: Vec<Container>,
}

#[derive(Debug, Clone, Deserialize)]
struct Container {
    path: Vec<String>,
    kind: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Identity {
    #[serde(default)]
    qualifier: HashMap<String, String>,
    #[serde(default)]
    keyless: HashMap<String, Vec<String>>,
}

/// One row of the property -> target table.
#[derive(Debug, Clone, Deserialize)]
struct RefRule {
    owner: String,
    property: String,
    target: String,
    scope: String,
}

impl ApexlangData {
    /// Parse a data table (the built-in one is `apexlang.toml`).
    pub fn parse(toml_src: &str) -> Result<Self> {
        let mut data: Self = toml::from_str(toml_src)
            .map_err(|e| Error::Extraction(format!("apexlang.toml: {e}")))?;
        let re = regex::Regex::new(&data.bind_variable.pattern)
            .map_err(|e| Error::Extraction(format!("apexlang.toml bind_variable.pattern: {e}")))?;
        if re.capture_names().flatten().all(|n| n != "item") {
            return Err(Error::Extraction(
                "apexlang.toml bind_variable.pattern needs the named group `item`".into(),
            ));
        }
        data.bind_variable.regex = Some(re);
        Ok(data)
    }

    /// The table compiled into the binary.
    pub fn builtin() -> &'static ApexlangData {
        static DATA: LazyLock<ApexlangData> = LazyLock::new(|| {
            ApexlangData::parse(DATA_TOML).expect("built-in apexlang.toml must parse")
        });
        &DATA
    }

    /// The row for `(owner, property)`: an exact owner wins over `*`.
    fn rule(&self, owner: &str, property: &str) -> Option<&RefRule> {
        self.refs
            .iter()
            .find(|r| r.owner == owner && r.property == property)
            .or_else(|| {
                self.refs
                    .iter()
                    .find(|r| r.owner == "*" && r.property == property)
            })
    }

    fn node_kind(&self, kind: &str) -> NodeKind {
        self.node_kind
            .get(kind)
            .and_then(|k| serde_json::from_value(serde_json::Value::String(k.clone())).ok())
            .unwrap_or_else(|| NodeKind::Other(kind.to_string()))
    }
}

/// Where a file sits in its application: the root directory and the container directories
/// (theme, plugin) between the root and the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppLayout {
    /// Repo-relative application root (`"."` for the repo root).
    pub root: String,
    /// `(kind, id)` of each container directory, outermost first.
    pub containers: Vec<(String, String)>,
}

/// The application layout of a repo-relative `.apx` path.
pub fn app_layout(path: &str, data: &ApexlangData) -> AppLayout {
    let norm = path.replace('\\', "/");
    let segs: Vec<&str> = norm.split('/').filter(|s| !s.is_empty()).collect();
    let dirs = &segs[..segs.len().saturating_sub(1)];
    // The left-most directory whose remaining path has one of the application shapes.
    let shape_fits = |rest: &[&str], shape: &[String]| -> bool {
        match shape.split_last() {
            Some((last, head)) if last == "**" => {
                rest.len() >= head.len() && head.iter().zip(rest).all(|(p, s)| fits(p, s))
            }
            _ => rest.len() == shape.len() && shape.iter().zip(rest).all(|(p, s)| fits(p, s)),
        }
    };
    let root_len = (0..dirs.len())
        .find(|&i| {
            data.layout
                .boundary
                .iter()
                .any(|b| shape_fits(&dirs[i..], b))
        })
        .unwrap_or(dirs.len());
    let root = if root_len == 0 {
        ".".to_string()
    } else {
        dirs[..root_len].join("/")
    };
    let rel = &dirs[root_len..];
    let mut containers = Vec::new();
    for c in &data.layout.container {
        if rel.len() < c.path.len() {
            continue;
        }
        let mut id = None;
        let matched = c
            .path
            .iter()
            .zip(rel.iter())
            .all(|(pat, seg)| match pat.as_str() {
                "*" => true,
                "{id}" => {
                    id = Some((*seg).to_string());
                    true
                }
                lit => lit == *seg,
            });
        // A pattern without `{id}` names a singleton container (id "").
        if matched {
            containers.push((c.kind.clone(), id.unwrap_or_default()));
        }
    }
    AppLayout { root, containers }
}

fn fits(pattern: &str, seg: &str) -> bool {
    pattern == "*" || pattern == seg
}

/// Extracts APEXlang components, `contains` edges and references.
pub struct ApexlangExtractor {
    language: tree_sitter::Language,
    query: Query,
    data: ApexlangData,
}

impl ApexlangExtractor {
    /// The extractor with the built-in grammar, query and data.
    pub fn new() -> Self {
        Self::with_data(ApexlangData::builtin().clone())
    }

    /// One process-wide extractor with the built-in data: the query is compiled once, not per file.
    pub fn shared() -> &'static ApexlangExtractor {
        static SHARED: LazyLock<ApexlangExtractor> = LazyLock::new(ApexlangExtractor::new);
        &SHARED
    }

    /// The extractor with a caller-supplied data table (same grammar and query).
    pub fn with_data(data: ApexlangData) -> Self {
        let language =
            crate::treesitter::builtin_language(LANGUAGE).expect("apexlang is wired in LANG_TABLE");
        let query = Query::new(&language, crate::treesitter::builtin_query(LANGUAGE))
            .expect("queries/apexlang.scm must compile");
        Self {
            language,
            query,
            data,
        }
    }
}

impl Default for ApexlangExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// One component during extraction.
struct Comp {
    kind: String,
    /// Index of the enclosing component, `None` at top level.
    parent: Option<usize>,
    symbol: SymbolId,
}

fn text<'a>(n: tree_sitter::Node, src: &'a [u8]) -> &'a str {
    n.utf8_text(src).unwrap_or("")
}

/// The content of a `"quoted"` token, with `\"` and `\\` unescaped.
fn unquote(raw: &str) -> String {
    let inner = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(raw);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                out.push(n);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A property key or a component id as written: strings unquoted.
fn name_of(n: tree_sitter::Node, src: &[u8]) -> String {
    if n.kind() == "string" {
        unquote(text(n, src))
    } else {
        text(n, src).to_string()
    }
}

/// A scalar property value as a key part: references lose `@`, strings their quotes.
fn scalar_value(v: tree_sitter::Node, src: &[u8]) -> Option<String> {
    match v.kind() {
        "reference" => Some(text(v, src).trim_start_matches('@').to_string()),
        "string" => Some(unquote(text(v, src))),
        "text" => Some(text(v, src).to_string()),
        _ => None,
    }
}

/// The component's own scalar properties by dotted path (groups and object values included,
/// nested components excluded).
fn own_properties(comp: tree_sitter::Node, src: &[u8]) -> HashMap<String, String> {
    fn walk(n: tree_sitter::Node, prefix: &str, src: &[u8], out: &mut HashMap<String, String>) {
        let mut cur = n.walk();
        for child in n.named_children(&mut cur) {
            match child.kind() {
                "property" => {
                    let Some(key) = child.child_by_field_name("key") else {
                        continue;
                    };
                    let path = join(prefix, &name_of(key, src));
                    if let Some(v) = child.child_by_field_name("value") {
                        if v.kind() == "object" {
                            walk(v, &path, src, out);
                        } else if let Some(s) = scalar_value(v, src) {
                            out.entry(path).or_insert(s);
                        }
                    }
                }
                "group" => {
                    if let Some(name) = child.child_by_field_name("name") {
                        walk(child, &join(prefix, text(name, src)), src, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = HashMap::new();
    walk(comp, "", src, &mut out);
    out
}

fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_string()
    } else {
        format!("{prefix}.{key}")
    }
}

/// The property path of a reference inside its owning component, and the owner's tree node.
fn reference_site<'t>(
    r: tree_sitter::Node<'t>,
    src: &[u8],
) -> Option<(String, tree_sitter::Node<'t>)> {
    let mut parts = Vec::new();
    let mut cur = r.parent();
    while let Some(n) = cur {
        match n.kind() {
            "component" => {
                parts.reverse();
                return Some((parts.join("."), n));
            }
            "property" => {
                if let Some(k) = n.child_by_field_name("key") {
                    parts.push(name_of(k, src));
                }
            }
            "group" => {
                if let Some(k) = n.child_by_field_name("name") {
                    parts.push(text(k, src).to_string());
                }
            }
            _ => {}
        }
        cur = n.parent();
    }
    None
}

fn span_of(n: tree_sitter::Node) -> Span {
    let s = n.start_position();
    let e = n.end_position();
    Span {
        start_byte: n.start_byte() as u32,
        end_byte: n.end_byte() as u32,
        start_line: s.row as u32,
        start_col: s.column as u32,
        end_line: e.row as u32,
        end_col: e.column as u32,
    }
}

/// The application root as a namespace descriptor: `#` and `%` are escaped, so no directory name
/// can fake a descriptor boundary (`a/app#b/...` would otherwise sit "under" application `a`).
fn escape_root(root: &str) -> String {
    root.replace('%', "%25").replace('#', "%23")
}

fn global(descriptors: Vec<Descriptor>) -> SymbolId {
    Symbol::global(SCHEME, None, descriptors).id()
}

fn ty(name: String) -> Descriptor {
    Descriptor::new(name, Suffix::Type)
}

/// The descriptor of one `(kind, key)` pair: `kind:part/part`, each part percent-encoded (so it
/// holds no `/`, `#` or `~`); `ordinal` >= 2 marks a positional duplicate.
fn element(kind: &str, parts: &[String], ordinal: usize) -> String {
    let mut e = apexlang_ref::encode(kind);
    if !parts.is_empty() {
        e.push(':');
        let enc: Vec<String> = parts.iter().map(|p| apexlang_ref::encode(p)).collect();
        e.push_str(&enc.join("/"));
    }
    if ordinal >= 2 {
        e.push('~');
        e.push_str(&ordinal.to_string());
    }
    e
}

impl Extractor for ApexlangExtractor {
    fn languages(&self) -> Vec<Language> {
        vec![Language::new(LANGUAGE)]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        extract_with(&self.language, &self.query, &self.data, file)
    }
}

/// The `LANG_TABLE` structured routine for `apexlang`: the built-in data over the effective
/// grammar and query (a grammar or query override reaches extraction through here).
pub(crate) fn extract_structured(
    language: &tree_sitter::Language,
    query: &Query,
    file: &SourceFile,
) -> Result<Extraction> {
    extract_with(language, query, ApexlangData::builtin(), file)
}

fn extract_with(
    ts_language: &tree_sitter::Language,
    query: &Query,
    data: &ApexlangData,
    file: &SourceFile,
) -> Result<Extraction> {
    let mut parser = Parser::new();
    parser
        .set_language(ts_language)
        .map_err(|e| Error::Extraction(e.to_string()))?;
    let tree = parser
        .parse(file.text.as_bytes(), None)
        .ok_or_else(|| Error::Extraction(format!("parse failed for {}", file.path)))?;
    let src = file.text.as_bytes();
    let language = Language::new(LANGUAGE);

    // The application this file belongs to: `<root>/app#` plus any container directories.
    let layout = app_layout(&file.path, data);
    let mut base = vec![
        Descriptor::new(escape_root(&layout.root), Suffix::Namespace),
        ty("app".to_string()),
    ];
    let app_symbol = global(base.clone());
    // Virtual ancestors (kind, symbol) of every top-level component, outermost first.
    let mut virtual_ancestors = vec![("app".to_string(), app_symbol.clone())];
    for (kind, id) in &layout.containers {
        let parts: Vec<String> = if id.is_empty() {
            Vec::new()
        } else {
            vec![id.clone()]
        };
        base.push(ty(element(kind, &parts, 1)));
        virtual_ancestors.push((kind.clone(), global(base.clone())));
    }

    // ── Collect components, references and fences in document order ──────────────────────
    let names = query.capture_names();
    let mut comp_nodes: Vec<tree_sitter::Node> = Vec::new();
    let mut ref_nodes: Vec<tree_sitter::Node> = Vec::new();
    let mut fence_nodes: Vec<tree_sitter::Node> = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), src);
    while let Some(m) = matches.next() {
        for c in m.captures {
            match names[c.index as usize] {
                "apexlang.component" => comp_nodes.push(c.node),
                "apexlang.reference" => ref_nodes.push(c.node),
                "apexlang.fence" => fence_nodes.push(c.node),
                _ => {}
            }
        }
    }
    comp_nodes.sort_by_key(|n| n.start_byte());
    comp_nodes.dedup_by_key(|n| n.id());
    ref_nodes.sort_by_key(|n| n.start_byte());
    ref_nodes.dedup_by_key(|n| n.id());
    fence_nodes.sort_by_key(|n| n.start_byte());
    fence_nodes.dedup_by_key(|n| n.id());

    // ── Mint identities top-down ─────────────────────────────────────────────────────────
    let index_of: HashMap<usize, usize> = comp_nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id(), i))
        .collect();
    let mut comps: Vec<Comp> = Vec::with_capacity(comp_nodes.len());
    let mut descriptors_of: Vec<Vec<Descriptor>> = Vec::with_capacity(comp_nodes.len());
    let mut seen: HashMap<(String, String), usize> = HashMap::new();
    let mut holder_refs: Vec<UnresolvedRef> = Vec::new();
    let mut container_claimed = false;
    let mut nodes = Vec::with_capacity(comp_nodes.len() + 1);
    let mut local_edges = Vec::new();
    let file_symbol = Symbol::file(&file.path).id();
    let floc = Location::new(&file.path, Span::ZERO);

    for &cn in &comp_nodes {
        let kind = cn
            .child_by_field_name("kind")
            .map(|k| text(k, src).to_string())
            .unwrap_or_default();
        let parent = {
            let mut cur = cn.parent();
            let mut found = None;
            while let Some(p) = cur {
                if p.kind() == "component" {
                    found = index_of.get(&p.id()).copied();
                    break;
                }
                cur = p.parent();
            }
            found
        };
        let props = own_properties(cn, src);
        let declared = cn.child_by_field_name("id").map(|n| name_of(n, src));
        // The key's parts are encoded one by one into the descriptor, so a `/` inside a part
        // can never fake a part boundary; the node name joins them as references spell them.
        let (parts, identity): (Vec<String>, &str) = match &declared {
            Some(id) => {
                let parts = match data
                    .identity
                    .qualifier
                    .get(&kind)
                    .and_then(|q| props.get(q))
                {
                    Some(q) => vec![q.clone(), id.clone()],
                    None => vec![id.clone()],
                };
                (parts, "declared")
            }
            None => {
                // Missing properties keep their position (as ""), so (A, -, B) != (A, B, -).
                let parts: Vec<String> = data
                    .identity
                    .keyless
                    .get(&kind)
                    .map(|ps| {
                        ps.iter()
                            .map(|p| props.get(p).cloned().unwrap_or_default())
                            .collect()
                    })
                    .unwrap_or_default();
                let parts = if parts.iter().all(String::is_empty) {
                    Vec::new()
                } else {
                    parts
                };
                (parts, "properties")
            }
        };
        let key = parts.join("/");

        let parent_desc: Vec<Descriptor> = match parent {
            Some(p) => descriptors_of[p].clone(),
            None => base.clone(),
        };
        let is_app = parent.is_none() && kind == "app";
        // The container directory's own declaration (`plugin x (` in its plugin.apx, `theme ut (`
        // in theme.apx) IS the container node: the first top-level component of the container's
        // kind. Its id need not match the directory (`plugins/template-component/
        // contentRowMinimum-theme-universal-theme/` declares `plugin contentRowMinimum`).
        let is_container = parent.is_none()
            && !container_claimed
            && layout.containers.last().is_some_and(|(ck, _)| *ck == kind);
        container_claimed |= is_container;
        let (descs, identity) = if is_app {
            (base[..2].to_vec(), identity)
        } else if is_container {
            (base.clone(), identity)
        } else {
            let parent_id = global(parent_desc.clone()).0;
            let count = seen
                .entry((parent_id, element(&kind, &parts, 1)))
                .and_modify(|c| *c += 1)
                .or_insert(1);
            let ordinal = *count;
            let mut d = parent_desc.clone();
            d.push(ty(element(&kind, &parts, ordinal)));
            let identity = if ordinal >= 2 || (key.is_empty() && identity == "properties") {
                "positional"
            } else {
                identity
            };
            (d, identity)
        };
        let symbol = global(descs.clone());
        // A top-level component sits in its application (or container directory) even though
        // that declaration is another file: a `contains` ref the resolver binds once the
        // holder exists, so it never dangles and a holder indexed later still links.
        if parent.is_none() && !is_app {
            let holder = if is_container {
                &virtual_ancestors[virtual_ancestors.len() - 2].1
            } else {
                &virtual_ancestors[virtual_ancestors.len() - 1].1
            };
            holder_refs.push(UnresolvedRef::new(
                symbol.clone(),
                apexlang_ref::container(holder.as_str()),
                EdgeKind::Contains,
                Location::new(&file.path, span_of(cn)),
            ));
        }

        let name = if key.is_empty() { kind.clone() } else { key };
        let mut node = Node::new(
            symbol.clone(),
            data.node_kind(&kind),
            name,
            language.clone(),
            Location::new(&file.path, span_of(cn)),
        );
        let decl_line = text(cn, src).lines().next().unwrap_or("");
        node.signature = Some(decl_line.trim_end_matches('(').trim().to_string());
        node.metadata
            .insert(KIND_METADATA_KEY.into(), kind.clone().into());
        node.metadata
            .insert(IDENTITY_METADATA_KEY.into(), identity.into());
        if data.bind_variable.targets.contains(&kind) {
            node.metadata
                .insert(apexlang_ref::BINDABLE_KEY.into(), true.into());
        }
        if let Some(label) = props.get("name") {
            node.metadata
                .insert("apexlang_label".into(), label.clone().into());
        }
        nodes.push(node);

        // Contains: the file holds every component; a parent component holds its children.
        local_edges.push(
            Edge::new(
                file_symbol.clone(),
                symbol.clone(),
                EdgeKind::Contains,
                ResolutionTier::Parsed,
                "apexlang",
            )
            .with_location(floc.clone()),
        );
        if let Some(p) = parent {
            local_edges.push(
                Edge::new(
                    comps[p].symbol.clone(),
                    symbol.clone(),
                    EdgeKind::Contains,
                    ResolutionTier::Parsed,
                    "apexlang",
                )
                .with_location(Location::new(&file.path, span_of(cn))),
            );
        }
        comps.push(Comp {
            kind,
            parent,
            symbol,
        });
        descriptors_of.push(descs);
    }

    // ── References ───────────────────────────────────────────────────────────────────────
    let mut refs = holder_refs;
    refs.reserve(ref_nodes.len());
    for rn in ref_nodes {
        let Some((property, owner_node)) = reference_site(rn, src) else {
            continue;
        };
        let Some(&owner) = index_of.get(&owner_node.id()) else {
            continue;
        };
        let raw = text(rn, src).trim_start_matches('@');
        let theme_relative = raw.starts_with('/');
        let slug = raw.trim_start_matches('/');
        let owner_kind = comps[owner].kind.as_str();
        let (target, scope) = match data.rule(owner_kind, &property) {
            Some(rule) if rule.target == apexlang_ref::NO_BIND => {
                (apexlang_ref::NO_BIND.to_string(), Some(app_symbol.clone()))
            }
            // `@/x` names a template of the theme in use. Only a reference made inside a
            // theme knows which theme that is; elsewhere (the application's current theme is
            // declared in another file) it never binds, rather than taking another theme's
            // homonym.
            Some(rule) if theme_relative => (
                rule.target.clone(),
                scope_symbol("theme", owner, &comps, &virtual_ancestors),
            ),
            Some(rule) => (
                rule.target.clone(),
                scope_symbol(&rule.scope, owner, &comps, &virtual_ancestors),
            ),
            None if theme_relative => (apexlang_ref::NO_BIND.to_string(), Some(app_symbol.clone())),
            None => (apexlang_ref::ANY_KIND.to_string(), Some(app_symbol.clone())),
        };
        // A scope that does not exist here (a `page`-scoped property outside any page) must
        // never widen to the application: the ref stays unresolved.
        let (target, scope) = match scope {
            Some(s) => (target, s),
            None => (apexlang_ref::NO_BIND.to_string(), app_symbol.clone()),
        };
        let mut r = UnresolvedRef::new(
            comps[owner].symbol.clone(),
            apexlang_ref::format(&target, slug, scope.as_str()),
            EdgeKind::References,
            Location::new(&file.path, span_of(rn)),
        );
        r.hints
            .insert("apexlang_property".into(), property.clone().into());
        refs.push(r);
    }

    // ── Embedded code (#314) and page-item bind variables (#315) ───────────────────────────
    // Every fenced block is a region of its owning component, keyed by the owner's path inside
    // the application plus the property that holds it (stable, unique, no offsets).
    let mut regions = Vec::with_capacity(fence_nodes.len());
    let mut region_owner = Vec::with_capacity(fence_nodes.len());
    for fence in fence_nodes {
        let Some((property, owner_node)) = reference_site(fence, src) else {
            continue;
        };
        let Some(&owner) = index_of.get(&owner_node.id()) else {
            continue;
        };
        let owner_sym = comps[owner].symbol.as_str();
        let owner_path = owner_sym
            .strip_prefix(app_symbol.as_str())
            .unwrap_or(owner_sym)
            .replace('#', "/");
        let tag = fence
            .child_by_field_name("language")
            .map(|t| text(t, src).to_string());
        let span = match fence.child_by_field_name("body") {
            Some(body) => span_of(body),
            // An empty block: a zero-width region at the closing fence.
            None => {
                let mut s = span_of(fence);
                s.start_byte = s.end_byte.saturating_sub(3);
                s.end_byte = s.start_byte;
                s.start_line = s.end_line;
                s.start_col = s.end_col.saturating_sub(3);
                s.end_col = s.start_col;
                s
            }
        };
        regions.push(crate::embedded::EmbeddedRegion {
            owner: comps[owner].symbol.clone(),
            key: format!("{owner_path}{property}"),
            tag,
            span,
        });
        region_owner.push(owner);
    }
    let embedded =
        crate::embedded::extract_regions(file, &regions, crate::embedded::EmbedTable::builtin());
    for ((region, outcome), &owner) in regions.iter().zip(&embedded.regions).zip(&region_owner) {
        bind_variable_refs(
            file,
            region,
            outcome,
            &comps[owner].symbol,
            &app_symbol,
            &data.bind_variable,
            &mut refs,
        );
    }
    nodes.extend(embedded.extraction.nodes);
    local_edges.extend(embedded.extraction.local_edges);
    refs.extend(embedded.extraction.refs);

    nodes.insert(
        0,
        Node::new(
            file_symbol,
            NodeKind::File,
            file.path.clone(),
            language,
            floc,
        ),
    );
    Ok(Extraction {
        nodes,
        local_edges,
        refs,
    })
}

/// Page-item bind variables (`:P4_NAME`) of one embedded region, as refs from the region's owner
/// to page item `P4_NAME` of the application. A parsed region skips its string literals and comments and
/// yields typed refs; an unparsed one is a text scan (quotes and comments skipped by the data's
/// delimiters) and yields heuristic refs (target kind prefixed with
/// [`apexlang_ref::HEURISTIC`]).
fn bind_variable_refs(
    file: &SourceFile,
    region: &crate::embedded::EmbeddedRegion,
    outcome: &crate::embedded::RegionOutcome,
    owner: &SymbolId,
    app_symbol: &SymbolId,
    bind: &BindVariable,
    out: &mut Vec<UnresolvedRef>,
) {
    let Some(re) = bind.regex.as_ref() else {
        return;
    };
    let tag = region.tag.as_deref().unwrap_or("").to_ascii_lowercase();
    if !bind.tags.contains(&tag) {
        return;
    }
    let base = region.span.start_byte as usize;
    let Some(body) = file.text.get(base..region.span.end_byte as usize) else {
        return;
    };
    // Strings, quoted identifiers and comments are skipped by an Oracle-aware lexical scan in
    // every block, parsed or not: the built-in SQL grammar knows no `q'[...]'` literal, so its
    // literal nodes cannot be trusted to cover one.
    let parsed = outcome.language.is_some();
    let skipped = text_scan_skips(body, &bind.text_scan);
    let kind = if parsed {
        apexlang_ref::BINDABLE.to_string()
    } else {
        format!("{}{}", apexlang_ref::HEURISTIC, apexlang_ref::BINDABLE)
    };
    // An item's own SQL naming the item itself (`... or id = :P1_X` in P1_X's LOV) is not a
    // dependency of anything.
    let owner_key = owner
        .as_str()
        .rsplit('#')
        .nth(1)
        .and_then(|d| d.split_once(':'));
    // Matches and skipped ranges are both in order: one forward pass over each (no quadratic
    // rescans on a long block), and one line index for every match's position.
    let mut skipped = skipped;
    skipped.sort_unstable();
    let mut next_skip = 0;
    let lines = LineIndex::new(body);
    for caps in re.captures_iter(body) {
        let (Some(whole), Some(item)) = (caps.get(0), caps.name("item")) else {
            continue;
        };
        while next_skip < skipped.len() && skipped[next_skip].1 <= whole.start() {
            next_skip += 1;
        }
        if skipped
            .get(next_skip)
            .is_some_and(|&(s, e)| s <= whole.start() && whole.start() < e)
        {
            continue;
        }
        let name = item.as_str().to_ascii_uppercase();
        if owner_key.is_some_and(|(k, id)| {
            bind.targets.iter().any(|t| apexlang_ref::encode(t) == k)
                && apexlang_ref::decode(id).as_deref() == Some(name.as_str())
        }) {
            continue;
        }
        // Page item names are unique in an application, and the page number in the name is only a
        // convention (a copied page keeps `P3_X` on page 503), so the scope is the application.
        let mut r = UnresolvedRef::new(
            owner.clone(),
            apexlang_ref::format(&kind, &name, app_symbol.as_str()),
            EdgeKind::References,
            Location::new(
                &file.path,
                lines.span(region.span, whole.start(), whole.end()),
            ),
        );
        r.hints
            .insert("apexlang_bind_variable".into(), whole.as_str().into());
        out.push(r);
    }
}

/// Body byte ranges a text scan skips, in order: quoted strings (a doubled quote stays inside),
/// alternative-quoted strings (`q'[...]'`), quoted identifiers, and line and block comments, using
/// the data's delimiters.
fn text_scan_skips(body: &str, scan: &TextScan) -> Vec<(usize, usize)> {
    let b = body.as_bytes();
    let q = scan.quote.as_bytes();
    let iq = scan.identifier_quote.as_bytes();
    let lc = scan.line_comment.as_bytes();
    let (bo, bc) = (
        scan.block_comment[0].as_bytes(),
        scan.block_comment[1].as_bytes(),
    );
    let at = |i: usize, pat: &[u8]| !pat.is_empty() && b[i..].starts_with(pat);
    // Index just past the next `pat` at or after `from`, or the end.
    let past = |from: usize, pat: &[u8]| -> usize {
        (from..b.len())
            .find(|&j| b[j..].starts_with(pat))
            .map_or(b.len(), |j| j + pat.len())
    };
    let alt_quote = |i: usize| -> Option<usize> {
        // `q'X ... X'`: a prefix, the quote, a delimiter, then the closing delimiter + quote.
        if i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_') {
            return None;
        }
        let prefix = scan
            .alternative_quote_prefixes
            .iter()
            .map(String::as_bytes)
            .find(|p| {
                !p.is_empty()
                    && b.len() - i > p.len() + q.len()
                    && b[i..i + p.len()].eq_ignore_ascii_case(p)
                    && b[i + p.len()..].starts_with(q)
            })?;
        let d = i + prefix.len() + q.len();
        let open = body.get(d..)?.chars().next()?;
        let close = scan
            .alternative_quote_pairs
            .iter()
            .find(|[o, _]| o.starts_with(open))
            .and_then(|[_, c]| c.chars().next())
            .unwrap_or(open);
        let mut closing = close.to_string().into_bytes();
        closing.extend_from_slice(q);
        Some(past(d + open.len_utf8(), &closing))
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let end = if let Some(end) = alt_quote(i) {
            end
        } else if at(i, q) {
            // A quote doubled inside a string is part of it.
            let mut j = i + q.len();
            loop {
                let k = past(j, q);
                if k < b.len() && b[k..].starts_with(q) {
                    j = k + q.len();
                } else {
                    break k;
                }
            }
        } else if at(i, iq) {
            past(i + iq.len(), iq)
        } else if at(i, lc) {
            (i..b.len()).find(|&j| b[j] == b'\n').unwrap_or(b.len())
        } else if at(i, bo) {
            past(i + bo.len(), bc)
        } else {
            i += 1;
            continue;
        };
        out.push((i, end));
        i = end.max(i + 1);
    }
    out
}

/// Line starts of a region body, for host spans of body offsets in O(log lines).
struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(body: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(body.match_indices('\n').map(|(i, _)| i + 1));
        Self { starts }
    }

    fn pos(&self, region: Span, off: usize) -> (u32, u32) {
        let line = self.starts.partition_point(|&s| s <= off) - 1;
        let col = off - self.starts[line];
        let col = if line == 0 {
            region.start_col as usize + col
        } else {
            col
        };
        (region.start_line + line as u32, col as u32)
    }

    /// The host span of body bytes `[start, end)`.
    fn span(&self, region: Span, start: usize, end: usize) -> Span {
        let base = region.start_byte;
        let (sl, sc) = self.pos(region, start);
        let (el, ec) = self.pos(region, end);
        Span {
            start_byte: base + start as u32,
            end_byte: base + end as u32,
            start_line: sl,
            start_col: sc,
            end_line: el,
            end_col: ec,
        }
    }
}

/// The export-metadata file beside an `application.apx` (#316), for any path form (repo-relative
/// or absolute); `None` for any other file. The indexer and the freshness check both fold it into
/// the application file's digest.
pub fn export_metadata_sidecar(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let meta = &ApexlangData::builtin().export_metadata;
    (path.file_name()? == meta.host_file.as_str()).then(|| path.with_file_name("").join(&meta.file))
}

/// The repo-relative export-metadata file for an `application.apx` (#316), `None` for any other
/// file. The indexer reads it (the `.apex` directory is never walked) and hands it to
/// [`apply_export_metadata`].
pub fn export_metadata_path(rel_path: &str) -> Option<String> {
    let meta = &ApexlangData::builtin().export_metadata;
    let norm = rel_path.replace('\\', "/");
    let (dir, name) = match norm.rsplit_once('/') {
        Some((d, n)) => (Some(d), n),
        None => (None, norm.as_str()),
    };
    (name == meta.host_file).then(|| match dir {
        Some(d) => format!("{d}/{}", meta.file),
        None => meta.file.clone(),
    })
}

/// Store the export metadata version on the application node of an `application.apx`
/// extraction (#316). `sidecar` is the metadata file's bytes, `None` when it is missing or
/// unreadable (the version is then `unknown`). A major version the data does not list prints a
/// loud marker and flags the node; extraction is never failed.
pub fn apply_export_metadata(extraction: &mut Extraction, host_path: &str, sidecar: Option<&[u8]>) {
    let meta = &ApexlangData::builtin().export_metadata;
    let version = sidecar
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok())
        .and_then(|v| {
            v.get(&meta.version_key)
                .and_then(|v| v.as_str())
                .map(str::to_string)
        });
    let Some(app) = extraction
        .nodes
        .iter_mut()
        .find(|n| n.metadata.get(KIND_METADATA_KEY).and_then(|v| v.as_str()) == Some("app"))
    else {
        return;
    };
    match version {
        Some(v) => {
            let major = v.split(['.', '+']).next().unwrap_or("");
            let supported = meta.supported_majors.iter().any(|m| m == major);
            app.metadata
                .insert(MMD_VERSION_KEY.into(), v.clone().into());
            app.metadata
                .insert(MMD_SUPPORTED_KEY.into(), supported.into());
            if !supported {
                eprintln!(
                    "APEXLANG-UNSUPPORTED-VERSION: {host_path}: {} {v} (supported majors: {}); \
                     extracted anyway, the application node is flagged",
                    meta.version_key,
                    meta.supported_majors.join(", ")
                );
            }
        }
        None => {
            app.metadata
                .insert(MMD_VERSION_KEY.into(), "unknown".into());
            app.metadata.insert(MMD_SUPPORTED_KEY.into(), false.into());
        }
    }
}

/// The symbol a reference's target must sit under, per the rule's `scope`; `None` when the scope
/// does not exist for this owner.
fn scope_symbol(
    scope: &str,
    owner: usize,
    comps: &[Comp],
    virtual_ancestors: &[(String, SymbolId)],
) -> Option<SymbolId> {
    match scope {
        "app" => Some(virtual_ancestors[0].1.clone()),
        "self" => Some(comps[owner].symbol.clone()),
        "parent" => Some(match comps[owner].parent {
            Some(p) => comps[p].symbol.clone(),
            None => virtual_ancestors.last()?.1.clone(),
        }),
        kind => {
            let mut cur = comps[owner].parent;
            while let Some(i) = cur {
                if comps[i].kind == kind {
                    return Some(comps[i].symbol.clone());
                }
                cur = comps[i].parent;
            }
            virtual_ancestors
                .iter()
                .rev()
                .find(|(k, _)| k == kind)
                .map(|(_, s)| s.clone())
        }
    }
}
