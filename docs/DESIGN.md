# krill — 경량 에이전트 개발 환경(ADE) 설계서

작성일 2026-08-17 · v0.1 (초안) · 스택: Rust

---

## 1. 한 줄 요약

Orca가 Electron 위에 VS Code와 Chromium까지 얹은 "풀 IDE형" ADE라면, **krill은 이미 존재하는 도구들 — tmux, git worktree, SSH/Tailscale, 각 에이전트 CLI — 를 지휘만 하는 수 MB짜리 단일 Rust 바이너리**다. 책상 앞에서는 TUI로, 밖에서는 내장 웹 UI를 Tailscale로 열어 폰에서 같은 세션을 보고 개입한다. 에이전트가 승인을 기다리면 폰으로 푸시 알림이 온다.

이름 제안: **krill(크릴)** — 범고래(Orca)의 반대편 끝에 있는 가장 작은 바다 생물. 대안으로 remora(빨판상어: 큰 물고기에 붙어 다니는 작은 놈 — 에이전트 CLI에 얹혀 가는 이 도구의 본질과 정확히 일치). 이름은 언제든 바꿀 수 있으니 이 문서에서는 krill로 통일한다.

## 2. 목표

여러 AI 코딩 에이전트(Claude Code, Codex, Gemini CLI, 임의의 CLI)를 각자 격리된 git worktree에서 병렬로 돌리고, 세션 목록·상태·디프를 한 화면에서 파악하며, 같은 네트워크가 아니어도 폰을 포함한 어떤 기기에서든 세션을 확인하고 입력할 수 있어야 한다. 에이전트가 사람의 결정을 기다리는 순간을 정확히 감지해 알림으로 불러내는 것이 핵심 가치다. 설치는 바이너리 하나 + 시스템에 이미 있는 git·tmux면 끝나야 한다.

## 3. 비목표 (v1에서 의도적으로 안 만드는 것)

코드 에디터 내장(에디터는 이미 각자 있다), 브라우저/미리보기 창 내장, **자체 클라우드 릴레이 서버와 계정 시스템**(운영 부담과 보안 책임이 생기는 순간 "가벼움"이 죽는다 — Orca 모바일 앱이 무거워지는 지점이 정확히 여기다), Windows 네이티브 지원(WSL2로 대체, 후술할 trait 설계로 여지는 남김), 에이전트 자체 구현(우리는 에이전트를 만들지 않고 실행만 한다).

무엇을 덜어냈는지 비교하면 이렇다.

| | Orca | krill |
|---|---|---|
| 셸/UI | Electron 데스크톱 앱 | 단일 바이너리 (TUI + 내장 웹) |
| 에디터 | VS Code 임베드 | 쓰던 에디터 그대로 |
| 웹 미리보기 | worktree별 Chromium 창 | 그냥 브라우저로 연다 |
| 모바일 | 전용 iOS/Android 앱 + 릴레이 | 반응형 웹 UI + Tailscale + ntfy 푸시 |
| 터미널 | 자체 WebGL 터미널 | tmux (지속성 공짜) |
| 설치 크기 | 수백 MB | 목표 10MB 미만 |

## 4. 설계 원칙

**원칙 1 — 어려운 문제는 이미 풀려 있다.** 세션 지속성은 tmux가, 파일 격리는 git worktree가, 원격 접속·암호화·NAT 통과는 Tailscale/SSH가, PR 생성은 gh가 해결한 문제다. krill이 직접 푸는 문제는 딱 두 개: "지금 어떤 세션이 무슨 상태인가"의 파악과, 그것을 보여주는 UI.

**원칙 2 — 에이전트는 블랙박스다.** stdin/stdout을 가진 CLI라면 무엇이든 config 한 줄로 등록된다. 벤더 중립성은 코드가 아니라 어댑터 프리셋(설정 데이터)으로 달성한다. 에이전트 CLI들의 플래그와 출력은 빠르게 바뀌므로, 코드 수정 없이 config 수정만으로 따라갈 수 있어야 한다.

**원칙 3 — 데몬을 강요하지 않는다.** TUI만 쓰는 동안 상주 프로세스는 0개다. 모든 상태는 tmux 세션 목록 + 디스크의 메타파일에서 매번 재구성한다. `krill serve`(웹)는 원격 접속이 필요할 때만 띄운다. serve가 죽어도 에이전트 세션은 tmux 안에서 계속 산다.

## 5. 아키텍처

```
  로컬 터미널 ─────────┐                  ┌───────── 폰/노트북 (외부망)
                      │                  │  Tailscale (WireGuard E2E)
                      ▼                  ▼
        ┌─────────────────────────────────────────────┐
        │           krill  (단일 Rust 바이너리)          │
        │                                             │
        │   TUI (ratatui)          Web (axum + ws)    │
        │        │                 xterm.js 내장       │
        │        └─────────┬───────────┘              │
        │                  ▼                          │
        │   core:  SessionManager · WorktreeManager   │
        │          AgentRegistry · StateStore         │
        │          EventHub (훅 수신 /api/hook)        │
        └─────────┬──────────────────┬────────────────┘
                  ▼                  ▼
             tmux 세션           git worktree
          (지속성 · 터미널)        (파일 격리)
                  │
        agent CLI (claude · codex · gemini · 임의 명령)
```

Cargo workspace 두 개의 crate로 나눈다. `krill-core`(라이브러리: 세션/worktree/상태/어댑터 로직, UI 무관)와 `krill`(바이너리: CLI 파싱, TUI, serve). 터미널 세션의 실체는 `SessionBackend` trait 뒤에 숨긴다 — v1 구현은 `TmuxBackend` 하나지만, 나중에 Windows가 필요해지면 portable-pty 기반 `PtyBackend`를 추가할 수 있는 자리를 남겨두는 것이다.

