use crate::profile::{ProfileFormatter, ProfileSummary};
use serde_json::Value;

pub struct SpeedscopeFormatter;

impl ProfileFormatter for SpeedscopeFormatter {
    fn format(&self, summary: &ProfileSummary) -> Value {
        summary.to_speedscope()
    }
}
