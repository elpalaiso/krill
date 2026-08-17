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

M2 완료(`--expose`만 M3~M4로 연기): `krill serve` 웹 UI(serve.rs, 설계
DESIGN.md §8.2). M2a: 카드 목록·미리보기 API + 토큰 인증(§7 규칙:
non-loopback 바인드는 토큰 필수). M2b: WebSocket 터미널(xterm.js 5.5를
web/assets에 벤더링, 스냅샷 → 로그 tail 스트림, 키입력은 send-keys -l) +
`--bind tailscale`. M2c: 퀵 리플라이 버튼(y⏎/n⏎/⏎/Esc/^C) + diff 뷰
(/api/diff, 512KB 캡). tokio/axum/serde는 바이너리 크레이트에만, 정적
자산은 include_str!(rust-embed 불필요).

M3a 완료(설계 DESIGN.md §6.1): 훅은 http가 아니라 **파일 기반** — `krill new`가
worktree에 .claude/settings.local.json을 주입(current_exe 절대 경로,
기존 파일은 존중)하고, 에이전트 훅이 `krill hook <needs-you|done> -i <id>`로
`state/<id>.kv`를 쓴다. 상태 판정: dead > (훅이 마지막 출력보다 새로우면
needs-you/done) > 30초 휴리스틱 — 출력이 재개되면 자동으로 훅 상태를 벗어난다.
Status 5종(needs-you ◆ / active ● / quiet ◌ / done ✓ / dead ✖)을 ls/TUI/웹이
공유. M3b: `krill hook`이 config `[notify] ntfy_topic`으로 푸시(curl 위임,
fire-and-forget — 훅이 네트워크 때문에 막히면 안 됨). M3c: `merge`(base
체크아웃 상태에서만, dirty 세션 거부 — 단 주입된 .claude/는 dirty로 안 침,
--squash는 스테이징만 하고 세션 유지, 성공 시 자동 정리) + `pr`(push 후
gh 위임). remove_session은 krill이 주입한 untracked settings.local.json을
먼저 치워 non-force worktree remove가 성립하게 한다. M3 완료.

M4 완료: CI(.github/workflows/ci.yml — push/PR마다 빌드+테스트), 릴리스
(release.yml — v* 태그 시 4개 플랫폼 네이티브 빌드 → tar.gz+sha256 릴리스
첨부, 크로스컴파일 없이 arm 러너 사용), Homebrew(Formula/krill.rb — 메인
리포가 곧 tap, 첫 태그 후 sha256 채우기). 릴리스 바이너리 실측 2.1MB
(xterm.js 내장 + tokio/axum/ratatui 포함 — 목표 10MB의 1/5).
릴리스 절차: 버전 올리고 `git tag vX.Y.Z && git push origin vX.Y.Z`.

M5a 완료(설계 DESIGN.md §12.1): flow 자동 체인 — config `[flows.<name>.<n>]`
번호 섹션(수제 파서 변경 없음, 스테이지 1..N 연속 강제), `krill new <이름>
--flow <flow> -m "목표"`가 `<이름>-1`을 만들고, `krill hook done`이 메타의
flow 필드를 보고 다음 스테이지를 `--from` 릴레이로 자동 스폰 — 체인 엔진이
훅 안에 있어 데몬 0 유지. Stop은 턴마다 발화하므로 다음 세션이 이미 있으면
무시(멱등 판정은 `session::flow_next` 순수 함수). 스폰 실패·unknown flow는
훅을 실패시키지 않고 stderr/ntfy로 보고. 훅 없는 에이전트가 마지막 외
스테이지에 있으면 시작 시 경고. 훅의 repo 해석은 worktree cwd가 아니라
meta.repo_path 기준(create_session_full의 `at` 씸). ls는 FLOW 컬럼(flow
세션이 있을 때만), TUI는 미리보기 타이틀에 `flow:stage` 표시.

M5b 완료(설계 DESIGN.md §12.1 결정 4): `krill duet <이름> -m "작업"` —
worker(일반 세션) + reviewer(같은 worktree에 tmux만 하나 더, `<이름>-rev`)의
턴제 핑퐁. 심판의 두뇌는 순수 상태 머신 `krill-core/src/duet.rs`의
`step(state, event)`이고, 훅/게이트 자식은 그 결정(Action)을 IO로 옮기기만
한다. 훅 식별: 한 worktree에 settings.local.json이 하나뿐이라, krill이
에이전트를 띄울 때 `KRILL_SESSION_ID=<id>`를 명령 앞에 심고 훅 명령은
`-i "${KRILL_SESSION_ID:-<literal>}"` (수동 실행은 literal 폴백). 프로토콜:
reviewer는 REVIEW.md만 작성(첫 줄 LGTM/ISSUES, LGTM 외 텍스트는 ISSUES로
간주), LGTM이면 gate(우선순위 CLI --gate > `[repos.*] gate` > `[duet]
gate`)를 detached `krill duet-gate` 자식으로 실행(훅을 블록하지 않음), 실패
출력은 GATE.md로. REVIEW/GATE.md 수명은 심판이 관리(다음 리뷰 전·LGTM 판독
후 삭제, merge의 dirty 판정과 rm의 non-force remove에서도 무시/정리).
라운드(재작업 횟수)가 캡(기본 2, `--max-rounds`) 도달 시 needs-you + ntfy.
duet 상태는 `state/<worker-id>.duet.kv`(awaiting 턴 뮤텍스 — 어긋난 Stop은
무시). rm은 worker에서 듀엣 전체 캐스케이드(reviewer만 rm하면 tmux+메타만),
merge는 reviewer 세션이면 거부. ls FLOW 컬럼·TUI 타이틀에 duet:역할 표시.

M5c 완료(설계 DESIGN.md §12.1 결정 6): `krill plan <이름> -m "목표"` →
planner 세션이 plan.md 체크리스트 작성, Done 훅이 plan.md 확인 시
phase=ready + needs-you(없으면 1회 재지시 후 사람에게), `krill approve
<이름>`이 reviewer를 붙이고 **planner 세션이 그대로 duet worker가 된다**
(맥락 누적). plan.md 자체가 작업 큐(다음 작업 = 첫 `- [ ]`, 실행 중 편집
가능), 작업마다 duet 상태 리셋(라운드 0, goal=작업 텍스트), 작업이 duet
통과 시 심판이 체크박스 갱신 후 그 갱신 포함 커밋(작업 1개 = 커밋 1개,
add에서 .claude 제외·REVIEW/GATE 정리). 전부 끝나면 phase=done + ntfy.
plan 메타는 `state/<id>.plan.kv`(krill-core/src/plan.rs — 파싱·상태 순수
함수), 훅 체인 순서는 flow → plan(planning만) → duet. ls FLOW 컬럼에
plan:phase 표시. **M5 완료 — 로드맵 M0~M5 전부 ✅.**

다음: v0.1.0 태그 릴리스(release.yml 첫 실행 검증, Formula sha256 채우기),
이후는 백로그(§13 리스크: --expose, 웹 리사이즈, control mode 등).

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
