//! Parse and apply one atomic batch of CLI annotations and semantics (garden#1453, garden#1470).

use serde::{Deserialize, Deserializer, Serialize};
use std::io::BufRead;
use wicked_estate_core::{
    Annotation, DEFAULT_ANNOTATION_TYPE, Error, GraphRead, GraphWrite, Result, SymbolId,
};
use wicked_estate_store::SqliteStore;

/// A validated operation with its original NDJSON line number.
#[derive(Debug)]
pub struct Row {
    line: usize,
    operation: Operation,
}

#[derive(Debug)]
enum Operation {
    Annotate {
        symbol: SymbolId,
        annotation: Annotation,
        replace: bool,
    },
    Semantics {
        symbol: SymbolId,
        description: Option<String>,
        requirement: Option<String>,
        validated: Option<bool>,
        validated_by: Option<String>,
    },
}

impl Operation {
    fn symbol(&self) -> &SymbolId {
        match self {
            Self::Annotate { symbol, .. } | Self::Semantics { symbol, .. } => symbol,
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
enum Input {
    Annotate {
        #[serde(deserialize_with = "nonempty")]
        symbol: String,
        #[serde(deserialize_with = "nonempty")]
        key: String,
        #[serde(deserialize_with = "nonempty")]
        value: String,
        #[serde(default = "note_type", deserialize_with = "nonempty")]
        r#type: String,
        #[serde(default = "full_confidence")]
        confidence: f64,
        #[serde(default, deserialize_with = "nonempty")]
        provenance: String,
        #[serde(default, deserialize_with = "nonempty")]
        author: String,
        #[serde(default)]
        replace: bool,
    },
    Semantics {
        #[serde(deserialize_with = "nonempty")]
        symbol: String,
        #[serde(default, deserialize_with = "optional_nonempty")]
        description: Option<String>,
        #[serde(default, deserialize_with = "optional_nonempty")]
        requirement: Option<String>,
        #[serde(default, deserialize_with = "present_bool")]
        validated: Option<bool>,
        #[serde(default, deserialize_with = "optional_nonempty")]
        validated_by: Option<String>,
    },
}

fn nonempty<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.is_empty() {
        return Err(serde::de::Error::custom("strings must be non-empty"));
    }
    Ok(value)
}

fn optional_nonempty<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    nonempty(deserializer).map(Some)
}

fn present_bool<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<bool>, D::Error> {
    bool::deserialize(deserializer).map(Some)
}

fn note_type() -> String {
    DEFAULT_ANNOTATION_TYPE.to_string()
}

fn full_confidence() -> f64 {
    1.0
}

fn refused(line: usize, reason: impl std::fmt::Display) -> Error {
    Error::Invalid(format!(
        "write-batch refused: row {line}: {reason}; nothing was written"
    ))
}

impl Input {
    fn validate(self) -> std::result::Result<Operation, &'static str> {
        match self {
            Self::Annotate {
                symbol,
                key,
                value,
                r#type,
                confidence,
                provenance,
                author,
                replace,
            } => {
                if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
                    return Err("confidence must be finite and in 0..=1");
                }
                Ok(Operation::Annotate {
                    symbol: SymbolId(symbol),
                    annotation: Annotation::new(r#type, key, value)
                        .with_confidence(confidence)
                        .with_provenance(provenance)
                        .with_author(author),
                    replace,
                })
            }
            Self::Semantics {
                symbol,
                description,
                requirement,
                validated,
                validated_by,
            } => {
                if validated.is_some() != validated_by.is_some() {
                    return Err("validated and validated_by must be given together");
                }
                if description.is_none() && requirement.is_none() && validated.is_none() {
                    return Err(
                        "semantics needs at least one of description, requirement, validated",
                    );
                }
                // Use the same actor validation as set_semantics before opening the store.
                if let (Some(value), Some(by)) = (validated, validated_by.as_deref()) {
                    wicked_estate_core::ValidationClaim::new(value, by)
                        .map_err(|_| "validated_by must name a non-empty actor")?;
                }
                Ok(Operation::Semantics {
                    symbol: SymbolId(symbol),
                    description,
                    requirement,
                    validated,
                    validated_by,
                })
            }
        }
    }
}

/// Parse and validate every non-blank line without touching a store.
pub fn parse(reader: impl BufRead) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let number = index + 1;
        let line = line.map_err(|e| refused(number, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let input: Input = serde_json::from_str(&line).map_err(|e| refused(number, e))?;
        let operation = input.validate().map_err(|e| refused(number, e))?;
        rows.push(Row {
            line: number,
            operation,
        });
    }
    Ok(rows)
}

/// Why a write to `symbol` is refused when it is not a node: one text for every CLI writer.
fn unknown_symbol(symbol: &SymbolId) -> String {
    format!(
        "unknown symbol {}: not a node in this graph \
         (resolve a name with `wicked-estate resolve <name> --json`)",
        symbol.0,
    )
}

/// Require a node at the CLI boundary without changing the store's absent-symbol contract.
pub fn require_node(store: &dyn GraphRead, symbol: &SymbolId, command: &str) -> Result<()> {
    if store.get_node(symbol)?.is_none() {
        return Err(Error::Invalid(format!(
            "{command}: {}",
            unknown_symbol(symbol)
        )));
    }
    Ok(())
}

/// Counts of the committed operations and the prior annotations they replaced.
#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub rows: usize,
    pub annotations: usize,
    pub replaced: usize,
    pub semantics: usize,
}

