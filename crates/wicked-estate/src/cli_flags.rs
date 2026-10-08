//! The argv contract for `main`'s bespoke dispatch arms (#197, #206, W8.6).
//!
//! `main` used to run one shared parser over every command: it recognised the union of every
//! arm's flags, coerced values with `parse().unwrap_or(default)`, let a repeated flag win last,
//! and pushed everything else into a `positional` list each arm read as much of as it liked. A
//! flag the arm never read, a malformed value, a surplus operand or an accidental repeat was
//! accepted and ignored: the command ran on its defaults and exited 0 with a plausible wrong
//! answer (`nodes --bogus-flag zzz` printed 61,182 rows; `--top abc` meant 20; `stats foo`
//! ignored `foo`).
//!
//! [`COMMANDS`] is now the single pre-I/O contract for every bespoke arm: the flags it owns,
//! each flag's value type ([`Ty`]), inline spelling and repeat policy, the command's typed
//! operands, and the [`Rule`]s between them — a required flag, or a flag that means nothing
//! without (or alongside) another and would otherwise be silently ignored. [`parse`] checks argv
//! against the row and returns the coerced [`Args`] the arm consumes — there is no second,
//! permissive parse. A default applies only when a flag is absent. Bridged commands (`tool_bridge`) never reach here; `lineage` and `supports` parse
//! their own argv strictly and are listed as [`Spec::SelfParsed`] so the table stays the
//! inventory of every bespoke arm.

/// How a value is checked and coerced. Every value must be non-empty.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ty {
    /// `--flag` takes no value.
    Switch,
    /// Free-form string, verbatim.
    Str,
    /// Decimal digits only (no sign), at most `max`.
    UInt { min: u64, max: u64 },
    /// Signed decimal integer (`i64`).
    Int,
    /// A finite decimal number in `min..=max`.
    Float { min: f64, max: f64 },
    /// `true|false` (and the `1|0`, `yes|no` spellings the CLI always accepted for true).
    Bool,
    /// One of a closed set.
    OneOf(&'static [&'static str]),
    /// Comma-separated; items are trimmed and none may be empty (`a,,b` is a typo, not two
    /// items).
    List,
    /// A repo label (`repo_scope::validate_label`).
    Label,
}

/// One `--flag` a command owns.
#[derive(Debug, Clone, Copy)]
pub struct Flag {
    /// As typed, without the leading `--`.
    pub name: &'static str,
    /// The value's identity: an alias (`--as`) shares its canonical flag's key (`repo`), so the
    /// two count as one flag for the repeat check and the arm reads one value.
    pub key: &'static str,
    pub ty: Ty,
    /// `--flag=V` is accepted as well as `--flag V`.
    pub inline: bool,
    /// Every occurrence is consumed, in order. Otherwise a second occurrence is an error.
    pub repeat: bool,
}

impl Flag {
    const fn new(name: &'static str, ty: Ty) -> Flag {
        Flag {
            name,
            key: name,
            ty,
            inline: false,
            repeat: false,
        }
    }
    const fn inline(self) -> Flag {
        Flag {
            inline: true,
            ..self
        }
    }
    const fn repeatable(self) -> Flag {
        Flag {
            repeat: true,
            ..self
        }
    }
    const fn alias_of(self, key: &'static str) -> Flag {
        Flag { key, ..self }
    }
}

/// One positional operand.
#[derive(Debug, Clone, Copy)]
pub struct Operand {
    /// Usage placeholder without brackets, e.g. `name` for `<name>`; a literal (`list`) when
    /// `ty` is a one-value [`Ty::OneOf`].
    pub name: &'static str,
    pub ty: Ty,
    pub required: bool,
}

/// A condition on the other flags given, by key.
#[derive(Debug, Clone, Copy)]
pub enum Cond {
    /// The flag is given.
    Given(&'static str),
    /// The flag is given with exactly this value.
    Is(&'static str, &'static str),
    /// The store this run opens is exactly this spec: the `--db` value, or else the default
    /// [`parse`] was handed — which the environment can set (`WICKED_ESTATE_DB`), so a rule on
    /// `--db` alone would miss it.
    Store(&'static str),
}

/// A constraint between a command's flags and operands, checked before any I/O. Each one closes
/// a path where the arm would otherwise run and silently ignore part of what was asked.
#[derive(Debug, Clone, Copy)]
pub enum Rule {
    /// The flag must be given.
    Required(&'static str),
    /// At least one of these flags must be given.
    AnyOf(&'static [&'static str]),
    /// Each flag means something only when the condition holds; otherwise the arm never reads
    /// it, so it is refused.
    Needs(&'static [&'static str], Cond),
    /// Each flag is ignored when the condition holds, so the combination is refused.
    Excludes(&'static [&'static str], Cond),
    /// The required operands may be replaced by any of these flags. `exclusive`: never both
    /// (`annotate <name> | --symbol <id>`); otherwise both may be given and the arm applies a
    /// documented precedence (`source`).
    OperandOr {
        flags: &'static [&'static str],
        exclusive: bool,
    },
}

#[derive(Debug)]
pub enum Spec {
    /// Owns exactly these flags (plus `-h`/`--help`) and operands.
    Owns {
        operands: &'static [Operand],
        flags: &'static [Flag],
        rules: &'static [Rule],
        /// Extra usage line(s), e.g. a documented range.
        note: &'static str,
    },
    /// Parses its own argv strictly. `values` are its value flags, so a `--help` in one of
    /// their value slots is a value, not a help request.
    SelfParsed { values: &'static [&'static str] },
}

#[derive(Debug)]
pub struct Command {
    pub name: &'static str,
    pub spec: Spec,
}

const fn str_flag(name: &'static str) -> Flag {
    Flag::new(name, Ty::Str)
}
const fn switch(name: &'static str) -> Flag {
    Flag::new(name, Ty::Switch)
}
/// A non-negative integer that fits this target's `usize`.
const fn count(name: &'static str) -> Flag {
    Flag::new(name, USIZE)
}
const fn non_negative(name: &'static str) -> Flag {
    Flag::new(
        name,
        Ty::Float {
            min: 0.0,
            max: f64::MAX,
        },
    )
}

const USIZE: Ty = Ty::UInt {
    min: 0,
    max: usize::MAX as u64,
};
const U64: Ty = Ty::UInt {
    min: 0,
    max: u64::MAX,
};

const IN_MEMORY: Cond = Cond::Store(":memory:");
const SEMANTIC: Cond = Cond::Is("weight", "semantic");
const BY_NAME_OR_SYMBOL: Rule = Rule::OperandOr {
    flags: &["symbol"],
    exclusive: true,
};

const DB: Flag = str_flag("db");
const JSON: Flag = switch("json");
const REPO: Flag = Flag::new("repo", Ty::Label).inline();
const AS: Flag = Flag::new("as", Ty::Label).inline().alias_of("repo");
const HISTORY: Flag = switch("history");
const KIND: Flag = str_flag("kind");
const SYMBOL: Flag = str_flag("symbol");
const TYPE: Flag = str_flag("type");

