//! Language selection for user-facing text. Two languages, on purpose:
//! Korean and English.
//!
//! Priority: `KRILL_LANG` env > `lang` key in config.toml >
//! `LC_ALL`/`LC_MESSAGES`/`LANG` > English.
//!
//! Project rule: every string a user sees goes through a `messages!`
//! catalog (core: `msg.rs`, bin: its own `msg.rs`). The macro forces
//! both languages to exist at compile time, so a translation can never
//! be forgotten.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Ko,
}

/// 0 = undecided, 1 = En, 2 = Ko.
static LANG: AtomicU8 = AtomicU8::new(0);

/// Current language. Falls back to env-only detection when `init` hasn't
/// run (library use, tests).
pub fn lang() -> Lang {
    match LANG.load(Ordering::Relaxed) {
        1 => Lang::En,
        2 => Lang::Ko,
        _ => {
            let l = pick(|k| std::env::var(k).ok());
            set(l);
            l
        }
    }
}

pub fn set(l: Lang) {
    LANG.store(match l { Lang::En => 1, Lang::Ko => 2 }, Ordering::Relaxed);
}

/// Full detection including the config file's `lang` key. Call once at
/// process start; config errors here are ignored (the command that needs
/// the config will report them properly).
pub fn init() {
    if let Some(l) = std::env::var("KRILL_LANG").ok().as_deref().and_then(parse_lang) {
        return set(l);
    }
    if let Some(l) = crate::config::Config::load()
        .ok()
        .and_then(|c| c.lang)
        .as_deref()
        .and_then(parse_lang)
    {
        return set(l);
    }
    set(pick(|k| std::env::var(k).ok()));
}

/// "ko", "ko_KR.UTF-8" → Ko; "en", "en_US" → En; anything else → None.
pub fn parse_lang(s: &str) -> Option<Lang> {
    let t = s.trim().to_ascii_lowercase();
    if t.starts_with("ko") {
        Some(Lang::Ko)
    } else if t.starts_with("en") {
        Some(Lang::En)
    } else {
        None
    }
}

/// Env-only detection, injectable for tests. Unparseable values fall
/// through to the next variable.
fn pick(get: impl Fn(&str) -> Option<String>) -> Lang {
    ["KRILL_LANG", "LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|k| get(k).as_deref().and_then(parse_lang))
        .unwrap_or(Lang::En)
}

/// Defines one `pub fn` per message that renders the current language,
/// plus a test-only `fixed` module where the language is passed
/// explicitly (so tests never touch the global). Both `en:` and `ko:`
/// are required — omitting one is a compile error.
#[macro_export]
macro_rules! messages {
    ($(
        $name:ident ( $( $arg:ident : $ty:ty ),* $(,)? ) => {
            en: $en:literal,
            ko: $ko:literal $(,)?
        }
    )*) => {
        $(
            pub fn $name( $( $arg: $ty ),* ) -> String {
                match $crate::i18n::lang() {
                    $crate::i18n::Lang::En => format!($en),
                    $crate::i18n::Lang::Ko => format!($ko),
                }
            }
        )*

        #[cfg(test)]
        #[allow(dead_code)]
        pub mod fixed {
            $(
                pub fn $name( lang: $crate::i18n::Lang, $( $arg: $ty ),* ) -> String {
                    match lang {
                        $crate::i18n::Lang::En => format!($en),
                        $crate::i18n::Lang::Ko => format!($ko),
                    }
                }
            )*
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lang_variants() {
        assert_eq!(parse_lang("ko"), Some(Lang::Ko));
        assert_eq!(parse_lang("KO"), Some(Lang::Ko));
        assert_eq!(parse_lang("ko_KR.UTF-8"), Some(Lang::Ko));
        assert_eq!(parse_lang("en"), Some(Lang::En));
        assert_eq!(parse_lang("en_US.UTF-8"), Some(Lang::En));
        assert_eq!(parse_lang("C"), None);
        assert_eq!(parse_lang("POSIX"), None);
        assert_eq!(parse_lang(""), None);
        assert_eq!(parse_lang("fr_FR"), None);
    }

    #[test]
    fn pick_priority_and_fallthrough() {
        let vars = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        assert_eq!(pick(vars(&[("KRILL_LANG", "ko"), ("LANG", "en_US")])), Lang::Ko);
        assert_eq!(pick(vars(&[("LC_ALL", "ko_KR.UTF-8"), ("LANG", "en_US")])), Lang::Ko);
        assert_eq!(pick(vars(&[("LC_MESSAGES", "en_US"), ("LANG", "ko_KR")])), Lang::En);
        assert_eq!(pick(vars(&[("LANG", "ko_KR.UTF-8")])), Lang::Ko);
        assert_eq!(pick(vars(&[("LANG", "C.UTF-8")])), Lang::En);
        assert_eq!(pick(vars(&[])), Lang::En);
        // an unparseable high-priority value falls through, not to En
        assert_eq!(pick(vars(&[("KRILL_LANG", "fr"), ("LANG", "ko_KR")])), Lang::Ko);
    }
}
