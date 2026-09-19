pub mod claude;
pub mod codex;
pub mod grok;
pub mod zai;

use crate::model::ProviderStatus;
use jiff::Timestamp;

pub type FetchFn = fn(Timestamp) -> ProviderStatus;

/// Fixed provider order; it is also the verdict's final tiebreak.
pub const ALL: [(&str, FetchFn); 4] = [
    (claude::NAME, |now| claude::fetch(claude::BASE_URL, now)),
    (codex::NAME, |now| codex::fetch(codex::BASE_URL, now)),
    (zai::NAME, |_now| zai::fetch(zai::BASE_URL)),
    (grok::NAME, |now| grok::fetch(grok::BASE_URL, now)),
];