## 6. 세션 모델

세션 하나는 (이름, 대상 리포, 브랜치 `krill/<이름>`, worktree 경로, 에이전트, tmux 세션명, 상태, 생성/최근활동 시각)으로 정의되고, 메타데이터는 `~/.local/share/krill/sessions/<id>.json`에 저장된다.

수명주기는 이렇다. `krill new fix-login -a claude`를 치면 base 브랜치에서 `krill/fix-login` 브랜치와 worktree를 만들고, tmux 세션을 띄워 그 안에서 에이전트 CLI를 실행하며, `tmux pipe-pane`으로 출력을 세션 로그 파일에 흘려보내기 시작한다. 이후 로컬 TUI든 원격 웹이든 모두 이 tmux 세션을 바라보는 뷰일 뿐이다. 작업이 끝나면 디프를 확인하고 `merge`(로컬 머지 + 정리) 또는 `pr`(gh로 PR 생성) 중 하나로 마무리하고, `rm`이 tmux 세션·worktree·브랜치를 청소한다.

상태는 다섯 가지로 요약한다.

| 상태 | 의미 | 감지 방법 |
|---|---|---|
| Working | 에이전트가 작업 중 | 출력 스트림에 활동 있음 |
| **NeedsYou** | 승인/입력 대기 — 알림 발생 | ① 훅(정확) ② 출력 침묵 + 프롬프트 패턴(휴리스틱) |
| Done | 턴 완료/작업 종료 | Stop 훅 또는 프로세스 종료 |
| Shell | 에이전트는 끝나고 셸만 남음 | 포그라운드 프로세스 검사 |
| Dead | tmux 세션이 사라짐 | `tmux ls` 대조 |

여기서 가장 중요한 기술적 발견: **Claude Code는 훅(hooks)에 `type: "http"`를 지원한다.** 즉 krill의 어댑터 프리셋이 worktree의 `.claude/settings.local.json`에 `Notification`(matcher: `permission_prompt|idle_prompt`), `Stop`, `SessionEnd` 훅을 심어두면, 에이전트가 승인을 기다리는 바로 그 순간 krill의 로컬 엔드포인트(`/api/hook`)로 JSON POST가 날아온다. 화면을 긁어 추측하는 게 아니라 에이전트가 직접 알려주는 것이다. 훅이 없는 에이전트는 출력 휴리스틱으로 폴백한다. "폰에서 알림 받고 → 열어서 승인" 흐름의 정확도가 여기서 나온다.

### 6.1 훅 구현 설계 (M3) — 파일 기반 결정

§6의 발견(Claude Code의 `type: "http"` 훅)은 유효하지만, **구현은 `type:
"command"` + 상태 파일을 채택한다.** 이유는 원칙 3이다: http 훅은 수신할
서버(`krill serve`)가 떠 있어야만 동작하는데, TUI만 쓰는 사용자에게도 정확
상태가 보여야 한다. command 훅이 `krill hook <state> -i <id>`를 실행해
`~/.local/share/krill/state/<id>.kv`를 쓰면 상주 프로세스 0으로 같은 정보가
전달되고, TUI/ls/웹 모두 이 파일을 읽는다. ntfy 푸시(M3b)도 같은 훅에서
직접 보내므로 푸시조차 서버가 필요 없다.

**주입.** `krill new`가 `[agents.*] hooks = "claude-code"`인 에이전트를 띄울
때 worktree의 `.claude/settings.local.json`을 생성한다(이미 있으면 리포의
것을 존중하고 건너뜀 — 파일이 gitignore 대상이라 신선한 worktree에는 보통
없다). 훅 명령은 `current_exe()`의 절대 경로를 써서 PATH 의존을 없앤다.
Notification → `needs-you`, Stop/SessionEnd → `done`.

**상태 판정 규칙** (`session::classify`, 순수 함수):

1. tmux 세션 소멸 → **dead** (훅보다 우선).
2. 훅 상태 파일이 마지막 출력 이후에 갱신됐으면(mtime 비교, `hook_age <=
   log_age`) → **needs-you** / **done**. 에이전트가 다시 출력을 내면 로그가
   더 새로워져 자동으로 훅 상태를 벗어난다 — 상태 리셋 훅이 따로 필요 없다.
3. 그 외 → 30초 휴리스틱 (**active** / **quiet**).

훅 없는 에이전트는 2를 건너뛰므로 기존 동작 그대로다. 정렬 우선순위는
needs-you > active > quiet > done > dead.

**슬라이스.** M3a: 상태 확장 + 주입 + `krill hook`(위 전부). M3b: ntfy —
`krill hook`이 config `[notify] ntfy_topic`으로 POST(curl 위임, 새 의존성
0). M3c: `merge`/`pr` 커맨드(git merge / branch push + gh 위임).

## 7. 원격 접속 설계 — "같은 네트워크가 아니어도"

요구사항은 두 가지가 충돌하는 것처럼 보인다: 어디서든 접속돼야 하지만, 릴레이 서버·계정·TLS 인증서를 우리가 운영하면 무거워진다. 해법은 **네트워크 계층을 통째로 위임하는 3단 구성**이다.

**기본 경로 — Tailscale (권장).** 개발 머신과 폰에 Tailscale을 설치하면 두 기기는 어떤 네트워크에 있든 WireGuard로 직결된 사설망(tailnet)에 속하게 된다. 개인 무료 플랜으로 충분하고, NAT 통과·암호화·기기 인증을 전부 Tailscale이 처리한다. `krill serve --bind tailscale`은 tailnet 주소(100.x)에만 바인드하므로 공인 인터넷에는 아무것도 노출되지 않는다 — 그래서 krill 코드에 TLS도 로그인 화면도 필요 없다. 폰 브라우저에서 `http://devbox:7777`로 끝. HTTPS가 필요하면 `tailscale serve`가 인증서까지 대신 발급해준다.

