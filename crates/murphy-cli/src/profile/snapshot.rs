use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
struct ProfileInvocation {
    pub kind: ProfileKind,
    pub cop_name: Option<String>,
    pub file: String,
    pub start_micros: u64,
    pub wall_micros: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProfileKind {
    Parse,
    NativeCopFile,
    NativeCopDispatch,
    MrubyCop,
}

impl ProfileKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::NativeCopFile => "native_cop_file",
            Self::NativeCopDispatch => "native_cop_dispatch",
            Self::MrubyCop => "mruby_cop",
        }
    }

    fn order_key(self) -> u8 {
        match self {
            Self::Parse => 0,
            Self::NativeCopFile => 1,
            Self::NativeCopDispatch => 2,
            Self::MrubyCop => 3,
        }
    }
}

#[derive(Default)]
pub struct ProfileSummary {
    /// Cop -> total wall time across all observed invocations (microseconds).
    cop_wall_micros: BTreeMap<String, u64>,

    /// Cop -> wall time spent in inspect_file stage.
    cop_file_wall_micros: BTreeMap<String, u64>,

    /// Cop -> wall time spent in dispatch stage.
    cop_dispatch_wall_micros: BTreeMap<String, u64>,

    /// Cop -> file -> wall time for that pair (microseconds).
    cop_file_micros: BTreeMap<String, BTreeMap<String, u64>>,

    /// Cop -> file -> wall time spent in inspect_file stage.
    cop_file_stage_file_micros: BTreeMap<String, BTreeMap<String, u64>>,

    /// Cop -> file -> wall time spent in dispatch stage.
    cop_dispatch_stage_file_micros: BTreeMap<String, BTreeMap<String, u64>>,

    /// File -> total wall time (native + mruby cop invocations on that file, microseconds).
    file_total_micros: BTreeMap<String, u64>,

    /// Number of cop-file invocations recorded for each cop.
    cop_invocation_count: BTreeMap<String, u64>,

    /// Number of inspect_file invocations recorded for each cop.
    cop_file_invocation_count: BTreeMap<String, u64>,

    /// Number of dispatch invocations recorded for each cop.
    cop_dispatch_invocation_count: BTreeMap<String, u64>,

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

    fn record(&mut self, kind: ProfileKind, cop: &str, file: &str, micros: u64) {
        if micros == 0 {
            return;
        }

        self.push_invocation(kind, Some(cop.to_string()), file, micros);
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

        match kind {
            ProfileKind::NativeCopFile => {
                *self
                    .cop_file_wall_micros
                    .entry(cop.to_string())
                    .or_default() += micros;
                *self
                    .cop_file_stage_file_micros
                    .entry(cop.to_string())
                    .or_default()
                    .entry(file.to_string())
                    .or_default() += micros;
                *self
                    .cop_file_invocation_count
                    .entry(cop.to_string())
                    .or_default() += 1;
            }
            ProfileKind::NativeCopDispatch => {
                *self
                    .cop_dispatch_wall_micros
                    .entry(cop.to_string())
                    .or_default() += micros;
                *self
                    .cop_dispatch_stage_file_micros
                    .entry(cop.to_string())
                    .or_default()
                    .entry(file.to_string())
                    .or_default() += micros;
                *self
                    .cop_dispatch_invocation_count
                    .entry(cop.to_string())
                    .or_default() += 1;
            }
            ProfileKind::MrubyCop | ProfileKind::Parse => {
                // parse and mruby are handled through dedicated code paths.
            }
        }

        self.cop_invocation_samples.push(micros);
    }

    pub fn record_native_file(&mut self, cop: &str, file: &str, micros: u64) {
        self.record(ProfileKind::NativeCopFile, cop, file, micros);
    }

    pub fn record_native_dispatch(&mut self, cop: &str, file: &str, micros: u64) {
        self.record(ProfileKind::NativeCopDispatch, cop, file, micros);
    }

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

    pub fn record_parse(&mut self, file: &str, micros: u128) {
        let wall = Self::to_u64(micros);
        self.push_invocation(ProfileKind::Parse, None, file, wall);
    }

    pub fn p95_us(&self) -> u64 {
        if self.cop_invocation_samples.is_empty() {
            return 0;
        }

        let mut samples = self.cop_invocation_samples.clone();
        samples.sort_unstable();
        let index = ((samples.len() * 95).saturating_sub(1)) / 100;
        samples[index]
    }

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

    pub fn unique_files(&self) -> Vec<String> {
        self.timeline
            .iter()
            .fold(BTreeSet::new(), |mut files, invocation| {
                files.insert(invocation.file.clone());
                files
            })
            .into_iter()
            .collect()
    }

    pub fn to_summary_profile(&self) -> serde_json::Value {
        let hot_files = self
            .hot_files(5)
            .into_iter()
            .map(|(file, micros)| serde_json::json!({"file": file, "wall_micros": micros}))
            .collect::<Vec<_>>();

        serde_json::json!({
            "cop_wall_micros": self.cop_wall_micros,
            "cop_file_wall_micros": self.cop_file_wall_micros,
            "cop_dispatch_wall_micros": self.cop_dispatch_wall_micros,
            "cop_file_micros": self.cop_file_micros,
            "cop_file_stage_file_micros": self.cop_file_stage_file_micros,
            "cop_dispatch_stage_file_micros": self.cop_dispatch_stage_file_micros,
            "cop_file_invocation_count": self.cop_file_invocation_count,
            "cop_dispatch_invocation_count": self.cop_dispatch_invocation_count,
            "p95_micros": self.p95_us(),
            "hot_files": hot_files,
            "invocation_count": self.cop_invocation_count,
        })
    }

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
