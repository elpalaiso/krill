mod args;
mod commands;

use krill_core::bail;
use krill_core::error::{Context, Result};

const HELP: &str = "\
krill — tiny orchestrator for AI coding agents (tmux + git worktrees)

에이전트를 git worktree로 격리해 병렬로 돌립니다. 세션의 실체는 tmux라서
krill이 꺼져 있어도 에이전트는 계속 일합니다.

사용법:
  krill                               세션 목록 (= krill ls)
  krill init                          설정 파일 생성 (~/.config/krill/config.toml)
  krill new <이름> [옵션]             새 세션: 브랜치 + worktree + tmux + 에이전트
      -a, --agent <이름>              에이전트 (config의 [agents.*])
      -r, --repo <이름>               대상 리포 (생략 시 현재 디렉토리의 리포)
      -m, --message <지시문>          에이전트에게 넘길 첫 지시문
          --from <세션>               다른 세션의 브랜치에서 시작 (릴레이 핸드오프)
  krill attach <이름> [-r <리포>]     tmux 접속 (분리: Ctrl-b d)
  krill diff <이름> [--stat]          base 대비 변경 내용 (커밋 전 변경 포함)
  krill rm <이름> [-f|--force]        세션 · worktree · 브랜치 정리
  krill --help | --version

예시:
  krill new fix-login -m \"로그인 버그 고쳐줘\"
  krill new review-login -a codex --from fix-login -m \"이 브랜치 리뷰하고 수정해\"
";

fn main() {
    if let Err(e) = run() {
        eprintln!("krill: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, rest) = match argv.split_first() {
        None => return commands::ls(),
        Some((c, r)) => (c.as_str(), r),
    };

    match cmd {
        "ls" => commands::ls(),
        "init" => commands::init(),
        "new" => {
            let o = args::parse(
                rest,
                &[("-a", "--agent"), ("-r", "--repo"), ("-m", "--message"), ("", "--from")],
                &[],
            )?;
            let name = o.pos.first().context("세션 이름이 필요합니다: krill new <이름>")?;
            commands::new(name, o.val("--agent"), o.val("--repo"), o.val("--message"), o.val("--from"))
        }
        "attach" => {
            let o = args::parse(rest, &[("-r", "--repo")], &[])?;
            let name = o.pos.first().context("세션 이름이 필요합니다: krill attach <이름>")?;
            commands::attach(name, o.val("--repo"))
        }
        "diff" => {
            let o = args::parse(rest, &[("-r", "--repo")], &[("", "--stat")])?;
            let name = o.pos.first().context("세션 이름이 필요합니다: krill diff <이름>")?;
            commands::diff(name, o.val("--repo"), o.flag("--stat"))
        }
        "rm" => {
            let o = args::parse(rest, &[("-r", "--repo")], &[("-f", "--force")])?;
            let name = o.pos.first().context("세션 이름이 필요합니다: krill rm <이름>")?;
            commands::rm(name, o.val("--repo"), o.flag("--force"))
        }
        "-h" | "--help" | "help" => {
            print!("{HELP}");
            Ok(())
        }
        "-V" | "--version" | "version" => {
            println!("krill {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => bail!("알 수 없는 명령: {other}\n\n{HELP}"),
    }
}
