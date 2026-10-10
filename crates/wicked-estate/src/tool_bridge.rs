//! RetrievalTool → CLI bridge: expose a [`RetrievalTool`] as a subcommand without a bespoke
//! dispatch arm.
//!
//! A bridged command is one row of [`COMMANDS`]: the tool, its operand, and a typed flag spec.
//! The bridge owns everything else — parsing, coercion, store opening, invocation, rendering —
//! and knows nothing about which tool it drives. The pattern was first hand-rolled in the
//! `semantic` arm of `main.rs`; that arm is NOT a row here (its operand is a free-text query and
//! it needs a concrete vector store) and stays bespoke until the bridge grows both.
//!
//! **Strict flags.** Bridged commands parse their own argv against [`FlagSpec`]; the bespoke
//! arms are checked against `cli_flags::COMMANDS` instead. Both close the accept-and-ignore
//! defect class of #197 / #206 / W8.6: `main` once ran one shared parser that pushed unknown
//! tokens into `positional` and silently swallowed any flag another command owned (`--top`,
//! `--file`, …). Here an unknown or repeated flag, or a value of the wrong type, is a non-zero
//! exit.
//! Values are coerced through the declared [`FlagType`] rather than guessed: a tool handed
//! `"4"` where it reads a number falls back to its default and returns a plausible wrong answer.
//!
//! **Defaults and ceilings stay in the tool.** The bridge sends only the flags the caller gave
//! and never clamps. Every RetrievalTool that lowers a caller value to its ceiling reports it as
//! a `CLAMPED:` diagnostic (`Lineage` excepted: WAVE-PLAN W8.5). Floor clamps (`0` → `1`) are
//! not reported.
//!
//! **Output contract.** `--json`: `content` — the same document the MCP tool returns, no extra
//! envelope — as exactly one JSON document on stdout, `diagnostics` on stderr one per line.
//! Default: `content` rendered for humans, then `diagnostics`, all on stdout.

use anyhow::Result;
use serde_json::{Map, Value};
use std::io::Write;
use wicked_estate_core::RetrievalTool;
use wicked_estate_core::observability::KeyValue;

/// How a flag's value is coerced into the request JSON. A type exists only once a bridged
/// command uses it (CLAUDE.md §5) — add `Bool` with its first consumer.
#[derive(Debug, Clone, Copy)]
pub enum FlagType {
    /// Non-negative integer → JSON number.
    U64,
    /// Free-form string → JSON string, passed through verbatim. An empty value is rejected:
    /// tools read `""` as an absent facet, so `--language ""` would silently widen the query.
    Str,
    /// Comma-separated → JSON array of strings; empty items are dropped.
    List,
    /// Comma-separated symbol names or `SymbolId`s → JSON array of `SymbolId`s. Each item is
    /// resolved like the operand (see [`OperandSpec`]); an unmatched or ambiguous item is an
    /// error, because a tool that ignores an unknown id answers a different question.
    /// Items are split on `,` and trimmed, so an item cannot contain a comma or edge whitespace
    /// (native `SymbolId` disambiguators are arity/hash, so none do).
    SymbolList,
    /// One of a closed set of strings. A tool that falls back to a default on an unrecognized
    /// value would otherwise answer a different question than the one asked.
    OneOf(&'static [&'static str]),
}

impl FlagType {
    fn placeholder(self) -> String {
        match self {
            FlagType::U64 => "N".into(),
            FlagType::Str => "S".into(),
            FlagType::List => "a,b".into(),
            FlagType::SymbolList => "s1,s2".into(),
            FlagType::OneOf(vals) => vals.join("|"),
        }
    }
}

/// Checks one value; `Err` carries the reason shown to the caller.
pub type Validator = fn(&str) -> std::result::Result<(), String>;

/// One `--flag` of a bridged command.
#[derive(Debug)]
pub struct FlagSpec {
    /// Without the leading `--`.
    pub flag: &'static str,
    /// The request key the value lands under.
    pub key: &'static str,
    pub ty: FlagType,
    /// Optional per-item check (each list item), for values whose valid set is owned by a type
    /// rather than spelled out here.
    pub validate: Option<Validator>,
    pub help: &'static str,
}

