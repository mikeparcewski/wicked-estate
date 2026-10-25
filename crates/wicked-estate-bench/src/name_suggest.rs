//! Did-you-mean suggestion bench (wicked-estate#358): top-3 hit rate and p95 latency of
//! [`wicked_estate_retrieve::suggest::suggest`] on a perturbation probe set.
//!
//! The corpus is OFFLINE and deterministic: [`CORPUS_SYMBOLS`] code-shaped symbols (PascalCase
//! classes with camelCase methods, snake_case and camelCase free functions) generated from a
//! fixed vocabulary into an in-memory `SqliteStore` — the default store, so the `name_prefix`
//! index path is what is measured. (The frozen baseline corpus is cloned from the network, which
//! the default `gate` must not need.) Each probe perturbs ONE real name the way an agent misses:
//!
//! * **case** — the name with its letter case changed (`createorder`, `CREATEORDER`);
//! * **qualified** — `Class.method` / `Class::method` for a method;
//! * **edit1** / **edit2** — one or two random edits (substitute, delete, insert, transpose).
//!
//! A probe HITS when the perturbed symbol's id is in the top 3 suggestions. Probes whose
//! perturbation happens to spell another real name exactly are dropped (they are not misses).
//! Gates: hit rate ≥ [`HIT_RATE_GATE`]; p95 ≤ [`P95_GATE_MS`] in a release build.

use anyhow::Result;
use std::time::Instant;
use wicked_estate_core::{
    Descriptor, GraphRead, GraphWrite, Language, Location, Node, NodeKind, Span, Suffix, Symbol,
    SymbolQuery,
};
use wicked_estate_retrieve::suggest::suggest;
use wicked_estate_store::SqliteStore;

/// Symbols in the synthetic corpus.
pub const CORPUS_SYMBOLS: usize = 10_000;
/// Probes per run.
pub const PROBES: usize = 400;
/// Gate: the intended symbol is in the top 3 at least this often (#358).
pub const HIT_RATE_GATE: f64 = 0.90;
/// Gate: p95 suggestion latency in a release build, milliseconds (#358).
pub const P95_GATE_MS: f64 = 25.0;

const VERBS: &[&str] = &[
    "get", "set", "create", "update", "delete", "find", "load", "save", "parse", "render", "build",
    "validate", "compute", "resolve", "fetch", "handle", "process", "register", "emit", "apply",
    "merge", "sync", "notify", "schedule", "cancel", "refresh", "index", "export",
];
const NOUNS: &[&str] = &[
    "order", "invoice", "customer", "account", "payment", "user", "session", "token", "config",
    "report", "event", "message", "queue", "cache", "record", "ledger", "policy", "rule", "graph",
    "node", "edge", "symbol", "module", "package", "request", "response", "handler", "service",
    "client", "worker", "batch", "item", "line", "price", "address", "profile",
];
const SUFFIXES: &[&str] = &[
    "", "s", "ById", "ByName", "List", "Map", "Async", "Internal", "Impl", "V2",
];

/// A tiny deterministic PRNG (SplitMix64) — no `rand` dependency, same sequence everywhere.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a>(&mut self, xs: &'a [&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }
}

fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
        None => String::new(),
    }
}

/// One corpus symbol: its node and, for a method, its class (the qualified-probe prefix).
struct Entry {
    node: Node,
    class: Option<String>,
}

