//! The project linker against a store (DES-S2 §3, slice X1): `wicked-estate link`.
//!
//! [`link_project`] reads the member labels' stored manifests, lockfiles, installed-package
//! records, parked `imports` references and recorded roots/remotes (the store only: no checkout,
//! no disk), runs the pure computation in `wicked_estate_resolve::link`, and writes the result
//! as ONE support owner, `("wicked-link", "project:<id>")`, replaced whole:
//!
//! - every link is a File -> File `imports` support fact (resolved_by `wicked-link`), carrying
//!   `link.{package, ecosystem, identity, consumer_version, provider_version, skew}`;
//! - every boundary (a "possible" item) is a hub node in the virtual file `@link/<project>`;
//! - meta `link:<project>:*` records the generation, the input fingerprint, the validated member
//!   tokens, the coverage summary and the last error.
//!
//! An unchanged input (same fingerprint) writes no hub or support row, only the validation record.
//! A label whose last index did not complete (`repo:<label>:indexing` set) is refused and reported
//! `incomplete`. The caller holds the writer lock ([`crate::writer_lock`]).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use wicked_estate_core::support::{SupportFact, SupportOwner};
use wicked_estate_core::{
    Edge, EdgeKind, Error, Language, Location, Node, NodeKind, ResolutionTier, Result, Span,
    Symbol, SymbolId,
};
use wicked_estate_resolve::link as l;
use wicked_estate_store::GraphStoreMutExt;

use crate::repo_scope;

/// The support-plane producer every link fact is owned by.
pub const PRODUCER: &str = "wicked-link";
/// `resolved_by` of every link edge: how answers recognise a link (a crossing).
pub const RESOLVED_BY: &str = wicked_estate_core::crossing::LINK_RESOLVED_BY;

fn owner(project: &str) -> Result<SupportOwner> {
    SupportOwner::new(PRODUCER, format!("project:{project}"))
}

fn hub_file(project: &str) -> String {
    format!("@link/{project}")
}

fn key(project: &str, name: &str) -> String {
    format!("link:{project}:{name}")
}

/// What one `link` run did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct LinkReport {
    pub project: String,
    pub generation: u64,
    /// Same fingerprint as the stored generation: nothing but the validation record was written.
    pub replayed: bool,
    pub links: usize,
    pub links_by_identity: BTreeMap<String, usize>,
    pub boundaries_by_reason: BTreeMap<String, usize>,
    /// Every possible (unlinked) item: who imports what, why it stayed possible, the candidates.
    pub possible: Vec<Possible>,
    /// Labels refused because their last index did not complete.
    pub incomplete: Vec<String>,
    pub members: Vec<String>,
    pub fingerprint: String,
}

/// One possible item of a link report (a boundary without its file list).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Possible {
    pub reason: String,
    pub ecosystem: String,
    pub consumer: String,
    pub package: String,
    pub why: Vec<String>,
    pub candidates: Vec<String>,
    pub files: usize,
}

/// Per-member coverage recorded by the last link (DES-S2 §5): what could not be linked, by reason.
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct MemberCoverage {
    pub label: String,
    pub links_out: usize,
    pub links_in: usize,
    /// reason -> packages
    pub boundaries: BTreeMap<String, Vec<String>>,
    pub incomplete: bool,
    /// Parked (unresolved) imports of this member, the pool package imports come from, and the
    /// manifests the linker read: imports with zero manifests means nothing could be declared,
    /// so nothing was linked.
    #[serde(default)]
    pub package_imports: usize,
    #[serde(default)]
    pub manifests: usize,
    /// reason -> number of packages (filled by [`LinkStatus::compact`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub boundary_counts: BTreeMap<String, usize>,
}

/// Gather one label's linker input from the store.
fn label_input(store: &dyn GraphStoreMutExt, label: &str) -> Result<l::LabelInput> {
    let pre = if label.is_empty() {
        String::new()
    } else {
        format!("{label}/")
    };
    let files: BTreeSet<String> = store
        .indexed_files()?
        .into_iter()
        .filter(|f| f.starts_with(&pre))
        .collect();
    let mut manifests = BTreeMap::new();
    for f in &files {
        let name = f.rsplit('/').next().unwrap_or(f);
        let wanted = matches!(
            name,
            "package.json"
                | "package-lock.json"
                | "pnpm-workspace.yaml"
                | "Cargo.toml"
                | "pyproject.toml"
        ) || (name.starts_with("tsconfig") && name.ends_with(".json"));
        if wanted && !f.contains("/node_modules/") {
            if let Some(t) = store.file_content(f)? {
                manifests.insert(f.clone(), t);
            }
        }
    }
    let record = if label.is_empty() {
        None
    } else {
        repo_scope::record(store, label)
    };
    let (root, remote, subpath) = match record {
        Some(r) => (Some(r.root), r.info.remote, r.subpath),
        None => (
            store.meta_get_key("indexed_root"),
            store.meta_get_key("repo_remote"),
            None,
        ),
    };
    // Lockfile evidence recorded at index time (the stored lockfile, when the index kept it,
    // is the same text and wins).
    let locks: BTreeMap<String, String> = store
        .meta_get_key(&repo_scope::meta_key(opt(label), "dep_locks"))
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    for (path, text) in locks {
        manifests.entry(path).or_insert(text);
    }
    let artifacts = store
        .meta_get_key(&repo_scope::meta_key(opt(label), "dep_artifacts"))
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    let imports = store
        .parked_import_refs(&pre)?
        .into_iter()
        .map(|(file, raw)| l::ImportRef { file, raw })
        .collect();
    Ok(l::LabelInput {
        label: label.to_string(),
        root,
        remote,
        subpath,
        files,
        manifests,
        artifacts,
        imports,
    })
}

