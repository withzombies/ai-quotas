use crate::model::{ProviderStatus, QuotaWindow};
use jiff::Timestamp;
use serde::Deserialize;

pub const NAME: &str = "claude";

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
        name: NAME,
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
