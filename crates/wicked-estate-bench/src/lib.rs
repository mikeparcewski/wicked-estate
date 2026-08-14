//! `wicked-estate-bench` — the agent-eval benchmark harness (the truth oracle).
//!
//! W0.6 ships the *framework* + the frozen corpus; baseline numbers are recorded at W1.6 once
//! extraction exists. Methodology: `docs/benchmark-methodology.md`.
//!
//! W1.6 adds the capability benchmark: `RepoMetrics`, `CapabilityReport`, and `run_benchmark`
//! measure engine speed, completeness, query latency, and context-pack compactness on real
//! codebases without requiring an LLM in the loop.

pub mod capability;
pub mod community_metrics;
pub mod memory_recall;

pub use capability::{
    CapabilityReport, ConfidenceBands, LangMatrixRow, RepoMetrics, ResolverStats, run_benchmark,
    write_coverage_matrix,
};
pub use community_metrics::{CommunityMetrics, community_metrics};

use serde::{Deserialize, Serialize};

/// One repository in the benchmark corpus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSpec {
    pub name: String,
    pub language: String,
    pub git_url: String,
    pub rev: String,
}

/// A retrieval question with a known-good answer set (repo-relative files).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalTask {
    pub id: String,
    pub repo: String,
    pub question: String,
    pub gold_files: Vec<String>,
}

/// Metrics for one arm of the A/B (baseline = no tool; treatment = with wicked_estate).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArmMetrics {
    pub tool_calls: u32,
    pub files_read: u32,
    pub tokens_in: u64,
    /// Fraction of `gold_files` the agent surfaced, in `[0,1]`.
    pub answer_file_recall: f32,
}

/// One task's A/B outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutcome {
    pub task_id: String,
    pub baseline: ArmMetrics,
    pub treatment: ArmMetrics,
}

impl TaskOutcome {
    /// Headline metric: `baseline.tokens_in / treatment.tokens_in`.
    pub fn token_reduction(&self) -> f32 {
        if self.treatment.tokens_in == 0 {
            return 0.0;
        }
        self.baseline.tokens_in as f32 / self.treatment.tokens_in as f32
    }
}

/// Aggregate report across the corpus — the gate signal (W1.6 / W8.1).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvalReport {
    pub outcomes: Vec<TaskOutcome>,
}

impl EvalReport {
    pub fn mean_token_reduction(&self) -> f32 {
        if self.outcomes.is_empty() {
            return 0.0;
        }
        self.outcomes
            .iter()
            .map(TaskOutcome::token_reduction)
            .sum::<f32>()
            / self.outcomes.len() as f32
    }

    pub fn mean_file_recall_treatment(&self) -> f32 {
        if self.outcomes.is_empty() {
            return 0.0;
        }
        self.outcomes
            .iter()
            .map(|o| o.treatment.answer_file_recall)
            .sum::<f32>()
            / self.outcomes.len() as f32
    }
}

/// The frozen baseline corpus (W0.6): TypeScript, Python, and a polyglot repo.
pub fn baseline_corpus() -> Vec<RepoSpec> {
    vec![
        RepoSpec {
            name: "ts-axios".into(),
            language: "typescript".into(),
            git_url: "https://github.com/axios/axios".into(),
            rev: "v1.7.9".into(),
        },
        RepoSpec {
            name: "py-flask".into(),
            language: "python".into(),
            git_url: "https://github.com/pallets/flask".into(),
            rev: "3.1.0".into(),
        },
        RepoSpec {
            name: "poly-tree-sitter".into(),
            language: "polyglot".into(),
            git_url: "https://github.com/tree-sitter/tree-sitter".into(),
            rev: "HEAD".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_spans_ts_python_polyglot() {
        let c = baseline_corpus();
        assert_eq!(c.len(), 3);
        assert!(c.iter().any(|r| r.language == "typescript"));
        assert!(c.iter().any(|r| r.language == "python"));
        assert!(c.iter().any(|r| r.language == "polyglot"));
    }

    #[test]
    fn token_reduction_is_baseline_over_treatment() {
        let o = TaskOutcome {
            task_id: "t1".into(),
            baseline: ArmMetrics {
                tokens_in: 1000,
                ..Default::default()
            },
            treatment: ArmMetrics {
                tokens_in: 100,
                ..Default::default()
            },
        };
        assert!((o.token_reduction() - 10.0).abs() < 1e-6);
    }
}
