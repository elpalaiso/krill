//! ko/en message catalog for the CLI (help text + command output).
//! Core has its own catalog in krill-core/src/msg.rs; the rule for both:
//! no user-facing string outside a catalog.

use krill_core::i18n::{lang, Lang};
use krill_core::messages;

pub fn help() -> &'static str {
    match lang() {
        Lang::Ko => HELP_KO,
        Lang::En => HELP_EN,
    }
}

const HELP_KO: &str = "\
krill — tiny orchestrator for AI coding agents (tmux + git worktrees)

에이전트를 git worktree로 격리해 병렬로 돌립니다. 세션의 실체는 tmux라서
krill이 꺼져 있어도 에이전트는 계속 일합니다.

사용법:
  krill                               TUI 대시보드 (터미널이 아니면 = krill ls)
  krill init                          설정 파일 생성 (~/.config/krill/config.toml)
  krill new <이름> [옵션]             새 세션: 브랜치 + worktree + tmux + 에이전트
      -a, --agent <이름>              에이전트 (config의 [agents.*])
      -r, --repo <이름>               대상 리포 (생략 시 현재 디렉토리의 리포)
      -m, --message <지시문>          에이전트에게 넘길 첫 지시문
          --from <세션>               다른 세션의 브랜치에서 시작 (릴레이 핸드오프)
  krill attach <이름> [-r <리포>]     tmux 접속 (분리: Ctrl-b d)
  krill diff <이름> [--stat]          base 대비 변경 내용 (커밋 전 변경 포함)
  krill rm <이름> [-f|--force]        세션 · worktree · 브랜치 정리
  krill serve [-b <주소>] [-p <포트>]  웹 UI (기본 127.0.0.1:7777, config [serve])
  krill --help | --version

예시:
  krill new fix-login -m \"로그인 버그 고쳐줘\"
  krill new review-login -a codex --from fix-login -m \"이 브랜치 리뷰하고 수정해\"

언어: KRILL_LANG=ko|en 또는 config.toml의 lang (기본: $LANG 자동 감지)
";

const HELP_EN: &str = "\
krill — tiny orchestrator for AI coding agents (tmux + git worktrees)

Runs each agent in parallel, isolated in its own git worktree. Sessions
live in tmux, so agents keep working even while krill is closed.

usage:
  krill                               TUI dashboard (non-TTY: = krill ls)
  krill init                          write the config file (~/.config/krill/config.toml)
  krill new <name> [options]          new session: branch + worktree + tmux + agent
      -a, --agent <name>              agent ([agents.*] in the config)
      -r, --repo <name>               target repo (default: the repo at cwd)
      -m, --message <prompt>          first instruction passed to the agent
          --from <session>            branch off another session's work (relay handoff)
  krill attach <name> [-r <repo>]     attach to the tmux session (detach: Ctrl-b d)
  krill diff <name> [--stat]          changes vs base (uncommitted included)
  krill rm <name> [-f|--force]        remove session · worktree · branch
  krill serve [-b <addr>] [-p <port>]  web UI (default 127.0.0.1:7777, config [serve])
  krill --help | --version

examples:
  krill new fix-login -m \"fix the login bug\"
  krill new review-login -a codex --from fix-login -m \"review this branch and fix it\"

language: KRILL_LANG=ko|en or lang in config.toml (default: auto-detect from $LANG)
";