/// Check every target and apply input order in one immediate transaction.
pub fn apply(store: &mut SqliteStore, rows: &[Row]) -> Result<Summary> {
    let Some(first) = rows.first() else {
        return Ok(Summary::default());
    };
    let mut line = first.line;
    let result = (|| {
        store.begin_batch()?;
        let mut unknown = None;
        let mut unknown_count = 0usize;
        for row in rows {
            line = row.line;
            if store.get_node(row.operation.symbol())?.is_none() {
                unknown.get_or_insert((row.line, unknown_symbol(row.operation.symbol())));
                unknown_count += 1;
            }
        }
        if let Some((number, reason)) = unknown {
            line = number;
            return Err(Error::Invalid(match unknown_count - 1 {
                0 => reason,
                others => format!("{reason}; {others} other row(s) name unknown ids"),
            }));
        }
        let mut summary = Summary::default();
        for row in rows {
            line = row.line;
            match &row.operation {
                Operation::Annotate {
                    symbol,
                    annotation,
                    replace,
                } => {
                    if *replace {
                        summary.replaced += store.delete_annotations(
                            symbol,
                            Some(&annotation.r#type),
                            &annotation.key,
                        )?;
                    }
                    store.annotate(symbol, annotation.clone())?;
                    summary.annotations += 1;
                }
                Operation::Semantics {
                    symbol,
                    description,
                    requirement,
                    validated,
                    validated_by,
                } => {
                    crate::set_semantics(
                        store,
                        &symbol.0,
                        description.as_deref(),
                        requirement.as_deref(),
                        *validated,
                        validated_by.as_deref(),
                    )?;
                    summary.semantics += 1;
                }
            }
            summary.rows += 1;
        }
        store.commit_batch()?;
        Ok(summary)
    })();
    match result {
        Ok(summary) => Ok(summary),
        Err(e) => {
            store
                .rollback_batch()
                .map_err(|rollback| refused(line, format!("{e}; rollback failed: {rollback}")))?;
            Err(refused(line, e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_defaults_and_tracks_input_lines() {
        let input = concat!(
            "\n",
            r#"{"op":"annotate","symbol":"id","key":"k","value":"v"}"#,
            "\n\n",
            r#"{"op":"semantics","symbol":"id","requirement":"R"}"#,
            "\n",
        );
        let rows = parse(input.as_bytes()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].line, 2);
        assert_eq!(rows[1].line, 4);
        let Operation::Annotate {
            annotation,
            replace,
            ..
        } = &rows[0].operation
        else {
            panic!("expected annotation");
        };
        assert_eq!(annotation.r#type, DEFAULT_ANNOTATION_TYPE);
        assert_eq!(annotation.confidence, 1.0);
        assert_eq!(annotation.provenance, "");
        assert_eq!(annotation.author, "");
        assert!(!*replace);
        assert!(parse(b"\n \n".as_slice()).unwrap().is_empty());
    }

    #[test]
    fn parser_refuses_invalid_rows_with_the_input_line() {
        for row in [
            "{",
            r#"{"op":"unknown","symbol":"id"}"#,
            r#"{"op":"annotate","symbol":"id","key":"k","value":"v","typo":1}"#,
            r#"{"op":"annotate","symbol":"id","key":"k","value":"v","confidence":1.5}"#,
            r#"{"op":"annotate","symbol":"id","key":"k","value":"v","confidence":-0.1}"#,
            r#"{"op":"annotate","symbol":"id","key":"k","value":"v","confidence":1e400}"#,
            r#"{"op":"annotate","symbol":"id","key":"k","value":"v","replace":null}"#,
            r#"{"op":"semantics","symbol":"id","validated":true}"#,
            r#"{"op":"semantics","symbol":"id","validated_by":"actor"}"#,
            r#"{"op":"semantics","symbol":"id","validated":false,"validated_by":" "}"#,
            r#"{"op":"semantics","symbol":"id","description":null}"#,
            r#"{"op":"semantics","symbol":"id","validated":null,"validated_by":"actor"}"#,
            r#"{"op":"semantics","symbol":"id","requirement":"R","typo":1}"#,
            r#"{"op":"semantics","symbol":"id"}"#,
        ] {
            let input = format!("\n{row}\n");
            let err = parse(input.as_bytes()).unwrap_err().to_string();
            assert!(err.contains("row 2"), "{row}: {err}");
            assert!(err.contains("nothing was written"), "{err}");
        }
    }

    #[test]
    fn parser_refuses_every_explicit_empty_string() {
        for field in ["symbol", "key", "value", "type", "provenance", "author"] {
            let mut row = serde_json::json!({
                "op":"annotate", "symbol":"id", "key":"k", "value":"v",
            });
            row[field] = serde_json::json!("");
            assert!(parse(row.to_string().as_bytes()).is_err(), "{field}");
        }
        for field in ["symbol", "description", "requirement", "validated_by"] {
            let mut row = serde_json::json!({
                "op":"semantics", "symbol":"id", "requirement":"R",
                "validated":false, "validated_by":"actor",
            });
            row[field] = serde_json::json!("");
            assert!(parse(row.to_string().as_bytes()).is_err(), "{field}");
        }
    }

    #[test]
    fn parser_accepts_a_false_validation_with_an_actor() {
        let input = r#"{"op":"semantics","symbol":"id","validated":false,"validated_by":"actor"}"#;
        assert_eq!(parse(input.as_bytes()).unwrap().len(), 1);
    }
}
