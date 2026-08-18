//! Config loading. M0 parses a small TOML subset by hand (sections,
//! string values, comments) — enough for this file's shape. Swap in the
//! real `toml` crate when other deps arrive (M1+).

use crate::error::{Context, Result};
use crate::i18n::Lang;
use crate::{bail, msg};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct AgentCfg {
    /// Command template. `{prompt}` is replaced with the `-m` message
    /// (shell-quoted). An empty cmd means "just a shell".
    pub cmd: String,
    /// Reserved for M3: hook preset name (e.g. "claude-code").
    pub hooks: Option<String>,
    /// Context-compaction command typed into the agent between plan
    /// tasks (design §12.1 decision 7, BACKLOG #9) — e.g. "/compact"
    /// for Claude Code. Config data, not code: principle 2.
    pub compact: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RepoCfg {
    pub path: PathBuf,
    pub base: String,
    /// Repo-specific duet gate command (e.g. "cargo test").
    pub gate: Option<String>,
}

/// `[serve]` — the M2 web UI. Security rule (design §7): binding a
/// non-loopback address without a token refuses to start.
#[derive(Debug, Clone)]
pub struct ServeCfg {
    pub port: u16,
    pub bind: String,
    pub token: Option<String>,
}

impl Default for ServeCfg {
    fn default() -> ServeCfg {
        ServeCfg { port: 7777, bind: "127.0.0.1".into(), token: None }
    }
}

/// `[duet]` — defaults for `krill duet` (M5b). Priority per field:
/// CLI flag > repo gate (gate only) > this section > built-in.
#[derive(Debug, Clone, Default)]
pub struct DuetCfg {
    pub reviewer: Option<String>,
    pub gate: Option<String>,
    pub max_rounds: Option<u32>,
    /// Plan walks: send the worker its agent's `compact` command every
    /// N completed tasks (0/absent = off). BACKLOG #9.
    pub compact_every: Option<u32>,
}

/// One stage of a `[flows.*]` chain (design §12.1). Stages are numbered
/// sections (`[flows.shipit.1]`) because the hand-rolled parser is flat
/// on purpose; the vector index is stage number − 1.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlowStage {
    /// Agent name; None = default_agent at spawn time.
    pub agent: Option<String>,
    /// Prompt template; `{goal}` is replaced with the flow's goal.
    pub m: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Message language override ("ko" | "en"); see i18n.rs for priority.
    pub lang: Option<String>,
    pub default_agent: Option<String>,
    pub agents: BTreeMap<String, AgentCfg>,
    pub repos: BTreeMap<String, RepoCfg>,
    pub serve: ServeCfg,
    /// `[notify] ntfy_topic` — bare topic ("krill-me-x8f2") or full URL.
    pub ntfy_topic: Option<String>,
    /// `[flows.<name>.<n>]` — stage chains for `krill new --flow` (M5a).
    pub flows: BTreeMap<String, Vec<FlowStage>>,
    /// `[duet]` — worker/reviewer ping-pong defaults (M5b).
    pub duet: DuetCfg,
}

/// The prompt a flow stage sends: the stage template with `{goal}`
/// substituted, the goal itself when there is no template, or nothing
/// (bare agent) when neither exists.
pub fn stage_prompt(stage_m: Option<&str>, goal: &str) -> Option<String> {
    match stage_m {
        Some(t) => Some(t.replace("{goal}", goal)),
        None if goal.is_empty() => None,
        None => Some(goal.to_string()),
    }
}

pub const DEFAULT_CONFIG_KO: &str = r#"# krill config
# 에이전트는 stdin/stdout을 가진 CLI라면 무엇이든 등록할 수 있습니다.
# {prompt} 자리에 `krill new -m "..."`의 지시문이 (따옴표 처리되어) 들어갑니다.

# lang = "ko"             # 메시지 언어: "ko" | "en" (기본: $LANG 자동 감지)

default_agent = "claude"

