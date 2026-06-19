//! Excel/XLSX decision table extractor (W15.3) — **column-config-driven, calamine-backed**.
//!
//! Reads one or more named sheets from a `.xlsx` workbook and turns each data row into a
//! [`NodeKind::Rule`] node whose [`NodeKind::Condition`], [`NodeKind::Action`], and
//! [`NodeKind::Fact`] columns become child nodes connected by [`EdgeKind::Contains`] edges from
//! the Rule.  The whole sheet is wrapped in a single [`NodeKind::RuleSet`] node that owns the
//! Rules via [`EdgeKind::Governs`] edges.
//!
//! # Feature gate
//!
//! This module is compiled only when the `excel-rules` cargo feature is enabled:
//!
//! ```toml
//! wicked-estate-extract = { features = ["excel-rules"] }
//! ```
//!
//! # Configuration
//!
//! The extractor is driven by an [`ExcelRulesConfig`] value (typically deserialised from TOML).
//! Example:
//!
//! ```toml
//! [engine]
//! name   = "loan-decisioning"
//! file_globs = ["rules/*.xlsx"]
//!
//! [[sheets]]
//! ruleset_name = "LoanApproval"
//! header_row   = 0
//!
//! [[sheets.columns]]
//! index = 0
//! role  = "rule_name"
//!
//! [[sheets.columns]]
//! index = 1
//! role  = "condition"
//!
//! [[sheets.columns]]
//! index = 2
//! role  = "action"
//! ```
//!
//! # Node topology per sheet
//!
//! ```text
//! RuleSet("LoanApproval")
//!   ─[Governs]→ Rule("Rule_A")
//!                 ─[Contains]→ Condition("age > 18")
//!                 ─[Contains]→ Action("approve")
//!   ─[Governs]→ Rule("Rule_B")
//!                 ─[Contains]→ Condition("income < 50000")
//!                 ─[Contains]→ Action("deny")
//! ```
//!
//! # Symbol IDs
//!
//! All nodes are [`Symbol::Synthetic`] with scheme `"excel-rules"`.  The id encodes the file,
//! sheet, and row so IDs are stable across column re-ordering or row text edits in *other* rows.
//!
//! | Node       | Synthetic id                                  |
//! |------------|-----------------------------------------------|
//! | RuleSet    | `<file>::<sheet_name>::ruleset`               |
//! | Rule       | `<file>::<sheet_name>::row_<row_idx>`         |
//! | Condition  | `<file>::<sheet_name>::row_<row_idx>::cond_<col_idx>` |
//! | Action     | `<file>::<sheet_name>::row_<row_idx>::act_<col_idx>` |
//! | Fact       | `<file>::<sheet_name>::row_<row_idx>::fact_<col_idx>` |

#[cfg(feature = "excel-rules")]
mod inner {
    use calamine::{Data, Reader, Xlsx};
    use serde::{Deserialize, Serialize};
    use wicked_estate_core::{
        Edge, EdgeKind, Extraction, Extractor, Language, Location, Node, NodeKind, ResolutionTier,
        Result, SourceFile, Span, Symbol,
    };

    // ── Public configuration types ────────────────────────────────────────────

    /// Top-level config handed to [`ExcelRulesExtractor::new`].
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ExcelRulesConfig {
        /// Engine / source metadata (name + file-glob pattern, if any).
        pub engine: ExcelEngineConfig,
        /// One entry per sheet to extract.  At least one entry is required.
        pub sheets: Vec<SheetConfig>,
    }

    /// Engine-level metadata (embedded in node provenance / signatures).
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ExcelEngineConfig {
        /// A human-readable name for the rule engine or workbook (e.g. `"loan-decisioning"`).
        pub name: String,
        /// Glob patterns that match the workbooks this extractor should handle.  Informational
        /// only — the extractor does not glob itself; the caller passes [`SourceFile`]s.
        #[serde(default)]
        pub file_globs: Vec<String>,
    }