fn opt(label: &str) -> Option<&str> {
    (!label.is_empty()).then_some(label)
}

/// The label's published generation and whether an index is in flight / failed.
fn token(store: &dyn GraphStoreMutExt, label: &str) -> (String, bool) {
    let generation = store
        .meta_get_key(&repo_scope::meta_key(opt(label), "graph_gen"))
        .unwrap_or_default();
    let indexing = store
        .meta_get_key(&repo_scope::meta_key(opt(label), "indexing"))
        .is_some_and(|v| !v.is_empty());
    (generation, indexing)
}

/// Run the project linker over `labels` (the authoritative member set; other labels in the
/// database are ignored and nothing links to them). The caller holds the writer lock.
pub fn link_project(
    store: &mut dyn GraphStoreMutExt,
    project: &str,
    labels: &[String],
) -> Result<LinkReport> {
    if project.is_empty() || project.contains('/') {
        return Err(Error::Invalid(format!(
            "link: invalid project id {project:?}"
        )));
    }
    crate::ensure_redacted(store)?;
    // One project per database: another project's links would stay traversable from this one's
    // answers, outside its member set (crew keeps one database per project).
    if let Some(other) = projects(store)
        .into_iter()
        .find(|p| p != project && !validated_members(store, p).is_empty())
    {
        return Err(Error::Invalid(format!(
            "link: this database is linked as project {other:?}; one project per database"
        )));
    }
    let result = link_inner(store, project, labels);
    if let Err(e) = &result {
        store.meta_set_key(
            &key(project, "last_error"),
            &format!("{} {e}", crate::rfc3339_now()),
        );
    }
    result
}

fn validated_members(store: &dyn GraphStoreMutExt, project: &str) -> Vec<String> {
    store
        .meta_get_key(&key(project, "validated_members"))
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default()
}

/// What a stored generation was computed under: the linker version and its rules.
fn rules_stamp() -> String {
    format!("{}:{}", l::LINKER_VERSION, l::rules_digest())
}

