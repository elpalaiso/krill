# krill 백로그

anago 개발(dogfood — DESIGN.md §12 협업 모드 실전 검증)에서 발견된 문제들.
발견 즉시 여기 기록하고, 수정은 별도 세션에서 한다.

## 2026-08-18 — anago M0 plan/duet 순회 중

### 1. 듀엣 리뷰어 사망 시 조용한 스톨 (심각) — ✅ 수정됨

**수정 (2026-08-18)**: 심판의 지시 전송을 `deliver_instruction`으로 교체 —
죽은 리뷰어는 메타의 cmd로 재스폰 후 재전송(PingReviewer /
ReinstructReviewer), worker 쪽 전달 실패 포함 모든 전달 실패는
NeedsYou 전이 + ntfy. 재스폰 경로는 강제 kill 후 훅 발화로 실기기 검증.

리뷰어 tmux 세션이 죽은 상태에서 worker 턴이 끝나면
`commands.rs`의 심판이 `let _ = tmux::send_line(&reviewer.tmux, ...)`로
실패를 무시한다(PingReviewer / ReinstructReviewer). 지시가 소실되고
듀엣은 `awaiting=reviewer`로 영원히 대기 — NeedsYou 전이도, ntfy 알림도
없다. 실제로 anago-m0 순회가 task 3에서 알림 없이 멈췄다.

제안: send_line 실패(또는 사전 `tmux::has` 체크) 시 리뷰어 재스폰 후
지시 재전송, 최소한 worker를 NeedsYou로 전이 + 알림.

### 2. codex TUI가 리뷰 턴 후 자체 종료 → 리뷰어 사망