**공개 URL이 정말 필요할 때 — Funnel/cloudflared.** 타인에게 데모를 보여주거나 Tailscale이 없는 기기에서 봐야 할 때만 `tailscale funnel` 또는 cloudflared 터널로 공개 HTTPS URL을 만든다. 이 경우에 한해 krill의 토큰 인증(아래)이 필수가 된다. krill은 이 도구들이 설치돼 있으면 `krill serve --expose`로 감지·연동만 하고, 터널 자체를 내장하지는 않는다.

**제로 설치 폴백 — SSH.** 이미 SSH가 되는 머신이라면 `ssh -L 7777:127.0.0.1:7777 devbox`로 웹 UI를 당겨오거나, 아예 폰의 SSH 클라이언트(Termius, Blink)로 접속해 `krill`(TUI)를 그대로 쓴다. TUI 자체가 원격 클라이언트가 되는 것 — 웹 서버가 죽어 있어도 원격 스토리가 성립하는 이중화다.

보안 규칙은 단순하고 완고하게 간다. 기본 바인드는 `127.0.0.1`. loopback이 아닌 주소에 바인드하려면 config에 토큰이 없을 경우 기동을 거부한다(토큰은 `Authorization` 헤더 또는 최초 접속 시 쿼리 파라미터 → 쿠키). 훅 수신 엔드포인트(`/api/hook`)는 항상 loopback 전용. TLS는 구현하지 않고 tailscale serve/역프록시에 위임한다.

**푸시 알림 — ntfy.** 세션이 NeedsYou/Done으로 전이하면 config에 적힌 ntfy.sh 토픽으로 POST 한 방을 보낸다. ntfy는 무료 앱 + 무서버(원하면 셀프호스트)로 폰 푸시가 되는 가장 가벼운 방법이다. 알림에는 웹 UI 딥링크(`http://devbox:7777/s/fix-login`)를 실어, 알림 탭 → 승인까지 두 번의 터치로 끝나게 한다. Orca 전용 모바일 앱의 핵심 가치(모니터링 + 알림)를 서버 인프라 0으로 재현하는 구성이다.

## 8. 인터페이스 스펙

CLI가 1차 인터페이스이고, TUI와 웹은 그 위의 뷰다.

| 명령 | 동작 |
|---|---|
| `krill new <이름> [-a 에이전트] [-r 리포] [-m "지시문"] [--from <세션>]` | worktree + tmux 세션 생성, 에이전트 실행. `--from`은 base 대신 앞 세션의 브랜치에서 시작(릴레이 핸드오프) |
| `krill ls` | 세션 목록과 상태, ±디프 통계 |
| `krill attach <이름>` | tmux attach (tmux 안이면 switch-client) |
| `krill diff <이름>` | base 대비 변경 내용 |
| `krill merge <이름> [--squash]` | base에 머지 후 세션 정리 |
| `krill pr <이름>` | gh로 PR 생성 |
| `krill rm <이름> [--force]` | tmux 세션·worktree·브랜치 정리 |
| `krill serve [--bind 127.0.0.1\|tailscale\|IP]` | 웹 UI 기동 |
| `krill` (인자 없음) | TUI 대시보드 |

TUI는 단일 화면이다. 왼쪽에 세션 목록(상태 아이콘, 에이전트, 경과 시간, ±줄 수), 오른쪽에 선택 세션의 최근 출력 미리보기. Enter로 attach, `n` 새 세션, `d` 디프, `m` 머지, `x` 정리. lazygit처럼 "한 화면에서 다 보이고, 키 하나로 들어간다"가 기준이다.

### 8.1 TUI 상세 설계 (M1) — 목업과 키맵

설계 태도: TUI는 **허브**다. 목록·상태·미리보기까지만 직접 그리고, 무거운 뷰(터미널
접속, 디프)는 TUI를 일시중단하고 기존 도구(tmux, git 페이저)에 위임한 뒤 복귀한다.
원칙 1("어려운 문제는 이미 풀려 있다")의 UI 버전이다.

**메인 화면.** 좌측 세션 목록(리포별 그룹), 우측 선택 세션의 라이브 미리보기.

```
┌─ krill ────────────────────────────┬─ fix-login · claude · +142 −38 ────────┐
│ myapp                              │ $ cargo test                           │
│▸● fix-login    claude    2m  +142  │    Compiling myapp v0.3.1              │
│ ◌ add-tests    codex    14m   +12  │ test result: ok. 42 passed             │
│ ✖ old-fix      shell     2h     -  │                                        │
│ web                                │ Login handling looks wrong in          │
│ ● hotfix       claude   30s    +3  │ session.rs — fixing the token          │
│                                    │ refresh path now...                    │
│                                    │ (2m 전 · krill/fix-login ← main)       │
├────────────────────────────────────┴────────────────────────────────────────┤
│ Enter attach  n new  d diff  x rm  r refresh  ? help  q quit                │
└─────────────────────────────────────────────────────────────────────────────┘
```

리포 그룹 헤더는 리포가 2개 이상일 때만 표시. 목록 컬럼은 상태 아이콘 · 이름 ·
에이전트 · 최근 활동 · +줄수(공간이 좁으면 삽입 수만). 미리보기 헤더에 이름 ·
에이전트 · 전체 diff 통계, 푸터에 최근 활동 시각과 브랜치 ← base.

**빈 상태.**

