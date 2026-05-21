use serde_json::Value;

pub mod snapshot;
pub mod speedscope;
pub mod summary;

pub use snapshot::ProfileSummary;
pub use speedscope::SpeedscopeFormatter;
pub use summary::SummaryFormatter;

pub trait ProfileFormatter {
    fn format(&self, summary: &ProfileSummary) -> Value;
}

/// Supported output modes for `--profile`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ProfileOutputFormat {
    Summary,
    Speedscope,
}

impl ProfileOutputFormat {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "summary" => Some(Self::Summary),
            "speedscope" => Some(Self::Speedscope),
            _ => None,
        }
    }
}