codex-cli 0.147.0이 첫 리뷰 턴 완료 후 "Shutting down..."을 찍고
스스로 종료했다(토큰 요약까지 출력하고 정상 종료 모양). 셸로 돌아온 뒤
세션도 사라짐. krill은 "shell survives when the agent exits" 설계지만
셸까지 죽어 세션이 dead가 됐다. codex의 idle 종료인지, 이후 도착한
send-keys가 셸에서 오작동한 것인지 원인 미확정 — 재현·조사 필요.
(#1과 결합해 조용한 스톨로 이어진 실제 트리거.)

### 3. send_line 제출 유실 — 텍스트만 남고 Enter가 안 먹힘 — ✅ 방어 추가

**수정 (2026-08-18)**: 심판의 지시 전송(`deliver_instruction`)에 한해
전송 1.2초 후 bare Enter 재전송(`tmux::press_enter`) — 빈 컴포저에선
no-op, 첫 Enter가 소실됐으면 제출. 재스폰 직후에는 TUI 부팅 대기 5초도
추가(지시가 셸에 떨어지는 것 방지). capture-pane 기반 제출 확인은 여전히
열린 개선안.

`tmux::send_line`은 literal 텍스트 직후 Enter를 보내는데, TUI 상태에
따라 Enter가 소실되어 지시문이 입력창에 타이핑만 된 채 제출되지 않는
경우가 있다(리뷰어 재기동 직후 codex에서 관찰). 제출 확인(capture-pane로
입력창 검사) 후 재시도하는 방어가 필요할 수 있다.

### 4. plan 순회가 한 라운드마다 태스크를 건너뜀 (심각 — 원인 확정) — ✅ 수정됨

**수정 (2026-08-18)**: 아래 제안대로 반영 — `finished`는 DuetState의
`goal` 우선(plan.md 재유도는 폴백), 지시문에 plan.md 편집 금지 문구 추가.

`commands.rs`의 `plan_next_task`(≈912행)가 "방금 끝난 태스크"를
DuetState의 `goal`이 아니라 **plan.md의 첫 미체크 박스에서 재유도**한다
(`plan::first_open_task(&md)`). worker 에이전트가 작업하면서 자기 태스크
박스를 스스로 [x] 체크하면, krill은 그 다음 태스크를 "끝난 것"으로
오인해 체크·커밋(`plan: <다음 태스크>`)하고 건너뛴다 — 실행도 리뷰도
없이. anago M0 순회에서 §7 결정과 json.rs 파서 태스크가 이렇게
증발했고(파서는 worker가 눈치채고 다음 턴에 직접 메꿈), 커밋 제목도
전부 한 칸씩 밀렸다.

수정: `finished`는 plan.md 재유도가 아니라 DuetState의 `goal`을 쓰고,
`check_task`는 goal과 일치하는 박스만 체크(이미 체크됐으면 no-op).
보조: `plan_task_instruction`에 "plan.md 체크는 krill이 한다 — 편집
금지" 문구 추가로 worker의 선의의 개입 차단.

### 5. codex 훅 경고 오탐 (사소) — ✅ 해소 (문서화)

**해소 (2026-08-18)**: 경고는 `hooks` 부재 시, 주입은 `hooks =
"claude-code"`일 때만이므로 — 자체 브리지 에이전트는 `hooks = "external"`
(임의 값)로 경고만 끄면 된다. config 템플릿에 codex 예시 주석 추가.

`[agents.codex]`가 notify 브리지(`krill-codex-notify` → `krill hook
done`)로 훅을 처리하는데도 duet/plan 시작 시 "agent 'codex' has no hook
preset" 경고가 뜬다. cmd에 내장된 notify 브리지를 인식하지 못함 —
경고 억제 수단(예: `hooks = "codex-notify"` 프리셋 또는 무시 플래그)이
필요하다.

### 6. stalled 듀엣을 재개할 명령이 없다 — ✅ 수정됨

**수정 (2026-08-18)**: `krill resume <name> [--rounds N]` 신설 — 전이는
순수 머신의 `Event::Resume`(Stalled에서만 유효, 라운드 리셋, `--rounds`로
캡 조정)이고 명령은 그 IO만 담당. worker에게 REVIEW.md/GATE.md 반영
재개 지시를 전송한다.

라운드 캡 초과로 `awaiting=stalled`가 되면(설계된 정지) duet::step은
이후 모든 이벤트를 무시한다 — 사람이 개입해 재개할 공식 경로가 없어,
`state/<id>.duet.kv`를 손으로 고쳐야 한다(awaiting=worker, round 조정).
anago M0의 `anago join` 태스크에서 실제로 필요했다. 제안:
`krill resume <name> [--rounds N]` — stalled 확인 후 awaiting=worker로
되돌리고 worker에게 REVIEW.md 반영 지시를 재전송.

### 7. plan 순회의 리뷰 범위가 브랜치 전체로 자람 — ✅ 수정됨 (범위 축소)

**수정 (2026-08-18)**: plan Running 중의 리뷰 지시는
`plan_review_instruction`(커밋 전 변경 = 현재 태스크 작업분만)으로 분기,
단발 duet은 기존 전체 범위 유지. 순회 종료 후 1회의 전체 브랜치 리뷰
(F단계 성격) 분리는 열린 제안으로 남김.

`duet_review_instruction`이 "current changes vs its base (committed and
uncommitted)"를 요구한다. 단발 duet에는 맞지만 plan 순회에서는 태스크마다
커밋이 쌓여, 후반 태스크의 리뷰어는 매 라운드 브랜치 전체 diff를 다시
리뷰한다(anago M0의 33번째 태스크 시점에 +14,106줄). 비용·소음이 태스크
수에 비례해 커지고, 현재 태스크와 무관한 기존 코드 지적이 ISSUES로
이어질 수 있다(라운드 캡 스톨의 원인(遠因) 후보). 제안: plan 순회의
리뷰 지시에는 범위를 "uncommitted changes"(= 현재 태스크의 작업분)로
좁히고, 전체 브랜치 리뷰는 순회 종료 후 1회(F단계 성격)로 분리.

### 8. worker 턴이 API 오류로 불완전 종료되면 듀엣이 조용히 대기 — ✅ 수정됨

**수정 (2026-08-19, M5d)**: 훅이 Notification payload를 분류해(stdin
JSON — M3a에서 버리던 것) "유휴 대기"일 때만 duet worker에게 1회
이어가기 재지시(`nudged` 플래그), 재발 시 Stalled 승격 + ntfy. 권한
프롬프트·분류 불명에는 절대 타이핑하지 않음. DESIGN §12.1 결정 7.

worker의 에이전트 턴이 프로바이더 API 오류("Server error mid-response")로
중단되면 Stop(done)이 아니라 Notification(needs-you)만 발화한다. 듀엣은
`awaiting=worker`로 남고 재지시가 없어, ◆ 표시 외엔 신호 없이 멈춘다
(anago M0 39번째 태스크에서 실제 발생 — 사람이 이어가기 지시로 복구).
제안: `awaiting=worker` 중 worker가 needs-you로 전이하면 심판이 1회
재지시(resume 문구)하고, 그래도 진행이 없으면 스톨로 승격 + ntfy.

