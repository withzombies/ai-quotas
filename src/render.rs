use crate::model::{ProviderStatus, human_duration};
use jiff::Timestamp;
use jiff::tz::TimeZone;

/// Render the status table. Pure: fixed `now` and `tz` give fixed output.
pub fn table(statuses: &[ProviderStatus], now: Timestamp, tz: &TimeZone) -> String {
    let mut rows: Vec<[String; 5]> = vec![[
        "PROVIDER".into(),
        "PLAN".into(),
        "WINDOW".into(),
        "USED".into(),
        "RESETS".into(),
    ]];

    for s in statuses {
        let plan = s.plan.clone().unwrap_or_else(|| "-".into());
        if let Some(reason) = &s.error {
            rows.push([
                s.name.into(),
                plan,
                format!("unavailable: {reason}"),
                String::new(),
                String::new(),
            ]);
            continue;
        }
        for w in &s.windows {
            let used = match w.used_pct {
                Some(pct) => format!("{pct:.0}%"),
                None => "?".into(),
            };
            let resets = match w.resets_at {
                Some(at) => format!(
                    "in {} ({})",
                    human_duration((at - now).get_seconds()),
                    at.to_zoned(tz.clone()).strftime("%b %d %H:%M")
                ),
                None => "-".into(),
            };
            rows.push([s.name.into(), plan.clone(), w.label.clone(), used, resets]);
        }
    }

    let mut widths = [0usize; 5];
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }

    let mut out = String::new();
    for row in &rows {
        let mut line = String::new();
        for (i, cell) in row.iter().enumerate() {
            let pad = " ".repeat(widths[i] - cell.chars().count());
            if i == 3 {
                // USED is right-aligned.
                line.push_str(&pad);
                line.push_str(cell);
            } else {
                line.push_str(cell);
                line.push_str(&pad);
            }
            if i < 4 {
                line.push_str("  ");
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::QuotaWindow;

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn tz() -> TimeZone {
        TimeZone::get("America/Los_Angeles").unwrap()
    }

    const NOW: &str = "2026-09-19T19:00:00Z"; // noon in Los Angeles

    #[test]
    fn renders_windows_with_local_reset_times() {
        let statuses = [ProviderStatus {
            name: "claude",
            plan: Some("max".into()),
            windows: vec![
                QuotaWindow {
                    label: "5h".into(),
                    used_pct: Some(33.0),
                    resets_at: Some(ts("2026-09-19T21:13:00Z")),
                },
                QuotaWindow {
                    label: "week".into(),
                    used_pct: Some(12.6),
                    resets_at: Some(ts("2026-09-22T19:00:00Z")),
                },
            ],
            error: None,
        }];
        let expected = "\
PROVIDER  PLAN  WINDOW  USED  RESETS
claude    max   5h       33%  in 2h 13m (Sep 19 14:13)
claude    max   week     13%  in 3d 0h (Sep 22 12:00)
";
        assert_eq!(table(&statuses, ts(NOW), &tz()), expected);
    }

    #[test]
    fn renders_unavailable_and_partial_rows() {
        let statuses = [
            ProviderStatus {
                name: "zai",
                plan: None,
                windows: vec![QuotaWindow {
                    label: "unknown".into(),
                    used_pct: None,
                    resets_at: None,
                }],
                error: None,
            },
            ProviderStatus {
                name: "grok",
                plan: None,
                windows: Vec::new(),
                error: Some("token expired — run 'grok login'".into()),
            },
        ];
        let expected = "\
PROVIDER  PLAN  WINDOW                                         USED  RESETS
zai       -     unknown                                           ?  -
grok      -     unavailable: token expired — run 'grok login'
";
        assert_eq!(table(&statuses, ts(NOW), &tz()), expected);
    }

    #[test]
    fn header_only_when_no_statuses() {
        assert_eq!(
            table(&[], ts(NOW), &tz()),
            "PROVIDER  PLAN  WINDOW  USED  RESETS\n"
        );
    }
}