```
┌─ krill ────────────────────────────┬────────────────────────────────────────┐
│                                    │                                        │
│   세션이 없습니다.                 │                                        │
│                                    │                                        │
│   n 새 세션 만들기                 │                                        │
│   (CLI: krill new <이름> -m "…")   │                                        │
│                                    │                                        │
├────────────────────────────────────┴────────────────────────────────────────┤
│ n new  ? help  q quit                                                       │
└─────────────────────────────────────────────────────────────────────────────┘
```

**정리 확인 모달 (x).** CLI `rm`과 같은 안전장치를 시각화한다 — 커밋 안 된 변경이
있으면 경고를 띄우고 [y]는 실패하며 [f]만 강제 삭제한다.

```
┌─ 세션 정리 ────────────────────────────────┐
│                                            │
│ 'old-fix' 세션과 worktree,                 │
│ 브랜치 krill/old-fix을(를) 삭제합니다.     │
│                                            │
│ 커밋 안 된 변경 +12 −0 이 있습니다!        │
│                                            │
│   [y] 삭제   [f] 강제 삭제   [N] 취소      │
│                                            │
└────────────────────────────────────────────┘
```

**새 세션 (n).** 모달 폼 대신 하단 한 줄 프롬프트 3단계 — vim 커맨드라인처럼
가볍고, 구현도 제일 작다. 리포는 TUI를 띄운 디렉토리 기준(CLI와 동일 규칙).

```
 new▸ 이름: fix-login_                    (Esc 취소)
 new▸ 에이전트: claude                    (Tab 순환: claude→codex→gemini→shell)
 new▸ 지시문: 로그인 버그 고쳐줘_          (Enter 생성 · 빈 값 = 셸만)
```

**키맵.**

| 키 | 동작 | 비고 |
|---|---|---|
| j/k, ↑/↓ | 세션 선택 이동 | |
| Enter | attach — TUI 일시중단 → `tmux attach` → detach 시 복귀 | tmux 안에서는 switch-client |
| n | 새 세션 (하단 3단계 프롬프트) | |
| d | diff — TUI 일시중단 → `git diff`(페이저·색 그대로) → 복귀 | Shift-D = `--stat` |
| x | 세션 정리 (확인 모달) | 모달에서 f = force |
| r | 즉시 새로고침 | 2초 자동 폴링과 별개 |
| J/K, PgDn/PgUp | 미리보기 스크롤 | |
| ? | 키 도움말 오버레이 | |
| q, Ctrl-c | 종료 | 에이전트 세션은 tmux에 계속 산다 |
| / | 이름 필터 | M1.5 (세션 10개 넘으면) |
| m, p | merge / PR | M3에서 활성화, 그전엔 미표시 |

**상태 아이콘.** M1은 전부 휴리스틱(§6), M3에서 훅 기반 상태가 추가된다.

| 아이콘 | 상태 | 판정 |
|---|---|---|
| ● | active — 30초 내 출력 | 로그 mtime (휴리스틱) |
| ◌ | quiet — 출력 침묵 | 〃 |
| ✖ | dead — tmux 세션 소멸 | `tmux ls` 대조 (정확) |
| ◆ | needs-you — 승인/입력 대기 | M3 훅 (정확). 목록 최상단 + 강조색 |
| ✓ | done — 턴/작업 종료 | M3 훅 |

정렬: 상태 우선순위(needs-you > active > quiet > dead) → 같은 상태끼리는 최근
활동순. 훅 없는 에이전트의 휴리스틱 상태에는 `~`를 덧붙여 폴백임을 표시한다(§13).

**기술 노트.**

- 미리보기는 로그 파일 tail이 아니라 `tmux capture-pane -p -t =name:`(선택 세션만,
  리프레시마다). 현재 화면 상태를 그대로 보여주고, `-e` 없이 쓰면 ANSI 코드가
  제거된 플레인 텍스트라 파싱이 필요 없다. 색 미리보기(`-e` + SGR 파싱)는 M1.5.
- attach는 M0 CLI처럼 exec로 자신을 대체하면 안 된다(복귀해야 하므로): raw mode
  해제 → alternate screen 이탈 → `tmux attach` 자식 프로세스 wait → 화면 복원.
- 폴링 비용: 리프레시당 `tmux list-sessions` 1회 + 세션별 로그 mtime stat + 선택
  세션 capture-pane 1회. diff 통계(git 호출)는 세션당 5초 캐시. 데몬 없음 원칙
  그대로 — 폴링은 TUI가 떠 있는 동안만.
- 코드 배치: 상태 판정은 기존 `krill-core::session::health` 재사용. TUI는
  `crates/krill`의 새 모듈(ui/)로, krill-core는 UI를 모른다는 원칙 유지.
- TUI의 모든 사용자 노출 문자열도 `messages!` 카탈로그를 거친다 (ko/en).

**구현 슬라이스.** M1a: 읽기 전용 대시보드(목록 + 미리보기 + attach + 종료) —
이것만으로 매일 쓸 수 있다. M1b: 액션(n/d/x, 모달, 프롬프트). M1c: 폴리시(필터,
색 미리보기, 리사이즈 다듬기).

웹 UI는 모바일 우선 반응형 한 페이지다. 세션이 상태색 카드로 나열되고, 카드를 탭하면 터미널 뷰(xterm.js — 읽기 + 입력 가능)와 디프 뷰가 열린다. NeedsYou 상태 카드에는 자주 쓰는 응답 버튼("y", "승인", "계속해")을 노출해 폰에서 타이핑 없이 넘길 수 있게 한다. 프런트엔드는 프레임워크 없이 vanilla JS + xterm.js를 rust-embed로 바이너리에 내장한다(html/css/js 도합 4파일 이내, CDN 의존 없음 — tailnet은 오프라인 환경일 수 있다).

### 8.2 serve 상세 설계 (M2)

