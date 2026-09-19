use crate::model::{ProviderStatus, QuotaWindow};
use jiff::Timestamp;
use serde::Deserialize;

pub const NAME: &str = "zai";

#[derive(Deserialize)]
struct Envelope {
    code: Option<i64>,
    msg: Option<String>,
    data: Option<Data>,
}

#[derive(Deserialize)]
struct Data {
    level: Option<String>,
    #[serde(default)]
    limits: Vec<Limit>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Limit {
    #[serde(rename = "type")]
    kind: Option<String>,
    unit: Option<i64>,
    number: Option<i64>,
    total: Option<f64>,
    usage: Option<f64>,
    current_value: Option<f64>,
    percentage: Option<f64>,
    /// Epoch milliseconds.
    next_reset_time: Option<i64>,
}

/// Window identity comes from (unit, number) — the type string has already
/// drifted once (TOKENS_LIMIT -> CREDIT_LIMIT).
fn label(limit: &Limit) -> String {
    match (limit.unit, limit.number) {
        (Some(3), Some(5)) => "5h".to_string(),
        (Some(6), Some(1)) => "week".to_string(),
        _ if limit.kind.as_deref() == Some("TIME_LIMIT") => "month (tools)".to_string(),
        _ => limit
            .kind
            .clone()
            .unwrap_or_else(|| "unknown".to_string())
            .to_lowercase(),
    }
}

fn used_pct(limit: &Limit) -> Option<f64> {
    if let Some(p) = limit.percentage {
        return Some(p);
    }
    let total = limit.total.or(limit.current_value)?;
    if total <= 0.0 {
        return None;
    }
    limit.usage.map(|u| u / total * 100.0)
}

pub fn parse_usage(body: &str) -> Result<ProviderStatus, String> {
    let envelope: Envelope =
        serde_json::from_str(body).map_err(|e| format!("schema mismatch: {e}"))?;

    let Some(data) = envelope.data else {
        let msg = envelope
            .msg
            .unwrap_or_else(|| "no data in response".to_string());
        let code = envelope
            .code
            .map(|c| format!(" (code {c})"))
            .unwrap_or_default();
        return Err(format!("api error: {msg}{code}"));
    };

    let windows: Vec<QuotaWindow> = data
        .limits
        .iter()
        .map(|l| QuotaWindow {
            label: label(l),
            used_pct: used_pct(l),
            resets_at: l
                .next_reset_time
                .and_then(|ms| Timestamp::from_millisecond(ms).ok()),
        })
        .collect();

    if windows.is_empty() {
        return Err("schema mismatch: no limits in response".to_string());
    }
    Ok(ProviderStatus {
        name: NAME,
        plan: data.level,
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

    #[test]
    fn parses_five_hour_weekly_and_tool_windows() {
        let status = parse_usage(include_str!("../../tests/fixtures/zai/quota_ok.json")).unwrap();
        assert_eq!(status.plan.as_deref(), Some("GLM Coding Max"));
        let summary: Vec<(&str, Option<f64>)> = status
            .windows
            .iter()
            .map(|w| (w.label.as_str(), w.used_pct))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("5h", Some(20.3)),
                ("week", Some(13.0)),
                ("month (tools)", Some(15.0)),
            ]
        );
        // nextResetTime is epoch milliseconds
        assert_eq!(
            status.windows[0].resets_at,
            Some(ts("2026-09-19T15:00:00Z"))
        );
        assert_eq!(status.windows[2].resets_at, None);
    }

    #[test]
    fn drifted_types_still_classify_by_unit_and_number() {
        let status =
            parse_usage(include_str!("../../tests/fixtures/zai/quota_drifted.json")).unwrap();
        let summary: Vec<(&str, Option<f64>)> = status
            .windows
            .iter()
            .map(|w| (w.label.as_str(), w.used_pct))
            .collect();
        // CREDIT_LIMIT with (3,5) is still the 5h window; percentage computed
        // from usage/total. The unrecognized entry degrades to a label with no
        // percentage — never 0%.
        assert_eq!(summary, vec![("5h", Some(25.0)), ("mystery_limit", None)]);
    }

    #[test]
    fn error_envelope_becomes_api_error() {
        assert_eq!(
            parse_usage(include_str!("../../tests/fixtures/zai/error_envelope.json")).unwrap_err(),
            "api error: invalid api key (code 401)"
        );
    }

    #[test]
    fn rejects_non_json() {
        assert!(
            parse_usage("<html>gateway timeout</html>")
                .unwrap_err()
                .starts_with("schema mismatch")
        );
    }
}
