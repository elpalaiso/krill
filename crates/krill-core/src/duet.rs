//! Duet (M5b, design §12.1 decision 4): turn-based worker/reviewer
//! ping-pong over one shared worktree. This module is the *referee's
//! brain* — a pure state machine. All IO (send-keys, gate subprocess,
//! ntfy, file deletion) lives in the binary crate, which only executes
//! the Action this module decides.

use crate::error::{Context, Result};
use crate::{kv, msg};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Which half of the duet a session is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuetRole {
    Worker,
    Reviewer,
}

impl DuetRole {
    pub fn parse(s: &str) -> Option<DuetRole> {
        match s {
            "worker" => Some(DuetRole::Worker),
            "reviewer" => Some(DuetRole::Reviewer),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            DuetRole::Worker => "worker",
            DuetRole::Reviewer => "reviewer",
        }
    }
}

/// Duet membership stored on a session's meta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuetRef {
    pub role: DuetRole,
    /// The peer session's name (same repo).
    pub peer: String,
}

/// Whose event the referee will act on next — the turn mutex. Stop
/// hooks fire on every turn, so anything not matching `awaiting` is
/// ignored (idempotent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Awaiting {
    Worker,
    Reviewer,
    /// LGTM received; a detached gate run decides completion.
    Gate,
    Done,
    /// Round cap hit — a human has been called (needs-you).
    Stalled,
}

impl Awaiting {
    pub fn parse(s: &str) -> Option<Awaiting> {
        match s {
            "worker" => Some(Awaiting::Worker),
            "reviewer" => Some(Awaiting::Reviewer),
            "gate" => Some(Awaiting::Gate),
            "done" => Some(Awaiting::Done),
            "stalled" => Some(Awaiting::Stalled),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Awaiting::Worker => "worker",
            Awaiting::Reviewer => "reviewer",
            Awaiting::Gate => "gate",
            Awaiting::Done => "done",
            Awaiting::Stalled => "stalled",
        }
    }
}

/// Mutable duet state, persisted as `state/<worker-id>.duet.kv`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuetState {
    /// Rework cycles sent back to the worker so far.
    pub round: u32,
    pub max_rounds: u32,
    /// Objective gate command ("" = none; LGTM alone completes).
    pub gate: String,
    /// The task goal, echoed into review instructions.
    pub goal: String,
    pub awaiting: Awaiting,
    /// The worker was re-nudged once this turn after going idle
    /// (aborted provider turn — design §12.1 decision 7). A second
    /// idle escalates to Stalled instead of nudging forever.
    pub nudged: bool,
}

impl DuetState {
    pub fn new(max_rounds: u32, gate: &str, goal: &str) -> DuetState {
        DuetState {
            round: 0,
            max_rounds,
            gate: gate.to_string(),
            goal: goal.to_string(),
            awaiting: Awaiting::Worker,
            nudged: false,
        }
    }

    fn to_map(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("round".into(), self.round.to_string());
        m.insert("max_rounds".into(), self.max_rounds.to_string());
        m.insert("gate".into(), self.gate.clone());
        m.insert("goal".into(), self.goal.clone());
        m.insert("awaiting".into(), self.awaiting.as_str().into());
        m.insert("nudged".into(), self.nudged.to_string());
        m
    }

    fn from_map(m: &BTreeMap<String, String>) -> Result<DuetState> {
        let req = |k: &str| -> Result<String> {
            m.get(k).cloned().with_context(|| msg::meta_field_missing(k))
        };
        Ok(DuetState {
            round: req("round")?.parse().context(msg::duet_state_parse_failed())?,
            max_rounds: req("max_rounds")?.parse().context(msg::duet_state_parse_failed())?,
            gate: m.get("gate").cloned().unwrap_or_default(),
            goal: m.get("goal").cloned().unwrap_or_default(),
            awaiting: Awaiting::parse(&req("awaiting")?)
                .with_context(|| msg::meta_field_missing("awaiting"))?,
            // Absent in pre-M5d state files — an old walk resumes clean.
            nudged: m.get("nudged").map(|v| v == "true").unwrap_or(false),
        })
    }

    pub fn save(&self, worker_id: &str) -> Result<()> {
        let path = state_path(worker_id)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        kv::write_file(&path, &self.to_map())
    }

    pub fn load(worker_id: &str) -> Result<DuetState> {
        DuetState::from_map(&kv::read_file(&state_path(worker_id)?)?)
    }

