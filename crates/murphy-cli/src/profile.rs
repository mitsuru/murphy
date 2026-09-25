//! `--profile` aggregation (Phase 9 B6, murphy-fmw.2.6).
//!
//! Collects per-cop wall times from the dispatcher's timed path
//! ([`murphy_core::dispatch::run_cops_with_options_context_and_diagnostics_timed`])
//! into the cop x file matrix the Phase 9 gate requires: per-cop wall time,
//! p95 over cop x file invocations, the cop x file matrix itself, and
//! hot-file detection — all emitted as JSON.
//!
//! [`ProfileSummary::to_summary_profile`] is the gate shape (the legacy
//! `--profile` keys, restored on the new dispatcher). [`to_speedscope`] keeps
//! the legacy Speedscope trace for browser-based flame analysis.
//!
//! Timer granularity is microseconds: a cop x file invocation under 1us
//! records `0` and is skipped in the wall-time maps (timeline only), so very
//! fast cops on tiny files may be absent from `cop_wall_micros`. Sums are
//! `u64` microseconds with saturating `u64::MAX` fallback on overflow.

use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
struct ProfileInvocation {
    kind: ProfileKind,
    cop_name: Option<String>,
    file: String,
    start_micros: u64,
    wall_micros: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProfileKind {
    Parse,
    NativeCop,
    MrubyCop,
}

impl ProfileKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::NativeCop => "native_cop",
            Self::MrubyCop => "mruby_cop",
        }
    }

    fn order_key(self) -> u8 {
        match self {
            Self::Parse => 0,
            Self::NativeCop => 1,
            Self::MrubyCop => 2,
        }
    }
}

/// Per-run profile sink. One lint run fills exactly one summary; files are
/// recorded independently (profiling bypasses content memoization so the
/// cop x file matrix attributes wall time per real file — see
/// `lint_files_profiled`).
#[derive(Default)]
pub struct ProfileSummary {
    /// Cop -> total wall time across all observed invocations (microseconds).
    cop_wall_micros: BTreeMap<String, u64>,

    /// Cop -> file -> wall time for that pair (microseconds).
    cop_file_micros: BTreeMap<String, BTreeMap<String, u64>>,

    /// File -> total wall time (native + mruby cop invocations on that file,
    /// microseconds). Parse time is excluded: hot files are cop hot spots.
    file_total_micros: BTreeMap<String, u64>,

    /// Number of cop x file invocations recorded for each cop.
    cop_invocation_count: BTreeMap<String, u64>,

    /// Raw per-invocation wall times for p95 calculation (microseconds).
    cop_invocation_samples: Vec<u64>,

    timeline: Vec<ProfileInvocation>,
    timeline_cursor: u64,
}

impl ProfileSummary {
    fn to_u64(value: u128) -> u64 {
        u64::try_from(value).unwrap_or(u64::MAX)
    }

    fn next_timeline_start(&mut self) -> u64 {
        let start = self.timeline_cursor;
        self.timeline_cursor += 1;
        start
    }

    fn timeline_dur(value: u64) -> u64 {
        if value == 0 { 1 } else { value }
    }

    fn push_invocation(
        &mut self,
        kind: ProfileKind,
        cop_name: Option<String>,
        file: &str,
        micros: u64,
    ) {
        let wall_micros = Self::timeline_dur(micros);
        self.timeline_cursor += wall_micros.saturating_sub(1);
        let invocation = ProfileInvocation {
            kind,
            cop_name,
            file: file.to_string(),
            start_micros: self.next_timeline_start(),
            wall_micros,
        };
        self.timeline.push(invocation);
    }

    fn record(&mut self, cop: &str, file: &str, micros: u64) {
        if micros == 0 {
            return;
        }

        self.push_invocation(ProfileKind::NativeCop, Some(cop.to_string()), file, micros);
        *self.cop_wall_micros.entry(cop.to_string()).or_default() += micros;
        *self
            .cop_file_micros
            .entry(cop.to_string())
            .or_default()
            .entry(file.to_string())
            .or_default() += micros;
        *self.file_total_micros.entry(file.to_string()).or_default() += micros;
        *self
            .cop_invocation_count
            .entry(cop.to_string())
            .or_default() += 1;
        self.cop_invocation_samples.push(micros);
    }

    /// Record one native cop x file wall time (microseconds). Zero-wall
    /// invocations (sub-microsecond) are skipped in the maps.
    pub fn record_native(&mut self, cop: &str, file: &str, micros: u64) {
        self.record(cop, file, micros);
    }

