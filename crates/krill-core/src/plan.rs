//! Plan mode (M5c, design §12.1 decision 6): a planner agent decomposes
//! a goal into a plan.md checklist, a human approves it, and the duet
//! referee walks the tasks. plan.md itself is the task queue — the next
//! task is the first unchecked box, so humans can edit it mid-flight.
//! This module is pure parsing + the plan state kv; IO stays in the
//! binary crate.

use crate::error::{Context, Result};
use crate::{kv, msg};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanPhase {
    /// The planner is (supposed to be) writing plan.md.
    Planning,
    /// plan.md exists — waiting for `krill approve`.
    Ready,
    /// Tasks are being walked by the duet referee.
    Running,
    Done,
}

impl PlanPhase {
    pub fn parse(s: &str) -> Option<PlanPhase> {
        match s {
            "planning" => Some(PlanPhase::Planning),
            "ready" => Some(PlanPhase::Ready),
            "running" => Some(PlanPhase::Running),
            "done" => Some(PlanPhase::Done),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            PlanPhase::Planning => "planning",
            PlanPhase::Ready => "ready",
            PlanPhase::Running => "running",
            PlanPhase::Done => "done",
        }
    }
}

/// Plan meta-state, persisted as `state/<id>.plan.kv`. The duet params
/// are captured at `krill plan` time and applied per task at approve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanState {
    pub phase: PlanPhase,
    pub goal: String,
    pub reviewer: Option<String>,
    pub gate: String,
    pub max_rounds: u32,
    /// Re-instruction attempts when the planner ends without plan.md.
    pub retries: u32,
}

impl PlanState {
    pub fn new(goal: &str, reviewer: Option<String>, gate: &str, max_rounds: u32) -> PlanState {
        PlanState {
            phase: PlanPhase::Planning,
            goal: goal.to_string(),
            reviewer,
            gate: gate.to_string(),
            max_rounds,
            retries: 0,
        }
    }

    fn to_map(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("phase".into(), self.phase.as_str().into());
        m.insert("goal".into(), self.goal.clone());
        if let Some(r) = &self.reviewer {
            m.insert("reviewer".into(), r.clone());
        }
        m.insert("gate".into(), self.gate.clone());
        m.insert("max_rounds".into(), self.max_rounds.to_string());
        m.insert("retries".into(), self.retries.to_string());
        m
    }

    fn from_map(m: &BTreeMap<String, String>) -> Result<PlanState> {
        let req = |k: &str| -> Result<String> {
            m.get(k).cloned().with_context(|| msg::meta_field_missing(k))
        };
        Ok(PlanState {
            phase: PlanPhase::parse(&req("phase")?)
                .with_context(|| msg::meta_field_missing("phase"))?,
            goal: m.get("goal").cloned().unwrap_or_default(),
            reviewer: m.get("reviewer").cloned(),
            gate: m.get("gate").cloned().unwrap_or_default(),
            max_rounds: req("max_rounds")?.parse().context(msg::duet_state_parse_failed())?,
            retries: m.get("retries").map_or(Ok(0), |v| v.parse()).context(msg::duet_state_parse_failed())?,
        })
    }

    pub fn save(&self, id: &str) -> Result<()> {
        let path = state_path(id)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        kv::write_file(&path, &self.to_map())
    }

    pub fn load(id: &str) -> Result<PlanState> {
        PlanState::from_map(&kv::read_file(&state_path(id)?)?)
    }

