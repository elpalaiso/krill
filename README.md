# krill 🦐

> tiny orchestrator for AI coding agents — tmux + git worktrees

Orca(범고래)가 Electron에 VS Code와 Chromium까지 얹은 풀 IDE형 ADE라면,
krill은 이미 있는 도구들(tmux, git worktree, 에이전트 CLI)을 지휘만 하는
단일 바이너리입니다. 세션의 실체는 tmux라서 krill이 꺼져 있어도 에이전트는
계속 일하고, 어떤 CLI 에이전트든(Claude Code, Codex, Gemini CLI, 임의 명령)
config 한 줄로 등록됩니다.

전체 그림은 [docs/DESIGN.md](docs/DESIGN.md) 참고.

## 요구사항

git, tmux (macOS / Linux / WSL2)

## 언어 (Language)

CLI 메시지는 한국어/영어를 지원합니다. 기본은 로케일(`$LANG` 등) 자동 감지:

```sh
KRILL_LANG=en krill ls              # 1회성 강제
```

고정하려면 `~/.config/krill/config.toml`에 `lang = "ko"` (또는 `"en"`).
우선순위: `KRILL_LANG` > config `lang` > `LC_ALL`/`LC_MESSAGES`/`LANG` > en.

## 설치

아직 첫 릴리스 태그(v*) 전이라 **소스 빌드가 유일한 설치 방법**입니다.
아래 Homebrew·릴리스 바이너리 안내는 첫 태그가 나온 뒤에 동작합니다.

```sh
# 소스에서 빌드 (현재 유일한 방법, 릴리스 바이너리 ~2MB)
git clone https://github.com/elpalaiso/krill
cd krill
cargo build --release        # target/release/krill
# 또는 PATH에 바로 설치:
cargo install --path crates/krill
```

### 릴리스 후 (첫 v* 태그 이후 사용 가능)

태그(v*)를 푸시하면 GitHub Actions가 4개 플랫폼(linux/mac × x86_64/arm64)
바이너리를 릴리스에 첨부합니다. 그 뒤에는 아래 방법을 쓸 수 있습니다.

```sh
# Homebrew (이 리포가 곧 tap)
brew tap elpalaiso/krill https://github.com/elpalaiso/krill
brew install --HEAD elpalaiso/krill/krill
```

## 빠른 시작

```sh
cargo install --path crates/krill   # 또는 cargo build --release

krill init                          # ~/.config/krill/config.toml 생성

cd ~/work/myapp
krill new fix-login -m "로그인 버그 고쳐줘"     # 브랜치+worktree+tmux+claude
krill new add-tests -a codex -m "테스트 보강"   # 병렬로 하나 더

krill                               # 세션 목록·상태·diff 통계
krill attach fix-login              # 들어가서 확인 (분리: Ctrl-b d)
# Claude Code는 새 worktree마다 폴더 신뢰를 한 번 묻습니다(◆ needs-you로 표시).
# attach하거나 웹 UI 퀵 리플라이(⏎)로 응답하면 됩니다.
krill diff fix-login                # base 대비 변경 (커밋 전 포함)
krill rm fix-login                  # tmux+worktree+브랜치 정리
```

릴레이 핸드오프 — 다른 플랫폼 모델로 교차 검증:

```sh
krill new review-login -a codex --from fix-login \
    -m "이 브랜치의 변경을 리뷰하고 버그를 수정해"
```

flow — 릴레이를 자동화한 체인. 스테이지가 Done이 되면(훅) 다음 스테이지가
이전 브랜치를 이어받아 자동 시작됩니다:

```sh
# config.toml: [flows.shipit.1] 구현 → [flows.shipit.2] 리뷰 (codex)
krill new fix-login --flow shipit -m "로그인 버그 고쳐줘"
```

듀엣 — 한 worktree에서 worker와 reviewer가 턴제로 핑퐁. reviewer는
REVIEW.md(첫 줄 LGTM/ISSUES)만 쓰고, LGTM은 객관 게이트(테스트 명령)를
통과해야 유효합니다. 심판은 모델이 아니라 krill의 결정적 코드입니다:

```sh
krill duet fix-login -m "로그인 버그 고쳐줘" \
    --reviewer codex --gate "cargo test" --max-rounds 2
```

plan — 대장(planner)이 큰 목표를 plan.md 체크리스트로 분해하면, 사람은
계획만 승인합니다. 이후 작업마다 듀엣이 돌고 통과할 때마다 커밋됩니다
(작업 1개 = 커밋 1개). plan.md가 곧 작업 큐라 실행 중에 편집해도 됩니다:

```sh
krill plan big-refactor -m "설정 모듈을 모두 TOML로 이관" --gate "cargo test"
# … planner가 plan.md 작성 → ◆ needs-you → plan.md 검토·수정 후:
krill approve big-refactor
```

순회 중에는 `krill ls`의 FLOW 컬럼이 진행률(`plan:12/41`)을 보여주고,
config에 `[notify] ntfy_topic`을 설정해 두면 **작업 하나가 통과·커밋될
때마다 폰으로 진행 푸시**가 옵니다. 리뷰 라운드 캡으로 순회가 멈추면
(◆) 푸시에 리뷰어의 첫 지적이 함께 실리므로, 지적이 타당하다 싶으면
`krill resume big-refactor`(필요하면 `--rounds N`으로 캡 조정) 한 줄로
재개하면 됩니다. worker의 턴이 프로바이더 오류로 끊긴 경우에는 krill이
1회 자동 재지시하고, 그래도 멈추면 같은 방식으로 사람을 부릅니다.

## 로드맵

| 단계 | 내용 | 상태 |
|---|---|---|
| M0 | 코어 CLI: new / ls / attach / diff / rm, `--from` 릴레이 | ✅ |
| M0.5 | 코어 유닛/통합 테스트, 메시지 ko/en i18n | ✅ |
| M1 | TUI 대시보드 (ratatui), 상태 휴리스틱 | ✅ |
| M2 | `krill serve`: 웹 UI + Tailscale 원격 접속 | ✅ (`--expose`만 후순위) |
| M3 | Claude Code 훅 연동(NeedsYou 정확 감지), ntfy 푸시, merge/pr | ✅ |
| M4 | 릴리스 CI, Homebrew tap | ✅ (첫 태그 릴리스 대기) |
| M5 | 협업 모드: flow 자동 체인, 듀엣(턴제 교차모델 리뷰), planner | ✅ |

## 라이선스

MIT
