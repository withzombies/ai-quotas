use ai_quotas::model::ProviderStatus;
use ai_quotas::{providers, render, verdict};
use clap::{CommandFactory, Parser};
use jiff::Timestamp;
use jiff::tz::TimeZone;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "Show AI subscription quota usage and which one to use now"
)]
struct Args {
    /// Only query these providers (repeatable)
    #[arg(
        short,
        long = "provider",
        value_name = "NAME",
        value_parser = ["claude", "codex", "zai", "grok"]
    )]
    provider: Vec<String>,
    /// Query a named Codex auth file instead of default Codex credentials (repeatable)
    #[arg(long, value_name = "LABEL=AUTH_JSON_PATH", value_parser = parse_codex_profile)]
    codex_profile: Vec<CodexProfile>,
}

#[derive(Clone, Debug)]
struct CodexProfile {
    label: String,
    path: PathBuf,
}

fn parse_codex_profile(value: &str) -> Result<CodexProfile, String> {
    let (label, path) = value
        .split_once('=')
        .ok_or("expected LABEL=AUTH_JSON_PATH")?;
    if label.trim().is_empty() || label.chars().any(char::is_control) || path.is_empty() {
        return Err(
            "profile requires a nonempty label without control characters and an auth file path"
                .into(),
        );
    }
    Ok(CodexProfile {
        label: label.trim().into(),
        path: path.into(),
    })
}

enum Query {
    Provider(&'static str, providers::FetchFn),
    Codex(CodexProfile),
}

impl Query {
    fn name(&self) -> String {
        match self {
            Self::Provider(name, _) => (*name).into(),
            Self::Codex(profile) => format!("codex ({})", profile.label),
        }
    }

    fn fetch(self, now: Timestamp) -> ProviderStatus {
        match self {
            Self::Provider(_, fetch) => fetch(now),
            Self::Codex(profile) => providers::codex::fetch_profile(
                providers::codex::BASE_URL,
                now,
                &profile.path,
                &profile.label,
            ),
        }
    }
}

fn selected_jobs(args: &Args) -> Result<Vec<Query>, String> {
    let mut labels = std::collections::HashSet::new();
    for profile in &args.codex_profile {
        if !labels.insert(&profile.label) {
            return Err(format!("duplicate Codex profile label: {}", profile.label));
        }
    }
    let mut jobs = Vec::new();
    for (name, fetch) in providers::ALL {
        if !args.provider.is_empty() && !args.provider.iter().any(|p| p == name) {
            continue;
        }
        if name == providers::codex::NAME && !args.codex_profile.is_empty() {
            jobs.extend(args.codex_profile.iter().cloned().map(Query::Codex));
        } else {
            jobs.push(Query::Provider(name, fetch));
        }
    }
    Ok(jobs)
}

fn main() {
    let args = Args::parse();
    let now = Timestamp::now();

    let selected = selected_jobs(&args).unwrap_or_else(|error| {
        Args::command()
            .error(clap::error::ErrorKind::ValueValidation, error)
            .exit()
    });
    let statuses: Vec<ProviderStatus> = std::thread::scope(|s| {
        let handles: Vec<_> = selected
            .into_iter()
            .map(|query| {
                let name = query.name();
                (name, s.spawn(move || query.fetch(now)))
            })
            .collect();
        handles
            .into_iter()
            .map(|(name, h)| {
                h.join()
                    .unwrap_or_else(|_| ProviderStatus::unavailable(name, "internal panic"))
            })
            .collect()
    });

    let color = std::io::IsTerminal::is_terminal(&std::io::stdout())
        && std::env::var_os("NO_COLOR").is_none();
    print!(
        "{}",
        render::table(&statuses, now, &TimeZone::system(), color)
    );
    println!("\n{}", verdict::verdict_line(&statuses, now));

    if !statuses.iter().any(|s| s.ok()) {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_replace_default_codex_and_keep_order() {
        let args = Args::try_parse_from([
            "ai-quotas",
            "--provider",
            "claude",
            "--provider",
            "codex",
            "--codex-profile",
            "First=/tmp/one/auth.json",
            "--codex-profile",
            "Second=/tmp/two=backup/auth.json",
        ])
        .unwrap();
        let jobs = selected_jobs(&args).unwrap();
        assert_eq!(
            jobs.iter().map(Query::name).collect::<Vec<_>>(),
            ["claude", "codex (First)", "codex (Second)"]
        );
        assert_eq!(
            args.codex_profile[1].path.to_str(),
            Some("/tmp/two=backup/auth.json")
        );
    }

    #[test]
    fn default_selection_is_unchanged_and_filters_apply() {
        let args = Args::try_parse_from(["ai-quotas"]).unwrap();
        assert_eq!(
            selected_jobs(&args)
                .unwrap()
                .iter()
                .map(Query::name)
                .collect::<Vec<_>>(),
            ["claude", "codex", "zai", "grok"]
        );
        let args = Args::try_parse_from([
            "ai-quotas",
            "-p",
            "claude",
            "--codex-profile",
            "First=/tmp/auth.json",
        ])
        .unwrap();
        assert_eq!(
            selected_jobs(&args)
                .unwrap()
                .iter()
                .map(Query::name)
                .collect::<Vec<_>>(),
            ["claude"]
        );
    }

    #[test]
    fn named_profiles_are_distinct_in_table_and_verdict() {
        let now: Timestamp = "2026-09-19T12:00:00Z".parse().unwrap();
        let mut first = providers::codex::parse_usage(
            include_str!("../tests/fixtures/codex/usage_ok.json"),
            now,
        )
        .unwrap();
        first.name = "codex (First)".into();
        let mut second = first.clone();
        second.name = "codex (Second)".into();
        second.windows[0].used_pct = Some(10.0);
        let statuses = [
            first,
            second,
            ProviderStatus::unavailable("codex (Third)", "missing auth file"),
        ];
        let output = render::table(&statuses, now, &TimeZone::UTC, false);
        for label in ["codex (First)", "codex (Second)", "codex (Third)"] {
            assert!(output.contains(label));
        }
        assert!(verdict::verdict_line(&statuses, now).starts_with("Verdict: use codex (Second)"));
    }

    #[test]
    fn missing_profile_credentials_keep_its_label_without_network() {
        let now: Timestamp = "2026-09-19T12:00:00Z".parse().unwrap();
        let status = providers::codex::fetch_profile(
            "http://127.0.0.1:1",
            now,
            std::path::Path::new("tests/fixtures/codex/nonexistent-auth-file.json"),
            "Missing",
        );
        assert_eq!(status.name, "codex (Missing)");
        assert!(!status.ok());
        assert!(status.error.unwrap().contains("no credentials"));
    }

    #[test]
    fn malformed_and_duplicate_labels_are_rejected() {
        for value in [
            "missing-separator",
            "=/tmp/auth.json",
            "First=",
            "bad\nlabel=/tmp/auth.json",
        ] {
            assert!(Args::try_parse_from(["ai-quotas", "--codex-profile", value]).is_err());
        }
        let args = Args::try_parse_from([
            "ai-quotas",
            "--codex-profile",
            "First=/tmp/one",
            "--codex-profile",
            "First=/tmp/two",
        ])
        .unwrap();
        assert!(selected_jobs(&args).is_err());
    }
}