서버는 무상태다 — 매 요청마다 tmux + 메타파일에서 재구성하므로(원칙 3) serve가
죽어도 잃는 것이 없고, TUI와 같은 코어 함수를 공유한다. 정적 자산은 3파일
(index.html + 벤더링한 xterm.js/css)이라 `include_str!`로 충분 — rust-embed는
파일이 더 늘 때까지 도입하지 않는다(원칙 4).

**API (M2a 기준).**

| 엔드포인트 | 응답 |
|---|---|
| `GET /` | 내장 index.html (카드 목록 + 미리보기, vanilla JS) |
| `GET /api/sessions` | `[{name, repo, agent, state, age, diff}]` — 2초 폴링 |
| `GET /api/preview/{repo}/{name}` | 현재 pane 텍스트 (capture-pane). 404 = 없음, 410 = tmux 죽음 |
| `GET /ws/{repo}/{name}` | 웹소켓 터미널(M2b): 접속 시 capture-pane -e 스냅샷(Text) → pipe-pane 로그 tail 스트림(Binary, 250ms 폴링) / 키입력은 Text로 받아 send-keys -l |
| `GET /assets/xterm.{js,css}` | 벤더링된 xterm.js 5.5 (무인증 — 세션 데이터 없음) |

**인증 (§7의 구현).** 규칙은 코드에 고정: `bind`가 loopback이 아니면 config
`[serve] token` 없이는 기동을 거부한다. 토큰이 설정되면 모든 요청이
`?token=` 또는 `Authorization: Bearer`를 요구한다(401). 페이지 JS는 URL의
`?token=`을 읽어 API 호출에 이어붙인다. TLS는 구현하지 않는다 — tailscale
serve/역프록시에 위임(§7).

**슬라이스.** M2a(완료): 읽기 전용 — 카드 목록 + 텍스트 미리보기 + 토큰 인증.
M2b(완료): 상호작용 — WebSocket + xterm.js 터미널(읽기+입력, 80×24 고정 — §13),
`--bind tailscale` 키워드(`tailscale ip -4` 자동 감지). 스트림 구조: 접속 시
현재 화면 스냅샷을 먼저 보내 mid-stream 깨짐을 막고, 이후는 pipe-pane 로그의
새 바이트만 따라간다(UTF-8 경계가 깨질 수 있어 Binary 프레임). M2c(완료): 퀵 리플라이 버튼(y⏎/n⏎/⏎/Esc/^C — 키보드와 같은 WS로 전송,
M3 NeedsYou의 전신) + diff 뷰(`GET /api/diff/{repo}/{name}`, worktree vs
base, 512KB 캡, 프리픽스 색상). `--expose`(funnel/cloudflared 연동)는
실환경 없이 검증 불가라 M3~M4 시점으로 연기 — M2는 이것을 제외하고 완료.

블로킹 작업(tmux/git 서브프로세스)은 전부 `spawn_blocking`으로 실행자 밖에서.
웹 터미널 리사이즈 문제(§13)는 M2b에서 "웹 뷰 80×24 고정"으로 단순화한다.

## 9. 설정 파일

`~/.config/krill/config.toml` 하나로 끝낸다.

```toml
[agents.claude]
cmd = "claude {prompt}"
hooks = "claude-code"        # 프리셋: NeedsYou/Done 훅 자동 주입

[agents.codex]
cmd = "codex {prompt}"

[agents.yolo]                 # 임의 명령도 에이전트다
cmd = "claude --dangerously-skip-permissions {prompt}"

[repos.myapp]
path = "~/work/myapp"
base = "main"

[serve]
port = 7777
bind = "127.0.0.1"            # "tailscale" | 특정 IP
token = ""                    # 비-loopback 바인드 시 필수

[notify]
ntfy_topic = "krill-jobata-x8f2"   # 추측 불가능한 랜덤 접미사
```

## 10. 기술 스택

| 영역 | 선택 | 비고 |
|---|---|---|
| CLI | clap | derive 스타일 |
| TUI | ratatui + crossterm | |
| 웹/비동기 | tokio + axum (+ WebSocket) | 터미널 스트림·이벤트 푸시 |
| 정적 자산 | rust-embed | xterm.js 포함 전부 바이너리에 |
| 직렬화 | serde + toml / serde_json | |
| tmux 제어 | `std::process::Command` 래퍼 | new-session, send-keys, capture-pane, pipe-pane, ls |
| 디프 | git에 위임 | krill은 렌더만 |
| 에러 | anyhow + thiserror | core는 thiserror, bin은 anyhow |

tmux control mode(`-CC`)나 portable-pty 직접 관리 같은 더 정교한 방식은 알면서도 미룬다 — v1에서는 tmux CLI 호출로 충분하고, 그 단순함이 곧 유지보수 가능성이다.

## 11. 마일스톤

| 단계 | 내용 | 예상 |
|---|---|---|
| M0 코어 CLI | new/ls/attach/diff/rm — tmux+worktree 래핑. **이 시점부터 이미 매일 쓸 수 있다** | 주말 1–2개 |
| M1 TUI | 대시보드, 상태 휴리스틱, 미리보기 | +1주 |
| M2 serve | 웹 UI(목록·터미널·입력), 토큰 인증, Tailscale 사용 가이드 | +1–2주 |
| M3 알림·훅 | Claude Code 훅 프리셋, ntfy 푸시, merge/pr 플로우 | +1주 |
| M4 배포 | GitHub Actions 크로스컴파일 릴리스(cargo-dist 채택 여부는 이 시점에 판단), Homebrew tap, `cargo install`, README + 데모 GIF, MIT 라이선스 공개 | +주말 |
| M5 협업 모드 | `--from` 릴레이(실은 M0에 포함 권장) → flow 자동 체인 → 듀엣(planner 분해 + 턴제 핑퐁 + 객관 게이트). 12장 참조 | v1 공개 후 |