[agents.claude]
cmd = "claude {prompt}"
hooks = "claude-code"     # NeedsYou/Done 상태 훅 자동 주입
# compact = "/compact"    # plan 순회에서 주기적으로 보낼 컨텍스트 압축 명령

[agents.codex]
cmd = "codex {prompt}"
# hooks = "external"      # 자체 notify 브리지로 `krill hook`을 직접 쏘는 에이전트는
                          # 이 값으로 훅 없음 경고를 끕니다 (주입은 "claude-code"만).

[agents.gemini]
cmd = "gemini {prompt}"

[agents.shell]
cmd = ""                  # 빈 cmd = 그냥 셸 (테스트용)

# [repos.myapp]
# path = "~/work/myapp"
# base = "main"

# [notify]
# ntfy_topic = "krill-나만아는-랜덤접미사"   # needs-you/done 시 폰 푸시 (ntfy.sh)

# flow: 스테이지가 Done이 되면 다음 스테이지가 자동으로 이어받습니다 (릴레이 체인).
# 시작: krill new <이름> --flow shipit -m "목표"  ({goal} 자리에 들어감)
# [flows.shipit.1]
# m = "구현해줘: {goal}"
# [flows.shipit.2]
# agent = "codex"                    # 생략 시 default_agent
# m = "직전 스테이지의 변경을 리뷰하고 문제를 직접 고쳐줘"

# 듀엣 기본값: krill duet <이름> -m "작업" (worker+reviewer 턴제 핑퐁)
# [duet]
# reviewer = "codex"                 # 기본 리뷰어 (생략 시 default_agent)
# gate = "cargo test"                # LGTM 뒤에 도는 객관 게이트 ([repos.*] gate가 우선)
# max_rounds = 2                     # 재작업 왕복 캡
# compact_every = 8                  # plan 순회에서 N개 작업마다 worker 컨텍스트 압축 (0 = 끔)
"#;

pub const DEFAULT_CONFIG_EN: &str = r#"# krill config
# Any CLI with stdin/stdout can be registered as an agent.
# {prompt} is replaced with the (shell-quoted) message from `krill new -m "..."`.

# lang = "en"             # message language: "ko" | "en" (default: auto-detect from $LANG)

default_agent = "claude"

[agents.claude]
cmd = "claude {prompt}"
hooks = "claude-code"     # auto-inject NeedsYou/Done status hooks
# compact = "/compact"    # context-compaction command sent periodically on plan walks

[agents.codex]
cmd = "codex {prompt}"
# hooks = "external"      # an agent that fires `krill hook` itself (e.g. via its
                          # own notify bridge) can silence the no-hooks warning
                          # with this; only "claude-code" triggers injection.

[agents.gemini]
cmd = "gemini {prompt}"

[agents.shell]
cmd = ""                  # empty cmd = plain shell (for testing)

# [repos.myapp]
# path = "~/work/myapp"
# base = "main"

# [notify]
# ntfy_topic = "krill-yours-x8f2"   # phone push on needs-you/done (ntfy.sh)

# flows: when a stage goes Done, the next stage takes over (relay chain).
# start with: krill new <name> --flow shipit -m "goal"  (fills {goal})
# [flows.shipit.1]
# m = "implement: {goal}"
# [flows.shipit.2]
# agent = "codex"                   # omitted = default_agent
# m = "review the previous stage's changes and fix any issues yourself"

# duet defaults: krill duet <name> -m "task" (worker+reviewer ping-pong)
# [duet]
# reviewer = "codex"                # default reviewer (omitted = default_agent)
# gate = "cargo test"               # objective gate after LGTM ([repos.*] gate wins)
# max_rounds = 2                    # rework round cap
# compact_every = 8                 # compact the worker's context every N plan tasks (0 = off)
"#;

