use crate::model::{ProviderStatus, QuotaWindow};
use jiff::Timestamp;
use serde::Deserialize;

pub const NAME: &str = "grok";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Billing {
    config: Option<Config>,
    subscription_tier: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    credit_usage_percent: Option<f64>,
    current_period: Option<Period>,
    monthly_limit: Option<Cents>,
    used: Option<Cents>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Period {
    #[serde(rename = "type")]
    kind: Option<String>,
    end: Option<String>,
}

/// USD cents in proto3 JSON: `{"val": 123}`, with `{}` meaning $0 because
/// zero scalars are omitted. Never treat `{}` as missing data.
#[derive(Deserialize)]
struct Cents {
    val: Option<i64>,
}

impl Cents {
    fn value(&self) -> i64 {
        self.val.unwrap_or(0)
    }
}

pub fn parse_usage(body: &str) -> Result<ProviderStatus, String> {
    let billing: Billing =
        serde_json::from_str(body).map_err(|e| format!("schema mismatch: {e}"))?;
    let Some(config) = billing.config else {
        return Err("schema mismatch: no billing config in response".to_string());
    };

    let label = match config
        .current_period
        .as_ref()
        .and_then(|p| p.kind.as_deref())
    {
        Some("USAGE_PERIOD_TYPE_WEEKLY") => "week",
        Some("USAGE_PERIOD_TYPE_MONTHLY") => "month",
        _ => "period",
    };
    let used_pct = config.credit_usage_percent.or_else(|| {
        // Legacy accounts report cent amounts instead of a percentage.
        let limit = config.monthly_limit.as_ref()?.value();
        if limit <= 0 {
            return None;
        }
        let used = config.used.as_ref().map(|c| c.value()).unwrap_or(0);
        Some(used as f64 / limit as f64 * 100.0)
    });
    let resets_at = config
        .current_period
        .and_then(|p| p.end)
        .and_then(|s| s.parse::<Timestamp>().ok());

    Ok(ProviderStatus {
        name: NAME,
        plan: billing.subscription_tier,
        windows: vec![QuotaWindow {
            label: label.to_string(),
            used_pct,
            resets_at,
        }],
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
    fn parses_unified_weekly_pool() {
        let status =
            parse_usage(include_str!("../../tests/fixtures/grok/billing_ok.json")).unwrap();
        assert_eq!(status.plan.as_deref(), Some("SuperGrok"));
        assert_eq!(status.windows.len(), 1);
        assert_eq!(status.windows[0].label, "week");
        assert_eq!(status.windows[0].used_pct, Some(41.5));
        assert_eq!(
            status.windows[0].resets_at,
            Some(ts("2026-09-22T00:00:00Z"))
        );
    }

    #[test]
    fn legacy_monthly_account_computes_percent_from_cents() {
        let status = parse_usage(include_str!(
            "../../tests/fixtures/grok/billing_monthly_no_percent.json"
        ))
        .unwrap();
        assert_eq!(status.plan, None);
        assert_eq!(status.windows[0].label, "month");
        // used is `{}` which is a real $0, not missing data: 0 of 3000 cents.
        assert_eq!(status.windows[0].used_pct, Some(0.0));
        assert_eq!(
            status.windows[0].resets_at,
            Some(ts("2026-10-01T00:00:00Z"))
        );
    }

    #[test]
    fn rejects_missing_config() {
        assert_eq!(
            parse_usage("{}").unwrap_err(),
            "schema mismatch: no billing config in response"
        );
        assert!(
            parse_usage("<html>403</html>")
                .unwrap_err()
                .starts_with("schema mismatch")
        );
    }
}
