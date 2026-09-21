use jiff::Timestamp;

#[derive(Debug, Clone, PartialEq)]
pub struct QuotaWindow {
    pub label: String,
    /// 0.0..=100.0; None when the provider reported a window without a percentage.
    pub used_pct: Option<f64>,
    pub resets_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderStatus {
    pub name: String,
    pub plan: Option<String>,
    pub windows: Vec<QuotaWindow>,
    pub error: Option<String>,
}

impl ProviderStatus {
    pub fn unavailable(name: impl Into<String>, reason: impl Into<String>) -> Self {
        ProviderStatus {
            name: name.into(),
            plan: None,
            windows: Vec::new(),
            error: Some(reason.into()),
        }
    }

    pub fn ok(&self) -> bool {
        self.error.is_none() && !self.windows.is_empty()
    }
}

/// "2d 4h", "1h 12m", "45m"; anything under a minute (or in the past) is "soon".
pub fn human_duration(seconds: i64) -> String {
    if seconds < 60 {
        return "soon".to_string();
    }
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_duration_formats() {
        assert_eq!(human_duration(-5), "soon");
        assert_eq!(human_duration(30), "soon");
        assert_eq!(human_duration(45 * 60), "45m");
        assert_eq!(human_duration(3_600 + 12 * 60), "1h 12m");
        assert_eq!(human_duration(2 * 86_400 + 4 * 3_600 + 30 * 60), "2d 4h");
    }

    #[test]
    fn ok_requires_no_error_and_windows() {
        let unavailable = ProviderStatus {
            name: "claude".into(),
            plan: None,
            windows: Vec::new(),
            error: Some("no credentials".to_string()),
        };
        assert!(!unavailable.ok());

        let empty = ProviderStatus {
            name: "claude".into(),
            plan: None,
            windows: Vec::new(),
            error: None,
        };
        assert!(!empty.ok());
    }
}