const fn req(name: &'static str) -> Operand {
    Operand {
        name,
        ty: Ty::Str,
        required: true,
    }
}
const fn opt(name: &'static str) -> Operand {
    Operand {
        name,
        ty: Ty::Str,
        required: false,
    }
}

const fn owns(operands: &'static [Operand], flags: &'static [Flag]) -> Spec {
    ruled(operands, flags, &[])
}

const fn ruled(
    operands: &'static [Operand],
    flags: &'static [Flag],
    rules: &'static [Rule],
) -> Spec {
    Spec::Owns {
        operands,
        flags,
        rules,
        note: "",
    }
}

/// Every bespoke command: its operands and the flags its arm reads.
pub const COMMANDS: &[Command] = &[
    Command {
        name: "index",
        spec: ruled(
            &[opt("path")],
            &[DB, REPO, AS, HISTORY, switch("embeddings"), switch("force")],
            // History and embeddings are skipped for an in-memory store: nothing persists.
            &[Rule::Excludes(&["history", "embeddings"], IN_MEMORY)],
        ),
    },
    Command {
        name: "scip",
        spec: owns(&[opt("root")], &[DB, REPO, AS, str_flag("scip-file")]),
    },
    Command {
        name: "tfstate",
        spec: owns(&[req("file")], &[DB]),
    },
    Command {
        name: "import-telemetry",
        spec: owns(&[req("file.json")], &[DB]),
    },
    Command {
        name: "drift",
        spec: owns(&[], &[DB]),
    },
    Command {
        name: "query",
        spec: owns(&[req("name")], &[DB, JSON]),
    },
    Command {
        name: "blast-radius",
        spec: owns(
            &[req("name")],
            &[
                DB,
                JSON,
                Flag::new(
                    "depth",
                    Ty::UInt {
                        min: 0,
                        max: wicked_estate_retrieve::BLAST_DEPTH_CEILING as u64,
                    },
                )
                .inline(),
            ],
        ),
    },
    Command {
        name: "path",
        spec: Spec::Owns {
            operands: &[req("from"), req("to")],
            flags: &[
                DB,
                JSON,
                // Above 16 clamps (documented); the clamp stays in the arm.
                Flag::new(
                    "max-depth",
                    Ty::UInt {
                        min: 1,
                        max: u32::MAX as u64,
                    },
                ),
            ],
            rules: &[],
            note: "<from> and <to> are each an exact symbol name or a SymbolId; --max-depth \
                   accepts 1..=16 (default 12, values above 16 clamp to 16)",
        },
    },
    Command {
        name: "lineage",
        spec: Spec::SelfParsed {
            values: &["db", "symbol", "depth", "relation"],
        },
    },
    Command {
        name: "supports",
        spec: Spec::SelfParsed {
            values: &["db", "source", "target", "kind", "producer", "snapshot"],
        },
    },
    Command {
        name: "stats",
        spec: owns(&[], &[DB, JSON]),
    },
    Command {
        name: "graph-view",
        spec: ruled(
            &[],
            &[
                DB,
                count("limit"),
                str_flag("focus"),
                switch("include-tests"),
                switch("include-trivial"),
                // Every pattern applies.
                str_flag("ignore").repeatable(),
            ],
            // `--limit 0` answers an empty slice before the focus symbol is ever looked up.
            &[Rule::Excludes(&["focus"], Cond::Is("limit", "0"))],
        ),
    },
    Command {
        name: "source",
        spec: ruled(
            &[req("name")],
            &[
                DB,
                JSON,
                Flag::new("symbols", Ty::List),
                count("cluster"),
                str_flag("file"),
                count("max-total-chars"),
                count("max-node-chars"),
                switch("signatures-only"),
            ],
            &[
                // Selectors and <name> may be combined; the arm applies the documented
                // precedence --symbols > --cluster > --file > <name>.
                Rule::OperandOr {
                    flags: &["symbols", "cluster", "file"],
                    exclusive: false,
                },
                // The budgets shape the JSON bundle; text mode prints whole bodies (#206).
                Rule::Needs(&["max-total-chars", "max-node-chars"], Cond::Given("json")),
            ],
        ),
    },
    Command {
        name: "semantic",
        spec: owns(&[req("query")], &[DB]),
    },
    Command {
        name: "cross-graph",
        // Every store is searched, in the order given (`--db` and `--dbs` interleave).
        spec: ruled(
            &[req("name")],
            &[DB.repeatable(), Flag::new("dbs", Ty::List)],
            &[Rule::AnyOf(&["db", "dbs"])],
        ),
    },
    Command {
        name: "compact",
        spec: owns(&[], &[DB]),
    },
    Command {
        name: "watch",
        spec: ruled(
            &[opt("path")],
            &[DB, REPO, AS, HISTORY],
            &[Rule::Excludes(&["history"], IN_MEMORY)],
        ),
    },
    Command {
        name: "subscribe",
        spec: owns(&[], &[DB, Flag::new("since", U64)]),
    },
    Command {
        name: "semantics",
        spec: ruled(
            &[req("symbol")],
            &[
                DB,
                str_flag("description"),
                str_flag("requirement"),
                Flag::new("validated", Ty::Bool),
                str_flag("validated-by"),
            ],
            // A validation claim names its actor (#79), and an actor without a claim is
            // recorded nowhere.
            &[
                Rule::Needs(&["validated"], Cond::Given("validated-by")),
                Rule::Needs(&["validated-by"], Cond::Given("validated")),
            ],
        ),
    },
    Command {
        name: "by-requirement",
        spec: owns(&[req("requirement")], &[DB]),
    },
    Command {
        name: "clusters",
        spec: ruled(
            &[Operand {
                name: "min-size",
                ty: USIZE,
                required: false,
            }],
            &[
                DB,
                JSON,
                switch("annotate"),
                switch("summary"),
                switch("hierarchical"),
                non_negative("resolution"),
                non_negative("package-bias"),
                Flag::new("weight", Ty::OneOf(&["graph", "semantic"])),
                count("k"),
                // Cosine distance (documented 0.0–2.0).
                Flag::new("eps", Ty::Float { min: 0.0, max: 2.0 }),
                count("min-pts"),
            ],
            &[
                // Each mode reads only its own knobs.
                Rule::Needs(&["k", "eps", "min-pts"], SEMANTIC),
                Rule::Excludes(
                    &["resolution", "package-bias", "hierarchical", "summary"],
                    SEMANTIC,
                ),
                // `--k` switches DBSCAN off, and with it `--eps`/`--min-pts`.
                Rule::Excludes(&["eps", "min-pts"], Cond::Given("k")),
                // `--summary` enriches the JSON objects; the text listing has no place for it.
                Rule::Needs(&["summary"], Cond::Given("json")),
            ],
        ),
    },
    Command {
        name: "context",
        spec: owns(&[req("name")], &[DB, JSON, count("budget")]),
    },
    Command {
        name: "annotate",
        spec: Spec::Owns {
            operands: &[req("name")],
            flags: &[
                DB,
                SYMBOL,
                str_flag("key"),
                str_flag("value"),
                TYPE,
                Flag::new("confidence", Ty::Float { min: 0.0, max: 1.0 }),
                str_flag("provenance"),
                str_flag("author"),
                switch("replace"),
            ],
            rules: &[
                BY_NAME_OR_SYMBOL,
                Rule::Required("key"),
                Rule::Required("value"),
            ],
            note: "",
        },
    },
    Command {
        name: "annotations",
        spec: ruled(
            &[req("name")],
            &[DB, JSON, SYMBOL, TYPE],
            &[BY_NAME_OR_SYMBOL],
        ),
    },
    Command {
        name: "stale-annotations",
        spec: owns(
            &[Operand {
                name: "cutoff-unix-seconds",
                ty: Ty::Int,
                required: true,
            }],
            &[DB, JSON],
        ),
    },
    Command {
        name: "fingerprint",
        spec: owns(&[req("name")], &[DB, switch("content")]),
    },
    Command {
        name: "changed-since",
        spec: owns(&[req("sha")], &[DB, JSON]),
    },
    Command {
        name: "entrypoints",
        spec: owns(&[], &[DB, JSON]),
    },
    Command {
        name: "leaves",
        spec: owns(&[], &[DB, JSON]),
    },
    Command {
        name: "dead-code",
        spec: owns(&[], &[DB, JSON]),
    },
    Command {
        name: "nodes",
        spec: ruled(
            &[],
            &[
                DB,
                JSON,
                KIND,
                str_flag("annotated-with"),
                switch("semantics"),
            ],
            &[
                // The annotation filter replaces the kind filter rather than narrowing it.
                Rule::Excludes(&["kind"], Cond::Given("annotated-with")),
                // The extra keys exist only on the JSON objects.
                Rule::Needs(&["semantics"], Cond::Given("json")),
            ],
        ),
    },
    Command {
        name: "resolve",
        spec: owns(
            &[req("name")],
            &[DB, JSON, KIND, str_flag("file"), switch("include-values")],
        ),
    },
    Command {
        name: "correspond",
        spec: ruled(
            &[],
            &[
                JSON,
                KIND,
                str_flag("db-a"),
                str_flag("db-b"),
                count("top"),
                non_negative("min-score"),
                switch("explain"),
            ],
            &[Rule::Required("db-a"), Rule::Required("db-b")],
        ),
    },
    Command {
        name: "export",
        spec: ruled(
            &[],
            &[
                DB,
                Flag::new("format", Ty::OneOf(&["ndjson", "json"])),
                switch("nodes-only"),
                switch("edges-only"),
            ],
            // Together they export nothing.
            &[Rule::Excludes(&["nodes-only"], Cond::Given("edges-only"))],
        ),
    },
    // Dispatched before the store spec resolves (#200), as `version`, `--version` or `-V`.
    Command {
        name: "version",
        spec: owns(&[], &[]),
    },
    Command {
        name: "plugins",
        spec: owns(
            &[Operand {
                name: "list",
                ty: Ty::OneOf(&["list"]),
                required: false,
            }],
            &[],
        ),
    },
];

