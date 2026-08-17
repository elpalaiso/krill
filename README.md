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

```sh
# Homebrew (이 리포가 곧 tap)
brew tap elpalaiso/krill https://github.com/elpalaiso/krill
brew install --HEAD elpalaiso/krill/krill

# 또는 소스에서 (릴리스 바이너리 ~2MB)
cargo install --path crates/krill
```

태그(v*)를 푸시하면 GitHub Actions가 4개 플랫폼(linux/mac × x86_64/arm64)
바이너리를 릴리스에 첨부합니다.

## 빠른 시작

```sh
cargo install --path crates/krill   # 또는 cargo build --release

krill init                          # ~/.config/krill/config.toml 생성

cd ~/work/myapp
krill new fix-login -m "로그인 버그 고쳐줘"     # 브랜치+worktree+tmux+claude
krill new add-tests -a codex -m "테스트 보강"   # 병렬로 하나 더

krill                               # 세션 목록·상태·diff 통계
krill attach fix-login              # 들어가서 확인 (분리: Ctrl-b d)
krill diff fix-login                # base 대비 변경 (커밋 전 포함)
krill rm fix-login                  # tmux+worktree+브랜치 정리
```

릴레이 핸드오프 — 다른 플랫폼 모델로 교차 검증:

```sh
krill new review-login -a codex --from fix-login \
    -m "이 브랜치의 변경을 리뷰하고 버그를 수정해"
```

## 로드맵

| 단계 | 내용 | 상태 |
|---|---|---|
| M0 | 코어 CLI: new / ls / attach / diff / rm, `--from` 릴레이 | ✅ |
| M0.5 | 코어 유닛/통합 테스트, 메시지 ko/en i18n | ✅ |
| M1 | TUI 대시보드 (ratatui), 상태 휴리스틱 | ✅ |
| M2 | `krill serve`: 웹 UI + Tailscale 원격 접속 | ✅ (`--expose`만 후순위) |
| M3 | Claude Code 훅 연동(NeedsYou 정확 감지), ntfy 푸시, merge/pr | ✅ |
| M4 | 릴리스 CI, Homebrew tap | ✅ (첫 태그 릴리스 대기) |
| M5 | 협업 모드: flow 자동 체인, 듀엣(턴제 교차모델 리뷰) | ⬜ |

## 라이선스

MIT
