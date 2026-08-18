mod ansi;
mod args;
mod commands;
mod msg;
mod serve;
mod ui;

use krill_core::bail;
use krill_core::error::{Context, Result};
use std::io::IsTerminal;

fn main() {
    if let Err(e) = run() {
        eprintln!("krill: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    // Decide the message language before anything can print.
    krill_core::i18n::init();

    let argv: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, rest) = match argv.split_first() {
        // No args: TUI dashboard on a real terminal, `ls` otherwise
        // (pipes, scripts).
        None if std::io::stdout().is_terminal() && std::io::stdin().is_terminal() => {
            return ui::run()
        }
        None => return commands::ls(),
        Some((c, r)) => (c.as_str(), r),
    };

    match cmd {
        "ls" => commands::ls(),
        "init" => commands::init(),
        "new" => {
            let o = args::parse(
                rest,
                &[("-a", "--agent"), ("-r", "--repo"), ("-m", "--message"), ("", "--from"), ("", "--flow")],
                &[],
            )?;
            let name = o.pos.first().context(msg::name_required("new"))?;
            commands::new(
                name,
                o.val("--agent"),
                o.val("--repo"),
                o.val("--message"),
                o.val("--from"),
                o.val("--flow"),
            )
        }
        "attach" => {
            let o = args::parse(rest, &[("-r", "--repo")], &[])?;
            let name = o.pos.first().context(msg::name_required("attach"))?;
            commands::attach(name, o.val("--repo"))
        }
        "diff" => {
            let o = args::parse(rest, &[("-r", "--repo")], &[("", "--stat")])?;
            let name = o.pos.first().context(msg::name_required("diff"))?;
            commands::diff(name, o.val("--repo"), o.flag("--stat"))
        }
        "duet" => {
            let o = args::parse(
                rest,
                &[
                    ("-a", "--agent"),
                    ("", "--reviewer"),
                    ("-r", "--repo"),
                    ("-m", "--message"),
                    ("", "--gate"),
                    ("", "--max-rounds"),
                ],
                &[],
            )?;
            let name = o.pos.first().context(msg::name_required("duet"))?;
            commands::duet(
                name,
                o.val("--agent"),
                o.val("--reviewer"),
                o.val("--repo"),
                o.val("--message"),
                o.val("--gate"),
                o.val("--max-rounds"),
            )
        }
        "plan" => {
            let o = args::parse(
                rest,
                &[
                    ("-a", "--agent"),
                    ("", "--reviewer"),
                    ("-r", "--repo"),
                    ("-m", "--message"),
                    ("", "--gate"),
                    ("", "--max-rounds"),
                ],
                &[],
            )?;
            let name = o.pos.first().context(msg::name_required("plan"))?;
            commands::plan(
                name,
                o.val("--agent"),
                o.val("--reviewer"),
                o.val("--repo"),
                o.val("--message"),
                o.val("--gate"),
                o.val("--max-rounds"),
            )
        }
        "approve" => {
            let o = args::parse(rest, &[("-r", "--repo")], &[])?;
            let name = o.pos.first().context(msg::name_required("approve"))?;
            commands::approve(name, o.val("--repo"))
        }
        "resume" => {
            let o = args::parse(rest, &[("-r", "--repo"), ("", "--rounds")], &[])?;
            let name = o.pos.first().context(msg::name_required("resume"))?;
            commands::resume(name, o.val("--repo"), o.val("--rounds"))
        }
        "duet-gate" => {
            // Internal: the detached gate child spawned by the referee.
            let o = args::parse(rest, &[("-i", "--id")], &[])?;
            let id = o.val("--id").context(msg::hook_usage())?;
            commands::duet_gate(id)
        }
        "hook" => {
            let o = args::parse(rest, &[("-i", "--id")], &[])?;
            let state = o.pos.first().context(msg::hook_usage())?;
            let id = o.val("--id").context(msg::hook_usage())?;
            commands::hook(state, id)
        }
        "serve" => {
            let o = args::parse(rest, &[("-b", "--bind"), ("-p", "--port")], &[])?;
            commands::serve(o.val("--bind"), o.val("--port"))
        }
        "merge" => {
            let o = args::parse(rest, &[("-r", "--repo")], &[("", "--squash")])?;
            let name = o.pos.first().context(msg::name_required("merge"))?;
            commands::merge(name, o.val("--repo"), o.flag("--squash"))
        }
        "pr" => {
            let o = args::parse(rest, &[("-r", "--repo")], &[])?;
            let name = o.pos.first().context(msg::name_required("pr"))?;
            commands::pr(name, o.val("--repo"))
        }
        "rm" => {
            let o = args::parse(rest, &[("-r", "--repo")], &[("-f", "--force")])?;
            let name = o.pos.first().context(msg::name_required("rm"))?;
            commands::rm(name, o.val("--repo"), o.flag("--force"))
        }
        "-h" | "--help" | "help" => {
            print!("{}", msg::help());
            Ok(())
        }
        "-V" | "--version" | "version" => {
            println!("krill {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => bail!("{}\n\n{}", msg::unknown_command(other), msg::help()),
    }
}
