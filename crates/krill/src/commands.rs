use crate::msg as m;
use krill_core::bail;
use krill_core::config::Config;
use krill_core::error::{Context, Result};
use krill_core::git;
use krill_core::duet::{self, Action, Awaiting, DuetRef, DuetRole, DuetState, Event};
use krill_core::plan::{self, PlanPhase, PlanState};
use krill_core::session::{self, FlowNext, FlowRef, SessionMeta, Status};
use krill_core::tmux;
use std::io::Write as _;
use std::process::Command;

const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const MAGENTA: &str = "\x1b[35m";
const BLUE: &str = "\x1b[34m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

/// tmux dislikes '.' and ':' in session names.
fn tmux_safe(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect()
}

pub fn init() -> Result<()> {
    let (path, created) = Config::init_file()?;
    std::fs::create_dir_all(krill_core::sessions_dir()?)?;
    std::fs::create_dir_all(krill_core::logs_dir()?)?;
    std::fs::create_dir_all(krill_core::worktrees_dir()?)?;
    if created {
        println!("{}", m::init_created(&path.display().to_string()));
        println!("{}", m::init_hint());
    } else {
        println!("{}", m::init_exists(&path.display().to_string()));
    }
    Ok(())
}

pub fn new(
    name: &str,
    agent: Option<&str>,
    repo: Option<&str>,
    message: Option<&str>,
    from: Option<&str>,
    flow: Option<&str>,
) -> Result<()> {
    let meta = match flow {
        Some(fl) => {
            if from.is_some() || agent.is_some() {
                bail!(m::flow_flag_conflict());
            }
            start_flow(name, fl, repo, message.unwrap_or(""))?
        }
        None => create_session(name, agent, repo, message, from)?,
    };
    println!("{BOLD}{}{RESET} {}", meta.name, m::session_started());
    println!("  repo     {} ({})", meta.repo_name, meta.repo_path.display());
    println!("  branch   {} (base: {})", meta.branch, meta.base);
    println!("  worktree {}", meta.worktree.display());
    println!(
        "  agent    {}{}",
        meta.agent,
        if meta.cmd.is_empty() { m::shell_only() } else { String::new() }
    );
    if let Some(f) = &meta.flow {
        println!("  flow     {} #{}", f.flow, f.stage);
    }
    println!();
    println!("{}", m::attach_hint(&format!("{BOLD}krill attach {}{RESET}", meta.name)));
    Ok(())
}

/// Kick off stage 1 of a `[flows.*]` chain: session `<name>-1`, with the
/// Done hook advancing the rest (design §12.1).
fn start_flow(name: &str, flow: &str, repo: Option<&str>, goal: &str) -> Result<SessionMeta> {
    let config = Config::load()?;
    let Some(stages) = config.flows.get(flow) else {
        bail!(m::flow_unknown(flow, &flow_names(&config)));
    };
    // A hookless agent can't fire the Done that advances the chain — warn
    // up front for every stage but the last.
    for (i, stage) in stages.iter().enumerate().take(stages.len().saturating_sub(1)) {
        if let Ok((agent_name, cfg)) = config.resolve_agent(stage.agent.as_deref()) {
            if cfg.hooks.is_none() {
                eprintln!("{}", m::flow_agent_no_hooks(&agent_name, i + 1));
            }
        }
    }
    let stage = &stages[0];
    let prompt = krill_core::config::stage_prompt(stage.m.as_deref(), goal);
    let fr = FlowRef { flow: flow.into(), stage: 1, base: name.into(), goal: goal.into() };
    create_session_full(
        &format!("{name}-1"),
        stage.agent.as_deref(),
        repo,
        prompt.as_deref(),
        None,
        None,
        Some(fr),
    )
}

fn flow_names(config: &Config) -> String {
    if config.flows.is_empty() {
        m::flow_none_registered()
    } else {
        config.flows.keys().cloned().collect::<Vec<_>>().join(", ")
    }
}

/// Gate priority: CLI flag > repo gate > [duet] gate > none.
fn resolve_gate(cli: Option<&str>, repo_name: &str, config: &Config) -> String {
    cli.map(str::to_string)
        .or_else(|| config.repos.get(repo_name).and_then(|r| r.gate.clone()))
        .or_else(|| config.duet.gate.clone())
        .unwrap_or_default()
}

/// `krill plan <name> -m "goal"` (M5c, §12.1 decision 6) — spawn a
/// planner session that writes a plan.md checklist. Its Done hook flips
/// the plan to Ready (needs-you); `krill approve` starts the task walk.
pub fn plan(
    name: &str,
    agent: Option<&str>,
    reviewer: Option<&str>,
    repo: Option<&str>,
    message: Option<&str>,
    gate: Option<&str>,
    max_rounds: Option<&str>,
) -> Result<()> {
    let Some(goal) = message else {
        bail!(m::plan_goal_required());
    };
    let config = Config::load()?;
    let max_rounds: u32 = match max_rounds {
        Some(v) => match v.parse().ok().filter(|n: &u32| *n >= 1) {
            Some(n) => n,
            None => bail!(m::duet_bad_rounds(v)),
        },
        None => config.duet.max_rounds.unwrap_or(2),
    };
    let reviewer = reviewer
        .map(str::to_string)
        .or_else(|| config.duet.reviewer.clone());
    for side in [agent, reviewer.as_deref()] {
        if let Ok((agent_name, cfg)) = config.resolve_agent(side) {
            if cfg.hooks.is_none() {
                eprintln!("{}", m::duet_no_hooks_warn(&agent_name));
            }
        }
    }

    let planner = create_session_full(
        name,
        agent,
        repo,
        Some(&m::plan_prompt(goal)),
        None,
        None,
        None,
    )?;
    let gate = resolve_gate(gate, &planner.repo_name, &config);
    if let Err(e) =
        krill_core::plan::PlanState::new(goal, reviewer, &gate, max_rounds).save(&planner.id())
    {
        let _ = remove_session(&planner, true);
        return Err(e);
    }

    println!("{BOLD}{}{RESET} {}", planner.name, m::plan_started());
    println!("  repo     {} ({})", planner.repo_name, planner.repo_path.display());
    println!("  branch   {} (base: {})", planner.branch, planner.base);
    println!("  planner  {}", planner.agent);
    println!(
        "  gate     {}  ·  max rounds {max_rounds}",
        if gate.is_empty() { m::duet_no_gate() } else { gate.clone() }
    );
    println!();
    println!("{}", m::plan_hint(&format!("{BOLD}krill approve {}{RESET}", planner.name)));
    Ok(())
}