    /// Record one mruby user-cop x file wall time (microseconds). The
    /// timeline keeps the mruby kind so traces distinguish engine cops from
    /// user cops; the wall-time maps aggregate both uniformly.
    pub fn record_mruby(&mut self, cop: &str, file: &str, micros: u64) {
        self.push_invocation(ProfileKind::MrubyCop, Some(cop.to_string()), file, micros);
        if micros == 0 {
            return;
        }
        *self.cop_wall_micros.entry(cop.to_string()).or_default() += micros;
        *self
            .cop_file_micros
            .entry(cop.to_string())
            .or_default()
            .entry(file.to_string())
            .or_default() += micros;
        *self.file_total_micros.entry(file.to_string()).or_default() += micros;
        *self
            .cop_invocation_count
            .entry(cop.to_string())
            .or_default() += 1;
        self.cop_invocation_samples.push(micros);
    }

    /// Record one file's parse wall time. Timeline-only: parse feeds the
    /// Speedscope trace but never the cop wall maps or hot files.
    pub fn record_parse(&mut self, file: &str, micros: u128) {
        let wall = Self::to_u64(micros);
        self.push_invocation(ProfileKind::Parse, None, file, wall);
    }

    /// p95 over recorded cop x file invocation wall times (microseconds).
    /// Zero when nothing was recorded (e.g. every cop was sub-microsecond).
    pub fn p95_us(&self) -> u64 {
        if self.cop_invocation_samples.is_empty() {
            return 0;
        }

        let mut samples = self.cop_invocation_samples.clone();
        samples.sort_unstable();
        let index = ((samples.len() * 95).saturating_sub(1)) / 100;
        samples[index]
    }

    /// Top `limit` files by total cop wall time, ties broken by path.
    pub fn hot_files(&self, limit: usize) -> Vec<(String, u64)> {
        let mut entries: Vec<(String, u64)> = self
            .file_total_micros
            .iter()
            .map(|(file, micros)| (file.clone(), *micros))
            .collect();

        entries.sort_by(|(left_file, left_time), (right_file, right_time)| {
            right_time
                .cmp(left_time)
                .then_with(|| left_file.cmp(right_file))
        });

        entries.truncate(limit);
        entries
    }

    fn unique_files(&self) -> Vec<String> {
        self.timeline
            .iter()
            .fold(BTreeSet::new(), |mut files, invocation| {
                files.insert(invocation.file.clone());
                files
            })
            .into_iter()
            .collect()
    }

    /// Phase 9 gate 5 shape: cop wall times + p95 + cop x file matrix +
    /// hot files, as JSON.
    pub fn to_summary_profile(&self) -> serde_json::Value {
        let hot_files = self
            .hot_files(5)
            .into_iter()
            .map(|(file, micros)| serde_json::json!({"file": file, "wall_micros": micros}))
            .collect::<Vec<_>>();

        serde_json::json!({
            "cop_wall_micros": self.cop_wall_micros,
            "cop_file_micros": self.cop_file_micros,
            "p95_micros": self.p95_us(),
            "hot_files": hot_files,
            "invocation_count": self.cop_invocation_count,
        })
    }

