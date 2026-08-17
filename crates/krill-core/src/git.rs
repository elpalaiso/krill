use crate::config::{expand_tilde, Config};
use crate::{bail, msg};
use crate::error::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A resolved target repository for a session.
#[derive(Debug, Clone)]
pub struct RepoRef {
    pub name: String,
    pub path: PathBuf,
    pub base: String,
    /// Duet gate command from `[repos.*] gate`, if configured.
    pub gate: Option<String>,
}

/// Run `git -C <dir> <args...>`, returning trimmed stdout or a rich error.
pub fn run(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .context(msg::git_not_found())?;
    if !out.status.success() {
        bail!(msg::git_cmd_failed(
            &args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
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
            let repos = if config.repos.is_empty() {
                msg::repo_none_registered()
            } else {
                config.repos.keys().cloned().collect::<Vec<_>>().join(", ")
            };
            msg::repo_unknown(name, &repos)
        })?;
        let path = expand_tilde(&rc.path);
        if toplevel(&path).is_none() {
            bail!(msg::repo_path_not_git(&path.display().to_string()));
        }
        return Ok(RepoRef { name: name.into(), path, base: rc.base.clone(), gate: rc.gate.clone() });
    }

    let Some(top) = toplevel(cwd) else {
        bail!(msg::not_in_repo());
    };

    // Is cwd's repo one of the configured ones?
    for (name, rc) in &config.repos {
        let p = expand_tilde(&rc.path);
        let same = match (p.canonicalize(), top.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => p == top,
        };
        if same {
            return Ok(RepoRef { name: name.clone(), path: top, base: rc.base.clone(), gate: rc.gate.clone() });
        }
    }

    // Ad-hoc: use the repo we're standing in.
    let name = top
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".into());
    let base = detect_base(&top);
    Ok(RepoRef { name, path: top, base, gate: None })
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
    parse_shortstat(&run(wt, &["diff", "--shortstat", base]).unwrap_or_default())
}

/// Parse `git diff --shortstat` output ("" = no changes).
fn parse_shortstat(s: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortstat_parsing() {
        assert_eq!(parse_shortstat(""), "clean");
        assert_eq!(parse_shortstat("3 files changed, 10 insertions(+), 2 deletions(-)"), "+10 −2");
        assert_eq!(parse_shortstat("1 file changed, 5 insertions(+)"), "+5 −0");
        assert_eq!(parse_shortstat("2 files changed, 4 deletions(-)"), "+0 −4");
    }
}