/// The single positional operand: an exact symbol name or a `SymbolId`, resolved to a
/// `SymbolId` before invocation by [`wicked_estate_core::resolve_operand`] — the same rule
/// `path` uses. Ambiguous or unmatched operands are errors, never a silent empty result.
#[derive(Debug)]
pub struct OperandSpec {
    /// Placeholder shown in usage, e.g. `<symbol>`.
    pub name: &'static str,
    pub key: &'static str,
}

/// Renders a tool's `content` for humans, one line per entry. Optional per row: the generic
/// indented rendering is the default.
pub type Renderer = fn(&Value) -> Vec<String>;

/// A RetrievalTool exposed as a subcommand.
pub struct BridgedCommand {
    pub name: &'static str,
    /// Other names for the same row, e.g. `hotspots` for `rank`.
    pub aliases: &'static [&'static str],
    pub tool: &'static dyn RetrievalTool,
    /// `None` for a command that takes no positional argument; any operand is then an error.
    pub operand: Option<OperandSpec>,
    pub flags: &'static [FlagSpec],
    pub render: Option<Renderer>,
}

fn edge_kind(s: &str) -> std::result::Result<(), String> {
    serde_json::from_value::<wicked_estate_core::EdgeKind>(Value::String(s.to_string()))
        .map(|_| ())
        .map_err(|_| format!("unknown edge kind {s:?} (snake_case, e.g. calls,imports,references)"))
}

/// `rank`'s human output: the `top N symbols by PageRank:` listing the bespoke arm printed.
fn render_hotspots(content: &Value) -> Vec<String> {
    let rows = content["hotspots"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut lines = vec![format!("top {} symbols by PageRank:", rows.len())];
    for r in rows {
        let text = |k: &str| r[k].as_str().unwrap_or("?").to_string();
        lines.push(format!(
            "  {:.4}  {} {} ({}:{})",
            r["score"].as_f64().unwrap_or(0.0),
            text("kind"),
            text("name"),
            text("file"),
            r["line_1based"]
        ));
    }
    lines
}

/// Every bridged command. Adding one is a row here — no new dispatch arm.
pub const COMMANDS: &[BridgedCommand] = &[
    BridgedCommand {
        name: "traverse",
        aliases: &[],
        tool: &wicked_estate_retrieve::TraverseGraph,
        operand: Some(OperandSpec {
            name: "<symbol>",
            key: "symbol",
        }),
        render: None,
        flags: &[
            FlagSpec {
                flag: "depth",
                key: "depth",
                ty: FlagType::U64,
                validate: None,
                help: "max hops; the tool clamps to its ceiling and reports the clamp",
            },
            FlagSpec {
                flag: "direction",
                key: "direction",
                ty: FlagType::OneOf(&["dependencies", "dependents", "both"]),
                validate: None,
                help: "dependencies = what <symbol> uses; dependents = what uses <symbol>",
            },
            FlagSpec {
                flag: "edge-kinds",
                key: "edge_kinds",
                ty: FlagType::List,
                validate: Some(edge_kind),
                help: "only follow these edge kinds (default: all)",
            },
            FlagSpec {
                flag: "max-nodes",
                key: "max_nodes",
                ty: FlagType::U64,
                validate: None,
                help: "node cap; the tool clamps to its ceiling and reports the clamp",
            },
        ],
    },
    BridgedCommand {
        name: "rank",
        aliases: &["hotspots"],
        tool: &wicked_estate_retrieve::RankHotspots,
        operand: None,
        render: Some(render_hotspots),
        flags: &[
            FlagSpec {
                flag: "limit",
                key: "limit",
                ty: FlagType::U64,
                validate: None,
                help: "how many symbols; the tool clamps to its ceiling and reports the clamp",
            },
            FlagSpec {
                flag: "seeds",
                key: "seeds",
                ty: FlagType::SymbolList,
                validate: None,
                help: "personalize toward these symbols (names or SymbolIds; no commas)",
            },
        ],
    },
    BridgedCommand {
        name: "rules-inventory",
        aliases: &[],
        tool: &wicked_estate_retrieve::RulesInventory,
        operand: None,
        render: None,
        flags: &[],
    },
    BridgedCommand {
        name: "rules-recall",
        aliases: &[],
        tool: &wicked_estate_retrieve::RulesRecall,
        operand: None,
        render: None,
        flags: &[
            FlagSpec {
                flag: "severity",
                key: "severity",
                ty: FlagType::Str,
                validate: None,
                help: "exact: info|warn|error|critical",
            },
            FlagSpec {
                flag: "rule-type",
                key: "rule_type",
                ty: FlagType::Str,
                validate: None,
                help: "exact: pattern|policy",
            },
            FlagSpec {
                flag: "steering-type",
                key: "steering_type",
                ty: FlagType::Str,
                validate: None,
                help: "exact steering page: architecture|development|security|testing|operations|compliance|design-ux (a pre-steering rule is architecture)",
            },
            FlagSpec {
                flag: "language",
                key: "language",
                ty: FlagType::Str,
                validate: None,
                help: "wildcard: also returns rules that name no language",
            },
            FlagSpec {
                flag: "layer",
                key: "layer",
                ty: FlagType::Str,
                validate: None,
                help: "wildcard: also returns rules that name no layer",
            },
            FlagSpec {
                flag: "framework",
                key: "framework",
                ty: FlagType::Str,
                validate: None,
                help: "wildcard: also returns rules that name no framework",
            },
            FlagSpec {
                flag: "scope",
                key: "scope",
                ty: FlagType::Str,
                validate: None,
                help: "only rules under this scope subtree (path prefix, e.g. wiki:architecture)",
            },
            FlagSpec {
                flag: "projects",
                key: "projects",
                ty: FlagType::List,
                validate: None,
                help: "also return rules scoped to these projects (omitted: global rules only)",
            },
            FlagSpec {
                flag: "limit",
                key: "limit",
                ty: FlagType::U64,
                validate: None,
                help: "how many rules; the tool clamps to its ceiling and reports the clamp",
            },
        ],
    },
];

pub fn lookup(name: &str) -> Option<&'static BridgedCommand> {
    COMMANDS
        .iter()
        .find(|c| c.name == name || c.aliases.contains(&name))
}