/// `krill approve <name>` — the human sign-off on plan.md. Attaches the
/// reviewer, turns the planner into the duet worker (context accrues in
/// one session), and sends the first task.
pub fn approve(name: &str, repo: Option<&str>) -> Result<()> {
    let mut worker = session::find(name, repo)?;
    let id = worker.id();
    let mut ps = match krill_core::plan::PlanState::load(&id) {
        Ok(ps) => ps,
        Err(_) => bail!(m::plan_not_a_plan(name)),
    };
    if ps.phase != krill_core::plan::PlanPhase::Ready {
        bail!(m::plan_wrong_phase(name, ps.phase.as_str()));
    }
    let plan_md = std::fs::read_to_string(worker.worktree.join("plan.md"))
        .context(m::plan_md_missing(name))?;
    let Some(first) = krill_core::plan::first_open_task(&plan_md) else {
        bail!(m::plan_no_tasks(name));
    };
    let (done, total) = krill_core::plan::progress(&plan_md);

    let config = Config::load()?;
    let rev_meta = spawn_reviewer_for(&worker, ps.reviewer.as_deref(), &config)?;
    worker.duet = Some(DuetRef { role: DuetRole::Worker, peer: rev_meta.name.clone() });
    worker.save()?;

    // Fresh duet round per task; the task text is the review goal.
    DuetState::new(ps.max_rounds, &ps.gate, &first).save(&id)?;
    ps.phase = krill_core::plan::PlanPhase::Running;
    ps.save(&id)?;
    tmux::send_line(&worker.tmux, &m::plan_task_instruction(&first))?;

    println!("{}", m::plan_approved(total - done, &rev_meta.agent));
    println!("  → {first}");
    Ok(())
}
/// worker's worktree — no branch or worktree of its own (§12.1). The
/// reviewer launches bare; instructions arrive per round via send-keys.
fn spawn_reviewer_for(
    worker: &SessionMeta,
    reviewer: Option<&str>,
    config: &Config,
) -> Result<SessionMeta> {
    let rev_name = format!("{}-rev", worker.name);
    let (rev_agent, rev_cfg) = config.resolve_agent(reviewer)?;
    if session::load_all()?
        .iter()
        .any(|s| s.name == rev_name && s.repo_name == worker.repo_name)
    {
        bail!(m::session_exists(&rev_name, &worker.repo_name));
    }
    let rev_id = format!("{}--{}", worker.repo_name, rev_name);
    let cmd = rev_cfg.cmd.replace("{prompt}", "").trim().to_string();
    let cmd = if !cmd.is_empty() {
        format!("KRILL_SESSION_ID={} {cmd}", krill_core::shell_quote(&rev_id))
    } else {
        cmd
    };
    let tmux_name = tmux_safe(&format!("krill_{}_{}", worker.repo_name, rev_name));
    let meta = SessionMeta {
        name: rev_name,
        repo_name: worker.repo_name.clone(),
        repo_path: worker.repo_path.clone(),
        base: worker.base.clone(),
        branch: worker.branch.clone(),
        worktree: worker.worktree.clone(), // shared — see §12.1
        agent: rev_agent,
        cmd: cmd.clone(),
        tmux: tmux_name.clone(),
        created_unix: krill_core::now_unix(),
        flow: None,
        duet: Some(DuetRef { role: DuetRole::Reviewer, peer: worker.name.clone() }),
    };
    if tmux::has(&tmux_name) {
        bail!(m::tmux_name_taken(&tmux_name));
    }
    tmux::new_session(&tmux_name, &meta.worktree)?;
    let spawn = || -> Result<()> {
        let log = meta.log_path()?;
        if let Some(dir) = log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        tmux::pipe_to_log(&tmux_name, &log)?;
        if !cmd.is_empty() {
            tmux::send_line(&tmux_name, &cmd)?;
        }
        Ok(())
    };
    if let Err(e) = spawn() {
        let _ = tmux::kill(&tmux_name);
        return Err(e);
    }
    meta.save()?;
    Ok(meta)
}

/// `krill duet <name> -m "task"` — turn-based worker/reviewer ping-pong
/// over one shared worktree (design §12.1 decision 4). The worker is a
/// normal session; the reviewer is a second tmux session in the same
/// worktree, and `krill hook done` referees the turns.
pub fn duet(
    name: &str,
    agent: Option<&str>,
    reviewer: Option<&str>,
    repo: Option<&str>,
    message: Option<&str>,
    gate: Option<&str>,
    max_rounds: Option<&str>,
) -> Result<()> {
    let Some(goal) = message else {
        bail!(m::duet_goal_required());
    };
    let config = Config::load()?;
    let max_rounds: u32 = match max_rounds {
        Some(v) => match v.parse().ok().filter(|n: &u32| *n >= 1) {
            Some(n) => n,
            None => bail!(m::duet_bad_rounds(v)),
        },
        None => config.duet.max_rounds.unwrap_or(2),
    };
    let reviewer = reviewer
        .map(str::to_string)
        .or_else(|| config.duet.reviewer.clone());

    // Hookless agents never fire the Done that drives the referee.
    for side in [agent, reviewer.as_deref()] {
        if let Ok((agent_name, cfg)) = config.resolve_agent(side) {
            if cfg.hooks.is_none() {
                eprintln!("{}", m::duet_no_hooks_warn(&agent_name));
            }
        }
    }

    // Worker first (normal session — worktree, branch, goal prompt).
    let mut worker = create_session_full(name, agent, repo, Some(goal), None, None, None)?;

    let rev_meta = match spawn_reviewer_for(&worker, reviewer.as_deref(), &config) {
        Ok(m) => m,
        Err(e) => {
            // No half-made duets: the worker goes too.
            let _ = remove_session(&worker, true);
            return Err(e);
        }
    };

    worker.duet = Some(DuetRef { role: DuetRole::Worker, peer: rev_meta.name.clone() });
    worker.save()?;

    let gate = resolve_gate(gate, &worker.repo_name, &config);
    DuetState::new(max_rounds, &gate, goal).save(&worker.id())?;

    println!("{BOLD}{}{RESET} {}", worker.name, m::duet_started());
    println!("  repo     {} ({})", worker.repo_name, worker.repo_path.display());
    println!("  branch   {} (base: {})", worker.branch, worker.base);
    println!("  worktree {}", worker.worktree.display());
    println!("  worker   {}  ·  reviewer {} ({})", worker.agent, rev_meta.agent, rev_meta.name);
    println!(
        "  gate     {}  ·  max rounds {max_rounds}",
        if gate.is_empty() { m::duet_no_gate() } else { gate.clone() }
    );
    println!();
    println!("{}", m::attach_hint(&format!("{BOLD}krill attach {}{RESET}", worker.name)));
    Ok(())
}

