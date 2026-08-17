//! Minimal error type so M0 stays dependency-free.
//! Swap for `anyhow`/`thiserror` when the dependency budget opens up (M1+).

use std::fmt;

#[derive(Debug)]
pub struct Error(String);

impl Error {
    pub fn msg(s: impl Into<String>) -> Self {
        Error(s.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// `bail!("...")` — return early with a formatted error.
#[macro_export]
macro_rules! bail {
    ($($t:tt)*) => {
        return Err($crate::error::Error::msg(format!($($t)*)))
    };
}

/// `.context("...")` / `.with_context(|| ...)` on Result and Option.
pub trait Context<T> {
    fn context(self, msg: &str) -> Result<T>;
    fn with_context(self, f: impl FnOnce() -> String) -> Result<T>;
}

impl<T, E: fmt::Display> Context<T> for std::result::Result<T, E> {
    fn context(self, msg: &str) -> Result<T> {
        self.map_err(|e| Error(format!("{msg}: {e}")))
    }
    fn with_context(self, f: impl FnOnce() -> String) -> Result<T> {
        self.map_err(|e| Error(format!("{}: {e}", f())))
    }
}

impl<T> Context<T> for Option<T> {
    fn context(self, msg: &str) -> Result<T> {
        self.ok_or_else(|| Error(msg.into()))
    }
    fn with_context(self, f: impl FnOnce() -> String) -> Result<T> {
        self.ok_or_else(|| Error(f()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boom() -> Result<()> {
        crate::bail!("boom {}", 42)
    }

    #[test]
    fn bail_formats() {
        assert_eq!(boom().unwrap_err().to_string(), "boom 42");
    }

    #[test]
    fn context_wraps_results_and_options() {
        let r: std::result::Result<(), &str> = Err("inner");
        assert_eq!(r.context("outer").unwrap_err().to_string(), "outer: inner");

        let r2: std::result::Result<(), &str> = Err("inner");
        assert_eq!(r2.with_context(|| format!("n={}", 1)).unwrap_err().to_string(), "n=1: inner");

        let none: Option<u8> = None;
        assert_eq!(none.context("need it").unwrap_err().to_string(), "need it");
        assert_eq!(Some(7u8).context("unused").unwrap(), 7);
    }

    #[test]
    fn io_error_converts() {
        let e: Error = std::io::Error::new(std::io::ErrorKind::NotFound, "gone").into();
        assert!(e.to_string().contains("gone"));
    }
}
