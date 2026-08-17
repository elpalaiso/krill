use crate::msg as m;
use krill_core::bail;
use krill_core::config::Config;
use krill_core::error::{Context, Result};
use krill_core::git;
use krill_core::session::{self, Health, SessionMeta};
use krill_core::tmux;
use std::io::Write as _;
use std::process::Command;

const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
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
) -> Result<()> {
    let meta = create_session(name, agent, repo, message, from)?;
    println!("{BOLD}{}{RESET} {}", meta.name, m::session_started());
    println!("  repo     {} ({})", meta.repo_name, meta.repo_path.display());
    println!("  branch   {} (base: {})", meta.branch, meta.base);
    println!("  worktree {}", meta.worktree.display());
    println!(
        "  agent    {}{}",
        meta.agent,
        if meta.cmd.is_empty() { m::shell_only() } else { String::new() }
    );
    println!();
    println!("{}", m::attach_hint(&format!("{BOLD}krill attach {}{RESET}", meta.name)));
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
    if !krill_core::valid_name(name) {
        bail!(m::invalid_session_name(name));
    }
    let config = Config::load()?;
    let cwd = std::env::current_dir()?;
    let repo_ref = git::resolve_repo(&config, repo, &cwd)?;
    let (agent_name, agent_cfg) = config.resolve_agent(agent)?;

    if session::load_all()?
        .iter()
        .any(|m| m.name == name && m.repo_name == repo_ref.name)
    {
        bail!(m::session_exists(name, &repo_ref.name));
    }

    // Relay handoff (--from): branch off another session's work instead of base.
    let base = match from {
        Some(f) => {
            let src = session::find(f, None)?;
            if src.repo_name != repo_ref.name {
                bail!(m::from_other_repo(&src.repo_name));
            }
            src.branch
        }
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
        state: String,
        last: String,
        diff: String,
    }

    let mut rows = Vec::new();
    for m in &metas {
        let (h, age) = session::health(m, &live);
        let (dot, state) = match h {
            Health::Active => (format!("{GREEN}●{RESET}"), "active".to_string()),
            Health::Quiet => (format!("{YELLOW}●{RESET}"), "quiet".to_string()),
            Health::Dead => (format!("{RED}✖{RESET}"), "dead".to_string()),
        };
        let attached = h != Health::Dead && tmux::attached_count(&m.tmux) > 0;
        let last = age.map(krill_core::fmt_age).unwrap_or_else(|| "-".into());
        let diff = if h == Health::Dead {
            "-".to_string()
        } else {
            git::shortstat(&m.worktree, &m.base)
        };
        rows.push(Row {
            dot,
            name: m.name.clone(),
            repo: m.repo_name.clone(),
            agent: m.agent.clone(),
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

    println!(
        "{DIM}  {:<nw$}  {:<rw$}  {:<aw$}  {:<sw$}  {:<lw$}  {}{RESET}",
        "NAME", "REPO", "AGENT", "STATE", "LAST", "DIFF"
    );
    for r in rows {
        println!(
            "{} {:<nw$}  {:<rw$}  {:<aw$}  {:<sw$}  {:<lw$}  {}",
            r.dot, r.name, r.repo, r.agent, r.state, r.last, r.diff
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

/// Kill tmux + remove worktree + delete branch + drop meta. Returns a
/// warning line when the branch is kept (not merged). No terminal
/// output — the CLI (`rm`) and the TUI both wrap this.
pub fn remove_session(meta: &SessionMeta, force: bool) -> Result<Option<String>> {
    if tmux::has(&meta.tmux) {
        tmux::kill(&meta.tmux)?;
    }
    if meta.worktree.exists() {
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
    fn first_line_trims_and_defaults() {
        assert_eq!(first_line("100.101.1.2\nfe80::1\n"), "100.101.1.2");
        assert_eq!(first_line("  spaced  \nrest"), "spaced");
        assert_eq!(first_line(""), "");
    }
}
