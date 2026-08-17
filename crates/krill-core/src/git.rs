use crate::bail;
use crate::config::{expand_tilde, Config};
use crate::error::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A resolved target repository for a session.
#[derive(Debug, Clone)]
pub struct RepoRef {
    pub name: String,
    pub path: PathBuf,
    pub base: String,
}

/// Run `git -C <dir> <args...>`, returning trimmed stdout or a rich error.
pub fn run(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .context("git을 실행할 수 없습니다 (설치돼 있나요?)")?;
    if !out.status.success() {
        bail!(
            "git {} 실패: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn toplevel(dir: &Path) -> Option<PathBuf> {
    run(dir, &["rev-parse", "--show-toplevel"]).ok().map(PathBuf::from)
}

/// Best-effort default branch detection: origin/HEAD > main > master > current.
pub fn detect_base(repo: &Path) -> String {
    if let Ok(s) = run(repo, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]) {
        if let Some(b) = s.strip_prefix("origin/") {
            return b.to_string();
        }
    }
    for b in ["main", "master"] {
        if run(repo, &["show-ref", "--verify", &format!("refs/heads/{b}")]).is_ok() {
            return b.to_string();
        }
    }
    run(repo, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|_| "main".into())
}

/// Resolve which repo a command targets: `-r` flag > configured repo
/// containing cwd > ad-hoc git repo at cwd.
pub fn resolve_repo(config: &Config, flag: Option<&str>, cwd: &Path) -> Result<RepoRef> {
    if let Some(name) = flag {
        let rc = config.repos.get(name).with_context(|| {
            format!(
                "'{}' 리포가 설정에 없습니다. 등록된 리포: {}",
                name,
                if config.repos.is_empty() {
                    "(없음)".to_string()
                } else {
                    config.repos.keys().cloned().collect::<Vec<_>>().join(", ")
                }
            )
        })?;
        let path = expand_tilde(&rc.path);
        if toplevel(&path).is_none() {
            bail!("리포 경로가 git 저장소가 아닙니다: {}", path.display());
        }
        return Ok(RepoRef { name: name.into(), path, base: rc.base.clone() });
    }

    let Some(top) = toplevel(cwd) else {
        bail!("git 리포 안이 아닙니다. 리포 안에서 실행하거나 -r <이름>으로 지정하세요.");
    };

    // Is cwd's repo one of the configured ones?
    for (name, rc) in &config.repos {
        let p = expand_tilde(&rc.path);
        let same = match (p.canonicalize(), top.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => p == top,
        };
        if same {
            return Ok(RepoRef { name: name.clone(), path: top, base: rc.base.clone() });
        }
    }

    // Ad-hoc: use the repo we're standing in.
    let name = top
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".into());
    let base = detect_base(&top);
    Ok(RepoRef { name, path: top, base })
}

pub fn worktree_add(repo: &Path, wt: &Path, branch: &str, base: &str) -> Result<()> {
    let wt_s = wt.to_string_lossy();
    run(repo, &["worktree", "add", "-b", branch, &wt_s, base]).map(|_| ())
}

pub fn worktree_remove(repo: &Path, wt: &Path, force: bool) -> Result<()> {
    let wt_s = wt.to_string_lossy().to_string();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&wt_s);
    run(repo, &args).map(|_| ())
}

pub fn branch_delete(repo: &Path, branch: &str, force: bool) -> Result<()> {
    run(repo, &["branch", if force { "-D" } else { "-d" }, branch]).map(|_| ())
}

/// Compact "+ins −del" summary of a worktree (including uncommitted
/// changes) vs its base, or "clean".
pub fn shortstat(wt: &Path, base: &str) -> String {
    let s = run(wt, &["diff", "--shortstat", base]).unwrap_or_default();
    let (mut files, mut ins, mut del) = (0u64, 0u64, 0u64);
    for part in s.split(',') {
        let part = part.trim();
        let n: u64 = part
            .split_whitespace()
            .next()
            .and_then(|x| x.parse().ok())
            .unwrap_or(0);
        if part.contains("insertion") {
            ins += n;
        } else if part.contains("deletion") {
            del += n;
        } else if part.contains("changed") {
            files += n;
        }
    }
    if files == 0 && ins == 0 && del == 0 {
        "clean".into()
    } else {
        format!("+{ins} −{del}")
    }
}