pub fn lookup(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// A checked, coerced value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Switch,
    Str(String),
    UInt(u64),
    Int(i64),
    Float(f64),
    Bool(bool),
    List(Vec<String>),
}

impl Value {
    /// Does this value spell `want`, as [`Cond::Is`] compares it?
    fn is(&self, want: &str) -> bool {
        match self {
            Value::Str(s) => s == want,
            Value::UInt(n) => n.to_string() == want,
            Value::Int(n) => n.to_string() == want,
            _ => false,
        }
    }
}

/// What [`parse`] found.
#[derive(Debug)]
pub enum Parsed {
    /// Not a bespoke command; the usage arm reports it.
    Unlisted,
    /// `--help`/`-h` in flag position.
    Help,
    /// A self-parsing command; it reads its argv itself, `--db` included.
    SelfParsing,
    Args(Args),
}

/// A bespoke command's argv, checked against its row and coerced once.
#[derive(Debug)]
pub struct Args {
    cmd: &'static str,
    flags: &'static [Flag],
    operands: Vec<Value>,
    /// `(key, value)` in argv order.
    values: Vec<(&'static str, Value)>,
}

impl Args {
    /// No flags and no operands, for the usage arm and the self-parsing commands, which read
    /// none of them from here.
    pub fn none() -> Args {
        Args {
            cmd: "",
            flags: &[],
            operands: Vec::new(),
            values: Vec::new(),
        }
    }

    /// The declaration for `key`. Reading a flag the row does not declare is a bug in the arm:
    /// the table must list exactly the flags its arm reads.
    fn declared(&self, key: &str) -> &Flag {
        self.flags
            .iter()
            .find(|f| f.key == key)
            .unwrap_or_else(|| panic!("{} reads --{key}, which its row does not own", self.cmd))
    }

    /// Does this command own `key`? For code shared by several arms (`--db`).
    pub fn owns(&self, key: &str) -> bool {
        self.flags.iter().any(|f| f.key == key)
    }