fn link_inner(
    store: &mut dyn GraphStoreMutExt,
    project: &str,
    labels: &[String],
) -> Result<LinkReport> {
    let mut report = LinkReport {
        project: project.to_string(),
        ..Default::default()
    };
    let mut inputs = Vec::new();
    let mut tokens = BTreeMap::new();
    for label in labels {
        let (generation, indexing) = token(store, label);
        if indexing {
            report.incomplete.push(label.clone());
            continue;
        }
        tokens.insert(label.clone(), generation);
        inputs.push(label_input(store, label)?);
        report.members.push(label.clone());
    }
    if !report.incomplete.is_empty() {
        // A member mid-index (or whose last index failed) has no complete input: publishing a
        // generation without it would validate the project as fresh. The previous generation
        // stays, and the project reads stale until the member's index completes and link reruns.
        report.generation = store
            .meta_get_key(&key(project, "generation"))
            .and_then(|g| g.parse().ok())
            .unwrap_or(0);
        if report.generation == 0 {
            // First run: register the project at generation 0 so its status (stale, with this
            // error) is visible rather than absent.
            remember_project(store, project)?;
            store.meta_put(&key(project, "generation"), "0")?;
            store.meta_put(
                &key(project, "validated_members"),
                &serde_json::to_string(labels)?,
            )?;
        }
        store.meta_put(
            &key(project, "last_error"),
            &format!(
                "{} not linked: incomplete member(s) {}",
                crate::rfc3339_now(),
                report.incomplete.join(", ")
            ),
        )?;
        return Ok(report);
    }
    let out = l::link(&inputs);
    report.fingerprint = out.fingerprint.clone();
    let owner = owner(project)?;
    let prev_gen = store.support_generation(&owner)?.unwrap_or(0);
    let prev_fp = store.meta_get_key(&key(project, "fingerprint"));
    let members_json = serde_json::to_string(&report.members)?;
    let tokens_json = serde_json::to_string(&tokens)?;

    for e in &out.edges {
        *report
            .links_by_identity
            .entry(e.identity.as_str().to_string())
            .or_default() += 1;
    }
    for b in &out.boundaries {
        *report
            .boundaries_by_reason
            .entry(b.reason.as_str().to_string())
            .or_default() += 1;
        report.possible.push(Possible {
            reason: b.reason.as_str().to_string(),
            ecosystem: b.ecosystem.clone(),
            consumer: b.consumer_label.clone(),
            package: b.package.clone(),
            why: b.why.clone(),
            candidates: b.candidates.clone(),
            files: b.files.len(),
        });
    }
    report.links = out.edges.len();

    let stamp = rules_stamp();
    // Replay only when the published support generation is still the one this record validated
    // (a `supports retract` of the link owner moves it, and then the links are rewritten).
    let published = store
        .meta_get_key(&key(project, "generation"))
        .and_then(|g| g.parse::<u64>().ok());
    if prev_fp.as_deref() == Some(out.fingerprint.as_str())
        && prev_gen > 0
        && published == Some(prev_gen)
        && store.meta_get_key(&key(project, "rules")).as_deref() == Some(stamp.as_str())
    {
        // Replay: only the validation record moves (one transaction).
        report.replayed = true;
        report.generation = prev_gen;
        store.begin_batch()?;
        let write = (|| -> Result<()> {
            store.meta_put(&key(project, "validated_tokens"), &tokens_json)?;
            store.meta_put(&key(project, "validated_members"), &members_json)?;
            store.meta_put(&key(project, "validated_at"), &crate::rfc3339_now())?;
            store.meta_put(&key(project, "last_error"), "")
        })();
        return match write {
            Ok(()) => store.commit_batch().map(|()| report),
            Err(e) => {
                store.rollback_batch()?;
                Err(e)
            }
        };
    }

    let generation = prev_gen + 1;
    let facts: Vec<SupportFact> = out
        .edges
        .iter()
        // One fact per package relation: two packages entering through one file are two facts.
        .map(|e| {
            SupportFact::new(
                format!(
                    "{} -> {} [{}:{}]",
                    e.from_file, e.to_file, e.ecosystem, e.package
                ),
                link_edge(e),
            )
        })
        .collect::<Result<_>>()?;
    let hubs: Vec<Node> = out
        .boundaries
        .iter()
        .map(|b| boundary_node(project, b))
        .collect();
    let coverage = coverage_of(&report.members, &report.incomplete, &out, &inputs);
    let coverage_json = serde_json::to_string(&coverage)?;

    // The graph AND its validation record publish in one transaction: a failure (or a crash)
    // leaves the previous generation with its own record (G2.6).
    store.begin_batch()?;
    let write = (|| -> Result<()> {
        store.remove_file(&hub_file(project))?;
        if !hubs.is_empty() {
            store.upsert_nodes(&hubs)?;
        }
        store.replace_edge_supports(&owner, generation, &facts)?;
        remember_project(store, project)?;
        store.meta_put(&key(project, "generation"), &generation.to_string())?;
        store.meta_put(&key(project, "fingerprint"), &out.fingerprint)?;
        store.meta_put(&key(project, "validated_tokens"), &tokens_json)?;
        store.meta_put(&key(project, "validated_members"), &members_json)?;
        store.meta_put(&key(project, "validated_at"), &crate::rfc3339_now())?;
        store.meta_put(&key(project, "rules"), &stamp)?;
        store.meta_put(&key(project, "coverage"), &coverage_json)?;
        store.meta_put(&key(project, "last_error"), "")?;
        // Cached answers predate these links.
        store.version_bump();
        Ok(())
    })();
    match write {
        Ok(()) => store.commit_batch()?,
        Err(e) => {
            store.rollback_batch()?;
            return Err(e);
        }
    }
    report.generation = generation;
    Ok(report)
}

fn link_edge(e: &l::LinkEdge) -> Edge {
    let mut edge = Edge::new(
        Symbol::file(e.from_file.clone()).id(),
        Symbol::file(e.to_file.clone()).id(),
        EdgeKind::Imports,
        ResolutionTier::ImportMap,
        RESOLVED_BY,
    );
    // The identity rule's confidence is the edge's own, so traversal and filters see it.
    edge.confidence = wicked_estate_core::Confidence::new(e.identity.confidence());
    let md = &mut edge.metadata;
    md.insert("link.ranks".into(), serde_json::json!(["package-manifest"]));
    md.insert("link.package".into(), e.package.clone().into());
    md.insert("link.ecosystem".into(), e.ecosystem.clone().into());
    md.insert("link.identity".into(), e.identity.as_str().into());
    md.insert(
        "link.confidence".into(),
        f64::from(e.identity.confidence()).into(),
    );
    md.insert(
        "link.consumer_label".into(),
        e.consumer_label.clone().into(),
    );
    md.insert(
        "link.provider_label".into(),
        e.provider_label.clone().into(),
    );
    if let Some(v) = &e.consumer_version {
        md.insert("link.consumer_version".into(), v.clone().into());
    }
    if let Some(v) = &e.provider_version {
        md.insert("link.provider_version".into(), v.clone().into());
    }
    md.insert("link.skew".into(), e.skew.into());
    edge
}

