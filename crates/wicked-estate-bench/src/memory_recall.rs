//! Memory recall benchmark — estate#88.
//!
//! Captures 25 developer-fact memories and measures keyword-FTS recall@5 against a
//! deterministic QA dataset. Gate: recall@5 ≥ 0.80.
//!
//! The dataset mirrors LongMemEval/LoCoMo patterns at small scale: each stored fact is a
//! single atomic statement; each query contains the anchor keyword from that statement so
//! BM25/FTS5 can retrieve it. With the default `HashEmbedder` (no real semantic model),
//! vector recall is disabled and the benchmark exercises the keyword path only.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use wicked_estate_memory::{MemoryEngine, RecallMode, ScopeFilter};
use wicked_estate_memory_core::{MemKind, Memory, Scope, Tier};

/// One QA pair in the benchmark dataset.
pub struct RecallQa {
    /// Content stored as a memory fact.
    pub content: &'static str,
    /// Recall query (intentionally shares keywords with content).
    pub query: &'static str,
    /// Substring that must appear in at least one of the top-k recalled items.
    pub answer_kw: &'static str,
}

pub const QA_DATASET: &[RecallQa] = &[
    RecallQa {
        content: "Kong handles API rate limiting and authentication for the gateway.",
        query: "What handles API rate limiting?",
        answer_kw: "Kong",
    },
    RecallQa {
        content: "PagerDuty manages the on-call rotation schedule for all teams.",
        query: "Which tool manages the on-call rotation?",
        answer_kw: "PagerDuty",
    },
    RecallQa {
        content: "Database migrations execute every Friday at 10 PM UTC.",
        query: "When do database migrations run?",
        answer_kw: "Friday",
    },
    RecallQa {
        content: "The build pipeline stores artifacts in S3 bucket ci-artifacts-prod.",
        query: "Where are build artifacts stored?",
        answer_kw: "S3",
    },
    RecallQa {
        content: "React is pinned at version 18.2.0 to avoid breaking changes in 18.3.",
        query: "Which React version is pinned?",
        answer_kw: "18.2",
    },
    RecallQa {
        content: "PostgreSQL is the primary transactional database.",
        query: "What is the primary database?",
        answer_kw: "PostgreSQL",
    },
    RecallQa {
        content: "Redis handles session caching and rate counters across all services.",
        query: "What handles session caching?",
        answer_kw: "Redis",
    },
    RecallQa {
        content: "Terraform manages cloud infrastructure provisioning for all environments.",
        query: "What manages infrastructure provisioning?",
        answer_kw: "Terraform",
    },
    RecallQa {
        content: "GraphQL is the API query language for the customer-facing endpoints.",
        query: "What API query language is used for customer endpoints?",
        answer_kw: "GraphQL",
    },
    RecallQa {
        content: "DataDog is used for application performance monitoring and alerting.",
        query: "Which tool is used for performance monitoring?",
        answer_kw: "DataDog",
    },
    RecallQa {
        content: "Go 1.22 is the minimum supported language version for all backend services.",
        query: "What is the minimum supported Go version?",
        answer_kw: "1.22",
    },
    RecallQa {
        content: "Jest is the test framework for JavaScript unit tests.",
        query: "What is the JavaScript test framework?",
        answer_kw: "Jest",
    },
    RecallQa {
        content: "Sentry captures production errors and crash reports from all services.",
        query: "Which tool captures production errors?",
        answer_kw: "Sentry",
    },
    RecallQa {
        content: "Kafka is the message broker for async event processing across services.",
        query: "What message broker is used for async events?",
        answer_kw: "Kafka",
    },
    RecallQa {
        content: "Elasticsearch backs the full-text search feature for the search API.",
        query: "What backs full-text search?",
        answer_kw: "Elasticsearch",
    },
    RecallQa {
        content: "Docker images use multi-stage builds to reduce the final image size.",
        query: "How are Docker images built?",
        answer_kw: "multi-stage",
    },
    RecallQa {
        content: "GitHub Actions runs the CI/CD pipeline on every pull request and main branch push.",
        query: "What runs the CI/CD pipeline?",
        answer_kw: "GitHub Actions",
    },
    RecallQa {
        content: "Nginx is the reverse proxy and load balancer in the production environment.",
        query: "What is the production reverse proxy?",
        answer_kw: "Nginx",
    },
    RecallQa {
        content: "JWT tokens expire after 24 hours; refresh tokens expire after 30 days.",
        query: "How long do JWT tokens last?",
        answer_kw: "JWT",
    },
    RecallQa {
        content: "The staging environment mirrors production except it uses a smaller RDS instance.",
        query: "How does the staging environment differ from production?",
        answer_kw: "staging",
    },
    RecallQa {
        content: "Vault by HashiCorp stores all secrets; no secrets are committed to version control.",
        query: "Where are secrets stored?",
        answer_kw: "Vault",
    },
    RecallQa {
        content: "Code reviews require at least two approvals before merging to the main branch.",
        query: "How many approvals are needed for a code review?",
        answer_kw: "two",
    },
    RecallQa {
        content: "The billing module integrates with Stripe for payment processing.",
        query: "Which payment processor is integrated with the billing module?",
        answer_kw: "Stripe",
    },
    RecallQa {
        content: "Prometheus scrapes metrics every 15 seconds; Grafana visualizes the dashboards.",
        query: "What visualizes Prometheus metrics?",
        answer_kw: "Grafana",
    },
    RecallQa {
        content: "The mobile app is built with React Native and Expo for cross-platform development.",
        query: "What framework is the mobile app built with?",
        answer_kw: "React Native",
    },
];