fn corpus(n: usize) -> Vec<Entry> {
    let mut rng = Rng(0x5EED_0358);
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        let verb = rng.pick(VERBS);
        let noun = rng.pick(NOUNS);
        let noun2 = rng.pick(NOUNS);
        let suffix = rng.pick(SUFFIXES);
        let module = format!("{}_{}", rng.pick(NOUNS), rng.below(40));
        let file = format!("src/{module}.ts");
        let (name, class, descriptors) = match rng.below(3) {
            // A method on a class.
            0 => {
                let class = format!("{}{}", upper_first(noun), upper_first(noun2));
                let name = format!("{verb}{}{suffix}", upper_first(noun2));
                let d = vec![
                    Descriptor::new(module.clone(), Suffix::Namespace),
                    Descriptor::new(class.clone(), Suffix::Type),
                    Descriptor::method(name.clone(), None),
                ];
                (name, Some(class), d)
            }
            // A snake_case free function.
            1 => {
                let name = format!("{verb}_{noun}_{noun2}");
                let d = vec![
                    Descriptor::new(module.clone(), Suffix::Namespace),
                    Descriptor::new(name.clone(), Suffix::Term),
                ];
                (name, None, d)
            }
            // A camelCase free function.
            _ => {
                let name = format!("{verb}{}{}{suffix}", upper_first(noun), upper_first(noun2));
                let d = vec![
                    Descriptor::new(module.clone(), Suffix::Namespace),
                    Descriptor::new(name.clone(), Suffix::Term),
                ];
                (name, None, d)
            }
        };
        let id = Symbol::global("scip-ts", None, descriptors).id();
        if !seen.insert(id.0.clone()) {
            continue;
        }
        let kind = if class.is_some() {
            NodeKind::Method
        } else {
            NodeKind::Function
        };
        let node = Node::new(
            id,
            kind,
            name,
            Language::new("typescript"),
            Location::new(file, Span::ZERO),
        );
        out.push(Entry { node, class });
    }
    out
}

const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyz";

fn edit(rng: &mut Rng, s: &str) -> String {
    let mut c: Vec<char> = s.chars().collect();
    let i = rng.below(c.len().max(1));
    let letter = LETTERS[rng.below(LETTERS.len())] as char;
    match rng.below(4) {
        0 if !c.is_empty() => c[i] = letter,
        1 if c.len() > 2 => {
            c.remove(i);
        }
        2 => c.insert(i, letter),
        _ if c.len() > 1 => {
            let j = if i + 1 < c.len() { i + 1 } else { i - 1 };
            c.swap(i, j);
        }
        _ => c.push(letter),
    }
    c.into_iter().collect()
}

/// The perturbation family of one probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Perturbation {
    Case,
    Qualified,
    Edit1,
    Edit2,
}

