use crate::model::{ProviderStatus, QuotaWindow};
use jiff::Timestamp;
use serde::Deserialize;

pub const NAME: &str = "claude";
pub const BASE_URL: &str = "https://api.anthropic.com";
/// Exactly what Claude Code 2.1.226 sends (verified against the binary).
const USER_AGENT: &str = "claude-cli/2.1.226";
const OAUTH_BETA: &str = "oauth-2025-04-20";

pub fn fetch(base_url: &str, now: Timestamp) -> ProviderStatus {
    try_fetch(base_url, now).unwrap_or_else(|e| ProviderStatus::unavailable(NAME, e))
}

/// A source can hold a token the API rejects — setup tokens and
/// CLAUDE_CODE_OAUTH_TOKEN are inference-only and this endpoint answers them
/// with a persistent 429 (verified: /api/oauth/profile says
/// oauth_scope_insufficient for the same token). So walk every source before
/// giving up.
fn try_fetch(base_url: &str, now: Timestamp) -> Result<ProviderStatus, String> {
    let sources = crate::creds::claude_cred_sources(now);
    if sources.is_empty() {
        return Err("no credentials — run 'claude auth login'".to_string());
    }
    let mut failures: Vec<String> = Vec::new();
    for (label, creds) in sources {
        match creds.and_then(|c| request_usage(base_url, c)) {
            Ok(status) => return Ok(status),
            Err(e) => {
                let hint = if label == "env token" && e.contains("429") {
                    " (setup tokens are inference-only; run 'claude auth login')"
                } else {
                    ""
                };
                failures.push(format!("{label}: {e}{hint}"));
            }
        }
    }
    Err(failures.join("; "))
}

fn request_usage(
    base_url: &str,
    creds: crate::creds::ClaudeCreds,
) -> Result<ProviderStatus, String> {
    let url = format!("{base_url}/api/oauth/usage");
    let headers = [
        ("Authorization", format!("Bearer {}", creds.access_token)),
        ("anthropic-beta", OAUTH_BETA.to_string()),
        ("Content-Type", "application/json".to_string()),
        ("User-Agent", USER_AGENT.to_string()),
        ("x-app", "cli".to_string()),
    ];
    let mut resp = crate::http::get(&url, &headers)?;
    let mut via_curl = "";
    if resp.status == 403 {
        // Anthropic's edge 403s some non-curl TLS fingerprints.
        resp = crate::http::get_via_curl(&url, &headers)?;
        via_curl = " (even via curl)";
    }
    if resp.status != 200 {
        return Err(format!("HTTP {}{via_curl}", resp.status));
    }
    let mut status = parse_usage(&resp.body)?;
    status.plan = creds.plan;
    Ok(status)
}

#[derive(Deserialize)]
struct Usage {
    five_hour: Option<Window>,
    seven_day: Option<Window>,
    seven_day_opus: Option<Window>,
    seven_day_sonnet: Option<Window>,
}

#[derive(Deserialize)]
struct Window {
    /// Already a percentage 0-100, NOT a fraction.
    utilization: Option<f64>,
    resets_at: Option<String>,
}

pub fn parse_usage(body: &str) -> Result<ProviderStatus, String> {
    let usage: Usage = serde_json::from_str(body).map_err(|e| format!("schema mismatch: {e}"))?;

    let buckets = [
        ("5h", usage.five_hour),
        ("week", usage.seven_day),
        ("week (Opus)", usage.seven_day_opus),
        ("week (Sonnet)", usage.seven_day_sonnet),
    ];
    let windows: Vec<QuotaWindow> = buckets
        .into_iter()
        .filter_map(|(label, w)| w.map(|w| (label, w)))
        .map(|(label, w)| QuotaWindow {
            label: label.to_string(),
            used_pct: w.utilization,
            resets_at: w.resets_at.and_then(|s| s.parse::<Timestamp>().ok()),
        })
        .collect();

    if windows.is_empty() {
        return Err("schema mismatch: no usage windows in response".to_string());
    }
    Ok(ProviderStatus {
        name: NAME.into(),
        plan: None, // filled from credentials (subscriptionType) by the caller
        windows,
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_response_skipping_null_buckets() {
        let status =
            parse_usage(include_str!("../../tests/fixtures/claude/usage_ok.json")).unwrap();
        assert_eq!(status.name, "claude");
        let summary: Vec<(&str, Option<f64>, bool)> = status
            .windows
            .iter()
            .map(|w| (w.label.as_str(), w.used_pct, w.resets_at.is_some()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("5h", Some(33.0), true),
                ("week", Some(13.0), true),
                ("week (Sonnet)", Some(1.0), true),
            ]
        );
        assert_eq!(
            status.windows[0].resets_at.unwrap(),
            "2026-09-19T21:13:00.528743Z".parse::<Timestamp>().unwrap()
        );
    }

    #[test]
    fn parses_minimal_response() {
        let status = parse_usage(include_str!(
            "../../tests/fixtures/claude/usage_minimal.json"
        ))
        .unwrap();
        assert_eq!(status.windows.len(), 1);
        assert_eq!(status.windows[0].used_pct, Some(5.0));
        assert_eq!(status.windows[0].resets_at, None);
    }

    #[test]
    fn tolerates_drifted_schema() {
        // Unknown buckets/fields are ignored; a bad timestamp degrades to None.
        let status = parse_usage(include_str!(
            "../../tests/fixtures/claude/usage_drifted.json"
        ))
        .unwrap();
        assert_eq!(status.windows.len(), 1);
        assert_eq!(status.windows[0].label, "5h");
        assert_eq!(status.windows[0].used_pct, Some(33.0));
        assert_eq!(status.windows[0].resets_at, None);
    }

    #[test]
    fn rejects_non_json_and_empty_responses() {
        assert!(
            parse_usage("<html>Sign in</html>")
                .unwrap_err()
                .starts_with("schema mismatch")
        );
        assert_eq!(
            parse_usage("{}").unwrap_err(),
            "schema mismatch: no usage windows in response"
        );
    }
}