    pub fn delete(id: &str) {
        if let Ok(p) = state_path(id) {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// `state/<id>.plan.kv` — next to the hook/duet state files.
pub fn state_path(id: &str) -> Result<PathBuf> {
    Ok(crate::session::state_dir()?.join(format!("{id}.plan.kv")))
}

// ---- plan.md checklist parsing ---------------------------------------------

/// A checklist line: `- [ ] task` / `- [x] task` (also `*` bullets, any
/// leading indentation, case-insensitive x). Everything else is prose.
fn parse_line(line: &str) -> Option<(&str, bool)> {
    let t = line.trim_start();
    let rest = t.strip_prefix("- ").or_else(|| t.strip_prefix("* "))?;
    let (mark, text) = if let Some(r) = rest.strip_prefix("[ ]") {
        (false, r)
    } else if let Some(r) = rest.strip_prefix("[x]").or_else(|| rest.strip_prefix("[X]")) {
        (true, r)
    } else {
        return None;
    };
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some((text, mark))
}

/// All checklist tasks in order: (text, done).
pub fn parse_plan(md: &str) -> Vec<(String, bool)> {
    md.lines()
        .filter_map(parse_line)
        .map(|(t, d)| (t.to_string(), d))
        .collect()
}

/// The next task to run — the first unchecked box.
pub fn first_open_task(md: &str) -> Option<String> {
    md.lines()
        .filter_map(parse_line)
        .find(|(_, done)| !done)
        .map(|(t, _)| t.to_string())
}

/// (done, total) checkbox counts.
pub fn progress(md: &str) -> (usize, usize) {
    let tasks = parse_plan(md);
    (tasks.iter().filter(|(_, d)| *d).count(), tasks.len())
}

/// Check off the first unchecked box whose text is `task`, preserving
/// the rest of the document byte for byte.
pub fn check_task(md: &str, task: &str) -> String {
    let mut done = false;
    let mut out: Vec<String> = Vec::new();
    for line in md.lines() {
        if !done {
            if let Some((text, false)) = parse_line(line) {
                if text == task {
                    let checked = line
                        .replacen("[ ]", "[x]", 1);
                    out.push(checked);
                    done = true;
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }
    let mut s = out.join("\n");
    if md.ends_with('\n') {
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = "\
# plan

intro prose
- [x] set up the module
- [ ] wire the parser
  - [ ] nested subtask
* [X] star bullet done
- [] not a checkbox
- [ ]
- [ ] final task
";

    #[test]
    fn parses_checklist_lines_only() {
        let tasks = parse_plan(PLAN);
        assert_eq!(
            tasks,
            vec![
                ("set up the module".to_string(), true),
                ("wire the parser".to_string(), false),
                ("nested subtask".to_string(), false),
                ("star bullet done".to_string(), true),
                ("final task".to_string(), false),
            ]
        );
        assert_eq!(progress(PLAN), (2, 5));
        assert!(parse_plan("no boxes here\n").is_empty());
    }

    #[test]
    fn first_open_task_walks_in_order() {
        assert_eq!(first_open_task(PLAN).as_deref(), Some("wire the parser"));
        assert_eq!(first_open_task("- [x] all done\n"), None);
        assert_eq!(first_open_task(""), None);
    }

    #[test]
    fn check_task_flips_exactly_one_box() {
        let updated = check_task(PLAN, "wire the parser");
        assert_eq!(first_open_task(&updated).as_deref(), Some("nested subtask"));
        assert_eq!(progress(&updated), (3, 5));
        // Everything else survives byte for byte.
        assert!(updated.contains("intro prose"));
        assert!(updated.contains("- [] not a checkbox"));
        assert!(updated.ends_with('\n'));
        // Same-text duplicates: only the first open one flips.
        let dup = "- [ ] same\n- [ ] same\n";
        let once = check_task(dup, "same");
        assert_eq!(progress(&once), (1, 2));
        // Unknown task: no change.
        assert_eq!(check_task(PLAN, "nonexistent"), PLAN);
    }

    #[test]
    fn state_kv_roundtrip_and_phases() {
        let s = PlanState::new("big goal", Some("codex".into()), "cargo test", 2);
        let back = PlanState::from_map(&s.to_map()).unwrap();
        assert_eq!(back, s);

        let mut running = s.clone();
        running.phase = PlanPhase::Running;
        running.retries = 1;
        running.reviewer = None;
        let back = PlanState::from_map(&running.to_map()).unwrap();
        assert_eq!(back, running);

        let mut bad = s.to_map();
        bad.insert("phase".into(), "thinking".into());
        assert!(PlanState::from_map(&bad).is_err());

        for p in [PlanPhase::Planning, PlanPhase::Ready, PlanPhase::Running, PlanPhase::Done] {
            assert_eq!(PlanPhase::parse(p.as_str()), Some(p));
        }
    }
}
