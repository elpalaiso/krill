//! Config loading. M0 parses a small TOML subset by hand (sections,
//! string values, comments) — enough for this file's shape. Swap in the
//! real `toml` crate when other deps arrive (M1+).

use crate::bail;
use crate::error::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct AgentCfg {
    /// Command template. `{prompt}` is replaced with the `-m` message
    /// (shell-quoted). An empty cmd means "just a shell".
    pub cmd: String,
    /// Reserved for M3: hook preset name (e.g. "claude-code").
    pub hooks: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RepoCfg {
    pub path: PathBuf,
    pub base: String,
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub default_agent: Option<String>,
    pub agents: BTreeMap<String, AgentCfg>,
    pub repos: BTreeMap<String, RepoCfg>,
}

pub const DEFAULT_CONFIG: &str = r#"# krill config
# 에이전트는 stdin/stdout을 가진 CLI라면 무엇이든 등록할 수 있습니다.
# {prompt} 자리에 `krill new -m "..."`의 지시문이 (따옴표 처리되어) 들어갑니다.

default_agent = "claude"

[agents.claude]
cmd = "claude {prompt}"
# hooks = "claude-code"   # M3: 상태 훅 자동 주입

[agents.codex]
cmd = "codex {prompt}"

[agents.gemini]
cmd = "gemini {prompt}"

[agents.shell]
cmd = ""                  # 빈 cmd = 그냥 셸 (테스트용)

# [repos.myapp]
# path = "~/work/myapp"
# base = "main"
"#;

impl Config {
    /// Load the config file, or fall back to built-in defaults if it
    /// doesn't exist yet (`krill init` writes it to disk).
    pub fn load() -> Result<Config> {
        let path = crate::config_path()?;
        if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("설정 파일을 읽을 수 없습니다: {}", path.display()))?;
            parse(&raw).with_context(|| format!("설정 파일 파싱 실패: {}", path.display()))
        } else {
            parse(DEFAULT_CONFIG).context("내장 기본 설정 파싱 실패(버그)")
        }
    }

    /// Write the default config to disk unless one already exists.
    /// Returns (path, created).
    pub fn init_file() -> Result<(PathBuf, bool)> {
        let path = crate::config_path()?;
        if path.exists() {
            return Ok((path, false));
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, DEFAULT_CONFIG)?;
        Ok((path, true))
    }

    /// Pick an agent: explicit flag > default_agent > sole entry.
    pub fn resolve_agent(&self, flag: Option<&str>) -> Result<(String, AgentCfg)> {
        let name = match flag {
            Some(n) => n.to_string(),
            None => match &self.default_agent {
                Some(d) => d.clone(),
                None if self.agents.len() == 1 => self.agents.keys().next().unwrap().clone(),
                None => bail!("에이전트를 지정하세요 (-a). 등록된 에이전트: {}", self.agent_names()),
            },
        };
        match self.agents.get(&name) {
            Some(cfg) => Ok((name, cfg.clone())),
            None => bail!(
                "'{}' 에이전트가 설정에 없습니다. 등록된 에이전트: {}",
                name,
                self.agent_names()
            ),
        }
    }

    fn agent_names(&self) -> String {
        if self.agents.is_empty() {
            "(없음 — `krill init` 후 config.toml을 편집하세요)".into()
        } else {
            self.agents.keys().cloned().collect::<Vec<_>>().join(", ")
        }
    }
}

/// Expand a leading `~` to the home directory.
pub fn expand_tilde(p: &Path) -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) => expand_tilde_with(p, Path::new(&home)),
        None => p.to_path_buf(),
    }
}

fn expand_tilde_with(p: &Path, home: &Path) -> PathBuf {
    match p.strip_prefix("~") {
        Ok(stripped) => home.join(stripped),
        Err(_) => p.to_path_buf(),
    }
}

// ---- tiny TOML-subset parser ------------------------------------------------

fn parse(raw: &str) -> Result<Config> {
    let mut config = Config::default();
    let mut section = String::new();

    for (idx, line) in raw.lines().enumerate() {
        let lineno = idx + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            let Some(name) = rest.strip_suffix(']') else {
                bail!("{lineno}행: 섹션 헤더가 ]로 끝나지 않습니다");
            };
            section = name.trim().to_string();
            continue;
        }
        let Some((key, rawval)) = line.split_once('=') else {
            bail!("{lineno}행: `key = value` 형식이 아닙니다: {line}");
        };
        let key = key.trim();
        let value = parse_value(rawval.trim())
            .with_context(|| format!("{lineno}행: 값 파싱 실패"))?;

        match section.as_str() {
            "" => {
                if key == "default_agent" {
                    config.default_agent = Some(value);
                }
                // unknown top-level keys are ignored (forward compat)
            }
            s if s.starts_with("agents.") => {
                let name = s.trim_start_matches("agents.").to_string();
                let entry = config.agents.entry(name).or_default();
                match key {
                    "cmd" => entry.cmd = value,
                    "hooks" => entry.hooks = Some(value),
                    _ => {}
                }
            }
            s if s.starts_with("repos.") => {
                let name = s.trim_start_matches("repos.").to_string();
                let entry = config.repos.entry(name).or_insert(RepoCfg {
                    path: PathBuf::new(),
                    base: "main".into(),
                });
                match key {
                    "path" => entry.path = PathBuf::from(value),
                    "base" => entry.base = value,
                    _ => {}
                }
            }
            _ => {} // unknown sections ignored (forward compat: [serve], [notify]…)
        }
    }

    for (name, rc) in &config.repos {
        if rc.path.as_os_str().is_empty() {
            bail!("[repos.{name}]에 path가 없습니다");
        }
    }
    Ok(config)
}