fn boundary_node(project: &str, b: &l::Boundary) -> Node {
    let id = format!(
        "@link/{project}/boundary/{}/{}/{}/{}",
        b.reason.as_str(),
        b.ecosystem,
        b.consumer_label,
        b.package
    );
    let mut n = Node::new(
        SymbolId(id),
        NodeKind::Other("boundary".into()),
        b.package.clone(),
        Language::new("link"),
        Location::new(hub_file(project), Span::ZERO),
    );
    let md = &mut n.metadata;
    md.insert("boundary.reason".into(), b.reason.as_str().into());
    md.insert("boundary.ecosystem".into(), b.ecosystem.clone().into());
    md.insert(
        "boundary.consumer_label".into(),
        b.consumer_label.clone().into(),
    );
    md.insert("boundary.files".into(), serde_json::json!(b.files));
    md.insert(
        "boundary.candidates".into(),
        serde_json::json!(b.candidates),
    );
    md.insert("boundary.why".into(), serde_json::json!(b.why));
    n
}

fn coverage_of(
    members: &[String],
    incomplete: &[String],
    out: &l::LinkOutput,
    inputs: &[l::LabelInput],
) -> Vec<MemberCoverage> {
    let mut by: BTreeMap<String, MemberCoverage> = BTreeMap::new();
    for m in members.iter().chain(incomplete) {
        by.insert(
            m.clone(),
            MemberCoverage {
                label: m.clone(),
                incomplete: incomplete.contains(m),
                ..Default::default()
            },
        );
    }
    for e in &out.edges {
        if let Some(c) = by.get_mut(&e.consumer_label) {
            c.links_out += 1;
        }
        if let Some(c) = by.get_mut(&e.provider_label) {
            c.links_in += 1;
        }
    }
    for b in &out.boundaries {
        if let Some(c) = by.get_mut(&b.consumer_label) {
            c.boundaries
                .entry(b.reason.as_str().to_string())
                .or_default()
                .push(b.package.clone());
        }
    }
    for li in inputs {
        if let Some(c) = by.get_mut(&li.label) {
            c.package_imports = li.imports.len();
            c.manifests = li.manifests.len();
        }
    }
    by.into_values().collect()
}

// ── Staleness / status, for answers ───────────────────────────────────────────────────────────

/// The link state answers report (DES-S2 §5, §6).
#[derive(Debug, Clone, Default, Serialize)]
pub struct LinkStatus {
    pub project: String,
    pub generation: u64,
    pub stale: bool,
    /// Members whose index moved, was removed, is mid-index or failed since the link was validated.
    pub stale_labels: Vec<String>,
    pub last_error: Option<String>,
    pub coverage: Vec<MemberCoverage>,
}

impl LinkStatus {
    /// The bounded form answers carry: each member's boundary packages become counts
    /// (`boundaries` maps a reason to at most one package, `boundary_counts` has the totals).
    pub fn compact(mut self) -> Self {
        for c in &mut self.coverage {
            for (reason, pkgs) in &mut c.boundaries {
                c.boundary_counts.insert(reason.clone(), pkgs.len());
                pkgs.truncate(1);
            }
        }
        self
    }
}

/// Every project id with a link generation in this store.
pub fn projects(store: &dyn GraphStoreMutExt) -> Vec<String> {
    store
        .meta_get_key("link_projects")
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default()
}

/// Compact live status of every linked project, for CLI and transport annotations.
pub fn answer_statuses(store: &dyn GraphStoreMutExt) -> Vec<LinkStatus> {
    projects(store)
        .iter()
        .filter_map(|project| status(store, project))
        .map(LinkStatus::compact)
        .collect()
}

/// The answers' `link` array, bounded to `budget` chars: while it is longer, the longest coverage
/// list (ties: the first project) loses its last member entry, and that project's object counts the
/// cut in `coverage_omitted` (absent when nothing was cut; `link --status` stays complete). Once no
/// coverage is left, the longest `stale_labels` list is cut the same way and counted in
/// `stale_labels_omitted`, and then a `last_error` whose JSON string is longer than 200 bytes is cut
/// to fit that and marked `last_error_truncated` (wicked-estate#559; bytes, so escaped control
/// characters count at their serialized size). The other status fields are never dropped, even
/// when they alone exceed `budget`. JSON-level counts, not [`LinkStatus`] fields, so the public struct
/// keeps its shape.
pub fn bounded_json(statuses: Vec<LinkStatus>, budget: usize) -> serde_json::Value {
    let mut rows: Vec<serde_json::Value> = statuses
        .iter()
        .map(|s| serde_json::to_value(s).unwrap_or_default())
        .collect();
    let list_len = |row: &serde_json::Value, list: &str| row[list].as_array().map_or(0, Vec::len);
    loop {
        let value = serde_json::Value::Array(rows.clone());
        if value.to_string().len() <= budget {
            return value;
        }
        let longest = |list: &str| {
            rows.iter()
                .enumerate()
                .filter(|(_, row)| list_len(row, list) > 0)
                .max_by(|(ai, a), (bi, b)| {
                    list_len(a, list)
                        .cmp(&list_len(b, list))
                        .then_with(|| bi.cmp(ai))
                })
                .map(|(index, _)| index)
        };
        let Some((index, list, count)) = longest("coverage")
            .map(|i| (i, "coverage", "coverage_omitted"))
            .or_else(|| {
                longest("stale_labels").map(|i| (i, "stale_labels", "stale_labels_omitted"))
            })
        else {
            // Last, a long `last_error` is cut to LAST_ERROR_CAP serialized bytes, marked truncated.
            let long = rows.iter().position(|row| {
                row["last_error"]
                    .as_str()
                    .is_some_and(|e| json_len(e) > LAST_ERROR_CAP)
            });
            let Some(index) = long else {
                return value;
            };
            let mut cut = String::new();
            let mut used = 0;
            for c in rows[index]["last_error"]
                .as_str()
                .unwrap_or_default()
                .chars()
            {
                used += json_len(c.encode_utf8(&mut [0; 4]));
                if used > LAST_ERROR_CAP {
                    break;
                }
                cut.push(c);
            }
            rows[index]["last_error"] = serde_json::json!(cut);
            rows[index]["last_error_truncated"] = serde_json::json!(true);
            continue;
        };
        let row = &mut rows[index];
        if let Some(items) = row[list].as_array_mut() {
            items.pop();
        }
        let omitted = row[count].as_u64().unwrap_or(0) + 1;
        row[count] = serde_json::json!(omitted);
    }
}

