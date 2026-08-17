use crate::error::{Context, Result};
use crate::{bail, kv};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub name: String,
    pub repo_name: String,
    pub repo_path: PathBuf,
    pub base: String,
    pub branch: String,
    pub worktree: PathBuf,
    pub agent: String,
    /// Full command line sent to the session ("" = plain shell).
    pub cmd: String,
    /// tmux session name.
    pub tmux: String,
    pub created_unix: u64,
}

impl SessionMeta {
    pub fn id(&self) -> String {
        format!("{}--{}", self.repo_name, self.name)
    }

    pub fn meta_path(&self) -> Result<PathBuf> {
        Ok(crate::sessions_dir()?.join(format!("{}.kv", self.id())))
    }

    pub fn log_path(&self) -> Result<PathBuf> {
        Ok(crate::logs_dir()?.join(format!("{}.log", self.id())))
    }

    fn to_map(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("name".into(), self.name.clone());
        m.insert("repo_name".into(), self.repo_name.clone());
        m.insert("repo_path".into(), self.repo_path.display().to_string());
        m.insert("base".into(), self.base.clone());
        m.insert("branch".into(), self.branch.clone());
        m.insert("worktree".into(), self.worktree.display().to_string());
        m.insert("agent".into(), self.agent.clone());
        m.insert("cmd".into(), self.cmd.clone());
        m.insert("tmux".into(), self.tmux.clone());
        m.insert("created_unix".into(), self.created_unix.to_string());
        m
    }

    fn from_map(m: &BTreeMap<String, String>) -> Result<SessionMeta> {
        let req = |k: &str| -> Result<String> {
            m.get(k).cloned().with_context(|| format!("세션 메타에 '{k}' 필드가 없습니다"))
        };
        Ok(SessionMeta {
            name: req("name")?,
            repo_name: req("repo_name")?,
            repo_path: PathBuf::from(req("repo_path")?),
            base: req("base")?,
            branch: req("branch")?,
            worktree: PathBuf::from(req("worktree")?),
            agent: req("agent")?,
            cmd: m.get("cmd").cloned().unwrap_or_default(),
            tmux: req("tmux")?,
            created_unix: req("created_unix")?
                .parse()
                .context("created_unix 파싱 실패")?,
        })
    }

    pub fn save(&self) -> Result<()> {
        let path = self.meta_path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        kv::write_file(&path, &self.to_map())
            .with_context(|| format!("세션 메타 저장 실패: {}", path.display()))
    }

    pub fn delete(&self) -> Result<()> {
        if let Ok(p) = self.meta_path() {
            let _ = std::fs::remove_file(p);
        }
        if let Ok(p) = self.log_path() {
            let _ = std::fs::remove_file(p);
        }
        Ok(())
    }
}

/// Session health as far as M0 can tell. Precise NeedsYou/Done states
/// arrive with hooks in M3 (design doc §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// tmux alive, output within the last 30s.
    Active,
    /// tmux alive, no recent output.
    Quiet,
    /// tmux session is gone.
    Dead,
}

pub fn health(meta: &SessionMeta, live_sessions: &[String]) -> (Health, Option<u64>) {
    if !live_sessions.iter().any(|s| s == &meta.tmux) {
        return (Health::Dead, None);
    }
    let age = meta
        .log_path()
        .ok()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok())
        .map(|d| d.as_secs());
    match age {
        Some(a) if a <= 30 => (Health::Active, Some(a)),
        other => (Health::Quiet, other),
    }
}

pub fn load_all() -> Result<Vec<SessionMeta>> {
    let dir = crate::sessions_dir()?;
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(out);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "kv") {
            match kv::read_file(&path).and_then(|m| SessionMeta::from_map(&m)) {
                Ok(meta) => out.push(meta),
                Err(e) => eprintln!("경고: 세션 메타 손상 무시 {} ({e})", path.display()),
            }
        }
    }
    out.sort_by_key(|m| std::cmp::Reverse(m.created_unix));
    Ok(out)
}

/// Find a session by name (unique across repos, or disambiguate with repo).
pub fn find(name: &str, repo: Option<&str>) -> Result<SessionMeta> {
    let all = load_all()?;
    let matches: Vec<_> = all
        .into_iter()
        .filter(|m| m.name == name && repo.map_or(true, |r| m.repo_name == r))
        .collect();
    match matches.len() {
        0 => bail!("'{name}' 세션이 없습니다. `krill ls`로 확인하세요."),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => bail!(
            "'{name}' 세션이 여러 리포에 있습니다 ({}). -r <리포>로 지정하세요.",
            matches
                .iter()
                .map(|m| m.repo_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
