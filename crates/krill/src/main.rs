mod args;
mod commands;
mod msg;

use krill_core::bail;
use krill_core::error::{Context, Result};

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
            let name = o.pos.first().context(msg::name_required("new"))?;
            commands::new(name, o.val("--agent"), o.val("--repo"), o.val("--message"), o.val("--from"))
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
