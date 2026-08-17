//! Integration tests against a real git binary — git is a hard runtime
//! requirement of krill, so exercising it here is fair game. Everything
//! runs inside per-test temp dirs and cleans up after itself.

use krill_core::config::{Config, RepoCfg};
use krill_core::git;
use std::path::{Path, PathBuf};

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(tag: &str) -> TempDir {
    let d = std::env::temp_dir().join(format!("krill-it-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    TempDir(d)
}

/// git init + one commit, with identity configured repo-locally.
fn init_repo(container: &Path) -> PathBuf {
    let repo = container.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git::run(&repo, &["init"]).unwrap();
    git::run(&repo, &["config", "user.email", "krill@test.invalid"]).unwrap();
    git::run(&repo, &["config", "user.name", "krill-test"]).unwrap();
    std::fs::write(repo.join("README.md"), "hello\n").unwrap();
    git::run(&repo, &["add", "."]).unwrap();
    git::run(&repo, &["commit", "-m", "init"]).unwrap();
    repo
}

#[test]
fn run_reports_failing_command_in_error() {
    let t = temp("run-err");
    let repo = init_repo(&t.0);
    let err = git::run(&repo, &["not-a-subcommand"]).unwrap_err().to_string();
    assert!(err.contains("not-a-subcommand"), "unexpected error: {err}");
}

#[test]
fn toplevel_and_detect_base() {
    let t = temp("base");
    let repo = init_repo(&t.0);

    let top = git::toplevel(&repo).unwrap();
    assert_eq!(top.canonicalize().unwrap(), repo.canonicalize().unwrap());
    assert!(git::toplevel(&t.0).is_none()); // container dir is not a repo

    // No origin/HEAD here, so detection lands on the actual local branch.
    let base = git::detect_base(&repo);
    let current = git::run(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap();
    assert_eq!(base, current);
}

#[test]
fn worktree_lifecycle_and_shortstat() {
    let t = temp("wt");
    let repo = init_repo(&t.0);
    let base = git::detect_base(&repo);
    let wt = t.0.join("wt-fix");

    git::worktree_add(&repo, &wt, "krill/fix", &base).unwrap();
    assert!(wt.join("README.md").exists());
    assert_eq!(git::shortstat(&wt, &base), "clean");

    std::fs::write(wt.join("README.md"), "hello\nworld\n").unwrap();
    assert_eq!(git::shortstat(&wt, &base), "+1 −0");

    // A dirty worktree must survive a non-forced remove attempt.
    assert!(git::worktree_remove(&repo, &wt, false).is_err());
    assert!(wt.exists());

    git::worktree_remove(&repo, &wt, true).unwrap();
    assert!(!wt.exists());

    git::branch_delete(&repo, "krill/fix", true).unwrap();
    assert!(git::run(&repo, &["show-ref", "--verify", "refs/heads/krill/fix"]).is_err());
}

#[test]
fn resolve_repo_flag_configured_and_ad_hoc() {
    let t = temp("resolve");
    let repo = init_repo(&t.0);

    // Ad-hoc: cwd inside an unconfigured repo uses the directory name.
    let empty = Config::default();
    let r = git::resolve_repo(&empty, None, &repo).unwrap();
    assert_eq!(r.name, "repo");
    assert_eq!(r.path.canonicalize().unwrap(), repo.canonicalize().unwrap());

    // Configured repo matched by path, then selected by flag.
    let mut cfg = Config::default();
    cfg.repos.insert("myname".into(), RepoCfg { path: repo.clone(), base: "main".into(), gate: None });
    assert_eq!(git::resolve_repo(&cfg, None, &repo).unwrap().name, "myname");
    let by_flag = git::resolve_repo(&cfg, Some("myname"), &t.0).unwrap();
    assert_eq!(by_flag.name, "myname");
    assert_eq!(by_flag.base, "main");

    // Errors: unknown flag name, and cwd outside any repo.
    assert!(git::resolve_repo(&cfg, Some("nope"), &repo).is_err());
    assert!(git::resolve_repo(&empty, None, &t.0).is_err());
}