/// Branch + worktree + tmux session + agent, returning the saved meta.
/// No terminal output — the CLI (`new`) and the TUI both wrap this.
pub fn create_session(
    name: &str,
    agent: Option<&str>,
    repo: Option<&str>,
    message: Option<&str>,
    from: Option<&str>,
) -> Result<SessionMeta> {
    create_session_full(name, agent, repo, message, from, None, None)
}

/// `at` overrides the cwd used for repo resolution — the hook chain runs
/// with cwd inside a worktree, which must not be mistaken for the repo.
fn create_session_full(
    name: &str,
    agent: Option<&str>,
    repo: Option<&str>,
    message: Option<&str>,
    from: Option<&str>,
    at: Option<&std::path::Path>,
    flow: Option<FlowRef>,
) -> Result<SessionMeta> {
    if !krill_core::valid_name(name) {
        bail!(m::invalid_session_name(name));
    }
    let config = Config::load()?;
    let cwd = match at {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    let repo_ref = git::resolve_repo(&config, repo, &cwd)?;
    let (agent_name, agent_cfg) = config.resolve_agent(agent)?;

    if session::load_all()?
        .iter()
        .any(|m| m.name == name && m.repo_name == repo_ref.name)
    {
        bail!(m::session_exists(name, &repo_ref.name));
    }

    // Relay handoff (--from): branch off another session's work instead of
    // base. Resolve within this repo first so a same-named session in
    // another repo can't shadow it; fall back for the friendlier error.
    let base = match from {
        Some(f) => match session::find(f, Some(&repo_ref.name)) {
            Ok(src) => src.branch,
            Err(e) => match session::find(f, None) {
                Ok(other) => bail!(m::from_other_repo(&other.repo_name)),
                Err(_) => return Err(e),
            },
        },
        None => repo_ref.base.clone(),
    };

    let branch = format!("krill/{name}");
    let worktree = krill_core::worktrees_dir()?.join(&repo_ref.name).join(name);
    if worktree.exists() {
        bail!(m::worktree_exists(&worktree.display().to_string()));
    }
    if let Some(parent) = worktree.parent() {
        std::fs::create_dir_all(parent)?;
    }

    git::worktree_add(&repo_ref.path, &worktree, &branch, &base)
        .with_context(|| m::worktree_create_failed(&base))?;

    // Hook preset (M3): agents that support hooks get NeedsYou/Done
    // reporting injected into their worktree before they start.
    let session_id = format!("{}--{}", repo_ref.name, name);
    if agent_cfg.hooks.as_deref() == Some("claude-code") {
        if let Err(e) = inject_claude_hooks(&worktree, &session_id) {
            // Roll back like a failed spawn — no half-made sessions.
            let _ = git::worktree_remove(&repo_ref.path, &worktree, true);
            let _ = git::branch_delete(&repo_ref.path, &branch, true);
            return Err(e);
        }
    }

    // Build the agent command line.
    let cmd = match message {
        Some(msg) => {
            let quoted = krill_core::shell_quote(msg);
            if agent_cfg.cmd.contains("{prompt}") {
                agent_cfg.cmd.replace("{prompt}", &quoted)
            } else if agent_cfg.cmd.is_empty() {
                String::new()
            } else {
                format!("{} {}", agent_cfg.cmd, quoted)
            }
        }
        None => agent_cfg.cmd.replace("{prompt}", "").trim().to_string(),
    };
    // Every agent carries its session id in the environment: hooked agents'
    // settings.local.json reads it (§12.1), and hookless agents can bridge
    // their own notify mechanisms into `krill hook` with it.
    let cmd = if !cmd.is_empty() {
        format!("KRILL_SESSION_ID={} {cmd}", krill_core::shell_quote(&session_id))
    } else {
        cmd
    };

    let tmux_name = tmux_safe(&format!("krill_{}_{}", repo_ref.name, name));
    let meta = SessionMeta {
        name: name.to_string(),
        repo_name: repo_ref.name.clone(),
        repo_path: repo_ref.path.clone(),
        base: base.clone(),
        branch: branch.clone(),
        worktree: worktree.clone(),
        agent: agent_name.clone(),
        cmd: cmd.clone(),
        tmux: tmux_name.clone(),
        created_unix: krill_core::now_unix(),
        flow,
        duet: None,
    };

    let spawn = || -> Result<()> {
        if tmux::has(&tmux_name) {
            bail!(m::tmux_name_taken(&tmux_name));
        }
        tmux::new_session(&tmux_name, &worktree)?;
        let log = meta.log_path()?;
        if let Some(dir) = log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        tmux::pipe_to_log(&tmux_name, &log)?;
        if !cmd.is_empty() {
            tmux::send_line(&tmux_name, &cmd)?;
        }
        Ok(())
    };

    if let Err(e) = spawn() {
        // Roll back so a failed spawn leaves nothing behind.
        if tmux::has(&tmux_name) {
            let _ = tmux::kill(&tmux_name);
        }
        let _ = git::worktree_remove(&repo_ref.path, &worktree, true);
        let _ = git::branch_delete(&repo_ref.path, &branch, true);
        return Err(e);
    }
    meta.save()?;
    Ok(meta)
}

pub fn ls() -> Result<()> {
    let metas = session::load_all()?;
    if metas.is_empty() {
        println!("{}", m::ls_empty());
        println!("{}", m::ls_hint());
        return Ok(());
    }
    let live = tmux::server_sessions();

    struct Row {
        dot: String,
        name: String,
        repo: String,
        agent: String,
        flow: String,
        state: String,
        last: String,
        diff: String,
    }

    let mut rows = Vec::new();
    for m in &metas {
        let (h, age) = session::status(m, &live);
        let (dot, state) = match h {
            Status::NeedsYou => (format!("{MAGENTA}◆{RESET}"), "needs-you".to_string()),
            Status::Active => (format!("{GREEN}●{RESET}"), "active".to_string()),
            Status::Quiet => (format!("{YELLOW}●{RESET}"), "quiet".to_string()),
            Status::Done => (format!("{BLUE}✓{RESET}"), "done".to_string()),
            Status::Dead => (format!("{RED}✖{RESET}"), "dead".to_string()),
        };
        let attached = h != Status::Dead && tmux::attached_count(&m.tmux) > 0;
        let last = age.map(krill_core::fmt_age).unwrap_or_else(|| "-".into());
        let diff = if h == Status::Dead {
            "-".to_string()
        } else {
            git::shortstat(&m.worktree, &m.base)
        };
        rows.push(Row {
            dot,
            name: m.name.clone(),
            repo: m.repo_name.clone(),
            agent: m.agent.clone(),
            flow: m
                .flow
                .as_ref()
                .map(|f| format!("{}:{}", f.flow, f.stage))
                .or_else(|| {
                    // A plan leader shows its phase; its reviewer (and
                    // plain duets) show the duet role.
                    PlanState::load(&m.id())
                        .ok()
                        .map(|p| format!("plan:{}", p.phase.as_str()))
                })
                .or_else(|| m.duet.as_ref().map(|d| format!("duet:{}", d.role.as_str())))
                .unwrap_or_else(|| "-".into()),
            state: if attached { format!("{state}+⌨") } else { state },
            last,
            diff,
        });
    }

    let w = |f: fn(&Row) -> usize, min: usize| {
        rows.iter().map(f).max().unwrap_or(0).max(min)
    };
    let nw = w(|r| r.name.len(), 4);
    let rw = w(|r| r.repo.len(), 4);
    let aw = w(|r| r.agent.len(), 5);
    let sw = w(|r| r.state.len(), 5);
    let lw = w(|r| r.last.len(), 4);

    // The FLOW column only appears once a flow session exists.
    let show_flow = rows.iter().any(|r| r.flow != "-");
    let fw = w(|r| r.flow.len(), 4);
    let flow_cell = |s: &str| if show_flow { format!("{s:<fw$}  ") } else { String::new() };

    println!(
        "{DIM}  {:<nw$}  {:<rw$}  {:<aw$}  {}{:<sw$}  {:<lw$}  {}{RESET}",
        "NAME", "REPO", "AGENT", flow_cell("FLOW"), "STATE", "LAST", "DIFF"
    );
    for r in rows {
        println!(
            "{} {:<nw$}  {:<rw$}  {:<aw$}  {}{:<sw$}  {:<lw$}  {}",
            r.dot, r.name, r.repo, r.agent, flow_cell(&r.flow), r.state, r.last, r.diff
        );
    }
    Ok(())
}

pub fn attach(name: &str, repo: Option<&str>) -> Result<()> {
    let meta = session::find(name, repo)?;
    if !tmux::has(&meta.tmux) {
        bail!(m::attach_dead(&meta.name));
    }
    tmux::attach_exec(&meta.tmux)
}

pub fn diff(name: &str, repo: Option<&str>, stat: bool) -> Result<()> {
    let meta = session::find(name, repo)?;
    if !meta.worktree.exists() {
        bail!(m::worktree_missing(&meta.worktree.display().to_string()));
    }
    diff_worktree(&meta.worktree, &meta.base, stat, false)
}

/// Run `git diff` against base with inherited stdio (pager and colors
/// intact). `hold_pager` pins LESS=R so the pager waits for `q` even on
/// one-screen diffs — without it git's default LESS=FRX exits instantly
/// and the resuming TUI would repaint over the diff before it can be
/// read. The CLI passes false (print-and-exit is right there); the TUI
/// passes true.
pub fn diff_worktree(worktree: &std::path::Path, base: &str, stat: bool, hold_pager: bool) -> Result<()> {
    // Diff the working tree against base — includes uncommitted changes,
    // which agents frequently leave behind.
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(worktree).arg("diff");
    if stat {
        cmd.arg("--stat");
    }
    cmd.arg(base);
    if hold_pager {
        cmd.env("LESS", "R");
    }
    let status = cmd.status().context(m::git_exec_failed())?;
    if !status.success() {
        bail!(m::git_diff_exit(&status.to_string()));
    }
    Ok(())
}

/// First line of a command's stdout ("" if none).
fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

/// `--bind tailscale`: ask the tailscale CLI for this machine's tailnet
/// IPv4 (design §7 — bind only the tailnet, expose nothing public).
fn tailscale_ip() -> Result<String> {
    let out = Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .context(m::tailscale_failed())?;
    let ip = first_line(&String::from_utf8_lossy(&out.stdout));
    if !out.status.success() || ip.is_empty() {
        bail!(m::tailscale_failed());
    }
    Ok(ip)
}

/// `krill serve` — flags override config `[serve]`; the token comes
/// from the config only (keeps it out of shell history).
pub fn serve(bind: Option<&str>, port: Option<&str>) -> Result<()> {
    let cfg = Config::load()?.serve;
    let bind = bind.unwrap_or(&cfg.bind).to_string();
    let bind = if bind == "tailscale" { tailscale_ip()? } else { bind };
    let port: u16 = match port {
        Some(p) => p.parse().ok().ok_or_else(|| {
            krill_core::error::Error::msg(m::serve_bad_port(p))
        })?,
        None => cfg.port,
    };
    crate::serve::run(&bind, port, cfg.token)
}

/// `krill merge <name> [--squash]` — merge the session branch into base
/// (which must be checked out in the repo) and clean the session up.
pub fn merge(name: &str, repo: Option<&str>, squash: bool) -> Result<()> {
    let meta = session::find(name, repo)?;

    // A duet is merged through its worker — the reviewer shares the
    // same branch, but its removal semantics would only clean half.
    if let Some(d) = &meta.duet {
        if d.role == DuetRole::Reviewer {
            bail!(m::merge_on_reviewer(&d.peer));
        }
    }

    // Uncommitted work can't be merged — make the user commit first.
    // krill's own injected .claude/ hook settings don't count as work.
    if meta.worktree.exists()
        && is_dirty(
            &git::run(&meta.worktree, &["status", "--porcelain"])?,
            meta.duet.is_some(),
        )
    {
        bail!(m::merge_dirty(&meta.name));
    }
    // Stubborn-simple (§7 spirit): merge only when base is checked out,
    // instead of silently switching the user's branches around.
    let current = git::run(&meta.repo_path, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if current != meta.base {
        bail!(m::merge_not_on_base(&meta.base));
    }

    if squash {
        git::run(&meta.repo_path, &["merge", "--squash", &meta.branch])?;
        println!("{}", m::merge_squashed(&meta.name, &meta.base));
        // Branch survives --squash (nothing points at the commits yet);
        // leave the session too, so the diff stays inspectable until
        // the user commits and runs rm.
        return Ok(());
    }

    git::run(&meta.repo_path, &["merge", "--no-edit", &meta.branch])?;
    println!("{}", m::merge_done(&meta.name, &meta.base));
    let warning = remove_session(&meta, false)?;
    if let Some(w) = warning {
        eprintln!("{DIM}{w}{RESET}");
    }
    println!("{}", m::rm_done(&meta.name));
    Ok(())
}

/// Any porcelain entry that isn't krill's injected .claude/ settings —
/// nor, for duet sessions, the referee's REVIEW.md/GATE.md protocol
/// files (they're krill's noise, not the agent's work).
fn is_dirty(porcelain: &str, ignore_duet_files: bool) -> bool {
    porcelain.lines().any(|l| {
        if l.len() <= 3 {
            return false;
        }
        let path = l[3..].trim_start();
        if path.starts_with(".claude") {
            return false;
        }
        if ignore_duet_files && (path == "REVIEW.md" || path == "GATE.md") {
            return false;
        }
        true
    })
}

/// `krill pr <name>` — push the branch and delegate to `gh pr create`
/// (interactive, run inside the worktree). The session stays alive.
pub fn pr(name: &str, repo: Option<&str>) -> Result<()> {
    let meta = session::find(name, repo)?;
    if !meta.worktree.exists() {
        bail!(m::worktree_missing(&meta.worktree.display().to_string()));
    }
    git::run(&meta.worktree, &["push", "-u", "origin", &meta.branch])?;
    println!("{}", m::pr_pushed(&meta.branch));
    let status = Command::new("gh")
        .args(["pr", "create", "--head", &meta.branch])
        .current_dir(&meta.worktree)
        .status()
        .context(m::gh_failed())?;
    if !status.success() {
        bail!(m::gh_exit(&status.to_string()));
    }
    Ok(())
}

pub fn rm(name: &str, repo: Option<&str>, force: bool) -> Result<()> {
    let meta = session::find(name, repo)?;

    if !force {
        eprint!("{}", m::rm_confirm(&meta.name, &meta.branch));
        std::io::stderr().flush().ok();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).context(m::stdin_read_failed())?;
        if !matches!(line.trim(), "y" | "Y") {
            println!("{}", m::rm_cancelled());
            return Ok(());
        }
    }

    let warning = remove_session(&meta, force)?;
    if let Some(w) = warning {
        eprintln!("{DIM}{w}{RESET}");
        eprintln!("{DIM}{}{RESET}", m::rm_branch_hint(&meta.branch));
    }
    println!("{}", m::rm_done(&meta.name));
    Ok(())
}

/// Claude Code settings.local.json with command hooks that report
/// session state back to krill (design §6.1 — file-based, no server).
fn hook_settings_json(id: &str, exe: &str) -> String {
    // `${KRILL_SESSION_ID:-…}`: krill-launched agents carry their own id
    // in the environment (two duet sessions share one worktree and thus
    // one settings file — design §12.1); a manually launched agent falls
    // back to the literal id this file was injected with.
    let cmd = |state: &str| format!("{exe} hook {state} -i \"${{KRILL_SESSION_ID:-{id}}}\"");
    serde_json::json!({
        "hooks": {
            "Notification": [
                { "hooks": [{ "type": "command", "command": cmd("needs-you") }] }
            ],
            "Stop": [
                { "hooks": [{ "type": "command", "command": cmd("done") }] }
            ],
            "SessionEnd": [
                { "hooks": [{ "type": "command", "command": cmd("done") }] }
            ]
        }
    })
    .to_string()
}

/// Write the hook preset into the worktree. An existing
/// settings.local.json is left untouched (the repo owns it).
fn inject_claude_hooks(worktree: &std::path::Path, id: &str) -> Result<()> {
    let dir = worktree.join(".claude");
    let path = dir.join("settings.local.json");
    if path.exists() {
        eprintln!("{}", m::hook_settings_exists(&path.display().to_string()));
        return Ok(());
    }
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "krill".into());
    std::fs::create_dir_all(&dir)?;
    std::fs::write(&path, hook_settings_json(id, &exe))?;
    Ok(())
}

