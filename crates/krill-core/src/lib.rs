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
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME 환경변수가 없습니다")
}

/// Pure join logic behind `config_path`, separated so tests don't need
/// to mutate the environment.
pub fn config_path_in(xdg_config_home: Option<&OsStr>, home: &Path) -> PathBuf {
    let base = match xdg_config_home {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => home.join(".config"),
    };
    base.join("krill").join("config.toml")
}

/// `$XDG_CONFIG_HOME/krill/config.toml` (기본 `~/.config/krill/config.toml`)
pub fn config_path() -> Result<PathBuf> {
    Ok(config_path_in(std::env::var_os("XDG_CONFIG_HOME").as_deref(), &home()?))
}

/// Pure join logic behind `data_dir` (see `config_path_in`).
pub fn data_dir_in(xdg_data_home: Option<&OsStr>, home: &Path) -> PathBuf {
    let base = match xdg_data_home {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => home.join(".local").join("share"),
    };
    base.join("krill")
}

/// `$XDG_DATA_HOME/krill` (기본 `~/.local/share/krill`)
pub fn data_dir() -> Result<PathBuf> {
    Ok(data_dir_in(std::env::var_os("XDG_DATA_HOME").as_deref(), &home()?))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_and_data_paths() {
        let home = Path::new("/home/u");
        let expect = PathBuf::from("/home/u/.config/krill/config.toml");
        assert_eq!(config_path_in(None, home), expect);
        assert_eq!(config_path_in(Some(OsStr::new("")), home), expect);
        assert_eq!(
            config_path_in(Some(OsStr::new("/xdg")), home),
            PathBuf::from("/xdg/krill/config.toml")
        );
        assert_eq!(data_dir_in(None, home), PathBuf::from("/home/u/.local/share/krill"));
        assert_eq!(data_dir_in(Some(OsStr::new("")), home), PathBuf::from("/home/u/.local/share/krill"));
        assert_eq!(data_dir_in(Some(OsStr::new("/xdg")), home), PathBuf::from("/xdg/krill"));
    }

    #[test]
    fn shell_quote_survives_single_quotes() {
        assert_eq!(shell_quote("abc"), "'abc'");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("don't"), r#"'don'\''t'"#);
    }

    #[test]
    fn valid_name_rules() {
        assert!(valid_name("fix-login"));
        assert!(valid_name("a_b_1"));
        assert!(valid_name(&"x".repeat(64)));
        assert!(!valid_name(""));
        assert!(!valid_name(&"x".repeat(65)));
        assert!(!valid_name("bad.name"));
        assert!(!valid_name("has space"));
        assert!(!valid_name("한글"));
    }

    #[test]
    fn fmt_age_units_and_boundaries() {
        assert_eq!(fmt_age(0), "0s");
        assert_eq!(fmt_age(59), "59s");
        assert_eq!(fmt_age(60), "1m");
        assert_eq!(fmt_age(3599), "59m");
        assert_eq!(fmt_age(3600), "1h");
        assert_eq!(fmt_age(86399), "23h");
        assert_eq!(fmt_age(86400), "1d");
        assert_eq!(fmt_age(90 * 86400), "90d");
    }
}