    fn one(&self, key: &str) -> Option<&Value> {
        assert!(
            !self.declared(key).repeat,
            "--{key} repeats; read it with all()"
        );
        self.values.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    pub fn switch(&self, key: &str) -> bool {
        self.one(key).is_some()
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        self.one(key).map(|v| match v {
            Value::Str(s) => s.as_str(),
            other => panic!("--{key} is {other:?}, not a string"),
        })
    }

    /// A string flag the row declares [`Rule::Required`], so it is always present.
    pub fn given(&self, key: &str) -> &str {
        self.str(key)
            .unwrap_or_else(|| panic!("{}: --{key} is not required by its row", self.cmd))
    }

    pub fn list(&self, key: &str) -> Option<&[String]> {
        self.one(key).map(|v| match v {
            Value::List(items) => items.as_slice(),
            other => panic!("--{key} is {other:?}, not a list"),
        })
    }

    pub fn u64(&self, key: &str) -> Option<u64> {
        self.one(key).map(|v| match v {
            Value::UInt(n) => *n,
            other => panic!("--{key} is {other:?}, not an unsigned integer"),
        })
    }

    /// The row bounds the value, so the conversion cannot fail.
    pub fn usize(&self, key: &str) -> Option<usize> {
        self.u64(key)
            .map(|n| usize::try_from(n).expect("bounded by the row"))
    }

    pub fn u32(&self, key: &str) -> Option<u32> {
        self.u64(key)
            .map(|n| u32::try_from(n).expect("bounded by the row"))
    }

    pub fn f64(&self, key: &str) -> Option<f64> {
        self.one(key).map(|v| match v {
            Value::Float(n) => *n,
            other => panic!("--{key} is {other:?}, not a number"),
        })
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        self.one(key).map(|v| match v {
            Value::Bool(b) => *b,
            other => panic!("--{key} is {other:?}, not a boolean"),
        })
    }

    /// Every value of the given flags, in argv order, with list items in place: repeatable
    /// flags, or flags that feed one list (`cross-graph --db a --dbs b,c` → `a, b, c`).
    pub fn all(&self, keys: &[&str]) -> Vec<&str> {
        for k in keys {
            self.declared(k);
        }
        self.values
            .iter()
            .filter(|(k, _)| keys.contains(k))
            .flat_map(|(k, v)| match v {
                Value::Str(s) => vec![s.as_str()],
                Value::List(items) => items.iter().map(String::as_str).collect(),
                other => panic!("--{k} is {other:?}, not a string"),
            })
            .collect()
    }

    pub fn operand(&self, i: usize) -> Option<&str> {
        self.operands.get(i).map(|v| match v {
            Value::Str(s) => s.as_str(),
            other => panic!("operand {i} is {other:?}, not a string"),
        })
    }

    /// An operand the row declares required, so it is always present.
    pub fn required(&self, i: usize) -> &str {
        self.operand(i)
            .unwrap_or_else(|| panic!("{}: operand {i} is not required by its row", self.cmd))
    }

    pub fn operand_usize(&self, i: usize) -> Option<usize> {
        self.operands.get(i).map(|v| match v {
            Value::UInt(n) => usize::try_from(*n).expect("bounded by the row"),
            other => panic!("operand {i} is {other:?}, not an unsigned integer"),
        })
    }

    pub fn operand_i64(&self, i: usize) -> Option<i64> {
        self.operands.get(i).map(|v| match v {
            Value::Int(n) => *n,
            other => panic!("operand {i} is {other:?}, not an integer"),
        })
    }
}

/// `-x` and `--x` are flags; `-`, `-1` and anything without a leading dash are operands.
fn is_flag(a: &str) -> bool {
    a.starts_with("--")
        || a.len() > 1 && a.starts_with('-') && a.as_bytes()[1].is_ascii_alphabetic()
}

fn placeholder(ty: Ty) -> String {
    match ty {
        Ty::Switch => String::new(),
        Ty::Str | Ty::Label => "V".into(),
        Ty::UInt { .. } | Ty::Int => "N".into(),
        Ty::Float { .. } => "F".into(),
        Ty::Bool => "true|false".into(),
        Ty::OneOf(vals) => vals.join("|"),
        Ty::List => "a,b".into(),
    }
}

/// `nodes [--db V] [--json] [--kind V] …`. A required flag has no brackets; flags that can
/// stand in for the operands are grouped with them: `(<name> | --symbol V)`.
fn usage(name: &str, spec: &Spec) -> String {
    let Spec::Owns {
        operands,
        flags,
        rules,
        note,
    } = spec
    else {
        return format!("usage: wicked-estate {name}");
    };
    let flag_text = |f: &Flag| match f.ty {
        Ty::Switch => format!("--{}", f.name),
        ty => format!("--{} {}", f.name, placeholder(ty)),
    };
    let required = |k: &str| {
        rules
            .iter()
            .any(|r| matches!(r, Rule::Required(r) if *r == k))
    };
    let stand_ins: &[&str] = rules
        .iter()
        .find_map(|r| match r {
            Rule::OperandOr { flags, .. } => Some(*flags),
            _ => None,
        })
        .unwrap_or(&[]);
    let mut s = format!("usage: wicked-estate {name}");
    let ops: Vec<String> = operands
        .iter()
        .map(|o| match (o.ty, o.required) {
            (Ty::OneOf(&[lit]), false) => format!("[{lit}]"),
            (_, true) => format!("<{}>", o.name),
            (_, false) => format!("[<{}>]", o.name),
        })
        .collect();
    if stand_ins.is_empty() {
        if !ops.is_empty() {
            s.push_str(&format!(" {}", ops.join(" ")));
        }
    } else {
        let mut group = vec![ops.join(" ")];
        group.extend(
            flags
                .iter()
                .filter(|f| stand_ins.contains(&f.key))
                .map(flag_text),
        );
        s.push_str(&format!(" ({})", group.join(" | ")));
    }
    for f in flags.iter().filter(|f| !stand_ins.contains(&f.key)) {
        let more = if f.repeat { "…" } else { "" };
        if required(f.key) {
            s.push_str(&format!(" {}{more}", flag_text(f)));
        } else {
            s.push_str(&format!(" [{}]{more}", flag_text(f)));
        }
    }
    if !note.is_empty() {
        s.push('\n');
        s.push_str(note);
    }
    s
}

/// Is `--help`/`-h` present in flag position? A known value flag without an inline `=`
/// consumes the next token, so a `--help` there is that flag's value, not a help request.
/// Unknown flags are stepped over: help still wins over an earlier typo.
fn help_in_flag_position(args: &[String], takes_value: impl Fn(&str) -> bool) -> bool {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--help" || a == "-h" {
            return true;
        }
        let Some(body) = a.strip_prefix("--") else {
            continue;
        };
        if !body.contains('=') && takes_value(body) {
            it.next();
        }
    }
    false
}

/// Check and coerce one value. `what` names the field in the error (`--top`, `<min-size>`).
fn coerce(ty: Ty, what: &str, v: &str) -> Result<Value, String> {
    if v.is_empty() {
        return Err(format!("{what} must not be empty"));
    }
    match ty {
        Ty::Switch => unreachable!("a switch has no value"),
        Ty::Str => Ok(Value::Str(v.to_string())),
        Ty::Label => wicked_estate::repo_scope::validate_label(v)
            .map(|()| Value::Str(v.to_string()))
            .map_err(|e| format!("{what} {v}: {e}")),
        Ty::UInt { min, max } => {
            if !v.bytes().all(|b| b.is_ascii_digit()) {
                return Err(format!("{what} must be a non-negative integer, got {v:?}"));
            }
            let n: u64 =
                v.parse().ok().filter(|n| *n <= max).ok_or_else(|| {
                    format!("{what} {v} is out of range: above the maximum of {max}")
                })?;
            if n < min {
                return Err(format!("{what} must be at least {min}, got {n}"));
            }
            Ok(Value::UInt(n))
        }
        Ty::Int => {
            let digits = v.strip_prefix('-').unwrap_or(v);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return Err(format!("{what} must be an integer, got {v:?}"));
            }
            v.parse()
                .map(Value::Int)
                .map_err(|_| format!("{what} {v} is out of range for a 64-bit integer"))
        }
        Ty::Float { min, max } => {
            let n: f64 = v
                .parse()
                .map_err(|_| format!("{what} must be a number, got {v:?}"))?;
            if !n.is_finite() {
                return Err(format!("{what} must be a finite number, got {v:?}"));
            }
            if n < min || n > max {
                return Err(if max == f64::MAX {
                    format!("{what} must be at least {min}, got {v}")
                } else {
                    format!("{what} must be within {min}..={max}, got {v}")
                });
            }
            Ok(Value::Float(n))
        }
        Ty::Bool => match v {
            "true" | "1" | "yes" => Ok(Value::Bool(true)),
            "false" | "0" | "no" => Ok(Value::Bool(false)),
            _ => Err(format!("{what} must be true or false, got {v:?}")),
        },
        Ty::List => {
            let items: Vec<String> = v.split(',').map(|i| i.trim().to_string()).collect();
            if items.iter().any(String::is_empty) {
                return Err(format!(
                    "{what} has an empty item in {v:?}; separate items with single commas"
                ));
            }
            Ok(Value::List(items))
        }
        Ty::OneOf(vals) => {
            if vals.contains(&v) {
                Ok(Value::Str(v.to_string()))
            } else {
                Err(format!(
                    "{what} must be one of {}, got {v:?}",
                    vals.join("|")
                ))
            }
        }
    }
}

