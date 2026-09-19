use jiff::Timestamp;
use jiff::tz::TimeZone;
use quotas::model::ProviderStatus;
use quotas::{providers, render, verdict};

fn main() {
    let now = Timestamp::now();

    let statuses: Vec<ProviderStatus> = std::thread::scope(|s| {
        let handles: Vec<_> = providers::ALL
            .iter()
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

    print!("{}", render::table(&statuses, now, &TimeZone::system()));
    println!("{}", verdict::verdict_line(&statuses, now));

    if !statuses.iter().any(|s| s.ok()) {
        std::process::exit(1);
    }
}