/// Most names one link diagnostic lists before it counts the rest.
const DIAG_NAME_CAP: usize = 3;
/// Longest `last_error` the bounded answer form keeps once the lists are cut (bytes of its JSON
/// string body, escapes included).
const LAST_ERROR_CAP: usize = 200;

/// `text`'s length inside a JSON string, escapes included, quotes excluded.
fn json_len(text: &str) -> usize {
    serde_json::to_string(text).map_or(text.len(), |j| j.len() - 2)
}

/// `names`, at most [`DIAG_NAME_CAP`] of them, then `+N more`.
fn capped_names<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    let names: Vec<&str> = names.into_iter().collect();
    let mut out = names
        .iter()
        .take(DIAG_NAME_CAP)
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > DIAG_NAME_CAP {
        out.push_str(&format!(", +{} more", names.len() - DIAG_NAME_CAP));
    }
    out
}

/// `arg` as one POSIX shell word: bare when it is made of characters no shell treats specially,
/// else single-quoted (wicked-estate#559 review: a suggested command must run as written, and a
/// project id or path must not inject shell syntax).
fn shell_word(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./:,=@%+-".contains(c));
    if plain {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

/// A validated member that is no longer in the graph: removed, or its label unregistered.
fn member_gone(store: &dyn GraphStoreMutExt, label: &str) -> bool {
    !label.is_empty()
        && (repo_scope::record(store, label).is_none()
            || !repo_scope::labels(store).iter().any(|l| l == label))
}

/// Longest database path a link diagnostic echoes (chars, quoted); a longer one is named by the
/// quoted placeholder `'<graph.db>'`, so a deep path cannot spend the link budget and a pasted
/// command stays one inert word (`link` refuses a missing graph, main.rs `require_existing_graph`).
const DB_ECHO_CAP: usize = 200;

/// The `--db` argument the diagnostics' commands carry.
fn db_word(db: &str) -> String {
    let word = shell_word(db);
    if word.len() <= DB_ECHO_CAP {
        word
    } else {
        "'<graph.db>'".to_string()
    }
}

/// The `LINK:` line counting the member entries the bounded `link` array cut, per project when
/// `named`, else in total (a form that echoes no project id, for ids too long to repeat).
fn omitted_line(cut: &[(String, u64)], db: &str, named: bool) -> String {
    let which = if named {
        let named: Vec<String> = cut.iter().map(|(p, n)| format!("'{p}' ({n})")).collect();
        format!(
            "for project(s) {}",
            capped_names(named.iter().map(String::as_str))
        )
    } else {
        let total: u64 = cut.iter().map(|(_, n)| n).sum();
        format!("({total}) across {} project(s)", cut.len())
    };
    format!(
        "LINK: member entries omitted {which}; see `wicked-estate link --db {} \
         --project '<id>' --status --json` for the full lists",
        db_word(db)
    )
}

/// What the transport adds to an answer that carries `crossings` (DES-S2 §5): the bounded `link`
/// array and its diagnostics, together within `budget` chars (wicked-estate#559; the answer
/// reserves [`wicked_estate_retrieve::LINK_RESERVE`] for both). One `LINK-STALE:` line per stale
/// project names the relink command over the project's validated members still in the graph;
/// when those lines would take more than half the budget they fold into one line naming the
/// projects, and past that into a line that echoes no stored value. The `link` array gets the
/// rest, less the exact room for the one `LINK:` line that counts cut entries. `db` is the path
/// the commands name. Arguments are shell-quoted.
pub fn answer_annotation(
    store: &dyn GraphStoreMutExt,
    db: &str,
    budget: usize,
) -> (serde_json::Value, Vec<String>) {
    let statuses = answer_statuses(store);
    let chars = |lines: &[String]| lines.iter().map(|l| l.len() + 1).sum::<usize>();
    let stale: Vec<&LinkStatus> = statuses.iter().filter(|s| s.stale).collect();
    let db_word = db_word(db);
    let mut lines: Vec<String> = stale
        .iter()
        .map(|status| {
            let reason = if !status.stale_labels.is_empty() {
                format!(
                    "members re-indexed or removed: {}",
                    capped_names(status.stale_labels.iter().map(String::as_str))
                )
            } else if status.last_error.is_some() {
                "the last link failed".to_string()
            } else {
                "the linker changed".to_string()
            };
            // The project's members as its last link recorded them (coverage is absent after
            // an incomplete first link), less any removed one: naming it would make the
            // command fail.
            let labels: Vec<String> = validated_members(store, &status.project)
                .into_iter()
                .filter(|label| !member_gone(store, label))
                .collect();
            let project = &status.project;
            let action = if labels.is_empty() {
                "no member is still in the graph; re-index its members, then relink".to_string()
            } else {
                format!(
                    "re-run `wicked-estate link --db {db_word} --project {} --labels {}`",
                    shell_word(project),
                    shell_word(&labels.join(","))
                )
            };
            format!(
                "LINK-STALE: project '{project}' links are stale ({reason}); cross-repo \
                 dependents may be missing or outdated - {action}"
            )
        })
        .collect();
    if chars(&lines) > budget / 2 {
        lines = vec![format!(
            "LINK-STALE: {} project(s) have stale links ({}); cross-repo dependents may be \
             missing or outdated - see `wicked-estate link --db {db_word} --project '<id>' \
             --status --json` for the members to relink",
            stale.len(),
            capped_names(stale.iter().map(|s| s.project.as_str()))
        )];
    }
    if chars(&lines) > budget / 2 {
        lines = vec![format!(
            "LINK-STALE: {} project(s) have stale links; see `wicked-estate link --project \
             '<id>' --status --json`",
            stale.len()
        )];
    }
    // Room for the LINK: line at its longest: every project with entries to cut, each with all
    // of them (one project per database, link.rs `link_project`, so this is the exact line).
    let worst: Vec<(String, u64)> = statuses
        .iter()
        .map(|s| {
            (
                s.project.clone(),
                (s.coverage.len() + s.stale_labels.len()) as u64,
            )
        })
        .filter(|(_, n)| *n > 0)
        .collect();
    // A project list longer than a quarter of the budget is counted, not named, so a long id
    // cannot crowd out the array that must carry it.
    let named = omitted_line(&worst, db, true).len() <= budget / 4;
    let reserve = if worst.is_empty() {
        0
    } else {
        omitted_line(&worst, db, named).len() + 1
    };
    let links = bounded_json(statuses, budget.saturating_sub(chars(&lines) + reserve));
    let cut: Vec<(String, u64)> = links
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let omitted = row["coverage_omitted"].as_u64().unwrap_or(0)
                + row["stale_labels_omitted"].as_u64().unwrap_or(0);
            (omitted > 0).then(|| {
                (
                    row["project"].as_str().unwrap_or_default().to_string(),
                    omitted,
                )
            })
        })
        .collect();
    if !cut.is_empty() {
        lines.push(omitted_line(&cut, db, named));
    }
    (links, lines)
}