/// Check `args` against `cmd`'s row and coerce every value, before any I/O. `default_db` is the
/// store spec used when `--db` is absent (already resolved from the environment), for
/// [`Cond::Store`]. `Err` is the full usage message plus the offending field.
pub fn parse(cmd: &str, args: &[String], default_db: &str) -> Result<Parsed, String> {
    let Some(command) = lookup(cmd) else {
        return Ok(Parsed::Unlisted);
    };
    let (operands, flags, rules) = match &command.spec {
        Spec::SelfParsed { values } => {
            if help_in_flag_position(args, |f| values.contains(&f)) {
                return Ok(Parsed::Help);
            }
            return Ok(Parsed::SelfParsing);
        }
        Spec::Owns {
            operands,
            flags,
            rules,
            ..
        } => (*operands, *flags, *rules),
    };
    // Not `any(--help)`: a `--help` in a value slot is that flag's value (refused below), or
    // `nodes --db --help --bogus` would skip the check — the accept-and-ignore path, for one token.
    let takes_value = |f: &str| flags.iter().any(|g| g.name == f && g.ty != Ty::Switch);
    if help_in_flag_position(args, takes_value) {
        return Ok(Parsed::Help);
    }
    let fail = |why: String| Err(format!("{}\n{why}", usage(cmd, &command.spec)));

    let mut raw_operands: Vec<&String> = Vec::new();
    let mut values: Vec<(&'static str, Value)> = Vec::new();
    // `(key, spelling)` of each flag given, for the repeat check's message.
    let mut seen: Vec<(&'static str, &'static str)> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if !is_flag(a) {
            raw_operands.push(a);
            continue;
        }
        let Some(body) = a.strip_prefix("--") else {
            return fail(format!("unknown flag {a:?}"));
        };
        let (name, inline) = match body.split_once('=') {
            Some((n, v)) => (n, Some(v)),
            None => (body, None),
        };
        let Some(flag) = flags.iter().find(|f| f.name == name) else {
            let owners: Vec<&str> = COMMANDS
                .iter()
                .filter(|c| matches!(&c.spec, Spec::Owns { flags, .. } if flags.iter().any(|f| f.name == name)))
                .map(|c| c.name)
                .collect();
            return fail(if owners.is_empty() {
                format!("unknown flag \"--{name}\"")
            } else {
                format!(
                    "unknown flag \"--{name}\" for {cmd} (accepted by: {})",
                    owners.join(", ")
                )
            });
        };
        if !flag.repeat {
            if let Some((_, prev)) = seen.iter().find(|(k, _)| *k == flag.key) {
                return fail(if *prev == flag.name {
                    format!("--{name} given more than once")
                } else {
                    format!("--{name} given more than once (it is --{prev} by another name)")
                });
            }
        }
        seen.push((flag.key, flag.name));
        let value = match (flag.ty, inline) {
            (Ty::Switch, Some(_)) => return fail(format!("--{name} takes no value")),
            (Ty::Switch, None) => Value::Switch,
            // `--file=x` is not a spelling this command owns; it was once dropped whole.
            (_, Some(_)) if !flag.inline => {
                return fail(format!(
                    "--{name}=… is not accepted; write --{name} <value>"
                ));
            }
            (ty, Some(v)) => match coerce(ty, &format!("--{name}"), v) {
                Ok(v) => v,
                Err(e) => return fail(e),
            },
            // A value is never a `--flag`: `--symbols --db x` would make `--db` the selector.
            (ty, None) => match it.next() {
                None => return fail(format!("--{name} requires a value")),
                Some(v) if v.starts_with("--") => {
                    return fail(format!("--{name} requires a value, got the flag {v:?}"));
                }
                Some(v) => match coerce(ty, &format!("--{name}"), v) {
                    Ok(v) => v,
                    Err(e) => return fail(e),
                },
            },
        };
        values.push((flag.key, value));
    }

    let given = |k: &str| values.iter().any(|(v, _)| *v == k);
    let explicit_db = values.iter().rev().find_map(|(k, v)| match (k, v) {
        (&"db", Value::Str(s)) => Some(s.as_str()),
        _ => None,
    });
    let holds = |c: Cond| match c {
        Cond::Given(k) => given(k),
        Cond::Is(k, want) => values.iter().any(|(v, x)| *v == k && x.is(want)),
        Cond::Store(want) => explicit_db.unwrap_or(default_db) == want,
    };
    let spell = |c: Cond| match c {
        Cond::Given(k) => format!("--{k}"),
        Cond::Is(k, want) => format!("--{k} {want}"),
        Cond::Store(want) if explicit_db.is_some() => format!("--db {want}"),
        Cond::Store(want) => {
            format!("the default store {want} (set by the environment; pass --db to override)")
        }
    };
    let dashed = |ks: &[&str]| ks.iter().map(|k| format!("--{k}")).collect::<Vec<_>>();

    if let Some(extra) = raw_operands.get(operands.len()) {
        return fail(format!("unexpected operand {extra:?}"));
    }
    let stand_in = rules.iter().find_map(|r| match r {
        Rule::OperandOr { flags, exclusive } => Some((*flags, *exclusive)),
        _ => None,
    });
    match stand_in {
        Some((flags, exclusive)) if flags.iter().any(|k| given(k)) => {
            if let (true, Some(extra)) = (exclusive, raw_operands.first()) {
                return fail(format!(
                    "give <{}> or {}, not both (got {extra:?} and {})",
                    operands[0].name,
                    dashed(flags).join("/"),
                    dashed(
                        &flags
                            .iter()
                            .copied()
                            .filter(|k| given(k))
                            .collect::<Vec<_>>()
                    )
                    .join(" ")
                ));
            }
        }
        _ => {
            if let Some(missing) = operands
                .iter()
                .skip(raw_operands.len())
                .find(|o| o.required)
            {
                return fail(match stand_in {
                    Some(([k], _)) => format!("<{}> or --{k} is required", missing.name),
                    Some((ks, _)) => format!(
                        "<{}> or one of {} is required",
                        missing.name,
                        dashed(ks).join("/")
                    ),
                    None => format!("<{}> is required", missing.name),
                });
            }
        }
    }
    for rule in rules {
        let broken = match *rule {
            Rule::Required(k) => (!given(k)).then(|| format!("--{k} is required")),
            Rule::AnyOf(ks) => (!ks.iter().any(|k| given(k)))
                .then(|| format!("one of {} is required", dashed(ks).join("/"))),
            Rule::Needs(fs, c) => fs
                .iter()
                .find(|f| given(f) && !holds(c))
                .map(|f| format!("--{f} applies only with {}", spell(c))),
            Rule::Excludes(fs, c) => fs
                .iter()
                .find(|f| given(f) && holds(c))
                .map(|f| format!("--{f} does not apply with {}", spell(c))),
            Rule::OperandOr { .. } => None,
        };
        if let Some(why) = broken {
            return fail(why);
        }
    }
    let mut checked = Vec::with_capacity(raw_operands.len());
    for (o, v) in operands.iter().zip(&raw_operands) {
        match coerce(o.ty, &format!("<{}>", o.name), v) {
            Ok(v) => checked.push(v),
            Err(e) => return fail(e),
        }
    }
    Ok(Parsed::Args(Args {
        cmd: command.name,
        flags,
        operands: checked,
        values,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(cmd: &str, a: &[&str]) -> Result<Parsed, String> {
        parse(
            cmd,
            &a.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            ".wicked-estate/graph.db",
        )
    }

    fn args(cmd: &str, a: &[&str]) -> Args {
        match parse_args(cmd, a) {
            Ok(Parsed::Args(x)) => x,
            other => panic!("{cmd} {a:?}: {other:?}"),
        }
    }

    fn err(cmd: &str, a: &[&str]) -> String {
        match parse_args(cmd, a) {
            Err(e) => e,
            other => panic!("{cmd} {a:?} was accepted: {other:?}"),
        }
    }

    fn is_help(cmd: &str, a: &[&str]) -> bool {
        matches!(parse_args(cmd, a), Ok(Parsed::Help))
    }

    #[test]
    fn the_table_is_well_formed() {
        for (i, c) in COMMANDS.iter().enumerate() {
            assert!(
                COMMANDS[..i].iter().all(|d| d.name != c.name),
                "{} listed twice",
                c.name
            );
            // A bridged command never reaches here, so a row would be dead.
            assert!(crate::tool_bridge::lookup(c.name).is_none(), "{}", c.name);
            let Spec::Owns {
                operands,
                flags,
                rules,
                ..
            } = &c.spec
            else {
                continue;
            };
            for (j, f) in flags.iter().enumerate() {
                assert!(
                    flags[..j].iter().all(|g| g.name != f.name),
                    "{}: --{} listed twice",
                    c.name,
                    f.name
                );
                assert!(
                    f.key == f.name || flags.iter().any(|g| g.name == f.key),
                    "{}: --{} aliases a flag the row does not own",
                    c.name,
                    f.name
                );
                assert!(
                    !(f.inline && f.ty == Ty::Switch),
                    "{}: --{}",
                    c.name,
                    f.name
                );
                assert!(!(f.repeat && f.ty != Ty::Str), "{}: --{}", c.name, f.name);
            }
            // Required operands come first, so arity is a range.
            let first_opt = operands.iter().position(|o| !o.required);
            assert!(
                first_opt.is_none_or(|p| operands[p..].iter().all(|o| !o.required)),
                "{}: a required operand follows an optional one",
                c.name
            );
            // Every rule names flags the row owns, and an `Is` value the flag can actually take,
            // or the rule could never fire.
            let owned = |k: &str| {
                assert!(
                    flags.iter().any(|f| f.key == k),
                    "{}: a rule names --{k}, which the row does not own",
                    c.name
                );
            };
            let cond = |cnd: Cond| match cnd {
                Cond::Given(k) => owned(k),
                // `--db` must be owned, or the rule could never be satisfied by overriding it.
                Cond::Store(_) => owned("db"),
                Cond::Is(k, v) => {
                    owned(k);
                    let f = flags.iter().find(|f| f.key == k).unwrap();
                    assert!(
                        coerce(f.ty, k, v).is_ok(),
                        "{}: --{k} cannot be {v:?}",
                        c.name
                    );
                }
            };
            let mut stand_ins = 0;
            for r in *rules {
                match *r {
                    Rule::Required(k) => {
                        owned(k);
                        let f = flags.iter().find(|f| f.key == k).unwrap();
                        assert!(f.ty != Ty::Switch, "{}: a required switch", c.name);
                    }
                    Rule::AnyOf(ks) => ks.iter().for_each(|k| owned(k)),
                    Rule::Needs(fs, cnd) | Rule::Excludes(fs, cnd) => {
                        fs.iter().for_each(|k| owned(k));
                        cond(cnd);
                    }
                    Rule::OperandOr { flags: fs, .. } => {
                        stand_ins += 1;
                        fs.iter().for_each(|k| owned(k));
                        assert!(
                            operands.len() == 1 && operands[0].required,
                            "{}: OperandOr stands in for exactly one required operand",
                            c.name
                        );
                    }
                }
            }
            assert!(stand_ins <= 1, "{}: two OperandOr rules", c.name);
        }
    }

    /// Every dispatch arm in `main` is a row here or a bridged command, and every row has an
    /// arm: an arm with neither would inherit no contract at all, and a row with no arm is dead.
    #[test]
    fn every_bespoke_dispatch_arm_is_in_the_table_or_the_bridge() {
        let main = include_str!("main.rs");
        let start = main
            .find("    match cmd {\n")
            .expect("main's dispatch match");
        let body = &main[start..];
        let end = body.find("\n        _ => {").expect("the usage arm");
        let mut arms: Vec<&str> = body[..end]
            .lines()
            .filter_map(|l| l.strip_prefix("        \""))
            .filter_map(|l| l.split_once("\" => {"))
            .map(|(name, _)| name)
            .collect();
        assert!(arms.len() > 30, "parsed too few arms: {arms:?}");
        // `version` is dispatched before the match, ahead of store resolution (#200).
        assert!(
            main.contains(r#"cli_flags::parse("version", rest, "")"#),
            "the early `version` dispatch no longer checks its row"
        );
        arms.push("version");
        for arm in &arms {
            assert!(
                lookup(arm).is_some() || crate::tool_bridge::lookup(arm).is_some(),
                "dispatch arm {arm:?} has no cli_flags row and no bridge entry"
            );
        }
        for c in COMMANDS {
            assert!(arms.contains(&c.name), "row {} has no dispatch arm", c.name);
        }
        for cmd in ["lineage", "supports"] {
            assert!(
                matches!(
                    lookup(cmd),
                    Some(Command {
                        spec: Spec::SelfParsed { .. },
                        ..
                    })
                ),
                "{cmd} has no SelfParsed row"
            );
        }
    }

    #[test]
    fn unknown_and_foreign_flags_are_rejected_197() {
        let e = err("nodes", &["--bogus-flag", "zzz"]);
        assert!(e.starts_with("usage: wicked-estate nodes"), "{e}");
        assert!(e.ends_with("unknown flag \"--bogus-flag\""), "{e}");
        let e = err("nodes", &["--symbol", "id"]);
        assert!(e.contains("accepted by: annotate, annotations"), "{e}");
        err("path", &["a", "b", "--top", "5"]);
        err("correspond", &["--db", "x.db"]);
        err("nodes", &["-x"]);
        err("nodes", &["--"]);
    }

    #[test]
    fn help_wins_only_in_flag_position_197() {
        assert!(is_help("nodes", &["--bogus", "--help"]));
        assert!(is_help("nodes", &["-h"]));
        assert!(is_help("query", &["a", "b", "--top", "x", "--help"]));
        let e = err("nodes", &["--db", "--help", "--bogus"]);
        assert!(
            e.ends_with("--db requires a value, got the flag \"--help\""),
            "{e}"
        );
        // A single-dash token is a value, so `-h` in a value slot is the value `-h`.
        let a = args("annotate", &["f", "--key", "k", "--value", "-h"]);
        assert_eq!(a.str("value"), Some("-h"));
        err("annotate", &["f", "--key", "k", "--value", "--help"]);
        // An inline value does not consume the next token, so help after it is a help request.
        assert!(is_help("index", &["--repo=x", "--help"]));
        // Self-parsed rows: help in flag position, but not in one of their value slots.
        assert!(is_help("lineage", &["--help"]));
        assert!(!is_help("lineage", &["--symbol", "x", "--depth", "--help"]));
        assert!(!is_help("supports", &["edge", "--source", "--help"]));
    }

    #[test]
    fn values_are_coerced_once_to_their_type() {
        let a = args(
            "correspond",
            &[
                "--db-a",
                "a",
                "--db-b",
                "b",
                "--top",
                "7",
                "--min-score",
                "0.5",
            ],
        );
        assert_eq!(a.usize("top"), Some(7));
        assert_eq!(a.f64("min-score"), Some(0.5));
        assert_eq!(
            a.str("kind"),
            None,
            "absent → None, the arm's default applies"
        );
        let a = args(
            "clusters",
            &[
                "5",
                "--weight",
                "semantic",
                "--eps",
                "0.2",
                "--min-pts",
                "4",
            ],
        );
        assert_eq!(a.operand_usize(0), Some(5));
        assert_eq!(a.str("weight"), Some("semantic"));
        assert_eq!(a.f64("eps"), Some(0.2));
        let a = args("stale-annotations", &["-1"]);
        assert_eq!(a.operand_i64(0), Some(-1));
        let a = args(
            "semantics",
            &["s", "--validated", "no", "--validated-by", "me"],
        );
        assert_eq!(a.bool("validated"), Some(false));
        let a = args("blast-radius", &["f", "--depth=3"]);
        assert_eq!(a.u32("depth"), Some(3));
        let a = args("index", &[".", "--as", "ledger"]);
        assert_eq!(
            a.str("repo"),
            Some("ledger"),
            "an alias lands on its canonical key"
        );
    }

    #[test]
    fn malformed_values_name_the_field() {
        for (cmd, a, field) in [
            (
                "correspond",
                &["--top", "abc"][..],
                "--top must be a non-negative integer",
            ),
            (
                "correspond",
                &["--top", "-1"][..],
                "--top must be a non-negative integer",
            ),
            (
                "correspond",
                &["--min-score", "NaN"][..],
                "--min-score must be a finite number",
            ),
            (
                "correspond",
                &["--min-score", "-0.1"][..],
                "--min-score must be at least 0",
            ),
            (
                "subscribe",
                &["--since", ""][..],
                "--since must not be empty",
            ),
            (
                "graph-view",
                &["--limit", "99999999999999999999999"][..],
                "--limit 99999999999999999999999 is out of range",
            ),
            (
                "clusters",
                &["--eps", "2.5"][..],
                "--eps must be within 0..=2",
            ),
            (
                "clusters",
                &["--weight", "x"][..],
                "--weight must be one of graph|semantic",
            ),
            (
                "clusters",
                &["-1"][..],
                "<min-size> must be a non-negative integer",
            ),
            (
                "stale-annotations",
                &["1.5"][..],
                "<cutoff-unix-seconds> must be an integer",
            ),
            ("plugins", &["lsit"][..], "<list> must be one of list"),
            ("blast-radius", &["f", "--depth=99"][..], "maximum of"),
            (
                "path",
                &["a", "b", "--max-depth", "0"][..],
                "--max-depth must be at least 1",
            ),
            (
                "index",
                &["--repo", "a/b"][..],
                "--repo a/b: invalid argument: invalid repo label",
            ),
        ] {
            let e = err(cmd, a);
            assert!(e.starts_with(&format!("usage: wicked-estate {cmd}")), "{e}");
            assert!(e.contains(field), "{cmd} {a:?}: {e}");
        }
        assert!(
            err("path", &["a"]).contains("1..=16"),
            "the path note is kept"
        );
    }

    #[test]
    fn operand_arity_is_exact() {
        assert!(err("stats", &["foo"]).ends_with("unexpected operand \"foo\""));
        assert!(err("query", &["a", "b"]).ends_with("unexpected operand \"b\""));
        assert!(err("query", &[]).ends_with("<name> is required"));
        assert!(err("path", &["a"]).contains("<to> is required"));
        assert_eq!(args("index", &[]).operand(0), None);
        assert_eq!(args("plugins", &["list"]).operand(0), Some("list"));
        assert_eq!(args("query", &["-"]).operand(0), Some("-"));
        // `<name>` XOR `--symbol`.
        assert!(err("annotate", &["f", "--symbol", "i"]).contains("not both"));
        assert!(err("annotations", &[]).contains("<name> or --symbol is required"));
        assert_eq!(
            args("annotations", &["--symbol", "i"]).str("symbol"),
            Some("i")
        );
    }

    #[test]
    fn repeats_are_errors_except_where_declared() {
        assert!(
            err("nodes", &["--kind", "A", "--kind", "B"]).ends_with("--kind given more than once")
        );
        assert!(err("nodes", &["--json", "--json"]).ends_with("--json given more than once"));
        assert!(
            err("index", &["--repo", "a", "--as", "b"]).contains("it is --repo by another name")
        );
        assert!(err("index", &["--as", "a", "--repo", "b"]).contains("it is --as by another name"));
        assert!(err("index", &["--as", "a", "--as", "b"]).ends_with("--as given more than once"));
        let a = args(
            "cross-graph",
            &["f", "--db", "b", "--dbs", "c,d", "--db", "a"],
        );
        assert_eq!(a.all(&["db", "dbs"]), ["b", "c", "d", "a"]);
        let a = args("graph-view", &["--ignore", "x", "--ignore", "y"]);
        assert_eq!(a.all(&["ignore"]), ["x", "y"]);
    }

    #[test]
    fn spelling_is_enforced() {
        assert!(err("source", &["f", "--file"]).ends_with("--file requires a value"));
        assert!(err("source", &["--file=a.py"]).contains("write --file <value>"));
        assert!(
            err("source", &["--symbols", "--db", "x.db"])
                .ends_with("--symbols requires a value, got the flag \"--db\"")
        );
        assert!(err("nodes", &["--json=1"]).ends_with("--json takes no value"));
        assert!(err("index", &["--repo="]).contains("--repo must not be empty"));
    }

    #[test]
    fn unlisted_and_self_parsed_pass_through() {
        assert!(matches!(
            parse_args("no-such-command", &["--bogus"]),
            Ok(Parsed::Unlisted)
        ));
        // `lineage`/`supports` reject unknown flags themselves, with their own usage.
        assert!(matches!(
            parse_args("lineage", &["--bogus", "--db", "g.db"]),
            Ok(Parsed::SelfParsing)
        ));
        assert!(matches!(
            parse_args("supports", &["owners"]),
            Ok(Parsed::SelfParsing)
        ));
    }

    /// Each declared combination rule: the flag the arm would ignore is refused before I/O, and
    /// the combinations the arm does read pass.
    #[test]
    fn combination_rules_refuse_what_the_arm_would_ignore() {
        for (cmd, a, why) in [
            ("annotate", &["f", "--value", "v"][..], "--key is required"),
            ("annotate", &["f", "--key", "k"][..], "--value is required"),
            ("correspond", &["--db-a", "a"][..], "--db-b is required"),
            ("cross-graph", &["f"][..], "one of --db/--dbs is required"),
            (
                "cross-graph",
                &["f", "--dbs", "a,,b"][..],
                "--dbs has an empty item in \"a,,b\"; separate items with single commas",
            ),
            (
                "source",
                &[][..],
                "<name> or one of --symbols/--cluster/--file is required",
            ),
            (
                "source",
                &["f", "--max-total-chars", "5"][..],
                "--max-total-chars applies only with --json",
            ),
            (
                "source",
                &["--symbols", "a,"][..],
                "--symbols has an empty item in \"a,\"; separate items with single commas",
            ),
            (
                "semantics",
                &["s", "--validated", "true"][..],
                "--validated applies only with --validated-by",
            ),
            (
                "semantics",
                &["s", "--validated-by", "me"][..],
                "--validated-by applies only with --validated",
            ),
            (
                "clusters",
                &["--k", "3"][..],
                "--k applies only with --weight semantic",
            ),
            (
                "clusters",
                &["--eps", "0.2", "--weight", "graph"][..],
                "--eps applies only with --weight semantic",
            ),
            (
                "clusters",
                &["--weight", "semantic", "--resolution", "2"][..],
                "--resolution does not apply with --weight semantic",
            ),
            (
                "clusters",
                &["--weight", "semantic", "--summary", "--json"][..],
                "--summary does not apply with --weight semantic",
            ),
            (
                "clusters",
                &["--weight", "semantic", "--k", "3", "--eps", "0.2"][..],
                "--eps does not apply with --k",
            ),
            (
                "clusters",
                &["--summary"][..],
                "--summary applies only with --json",
            ),
            (
                "nodes",
                &["--kind", "Function", "--annotated-with", "k"][..],
                "--kind does not apply with --annotated-with",
            ),
            (
                "nodes",
                &["--semantics"][..],
                "--semantics applies only with --json",
            ),
            (
                "export",
                &["--nodes-only", "--edges-only"][..],
                "--nodes-only does not apply with --edges-only",
            ),
            (
                "graph-view",
                &["--focus", "f", "--limit", "0"][..],
                "--focus does not apply with --limit 0",
            ),
            (
                "index",
                &[".", "--db", ":memory:", "--history"][..],
                "--history does not apply with --db :memory:",
            ),
            (
                "index",
                &[".", "--db", ":memory:", "--embeddings"][..],
                "--embeddings does not apply with --db :memory:",
            ),
            (
                "watch",
                &[".", "--db", ":memory:", "--history"][..],
                "--history does not apply with --db :memory:",
            ),
        ] {
            let e = err(cmd, a);
            assert!(e.starts_with(&format!("usage: wicked-estate {cmd}")), "{e}");
            assert!(e.ends_with(why), "{cmd} {a:?}: {e}");
        }
        // What the arms do read still passes.
        args("source", &["--symbols", "a, b"]);
        args(
            "source",
            &["f", "--file", "x.py", "--json", "--max-total-chars", "5"],
        );
        args(
            "semantics",
            &["s", "--validated", "false", "--validated-by", "me"],
        );
        args("clusters", &["--weight", "semantic", "--k", "3", "--json"]);
        args(
            "clusters",
            &["--weight", "semantic", "--eps", "0.2", "--min-pts", "2"],
        );
        args(
            "clusters",
            &[
                "--weight",
                "graph",
                "--resolution",
                "2",
                "--summary",
                "--json",
            ],
        );
        args(
            "nodes",
            &["--annotated-with", "k=v", "--json", "--semantics"],
        );
        args("graph-view", &["--focus", "f", "--limit", "10"]);
        args("index", &[".", "--db", "g.db", "--history", "--embeddings"]);
        assert_eq!(
            args("source", &["--symbols", "a, b"])
                .list("symbols")
                .unwrap(),
            ["a", "b"]
        );
    }

    /// The store rule sees the store this run will open, wherever its spec came from.
    #[test]
    fn store_rules_see_the_environment_default_and_an_explicit_override() {
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let e = parse("index", &argv(&[".", "--history"]), ":memory:").unwrap_err();
        assert!(
            e.ends_with(
                "--history does not apply with the default store :memory: (set by the \
                 environment; pass --db to override)"
            ),
            "{e}"
        );
        assert!(parse("watch", &argv(&["--history"]), ":memory:").is_err());
        assert!(parse("index", &argv(&[".", "--embeddings"]), ":memory:").is_err());
        // An explicit file store overrides an in-memory default, and the reverse.
        assert!(matches!(
            parse(
                "index",
                &argv(&[".", "--history", "--db", "g.db"]),
                ":memory:"
            ),
            Ok(Parsed::Args(_))
        ));
        let e = parse(
            "index",
            &argv(&[".", "--history", "--db", ":memory:"]),
            "g.db",
        )
        .unwrap_err();
        assert!(
            e.ends_with("--history does not apply with --db :memory:"),
            "{e}"
        );
    }

    #[test]
    fn usage_shows_required_flags_and_operand_stand_ins() {
        let e = err("annotate", &[]);
        assert!(
            e.starts_with(
                "usage: wicked-estate annotate (<name> | --symbol V) [--db V] --key V --value V"
            ),
            "{e}"
        );
        let e = err("source", &[]);
        assert!(
            e.starts_with(
                "usage: wicked-estate source (<name> | --symbols a,b | --cluster N | --file V)"
            ),
            "{e}"
        );
    }

    #[test]
    #[should_panic(expected = "which its row does not own")]
    fn reading_an_undeclared_flag_is_a_bug() {
        args("stats", &[]).str("kind");
    }
}
