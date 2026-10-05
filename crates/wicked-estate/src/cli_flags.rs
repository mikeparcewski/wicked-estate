//! Per-command flag ownership for `main`'s bespoke dispatch arms (#197, #206).
//!
//! `main`'s shared parser recognises the union of every arm's flags and pushes anything else
//! into `positional`; each arm then reads only the flags it owns. A flag the arm never reads —
//! a typo, or one another command owns (`nodes --symbol <id>`) — was accepted and ignored: the
//! command ran on its defaults and exited 0 with a plausible wrong answer (61,182 rows for
//! `nodes --bogus-flag zzz`). [`check`] runs before that parser and rejects any flag the
//! dispatched command does not own, closing the accept-and-ignore path for every bespoke arm
//! at the one seam they share.
//!
//! The table is ownership and arity only, so a value is never mistaken for a flag. Parsing,
//! defaults and value coercion stay in the shared parser and the arms; a row must list exactly
//! the flags its arm reads. Bridged commands (`tool_bridge`) never reach here, and `lineage`
//! parses its own argv strictly.

/// How a flag consumes argv.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Arity {
    /// `--flag`.
    Switch,
    /// `--flag V`: the next token, which must not itself be a `--flag`.
    Value,
    /// `--flag V` or `--flag=V`: the owning parser handles the inline form too.
    ValueOrInline,
}

