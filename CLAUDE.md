# krill

AI 코딩 에이전트를 git worktree + tmux로 격리해 병렬 실행하는 초경량 오케스트레이터(ADE).
Orca/Xirp의 경량 대안. **전체 설계는 docs/DESIGN.md — 작업 전에 반드시 먼저 읽을 것.**

## 현재 상태

M0 완료: `new / ls / attach / diff / rm` + `--from` 릴레이 핸드오프. 순수 std로 외부
크레이트 0개인데 이것은 의도된 설계다(릴리스 바이너리 515KB). 실기기 검증 완료.

다음 마일스톤 순서: M1 TUI(ratatui) → M2 `krill serve`(웹 UI + Tailscale 원격) →
M3 Claude Code 훅 연동 + ntfy 푸시 + merge/pr → M4 릴리스 CI/brew → M5 듀엣(턴제
교차모델 리뷰, docs/DESIGN.md §12).

## 설계 원칙 (요약 — 상세는 DESIGN.md §4)

1. 어려운 문제(세션 지속성·격리·원격·암호화)는 tmux/git/Tailscale/SSH에 위임한다.
   krill이 직접 푸는 문제는 "상태 파악"과 "UI" 둘뿐이다.
2. 에이전트는 블랙박스: 특정 벤더를 코드에 하드코딩하지 않는다. 어댑터는 config
   데이터(`[agents.*]`)로 유지해 릴리스 없이 따라간다.
3. 데몬을 강요하지 않는다: TUI만 쓰면 상주 프로세스 0. 상태는 tmux + 메타파일에서
   매번 재구성한다. serve가 죽어도 에이전트 세션은 산다.
4. 의존성은 마일스톤이 요구할 때만 추가한다 (M1: ratatui/crossterm/clap 허용,
   M2: tokio/axum/rust-embed 허용). "가벼움"이 이 프로젝트의 정체성이다.

## 구조

- `crates/krill-core` — 라이브러리: config, git(worktree), tmux, session, kv, error.
  UI를 모른다. TUI/웹은 이 위의 얇은 뷰여야 한다.
- `crates/krill` — 바이너리: main(디스패치), args(플래그 파서), commands(구현).
- 데이터: `~/.local/share/krill/{sessions,logs,worktrees}`, 설정: `~/.config/krill/config.toml`.

## 빌드와 테스트

`cargo build` → `target/debug/krill`. 수동 검증 시나리오: 더미 git 리포에서
`-a shell` 에이전트로 new → (tmux send-keys로 작업 시뮬레이션) → ls → diff →
`--from` 릴레이 → rm 후 tmux/worktree/브랜치 잔여물 0 확인. 자동 테스트는 TODO
(core의 kv/config 파서부터 유닛 테스트 붙이는 게 좋은 시작점).

## 주의할 함정

- tmux 타깃 문법: 세션 명령(has/kill/attach)은 `=name`, pane 명령(send-keys,
  pipe-pane, display-message)은 `=name:`(끝 콜론 필수). 실제로 밟았던 버그다.
- `krill new` 실패 시 롤백에서 tmux kill + worktree remove + branch delete를
  모두 해야 잔여물이 안 남는다 (commands.rs의 spawn 클로저 패턴 유지).
- 에러 메시지는 한국어, 코드 주석은 한/영 혼용 OK.
