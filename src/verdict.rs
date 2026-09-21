use crate::model::{ProviderStatus, QuotaWindow, human_duration};
use jiff::Timestamp;

struct Candidate<'a> {
    name: &'a str,
    binding: &'a QuotaWindow,
    headroom: f64,
}

/// The window that constrains this provider most: highest used_pct,
/// ties broken by earliest reset, then by position.
fn binding_window(windows: &[QuotaWindow]) -> Option<&QuotaWindow> {
    let mut best: Option<&QuotaWindow> = None;
    for w in windows.iter().filter(|w| w.used_pct.is_some()) {
        let Some(b) = best else {
            best = Some(w);
            continue;
        };
        let tighter = w.used_pct > b.used_pct
            || (w.used_pct == b.used_pct && earlier(w.resets_at, b.resets_at));
        if tighter {
            best = Some(w);
        }
    }
    best
}

fn earlier(a: Option<Timestamp>, b: Option<Timestamp>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a < b,
        (Some(_), None) => true,
        _ => false,
    }
}

fn resets_in(window: &QuotaWindow, now: Timestamp) -> Option<String> {
    window
        .resets_at
        .map(|at| human_duration((at - now).get_seconds()))
}

/// Deterministic recommendation. Statuses must be in the fixed provider order;
/// that order is the final tiebreak.
pub fn verdict_line(statuses: &[ProviderStatus], now: Timestamp) -> String {
    let candidates: Vec<Candidate> = statuses
        .iter()
        .filter(|s| s.ok())
        .filter_map(|s| {
            binding_window(&s.windows).map(|binding| Candidate {
                name: &s.name,
                binding,
                headroom: (100.0 - binding.used_pct.unwrap()).clamp(0.0, 100.0),
            })
        })
        .collect();

    if candidates.is_empty() {
        return "Verdict: no quota data available.".to_string();
    }

    let (available, exhausted): (Vec<&Candidate>, Vec<&Candidate>) =
        candidates.iter().partition(|c| c.headroom > 0.0);

    if available.is_empty() {
        let next = exhausted
            .iter()
            .filter(|c| c.binding.resets_at.is_some())
            .min_by_key(|c| c.binding.resets_at);
        return match next {
            Some(c) => format!(
                "Verdict: all subscriptions exhausted; earliest reset: {} ({}) in {}.",
                c.name,
                c.binding.label,
                resets_in(c.binding, now).unwrap()
            ),
            None => "Verdict: all subscriptions exhausted.".to_string(),
        };
    }

    // Rank: 5-point headroom buckets, then earlier binding reset, then input order
    // (stable sort keeps it).
    let mut ranked = available;
    ranked.sort_by(|a, b| {
        let bucket = |c: &Candidate| (c.headroom / 5.0).floor() as i64;
        bucket(b)
            .cmp(&bucket(a))
            .then_with(|| match (a.binding.resets_at, b.binding.resets_at) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
    });
    let winner = ranked[0];

    let reset_clause = match resets_in(winner.binding, now) {
        Some(d) => format!(", resets in {d}"),
        None => String::new(),
    };
    format!(
        "Verdict: use {} — {:.0}% headroom on its tightest window ({}){reset_clause}.",
        winner.name, winner.headroom, winner.binding.label
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn provider(
        name: &'static str,
        windows: &[(&str, Option<f64>, Option<&str>)],
    ) -> ProviderStatus {
        ProviderStatus {
            name: name.into(),
            plan: None,
            windows: windows
                .iter()
                .map(|(label, pct, reset)| QuotaWindow {
                    label: label.to_string(),
                    used_pct: *pct,
                    resets_at: reset.map(ts),
                })
                .collect(),
            error: None,
        }
    }

    const NOW: &str = "2026-09-19T12:00:00Z";

    #[test]
    fn empty_input_has_no_data() {
        assert_eq!(
            verdict_line(&[], ts(NOW)),
            "Verdict: no quota data available."
        );
    }

    #[test]
    fn errored_and_pctless_providers_are_ineligible() {
        let statuses = [
            ProviderStatus {
                name: "claude".into(),
                plan: None,
                windows: Vec::new(),
                error: Some("no credentials".to_string()),
            },
            provider("zai", &[("5h", None, Some("2026-09-19T14:00:00Z"))]),
        ];
        assert_eq!(
            verdict_line(&statuses, ts(NOW)),
            "Verdict: no quota data available."
        );
    }

    #[test]
    fn clear_winner_by_headroom() {
        let statuses = [
            provider(
                "claude",
                &[("5h", Some(80.0), Some("2026-09-19T14:00:00Z"))],
            ),
            provider("codex", &[("5h", Some(38.0), Some("2026-09-19T15:12:00Z"))]),
        ];
        assert_eq!(
            verdict_line(&statuses, ts(NOW)),
            "Verdict: use codex — 62% headroom on its tightest window (5h), resets in 3h 12m."
        );
    }

    #[test]
    fn binding_window_is_the_tightest() {
        let statuses = [provider(
            "claude",
            &[
                ("5h", Some(10.0), Some("2026-09-19T14:00:00Z")),
                ("week", Some(90.0), Some("2026-09-22T12:00:00Z")),
            ],
        )];
        assert_eq!(
            verdict_line(&statuses, ts(NOW)),
            "Verdict: use claude — 10% headroom on its tightest window (week), resets in 3d 0h."
        );
    }

    #[test]
    fn near_tie_broken_by_earlier_reset() {
        // 61% and 63% headroom share the 60-65 bucket; codex resets sooner and wins
        // even though claude comes first in provider order.
        let statuses = [
            provider(
                "claude",
                &[("5h", Some(37.0), Some("2026-09-19T16:00:00Z"))],
            ),
            provider("codex", &[("5h", Some(39.0), Some("2026-09-19T13:00:00Z"))]),
        ];
        assert_eq!(
            verdict_line(&statuses, ts(NOW)),
            "Verdict: use codex — 61% headroom on its tightest window (5h), resets in 1h 0m."
        );
    }

    #[test]
    fn exact_tie_falls_back_to_provider_order() {
        let statuses = [
            provider(
                "claude",
                &[("5h", Some(40.0), Some("2026-09-19T14:00:00Z"))],
            ),
            provider("codex", &[("5h", Some(40.0), Some("2026-09-19T14:00:00Z"))]),
        ];
        assert_eq!(
            verdict_line(&statuses, ts(NOW)),
            "Verdict: use claude — 60% headroom on its tightest window (5h), resets in 2h 0m."
        );
    }

    #[test]
    fn all_exhausted_reports_earliest_reset() {
        let statuses = [
            provider(
                "claude",
                &[("week", Some(100.0), Some("2026-09-22T12:00:00Z"))],
            ),
            provider(
                "codex",
                &[("5h", Some(100.0), Some("2026-09-19T13:12:00Z"))],
            ),
        ];
        assert_eq!(
            verdict_line(&statuses, ts(NOW)),
            "Verdict: all subscriptions exhausted; earliest reset: codex (5h) in 1h 12m."
        );
    }

    #[test]
    fn all_exhausted_without_resets() {
        let statuses = [provider("grok", &[("period", Some(100.0), None)])];
        assert_eq!(
            verdict_line(&statuses, ts(NOW)),
            "Verdict: all subscriptions exhausted."
        );
    }

    #[test]
    fn winner_without_reset_omits_clause() {
        let statuses = [provider("grok", &[("period", Some(25.0), None)])];
        assert_eq!(
            verdict_line(&statuses, ts(NOW)),
            "Verdict: use grok — 75% headroom on its tightest window (period)."
        );
    }
}