/// `krill hook <state> -i <id>` — called by agent hooks, writes the
/// state file that status() layers over the activity heuristic. The
/// hook payload on stdin is drained and discarded (M3a). With a
/// `[notify] ntfy_topic` configured it also fires a phone push (M3b) —
/// straight from the hook, so notifications need no server either.
pub fn hook(state: &str, id: &str) -> Result<()> {
    let Some(hs) = krill_core::session::HookState::parse(state) else {
        bail!(m::hook_usage());
    };
    // An empty id means the ${KRILL_SESSION_ID:-…} expansion had nothing
    // to offer (edge: hand-edited settings) — a silent no-op, never an
    // agent-facing failure.
    if id.is_empty() {
        return Ok(());
    }
    let mut sink = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut sink);
    session::write_hook_state(id, hs)?;

    let config = Config::load().ok();

    // Flow chain (M5a), plan phases (M5c), duet referee (M5b): Done may
    // advance a chain, flip a plan to Ready, or hand the duet turn over.
    // A running plan *is* a duet, so the plan layer only owns the
    // planning phase and falls through otherwise. The outcome replaces
    // the generic ntfy body.
    let note = match (&config, hs) {
        (Some(cfg), krill_core::session::HookState::Done) => advance_flow(cfg, id)
            .or_else(|| advance_plan(id))
            .or_else(|| advance_duet(id)),
        _ => None,
    };

    if let Some(topic) = config.and_then(|c| c.ntfy_topic) {
        let body = note.unwrap_or_else(|| match hs {
            krill_core::session::HookState::NeedsYou => m::ntfy_needs_you(id),
            krill_core::session::HookState::Done => m::ntfy_done(id),
        });
        ntfy_push(&topic, &body);
    }
    Ok(())
}

