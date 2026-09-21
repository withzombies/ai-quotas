use crate::model::{ProviderStatus, human_duration};
use jiff::Timestamp;
use jiff::tz::TimeZone;

const BAR_WIDTH: usize = 20;

const RESET: &str = "\x1b[0m";
const DIM: &str = "2";
const RED: &str = "38;5;203";
const YELLOW: &str = "38;5;179";
const GREEN: &str = "38;5;114";

struct Style {
    enabled: bool,
}

impl Style {
    fn paint(&self, code: &str, text: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{text}{RESET}")
        } else {
            text.to_string()
        }
    }
}

/// Every provider keeps a fixed accent color so rows are recognizable at a glance.
fn provider_color(name: &str) -> &'static str {
    match name.split(" (").next().unwrap_or(name) {
        "claude" => "1;38;5;208", // orange
        "codex" => "1;38;5;75",   // blue
        "zai" => "1;38;5;170",    // purple
        "grok" => "1;38;5;114",   // green
        _ => "1",
    }
}

fn severity_color(pct: f64) -> &'static str {
    if pct >= 80.0 {
        RED
    } else if pct >= 50.0 {
        YELLOW
    } else {
        GREEN
    }
}

fn bar(pct: f64) -> String {
    let filled =
        ((pct / 100.0 * BAR_WIDTH as f64).round() as i64).clamp(0, BAR_WIDTH as i64) as usize;
    format!("{}{}", "█".repeat(filled), "░".repeat(BAR_WIDTH - filled))
}

/// Render grouped provider blocks with usage bars. Pure: fixed `now`, `tz`,
/// and `color` give fixed output.
pub fn table(statuses: &[ProviderStatus], now: Timestamp, tz: &TimeZone, color: bool) -> String {
    let sty = Style { enabled: color };
    let label_width = statuses
        .iter()
        .flat_map(|s| &s.windows)
        .map(|w| w.label.chars().count())
        .max()
        .unwrap_or(0);

    let blocks: Vec<String> = statuses
        .iter()
        .map(|s| {
            let name = sty.paint(provider_color(&s.name), &s.name);
            if let Some(reason) = &s.error {
                return format!(
                    "{name} · {}\n",
                    sty.paint(RED, &format!("unavailable: {reason}"))
                );
            }
            let mut block = match &s.plan {
                Some(plan) => format!("{name} · {}\n", sty.paint(DIM, plan)),
                None => format!("{name}\n"),
            };
            for w in &s.windows {
                let (bar_str, pct_str, code) = match w.used_pct {
                    Some(pct) => (bar(pct), format!("{pct:.0}%"), severity_color(pct)),
                    None => ("░".repeat(BAR_WIDTH), "?".to_string(), DIM),
                };
                let resets = match w.resets_at {
                    Some(at) => format!(
                        "resets in {} ({})",
                        human_duration((at - now).get_seconds()),
                        at.to_zoned(tz.clone()).strftime("%b %d %H:%M")
                    ),
                    None => "-".to_string(),
                };
                block.push_str(&format!(
                    "  {:<label_width$}  [{}]  {}  {}\n",
                    w.label,
                    sty.paint(code, &bar_str),
                    sty.paint(code, &format!("{pct_str:>4}")),
                    sty.paint(DIM, &resets),
                ));
            }
            block
        })
        .collect();

    blocks.join("\n")
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

    fn claude_status() -> ProviderStatus {
        ProviderStatus {
            name: "claude".into(),
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
        }
    }

    #[test]
    fn renders_grouped_windows_with_bars_and_local_times() {
        let expected = "\
claude · max
  5h    [███████░░░░░░░░░░░░░]   33%  resets in 2h 13m (Sep 19 14:13)
  week  [███░░░░░░░░░░░░░░░░░]   13%  resets in 3d 0h (Sep 22 12:00)
";
        assert_eq!(table(&[claude_status()], ts(NOW), &tz(), false), expected);
    }

    #[test]
    fn renders_partial_and_unavailable_blocks() {
        let statuses = [
            ProviderStatus {
                name: "zai".into(),
                plan: None,
                windows: vec![QuotaWindow {
                    label: "unknown".into(),
                    used_pct: None,
                    resets_at: None,
                }],
                error: None,
            },
            ProviderStatus {
                name: "grok".into(),
                plan: None,
                windows: Vec::new(),
                error: Some("token expired — run 'grok login'".into()),
            },
        ];
        let expected = "\
zai
  unknown  [░░░░░░░░░░░░░░░░░░░░]     ?  -

grok · unavailable: token expired — run 'grok login'
";
        assert_eq!(table(&statuses, ts(NOW), &tz(), false), expected);
    }

    #[test]
    fn empty_input_renders_nothing() {
        assert_eq!(table(&[], ts(NOW), &tz(), false), "");
    }

    #[test]
    fn color_mode_applies_provider_accent_and_severity() {
        let mut status = claude_status();
        status.windows.truncate(1);
        status.windows[0].used_pct = Some(90.0);
        let out = table(&[status], ts(NOW), &tz(), true);
        assert!(
            out.contains("\x1b[1;38;5;208mclaude\x1b[0m"),
            "provider accent missing: {out:?}"
        );
        assert!(
            out.contains("\x1b[38;5;203m"),
            "red severity missing: {out:?}"
        );
        assert!(
            out.contains("\x1b[2mmax\x1b[0m"),
            "dim plan missing: {out:?}"
        );
    }

    #[test]
    fn bar_fill_is_proportional_and_clamped() {
        assert_eq!(bar(0.0), "░".repeat(20));
        assert_eq!(bar(100.0), "█".repeat(20));
        assert_eq!(bar(150.0), "█".repeat(20));
        assert_eq!(bar(-5.0), "░".repeat(20));
        assert_eq!(bar(50.0), format!("{}{}", "█".repeat(10), "░".repeat(10)));
    }
}