impl BridgedCommand {
    /// `traverse <symbol> [--depth N] … [--json] [--db ...]`
    pub fn usage_line(&self) -> String {
        let mut s = self.name.to_string();
        if let Some(op) = &self.operand {
            s.push_str(&format!(" {}", op.name));
        }
        for f in self.flags {
            s.push_str(&format!(" [--{} {}]", f.flag, f.ty.placeholder()));
        }
        s.push_str(" [--json] [--db ...]");
        s
    }

    pub fn help_text(&self) -> String {
        let mut s = format!(
            "usage: wicked-estate {}\n\n{}\n\n",
            self.usage_line(),
            self.tool.description()
        );
        if !self.aliases.is_empty() {
            s.push_str(&format!("  also: {}\n\n", self.aliases.join(", ")));
        }
        if let Some(op) = &self.operand {
            s.push_str(&format!("  {:<28}exact symbol name or SymbolId\n", op.name));
        }
        for f in self.flags {
            let head = format!("--{} {}", f.flag, f.ty.placeholder());
            if head.len() < 28 {
                s.push_str(&format!("  {head:<28}{}\n", f.help));
            } else {
                // Too wide for the column: help on its own line rather than run together.
                s.push_str(&format!("  {head}\n  {:<28}{}\n", "", f.help));
            }
        }
        s.push_str(&format!(
            "  {:<28}content as one JSON document on stdout; diagnostics on stderr\n",
            "--json"
        ));
        s.push_str(&format!(
            "  {:<28}graph database (default: resolved store)\n",
            "--db S"
        ));
        s
    }
}

/// A parsed command line. `request` holds the operand unresolved; [`run`] resolves it.
#[derive(Debug, PartialEq)]
pub struct Invocation {
    pub request: Value,
    pub json: bool,
    pub db: Option<String>,
    pub help: bool,
}