순서에 담긴 의도: 가치가 가장 빨리 나오는 것부터(M0만으로 "worktree 병렬 에이전트" 워크플로가 성립), 원격(M2–M3)은 그 위에 얹는다. 그리고 당연히 — 이 코드의 상당 부분은 에이전트에게 시키면 된다. 도구로 도구를 만드는 부트스트래핑이 이 프로젝트의 재미 포인트다.

## 12. 협업 모드 — 릴레이, 플로우, 듀엣 (M5)

병렬 격리(각자 딴 일)가 v1의 기본형이라면, 협업(서로의 결과를 주고받으며 한 목표를 향해)은 세 층위로 확장한다. 공통 전제는 하나 — **에이전트 간 통신 채널을 새로 만들지 않는다.** 모든 교환은 git(브랜치·커밋)과 파일(HANDOFF.md, REVIEW.md, plan.yaml)로 이루어지므로 플랫폼이 달라도 통한다. 대화 내부 상태는 벤더 간에 이식되지 않으니, 맥락은 항상 파일로 외부화하는 것이 규약이다.

**층위 1 — 릴레이 (`--from`, v1 포함).** 새 세션의 시작점을 앞 세션의 브랜치로 잡는다. `krill new review-login -a codex --from impl-login`처럼 Claude의 구현을 Codex가 이어받아 리뷰·수정한다. 교차 플랫폼 리뷰는 같은 모델의 셀프 리뷰와 달리 맹점이 겹치지 않아 검증 효과가 실재한다.

**층위 2 — 플로우 (훅 트리거 자동 체인).** 세션이 Done으로 전이하면(Stop/SessionEnd 훅) krill이 flow 정의의 다음 스테이지 세션을 자동 생성한다. 사람은 NeedsYou와 체인의 끝에서만 호출된다.

**층위 3 — 듀엣 (턴제 핑퐁).** "작업이 끝나야 주고받는" 릴레이의 한계를 넘는 모드. 핵심 발상 두 가지다. 첫째, 대장(planner) 에이전트가 큰 목표를 작은 작업들(수십 분 단위)로 분해해 plan.yaml로 저장하고, 사람은 계획만 승인한다. 둘째, 핸드오프 트리거를 "작업 완료"에서 **"턴 종료"**로 내린다 — Stop 훅은 에이전트가 응답 하나를 마칠 때마다 발화하므로, worker의 턴이 끝나면 krill이 reviewer 세션에 `send-keys`로 리뷰를 지시하고, reviewer의 턴이 끝나면 지적사항을 다시 worker에게 보낸다. 두 세션 모두 tmux에 상주하므로 양쪽 모델이 프로젝트 전체 맥락을 자기 세션에 누적한다는 부수 효과도 있다(릴레이는 핸드오프마다 맥락이 리셋된다).

```
 planner ──▶ plan.yaml (작은 작업 N개)          ← 사람은 계획 승인만
                 │
        krill flow 엔진 (모델이 아닌 결정적 코드가 심판)
                 │  작업 하나씩
   worker(모델 A) ──턴 끝(Stop 훅)──▶ reviewer(모델 B)
        ▲                                 │
        └──── 지적사항 send-keys ◀─────────┘
   종료: LGTM + gate 통과 → 커밋 → 다음 작업 / 라운드 캡 초과 → NeedsYou
```

설계 규칙 세 가지. 동시 편집은 하지 않는다 — 한 시점에 쓰는 자는 한 명뿐인 턴제 뮤텍스로, 실시간 협상·충돌 중재라는 미해결 난제를 우회하면서 교차 검증의 품질 효과만 취한다. 심판은 모델이 아니라 krill의 결정적 코드다 — 턴 순서·종료 판정은 코드가 하고, reviewer의 "LGTM"은 객관 게이트(테스트·린트 명령 통과)와 결합해야만 유효하다(모델 둘이 서로 관대해지는 담합 방지). 라운드에는 캡을 건다 — 작업당 리뷰 왕복 1–2회, flow 단위 토큰 예산. 작게 쪼갤수록 왕복 오버헤드 비율이 커지므로 캡 없는 핑퐁은 품질이 아니라 비용만 늘린다.

```toml
[flows.feature]
planner = { agent = "claude", m = "목표를 30분 내외 작업들로 분해해 plan.yaml로 저장" }

[flows.feature.each_task]
worker   = "claude"
reviewer = { agent = "codex", max_rounds = 2 }
gate     = "cargo test && cargo clippy"   # 이걸 못 넘으면 LGTM 무효
```

명시적으로 미루는 것: 여러 에이전트가 같은 파일을 동시에 만지며 역할을 실시간 협상하는 자유 협업형. 오케스트레이터 자체가 또 하나의 모델이 되어야 하고 업계 표준(A2A 등)도 정리되지 않은 연구 영역이며, 실용 가치의 대부분은 듀엣 + 작업의 파일 경계 분할로 이미 확보된다.

### 12.1 M5 구현 설계 — 슬라이스와 결정

**결정 1 — 엔진은 훅이다 (데몬 0 유지).** flow/듀엣의 "심판"은 상주
프로세스가 아니라 `krill hook done -i <id>` 안에서 돈다. 훅은 이미 모든 턴
종료(Stop/SessionEnd)마다 발화하므로, 여기서 세션 메타의 flow 필드를 보고
다음 행동(다음 스테이지 스폰, 리뷰 지시 send-keys)을 결정하면 §6.1과 같은
이유로 서버가 필요 없다. 따름정리: **flow에 참여하는 에이전트는 훅 프리셋이
있어야 한다** (`hooks = "claude-code"`). 훅 없는 에이전트는 체인의 마지막
스테이지로는 쓸 수 있지만 중간 스테이지로는 경고한다.