    /// Speedscope-compatible `traceEvents` payload (legacy
    /// `--profile-format speedscope` shape). One thread per file, ordered by
    /// file path so thread ids are deterministic.
    pub fn to_speedscope(&self) -> serde_json::Value {
        let mut files = self.unique_files();
        files.sort_unstable();

        let mut file_to_tid = BTreeMap::new();
        for (idx, file) in files.iter().enumerate() {
            file_to_tid.insert(file.as_str(), idx as u64 + 1);
        }

        let mut events = self
            .timeline
            .iter()
            .map(|inv| {
                let process_id = 1;
                let thread_id = file_to_tid.get(inv.file.as_str()).copied().unwrap_or(1);
                let cop_name = inv.cop_name.as_deref().unwrap_or("");
                let name = if inv.kind == ProfileKind::Parse {
                    String::from("parse")
                } else {
                    format!("{kind}:{cop}", kind = inv.kind.as_str(), cop = cop_name)
                };
                let event = serde_json::json!({
                    "name": name,
                    "cat": inv.kind.as_str(),
                    "ph": "X",
                    "ts": inv.start_micros,
                    "dur": inv.wall_micros,
                    "pid": process_id,
                    "tid": thread_id,
                    "args": {
                        "file": inv.file.clone(),
                        "cop": cop_name,
                        "thread_name": format!("file:{file}", file = inv.file),
                    },
                });
                (
                    inv.start_micros,
                    thread_id,
                    inv.kind.order_key(),
                    inv.file.clone(),
                    event,
                )
            })
            .collect::<Vec<_>>();

        events.sort_by(
            |(left_start, left_tid, left_kind, left_file, _),
             (right_start, right_tid, right_kind, right_file, _)| {
                left_start
                    .cmp(right_start)
                    .then_with(|| left_tid.cmp(right_tid))
                    .then_with(|| left_kind.cmp(right_kind))
                    .then_with(|| left_file.cmp(right_file))
            },
        );

        let events = events
            .into_iter()
            .map(|(_, _, _, _, event)| event)
            .collect::<Vec<_>>();

        serde_json::json!({
            "traceEvents": events,
            "event_count": self.timeline.len(),
            "process_name": "murphy-lint",
            "pid": 1,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_summary() -> ProfileSummary {
        let mut s = ProfileSummary::default();
        s.record_parse("a.rb", 10);
        s.record_native("Lint/Debugger", "a.rb", 100);
        s.record_native("Lint/Debugger", "b.rb", 300);
        s.record_native("Style/Foo", "a.rb", 200);
        s.record_mruby("Murphy/Mruby/bar", "b.rb", 400);
        s
    }

    #[test]
    fn summary_aggregates_cop_wall_and_matrix() {
        let s = sample_summary();
        let v = s.to_summary_profile();
        assert_eq!(v["cop_wall_micros"]["Lint/Debugger"], 400);
        assert_eq!(v["cop_wall_micros"]["Style/Foo"], 200);
        assert_eq!(v["cop_wall_micros"]["Murphy/Mruby/bar"], 400);
        assert_eq!(v["cop_file_micros"]["Lint/Debugger"]["a.rb"], 100);
        assert_eq!(v["cop_file_micros"]["Lint/Debugger"]["b.rb"], 300);
        assert_eq!(v["cop_file_micros"]["Style/Foo"]["a.rb"], 200);
        assert_eq!(v["invocation_count"]["Lint/Debugger"], 2);
        assert_eq!(v["invocation_count"]["Style/Foo"], 1);
    }

    #[test]
    fn summary_hot_files_rank_by_cop_wall_with_path_tiebreak() {
        let s = sample_summary();
        // a.rb: 100 + 200 = 300; b.rb: 300 + 400 = 700.
        let v = s.to_summary_profile();
        let hot = v["hot_files"].as_array().expect("hot_files array");
        assert_eq!(hot.len(), 2);
        assert_eq!(hot[0]["file"], "b.rb");
        assert_eq!(hot[0]["wall_micros"], 700);
        assert_eq!(hot[1]["file"], "a.rb");
        assert_eq!(hot[1]["wall_micros"], 300);
    }

    #[test]
    fn summary_p95_over_invocation_samples() {
        let s = sample_summary();
        // Samples [100, 300, 200, 400] -> sorted [100, 200, 300, 400],
        // index (4*95-1)/100 = 3 -> 400.
        assert_eq!(s.p95_us(), 400);
        assert_eq!(s.to_summary_profile()["p95_micros"], 400);
    }

    #[test]
    fn summary_skips_zero_wall_invocations_in_maps() {
        let mut s = ProfileSummary::default();
        s.record_native("Lint/Debugger", "a.rb", 0);
        s.record_mruby("Murphy/Mruby/bar", "a.rb", 0);
        let v = s.to_summary_profile();
        assert!(v["cop_wall_micros"].as_object().unwrap().is_empty());
        assert!(v["cop_file_micros"].as_object().unwrap().is_empty());
        assert_eq!(v["p95_micros"], 0);
        assert!(v["hot_files"].as_array().unwrap().is_empty());
    }

    #[test]
    fn summary_parse_feeds_timeline_but_not_cop_maps() {
        let mut s = ProfileSummary::default();
        s.record_parse("a.rb", 50);
        let v = s.to_summary_profile();
        assert!(v["cop_wall_micros"].as_object().unwrap().is_empty());
        let trace = s.to_speedscope();
        let events = trace["traceEvents"].as_array().expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["name"], "parse");
        assert_eq!(trace["event_count"], 1);
        assert_eq!(trace["process_name"], "murphy-lint");
    }

    #[test]
    fn speedscope_thread_ids_follow_sorted_file_order() {
        let mut s = ProfileSummary::default();
        s.record_native("Lint/B", "z.rb", 5);
        s.record_native("Lint/A", "a.rb", 7);
        let trace = s.to_speedscope();
        let events = trace["traceEvents"].as_array().expect("events");
        assert_eq!(
            events.len(),
            trace["event_count"].as_u64().unwrap() as usize
        );
        let mut tids: Vec<(u64, String)> = events
            .iter()
            .map(|e| {
                (
                    e["tid"].as_u64().expect("tid"),
                    e["args"]["thread_name"]
                        .as_str()
                        .expect("thread")
                        .to_owned(),
                )
            })
            .collect();
        tids.sort();
        tids.dedup();
        let files: Vec<String> = tids.into_iter().map(|(_, t)| t).collect();
        assert_eq!(files, vec!["file:a.rb", "file:z.rb"]);
    }
}
