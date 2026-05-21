use crate::profile::{ProfileFormatter, ProfileSummary};
use serde_json::Value;

pub struct SummaryFormatter;

impl ProfileFormatter for SummaryFormatter {
    fn format(&self, summary: &ProfileSummary) -> Value {
        summary.to_summary_profile()
    }
}
