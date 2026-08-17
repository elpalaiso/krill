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

    println!("{BOLD}{name}{RESET} {}", m::session_started());
    println!("  repo     {} ({})", repo_ref.name, repo_ref.path.display());
    println!("  branch   {branch} (base: {base})");
    println!("  worktree {}", worktree.display());
    println!("  agent    {agent_name}{}", if cmd.is_empty() { m::shell_only() } else { String::new() });
    println!();
    println!("{}", m::attach_hint(&format!("{BOLD}krill attach {name}{RESET}")));
    Ok(())
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
    // Diff the working tree against base — includes uncommitted changes,
    // which agents frequently leave behind.
    let mut args: Vec<String> = vec![
        "-C".into(),
        meta.worktree.display().to_string(),
        "diff".into(),
    ];
    if stat {
        args.push("--stat".into());
    }
    args.push(meta.base.clone());
    let status = Command::new("git")
        .args(&args)
        .status()
        .context(m::git_exec_failed())?;
    if !status.success() {
        bail!(m::git_diff_exit(&status.to_string()));
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

    if tmux::has(&meta.tmux) {
        tmux::kill(&meta.tmux)?;
    }
    if meta.worktree.exists() {
        if let Err(e) = git::worktree_remove(&meta.repo_path, &meta.worktree, force) {
            bail!(m::rm_worktree_failed(&e.to_string(), &meta.name));
        }
    }
    let _ = git::run(&meta.repo_path, &["worktree", "prune"]);
    if let Err(e) = git::branch_delete(&meta.repo_path, &meta.branch, force) {
        eprintln!("{DIM}{}{RESET}", m::rm_branch_kept(&e.to_string()));
        eprintln!("{DIM}{}{RESET}", m::rm_branch_hint(&meta.branch));
    }
    meta.delete()?;
    println!("{}", m::rm_done(&meta.name));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::tmux_safe;

    #[test]
    fn tmux_safe_replaces_chars_tmux_dislikes() {
        assert_eq!(tmux_safe("krill_web_fix-1"), "krill_web_fix-1");
        assert_eq!(tmux_safe("a.b:c d"), "a-b-c-d");
        assert_eq!(tmux_safe("한글x"), "--x");
    }
}