**결정 2 — 스테이지는 번호 섹션.** 수제 TOML 파서(§9)는 flat 섹션 +
스칼라만 지원하고, 이는 의도된 제약이다. 인라인 테이블 배열 대신:

```toml
[flows.shipit.1]
agent = "claude"
m = "구현해줘: {goal}"

[flows.shipit.2]
agent = "codex"          # 생략 시 default_agent
m = "직전 스테이지의 변경을 리뷰하고 문제를 직접 고쳐줘: {goal}"
```

스테이지 번호는 1부터 연속이어야 한다(빠진 번호는 파스 에러 — 조용한
순서 꼬임 방지). 프롬프트 규칙: 스테이지에 `m`이 있으면 `{goal}` 치환,
없으면 goal 원문, goal도 없으면 맨 에이전트.

**결정 3 — 체인은 릴레이의 자동화다.** `krill new <이름> --flow shipit -m
"목표"`가 스테이지 1 세션 `<이름>-1`을 만들고, Done 훅이 `<이름>-2`를
`--from <이름>-1`과 동일한 방식(이전 스테이지 브랜치에서 분기)으로 스폰한다.
이전 세션은 죽이지 않는다 — 사람이 각 스테이지의 맥락을 검수할 수 있고,
정리는 기존 rm/merge가 담당한다. Stop은 턴마다 발화하므로 **다음 스테이지
세션이 이미 존재하면 무시**(멱등). 스폰 실패는 훅을 실패시키지 않고 ntfy +
stderr로 알린다. 메타 추가 필드: `flow`, `flow_stage`, `flow_base`,
`flow_goal` (전부 optional — 기존 메타와 호환).

**결정 4 — 듀엣은 한 worktree, 두 tmux.** git은 같은 브랜치를 두
worktree에 못 얹고, 턴제 뮤텍스라 동시 쓰기도 없다. 따라서 worker/reviewer는
같은 worktree에 tmux 세션만 둘이다(reviewer가 worker의 **커밋 전** 변경까지
봐야 하므로 분리 worktree는 답이 아니다). 교환 프로토콜은 파일: reviewer는
코드가 아니라 `REVIEW.md`만 쓰고(첫 줄 `LGTM` 또는 `ISSUES`), worker가
지적을 반영한다. 심판은 결정적 코드: reviewer의 Done 훅이 REVIEW.md 첫 줄을
읽고 LGTM이면 gate 명령을 돌린다 — 단, cargo test 같은 게이트는 느리므로
훅이 직접 돌리지 않고 **detached 자식 `krill` 프로세스로 위임**해
훅(에이전트의 턴 종료)을 블록하지 않는다. gate 통과 → 완료(ntfy), ISSUES
또는 gate 실패 → 라운드 캡 안에서 worker에 send-keys, 캡 초과 → needs-you.

세부 규칙 네 가지(M5b). **훅 식별**: worktree당 settings.local.json이
하나라 두 세션의 훅이 같은 id로 보고하게 되는 문제는, krill이 에이전트를
띄울 때 `KRILL_SESSION_ID=<id>`를 명령 앞에 심고 훅 명령을
`-i "${KRILL_SESSION_ID:-<literal-id>}"`로 주입해 푼다 — krill이 띄운
에이전트는 자기 id로 보고하고, 사용자가 worktree에서 수동 실행한 에이전트는
literal 폴백(기존 동작)으로 남는다. **파일 수명**: REVIEW.md/GATE.md는
uncommitted 잡음이므로 심판이 관리한다 — worker의 턴이 끝나 reviewer를
호출하기 직전에 삭제(이전 라운드의 stale 판정 방지), LGTM 판독 직후 삭제
(gate·머지를 오염시키지 않게). reviewer가 REVIEW.md를 안 썼으면 재지시하되
라운드를 소모한다(무한 루프 방지). **턴 뮤텍스**: 듀엣 상태 파일
(`state/<worker-id>.duet.kv`: round, max_rounds, gate, awaiting)의 awaiting
필드가 지금 누구의 Done이 유효한지 정한다 — Stop은 턴마다 발화하므로
awaiting과 다른 쪽의 훅은 무시(멱등). **라운드**: round = worker에게
되돌려 보낸 재작업 횟수. ISSUES·gate 실패가 라운드를 소모하고, 캡(기본 2)
도달 시 needs-you + ntfy로 사람을 부른다. 상태 전이는 순수 함수
(`duet::step`)로 두고 훅/자식 프로세스는 그 결정을 IO로 옮기기만 한다.

**결정 5 — plan.yaml이 아니라 plan.md.** krill-core는 순수 std라 YAML
파서를 넣지 않는다(원칙 4). planner의 산출물은 마크다운 체크리스트
(`- [ ] 작업`)로 한다 — 결정적 파싱이 자명하고, 사람이 그대로 편집·승인할
수 있고, 에이전트도 잘 쓴다. §12 본문의 plan.yaml 표기는 이 형식으로
대체된 것으로 읽는다.