#[derive(Debug, Clone, Copy)]
pub struct Flag(pub &'static str, pub Arity);

#[derive(Debug)]
pub enum Spec {
    /// Owns exactly these flags (plus `-h`/`--help`). `operands` is the usage placeholder.
    Owns {
        operands: &'static str,
        flags: &'static [Flag],
    },
    /// Parses its own argv strictly; [`check`] leaves it alone.
    SelfParsed,
}

#[derive(Debug)]
pub struct Command {
    pub name: &'static str,
    pub spec: Spec,
}

use Arity::{Switch, Value, ValueOrInline};

const DB: Flag = Flag("db", Value);
const JSON: Flag = Flag("json", Switch);
const REPO: Flag = Flag("repo", ValueOrInline);
const AS: Flag = Flag("as", ValueOrInline);
const HISTORY: Flag = Flag("history", Switch);
const KIND: Flag = Flag("kind", Value);

const fn owns(operands: &'static str, flags: &'static [Flag]) -> Spec {
    Spec::Owns { operands, flags }
}

/// Every bespoke command and the flags its arm reads.
pub const COMMANDS: &[Command] = &[
    Command {
        name: "index",
        spec: owns(
            "<path>",
            &[
                DB,
                REPO,
                AS,
                HISTORY,
                Flag("embeddings", Switch),
                Flag("force", Switch),
            ],
        ),
    },
    Command {
        name: "scip",
        spec: owns("<root>", &[DB, REPO, AS, Flag("scip-file", Value)]),
    },
    Command {
        name: "tfstate",
        spec: owns("<file>", &[DB]),
    },
    Command {
        name: "import-telemetry",
        spec: owns("<file.json>", &[DB]),
    },
    Command {
        name: "drift",
        spec: owns("", &[DB]),
    },
    Command {
        name: "query",
        spec: owns("<name>", &[DB]),
    },
    Command {
        name: "blast-radius",
        spec: owns("<name>", &[DB, JSON, Flag("depth", ValueOrInline)]),
    },
    Command {
        name: "path",
        spec: owns("<from> <to>", &[DB, JSON, Flag("max-depth", Value)]),
    },
    Command {
        name: "lineage",
        spec: Spec::SelfParsed,
    },
    Command {
        name: "stats",
        spec: owns("", &[DB]),
    },
    Command {
        name: "graph-view",
        spec: owns(
            "",
            &[
                DB,
                Flag("limit", Value),
                Flag("focus", Value),
                Flag("include-tests", Switch),
                Flag("include-trivial", Switch),
                Flag("ignore", Value),
            ],
        ),
    },
    Command {
        name: "source",
        spec: owns(
            "[<name>]",
            &[
                DB,
                JSON,
                Flag("symbols", Value),
                Flag("cluster", Value),
                Flag("file", Value),
                Flag("max-total-chars", Value),
                Flag("max-node-chars", Value),
                Flag("signatures-only", Switch),
            ],
        ),
    },
    Command {
        name: "semantic",
        spec: owns("<query>", &[DB]),
    },
    Command {
        name: "cross-graph",
        spec: owns("<name>", &[DB, Flag("dbs", Value)]),
    },
    Command {
        name: "compact",
        spec: owns("", &[DB]),
    },
    Command {
        name: "watch",
        spec: owns("<path>", &[DB, REPO, AS, HISTORY]),
    },
    Command {
        name: "subscribe",
        spec: owns("", &[DB, Flag("since", Value)]),
    },
    Command {
        name: "semantics",
        spec: owns(
            "<symbol>",
            &[
                DB,
                Flag("description", Value),
                Flag("requirement", Value),
                Flag("validated", Value),
                Flag("validated-by", Value),
            ],
        ),
    },
    Command {
        name: "by-requirement",
        spec: owns("<requirement>", &[DB]),
    },
    Command {
        name: "clusters",
        spec: owns(
            "[<min-size>]",
            &[
                DB,
                JSON,
                Flag("annotate", Switch),
                Flag("summary", Switch),
                Flag("hierarchical", Switch),
                Flag("resolution", Value),
                Flag("package-bias", Value),
                Flag("weight", Value),
                Flag("k", Value),
                Flag("eps", Value),
                Flag("min-pts", Value),
            ],
        ),
    },
    Command {
        name: "context",
        spec: owns("<name>", &[DB, JSON, Flag("budget", Value)]),
    },
    Command {
        name: "annotate",
        spec: owns(
            "<name>",
            &[
                DB,
                Flag("symbol", Value),
                Flag("key", Value),
                Flag("value", Value),
                Flag("type", Value),
                Flag("confidence", Value),
                Flag("provenance", Value),
                Flag("author", Value),
                Flag("replace", Switch),
            ],
        ),
    },
    Command {
        name: "annotations",
        spec: owns(
            "<name>",
            &[DB, JSON, Flag("symbol", Value), Flag("type", Value)],
        ),
    },
    Command {
        name: "stale-annotations",
        spec: owns("<cutoff>", &[DB, JSON]),
    },
    Command {
        name: "fingerprint",
        spec: owns("<name>", &[DB, Flag("content", Switch)]),
    },
    Command {
        name: "changed-since",
        spec: owns("<sha>", &[DB, JSON]),
    },
    Command {
        name: "entrypoints",
        spec: owns("", &[DB, JSON]),
    },
    Command {
        name: "leaves",
        spec: owns("", &[DB, JSON]),
    },
    Command {
        name: "dead-code",
        spec: owns("", &[DB, JSON]),
    },
    Command {
        name: "nodes",
        spec: owns(
            "",
            &[
                DB,
                JSON,
                KIND,
                Flag("annotated-with", Value),
                Flag("semantics", Switch),
            ],
        ),
    },
    Command {
        name: "resolve",
        spec: owns("<name>", &[DB, JSON, KIND, Flag("file", Value)]),
    },
    Command {
        name: "correspond",
        spec: owns(
            "",
            &[
                JSON,
                KIND,
                Flag("db-a", Value),
                Flag("db-b", Value),
                Flag("top", Value),
                Flag("min-score", Value),
                Flag("explain", Switch),
            ],
        ),
    },
    Command {
        name: "export",
        spec: owns(
            "",
            &[
                DB,
                Flag("format", Value),
                Flag("nodes-only", Switch),
                Flag("edges-only", Switch),
            ],
        ),
    },
    Command {
        name: "plugins",
        spec: owns("list", &[]),
    },
];

pub fn lookup(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// `-x` and `--x` are flags; `-`, `-1` and anything without a leading dash are operands.
fn is_flag(a: &str) -> bool {
    a.starts_with("--")
        || a.len() > 1 && a.starts_with('-') && a.as_bytes()[1].is_ascii_alphabetic()
}

/// `nodes [--db V] [--json] [--kind V] …`
fn usage(name: &str, operands: &str, flags: &[Flag]) -> String {
    let mut s = format!("usage: wicked-estate {name}");
    if !operands.is_empty() {
        s.push_str(&format!(" {operands}"));
    }
    for Flag(f, arity) in flags {
        match arity {
            Switch => s.push_str(&format!(" [--{f}]")),
            Value | ValueOrInline => s.push_str(&format!(" [--{f} V]")),
        }
    }
    s
}

/// Reject any flag `cmd` does not own, before the shared parser can swallow it. `Ok` for a
/// command not in the table (the usage arm reports it), a self-parsing one, or a help request
/// (help wins, as it did before this check existed). `Err` is the full usage message.
pub fn check(cmd: &str, args: &[String]) -> Result<(), String> {
    let Some(Command {
        spec: Spec::Owns { operands, flags },
        ..
    }) = lookup(cmd)
    else {
        return Ok(());
    };
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(());
    }
    let fail = |why: String| Err(format!("{}\n{why}", usage(cmd, operands, flags)));
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if !is_flag(a) {
            continue;
        }
        let Some(body) = a.strip_prefix("--") else {
            return fail(format!("unknown flag {a:?}"));
        };
        let (name, inline) = match body.split_once('=') {
            Some((n, v)) => (n, Some(v)),
            None => (body, None),
        };
        let Some(Flag(_, arity)) = flags.iter().find(|f| f.0 == name) else {
            let owners: Vec<&str> = COMMANDS
                .iter()
                .filter(|c| matches!(&c.spec, Spec::Owns { flags, .. } if flags.iter().any(|f| f.0 == name)))
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
        match (arity, inline) {
            (Switch, Some(_)) => return fail(format!("--{name} takes no value")),
            (Switch, None) | (ValueOrInline, Some(_)) => {}
            // The shared parser matches `--file` exactly, so `--file=x` was dropped whole.
            (Value, Some(_)) => {
                return fail(format!(
                    "--{name}=… is not accepted; write --{name} <value>"
                ));
            }
            // The shared parser takes the next token unconditionally, so `--symbols --db x`
            // made `--db` the selector and dropped the store. Refused, as the bridge, `lineage`
            // and `--repo` already refuse it.
            (Value | ValueOrInline, None) => match it.next() {
                None => return fail(format!("--{name} requires a value")),
                Some(v) if v.starts_with("--") => {
                    return fail(format!("--{name} requires a value, got the flag {v:?}"));
                }
                Some(_) => {}
            },
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_args(cmd: &str, a: &[&str]) -> Result<(), String> {
        check(cmd, &a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn the_table_is_well_formed() {
        for (i, c) in COMMANDS.iter().enumerate() {
            assert!(
                COMMANDS[..i].iter().all(|d| d.name != c.name),
                "{} listed twice",
                c.name
            );
            // A bridged command never reaches the shared parser, so a row here would be dead.
            assert!(crate::tool_bridge::lookup(c.name).is_none(), "{}", c.name);
            if let Spec::Owns { flags, .. } = &c.spec {
                for (j, f) in flags.iter().enumerate() {
                    assert!(
                        flags[..j].iter().all(|g| g.0 != f.0),
                        "{}: --{} listed twice",
                        c.name,
                        f.0
                    );
                }
            }
        }
    }

    #[test]
    fn unknown_and_foreign_flags_are_rejected_197() {
        let e = check_args("nodes", &["--bogus-flag", "zzz"]).unwrap_err();
        assert!(e.starts_with("usage: wicked-estate nodes"), "{e}");
        assert!(e.ends_with("unknown flag \"--bogus-flag\""), "{e}");
        // Parsed by the shared loop for `annotate`; `nodes` never read it.
        let e = check_args("nodes", &["--symbol", "id"]).unwrap_err();
        assert!(e.contains("accepted by: annotate, annotations"), "{e}");
        assert!(check_args("path", &["a", "b", "--top", "5"]).is_err());
        assert!(check_args("correspond", &["--db", "x.db"]).is_err());
        assert!(check_args("nodes", &["-x"]).is_err());
        assert!(check_args("nodes", &["--"]).is_err());
    }

    #[test]
    fn owned_flags_values_and_operands_pass() {
        assert!(check_args("nodes", &["--kind", "Function", "--json", "--semantics"]).is_ok());
        // A value is consumed, so one that merely starts with a dash is not checked as a flag.
        assert!(check_args("annotate", &["f", "--value", "-x", "--key", "k"]).is_ok());
        assert!(check_args("blast-radius", &["f", "--depth=3"]).is_ok());
        assert!(check_args("index", &[".", "--repo=ledger", "--db", ":memory:"]).is_ok());
        assert!(check_args("cross-graph", &["f", "--db", "a", "--db", "b"]).is_ok());
        // Operands may look numeric-negative or be a lone dash.
        assert!(check_args("clusters", &["-1"]).is_ok());
        assert!(check_args("query", &["-"]).is_ok());
    }

    #[test]
    fn arity_is_enforced() {
        assert!(
            check_args("source", &["f", "--file"])
                .unwrap_err()
                .ends_with("--file requires a value")
        );
        // `--file=x` matched no shared-parser arm, so it was dropped whole.
        assert!(
            check_args("source", &["--file=a.py"])
                .unwrap_err()
                .contains("write --file <value>")
        );
        assert!(
            check_args("source", &["--symbols", "--db", "x.db"])
                .unwrap_err()
                .ends_with("--symbols requires a value, got the flag \"--db\"")
        );
        assert!(
            check_args("nodes", &["--json=1"])
                .unwrap_err()
                .ends_with("--json takes no value")
        );
    }

    #[test]
    fn help_unknown_commands_and_self_parsers_pass_through() {
        assert!(check_args("nodes", &["--bogus", "--help"]).is_ok());
        assert!(check_args("no-such-command", &["--bogus"]).is_ok());
        // `lineage` rejects unknown flags itself, with its own usage.
        assert!(check_args("lineage", &["--bogus"]).is_ok());
    }
}
