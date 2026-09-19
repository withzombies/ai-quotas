mod model;
mod verdict;

use jiff::Timestamp;

fn main() {
    let statuses: Vec<model::ProviderStatus> = Vec::new();
    println!("{}", verdict::verdict_line(&statuses, Timestamp::now()));
}