    pub fn delete(worker_id: &str) {
        if let Ok(p) = state_path(worker_id) {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// `state/<worker-id>.duet.kv` — next to the hook state files.
pub fn state_path(worker_id: &str) -> Result<PathBuf> {
    Ok(crate::session::state_dir()?.join(format!("{worker_id}.duet.kv")))
}

/// The reviewer's REVIEW.md verdict: decided by the first line only, so
/// the rest of the file is free-form review text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Lgtm,
    Issues,
    /// REVIEW.md absent or unreadable — the reviewer skipped protocol.
    Missing,
}

pub fn parse_verdict(review: Option<&str>) -> Verdict {
    match review {
        Some(text) => match text.lines().next().map(str::trim) {
            Some(first) if first.eq_ignore_ascii_case("LGTM") => Verdict::Lgtm,
            // Any other content counts as issues to fix — a reviewer
            // that wrote prose without the marker still gets heard.
            Some(_) => Verdict::Issues,
            None => Verdict::Missing,
        },
        None => Verdict::Missing,
    }
}

/// What an agent's Notification hook payload means (design §12.1
/// decision 7). Only Idle may ever be answered with typed input —
/// send-keys into a permission dialog presses arbitrary buttons, so
/// Permission and Unknown must never be nudged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    /// The agent is sitting at an empty prompt waiting for input.
    Idle,
    /// The agent is asking the human to approve something.
    Permission,
    /// Empty or unrecognized payload (other agents, old presets).
    Unknown,
}

/// Classify a Notification hook payload by its message text. Pure
/// substring matching on the raw JSON — core stays std-only, and the
/// shapes come from the `claude-code` hook preset; agents without a
/// payload land on Unknown and are left alone.
pub fn classify_notification(payload: &str) -> NotificationKind {
    let p = payload.to_ascii_lowercase();
    // Permission wins over idle if both somehow appear — never type.
    if p.contains("permission") || p.contains("approval") {
        NotificationKind::Permission
    } else if p.contains("waiting for your input") || p.contains("waiting for input") {
        NotificationKind::Idle
    } else {
        NotificationKind::Unknown
    }
}

/// Triage material for a stall notification: the first finding in a
/// REVIEW.md body and how many findings there are, so the human can
/// judge "valid → resume" from the push alone (§12.1 decision 7). The
/// verdict line is skipped; findings are top-level `- ` bullets, with
/// the first non-empty body line as fallback. `max_chars` bounds the
/// excerpt (char-safe, `…`-terminated).
pub fn review_excerpt(review: &str, max_chars: usize) -> Option<(String, usize)> {
    let body: Vec<&str> = review.lines().skip(1).collect();
    let bullets: Vec<&str> = body
        .iter()
        .map(|l| l.trim_start())
        .filter(|l| l.starts_with("- "))
        .map(|l| l["- ".len()..].trim())
        .collect();
    let (first, count) = match bullets.split_first() {
        Some((first, rest)) => (*first, rest.len() + 1),
        None => (*body.iter().find(|l| !l.trim().is_empty())?, 1),
    };
    let first = first.trim();
    let excerpt = if first.chars().count() > max_chars {
        let cut: String = first.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{cut}…")
    } else {
        first.to_string()
    };
    Some((excerpt, count))
}

/// A turn-end (or gate-end) event the referee reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    WorkerDone,
    ReviewerDone(Verdict),
    GateFinished { pass: bool },
    /// A human resumes a stalled duet (`krill resume`), optionally with
    /// a new round cap. Only valid while Stalled — out of turn otherwise.
    Resume { new_max: Option<u32> },
    /// The worker went idle mid-task (idle Notification while the duet
    /// awaits the worker — typically an aborted provider turn, design
    /// §12.1 decision 7). Out of turn unless awaiting=worker.
    WorkerIdle,
}

/// What the binary must do after a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Delete REVIEW.md/GATE.md, then instruct the reviewer to review.
    PingReviewer,
    /// Instruct the worker to address REVIEW.md.
    PingWorkerReview,
    /// Instruct the worker to fix the gate failure (GATE.md).
    PingWorkerGate,
    /// The reviewer skipped REVIEW.md — ask again.
    ReinstructReviewer,
    /// A human resumed a stalled duet: instruct the worker to pick the
    /// task back up (REVIEW.md/GATE.md may or may not still exist —
    /// the stall cause isn't recorded, so the instruction names both).
    PingWorkerResume,
    /// LGTM with a gate configured: run it detached.
    RunGate,
    /// Duet finished successfully.
    Complete,
    /// Round cap hit: mark needs-you and call the human.
    Stall,
    /// The worker went idle mid-task: send it a continue instruction
    /// (once per turn — a second idle produces Stall instead).
    NudgeWorker,
}

