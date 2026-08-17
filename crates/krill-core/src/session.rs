use crate::error::{Context, Result};
use crate::{bail, kv, msg};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
            m.get(k).cloned().with_context(|| msg::meta_field_missing(k))
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
                .context(msg::meta_created_parse_failed())?,
        })
    }

    pub fn save(&self) -> Result<()> {
        self.save_in(&crate::sessions_dir()?)
    }

    fn save_in(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{}.kv", self.id()));
        kv::write_file(&path, &self.to_map())
            .with_context(|| msg::meta_save_failed(&path.display().to_string()))
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
    let alive = live_sessions.iter().any(|s| s == &meta.tmux);
    let age = if alive {
        meta.log_path()
            .ok()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .map(|d| d.as_secs())
    } else {
        None
    };
    classify(alive, age)
}

/// Pure health rule: dead beats everything; ≤30s-old output = active.
fn classify(alive: bool, age_secs: Option<u64>) -> (Health, Option<u64>) {
    if !alive {
        return (Health::Dead, None);
    }
    match age_secs {
        Some(a) if a <= 30 => (Health::Active, Some(a)),
        other => (Health::Quiet, other),
    }
}

pub fn load_all() -> Result<Vec<SessionMeta>> {
    load_all_from(&crate::sessions_dir()?)
}

fn load_all_from(dir: &Path) -> Result<Vec<SessionMeta>> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(out);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "kv") {
            match kv::read_file(&path).and_then(|m| SessionMeta::from_map(&m)) {
                Ok(meta) => out.push(meta),
                Err(e) => eprintln!(
                    "{}",
                    msg::meta_corrupt_ignored(&path.display().to_string(), &e.to_string())
                ),
            }
        }
    }
    out.sort_by_key(|m| std::cmp::Reverse(m.created_unix));
    Ok(out)
}

/// Find a session by name (unique across repos, or disambiguate with repo).
pub fn find(name: &str, repo: Option<&str>) -> Result<SessionMeta> {
    find_among(load_all()?, name, repo)
}

fn find_among(all: Vec<SessionMeta>, name: &str, repo: Option<&str>) -> Result<SessionMeta> {
    let matches: Vec<_> = all
        .into_iter()
        .filter(|m| m.name == name && repo.map_or(true, |r| m.repo_name == r))
        .collect();
    match matches.len() {
        0 => bail!(msg::session_not_found(name)),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => bail!(msg::session_ambiguous(
            name,
            &matches
                .iter()
                .map(|m| m.repo_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(name: &str, repo: &str, created: u64) -> SessionMeta {
        SessionMeta {
            name: name.into(),
            repo_name: repo.into(),
            repo_path: PathBuf::from("/tmp/repo"),
            base: "main".into(),
            branch: format!("krill/{name}"),
            worktree: PathBuf::from("/tmp/wt").join(name),
            agent: "shell".into(),
            cmd: "claude 'do it'\nsecond line".into(),
            tmux: format!("krill_{repo}_{name}"),
            created_unix: created,
        }
    }

    #[test]
    fn id_combines_repo_and_name() {
        assert_eq!(meta("fix", "web", 1).id(), "web--fix");
    }

    #[test]
    fn map_roundtrip_preserves_every_field() {
        let m = meta("fix", "web", 42);
        let back = SessionMeta::from_map(&m.to_map()).unwrap();
        assert_eq!(back.name, m.name);
        assert_eq!(back.repo_name, m.repo_name);
        assert_eq!(back.repo_path, m.repo_path);
        assert_eq!(back.base, m.base);
        assert_eq!(back.branch, m.branch);
        assert_eq!(back.worktree, m.worktree);
        assert_eq!(back.agent, m.agent);
        assert_eq!(back.cmd, m.cmd);
        assert_eq!(back.tmux, m.tmux);
        assert_eq!(back.created_unix, m.created_unix);
    }

    #[test]
    fn from_map_defaults_cmd_but_requires_the_rest() {
        let mut no_cmd = meta("a", "r", 1).to_map();
        no_cmd.remove("cmd");
        assert_eq!(SessionMeta::from_map(&no_cmd).unwrap().cmd, "");

        let mut no_branch = meta("a", "r", 1).to_map();
        no_branch.remove("branch");
        assert!(SessionMeta::from_map(&no_branch).is_err());

        let mut bad_time = meta("a", "r", 1).to_map();
        bad_time.insert("created_unix".into(), "NaN".into());
        assert!(SessionMeta::from_map(&bad_time).is_err());
    }

    #[test]
    fn store_roundtrip_skips_corrupt_and_sorts_newest_first() {
        let dir = std::env::temp_dir().join(format!("krill-test-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        meta("old", "web", 100).save_in(&dir).unwrap();
        meta("new", "web", 200).save_in(&dir).unwrap();
        std::fs::write(dir.join("broken.kv"), "name=only-a-name\n").unwrap();
        std::fs::write(dir.join("not-meta.txt"), "ignored\n").unwrap();

        let all = load_all_from(&dir).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "new"); // newest first
        assert_eq!(all[1].name, "old");
        assert_eq!(all[1].cmd, "claude 'do it'\nsecond line"); // kv escaping roundtrips

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_all_from_missing_dir_is_empty() {
        let dir = std::env::temp_dir().join(format!("krill-test-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load_all_from(&dir).unwrap().is_empty());
    }

    #[test]
    fn find_among_unique_ambiguous_and_filtered() {
        let all = vec![meta("a", "r1", 1), meta("a", "r2", 2), meta("b", "r1", 3)];
        assert!(find_among(all.clone(), "missing", None).is_err());
        assert_eq!(find_among(all.clone(), "b", None).unwrap().repo_name, "r1");
        assert!(find_among(all.clone(), "a", None).is_err()); // two repos have "a"
        assert_eq!(find_among(all.clone(), "a", Some("r2")).unwrap().repo_name, "r2");
        assert!(find_among(all, "b", Some("r2")).is_err());
    }

    #[test]
    fn classify_health_states() {
        assert_eq!(classify(false, None), (Health::Dead, None));
        assert_eq!(classify(true, Some(0)), (Health::Active, Some(0)));
        assert_eq!(classify(true, Some(30)), (Health::Active, Some(30)));
        assert_eq!(classify(true, Some(31)), (Health::Quiet, Some(31)));
        assert_eq!(classify(true, None), (Health::Quiet, None));
    }
}