/// Remember `project` as linked in this store (so answers can find its status).
pub fn remember_project(store: &mut dyn GraphStoreMutExt, project: &str) -> Result<()> {
    let mut all = projects(store);
    if !all.iter().any(|p| p == project) {
        all.push(project.to_string());
        all.sort();
        store.meta_put("link_projects", &serde_json::to_string(&all)?)?;
    }
    Ok(())
}

/// Compare the live member tokens with the ones the last link validated.
pub fn status(store: &dyn GraphStoreMutExt, project: &str) -> Option<LinkStatus> {
    let generation: u64 = store
        .meta_get_key(&key(project, "generation"))?
        .parse()
        .ok()?;
    let tokens: BTreeMap<String, String> = store
        .meta_get_key(&key(project, "validated_tokens"))
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    let mut stale_labels = Vec::new();
    // Every validated member must still exist, with the generation it was linked at.
    for label in validated_members(store, project) {
        let gone = member_gone(store, &label);
        let (live, indexing) = token(store, &label);
        if gone || indexing || tokens.get(&label) != Some(&live) {
            stale_labels.push(label);
        }
    }
    let last_error = store
        .meta_get_key(&key(project, "last_error"))
        .filter(|e| !e.is_empty());
    let version_moved =
        store.meta_get_key(&key(project, "rules")).as_deref() != Some(rules_stamp().as_str());
    let coverage = store
        .meta_get_key(&key(project, "coverage"))
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    Some(LinkStatus {
        project: project.to_string(),
        generation,
        stale: !stale_labels.is_empty() || last_error.is_some() || version_moved,
        stale_labels,
        last_error,
        coverage,
    })
}

// ── Installed npm packages, recorded at index time ───────────────────────────────────────────