/// Parse argv (after the command name) against the command's spec. Pure — no store access.
pub fn parse(cmd: &BridgedCommand, args: &[String]) -> std::result::Result<Invocation, String> {
    let mut req = Map::new();
    let mut json = false;
    let mut db: Option<String> = None;
    let mut help = false;
    let mut operands: Vec<&str> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    let mut it = args.iter();
    let mut operands_only = false;

    while let Some(arg) = it.next() {
        if operands_only || !arg.starts_with('-') {
            operands.push(arg);
            continue;
        }
        if arg == "--" {
            operands_only = true;
            continue;
        }
        if arg == "-h" || arg == "--help" {
            help = true;
            continue;
        }
        let Some(body) = arg.strip_prefix("--") else {
            return Err(format!("unknown flag {arg:?}"));
        };
        let (name, inline) = match body.split_once('=') {
            Some((n, v)) => (n, Some(v)),
            None => (body, None),
        };
        // Repeats are rejected rather than last-wins: `--depth 4 --depth 8` is a caller
        // mistake, and silently picking one is the same guess this bridge exists to refuse.
        let canonical: &str = match name {
            "json" | "db" => name,
            _ => match cmd.flags.iter().find(|f| f.flag == name) {
                Some(f) => f.flag,
                None => return Err(format!("unknown flag \"--{name}\"")),
            },
        };
        if seen.contains(&canonical) {
            return Err(format!("--{name} given more than once"));
        }
        seen.push(canonical);

        let mut value = |what: &str| -> std::result::Result<String, String> {
            match inline {
                // An empty value is a missing value: `--db=` opened a private temp database.
                Some("") => Err(format!("--{name} requires {what}, got an empty value")),
                Some(v) => Ok(v.to_string()),
                None => match it.next() {
                    Some(v) if v.is_empty() => {
                        Err(format!("--{name} requires {what}, got an empty value"))
                    }
                    Some(v) if !v.starts_with("--") => Ok(v.clone()),
                    _ => Err(format!("--{name} requires {what}")),
                },
            }
        };

        match name {
            "json" => {
                if inline.is_some() {
                    return Err("--json takes no value".into());
                }
                json = true;
            }
            "db" => db = Some(value("a database spec")?),
            _ => {
                let f = cmd
                    .flags
                    .iter()
                    .find(|f| f.flag == name)
                    .expect("checked above");
                let v = match f.ty {
                    FlagType::U64 => {
                        let v = value("a number")?;
                        let n: u64 = v.parse().map_err(|_| {
                            format!("--{name} expects a non-negative integer, got {v:?}")
                        })?;
                        Value::from(n)
                    }
                    FlagType::Str => {
                        let v = value("a value")?;
                        check(f, &v)?;
                        Value::String(v)
                    }
                    FlagType::OneOf(allowed) => {
                        let v = value(&format!("one of {}", allowed.join("|")))?;
                        if !allowed.contains(&v.as_str()) {
                            return Err(format!(
                                "--{name} expects one of {}, got {v:?}",
                                allowed.join("|")
                            ));
                        }
                        Value::String(v)
                    }
                    FlagType::List | FlagType::SymbolList => {
                        let v = value("a comma-separated list")?;
                        let mut items = Vec::new();
                        for item in v.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                            check(f, item)?;
                            items.push(Value::String(item.to_string()));
                        }
                        // Zero items would reach the tool as `[]`, which a filter reads as "no
                        // filter" — `--edge-kinds ,` would silently widen to every kind.
                        if items.is_empty() {
                            return Err(format!("--{name} requires at least one item"));
                        }
                        Value::Array(items)
                    }
                };
                req.insert(f.key.to_string(), v);
            }
        }
    }

    if !help {
        match (&cmd.operand, operands.as_slice()) {
            (None, []) => {}
            (None, any) => {
                return Err(format!("takes no positional argument, got {any:?}"));
            }
            (Some(op), [one]) => {
                req.insert(op.key.to_string(), Value::String(one.to_string()));
            }
            (Some(op), []) => return Err(format!("missing {}", op.name)),
            (Some(op), many) => {
                return Err(format!(
                    "expected exactly one {}, got {}: {many:?}",
                    op.name,
                    many.len()
                ));
            }
        }
    }

    Ok(Invocation {
        request: Value::Object(req),
        json,
        db,
        help,
    })
}