/// The referee. Pure: (state, event) → (state', action). `None` means
/// the event was out of turn (duplicate Stop, late gate) — ignore it.
pub fn step(state: &DuetState, event: Event) -> (DuetState, Option<Action>) {
    let mut next = state.clone();
    let action = match (state.awaiting, event) {
        (Awaiting::Worker, Event::WorkerDone) => {
            next.awaiting = Awaiting::Reviewer;
            next.nudged = false;
            Action::PingReviewer
        }
        (Awaiting::Worker, Event::WorkerIdle) => {
            if state.nudged {
                // Nudged once already and it went idle again — a human
                // is needed; `krill resume` is the recovery path.
                next.awaiting = Awaiting::Stalled;
                Action::Stall
            } else {
                next.nudged = true;
                Action::NudgeWorker
            }
        }
        (Awaiting::Reviewer, Event::ReviewerDone(v)) => match v {
            Verdict::Lgtm if state.gate.is_empty() => {
                next.awaiting = Awaiting::Done;
                Action::Complete
            }
            Verdict::Lgtm => {
                next.awaiting = Awaiting::Gate;
                Action::RunGate
            }
            Verdict::Issues if state.round < state.max_rounds => {
                next.round += 1;
                next.awaiting = Awaiting::Worker;
                next.nudged = false;
                Action::PingWorkerReview
            }
            Verdict::Missing if state.round < state.max_rounds => {
                // Consume a round so a protocol-blind reviewer can't
                // loop forever; awaiting stays with the reviewer.
                next.round += 1;
                Action::ReinstructReviewer
            }
            Verdict::Issues | Verdict::Missing => {
                next.awaiting = Awaiting::Stalled;
                Action::Stall
            }
        },
        (Awaiting::Gate, Event::GateFinished { pass: true }) => {
            next.awaiting = Awaiting::Done;
            Action::Complete
        }
        (Awaiting::Stalled, Event::Resume { new_max }) => {
            // The human's intervention grants a fresh set of rework
            // rounds; the cap changes only when they say so.
            next.round = 0;
            if let Some(mx) = new_max {
                next.max_rounds = mx;
            }
            next.awaiting = Awaiting::Worker;
            next.nudged = false;
            Action::PingWorkerResume
        }
        (Awaiting::Gate, Event::GateFinished { pass: false }) => {
            if state.round < state.max_rounds {
                next.round += 1;
                next.awaiting = Awaiting::Worker;
                next.nudged = false;
                Action::PingWorkerGate
            } else {
                next.awaiting = Awaiting::Stalled;
                Action::Stall
            }
        }
        _ => return (next, None), // out of turn — ignore
    };
    (next, Some(action))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(round: u32, max: u32, gate: &str, awaiting: Awaiting) -> DuetState {
        DuetState {
            round,
            max_rounds: max,
            gate: gate.into(),
            goal: "the goal".into(),
            awaiting,
            nudged: false,
        }
    }

    #[test]
    fn idle_worker_is_nudged_once_then_stalls() {
        let (next, a) = step(&st(0, 2, "", Awaiting::Worker), Event::WorkerIdle);
        assert_eq!(a, Some(Action::NudgeWorker));
        assert!(next.nudged);
        assert_eq!(next.awaiting, Awaiting::Worker);
        // A second idle after the nudge calls the human.
        let (next2, a2) = step(&next, Event::WorkerIdle);
        assert_eq!(a2, Some(Action::Stall));
        assert_eq!(next2.awaiting, Awaiting::Stalled);
    }

    #[test]
    fn idle_is_out_of_turn_unless_awaiting_worker() {
        for aw in [Awaiting::Reviewer, Awaiting::Gate, Awaiting::Done, Awaiting::Stalled] {
            let s = st(0, 2, "", aw);
            assert_eq!(step(&s, Event::WorkerIdle), (s.clone(), None));
        }
    }

    #[test]
    fn nudged_resets_when_a_fresh_worker_turn_starts() {
        let mut s = st(0, 2, "", Awaiting::Worker);
        s.nudged = true;
        // Completing the turn clears it…
        let (next, _) = step(&s, Event::WorkerDone);
        assert!(!next.nudged);
        // …as do a rework round and a human resume.
        let mut rev = st(0, 2, "", Awaiting::Reviewer);
        rev.nudged = true;
        let (next, _) = step(&rev, Event::ReviewerDone(Verdict::Issues));
        assert!(!next.nudged);
        let mut stalled = st(2, 2, "", Awaiting::Stalled);
        stalled.nudged = true;
        let (next, _) = step(&stalled, Event::Resume { new_max: None });
        assert!(!next.nudged);
    }

    #[test]
    fn notification_classification_never_types_into_dialogs() {
        use NotificationKind::*;
        assert_eq!(classify_notification(r#"{"message":"Claude is waiting for your input"}"#), Idle);
        assert_eq!(
            classify_notification(r#"{"message":"Claude needs your permission to use Bash"}"#),
            Permission
        );
        // Both markers present → Permission wins (never type).
        assert_eq!(
            classify_notification("waiting for your input… needs permission"),
            Permission
        );
        assert_eq!(classify_notification(""), Unknown);
        assert_eq!(classify_notification("something else entirely"), Unknown);
    }

    #[test]
    fn review_excerpt_gives_first_finding_and_count() {
        let review = "ISSUES\n\n- first finding here\n  detail line\n- second finding\n";
        assert_eq!(review_excerpt(review, 100), Some(("first finding here".into(), 2)));
        // No bullets — first non-empty body line, count 1.
        assert_eq!(review_excerpt("ISSUES\nprose only\n", 100), Some(("prose only".into(), 1)));
        // Verdict-only file has no material.
        assert_eq!(review_excerpt("LGTM\n", 100), None);
        // Long findings are cut on a char boundary with an ellipsis.
        let long = format!("ISSUES\n- {}\n", "가".repeat(50));
        let (cut, _) = review_excerpt(&long, 10).unwrap();
        assert_eq!(cut.chars().count(), 10);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn verdict_first_line_rules() {
        assert_eq!(parse_verdict(Some("LGTM\nnice work")), Verdict::Lgtm);
        assert_eq!(parse_verdict(Some("  lgtm  ")), Verdict::Lgtm);
        assert_eq!(parse_verdict(Some("ISSUES\n- bug in x")), Verdict::Issues);
        assert_eq!(parse_verdict(Some("looks broken")), Verdict::Issues);
        assert_eq!(parse_verdict(Some("")), Verdict::Missing);
        assert_eq!(parse_verdict(None), Verdict::Missing);
    }

    #[test]
    fn worker_done_hands_the_turn_to_the_reviewer() {
        let (next, a) = step(&st(0, 2, "", Awaiting::Worker), Event::WorkerDone);
        assert_eq!(a, Some(Action::PingReviewer));
        assert_eq!(next.awaiting, Awaiting::Reviewer);
        assert_eq!(next.round, 0);
    }

    #[test]
    fn out_of_turn_events_are_ignored() {
        // Duplicate Stop from the worker while the reviewer is at bat.
        let s = st(0, 2, "", Awaiting::Reviewer);
        assert_eq!(step(&s, Event::WorkerDone), (s.clone(), None));
        // Reviewer chatter while the worker is at bat.
        let s = st(1, 2, "", Awaiting::Worker);
        assert_eq!(step(&s, Event::ReviewerDone(Verdict::Lgtm)), (s.clone(), None));
        // Late gate result after completion.
        let s = st(0, 2, "make test", Awaiting::Done);
        assert_eq!(step(&s, Event::GateFinished { pass: true }), (s.clone(), None));
        // Nothing moves once stalled — the human owns it now.
        let s = st(2, 2, "", Awaiting::Stalled);
        assert_eq!(step(&s, Event::WorkerDone), (s.clone(), None));
    }

    #[test]
    fn lgtm_completes_directly_without_a_gate() {
        let (next, a) = step(&st(1, 2, "", Awaiting::Reviewer), Event::ReviewerDone(Verdict::Lgtm));
        assert_eq!(a, Some(Action::Complete));
        assert_eq!(next.awaiting, Awaiting::Done);
    }

    #[test]
    fn lgtm_with_a_gate_runs_it_and_gate_decides() {
        let (next, a) = step(&st(0, 2, "cargo test", Awaiting::Reviewer), Event::ReviewerDone(Verdict::Lgtm));
        assert_eq!(a, Some(Action::RunGate));
        assert_eq!(next.awaiting, Awaiting::Gate);

        let (done, a) = step(&next, Event::GateFinished { pass: true });
        assert_eq!(a, Some(Action::Complete));
        assert_eq!(done.awaiting, Awaiting::Done);

        let (back, a) = step(&next, Event::GateFinished { pass: false });
        assert_eq!(a, Some(Action::PingWorkerGate));
        assert_eq!(back.awaiting, Awaiting::Worker);
        assert_eq!(back.round, 1); // a failed gate consumes a round
    }

    #[test]
    fn issues_consume_rounds_until_the_cap_stalls() {
        let (r1, a) = step(&st(0, 2, "", Awaiting::Reviewer), Event::ReviewerDone(Verdict::Issues));
        assert_eq!(a, Some(Action::PingWorkerReview));
        assert_eq!((r1.round, r1.awaiting), (1, Awaiting::Worker));

        let (r2, _) = step(&r1, Event::WorkerDone);
        let (r3, a) = step(&r2, Event::ReviewerDone(Verdict::Issues));
        assert_eq!(a, Some(Action::PingWorkerReview));
        assert_eq!(r3.round, 2);

        let (r4, _) = step(&r3, Event::WorkerDone);
        let (stalled, a) = step(&r4, Event::ReviewerDone(Verdict::Issues));
        assert_eq!(a, Some(Action::Stall));
        assert_eq!(stalled.awaiting, Awaiting::Stalled);
    }

    #[test]
    fn missing_review_reinstrucs_but_still_burns_a_round() {
        let (next, a) = step(&st(0, 1, "", Awaiting::Reviewer), Event::ReviewerDone(Verdict::Missing));
        assert_eq!(a, Some(Action::ReinstructReviewer));
        assert_eq!((next.round, next.awaiting), (1, Awaiting::Reviewer));
        // Cap reached: the next miss stalls instead of looping forever.
        let (stalled, a) = step(&next, Event::ReviewerDone(Verdict::Missing));
        assert_eq!(a, Some(Action::Stall));
        assert_eq!(stalled.awaiting, Awaiting::Stalled);
    }

    #[test]
    fn resume_reawakens_only_a_stalled_duet() {
        // Stalled + Resume → worker's turn, rounds reset, cap kept.
        let (next, a) = step(&st(2, 2, "", Awaiting::Stalled), Event::Resume { new_max: None });
        assert_eq!(a, Some(Action::PingWorkerResume));
        assert_eq!((next.round, next.max_rounds, next.awaiting), (0, 2, Awaiting::Worker));
        // An explicit --rounds raises the cap.
        let (next, _) = step(&st(2, 2, "", Awaiting::Stalled), Event::Resume { new_max: Some(5) });
        assert_eq!(next.max_rounds, 5);
        // Anywhere else, Resume is out of turn.
        for aw in [Awaiting::Worker, Awaiting::Reviewer, Awaiting::Gate, Awaiting::Done] {
            let s = st(1, 2, "", aw);
            assert_eq!(step(&s, Event::Resume { new_max: None }), (s.clone(), None));
        }
    }

    #[test]
    fn gate_failure_at_the_cap_stalls() {
        let (next, a) = step(&st(2, 2, "make test", Awaiting::Gate), Event::GateFinished { pass: false });
        assert_eq!(a, Some(Action::Stall));
        assert_eq!(next.awaiting, Awaiting::Stalled);
    }

    #[test]
    fn state_kv_roundtrip() {
        let s = st(1, 3, "cargo test && cargo clippy", Awaiting::Gate);
        let back = DuetState::from_map(&s.to_map()).unwrap();
        assert_eq!(back, s);

        let mut bad = s.to_map();
        bad.insert("round".into(), "NaN".into());
        assert!(DuetState::from_map(&bad).is_err());
        let mut noawait = s.to_map();
        noawait.remove("awaiting");
        assert!(DuetState::from_map(&noawait).is_err());
    }

    #[test]
    fn role_and_awaiting_parse_roundtrip() {
        for r in [DuetRole::Worker, DuetRole::Reviewer] {
            assert_eq!(DuetRole::parse(r.as_str()), Some(r));
        }
        assert_eq!(DuetRole::parse("referee"), None);
        for a in [Awaiting::Worker, Awaiting::Reviewer, Awaiting::Gate, Awaiting::Done, Awaiting::Stalled] {
            assert_eq!(Awaiting::parse(a.as_str()), Some(a));
        }
        assert_eq!(Awaiting::parse(""), None);
    }
}