/// One bench run's result.
#[derive(Debug, Clone)]
pub struct SuggestReport {
    pub corpus_symbols: usize,
    pub probes: usize,
    pub hits: usize,
    pub hit_rate: f64,
    /// Per family: (probes, hits).
    pub by_family: Vec<(Perturbation, usize, usize)>,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

/// Build the corpus, run [`PROBES`] perturbation probes, and measure.
pub fn run_name_suggest_bench(corpus_symbols: usize, probes: usize) -> Result<SuggestReport> {
    let entries = corpus(corpus_symbols);
    let mut store = SqliteStore::in_memory().map_err(|e| anyhow::anyhow!("{e}"))?;
    store.begin_batch().map_err(|e| anyhow::anyhow!("{e}"))?;
    let nodes: Vec<Node> = entries.iter().map(|e| e.node.clone()).collect();
    store
        .upsert_nodes(&nodes)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    store.commit_batch().map_err(|e| anyhow::anyhow!("{e}"))?;

    let exists = |probe: &str| -> bool {
        store
            .find_symbols(&SymbolQuery {
                exact_name: Some(probe.to_string()),
                limit: Some(1),
                ..Default::default()
            })
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    };

    let mut rng = Rng(0xB0B_0358);
    let mut lat_ms: Vec<f64> = Vec::with_capacity(probes);
    let mut by_family: std::collections::HashMap<Perturbation, (usize, usize)> =
        std::collections::HashMap::new();
    let (mut done, mut hits) = (0usize, 0usize);
    let mut attempts = 0usize;
    while done < probes && attempts < probes * 20 {
        attempts += 1;
        let e = &entries[rng.below(entries.len())];
        let name = e.node.name.as_str();
        let family = match rng.below(4) {
            0 => Perturbation::Case,
            1 if e.class.is_some() => Perturbation::Qualified,
            1 | 2 => Perturbation::Edit1,
            _ => Perturbation::Edit2,
        };
        let probe = match family {
            Perturbation::Case => {
                if rng.below(2) == 0 {
                    name.to_ascii_lowercase()
                } else {
                    name.to_ascii_uppercase()
                }
            }
            Perturbation::Qualified => {
                let sep = [".", "::", "#"][rng.below(3)];
                let class = e.class.as_deref().unwrap_or_default();
                format!("{}{sep}{name}", class.to_ascii_lowercase())
            }
            Perturbation::Edit1 => edit(&mut rng, name),
            Perturbation::Edit2 => {
                let once = edit(&mut rng, name);
                edit(&mut rng, &once)
            }
        };
        // A perturbation that spells a real name (or the original) is not a miss.
        if probe == name || exists(&probe) {
            continue;
        }
        let t = Instant::now();
        let (found, _) = suggest(&store, &probe, 5).map_err(|e| anyhow::anyhow!("{e}"))?;
        lat_ms.push(t.elapsed().as_secs_f64() * 1_000.0);
        let hit = found.iter().take(3).any(|s| s.node.symbol == e.node.symbol);
        let slot = by_family.entry(family).or_insert((0, 0));
        slot.0 += 1;
        if hit {
            slot.1 += 1;
            hits += 1;
        }
        done += 1;
    }
    lat_ms.sort_by(|a, b| a.total_cmp(b));
    let pct = |p: f64| -> f64 {
        if lat_ms.is_empty() {
            return 0.0;
        }
        let i = ((p * lat_ms.len() as f64).ceil() as usize).clamp(1, lat_ms.len()) - 1;
        lat_ms[i]
    };
    let mut by_family: Vec<(Perturbation, usize, usize)> =
        by_family.into_iter().map(|(k, (n, h))| (k, n, h)).collect();
    by_family.sort_by_key(|(k, _, _)| format!("{k:?}"));
    Ok(SuggestReport {
        corpus_symbols: entries.len(),
        probes: done,
        hits,
        hit_rate: hits as f64 / done.max(1) as f64,
        by_family,
        p50_ms: pct(0.50),
        p95_ms: pct(0.95),
        max_ms: lat_ms.last().copied().unwrap_or(0.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The #358 gates. The hit-rate gate holds in every build. The latency gate is the release
    /// number the issue specifies; an unoptimized test build is ~10x slower, so there it is held
    /// to 10x the release gate — a regression tripwire, not the claim.
    #[test]
    fn name_suggest_meets_the_358_gates() {
        let r = run_name_suggest_bench(CORPUS_SYMBOLS, PROBES).expect("bench runs");
        eprintln!(
            "name-suggest bench: {} symbols, {} probes, top-3 {:.1}% ({}/{}), p50 {:.2} ms, \
             p95 {:.2} ms, max {:.2} ms, by family {:?}",
            r.corpus_symbols,
            r.probes,
            r.hit_rate * 100.0,
            r.hits,
            r.probes,
            r.p50_ms,
            r.p95_ms,
            r.max_ms,
            r.by_family
        );
        assert_eq!(r.probes, PROBES, "the probe set must be full");
        assert!(
            r.hit_rate >= HIT_RATE_GATE,
            "top-3 hit rate {:.3} below the {HIT_RATE_GATE} gate: {:?}",
            r.hit_rate,
            r.by_family
        );
        let ceiling = if cfg!(debug_assertions) {
            P95_GATE_MS * 10.0
        } else {
            P95_GATE_MS
        };
        assert!(
            r.p95_ms <= ceiling,
            "p95 {:.2} ms above the {ceiling} ms ceiling",
            r.p95_ms
        );
    }

    #[test]
    fn corpus_and_probes_are_deterministic() {
        let a = corpus(500);
        let b = corpus(500);
        assert_eq!(a.len(), 500);
        assert!(
            a.iter()
                .zip(&b)
                .all(|(x, y)| x.node.symbol == y.node.symbol)
        );
    }
}