fn check(f: &FlagSpec, v: &str) -> std::result::Result<(), String> {
    match f.validate {
        Some(validate) => validate(v).map_err(|e| format!("--{}: {e}", f.flag)),
        None => Ok(()),
    }
}

/// Emits one CLI span: `(name, attributes, start_ns, end_ns)`. `main` owns the sink.
pub type SpanEmitter<'a> = &'a dyn Fn(&str, Vec<KeyValue>, u64, u64);

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

/// Parse, open the store, resolve the operand, invoke, render. `default_db` is the spec `main`
/// resolved from the runtime profile; `--db` overrides it. Emits `wicked_estate.<command>`, the
/// span every bespoke read arm emits.
pub fn run(
    cmd: &BridgedCommand,
    args: &[String],
    default_db: String,
    span: SpanEmitter<'_>,
) -> Result<()> {
    let inv = parse(cmd, args)
        .map_err(|e| anyhow::anyhow!("usage: wicked-estate {}\n{e}", cmd.usage_line()))?;
    if inv.help {
        print!("{}", cmd.help_text());
        return Ok(());
    }
    let db = inv.db.unwrap_or(default_db);
    // A read-only command never creates the graph it reads (see `require_existing_graph`).
    super::require_existing_graph(&db, cmd.name)?;
    let store = wicked_estate_store::open_store_ext(&db).map_err(super::to_any)?;

    let mut request = inv.request;
    let resolve = |given: &str| -> Result<Value> {
        let ids = wicked_estate_core::resolve_operand(&*store, given).map_err(super::to_any)?;
        match ids.as_slice() {
            [id] => Ok(Value::String(id.0.clone())),
            [] => anyhow::bail!(
                "{}: no symbol named {given:?} and no node with that id in {db}",
                cmd.name
            ),
            many => anyhow::bail!(
                "{}: {given:?} names {} symbols — pass one SymbolId:\n  {}",
                cmd.name,
                many.len(),
                many.iter()
                    .map(|s| s.0.as_str())
                    .collect::<Vec<_>>()
                    .join("\n  ")
            ),
        }
    };
    if let Some(op) = &cmd.operand {
        let given = request[op.key].as_str().unwrap_or_default().to_string();
        request[op.key] = resolve(&given)?;
    }
    for f in cmd
        .flags
        .iter()
        .filter(|f| matches!(f.ty, FlagType::SymbolList))
    {
        if let Some(Value::Array(items)) = request.get(f.key).cloned() {
            let mut ids = Vec::with_capacity(items.len());
            for item in &items {
                ids.push(resolve(item.as_str().unwrap_or_default())?);
            }
            request[f.key] = Value::Array(ids);
        }
    }

    let t_start = now_ns();
    let result = cmd.tool.invoke(&*store, &request).map_err(super::to_any)?;
    let t_end = now_ns();
    // Real freshness, computed here as the MCP server does. The tool's own staleness line is a
    // placeholder addressed to the transport — this is that transport, so it is REPLACED, and
    // R5 still holds: freshness is always stated, as "N commits", "0 commits" or "unknown".
    let mut diagnostics = result.diagnostics;
    diagnostics.retain(|d| d != wicked_estate_retrieve::STALENESS_PLACEHOLDER);
    diagnostics.extend(super::staleness_report(store.as_ref(), &db).statements(&db));
    super::maybe_warn_version_mismatch(store.as_ref(), &db);

    // The span counts the diagnostics the caller actually receives, not the tool's raw list.
    let mut attrs = vec![
        KeyValue::str("tool.name", cmd.tool.name()),
        KeyValue::int("diagnostics.count", diagnostics.len() as i64),
    ];
    if let Some(op) = &cmd.operand {
        attrs.push(KeyValue::str(
            "symbol.id",
            request[op.key].as_str().unwrap_or_default(),
        ));
    }
    span(
        &format!("wicked_estate.{}", cmd.name),
        attrs,
        t_start,
        t_end,
    );

    let mut out = std::io::stdout().lock();
    if inv.json {
        writeln!(out, "{}", serde_json::to_string(&result.content)?)?;
        let mut err = std::io::stderr().lock();
        for d in &diagnostics {
            writeln!(err, "{d}")?;
        }
    } else {
        let lines = match cmd.render {
            Some(row_render) => row_render(&result.content),
            None => {
                let mut lines = Vec::new();
                render(&result.content, 0, &mut lines);
                lines
            }
        };
        for l in &lines {
            writeln!(out, "{l}")?;
        }
        if !diagnostics.is_empty() {
            writeln!(out)?;
            for d in &diagnostics {
                writeln!(out, "{d}")?;
            }
        }
    }
    Ok(())
}

fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::Null => Some("null".into()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        Value::Array(a) if a.is_empty() => Some("[]".into()),
        Value::Object(o) if o.is_empty() => Some("{}".into()),
        _ => None,
    }
}

/// Tool-agnostic indented rendering: `key: value` for objects, `- item` for arrays.
fn render(v: &Value, indent: usize, lines: &mut Vec<String>) {
    let pad = "  ".repeat(indent);
    match v {
        Value::Object(o) => {
            for (k, val) in o {
                match scalar(val) {
                    Some(s) => lines.push(format!("{pad}{k}: {s}")),
                    None => {
                        lines.push(format!("{pad}{k}:"));
                        render(val, indent + 1, lines);
                    }
                }
            }
        }
        Value::Array(a) => {
            for item in a {
                match scalar(item) {
                    Some(s) => lines.push(format!("{pad}- {s}")),
                    None => {
                        let start = lines.len();
                        render(item, indent + 1, lines);
                        // Hang the first line of the item off the dash.
                        if let Some(first) = lines.get_mut(start) {
                            let body = first.trim_start().to_string();
                            *first = format!("{pad}- {body}");
                        }
                    }
                }
            }
        }
        other => lines.push(format!("{pad}{}", scalar(other).unwrap_or_default())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn traverse() -> &'static BridgedCommand {
        lookup("traverse").unwrap()
    }

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|a| a.to_string()).collect()
    }

    fn req(s: &[&str]) -> Value {
        parse(traverse(), &args(s)).unwrap().request
    }

    fn err(s: &[&str]) -> String {
        parse(traverse(), &args(s)).unwrap_err()
    }

    #[test]
    fn only_given_flags_reach_the_request() {
        // Defaults are the tool's; the bridge must not restate them.
        assert_eq!(req(&["f"]), json!({"symbol": "f"}));
    }

    #[test]
    fn u64_coerces_to_a_json_number_not_a_string() {
        assert_eq!(req(&["f", "--depth", "4"])["depth"], json!(4));
        assert_eq!(req(&["f", "--max-nodes=900"])["max_nodes"], json!(900));
        assert!(err(&["f", "--depth", "four"]).contains("non-negative integer"));
        assert!(err(&["f", "--depth", "-1"]).contains("non-negative integer"));
        assert!(err(&["f", "--depth"]).contains("--depth requires"));
    }

    #[test]
    fn list_coerces_to_an_array_and_validates_each_item() {
        assert_eq!(
            req(&["f", "--edge-kinds", "calls, imports,"])["edge_kinds"],
            json!(["calls", "imports"])
        );
        // A typo must not degrade to "all kinds" (the tool's meaning of an empty filter).
        assert!(err(&["f", "--edge-kinds", "calls,cals"]).contains("unknown edge kind \"cals\""));
        // Nor may an empty list: `[]` is the tool's "every kind".
        for empty in [
            &["f", "--edge-kinds", ","][..],
            &["f", "--edge-kinds", ""][..],
        ] {
            let e = err(empty);
            assert!(
                e.contains("at least one item") || e.contains("empty value"),
                "{e}"
            );
        }
        assert!(err(&["f", "--edge-kinds="]).contains("empty value"));
    }

    #[test]
    fn one_of_rejects_values_outside_the_set() {
        assert_eq!(
            req(&["f", "--direction", "dependents"])["direction"],
            json!("dependents")
        );
        assert!(err(&["f", "--direction", "sideways"]).contains("expects one of"));
    }

    #[test]
    fn unknown_repeated_and_foreign_flags_are_rejected() {
        assert_eq!(
            err(&["f", "--bogus-flag", "x"]),
            "unknown flag \"--bogus-flag\""
        );
        // Owned by other commands (bespoke arms) — not by this one, so rejected here.
        assert!(err(&["f", "--top", "5"]).starts_with("unknown flag"));
        assert!(err(&["f", "--repo", "a"]).starts_with("unknown flag"));
        assert!(err(&["f", "-x"]).starts_with("unknown flag"));
        assert!(err(&["f", "--depth", "1", "--depth", "2"]).contains("more than once"));
    }

    #[test]
    fn exactly_one_operand() {
        assert!(err(&[]).contains("missing <symbol>"));
        assert!(err(&["a", "b"]).contains("exactly one"));
        // `--` ends flags, so an operand may begin with a dash.
        assert_eq!(req(&["--", "-odd"])["symbol"], json!("-odd"));
    }

    #[test]
    fn a_command_without_an_operand_rejects_one_and_aliases_resolve() {
        let rank = lookup("rank").unwrap();
        assert!(std::ptr::eq(lookup("hotspots").unwrap(), rank));
        let p = |a: &[&str]| parse(rank, &args(a));
        assert_eq!(p(&[]).unwrap().request, json!({}));
        assert!(
            p(&["f"])
                .unwrap_err()
                .contains("takes no positional argument")
        );
        // Symbol lists parse like lists; resolution against the store happens in `run`.
        assert_eq!(
            p(&["--seeds", "a, b"]).unwrap().request["seeds"],
            json!(["a", "b"])
        );
        assert!(
            p(&["--seeds", ","])
                .unwrap_err()
                .contains("at least one item")
        );
        assert!(!rank.usage_line().contains('<'), "{}", rank.usage_line());
    }

    #[test]
    fn str_passes_the_value_through_and_rejects_an_empty_or_missing_one() {
        let recall = lookup("rules-recall").unwrap();
        let p = |a: &[&str]| parse(recall, &args(a));
        assert_eq!(
            p(&["--severity", "error", "--language=python"])
                .unwrap()
                .request,
            json!({"severity": "error", "language": "python"})
        );
        // A numeric-looking value stays a string: the tool reads facets with `as_str`.
        assert_eq!(p(&["--layer", "7"]).unwrap().request["layer"], json!("7"));
        // `""` is the tool's "no facet" — it would silently widen the recall.
        assert!(p(&["--language", ""]).unwrap_err().contains("empty value"));
        assert!(p(&["--language="]).unwrap_err().contains("empty value"));
        assert!(p(&["--scope"]).unwrap_err().contains("--scope requires"));
        assert!(
            p(&["--scope", "--json"])
                .unwrap_err()
                .contains("--scope requires")
        );
        assert!(
            p(&["--layer", "a", "--layer", "b"])
                .unwrap_err()
                .contains("more than once")
        );
        assert!(
            p(&["x"])
                .unwrap_err()
                .contains("takes no positional argument")
        );
    }

    #[test]
    fn json_db_and_help_are_common_flags() {
        let inv = parse(traverse(), &args(&["f", "--json", "--db", "x.db"])).unwrap();
        assert!(inv.json);
        assert_eq!(inv.db.as_deref(), Some("x.db"));
        assert!(err(&["f", "--json=1"]).contains("no value"));
        // `--db=` would open a private temporary SQLite database and report "no symbol".
        assert!(err(&["f", "--db="]).contains("empty value"));
        assert!(err(&["f", "--db", ""]).contains("empty value"));
        // Help needs no operand.
        assert!(parse(traverse(), &args(&["--help"])).unwrap().help);
    }

    #[test]
    fn help_is_generated_from_the_spec_and_the_tool_description() {
        let h = traverse().help_text();
        assert!(h.contains(traverse().tool.description()));
        for f in traverse().flags {
            assert!(
                h.contains(&format!("--{}", f.flag)),
                "{} missing from help",
                f.flag
            );
        }
    }

    #[test]
    fn render_is_indented_key_value() {
        let mut lines = Vec::new();
        render(
            &json!({"a": 1, "b": [{"x": "y", "z": []}], "c": {"d": true}}),
            0,
            &mut lines,
        );
        assert_eq!(
            lines,
            vec!["a: 1", "b:", "  - x: y", "    z: []", "c:", "  d: true"]
        );
    }
}
