use clap::Parser;
use jiff::Timestamp;
use jiff::tz::TimeZone;
use quotas::model::ProviderStatus;
use quotas::{providers, render, verdict};

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
}

fn main() {
    let args = Args::parse();
    let now = Timestamp::now();

    let selected: Vec<_> = providers::ALL
        .iter()
        .filter(|(name, _)| args.provider.is_empty() || args.provider.iter().any(|p| p == name))
        .collect();

    let statuses: Vec<ProviderStatus> = std::thread::scope(|s| {
        let handles: Vec<_> = selected
            .into_iter()
            .map(|(name, fetch)| (*name, s.spawn(move || fetch(now))))
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
