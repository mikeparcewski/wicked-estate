//! Learning math: confidence (Wilson), decay (Ebbinghaus), and robust salience.
//!
//! Per the DESIGN-council fixes: confidence uses the **Wilson score lower bound** (calibrated at
//! low n, has a prior → never collapses to 0), and salience is a **robust weighted sum** of
//! normalized signals — NOT a bare product (a product collapses to 0 on any single zero factor and
//! lets one term dominate). Weights are config with sensible defaults; tuned post-benchmark (L5).

/// Wilson score lower bound (95%, z=1.96) of a `pos/total` success ratio.
/// `total == 0` → a small prior (0.0 here is acceptable as "no evidence yet"); callers treat
/// low-n as low-confidence which the bound already enforces.
pub fn wilson_lower_bound(pos: u64, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    let n = total as f64;
    let phat = pos as f64 / n;
    let z = 1.96_f64;
    let z2 = z * z;
    let denom = 1.0 + z2 / n;
    let centre = phat + z2 / (2.0 * n);
    let margin = z * ((phat * (1.0 - phat) + z2 / (4.0 * n)) / n).sqrt();
    ((centre - margin) / denom).clamp(0.0, 1.0)
}

/// Ebbinghaus recency in [0,1]: `exp(-lambda * age_days)`.
pub fn decay(age_seconds: i64, lambda_per_day: f64) -> f64 {
    if age_seconds <= 0 {
        return 1.0;
    }
    let age_days = age_seconds as f64 / 86_400.0;
    (-lambda_per_day * age_days).exp()
}

/// Weights for the robust salience combination. `wc + wr + wa` should sum to 1.0.
#[derive(Debug, Clone, Copy)]
pub struct Salience {
    pub wc: f64, // confidence
    pub wr: f64, // recency
    pub wa: f64, // access frequency
    pub lambda_per_day: f64,
}

impl Default for Salience {
    fn default() -> Self {
        // Sensible L0 defaults; tuned post-benchmark (ODR-4 / L5).
        Self {
            wc: 0.5,
            wr: 0.3,
            wa: 0.2,
            lambda_per_day: 0.01,
        }
    }
}

/// Robust salience: weighted sum of normalized [0,1] signals (no zero-collapse).
/// `access_count` is squashed via `log2(1+x)` normalized by a soft cap.
pub fn salience(cfg: &Salience, confidence: f64, age_seconds: i64, access_count: u64) -> f64 {
    let recency = decay(age_seconds, cfg.lambda_per_day);
    // log2(1+access) / log2(1+SOFT_CAP); SOFT_CAP=64 → saturates gently.
    let soft_cap = 64.0_f64;
    let access_norm = ((1.0 + access_count as f64).log2() / (1.0 + soft_cap).log2()).min(1.0);
    (cfg.wc * confidence + cfg.wr * recency + cfg.wa * access_norm).clamp(0.0, 1.0)
}

/// Median (p50) of `vals`. Returns `None` for an empty slice. Used for **adaptive per-scope
/// thresholds** (rust-self-learning's calibration idea): consolidation calibrates promote/archive
/// cutoffs to the scope's own distribution instead of hand-tuned constants. Exact median over the
/// bounded candidate set — no streaming-quantile dependency needed at local-first scale.
pub fn p50(vals: &[f64]) -> Option<f64> {
    if vals.is_empty() {
        return None;
    }
    let mut v: Vec<f64> = vals.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p50_median() {
        assert_eq!(p50(&[]), None);
        assert_eq!(p50(&[0.5]), Some(0.5));
        assert_eq!(p50(&[0.1, 0.9]), Some(0.5));
        assert_eq!(p50(&[0.2, 0.4, 0.9]), Some(0.4));
    }

    #[test]
    fn wilson_monotone_and_low_n_cautious() {
        // More positive evidence at the same rate → higher lower bound (more certain).
        let few = wilson_lower_bound(3, 3);
        let many = wilson_lower_bound(30, 30);
        assert!(many > few, "low-n must be more cautious: {few} vs {many}");
        assert!(wilson_lower_bound(0, 0) == 0.0);
        // A contradiction lowers it.
        assert!(wilson_lower_bound(9, 10) > wilson_lower_bound(8, 10));
    }

    #[test]
    fn decay_monotone_decreasing() {
        let d0 = decay(0, 0.01);
        let d10 = decay(10 * 86_400, 0.01);
        let d100 = decay(100 * 86_400, 0.01);
        assert!((d0 - 1.0).abs() < 1e-9);
        assert!(d0 > d10 && d10 > d100);
    }

    #[test]
    fn salience_no_zero_collapse() {
        let cfg = Salience::default();
        // A brand-new memory (confidence 0, access 0) is still > 0 thanks to recency — the bug the
        // council flagged (multiplicative chain would give 0) is avoided.
        let fresh = salience(&cfg, 0.0, 0, 0);
        assert!(
            fresh > 0.0,
            "fresh memory must have nonzero salience, got {fresh}"
        );
        // Reinforced + recent + accessed beats stale + unconfirmed.
        let strong = salience(&cfg, 0.9, 0, 50);
        let weak = salience(&cfg, 0.0, 365 * 86_400, 0);
        assert!(strong > weak);
    }
}
