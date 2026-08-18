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

### 5. codex 훅 경고 오탐 (사소)

`[agents.codex]`가 notify 브리지(`krill-codex-notify` → `krill hook
done`)로 훅을 처리하는데도 duet/plan 시작 시 "agent 'codex' has no hook
preset" 경고가 뜬다. cmd에 내장된 notify 브리지를 인식하지 못함 —
경고 억제 수단(예: `hooks = "codex-notify"` 프리셋 또는 무시 플래그)이
필요하다.
