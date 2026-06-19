# W15 Rules Engine Layer — Delivery Receipt

Date: 2026-06-19 (Round 2: #21–#24); 2026-06-18 (Round 1: #14–#27)
Branch: `feat/drl-extractor` (PR #34) → main
Reviewer: Antagonist agents — Round 1 `ab027ede7c6d0437b`, Round 2 `ad95c17269bc1a350` (independent reviews, no shared context)

## Delivered

| Issue | Feature | Commit | Status |
|-------|---------|--------|--------|
| #14 | W15.1 NodeKind/EdgeKind: Rule, RuleSet, Condition, Action, Fact + Governs, Evaluates, Produces, InvokedBy | d3c340e | CLOSED |
| #15 | W15.2 AWS Config / Azure Policy JSON rules | 617f1e4 | CLOSED |
| #16 | W15.4 Camunda DMN extractor | f64329f | CLOSED |
| #17 | W15.12 IBM ODM BAL/IRL regex extractor | 43c0d90 | CLOSED |
| #18 | W15.13 Rules bridge resolver (ExtraEdgeExtractor + RulesBridgeResolver) | b2f6e5d | CLOSED |
| #19 | W15.6 Salesforce Flow extractor | 37e1027 | CLOSED |
| #20 | W15.7 CLIPS/Jess extractor | 8d9b800 | CLOSED |
| #25 | W15 guard/test hardening | 802e9b2 | CLOSED |
| #26 | W15 CI/release workflows | 802e9b2 | CLOSED |
| #27 | W15 ExtraEdgeExtractor W15 EdgeKind/NodeKind | b2f6e5d | CLOSED |
| #21 | W15.9 Drools DRL `.drl` heuristic extractor (regex; package→RuleSet, rule/when/then→Rule/Condition/Action, declare→Fact) | PR #34 | CLOSED |
| #22 | W15.8 OPA/Rego `.rego` rules-layer extractor (supplements the arborium-rego tree-sitter code parse with RuleSet/Rule/Condition/Action/Fact) | PR #34 | CLOSED |
| #23 | W15.10 Progress Corticon `.ers`/`.ecore` extractor (dedicated roxmltree pass against the real EMF/XMI structure from `corticon/corticon-classic-samples`) | PR #34 | CLOSED |
| #24 | W15.11 FICO Blaze `.brl` SRL heuristic extractor; `.xls` via existing W15.3 Excel infra; `.rma` specimen-blocked | PR #34 | PARTIAL |

### Round 2 — methodology: plan → execute → review → resolve → antagonist → resolve

All four were originally marked "blocked." Re-assessment found the blockers were
overstated: DRL/Rego/Blaze syntaxes are publicly documented, and Corticon ships a
**public** sample corpus (`corticon/corticon-classic-samples`) with real `.ers`
files — so faithful extractors were built rather than deferred. All four are wired
into `index` dispatch and verified end-to-end (functional test below). Mechanism
deviations from the issue ACs (DRL/Blaze use regex not tree-sitter; Corticon uses a
dedicated roxmltree pass not the generic TOML config; Blaze uses a bespoke
`Extractor` not `ExtraEdgeExtractor`) are deliberate and consistent with the
existing ODM/CLIPS heuristic extractors — the generic element→kind config cannot
name a node from a grandchild attribute nor emit Corticon's nameless positional
`<rule>` rows. A future tree-sitter grammar can supersede any of these behind the
same `NodeKind`s with no downstream change.

## Antagonist Review Findings → Resolutions

| Finding | Severity | Fix Commit |
|---------|----------|-----------|
| W15.13 synthetic SymbolId mismatch — dangling edges | CRITICAL | 8c8cb39, 81b1e42 |
| label_capture on pattern with no captures | CRITICAL | 8c8cb39 |
| CamundaDmnExtractor::new() returns Result<Self> | IMPORTANT | 32b2203 |
| odm.rs BAL Condition/Action line_span stale | IMPORTANT | 8c8cb39 |
| dmn.rs zero tests | IMPORTANT | 32b2203 |
| CLIPS/Jess in worktree only | IMPORTANT | 8d9b800 |

## Round 2 — Antagonist Review Findings → Resolutions (PR #34)

Independent adversarial review (general-purpose agent, no shared context) of the
four new extractors. Found a BLOCKER + a family of comment/string-blindness bugs.

| Finding | Severity | Resolution |
|---------|----------|-----------|
| B1: Corticon never dispatched during `index` (tested-but-never-run; not compiled into indexer) | BLOCKER | Feature-gated dispatch `.ers`/`.erf`/`.ecore`→Corticon, `.dmn`→DMN via grammarless path; new `xml-rules`/`excel-rules` features on the wicked-estate crate. Functional test confirms end-to-end. |
| M1: DRL `rule`/`declare` inside comments → false-positive nodes | MAJOR | `crate::rules_text::blank_c_comments` before matching |
| M4 Rego / M3 Blaze: `}` inside a string truncates the body | MAJOR | string-aware `match_brace_end` over a masked copy |
| M5: Rego idiomatic multi-line `allow\n{ … }` produced zero nodes | MAJOR | next-non-blank-line brace detection |
| M2: Blaze `then` inside a string splits condition/action | MAJOR | structural scan over string-masked copy |
| m1: Rego `import data.x` misclassified as a Fact | MINOR | skip `import` lines in the ref scan |
| m2: Rego package-less fragment dropped all Facts | MINOR | emit Facts regardless; edge only when a RuleSet exists |
| m3: DRL rule missing `end` swallows the next rule | MINOR | bound body by the next `rule`/`declare` header |
| m4: Corticon only first `<ruleset>` vocabulary → Fact | MINOR | iterate all `<ruleset>` (deduped) |

Each finding has a dedicated regression test (+15 total) that reproduces it.

## Specimen-blocked (residual, external dependency)

| Track | Reason |
|-------|--------|
| #24 FICO Blaze `.rma` project XML | Proprietary serialization; zero public specimens (verified via GitHub code search). W15.2 `XmlRulesExtractor` is ready to onboard a config + fixture once a specimen is provided. |

(#24 `.xls` decision tables are handled by the existing W15.3 `ExcelRulesExtractor`
— a Blaze decision table is a generic decision table + a column config; the Excel
suite already proves this. Only `.rma` remains genuinely blocked.)

## Gates at delivery (Round 2 — PR #34)

- `cargo fmt --all -- --check`: clean
- `cargo test --workspace` (default features): 0 failed
- `cargo test -p wicked-estate-extract --features xml-rules,excel-rules --lib`: 356 passed, 0 failed
- `cargo clippy --workspace --all-targets` `RUSTFLAGS=-D warnings` (default + rules features): 0 warnings
- `cargo build --release --features xml-rules,excel-rules`: ok
- **Functional test**: `wicked-estate index` of a dir with `.drl`/`.rego`/`.brl`/`.ers`/`.ecore` →
  `Rule Approve high score` (DRL), `Rule allow` + `Function allow` (Rego rules + code layers),
  `Rule HighBalance` (Blaze), `RuleSet Maintenance_Change_Tires` (Corticon), `Fact Applicant` (DRL) —
  all extractors produce nodes end-to-end; `evaluates`+`produces` edges confirm Corticon dispatch.
- Antagonist review: BLOCKER + 8 findings → all resolved → PASS

## Gates at delivery (Round 1)

- `cargo test --workspace`: 851 passed, 0 failed (SHA 1b75127)
- `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings
- Antagonist review: CONDITIONAL → all findings resolved → PASS