/// The default config template in the current language. Both variants
/// parse to the same semantic Config; only comments differ.
pub fn default_config() -> &'static str {
    match crate::i18n::lang() {
        Lang::Ko => DEFAULT_CONFIG_KO,
        Lang::En => DEFAULT_CONFIG_EN,
    }
}

impl Config {
    /// Load the config file, or fall back to built-in defaults if it
    /// doesn't exist yet (`krill init` writes it to disk).
    pub fn load() -> Result<Config> {
        let path = crate::config_path()?;
        if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| msg::config_read_failed(&path.display().to_string()))?;
            parse(&raw).with_context(|| msg::config_parse_failed(&path.display().to_string()))
        } else {
            parse(default_config()).context(msg::builtin_config_broken())
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
        std::fs::write(&path, default_config())?;
        Ok((path, true))
    }

    /// Pick an agent: explicit flag > default_agent > sole entry.
    pub fn resolve_agent(&self, flag: Option<&str>) -> Result<(String, AgentCfg)> {
        let name = match flag {
            Some(n) => n.to_string(),
            None => match &self.default_agent {
                Some(d) => d.clone(),
                None if self.agents.len() == 1 => self.agents.keys().next().unwrap().clone(),
                None => bail!(msg::agent_required(&self.agent_names())),
            },
        };
        match self.agents.get(&name) {
            Some(cfg) => Ok((name, cfg.clone())),
            None => bail!(msg::agent_unknown(&name, &self.agent_names())),
        }
    }

    fn agent_names(&self) -> String {
        if self.agents.is_empty() {
            msg::agent_none_registered()
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
    // flow name → stage number → stage; folded into config.flows at the end.
    let mut flow_stages: BTreeMap<String, BTreeMap<usize, FlowStage>> = BTreeMap::new();

    for (idx, line) in raw.lines().enumerate() {
        let lineno = idx + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            let Some(name) = rest.strip_suffix(']') else {
                bail!(msg::cfg_section_unterminated(lineno));
            };
            section = name.trim().to_string();
            continue;
        }
        let Some((key, rawval)) = line.split_once('=') else {
            bail!(msg::cfg_not_key_value(lineno, line));
        };
        let key = key.trim();
        let value = parse_value(rawval.trim())
            .with_context(|| msg::cfg_value_parse_failed(lineno))?;

        match section.as_str() {
            "" => {
                match key {
                    "default_agent" => config.default_agent = Some(value),
                    "lang" => config.lang = Some(value),
                    _ => {} // unknown top-level keys are ignored (forward compat)
                }
            }
            s if s.starts_with("agents.") => {
                let name = s.trim_start_matches("agents.").to_string();
                let entry = config.agents.entry(name).or_default();
                match key {
                    "cmd" => entry.cmd = value,
                    "hooks" => entry.hooks = Some(value),
                    "compact" if !value.is_empty() => entry.compact = Some(value),
                    _ => {}
                }
            }
            s if s.starts_with("repos.") => {
                let name = s.trim_start_matches("repos.").to_string();
                let entry = config.repos.entry(name).or_insert(RepoCfg {
                    path: PathBuf::new(),
                    base: "main".into(),
                    gate: None,
                });
                match key {
                    "path" => entry.path = PathBuf::from(value),
                    "base" => entry.base = value,
                    "gate" if !value.is_empty() => entry.gate = Some(value),
                    _ => {}
                }
            }
            "duet" => match key {
                "reviewer" if !value.is_empty() => config.duet.reviewer = Some(value),
                "gate" if !value.is_empty() => config.duet.gate = Some(value),
                "max_rounds" => {
                    config.duet.max_rounds = Some(
                        value.parse().with_context(|| msg::cfg_value_parse_failed(lineno))?,
                    )
                }
                "compact_every" => {
                    config.duet.compact_every = Some(
                        value.parse().with_context(|| msg::cfg_value_parse_failed(lineno))?,
                    )
                }
                _ => {}
            },
            s if s.starts_with("flows.") => {
                // `flows.<name>.<n>`; a non-numeric tail (future keys like
                // `[flows.x.gate]`) is ignored for forward compat.
                let rest = s.trim_start_matches("flows.");
                if let Some((flow, n)) = rest.rsplit_once('.') {
                    if let Ok(n) = n.parse::<usize>() {
                        let stage = flow_stages
                            .entry(flow.to_string())
                            .or_default()
                            .entry(n)
                            .or_default();
                        match key {
                            "agent" => stage.agent = Some(value),
                            "m" => stage.m = Some(value),
                            _ => {}
                        }
                    }
                }
            }
            "notify" => {
                if key == "ntfy_topic" && !value.is_empty() {
                    config.ntfy_topic = Some(value);
                }
            }
            "serve" => match key {
                "port" => {
                    config.serve.port = value
                        .parse()
                        .with_context(|| msg::cfg_value_parse_failed(lineno))?
                }
                "bind" => config.serve.bind = value,
                "token" if !value.is_empty() => config.serve.token = Some(value),
                _ => {}
            },
            _ => {} // unknown sections ignored (forward compat: [notify]…)
        }
    }

    for (name, rc) in &config.repos {
        if rc.path.as_os_str().is_empty() {
            bail!(msg::cfg_repo_missing_path(name));
        }
    }
    // Stage numbers must run 1..=N with no gaps — a silently reordered
    // chain would be worse than an error.
    for (flow, stages) in flow_stages {
        let n = stages.len();
        if !(1..=n).all(|i| stages.contains_key(&i)) {
            bail!(msg::cfg_flow_stages_not_contiguous(&flow));
        }
        config.flows.insert(flow, stages.into_values().collect());
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
                        None => bail!(msg::cfg_trailing_backslash()),
                    },
                    _ => out.push(c),
                }
            }
            bail!(msg::cfg_unclosed_dquote())
        }
        Some('\'') => {
            let rest: String = chars.collect();
            match rest.split_once('\'') {
                Some((inner, _)) => Ok(inner.to_string()),
                None => bail!(msg::cfg_unclosed_squote()),
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
    fn parses_both_default_templates_identically() {
        for tpl in [DEFAULT_CONFIG_KO, DEFAULT_CONFIG_EN] {
            let c = parse(tpl).unwrap();
            assert_eq!(c.default_agent.as_deref(), Some("claude"));
            assert_eq!(c.agents["claude"].cmd, "claude {prompt}");
            assert_eq!(c.agents["claude"].hooks.as_deref(), Some("claude-code"));
            assert_eq!(c.agents["shell"].cmd, "");
            assert!(c.agents.contains_key("codex") && c.agents.contains_key("gemini"));
            assert!(c.repos.is_empty());
            assert!(c.lang.is_none()); // commented out
        }
    }

    #[test]
    fn parses_sections_keys_and_value_styles() {
        let c = parse(
            r##"
lang = "ko"
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
        assert_eq!(c.lang.as_deref(), Some("ko"));
        assert_eq!(c.default_agent.as_deref(), Some("a"));
        assert_eq!(c.agents["a"].hooks.as_deref(), Some("claude-code"));
        assert_eq!(c.agents["b"].cmd, r#"literal "quoted""#);
        assert_eq!(c.repos["web"].path, PathBuf::from("~/work/web"));
        assert_eq!(c.repos["web"].base, "develop");
        assert_eq!(c.repos.len(), 1); // [serve] is not a repo
        assert_eq!(c.serve.port, 7777); // [serve] port not set in this fixture
    }

    #[test]
    fn parses_serve_section_with_defaults() {
        let d = parse("").unwrap().serve;
        assert_eq!(d.port, 7777);
        assert_eq!(d.bind, "127.0.0.1");
        assert!(d.token.is_none());

        let s = parse("[serve]\nport = 8080\nbind = \"0.0.0.0\"\ntoken = \"s3cret\"\n")
            .unwrap()
            .serve;
        assert_eq!(s.port, 8080);
        assert_eq!(s.bind, "0.0.0.0");
        assert_eq!(s.token.as_deref(), Some("s3cret"));

        assert!(parse("[serve]\nport = not-a-number\n").is_err());
        assert!(parse("[serve]\ntoken = \"\"\n").unwrap().serve.token.is_none());
    }

    #[test]
    fn parses_notify_section() {
        assert!(parse("").unwrap().ntfy_topic.is_none());
        let c = parse("[notify]\nntfy_topic = \"krill-me-x8f2\"\n").unwrap();
        assert_eq!(c.ntfy_topic.as_deref(), Some("krill-me-x8f2"));
        assert!(parse("[notify]\nntfy_topic = \"\"\n").unwrap().ntfy_topic.is_none());
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
    fn parses_duet_section_and_repo_gate() {
        let c = parse(
            "[duet]\nreviewer = \"codex\"\ngate = \"cargo test\"\nmax_rounds = 3\n\
             compact_every = 8\n\
             [repos.web]\npath = \"/w\"\ngate = \"npm test\"\n\
             [agents.claude]\ncmd = \"claude {prompt}\"\ncompact = \"/compact\"\n",
        )
        .unwrap();
        assert_eq!(c.duet.reviewer.as_deref(), Some("codex"));
        assert_eq!(c.duet.gate.as_deref(), Some("cargo test"));
        assert_eq!(c.duet.max_rounds, Some(3));
        assert_eq!(c.duet.compact_every, Some(8));
        assert_eq!(c.agents["claude"].compact.as_deref(), Some("/compact"));
        assert_eq!(c.repos["web"].gate.as_deref(), Some("npm test"));

        assert!(parse("[duet]\nmax_rounds = not-a-number\n").is_err());
        let d = parse("").unwrap().duet;
        assert!(d.reviewer.is_none() && d.gate.is_none() && d.max_rounds.is_none());
        assert!(d.compact_every.is_none());
        assert!(parse("[agents.a]\ncompact = \"\"\n").unwrap().agents["a"].compact.is_none());
        assert!(parse("[duet]\ngate = \"\"\n").unwrap().duet.gate.is_none());
    }

    #[test]
    fn parses_flow_stages_in_order() {
        let c = parse(
            r#"
[flows.shipit.2]
agent = "codex"
m = "review: {goal}"

[flows.shipit.1]
m = "implement: {goal}"

[flows.other.1]
agent = "claude"

[flows.shipit.gate]
cmd = "ignored for now (forward compat)"
"#,
        )
        .unwrap();
        let ship = &c.flows["shipit"];
        assert_eq!(ship.len(), 2); // out-of-file-order sections still sort 1,2
        assert_eq!(ship[0].agent, None);
        assert_eq!(ship[0].m.as_deref(), Some("implement: {goal}"));
        assert_eq!(ship[1].agent.as_deref(), Some("codex"));
        assert_eq!(c.flows["other"][0].agent.as_deref(), Some("claude"));
        assert!(parse("").unwrap().flows.is_empty());
    }

    #[test]
    fn rejects_flow_stage_gaps() {
        assert!(parse("[flows.x.1]\nm = \"a\"\n[flows.x.3]\nm = \"b\"\n").is_err());
        assert!(parse("[flows.x.0]\nm = \"a\"\n").is_err()); // must start at 1
        assert!(parse("[flows.x.2]\nm = \"a\"\n").is_err());
    }

    #[test]
    fn stage_prompt_rules() {
        assert_eq!(stage_prompt(Some("do: {goal}!"), "fix"), Some("do: fix!".into()));
        assert_eq!(stage_prompt(Some("static"), "fix"), Some("static".into()));
        assert_eq!(stage_prompt(Some("g={goal}"), ""), Some("g=".into()));
        assert_eq!(stage_prompt(None, "fix"), Some("fix".into()));
        assert_eq!(stage_prompt(None, ""), None);
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
