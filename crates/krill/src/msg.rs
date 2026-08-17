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
          --flow <flow>               [flows.*] 체인 시작 — 스테이지가 Done이 되면
                                      다음 스테이지를 자동 릴레이 (-m = 목표, {goal})
  krill duet <이름> -m \"작업\"         worker+reviewer 턴제 핑퐁 (한 worktree)
      -a <에이전트> · --reviewer <에이전트> · --gate <명령> · --max-rounds <N>
  krill attach <이름> [-r <리포>]     tmux 접속 (분리: Ctrl-b d)
  krill diff <이름> [--stat]          base 대비 변경 내용 (커밋 전 변경 포함)
  krill merge <이름> [--squash]       base에 머지 후 세션 정리 (base 체크아웃 상태에서)
  krill pr <이름>                     브랜치 푸시 + gh pr create
  krill rm <이름> [-f|--force]        세션 · worktree · 브랜치 정리
  krill serve [-b <주소>|tailscale] [-p <포트>]  웹 UI (기본 127.0.0.1:7777)
  krill hook <상태> -i <세션ID>       (내부용) 에이전트 훅이 상태를 보고
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
          --flow <flow>               start a [flows.*] chain — each Done stage
                                      auto-relays the next (-m = goal, {goal})
  krill duet <name> -m \"task\"         worker+reviewer turn-based ping-pong (one worktree)
      -a <agent> · --reviewer <agent> · --gate <cmd> · --max-rounds <N>
  krill attach <name> [-r <repo>]     attach to the tmux session (detach: Ctrl-b d)
  krill diff <name> [--stat]          changes vs base (uncommitted included)
  krill merge <name> [--squash]       merge into base + clean up (run with base checked out)
  krill pr <name>                     push the branch + gh pr create
  krill rm <name> [-f|--force]        remove session · worktree · branch
  krill serve [-b <addr>|tailscale] [-p <port>]  web UI (default 127.0.0.1:7777)
  krill hook <state> -i <session-id>  (internal) agent hooks report state
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
    tailscale_failed() => {
        en: "failed to resolve the tailscale address (is tailscale installed and up?)",
        ko: "tailscale 주소를 확인할 수 없습니다 (tailscale이 설치·실행 중인가요?)",
    }
    hook_usage() => {
        en: "usage: krill hook <needs-you|done> -i <session-id>",
        ko: "사용법: krill hook <needs-you|done> -i <세션ID>",
    }
    hook_settings_exists(path: &str) => {
        en: "keeping the existing hook settings: {path}",
        ko: "기존 훅 설정을 유지합니다: {path}",
    }
    ntfy_needs_you(id: &str) => {
        en: "{id} needs you",
        ko: "{id} 승인 대기 중",
    }
    ntfy_done(id: &str) => {
        en: "{id} done",
        ko: "{id} 작업 완료",
    }
    flow_unknown(name: &str, flows: &str) => {
        en: "flow '{name}' is not in the config. registered flows: {flows}",
        ko: "'{name}' flow가 설정에 없습니다. 등록된 flow: {flows}",
    }
    flow_none_registered() => {
        en: "(none — add [flows.<name>.1] sections to config.toml)",
        ko: "(없음 — config.toml에 [flows.<이름>.1] 섹션을 추가하세요)",
    }
    flow_flag_conflict() => {
        en: "--flow cannot be combined with --from or -a (stages define both)",
        ko: "--flow는 --from, -a와 함께 쓸 수 없습니다 (스테이지가 대신 정합니다)",
    }
    flow_agent_no_hooks(agent: &str, stage: usize) => {
        en: "warning: agent '{agent}' (stage {stage}) has no hook preset — the chain will stall there; add `hooks = \"claude-code\"` or make it the last stage.",
        ko: "경고: '{agent}' 에이전트(스테이지 {stage})에 훅 프리셋이 없어 체인이 거기서 멈춥니다 — `hooks = \"claude-code\"`를 추가하거나 마지막 스테이지로 두세요.",
    }
    ntfy_flow_next(flow: &str, stage: usize, total: usize, name: &str, agent: &str) => {
        en: "flow {flow}: stage {stage}/{total} started — {name} ({agent})",
        ko: "flow {flow}: 스테이지 {stage}/{total} 시작 — {name} ({agent})",
    }
    ntfy_flow_done(flow: &str, name: &str) => {
        en: "flow {flow} finished at {name} — review the result",
        ko: "flow {flow} 완료 ({name}) — 결과를 확인하세요",
    }
    ntfy_flow_spawn_failed(flow: &str, name: &str, err: &str) => {
        en: "flow {flow}: failed to start {name}: {err}",
        ko: "flow {flow}: {name} 시작 실패: {err}",
    }
    duet_goal_required() => {
        en: "duet needs a task: krill duet <name> -m \"task\"",
        ko: "듀엣에는 작업이 필요합니다: krill duet <이름> -m \"작업\"",
    }
    duet_bad_rounds(v: &str) => {
        en: "--max-rounds must be a number ≥ 1: {v}",
        ko: "--max-rounds는 1 이상의 숫자여야 합니다: {v}",
    }
    duet_no_hooks_warn(agent: &str) => {
        en: "warning: agent '{agent}' has no hook preset — the duet referee only moves on hook events; add `hooks = \"claude-code\"`.",
        ko: "경고: '{agent}' 에이전트에 훅 프리셋이 없습니다 — 듀엣 심판은 훅 이벤트로만 움직입니다; `hooks = \"claude-code\"`를 추가하세요.",
    }
    duet_started() => {
        en: "duet started",
        ko: "듀엣 시작",
    }
    duet_no_gate() => {
        en: "(none — LGTM alone completes)",
        ko: "(없음 — LGTM만으로 완료)",
    }
    duet_review_instruction(goal: &str) => {
        en: "You are the reviewer in a krill duet. Goal: {goal}. Review the worktree's current changes vs its base (committed and uncommitted). Do NOT edit code — write only REVIEW.md: first line exactly LGTM or ISSUES, then your findings.",
        ko: "당신은 krill 듀엣의 리뷰어입니다. 목표: {goal}. worktree의 현재 변경(base 대비, 커밋 전 포함)을 리뷰하세요. 코드는 수정하지 말고 REVIEW.md만 작성하세요: 첫 줄은 정확히 LGTM 또는 ISSUES, 이어서 지적 내용.",
    }
    duet_fix_instruction(round: u32, max: u32) => {
        en: "The reviewer left findings in REVIEW.md — address them in the code (round {round}/{max}).",
        ko: "리뷰어가 REVIEW.md에 지적을 남겼습니다 — 코드에 반영하세요 (라운드 {round}/{max}).",
    }
    duet_review_missing() => {
        en: "REVIEW.md was not found. Write REVIEW.md now: first line exactly LGTM or ISSUES, then your findings.",
        ko: "REVIEW.md가 없습니다. 지금 REVIEW.md를 작성하세요: 첫 줄은 정확히 LGTM 또는 ISSUES, 이어서 지적 내용.",
    }
    duet_gate_fix_instruction(round: u32, max: u32) => {
        en: "The gate command failed — see GATE.md and fix the code (round {round}/{max}).",
        ko: "게이트 명령이 실패했습니다 — GATE.md를 보고 코드를 고치세요 (라운드 {round}/{max}).",
    }
    duet_gate_run_failed(gate: &str) => {
        en: "failed to run the gate command: {gate}",
        ko: "게이트 명령을 실행할 수 없습니다: {gate}",
    }
    gate_md_header(gate: &str) => {
        en: "# Gate failed\n\nCommand: `{gate}` (output tail below)",
        ko: "# 게이트 실패\n\n명령: `{gate}` (아래는 출력 끝부분)",
    }
    ntfy_duet_done(name: &str) => {
        en: "duet {name}: review passed — ready to merge (krill merge {name})",
        ko: "듀엣 {name}: 리뷰 통과 — 머지 준비 완료 (krill merge {name})",
    }
    ntfy_duet_stalled(name: &str, rounds: u32) => {
        en: "duet {name} needs you: round cap ({rounds}) reached",
        ko: "듀엣 {name} 확인 필요: 라운드 캡({rounds}) 도달",
    }
    merge_on_reviewer(worker: &str) => {
        en: "this is the reviewer half of a duet — merge via its worker: krill merge {worker}",
        ko: "듀엣의 리뷰어 세션입니다 — worker로 머지하세요: krill merge {worker}",
    }
    merge_dirty(name: &str) => {
        en: "session '{name}' has uncommitted changes — commit them in the session first (krill attach {name}).",
        ko: "'{name}' 세션에 커밋 안 된 변경이 있습니다 — 세션에서 먼저 커밋하세요 (krill attach {name}).",
    }
    merge_not_on_base(base: &str) => {
        en: "the repo is not on the base branch — check out {base} first.",
        ko: "리포가 base 브랜치에 있지 않습니다 — 먼저 {base}를 체크아웃하세요.",
    }
    merge_done(name: &str, base: &str) => {
        en: "merged '{name}' into {base}.",
        ko: "'{name}'을(를) {base}에 머지했습니다.",
    }
    merge_squashed(name: &str, base: &str) => {
        en: "squashed '{name}' into the {base} index — review and commit it.",
        ko: "'{name}'을(를) {base}에 squash 스테이징했습니다 — 확인 후 커밋하세요.",
    }
    pr_pushed(branch: &str) => {
        en: "pushed {branch} — opening a PR with gh…",
        ko: "{branch} 푸시 완료 — gh로 PR을 엽니다…",
    }
    gh_failed() => {
        en: "failed to run gh (is the GitHub CLI installed and authenticated?)",
        ko: "gh를 실행할 수 없습니다 (GitHub CLI가 설치·로그인돼 있나요?)",
    }
    gh_exit(status: &str) => {
        en: "gh pr create exit status: {status}",
        ko: "gh pr create 종료 코드: {status}",
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