messages! {
    name_required(cmd: &str) => {
        en: "session name required: krill {cmd} <name>",
        ko: "세션 이름이 필요합니다: krill {cmd} <이름>",
    }
    unknown_command(cmd: &str) => {
        en: "unknown command: {cmd}",
        ko: "알 수 없는 명령: {cmd}",
    }
    opt_needs_value(opt: &str) => {
        en: "option {opt} requires a value",
        ko: "{opt} 옵션에 값이 필요합니다",
    }
    unknown_option(opt: &str) => {
        en: "unknown option: {opt} (see: krill --help)",
        ko: "알 수 없는 옵션: {opt} (도움말: krill --help)",
    }
    init_created(path: &str) => {
        en: "config file created: {path}",
        ko: "설정 파일 생성: {path}",
    }
    init_hint() => {
        en: "edit the agents and repos, then start with `krill new <name>`.",
        ko: "에이전트와 리포를 편집한 뒤 `krill new <이름>`으로 시작하세요.",
    }
    init_exists(path: &str) => {
        en: "config file already exists: {path}",
        ko: "설정 파일이 이미 있습니다: {path}",
    }
    invalid_session_name(name: &str) => {
        en: "session names are up to 64 chars of letters, digits, - and _: '{name}'",
        ko: "세션 이름은 영숫자/대시/언더스코어 64자 이내여야 합니다: '{name}'",
    }
    session_exists(name: &str, repo: &str) => {
        en: "session '{name}' already exists (repo: {repo}). pick another name or run `krill rm {name}` first.",
        ko: "'{name}' 세션이 이미 있습니다 (repo: {repo}). 다른 이름을 쓰거나 먼저 `krill rm {name}` 하세요.",
    }
    from_other_repo(repo: &str) => {
        en: "the --from session lives in a different repo: {repo}",
        ko: "--from 세션이 다른 리포에 있습니다: {repo}",
    }
    worktree_exists(path: &str) => {
        en: "worktree path already exists: {path}",
        ko: "worktree 경로가 이미 존재합니다: {path}",
    }
    worktree_create_failed(base: &str) => {
        en: "failed to create worktree (base: {base})",
        ko: "worktree 생성 실패 (base: {base})",
    }
    tmux_name_taken(name: &str) => {
        en: "tmux session name already in use: {name}",
        ko: "tmux 세션 이름이 이미 사용 중입니다: {name}",
    }
    session_started() => {
        en: "session started",
        ko: "세션 시작",
    }
    shell_only() => {
        en: " (shell only)",
        ko: " (셸만)",
    }
    attach_hint(cmd: &str) => {
        en: "attach: {cmd}   (detach: Ctrl-b d)",
        ko: "접속: {cmd}   (분리: Ctrl-b d)",
    }
    ls_empty() => {
        en: "no sessions.",
        ko: "세션이 없습니다.",
    }
    ls_hint() => {
        en: "get started: krill new <name> -m \"prompt\"   (setup: krill init)",
        ko: "시작하기: krill new <이름> -m \"지시문\"   (설정: krill init)",
    }
    attach_dead(name: &str) => {
        en: "tmux for session '{name}' is dead. clean up with `krill rm {name}` and create it again.",
        ko: "'{name}' 세션의 tmux가 죽어 있습니다. `krill rm {name}`로 정리 후 다시 만드세요.",
    }
    worktree_missing(path: &str) => {
        en: "worktree does not exist: {path}",
        ko: "worktree가 없습니다: {path}",
    }
    git_exec_failed() => {
        en: "failed to run git",
        ko: "git 실행 실패",
    }
    git_diff_exit(status: &str) => {
        en: "git diff exit status: {status}",
        ko: "git diff 종료 코드: {status}",
    }
    rm_confirm(name: &str, branch: &str) => {
        en: "deleting session '{name}', its worktree and branch {branch}. continue? [y/N] ",
        ko: "'{name}' 세션과 worktree, 브랜치 {branch}을(를) 삭제합니다. 계속? [y/N] ",
    }
    rm_cancelled() => {
        en: "cancelled.",
        ko: "취소했습니다.",
    }
    stdin_read_failed() => {
        en: "failed to read input",
        ko: "입력 읽기 실패",
    }
    rm_worktree_failed(err: &str, name: &str) => {
        en: "failed to remove the worktree — it probably has uncommitted changes.\n  {err}\nforce it: krill rm {name} --force",
        ko: "worktree 제거 실패 — 커밋 안 된 변경이 있는 것 같습니다.\n  {err}\n강제 삭제: krill rm {name} --force",
    }
    rm_branch_kept(err: &str) => {
        en: "keeping the branch (not merged): {err}",
        ko: "브랜치는 남겨둡니다 (머지되지 않음): {err}",
    }
    rm_branch_hint(branch: &str) => {
        en: "to delete it too: krill rm --force, or git branch -D {branch}",
        ko: "브랜치까지 지우려면: krill rm --force 또는 git branch -D {branch}",
    }
    rm_done(name: &str) => {
        en: "cleaned up: {name}",
        ko: "정리 완료: {name}",
    }
    tui_hint() => {
        en: "Enter attach · n new · d diff · x rm · / filter · ? help · q quit",
        ko: "Enter 접속 · n 새 세션 · d diff · x 정리 · / 필터 · ? 도움말 · q 종료",
    }
    tui_help_title() => {
        en: "keys",
        ko: "키",
    }
    tui_help_body() => {
        en: "j/k, ↑/↓       move selection\nEnter          attach (detach: Ctrl-b d)\nn              new session (name → agent → prompt)\nd / D          diff vs base (full / --stat)\nx              remove session (modal: y/f/N)\nJ/K, PgUp/PgDn scroll preview\n/              filter by name (Esc clears)\nr              refresh now\n?              toggle this help\nq, Ctrl-c      quit (sessions keep running in tmux)",
        ko: "j/k, ↑/↓       세션 선택 이동\nEnter          접속 (분리: Ctrl-b d)\nn              새 세션 (이름 → 에이전트 → 지시문)\nd / D          base 대비 diff (전체 / --stat)\nx              세션 정리 (모달: y/f/N)\nJ/K, PgUp/PgDn 미리보기 스크롤\n/              이름 필터 (Esc 해제)\nr              즉시 새로고침\n?              이 도움말 열기/닫기\nq, Ctrl-c      종료 (세션은 tmux에서 계속 실행)",
    }
    tui_last_output(age: &str) => {
        en: "last output {age} ago",
        ko: "마지막 출력 {age} 전",
    }
    tui_no_output() => {
        en: "(no output yet)",
        ko: "(아직 출력이 없습니다)",
    }
    tui_rm_title() => {
        en: "remove session",
        ko: "세션 정리",
    }
    tui_rm_body(name: &str, branch: &str) => {
        en: "deleting session '{name}', its worktree and branch {branch}.",
        ko: "'{name}' 세션과 worktree, 브랜치 {branch}을(를) 삭제합니다.",
    }
    tui_rm_dirty(diff: &str) => {
        en: "uncommitted changes: {diff}",
        ko: "커밋 안 된 변경: {diff}",
    }
    tui_rm_keys() => {
        en: "[y] delete   [f] force   [N] cancel",
        ko: "[y] 삭제   [f] 강제 삭제   [N] 취소",
    }
    tui_new_name() => {
        en: "new▸ name: ",
        ko: "new▸ 이름: ",
    }
    tui_new_agent() => {
        en: "new▸ agent: ",
        ko: "new▸ 에이전트: ",
    }
    tui_new_message() => {
        en: "new▸ prompt: ",
        ko: "new▸ 지시문: ",
    }
    tui_new_esc() => {
        en: "(Enter next · Esc cancel)",
        ko: "(Enter 다음 · Esc 취소)",
    }
    tui_new_tab() => {
        en: "(Tab cycles · Enter next · Esc cancel)",
        ko: "(Tab 전환 · Enter 다음 · Esc 취소)",
    }
    tui_new_enter() => {
        en: "(Enter creates · empty = shell only · Esc cancel)",
        ko: "(Enter 생성 · 빈 값 = 셸만 · Esc 취소)",
    }
    tui_filter_hint() => {
        en: "(Enter keep · Esc clear)",
        ko: "(Enter 유지 · Esc 해제)",
    }
    serve_listening(addr: &str) => {
        en: "web UI listening on http://{addr}  (stop: Ctrl-C)",
        ko: "웹 UI: http://{addr}  (중지: Ctrl-C)",
    }
    serve_token_required() => {
        en: "refusing to bind a non-loopback address without a token — set token under [serve] in config.toml (design doc §7)",
        ko: "토큰 없이는 loopback이 아닌 주소에 바인드할 수 없습니다 — config.toml의 [serve]에 token을 설정하세요 (설계서 §7)",
    }
    serve_bad_bind(addr: &str) => {
        en: "cannot parse bind address: {addr}",
        ko: "바인드 주소를 해석할 수 없습니다: {addr}",
    }
    serve_bad_port(port: &str) => {
        en: "cannot parse port: {port}",
        ko: "포트를 해석할 수 없습니다: {port}",
    }
    serve_bind_failed(addr: &str) => {
        en: "failed to bind {addr}",
        ko: "{addr} 바인드 실패",
    }
    serve_start_failed() => {
        en: "failed to start the web server",
        ko: "웹 서버 시작 실패",
    }
}

#[cfg(test)]
mod tests {
    use super::fixed;
    use krill_core::i18n::Lang;

    #[test]
    fn catalog_renders_in_both_languages() {
        assert!(fixed::session_exists(Lang::En, "x", "web").contains("already exists"));
        assert!(fixed::session_exists(Lang::Ko, "x", "web").contains("이미 있습니다"));
        assert!(fixed::name_required(Lang::En, "new").contains("krill new <name>"));
        assert!(fixed::name_required(Lang::Ko, "diff").contains("krill diff <이름>"));
        assert!(fixed::rm_confirm(Lang::En, "s", "krill/s").ends_with("[y/N] "));
        assert!(fixed::rm_confirm(Lang::Ko, "s", "krill/s").ends_with("[y/N] "));
    }

    #[test]
    fn help_exists_in_both_languages() {
        assert!(super::HELP_KO.contains("krill new") && super::HELP_KO.contains("사용법"));
        assert!(super::HELP_EN.contains("krill new") && super::HELP_EN.contains("usage"));
        assert_ne!(super::HELP_KO, super::HELP_EN);
    }
}
