use crate::model::{ProviderStatus, QuotaWindow};
use jiff::Timestamp;
use serde::Deserialize;

pub const NAME: &str = "codex";
pub const BASE_URL: &str = "https://chatgpt.com/backend-api";

pub fn fetch(base_url: &str, now: Timestamp) -> ProviderStatus {
    try_fetch(base_url, now).unwrap_or_else(|e| ProviderStatus::unavailable(NAME, e))
}

fn try_fetch(base_url: &str, now: Timestamp) -> Result<ProviderStatus, String> {
    let creds = crate::creds::load_codex_creds()?;
    let mut headers = vec![
        ("Authorization", format!("Bearer {}", creds.access_token)),
        ("Accept", "application/json".to_string()),
        ("User-Agent", "codex-cli".to_string()),
    ];
    if let Some(account_id) = &creds.account_id {
        headers.push(("ChatGPT-Account-Id", account_id.clone()));
    }
    let resp = crate::http::get(&format!("{base_url}/wham/usage"), &headers)?;
    if resp.status != 200 {
        return Err(format!("HTTP {}", resp.status));
    }
    parse_usage(&resp.body, now)
}

#[derive(Deserialize)]
struct Usage {
    plan_type: Option<String>,
    rate_limit: Option<RateLimit>,
    /// Arrives as an explicit null on some accounts, so `serde(default)` on a
    /// bare Vec is not enough.
    additional_rate_limits: Option<Vec<AdditionalLimit>>,
}

#[derive(Deserialize)]
struct RateLimit {
    allowed: Option<bool>,
    limit_reached: Option<bool>,
    primary_window: Option<Window>,
    secondary_window: Option<Window>,
}

#[derive(Deserialize)]
struct AdditionalLimit {
    limit_name: Option<String>,
    metered_feature: Option<String>,
    rate_limit: Option<RateLimit>,
}

#[derive(Deserialize)]
struct Window {
    used_percent: Option<f64>,
    limit_window_seconds: Option<i64>,
    reset_after_seconds: Option<i64>,
    reset_at: Option<i64>,
}

/// Windows must be identified by duration — primary is not guaranteed to be 5h.
fn duration_label(seconds: Option<i64>) -> String {
    match seconds {
        Some(18_000) => "5h".to_string(),
        Some(604_800) => "week".to_string(),
        Some(s) if s > 0 && s % 86_400 == 0 => format!("{}d", s / 86_400),
        Some(s) if s > 0 && s % 3_600 == 0 => format!("{}h", s / 3_600),
        Some(s) if s > 0 => format!("{}m", s / 60),
        _ => "window".to_string(),
    }
}

fn to_quota_window(w: Window, label_prefix: &str, exhausted: bool, now: Timestamp) -> QuotaWindow {
    let label = format!("{label_prefix}{}", duration_label(w.limit_window_seconds));
    // A blocked account has no headroom regardless of the reported percentage.
    let used_pct = w
        .used_percent
        .map(|p| if exhausted { p.max(100.0) } else { p });
    let resets_at = w
        .reset_at
        .and_then(|s| Timestamp::from_second(s).ok())
        .or_else(|| {
            w.reset_after_seconds
                .map(|s| now + jiff::SignedDuration::from_secs(s))
        });
    QuotaWindow {
        label,
        used_pct,
        resets_at,
    }
}

fn rate_limit_windows(
    rl: RateLimit,
    label_prefix: &str,
    now: Timestamp,
    out: &mut Vec<QuotaWindow>,
) {
    let exhausted = rl.limit_reached == Some(true) || rl.allowed == Some(false);
    for w in [rl.primary_window, rl.secondary_window]
        .into_iter()
        .flatten()
    {
        out.push(to_quota_window(w, label_prefix, exhausted, now));
    }
}

pub fn parse_usage(body: &str, now: Timestamp) -> Result<ProviderStatus, String> {
    let usage: Usage = serde_json::from_str(body).map_err(|e| format!("schema mismatch: {e}"))?;

    let mut windows = Vec::new();
    if let Some(rl) = usage.rate_limit {
        rate_limit_windows(rl, "", now, &mut windows);
    }
    for extra in usage.additional_rate_limits.unwrap_or_default() {
        let name = extra
            .limit_name
            .or(extra.metered_feature)
            .unwrap_or_else(|| "extra".to_string());
        if let Some(rl) = extra.rate_limit {
            rate_limit_windows(rl, &format!("{name} "), now, &mut windows);
        }
    }

    if windows.is_empty() {
        return Err("schema mismatch: no rate-limit windows in response".to_string());
    }
    Ok(ProviderStatus {
        name: NAME,
        plan: usage.plan_type,
        windows,
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    // 2026-09-19T12:00:00Z == unix 1789819200; fixtures are built around it.
    const NOW: &str = "2026-09-19T12:00:00Z";

    #[test]
    fn parses_both_windows_classified_by_duration() {
        let status = parse_usage(
            include_str!("../../tests/fixtures/codex/usage_ok.json"),
            ts(NOW),
        )
        .unwrap();
        assert_eq!(status.plan.as_deref(), Some("plus"));
        let summary: Vec<(&str, Option<f64>)> = status
            .windows
            .iter()
            .map(|w| (w.label.as_str(), w.used_pct))
            .collect();
        assert_eq!(summary, vec![("5h", Some(42.0)), ("week", Some(17.0))]);
        assert_eq!(
            status.windows[0].resets_at,
            Some(ts("2026-09-19T13:42:00Z"))
        );
        assert_eq!(
            status.windows[1].resets_at,
            Some(ts("2026-09-22T23:20:00Z"))
        );
    }

    #[test]
    fn missing_secondary_is_omitted_and_reset_falls_back_to_offset() {
        let status = parse_usage(
            include_str!("../../tests/fixtures/codex/usage_missing_secondary.json"),
            ts(NOW),
        )
        .unwrap();
        assert_eq!(status.plan.as_deref(), Some("pro"));
        assert_eq!(status.windows.len(), 1);
        assert_eq!(status.windows[0].used_pct, Some(8.0));
        // no reset_at in fixture: now + reset_after_seconds (6120s)
        assert_eq!(
            status.windows[0].resets_at,
            Some(ts("2026-09-19T13:42:00Z"))
        );
    }

    #[test]
    fn limit_reached_zeroes_headroom_and_additional_limits_are_kept() {
        let status = parse_usage(
            include_str!("../../tests/fixtures/codex/usage_limit_reached.json"),
            ts(NOW),
        )
        .unwrap();
        let summary: Vec<(&str, Option<f64>)> = status
            .windows
            .iter()
            .map(|w| (w.label.as_str(), w.used_pct))
            .collect();
        // 87% reported but limit_reached: clamped to 100. Odd 1h duration labeled
        // from seconds; the additional codex-max limit keeps its own headroom.
        assert_eq!(
            summary,
            vec![("1h", Some(100.0)), ("codex-max 30d", Some(3.0))]
        );
    }

    #[test]
    fn rejects_responses_without_windows() {
        assert!(
            parse_usage("{}", ts(NOW))
                .unwrap_err()
                .starts_with("schema mismatch")
        );
        assert!(
            parse_usage("Cloudflare error", ts(NOW))
                .unwrap_err()
                .starts_with("schema mismatch")
        );
    }
}
