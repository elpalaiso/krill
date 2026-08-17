use crate::bail;
use crate::error::{Context, Error, Result};
use std::path::Path;
use std::process::Command;

fn tmux(args: &[&str]) -> Result<std::process::Output> {
    Command::new("tmux")
        .args(args)
        .output()
        .context("tmux를 실행할 수 없습니다 (설치돼 있나요?)")
}

fn ok(args: &[&str]) -> Result<String> {
    let out = tmux(args)?;
    if !out.status.success() {
        bail!(
            "tmux {} 실패: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Exact-match session target (`=name` prevents prefix matching).
fn target(name: &str) -> String {
    format!("={name}")
}

/// Pane-target form: commands that address a pane (send-keys, pipe-pane,
/// display-message) reject a bare `=name` — the trailing `:` selects the
/// session's active window/pane.
fn pane_target(name: &str) -> String {
    format!("={name}:")
}

/// All live tmux session names ([] if the server isn't running).
pub fn server_sessions() -> Vec<String> {
    match tmux(&["list-sessions", "-F", "#{session_name}"]) {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

pub fn has(name: &str) -> bool {
    matches!(tmux(&["has-session", "-t", &target(name)]), Ok(o) if o.status.success())
}

/// Create a detached session running the default shell in `cwd`.
pub fn new_session(name: &str, cwd: &Path) -> Result<()> {
    let cwd_s = cwd.to_string_lossy();
    ok(&["new-session", "-d", "-s", name, "-c", &cwd_s]).map(|_| ())
}

/// Type a line into the session (literal text, then Enter). Running the
/// agent via send-keys — instead of as the session command — means the
/// shell survives when the agent exits ("Shell" state in the design doc).
pub fn send_line(name: &str, line: &str) -> Result<()> {
    let t = pane_target(name);
    ok(&["send-keys", "-t", &t, "-l", "--", line])?;
    ok(&["send-keys", "-t", &t, "Enter"]).map(|_| ())
}

/// Stream all pane output into a log file (append).
pub fn pipe_to_log(name: &str, log: &Path) -> Result<()> {
    let cmd = format!("cat >> {}", crate::shell_quote(&log.to_string_lossy()));
    ok(&["pipe-pane", "-t", &pane_target(name), "-o", &cmd]).map(|_| ())
}

pub fn kill(name: &str) -> Result<()> {
    ok(&["kill-session", "-t", &target(name)]).map(|_| ())
}

/// How many clients are attached to the session.
pub fn attached_count(name: &str) -> u32 {
    tmux(&["display-message", "-p", "-t", &pane_target(name), "#{session_attached}"])
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(0)
}

/// Replace this process with `tmux attach` (or `switch-client` when
/// already inside tmux). Only returns on failure.
pub fn attach_exec(name: &str) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let t = target(name);
    let err = if std::env::var_os("TMUX").is_some() {
        Command::new("tmux").args(["switch-client", "-t", &t]).exec()
    } else {
        Command::new("tmux").args(["attach-session", "-t", &t]).exec()
    };
    Err(Error::msg(format!("tmux attach 실패: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_syntax_exact_match_and_pane_colon() {
        // Session commands use `=name`; pane commands need the trailing
        // colon (`=name:`) — a real bug we shipped once (CLAUDE.md).
        assert_eq!(target("fix"), "=fix");
        assert_eq!(pane_target("fix"), "=fix:");
    }

    #[test]
    fn server_sessions_never_panics_without_a_server() {
        // With no tmux server (or no tmux at all) this must degrade to [].
        let _ = server_sessions();
    }
}