### 9. plan 순회에서 worker 세션 컨텍스트가 소진됨

듀엣 설계는 "양쪽 모델이 프로젝트 전체 맥락을 자기 세션에 누적"을
장점으로 꼽지만(§12), 40개 태스크 순회의 후반(38번째 태스크)에서 worker
Claude 세션이 컨텍스트 100%에 도달했다. 이후 태스크의 작업 품질 저하·
자동 압축 지연의 원인이 될 수 있다. 제안: plan 순회에서 태스크 N개마다
(또는 컨텍스트 임계에서) worker에게 `/compact`를 지시하거나, HANDOFF
요약 후 세션을 로테이션하는 옵션. 최소한 문서에 장기 순회의 컨텍스트
한계를 명시.

### 10. `krill pr`이 비대화형에서 실패 — ✅ 수정됨

**수정 (2026-08-19, M5d)**: plan 세션이면 plan.md 첫 헤딩을 제목으로,
체크리스트+게이트 계약+HUMAN-VERIFY.md 포인터를 본문으로 생성해 gh에
넘긴다(`plan::pr_title`/`pr_body` 순수 함수). 일반 세션은 TTY에선 기존
gh 프롬프트 유지, 비대화형이면 `--fill`.

`gh pr create`를 인자 없이 위임해서, TTY가 아니면 "must provide --title
and --body"로 실패한다(브랜치 푸시는 성공). anago M0 PR에서 실제 발생.
제안: 비대화형이면 `--fill`을 붙이거나, plan 세션이면 plan.md(완료 태스크
목록)와 HUMAN-VERIFY.md 존재 여부로 제목·본문을 생성해 넘긴다.

## 기능 제안 — 순회 베이비시팅의 내장화 (2026-08-18, anago M0 회고)
— **✅ M5d로 구현됨 (2026-08-19)**: F1(스톨 ntfy에 리뷰 첫 지적+건수),
F2(ls/TUI `plan:12/41` + README `[notify]` 문서화), F3(#8 worker 유휴
재지시), F4(#10 plan PR 본문 자동 생성). 남은 것: 웹 UI의 REVIEW.md
열람·resume 버튼(F1의 후반부).

anago M0 순회(41 태스크, ~5시간) 동안 외부 에이전트(Claude Code 세션)가
수동으로 맡았던 역할들을 krill이 흡수할 수 있는지 검토한 결과. 원칙:
데몬 0(§6.1 훅 엔진) 유지 — 아래 제안은 모두 훅/기존 명령의 확장이다.

### F1. 스톨 triage UX — 알림에 판단 재료를 실어라

순회 중 스톨 5회의 human 개입은 전부 "REVIEW.md 읽고 → 타당하면
resume"의 반복이었다. needs-you ntfy에 REVIEW.md 첫 지적 요약(첫 줄
+ N건)을 포함하고, 웹 UI 세션 카드에서 REVIEW.md 열람 + resume
원클릭 버튼을 제공하면 개입 비용이 크게 준다. (자동 resume 정책은
비추천 — 라운드 캡은 담합·비용 방지 장치라 모델 판단으로 우회하면
§12의 "심판은 코드" 원칙이 무너진다. 캡을 config로 올리는 것으로 충분.)

### F2. plan 진행 가시성

`krill ls`/TUI/웹에 plan 세션의 진행률(12/41)·현재 태스크·라운드·
awaiting을 표시. ntfy_plan_progress는 이미 있으니 [notify] 설정만 하면
태스크별 푸시가 온다는 것을 README에 문서화.

### F3. 심판 self-heal 마무리 (#8)

리뷰어 사망 재스폰·Enter 재전송은 이미 들어갔다(#1·#3). 남은 구멍은
worker 쪽: `awaiting=worker` 중 worker가 needs-you로 전이(API 오류
중단)하면 심판이 1회 재지시, 무progress면 스톨 승격. 이것까지 되면
외부 감시자의 "소생" 역할은 소멸한다.

### F4. 순회 종료 리포트

plan done 시 요약 생성: 완료/전체, 커밋 목록, 게이트 결과, 스톨 횟수,
HUMAN-VERIFY.md 유무. `krill pr`(#10 수정 후)이 이걸 PR 본문으로 쓰면
"순회 끝 → PR"이 한 명령이 된다.