/// For every `package.json` of the indexed label, record each declared dependency's INSTALLED
/// manifest (`node_modules/<dep>/package.json`, resolved the way Node does: the nearest
/// `node_modules` at or above the manifest, inside the root) and its installed-tree record
/// (`node_modules/.package-lock.json`). Read through the confined reader and redacted with X0's
/// manifest projection before it is kept (DES-S1 §2.1 rule 3, R4-02). Best-effort: a missing
/// install records nothing (the linker then reports `dep:provider-unverified`).
pub fn record_dep_artifacts(store: &mut dyn GraphStoreMutExt, root: &Path, repo: Option<&str>) {
    let Some(confined) = crate::confined::ConfinedRoot::new(root) else {
        return;
    };
    let pre = repo.map(|l| format!("{l}/")).unwrap_or_default();
    let manifests: Vec<String> = store
        .indexed_files()
        .unwrap_or_default()
        .into_iter()
        .filter(|f| {
            f.starts_with(&pre) && f.ends_with("package.json") && !f.ends_with(".package-lock.json")
        })
        .filter(|f| f.rsplit('/').next() == Some("package.json") && !f.contains("/node_modules/"))
        .collect();
    let mut out: Vec<l::DepArtifact> = Vec::new();
    // Lockfile projections: store path -> the `packages` entries of the declared dependencies.
    // A lockfile is often too large for the index to keep (minified/huge files are skipped), so
    // the linker's lock evidence is recorded here, read the same confined, redacted way.
    let mut locks: BTreeMap<String, serde_json::Map<String, serde_json::Value>> = BTreeMap::new();
    for m in manifests {
        // Read from the checkout (confined, redacted the way the index stores it): the stored
        // text may be withheld at this point (the redaction guard), and the index just read the
        // same file.
        let rel = m.strip_prefix(&pre).unwrap_or(&m);
        let Some(text) = confined
            .read(&root.join(rel))
            .and_then(|b| String::from_utf8(b).ok())
            .map(|t| wicked_estate_extract::redact::redact_source("package.json", &t))
        else {
            continue;
        };
        let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let manifest_dir = m
            .rsplit_once('/')
            .map(|(d, _)| d.to_string())
            .unwrap_or_default();
        let local_dir = manifest_dir
            .strip_prefix(pre.trim_end_matches('/'))
            .unwrap_or(&manifest_dir)
            .trim_start_matches('/');
        let mut deps: BTreeSet<String> = BTreeSet::new();
        for t in [
            "dependencies",
            "devDependencies",
            "peerDependencies",
            "optionalDependencies",
        ] {
            if let Some(o) = pkg.get(t).and_then(|x| x.as_object()) {
                deps.extend(o.keys().cloned());
            }
        }
        if let Some((lock_local, lock)) = nearest_lockfile(&confined, root, local_dir) {
            let rel = local_dir
                .strip_prefix(lock_local.as_str())
                .unwrap_or(local_dir)
                .trim_matches('/');
            let lock_store = format!(
                "{pre}{}{}",
                lock_local,
                if lock_local.is_empty() {
                    "package-lock.json"
                } else {
                    "/package-lock.json"
                }
            );
            let entries = locks.entry(lock_store).or_default();
            for dep in &deps {
                let keys = [
                    if rel.is_empty() {
                        format!("node_modules/{dep}")
                    } else {
                        format!("{rel}/node_modules/{dep}")
                    },
                    format!("node_modules/{dep}"),
                ];
                for k in keys {
                    if let Some(e) = lock.get("packages").and_then(|p| p.get(&k)) {
                        entries.insert(k, e.clone());
                    }
                }
            }
        }
        for dep in deps {
            if let Some(a) = installed_artifact(&confined, root, local_dir, &dep) {
                out.push(l::DepArtifact {
                    manifest_dir: manifest_dir.clone(),
                    ..a
                });
            }
        }
    }
    if let Ok(j) = serde_json::to_string(&out) {
        store.meta_set_key(&repo_scope::meta_key(repo, "dep_artifacts"), &j);
    }
    let locks: BTreeMap<String, String> = locks
        .into_iter()
        .map(|(k, packages)| (k, serde_json::json!({ "packages": packages }).to_string()))
        .collect();
    if let Ok(j) = serde_json::to_string(&locks) {
        store.meta_set_key(&repo_scope::meta_key(repo, "dep_locks"), &j);
    }
}

/// The nearest `package-lock.json` at or above `local_dir` inside `root`: its directory (relative
/// to the root) and its redacted JSON.
fn nearest_lockfile(
    confined: &crate::confined::ConfinedRoot,
    root: &Path,
    local_dir: &str,
) -> Option<(String, serde_json::Value)> {
    let mut rel: Vec<&str> = local_dir.split('/').filter(|s| !s.is_empty()).collect();
    loop {
        let dir = rel.join("/");
        let path = if dir.is_empty() {
            root.join("package-lock.json")
        } else {
            root.join(&dir).join("package-lock.json")
        };
        if let Some(bytes) = confined.read(&path) {
            let text = String::from_utf8(bytes).ok()?;
            let text = wicked_estate_extract::redact::redact_source("package-lock.json", &text);
            return Some((dir, serde_json::from_str(&text).ok()?));
        }
        rel.pop()?;
    }
}

