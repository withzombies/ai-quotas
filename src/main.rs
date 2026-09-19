use jiff::Timestamp;
use jiff::tz::TimeZone;
use quotas::{model, render, verdict};

fn main() {
    let statuses: Vec<model::ProviderStatus> = Vec::new();
    let now = Timestamp::now();
    print!("{}", render::table(&statuses, now, &TimeZone::system()));
    println!("{}", verdict::verdict_line(&statuses, now));
}
