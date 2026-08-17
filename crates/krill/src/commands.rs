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
        println!("설정 파일 생성: {}", path.display());
        println!("에이전트와 리포를 편집한 뒤 `krill new <이름>`으로 시작하세요.");
    } else {
        println!("설정 파일이 이미 있습니다: {}", path.display());
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
        bail!("세션 이름은 영숫자/대시/언더스코어 64자 이내여야 합니다: '{name}'");
    }
    let config = Config::load()?;
    let cwd = std::env::current_dir()?;
    let repo_ref = git::resolve_repo(&config, repo, &cwd)?;
    let (agent_name, agent_cfg) = config.resolve_agent(agent)?;

    if session::load_all()?
        .iter()
        .any(|m| m.name == name && m.repo_name == repo_ref.name)
    {
        bail!(
            "'{name}' 세션이 이미 있습니다 (repo: {}). 다른 이름을 쓰거나 먼저 `krill rm {name}` 하세요.",
            repo_ref.name
        );
    }

    // Relay handoff (--from): branch off another session's work instead of base.
    let base = match from {
        Some(f) => {
            let src = session::find(f, None)?;
            if src.repo_name != repo_ref.name {
                bail!("--from 세션이 다른 리포에 있습니다: {}", src.repo_name);
            }
            src.branch
        }
        None => repo_ref.base.clone(),
    };

    let branch = format!("krill/{name}");
    let worktree = krill_core::worktrees_dir()?.join(&repo_ref.name).join(name);
    if worktree.exists() {
        bail!("worktree 경로가 이미 존재합니다: {}", worktree.display());
    }
    if let Some(parent) = worktree.parent() {
        std::fs::create_dir_all(parent)?;
    }

    git::worktree_add(&repo_ref.path, &worktree, &branch, &base)
        .with_context(|| format!("worktree 생성 실패 (base: {base})"))?;

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
            bail!("tmux 세션 이름이 이미 사용 중입니다: {tmux_name}");
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

    println!("{BOLD}{name}{RESET} 세션 시작");
    println!("  repo     {} ({})", repo_ref.name, repo_ref.path.display());
    println!("  branch   {branch} (base: {base})");
    println!("  worktree {}", worktree.display());
    println!("  agent    {agent_name}{}", if cmd.is_empty() { " (셸만)" } else { "" });
    println!();
    println!("접속: {BOLD}krill attach {name}{RESET}   (분리: Ctrl-b d)");
    Ok(())
}

pub fn ls() -> Result<()> {
    let metas = session::load_all()?;
    if metas.is_empty() {
        println!("세션이 없습니다.");
        println!("시작하기: krill new <이름> -m \"지시문\"   (설정: krill init)");
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
        bail!(
            "'{}' 세션의 tmux가 죽어 있습니다. `krill rm {}`로 정리 후 다시 만드세요.",
            meta.name, meta.name
        );
    }
    tmux::attach_exec(&meta.tmux)
}

pub fn diff(name: &str, repo: Option<&str>, stat: bool) -> Result<()> {
    let meta = session::find(name, repo)?;
    if !meta.worktree.exists() {
        bail!("worktree가 없습니다: {}", meta.worktree.display());
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
        .context("git 실행 실패")?;
    if !status.success() {
        bail!("git diff 종료 코드: {status}");
    }
    Ok(())
}

pub fn rm(name: &str, repo: Option<&str>, force: bool) -> Result<()> {
    let meta = session::find(name, repo)?;

    if !force {
        eprint!(
            "'{}' 세션과 worktree, 브랜치 {}을(를) 삭제합니다. 계속? [y/N] ",
            meta.name, meta.branch
        );
        std::io::stderr().flush().ok();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).context("입력 읽기 실패")?;
        if !matches!(line.trim(), "y" | "Y") {
            println!("취소했습니다.");
            return Ok(());
        }
    }

    if tmux::has(&meta.tmux) {
        tmux::kill(&meta.tmux)?;
    }
    if meta.worktree.exists() {
        if let Err(e) = git::worktree_remove(&meta.repo_path, &meta.worktree, force) {
            bail!(
                "worktree 제거 실패 — 커밋 안 된 변경이 있는 것 같습니다.\n  {e}\n강제 삭제: krill rm {} --force",
                meta.name
            );
        }
    }
    let _ = git::run(&meta.repo_path, &["worktree", "prune"]);
    if let Err(e) = git::branch_delete(&meta.repo_path, &meta.branch, force) {
        eprintln!("{DIM}브랜치는 남겨둡니다 (머지되지 않음): {e}{RESET}");
        eprintln!(
            "{DIM}브랜치까지 지우려면: krill rm --force 또는 git branch -D {}{RESET}",
            meta.branch
        );
    }
    meta.delete()?;
    println!("정리 완료: {}", meta.name);
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