fn installed_artifact(
    confined: &crate::confined::ConfinedRoot,
    root: &Path,
    local_dir: &str,
    dep: &str,
) -> Option<l::DepArtifact> {
    let mut dir = if local_dir.is_empty() {
        root.to_path_buf()
    } else {
        root.join(local_dir)
    };
    loop {
        let nm = dir.join("node_modules");
        let manifest = nm.join(dep).join("package.json");
        if let Some(bytes) = confined.read(&manifest) {
            let text = String::from_utf8(bytes).ok()?;
            let text = wicked_estate_extract::redact::redact_source("package.json", &text);
            let v: serde_json::Value = serde_json::from_str(&text).ok()?;
            let (repository, directory) = match v.get("repository") {
                Some(serde_json::Value::String(s)) => (Some(s.clone()), None),
                Some(o) => (
                    o.get("url").and_then(|u| u.as_str()).map(str::to_string),
                    o.get("directory")
                        .and_then(|u| u.as_str())
                        .map(str::to_string),
                ),
                None => (None, None),
            };
            // The record EXISTS when the file does: an unreadable or malformed one is a broken
            // install (no entry -> never bound), never "no record" (a version-only downgrade).
            let tree_bytes = confined.read(&nm.join(".package-lock.json"));
            let tree_record = tree_bytes.is_some();
            let tree = tree_bytes
                .and_then(|b| String::from_utf8(b).ok())
                .map(|t| wicked_estate_extract::redact::redact_source(".package-lock.json", &t))
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
            let entry = tree
                .as_ref()
                .and_then(|t| t.get("packages"))
                .and_then(|p| p.get(format!("node_modules/{dep}")));
            let s = |k: &str| {
                entry
                    .and_then(|e| e.get(k))
                    .and_then(|x| x.as_str())
                    .map(str::to_string)
            };
            return Some(l::DepArtifact {
                manifest_dir: String::new(),
                package: dep.to_string(),
                name: v.get("name").and_then(|x| x.as_str()).map(str::to_string),
                tree_record,
                version: v
                    .get("version")
                    .and_then(|x| x.as_str())
                    .map(str::to_string),
                repository,
                directory,
                tree_version: s("version"),
                tree_resolved: s("resolved"),
                tree_integrity: s("integrity"),
            });
        }
        if dir == root || !dir.pop() || !dir.starts_with(root) {
            return None;
        }
    }
}

#[cfg(test)]
mod answer_tests {
    use super::*;

    #[test]
    fn bounded_json_cuts_and_counts_stale_labels_once_coverage_is_gone_559() {
        let status = LinkStatus {
            project: "p".into(),
            generation: 1,
            stale: true,
            stale_labels: (0..40)
                .map(|i| format!("member_{i:02}_with_a_long_descriptive_name"))
                .collect(),
            coverage: vec![MemberCoverage {
                label: "only".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let bounded = bounded_json(vec![status], 600);
        assert!(bounded.to_string().len() <= 600, "{bounded}");
        let row = &bounded[0];
        assert_eq!(row["coverage"], serde_json::json!([]));
        assert_eq!(row["coverage_omitted"], 1);
        let kept = row["stale_labels"].as_array().unwrap().len() as u64;
        assert!(kept > 0, "{row}");
        assert_eq!(kept + row["stale_labels_omitted"].as_u64().unwrap(), 40);
        assert_eq!(row["stale"], true);
    }

    #[test]
    fn answer_status_coverage_is_bounded_and_counts_every_omission() {
        let make = |project: &str, count: usize| LinkStatus {
            project: project.into(),
            generation: 7,
            coverage: (0..count)
                .map(|i| MemberCoverage {
                    label: format!("member_{i:03}_with_a_long_descriptive_name"),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let statuses = vec![make("long", 12), make("short", 2)];
        let full = serde_json::to_value(&statuses).unwrap();
        assert!(full[0].get("coverage_omitted").is_none());
        let budget = serde_json::to_string(&full).unwrap().len() - 1;
        let bounded = bounded_json(statuses.clone(), budget);
        assert!(serde_json::to_string(&bounded).unwrap().len() <= budget);
        assert_eq!(bounded[0]["coverage_omitted"], 1);
        assert_eq!(bounded[1]["coverage"].as_array().unwrap().len(), 2);
        assert!(bounded[1].get("coverage_omitted").is_none());

        let bounded = bounded_json(statuses, 500);
        assert!(serde_json::to_string(&bounded).unwrap().len() <= 500);
        for (i, total) in [12, 2].into_iter().enumerate() {
            let kept = bounded[i]["coverage"].as_array().unwrap().len();
            let omitted = bounded[i]["coverage_omitted"].as_u64().unwrap_or(0) as usize;
            assert_eq!(kept + omitted, total);
            assert_eq!(bounded[i]["generation"], 7);
            assert_eq!(bounded[i]["stale"], false);
        }
    }
}
