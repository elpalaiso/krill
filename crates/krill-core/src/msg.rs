//! ko/en message catalog for krill-core. See i18n.rs for the rule:
//! user-facing strings live here, never inline in the code.

crate::messages! {
    home_missing() => {
        en: "HOME environment variable is not set",
        ko: "HOME 환경변수가 없습니다",
    }
    config_read_failed(path: &str) => {
        en: "failed to read config file: {path}",
        ko: "설정 파일을 읽을 수 없습니다: {path}",
    }
    config_parse_failed(path: &str) => {
        en: "failed to parse config file: {path}",
        ko: "설정 파일 파싱 실패: {path}",
    }
    builtin_config_broken() => {
        en: "failed to parse the built-in default config (bug)",
        ko: "내장 기본 설정 파싱 실패(버그)",
    }
    agent_required(agents: &str) => {
        en: "specify an agent (-a). registered agents: {agents}",
        ko: "에이전트를 지정하세요 (-a). 등록된 에이전트: {agents}",
    }
    agent_unknown(name: &str, agents: &str) => {
        en: "agent '{name}' is not in the config. registered agents: {agents}",
        ko: "'{name}' 에이전트가 설정에 없습니다. 등록된 에이전트: {agents}",
    }
    agent_none_registered() => {
        en: "(none — run `krill init`, then edit config.toml)",
        ko: "(없음 — `krill init` 후 config.toml을 편집하세요)",
    }
    cfg_section_unterminated(lineno: usize) => {
        en: "line {lineno}: section header does not end with ]",
        ko: "{lineno}행: 섹션 헤더가 ]로 끝나지 않습니다",
    }
    cfg_not_key_value(lineno: usize, line: &str) => {
        en: "line {lineno}: not in `key = value` form: {line}",
        ko: "{lineno}행: `key = value` 형식이 아닙니다: {line}",
    }
    cfg_value_parse_failed(lineno: usize) => {
        en: "line {lineno}: failed to parse value",
        ko: "{lineno}행: 값 파싱 실패",
    }
    cfg_repo_missing_path(name: &str) => {
        en: "[repos.{name}] has no path",
        ko: "[repos.{name}]에 path가 없습니다",
    }
    cfg_trailing_backslash() => {
        en: "string ends on a bare \\",
        ko: "문자열이 \\ 로 끝났습니다",
    }
    cfg_unclosed_dquote() => {
        en: "missing closing double quote (\")",
        ko: "닫는 따옴표(\")가 없습니다",
    }
    cfg_unclosed_squote() => {
        en: "missing closing single quote (')",
        ko: "닫는 따옴표(')가 없습니다",
    }
    git_not_found() => {
        en: "failed to run git (is it installed?)",
        ko: "git을 실행할 수 없습니다 (설치돼 있나요?)",
    }
    git_cmd_failed(cmd: &str, stderr: &str) => {
        en: "git {cmd} failed: {stderr}",
        ko: "git {cmd} 실패: {stderr}",
    }
    repo_unknown(name: &str, repos: &str) => {
        en: "repo '{name}' is not in the config. registered repos: {repos}",
        ko: "'{name}' 리포가 설정에 없습니다. 등록된 리포: {repos}",
    }
    repo_none_registered() => {
        en: "(none)",
        ko: "(없음)",
    }
    not_in_repo() => {
        en: "not inside a git repo. run from within one, or pick with -r <name>.",
        ko: "git 리포 안이 아닙니다. 리포 안에서 실행하거나 -r <이름>으로 지정하세요.",
    }
    repo_path_not_git(path: &str) => {
        en: "repo path is not a git repository: {path}",
        ko: "리포 경로가 git 저장소가 아닙니다: {path}",
    }
    tmux_not_found() => {
        en: "failed to run tmux (is it installed?)",
        ko: "tmux를 실행할 수 없습니다 (설치돼 있나요?)",
    }
    tmux_cmd_failed(cmd: &str, stderr: &str) => {
        en: "tmux {cmd} failed: {stderr}",
        ko: "tmux {cmd} 실패: {stderr}",
    }
    tmux_attach_failed(err: &str) => {
        en: "tmux attach failed: {err}",
        ko: "tmux attach 실패: {err}",
    }
    meta_field_missing(field: &str) => {
        en: "session meta is missing the '{field}' field",
        ko: "세션 메타에 '{field}' 필드가 없습니다",
    }
    meta_created_parse_failed() => {
        en: "failed to parse created_unix",
        ko: "created_unix 파싱 실패",
    }
    meta_save_failed(path: &str) => {
        en: "failed to save session meta: {path}",
        ko: "세션 메타 저장 실패: {path}",
    }
    meta_corrupt_ignored(path: &str, err: &str) => {
        en: "warning: ignoring corrupt session meta {path} ({err})",
        ko: "경고: 세션 메타 손상 무시 {path} ({err})",
    }
    session_not_found(name: &str) => {
        en: "session '{name}' not found. check with `krill ls`.",
        ko: "'{name}' 세션이 없습니다. `krill ls`로 확인하세요.",
    }
    session_ambiguous(name: &str, repos: &str) => {
        en: "session '{name}' exists in multiple repos ({repos}). pick one with -r <repo>.",
        ko: "'{name}' 세션이 여러 리포에 있습니다 ({repos}). -r <리포>로 지정하세요.",
    }
}

#[cfg(test)]
mod tests {
    use super::fixed;
    use crate::i18n::Lang;

    #[test]
    fn both_languages_render_and_interpolate() {
        let en = fixed::session_not_found(Lang::En, "fix-login");
        let ko = fixed::session_not_found(Lang::Ko, "fix-login");
        assert!(en.contains("fix-login") && en.contains("not found"), "{en}");
        assert!(ko.contains("fix-login") && ko.contains("세션"), "{ko}");
        assert_ne!(en, ko);

        assert!(fixed::cfg_not_key_value(Lang::En, 3, "x").contains("line 3"));
        assert!(fixed::cfg_not_key_value(Lang::Ko, 3, "x").contains("3행"));
        assert!(fixed::git_cmd_failed(Lang::En, "status", "boom").contains("git status failed: boom"));
    }
}