    /// Configuration for a single sheet within the workbook.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SheetConfig {
        /// Sheet name to open.  `None` → use the first sheet in the workbook.
        #[serde(default)]
        pub sheet_name: Option<String>,
        /// Zero-indexed row number of the header row (default `0` = first row).
        #[serde(default)]
        pub header_row: usize,
        /// The name to give the [`NodeKind::RuleSet`] node emitted for this sheet.
        pub ruleset_name: String,
        /// Column role declarations.  Columns not listed here are silently skipped.
        pub columns: Vec<ColumnConfig>,
    }

    /// A single column declaration.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ColumnConfig {
        /// Zero-indexed column index within the sheet.
        pub index: usize,
        /// The semantic role this column plays in each rule row.
        pub role: ColumnRole,
    }

    /// The semantic role a column plays in the decision table.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum ColumnRole {
        /// The primary name for the rule row → emitted as the [`NodeKind::Rule`] node's name.
        RuleName,
        /// A condition (guard) column → emitted as a [`NodeKind::Condition`] child node.
        Condition,
        /// An action (consequence) column → emitted as a [`NodeKind::Action`] child node.
        Action,
        /// A fact (domain object) column → emitted as a [`NodeKind::Fact`] child node.
        Fact,
    }

    // ── Extractor ─────────────────────────────────────────────────────────────

    /// Extracts decision tables from Excel/XLSX workbooks.
    ///
    /// Each configured sheet produces:
    /// - one [`NodeKind::RuleSet`] node,
    /// - one [`NodeKind::Rule`] node per non-empty data row,
    /// - one child node (`Condition` / `Action` / `Fact`) per configured non-`RuleName` column,
    /// - [`EdgeKind::Governs`] edges from the `RuleSet` to each `Rule`,
    /// - [`EdgeKind::Contains`] edges from each `Rule` to its child nodes.
    pub struct ExcelRulesExtractor {
        config: ExcelRulesConfig,
    }

    impl ExcelRulesExtractor {
        pub fn new(config: ExcelRulesConfig) -> Self {
            Self { config }
        }
    }

    impl Extractor for ExcelRulesExtractor {
        fn languages(&self) -> Vec<Language> {
            // The extractor is file-type-driven, not grammar-driven; advertise `xlsx`.
            vec![Language::new("xlsx")]
        }

        fn extract(&self, file: &SourceFile) -> Result<Extraction> {
            // XLSX is a binary ZIP-based format.  `SourceFile.text` is designed for text source
            // files and cannot reliably carry binary data.  This extractor therefore opens the
            // workbook from the filesystem path recorded in `file.path`.  In tests, pass the
            // absolute path to the fixture file and leave `file.text` empty.
            let mut workbook: Xlsx<_> = calamine::open_workbook::<Xlsx<_>, _>(&file.path)
                .map_err(|e| wicked_estate_core::Error::Extraction(e.to_string()))?;

            let mut nodes: Vec<Node> = Vec::new();
            let mut local_edges: Vec<Edge> = Vec::new();

            for sheet_cfg in &self.config.sheets {
                extract_sheet(
                    &mut workbook,
                    sheet_cfg,
                    &file.path,
                    &mut nodes,
                    &mut local_edges,
                )?;
            }

            Ok(Extraction {
                nodes,
                local_edges,
                refs: Vec::new(),
            })
        }
    }

    // ── Sheet extraction ──────────────────────────────────────────────────────

    fn extract_sheet<R: std::io::Read + std::io::Seek>(
        workbook: &mut Xlsx<R>,
        cfg: &SheetConfig,
        file_path: &str,
        nodes: &mut Vec<Node>,
        edges: &mut Vec<Edge>,
    ) -> Result<()> {
        // Determine the sheet name to open.
        let sheet_name: String = match &cfg.sheet_name {
            Some(n) => n.clone(),
            None => workbook.sheet_names().first().cloned().ok_or_else(|| {
                wicked_estate_core::Error::Extraction("workbook has no sheets".into())
            })?,
        };

        let range = workbook
            .worksheet_range(&sheet_name)
            .map_err(|e| wicked_estate_core::Error::Extraction(e.to_string()))?;

        // The RuleSet node — one per sheet.
        let ruleset_id =
            Symbol::synthetic("excel-rules", format!("{file_path}::{sheet_name}::ruleset")).id();
        let ruleset_node = Node::new(
            ruleset_id.clone(),
            NodeKind::RuleSet,
            cfg.ruleset_name.clone(),
            Language::new("xlsx"),
            Location::new(file_path, Span::ZERO),
        );
        nodes.push(ruleset_node);

        // Identify which column index holds the rule name.
        let rule_name_col = cfg
            .columns
            .iter()
            .find(|c| c.role == ColumnRole::RuleName)
            .map(|c| c.index);

        // Iterate over all rows, skipping the header row.
        for (row_offset, row) in range.rows().enumerate() {
            // Skip the header row(s).
            if row_offset <= cfg.header_row {
                continue;
            }
            let data_row_idx = row_offset; // preserve original sheet row offset for stable IDs

            // Determine the rule name (from the designated column, or a synthetic fallback).
            let rule_name = match rule_name_col {
                Some(col) => {
                    let cell_str = cell_string(row.get(col));
                    if cell_str.is_empty() {
                        // Skip rows where the rule-name cell is empty.
                        continue;
                    }
                    cell_str
                }
                None => format!("row_{data_row_idx}"),
            };

            // Rule node.
            let rule_id = Symbol::synthetic(
                "excel-rules",
                format!("{file_path}::{sheet_name}::row_{data_row_idx}"),
            )
            .id();
            let rule_node = Node::new(
                rule_id.clone(),
                NodeKind::Rule,
                rule_name,
                Language::new("xlsx"),
                Location::new(file_path, Span::ZERO),
            );
            nodes.push(rule_node);

            // RuleSet –[Governs]→ Rule
            edges.push(Edge::new(
                ruleset_id.clone(),
                rule_id.clone(),
                EdgeKind::Governs,
                ResolutionTier::Parsed,
                "excel-rules",
            ));

            // Child nodes for each non-RuleName column.
            for col_cfg in &cfg.columns {
                if col_cfg.role == ColumnRole::RuleName {
                    continue;
                }
                let cell_str = cell_string(row.get(col_cfg.index));
                if cell_str.is_empty() {
                    continue;
                }

                let (child_kind, id_tag) = match col_cfg.role {
                    ColumnRole::Condition => (NodeKind::Condition, "cond"),
                    ColumnRole::Action => (NodeKind::Action, "act"),
                    ColumnRole::Fact => (NodeKind::Fact, "fact"),
                    ColumnRole::RuleName => unreachable!(),
                };

                let child_id = Symbol::synthetic(
                    "excel-rules",
                    format!(
                        "{file_path}::{sheet_name}::row_{data_row_idx}::{id_tag}_{}",
                        col_cfg.index
                    ),
                )
                .id();

                let child_node = Node::new(
                    child_id.clone(),
                    child_kind,
                    cell_str,
                    Language::new("xlsx"),
                    Location::new(file_path, Span::ZERO),
                );
                nodes.push(child_node);

                // Rule –[Contains]→ child
                edges.push(Edge::new(
                    rule_id.clone(),
                    child_id,
                    EdgeKind::Contains,
                    ResolutionTier::Parsed,
                    "excel-rules",
                ));
            }
        }

        Ok(())
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Extract a printable string from a calamine [`Data`] cell, returning `""` for empty/null.
    fn cell_string(cell: Option<&Data>) -> String {
        match cell {
            None => String::new(),
            Some(Data::Empty) => String::new(),
            Some(Data::String(s)) => s.clone(),
            Some(Data::Float(f)) => f.to_string(),
            Some(Data::Int(i)) => i.to_string(),
            Some(Data::Bool(b)) => b.to_string(),
            Some(Data::Error(_)) => String::new(),
            Some(Data::DateTime(f)) => f.to_string(),
            Some(Data::DateTimeIso(s)) => s.clone(),
            Some(Data::DurationIso(s)) => s.clone(),
        }
    }
}

// ── Re-export under feature flag ──────────────────────────────────────────────

#[cfg(feature = "excel-rules")]
pub use inner::{
    ColumnConfig, ColumnRole, ExcelEngineConfig, ExcelRulesConfig, ExcelRulesExtractor, SheetConfig,
};