**결정 6 — plan.md가 곧 작업 큐다 (M5c).** `krill plan <이름> -m "목표"`가
planner 세션을 띄워 plan.md 체크리스트를 쓰게 하고, Done 훅이 plan.md를
확인하면 needs-you + ntfy로 사람을 부른다(계획 승인은 언제나 사람 —
plan.md를 직접 고쳐도 된다. planner가 plan.md 없이 턴을 끝내면 1회 재지시
후 사람에게 넘긴다). `krill approve <이름>`이 reviewer 세션을 붙여 승인
시점부터 **planner 세션이 그대로 duet worker가 된다** — 세션을 갈아끼우지
않으므로 프로젝트 맥락이 작업 내내 누적된다(§12 본문의 부수 효과). 작업
진행 상태는 별도 DB가 아니라 plan.md의 체크박스 그 자체다: 다음 작업 =
첫 `- [ ]` 줄, 실행 중에 사람이 작업을 추가·삭제해도 그대로 반영된다.
작업마다 duet 상태를 새로 시작(라운드 0, goal=작업 텍스트)하고, 작업이
duet를 통과하면 심판이 체크박스를 갱신한 뒤 그 갱신까지 포함해 커밋한다
(`git add -A`에서 `.claude`는 제외, REVIEW/GATE.md는 커밋 전 정리) — 작업
하나 = 커밋 하나. 모든 작업이 끝나면 phase=done + ntfy. plan 메타 상태
(phase·reviewer·gate·캡)는 `state/<id>.plan.kv`.

**슬라이스.** M5a: `[flows.*]` 파싱 + `krill new --flow` + Done 훅 자동
체인 + ls/TUI flow 표기. M5b: `krill duet`(단일 작업 핑퐁 — 공유 worktree,
REVIEW.md 프로토콜, gate, 라운드 캡). M5c: `krill plan`/`krill approve` +
plan.md 순회(작업마다 듀엣, 완료 시 커밋 + 체크박스 갱신).

## 13. 리스크와 열린 질문

**tmux 의존.** macOS/Linux에서는 사실상 표준이지만 Windows 네이티브가 없다. v1은 WSL2 안내로 대응하고, 수요가 생기면 `SessionBackend` trait에 PtyBackend를 추가한다.

**에이전트 CLI의 빠른 변화.** 플래그·출력 포맷·훅 스펙이 계속 바뀐다. 어댑터를 코드가 아닌 config 데이터 + 프리셋 파일로 유지해, 릴리스 없이 따라갈 수 있게 한다.

**휴리스틱의 부정확성.** "침묵 + 프롬프트 패턴"은 오탐이 있다. 훅을 지원하는 에이전트(Claude Code)를 1급 시민으로 두고, 휴리스틱은 어디까지나 폴백임을 UI에도 표시한다(상태 아이콘에 ~표시 등).

**실기기 dogfood에서 발견 (v0.1.0 직전, 실제 Claude Code 에이전트로 전
수명주기 검증).** 두 가지를 기록해 둔다. ① Claude Code는 턴 종료 후 유휴
~60초에 "입력 대기" Notification을 추가로 보내므로, Stop이 쓴 done을
needs-you가 덮는다 — REPL이 입력을 기다리는 건 사실이라 완전한 오탐은
아니지만, 승인 대기와 유휴 대기는 무게가 다르다. 훅 payload(현재 stdin에서
읽고 버림)의 message를 파싱해 둘을 구분하면 done이 유지된다. v0.1.x 후보.
② worktree가 세션마다 새 경로라 Claude Code의 폴더 신뢰 대화상자가
`krill new`마다 뜬다(◆ needs-you로 정확히 감지되긴 한다). 신뢰 확인은
보안 기능이므로 krill이 우회하지 않는다 — 첫 attach(또는 웹 퀵 리플라이
⏎)에서 한 번 응답하면 되고, README에 안내한다. 참고로 검증 중 M3의 mtime
레이어링(`hook_age <= log_age`)과 `${KRILL_SESSION_ID:-…}` 훅 식별은 실제
에이전트의 훅 타이밍에서 그대로 성립했다.

**Codex 리뷰어 dogfood에서 발견 (duet: worker=Claude, reviewer=Codex 실전
검증).** 훅 없는 에이전트도 자기 notify 메커니즘으로 `krill hook`을 쏠 수
있다 — Codex는 `-c notify=[...]`에 브리지 스크립트(턴 완료 JSON을 받아
`krill hook done -i "$KRILL_SESSION_ID"` 실행)를 걸면 duet 심판이 그대로
돌아간다. 어댑터=config 데이터 원칙이 실전에서 성립한 사례. 이를 위해
`KRILL_SESSION_ID` 주입 조건에서 `hooks.is_some()` 게이트를 제거했다(훅리스
에이전트도 세션 id는 알아야 브리지가 가능). 남은 이슈 ①: 심판이 리뷰
지시를 send-keys로 보낼 때 Codex 컴포저가 텍스트는 받되 Enter가 제출로
이어지지 않는 경우가 있다 — 텍스트와 Enter 사이 짧은 지연 또는 Enter 재전송
검토, v0.1.x 후보. ② 전역 `~/.codex/config.toml`의 notify는 데스크톱 앱이
쓰고 있을 수 있으므로 문서는 반드시 per-invocation `-c` 방식을 안내할 것.

**웹 터미널 리사이즈.** tmux는 가장 작은 클라이언트에 맞춰 리렌더하므로 폰 접속이 로컬 화면을 좁힐 수 있다. v1은 웹 뷰 크기를 고정(80×24 기준)하고 세션당 "웹은 읽기 중심, 입력은 짧게"로 단순화, control mode 전환은 v2 과제로.

**동시 입력.** 여러 기기가 같은 세션에 입력하면 마지막 입력이 이긴다. v1은 다른 클라이언트 접속 중임을 배지로 표시하는 것까지만.

**시크릿.** API 키는 각 에이전트 CLI가 자기 방식대로 관리한다. krill은 어떤 자격증명도 저장하지 않는다 — 저장하지 않는 것이 최고의 보안이다.

---

*다음 단계: 이 설계가 괜찮으면 M0(코어 CLI) 스캐폴드부터 구현. 문서의 모든 결정은 뒤집을 수 있는 초안이며, 특히 이름·포트 번호·키바인딩은 취향의 영역.*