/// Best-effort push, delegated to curl (principle 1), fire-and-forget —
/// a hook must never block or fail the agent on network issues.
fn ntfy_push(topic: &str, body: &str) {
    let _ = Command::new("curl")
        .args(["-fsS", "-m", "5", "-H", "Title: krill", "-d", body, &ntfy_url(topic)])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Plan phase transitions (M5c). Owns only the planning phase: when the
/// planner's turn ends, plan.md with open tasks → Ready + needs-you;
/// missing plan.md → one re-instruction, then hand it to the human.
/// Ready/Running/Done fall through (`None`) — Running is duet territory.
fn advance_plan(id: &str) -> Option<String> {
    let mut ps = PlanState::load(id).ok()?;
    if ps.phase != PlanPhase::Planning {
        return None;
    }
    let meta = session::find_by_id(id).ok()?;
    let plan_md = std::fs::read_to_string(meta.worktree.join("plan.md")).ok();
    let has_tasks = plan_md
        .as_deref()
        .and_then(plan::first_open_task)
        .is_some();
    if has_tasks {
        ps.phase = PlanPhase::Ready;
        ps.save(id).ok()?;
        let _ = session::write_hook_state(id, krill_core::session::HookState::NeedsYou);
        let (_, total) = plan::progress(plan_md.as_deref().unwrap_or(""));
        Some(m::ntfy_plan_ready(&meta.name, total))
    } else if ps.retries < 1 {
        ps.retries += 1;
        ps.save(id).ok()?;
        let _ = tmux::send_line(&meta.tmux, &m::plan_replan_instruction());
        None
    } else {
        // The planner won't produce a plan — the human can write
        // plan.md by hand and approve it.
        ps.phase = PlanPhase::Ready;
        ps.save(id).ok()?;
        let _ = session::write_hook_state(id, krill_core::session::HookState::NeedsYou);
        Some(m::ntfy_plan_no_plan(&meta.name))
    }
}

/// What a duet Complete means: for a plain duet, done; for a running
/// plan, commit the task, tick its box, and send the next one.
fn duet_complete_note(worker: &SessionMeta) -> Option<String> {
    match PlanState::load(&worker.id()) {
        Ok(ps) if ps.phase == PlanPhase::Running => plan_next_task(worker, ps),
        _ => Some(m::ntfy_duet_done(&worker.name)),
    }
}

/// One task cleared the duet: check its box, commit box + work as one
/// commit (task = commit), then start the next task or finish the plan.
fn plan_next_task(worker: &SessionMeta, mut ps: PlanState) -> Option<String> {
    let plan_path = worker.worktree.join("plan.md");
    let md = std::fs::read_to_string(&plan_path).ok()?;
    // The finished task is the duet goal — never re-derived from plan.md's
    // first open box: if the worker edited plan.md mid-task (e.g. checked
    // its own box), the first open box is already the NEXT task, which
    // would get checked and committed without ever running.
    let finished = DuetState::load(&worker.id())
        .ok()
        .map(|d| d.goal)
        .or_else(|| plan::first_open_task(&md));
    if let Some(task) = &finished {
        let _ = std::fs::write(&plan_path, plan::check_task(&md, task));
    }
    // Commit the task's work. Protocol files are krill's, not work, and
    // the injected .claude/ settings never belong in history.
    let _ = std::fs::remove_file(worker.worktree.join("REVIEW.md"));
    let _ = std::fs::remove_file(worker.worktree.join("GATE.md"));
    let _ = git::run(&worker.worktree, &["add", "-A", "--", ".", ":(exclude).claude"]);
    if let Some(task) = &finished {
        let _ = git::run(&worker.worktree, &["commit", "-m", &format!("plan: {task}")]);
    }

    let md = std::fs::read_to_string(&plan_path).ok()?;
    let (done, total) = plan::progress(&md);
    match plan::first_open_task(&md) {
        Some(next) => {
            // Fresh duet round for the next task.
            DuetState::new(ps.max_rounds, &ps.gate, &next).save(&worker.id()).ok()?;
            if deliver_instruction(worker, &m::plan_task_instruction(&next), false) {
                Some(m::ntfy_plan_progress(&worker.name, done, total))
            } else {
                let _ = session::write_hook_state(&worker.id(), krill_core::session::HookState::NeedsYou);
                Some(m::ntfy_duet_worker_dead(&worker.name))
            }
        }
        None => {
            ps.phase = PlanPhase::Done;
            ps.save(&worker.id()).ok()?;
            Some(m::ntfy_plan_done(&worker.name))
        }
    }
}

/// Re-create a dead duet session's tmux half in place: same name, same
/// worktree, same launch cmd (the meta stores it verbatim, env prefix
/// included). Only worth doing for roles that carry no turn context —
/// the reviewer gets its full instruction every round (§12.1).
fn revive_session(meta: &SessionMeta) -> Result<()> {
    tmux::new_session(&meta.tmux, &meta.worktree)?;
    let log = meta.log_path()?;
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    tmux::pipe_to_log(&meta.tmux, &log)?;
    if !meta.cmd.is_empty() {
        tmux::send_line(&meta.tmux, &meta.cmd)?;
        // Give the agent TUI a beat to boot — an instruction typed into
        // a still-loading pane lands in the shell instead (BACKLOG #2).
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
    Ok(())
}

/// Deliver a referee instruction, instead of the old fire-and-forget
/// `let _ = send_line(...)` (BACKLOG #1: a dead reviewer swallowed the
/// instruction and the duet waited forever, silently). A dead target is
/// revived first when `revive` is set; after sending, a delayed bare
/// Enter re-submits in case the composer ate the first one (BACKLOG #3).
/// `false` means the instruction could not be delivered — the caller
/// must hand the duet to the human (needs-you + ntfy).
fn deliver_instruction(meta: &SessionMeta, text: &str, revive: bool) -> bool {
    if !tmux::has(&meta.tmux) {
        if !revive || revive_session(meta).is_err() {
            return false;
        }
    }
    if tmux::send_line(&meta.tmux, text).is_err() {
        return false;
    }
    std::thread::sleep(std::time::Duration::from_millis(1200));
    let _ = tmux::press_enter(&meta.tmux);
    true
}

/// The duet referee (M5b): map this session's Done to an Event, run the
/// pure `duet::step`, then execute the decided Action (send-keys /
/// detached gate / ntfy). Out-of-turn Stops fall through to None.
fn advance_duet(id: &str) -> Option<String> {
    let meta = session::find_by_id(id).ok()?;
    let dref = meta.duet.clone()?;
    let (worker, reviewer) = match dref.role {
        DuetRole::Worker => {
            let rev = session::find(&dref.peer, Some(&meta.repo_name)).ok()?;
            (meta, rev)
        }
        DuetRole::Reviewer => {
            let w = session::find(&dref.peer, Some(&meta.repo_name)).ok()?;
            (w, meta)
        }
    };
    let review_path = worker.worktree.join("REVIEW.md");
    let event = match dref.role {
        DuetRole::Worker => Event::WorkerDone,
        DuetRole::Reviewer => {
            let review = std::fs::read_to_string(&review_path).ok();
            Event::ReviewerDone(duet::parse_verdict(review.as_deref()))
        }
    };
    let worker_id = worker.id();
    let state = DuetState::load(&worker_id).ok()?;
    let (next, action) = duet::step(&state, event);
    let action = action?;
    next.save(&worker_id).ok()?;

    match action {
        Action::PingReviewer => {
            // A stale verdict from the previous round must not survive
            // into this one (§12.1 file lifecycle).
            let _ = std::fs::remove_file(&review_path);
            let _ = std::fs::remove_file(worker.worktree.join("GATE.md"));
            // Plan walks scope the review to the current task's work —
            // the branch gains a commit per task, and re-reviewing the
            // whole diff grows without bound (BACKLOG #7). One-shot
            // duets keep the full branch-vs-base scope.
            let text = match PlanState::load(&worker_id) {
                Ok(ps) if ps.phase == PlanPhase::Running => m::plan_review_instruction(&next.goal),
                _ => m::duet_review_instruction(&next.goal),
            };
            if deliver_instruction(&reviewer, &text, true) {
                None
            } else {
                let _ = session::write_hook_state(&worker_id, krill_core::session::HookState::NeedsYou);
                Some(m::ntfy_duet_reviewer_dead(&worker.name))
            }
        }
        Action::PingWorkerReview => {
            if deliver_instruction(&worker, &m::duet_fix_instruction(next.round, next.max_rounds), false) {
                None
            } else {
                let _ = session::write_hook_state(&worker_id, krill_core::session::HookState::NeedsYou);
                Some(m::ntfy_duet_worker_dead(&worker.name))
            }
        }
        Action::ReinstructReviewer => {
            if deliver_instruction(&reviewer, &m::duet_review_missing(), true) {
                None
            } else {
                let _ = session::write_hook_state(&worker_id, krill_core::session::HookState::NeedsYou);
                Some(m::ntfy_duet_reviewer_dead(&worker.name))
            }
        }
        Action::RunGate => {
            // LGTM is consumed — REVIEW.md must not pollute gate/merge.
            let _ = std::fs::remove_file(&review_path);
            // The gate (e.g. cargo test) is slow: run it in a detached
            // child so the reviewer's Stop hook returns instantly.
            let exe = std::env::current_exe().unwrap_or_else(|_| "krill".into());
            let _ = Command::new(exe)
                .args(["duet-gate", "-i", &worker_id])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            None
        }
        Action::Complete => {
            let _ = std::fs::remove_file(&review_path);
            duet_complete_note(&worker)
        }
        Action::Stall => {
            let _ = session::write_hook_state(&worker_id, krill_core::session::HookState::NeedsYou);
            Some(m::ntfy_duet_stalled(&worker.name, next.max_rounds))
        }
        // Only the gate child (PingWorkerGate) and the resume command
        // (PingWorkerResume) can produce these actions.
        Action::PingWorkerGate | Action::PingWorkerResume => None,
    }
}

/// `krill resume <name> [--rounds N]` (BACKLOG #6) — the human override
/// for a stalled duet: reset the rework rounds (optionally raising the
/// cap) and hand the turn back to the worker. The state transition is
/// `duet::step`'s — this is just its IO.
pub fn resume(name: &str, repo: Option<&str>, rounds: Option<&str>) -> Result<()> {
    let meta = session::find(name, repo)?;
    if let Some(d) = &meta.duet {
        if d.role == DuetRole::Reviewer {
            bail!(m::resume_on_reviewer(&d.peer));
        }
    }
    let worker_id = meta.id();
    let Ok(state) = DuetState::load(&worker_id) else {
        bail!(m::resume_not_duet(name));
    };
    let new_max: Option<u32> = match rounds {
        Some(v) => match v.parse().ok().filter(|n: &u32| *n >= 1) {
            Some(n) => Some(n),
            None => bail!(m::duet_bad_rounds(v)),
        },
        None => None,
    };
    let (next, action) = duet::step(&state, Event::Resume { new_max });
    if action.is_none() {
        bail!(m::resume_not_stalled(name, state.awaiting.as_str()));
    }
    next.save(&worker_id)?;
    if !deliver_instruction(&meta, &m::duet_resume_instruction(next.max_rounds), false) {
        bail!(m::resume_send_failed(name));
    }
    println!("{}", m::resume_done(name, next.max_rounds));
    Ok(())
}

/// `krill duet-gate -i <worker-id>` (internal) — the detached gate run.
/// Executes the gate command in the worktree, then feeds the result back
/// through `duet::step`.
pub fn duet_gate(worker_id: &str) -> Result<()> {
    let worker = session::find_by_id(worker_id)?;
    let state = DuetState::load(worker_id)?;
    if state.awaiting != Awaiting::Gate {
        return Ok(()); // late or duplicate — ignore
    }
    let out = Command::new("sh")
        .args(["-c", &state.gate])
        .current_dir(&worker.worktree)
        .output()
        .context(m::duet_gate_run_failed(&state.gate))?;
    let pass = out.status.success();
    if !pass {
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        let body = format!(
            "{}\n\n```\n{}\n```\n",
            m::gate_md_header(&state.gate),
            tail_chars(&text, 4000)
        );
        let _ = std::fs::write(worker.worktree.join("GATE.md"), body);
    }
    let (next, action) = duet::step(&state, Event::GateFinished { pass });
    let Some(action) = action else { return Ok(()) };
    next.save(worker_id)?;
    let note = match action {
        Action::Complete => duet_complete_note(&worker),
        Action::PingWorkerGate => {
            if deliver_instruction(&worker, &m::duet_gate_fix_instruction(next.round, next.max_rounds), false) {
                None
            } else {
                let _ = session::write_hook_state(worker_id, krill_core::session::HookState::NeedsYou);
                Some(m::ntfy_duet_worker_dead(&worker.name))
            }
        }
        Action::Stall => {
            let _ = session::write_hook_state(worker_id, krill_core::session::HookState::NeedsYou);
            Some(m::ntfy_duet_stalled(&worker.name, next.max_rounds))
        }
        _ => None,
    };
    if let (Some(topic), Some(body)) =
        (Config::load().ok().and_then(|c| c.ntfy_topic), note)
    {
        ntfy_push(&topic, &body);
    }
    Ok(())
}

/// Last `max` characters, cut on a char boundary (like serve's diff cap).
fn tail_chars(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut start = s.len() - max;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

/// Advance a flow chain after a Done hook. Never fails the hook: spawn
/// errors are reported (stderr + ntfy body) and swallowed. Returns the
/// ntfy body describing what the chain did, if it did anything.
fn advance_flow(config: &Config, id: &str) -> Option<String> {
    let meta = session::find_by_id(id).ok()?;
    let fr = meta.flow.clone()?;
    let taken: Vec<String> = session::load_all()
        .ok()?
        .into_iter()
        .filter(|m| m.repo_name == meta.repo_name)
        .map(|m| m.name)
        .collect();
    match session::flow_next(&config.flows, &fr, &taken) {
        FlowNext::Spawn { stage_no, stage, name } => {
            let total = config.flows[&fr.flow].len();
            let prompt = krill_core::config::stage_prompt(stage.m.as_deref(), &fr.goal);
            let next_fr = FlowRef { stage: stage_no, ..fr.clone() };
            match create_session_full(
                &name,
                stage.agent.as_deref(),
                None,
                prompt.as_deref(),
                Some(&meta.name),
                Some(&meta.repo_path), // never resolve the repo from the worktree cwd
                Some(next_fr),
            ) {
                Ok(next) => Some(m::ntfy_flow_next(&fr.flow, stage_no, total, &next.name, &next.agent)),
                Err(e) => {
                    eprintln!("krill: {e}");
                    Some(m::ntfy_flow_spawn_failed(&fr.flow, &name, &e.to_string()))
                }
            }
        }
        FlowNext::End => Some(m::ntfy_flow_done(&fr.flow, &meta.name)),
        FlowNext::Exists => None, // Stop fires every turn — nothing new to do
        FlowNext::UnknownFlow => {
            eprintln!("krill: {}", m::flow_unknown(&fr.flow, &flow_names(config)));
            None
        }
    }
}

/// Bare topics go to ntfy.sh; anything with a scheme is used verbatim
/// (self-hosted ntfy, or any endpoint that accepts a plain POST).
fn ntfy_url(topic: &str) -> String {
    if topic.contains("://") {
        topic.to_string()
    } else {
        format!("https://ntfy.sh/{topic}")
    }
}

/// Kill tmux + remove worktree + delete branch + drop meta. Returns a
/// warning line when the branch is kept (not merged). No terminal
/// output — the CLI (`rm`) and the TUI both wrap this.
pub fn remove_session(meta: &SessionMeta, force: bool) -> Result<Option<String>> {
    // Reviewer half of a duet: only its tmux + meta go — the worktree
    // and branch belong to the worker.
    if matches!(&meta.duet, Some(d) if d.role == DuetRole::Reviewer) {
        if tmux::has(&meta.tmux) {
            tmux::kill(&meta.tmux)?;
        }
        meta.delete()?;
        return Ok(None);
    }
    // Worker half: the duet is one unit — take the reviewer down too.
    if let Some(d) = &meta.duet {
        if let Ok(rev) = session::find(&d.peer, Some(&meta.repo_name)) {
            if tmux::has(&rev.tmux) {
                let _ = tmux::kill(&rev.tmux);
            }
            let _ = rev.delete();
        }
        DuetState::delete(&meta.id());
    }
    PlanState::delete(&meta.id());
    if tmux::has(&meta.tmux) {
        tmux::kill(&meta.tmux)?;
    }
    if meta.worktree.exists() {
        // Leftover referee protocol files are krill's, not the agent's —
        // untracked ones would block a non-force worktree remove.
        if meta.duet.is_some() {
            for f in ["REVIEW.md", "GATE.md"] {
                let p = meta.worktree.join(f);
                if p.exists()
                    && git::run(&meta.worktree, &["ls-files", f])
                        .map(|out| out.is_empty())
                        .unwrap_or(false)
                {
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
        // krill's own injected hook settings are untracked and would
        // make a clean worktree look dirty to `git worktree remove` —
        // clean them up first (only when untracked, i.e. ours).
        let injected = meta.worktree.join(".claude").join("settings.local.json");
        if injected.exists()
            && git::run(&meta.worktree, &["ls-files", ".claude/settings.local.json"])
                .map(|out| out.is_empty())
                .unwrap_or(false)
        {
            let _ = std::fs::remove_file(&injected);
            let _ = std::fs::remove_dir(meta.worktree.join(".claude"));
        }
        if let Err(e) = git::worktree_remove(&meta.repo_path, &meta.worktree, force) {
            bail!(m::rm_worktree_failed(&e.to_string(), &meta.name));
        }
    }
    let _ = git::run(&meta.repo_path, &["worktree", "prune"]);
    let warning = git::branch_delete(&meta.repo_path, &meta.branch, force)
        .err()
        .map(|e| m::rm_branch_kept(&e.to_string()));
    meta.delete()?;
    Ok(warning)
}

#[cfg(test)]
mod tests {
    use super::{first_line, tmux_safe};

    #[test]
    fn tmux_safe_replaces_chars_tmux_dislikes() {
        assert_eq!(tmux_safe("krill_web_fix-1"), "krill_web_fix-1");
        assert_eq!(tmux_safe("a.b:c d"), "a-b-c-d");
        assert_eq!(tmux_safe("한글x"), "--x");
    }

    #[test]
    fn is_dirty_ignores_injected_claude_settings() {
        assert!(!super::is_dirty("", false));
        assert!(!super::is_dirty("?? .claude/\n", false));
        assert!(!super::is_dirty("?? .claude/settings.local.json\n", false));
        assert!(super::is_dirty(" M src/main.rs\n", false));
        assert!(super::is_dirty("?? .claude/\n?? new-file.rs\n", false));
    }

    #[test]
    fn is_dirty_ignores_duet_protocol_files_only_for_duets() {
        assert!(super::is_dirty("?? REVIEW.md\n", false));
        assert!(!super::is_dirty("?? REVIEW.md\n?? GATE.md\n", true));
        assert!(super::is_dirty("?? REVIEW.md\n M src/lib.rs\n", true));
        // Only root-level protocol files are krill's.
        assert!(super::is_dirty("?? docs/REVIEW.md\n", true));
    }

    #[test]
    fn tail_chars_cuts_on_char_boundaries() {
        assert_eq!(super::tail_chars("abcdef", 10), "abcdef");
        assert_eq!(super::tail_chars("abcdef", 3), "def");
        // 한글 is 3 bytes/char: a naive byte slice would panic.
        let s = "가나다";
        assert_eq!(super::tail_chars(s, 4), "다");
        assert_eq!(super::tail_chars(s, 6), "나다");
    }

    #[test]
    fn ntfy_url_prepends_ntfy_sh_for_bare_topics() {
        assert_eq!(super::ntfy_url("krill-x"), "https://ntfy.sh/krill-x");
        assert_eq!(super::ntfy_url("http://127.0.0.1:9/t"), "http://127.0.0.1:9/t");
        assert_eq!(super::ntfy_url("https://my.ntfy/t"), "https://my.ntfy/t");
    }

    #[test]
    fn first_line_trims_and_defaults() {
        assert_eq!(first_line("100.101.1.2\nfe80::1\n"), "100.101.1.2");
        assert_eq!(first_line("  spaced  \nrest"), "spaced");
        assert_eq!(first_line(""), "");
    }
}
