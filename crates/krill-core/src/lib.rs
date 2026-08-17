//! krill-core: session, worktree, tmux and config management.
//!
//! Design doc: docs/DESIGN.md. This crate knows nothing about UI —
//! the TUI (M1) and web server (M2) are thin views over these primitives.
//!
//! M0 is intentionally dependency-free (std only). clap/serde/ratatui
//! arrive with M1+.

pub mod config;
pub mod error;
pub mod git;
pub mod kv;
pub mod session;
pub mod tmux;

use error::{Context, Result};
use std::path::PathBuf;

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME 환경변수가 없습니다")
}

/// `$XDG_CONFIG_HOME/krill/config.toml` (기본 `~/.config/krill/config.toml`)
pub fn config_path() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => home()?.join(".config"),
    };
    Ok(base.join("krill").join("config.toml"))
}

/// `$XDG_DATA_HOME/krill` (기본 `~/.local/share/krill`)
pub fn data_dir() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_DATA_HOME") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => home()?.join(".local").join("share"),
    };
    Ok(base.join("krill"))
}

pub fn sessions_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("sessions"))
}

pub fn logs_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("logs"))
}

pub fn worktrees_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("worktrees"))
}

/// Single-quote a string for POSIX shells.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Session names become branch/tmux/file names, so keep them boring.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// "3s" / "5m" / "2h" / "4d"
pub fn fmt_age(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86400),
    }
}