/// `"quoted"` (with \\ \" \n escapes), `'literal'`, or a bare token.
/// Anything after a closing quote (e.g. a trailing comment) is ignored.
fn parse_value(raw: &str) -> Result<String> {
    let mut chars = raw.chars();
    match chars.next() {
        Some('"') => {
            let mut out = String::new();
            while let Some(c) = chars.next() {
                match c {
                    '"' => return Ok(out),
                    '\\' => match chars.next() {
                        Some('n') => out.push('\n'),
                        Some('t') => out.push('\t'),
                        Some('"') => out.push('"'),
                        Some('\\') => out.push('\\'),
                        Some(other) => {
                            out.push('\\');
                            out.push(other);
                        }
                        None => bail!("문자열이 \\ 로 끝났습니다"),
                    },
                    _ => out.push(c),
                }
            }
            bail!("닫는 따옴표(\")가 없습니다")
        }
        Some('\'') => {
            let rest: String = chars.collect();
            match rest.split_once('\'') {
                Some((inner, _)) => Ok(inner.to_string()),
                None => bail!("닫는 따옴표(')가 없습니다"),
            }
        }
        _ => {
            // bare value: cut at a trailing comment
            let v = raw.split('#').next().unwrap_or("").trim();
            Ok(v.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_template() {
        let c = parse(DEFAULT_CONFIG).unwrap();
        assert_eq!(c.default_agent.as_deref(), Some("claude"));
        assert_eq!(c.agents["claude"].cmd, "claude {prompt}");
        assert!(c.agents["claude"].hooks.is_none()); // commented out
        assert_eq!(c.agents["shell"].cmd, "");
        assert!(c.agents.contains_key("codex") && c.agents.contains_key("gemini"));
        assert!(c.repos.is_empty());
    }

    #[test]
    fn parses_sections_keys_and_value_styles() {
        let c = parse(
            r##"
default_agent = "a"
future_key = "ignored"

[agents.a]
cmd = "a {prompt}"
hooks = "claude-code"
future = "ignored"

[agents.b]
cmd = 'literal "quoted"'

[repos.web]
path = "~/work/web"
base = develop # trailing comment on a bare value

[serve]
port = 7777
"##,
        )
        .unwrap();
        assert_eq!(c.default_agent.as_deref(), Some("a"));
        assert_eq!(c.agents["a"].hooks.as_deref(), Some("claude-code"));
        assert_eq!(c.agents["b"].cmd, r#"literal "quoted""#);
        assert_eq!(c.repos["web"].path, PathBuf::from("~/work/web"));
        assert_eq!(c.repos["web"].base, "develop");
        assert_eq!(c.repos.len(), 1); // [serve] ignored, not a repo
    }

    #[test]
    fn parses_escapes_in_double_quotes() {
        let c = parse("[agents.x]\ncmd = \"l1\\nl2\\t \\\"q\\\" back\\\\slash \\qkeep\"\n").unwrap();
        assert_eq!(c.agents["x"].cmd, "l1\nl2\t \"q\" back\\slash \\qkeep");
    }

    #[test]
    fn text_after_closing_quote_is_ignored() {
        let c = parse("default_agent = \"x\"   # note\n").unwrap();
        assert_eq!(c.default_agent.as_deref(), Some("x"));
    }

    #[test]
    fn rejects_malformed_input() {
        assert!(parse("[agents.x\ncmd = \"\"\n").is_err()); // unterminated section header
        assert!(parse("just-a-token\n").is_err()); // not key = value
        assert!(parse("x = \"unterminated\n").is_err()); // unclosed "
        assert!(parse("x = 'unterminated\n").is_err()); // unclosed '
        assert!(parse("x = \"abc\\").is_err()); // string ends on a bare backslash
        assert!(parse("[repos.r]\nbase = main\n").is_err()); // repo without path
    }

    #[test]
    fn resolve_agent_priority_and_errors() {
        let c = parse("default_agent = \"b\"\n[agents.a]\ncmd = \"a\"\n[agents.b]\ncmd = \"b\"\n").unwrap();
        assert_eq!(c.resolve_agent(Some("a")).unwrap().0, "a");
        assert_eq!(c.resolve_agent(None).unwrap().0, "b");
        assert!(c.resolve_agent(Some("zz")).is_err());

        let sole = parse("[agents.only]\ncmd = \"x\"\n").unwrap();
        assert_eq!(sole.resolve_agent(None).unwrap().0, "only");

        let two_no_default = parse("[agents.a]\ncmd = \"\"\n[agents.b]\ncmd = \"\"\n").unwrap();
        assert!(two_no_default.resolve_agent(None).is_err());
        assert!(Config::default().resolve_agent(None).is_err());
    }

    #[test]
    fn tilde_expansion() {
        let home = Path::new("/home/u");
        assert_eq!(expand_tilde_with(Path::new("~/w/x"), home), PathBuf::from("/home/u/w/x"));
        assert_eq!(expand_tilde_with(Path::new("/abs/p"), home), PathBuf::from("/abs/p"));
        assert_eq!(expand_tilde_with(Path::new("rel/p"), home), PathBuf::from("rel/p"));
        assert_eq!(expand_tilde_with(Path::new("~user/x"), home), PathBuf::from("~user/x"));
    }
}
