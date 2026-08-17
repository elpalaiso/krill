# krill

AI 코딩 에이전트를 git worktree + tmux로 격리해 병렬 실행하는 초경량 오케스트레이터(ADE).
Orca/Xirp의 경량 대안. **전체 설계는 docs/DESIGN.md — 작업 전에 반드시 먼저 읽을 것.**

## 현재 상태

M0 완료: `new / ls / attach / diff / rm` + `--from` 릴레이 핸드오프. 순수 std로 외부
크레이트 0개인데 이것은 의도된 설계다(릴리스 바이너리 515KB). 실기기 검증 완료.
이후 추가: 코어 유닛 테스트 + 실제 git 통합 테스트(`cargo test`), CLI 메시지
ko/en i18n(`messages!` 카탈로그, 로케일 자동 감지), M1a TUI(읽기 전용 대시보드 —
목록·라이브 미리보기·attach·도움말, ratatui는 바이너리 크레이트에만. 인자 없이
`krill` 실행 시 TTY면 TUI, 아니면 ls) + M1b 액션(n 새 세션 3단계 프롬프트,
d/D diff — TUI 일시중단 후 페이저(LESS=R로 짧은 diff도 유지), x 정리 모달 —
dirty 경고·y/f/N) + M1c 폴리시(/ 이름 필터, 색 미리보기 — capture-pane -e를
ansi.rs의 SGR 파서로 렌더). M1 완료. commands.rs의 create_session/remove_session은
무출력 코어라 CLI와 TUI가 공유한다.

M2 진행: `krill serve` 웹 UI(serve.rs, 설계 DESIGN.md §8.2). M2a: 카드
목록·미리보기 API + 토큰 인증(§7 규칙: non-loopback 바인드는 토큰 필수).
M2b: WebSocket 터미널(xterm.js 5.5를 web/assets에 벤더링, 스냅샷 → 로그 tail
스트림, 키입력은 send-keys -l) + `--bind tailscale`. tokio/axum/serde는
바이너리 크레이트에만, 정적 자산은 include_str!(rust-embed 불필요).
남은 M2c: 퀵 리플라이·diff 뷰.

다음 마일스톤 순서: M2 마무리(M2b/c) →
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
   M2: tokio/axum/serde/rust-embed 허용 — 모두 바이너리 크레이트에만, krill-core는
   순수 std 유지). "가벼움"이 이 프로젝트의 정체성이다.

## 구조

- `crates/krill-core` — 라이브러리: config, git(worktree), tmux, session, kv, error.
  UI를 모른다. TUI/웹은 이 위의 얇은 뷰여야 한다.
- `crates/krill` — 바이너리: main(디스패치), args(플래그 파서), commands(구현),
  ui(TUI — 설계는 docs/DESIGN.md §8.1, TUI는 attach 시 일시중단 후 복귀하는 허브).
- 데이터: `~/.local/share/krill/{sessions,logs,worktrees}`, 설정: `~/.config/krill/config.toml`.

## 빌드와 테스트

`cargo build` → `target/debug/krill`. `cargo test` = 유닛 테스트(kv/config
파서, 세션 스토어, 헬스 판정, i18n, 플래그 파서) + 실제 git을 쓰는 통합 테스트
(crates/krill-core/tests/git_integration.rs). 테스트는 env를 변경하지 않는다 —
env 의존 로직은 순수 함수 씸(`config_path_in`, `classify`, `find_among` 등)으로
분리해 테스트한다. 이 패턴을 유지할 것.

수동 검증 시나리오: 더미 git 리포에서 `-a shell` 에이전트로 new → (tmux
send-keys로 작업 시뮬레이션) → ls → diff → `--from` 릴레이 → rm 후
tmux/worktree/브랜치 잔여물 0 확인.

## 주의할 함정

- tmux 타깃 문법: 세션 명령(has/kill/attach)은 `=name`, pane 명령(send-keys,
  pipe-pane, display-message)은 `=name:`(끝 콜론 필수). 실제로 밟았던 버그다.
- `krill new` 실패 시 롤백에서 tmux kill + worktree remove + branch delete를
  모두 해야 잔여물이 안 남는다 (commands.rs의 spawn 클로저 패턴 유지).
- **사용자 노출 문자열은 전부 `messages!` 카탈로그를 거친다** (core:
  `krill-core/src/msg.rs`, bin: `krill/src/msg.rs`). ko/en 둘 다 매크로가
  컴파일 타임에 강제하므로 하드코딩 금지. 언어 우선순위: `KRILL_LANG` >
  config `lang` > `LC_ALL`/`LC_MESSAGES`/`LANG` > en (i18n.rs). 지원 언어는
  ko/en 두 개만. 코드 주석은 한/영 혼용 OK.