/// recall@k benchmark result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallReport {
    pub k: usize,
    pub hits: usize,
    pub total: usize,
    pub recall_at_k: f64,
    /// `true` when `recall_at_k >= GATE`.
    pub pass: bool,
}

/// Gate threshold: recall@5 must be ≥ 0.80.
pub const GATE: f64 = 0.80;

/// Run the memory recall benchmark.
///
/// Captures all 25 facts into a fresh in-memory `MemoryEngine`, then queries each with its
/// paired question and checks whether the expected answer keyword appears in the top-`k`
/// recalled items. Returns a `RecallReport` with the recall@k score and pass/fail verdict.
pub fn run_memory_recall_bench(k: usize) -> Result<RecallReport> {
    let mut engine = MemoryEngine::in_memory().map_err(|e| anyhow::anyhow!("{e}"))?;
    // Deterministic baseline timestamp — the engine is tested in a time-frozen state.
    let now = 1_700_000_000i64;

    for qa in QA_DATASET {
        let mem = Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            Scope::root(),
            qa.content,
            now,
        );
        engine.capture(&mem).map_err(|e| anyhow::anyhow!("{e}"))?;
    }

    let mut hits = 0usize;
    for qa in QA_DATASET {
        let results = engine
            .recall_ranked(
                qa.query,
                ScopeFilter::Subtree(""),
                &[],
                k,
                now,
                RecallMode::Hybrid,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        if results.iter().any(|r| r.content.contains(qa.answer_kw)) {
            hits += 1;
        }
    }

    let total = QA_DATASET.len();
    let recall_at_k = hits as f64 / total as f64;
    Ok(RecallReport {
        k,
        hits,
        total,
        recall_at_k,
        pass: recall_at_k >= GATE,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_has_25_entries() {
        assert_eq!(QA_DATASET.len(), 25);
    }

    #[test]
    fn recall_at_5_meets_gate() {
        let report = run_memory_recall_bench(5).expect("recall bench failed");
        assert!(
            report.pass,
            "recall@5 = {:.2} ({}/{}) is below gate {GATE}",
            report.recall_at_k, report.hits, report.total,
        );
    }

    #[test]
    fn dataset_integrity_answer_kw_appears_in_content() {
        // Each answer_kw must be a substring of its own content — if not, the benchmark
        // can never pass even with perfect recall (the wrong fact is in the store).
        for qa in QA_DATASET {
            assert!(
                qa.content.contains(qa.answer_kw),
                "answer_kw {:?} not found in content: {:?}",
                qa.answer_kw,
                qa.content,
            );
        }
    }

    #[test]
    fn recall_with_no_memories_returns_empty() {
        // An engine with no stored memories must return an empty list for any query —
        // the engine must not hallucinate or surface garbage results.
        let engine = MemoryEngine::in_memory().expect("in_memory engine failed");
        let now = 1_700_000_000i64;
        let results = engine
            .recall_ranked(
                "What handles API rate limiting?",
                ScopeFilter::Subtree(""),
                &[],
                5,
                now,
                RecallMode::Hybrid,
            )
            .expect("recall_ranked failed on empty engine");
        assert!(
            results.is_empty(),
            "empty engine returned {} results; expected 0",
            results.len()
        );
    }
}
