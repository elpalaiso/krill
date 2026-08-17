//! Tiny flag parser (M0 has no clap). Supports `-a val` / `--agent val`
//! style; `--flag=val` is not supported yet.

use crate::msg;
use krill_core::bail;
use krill_core::error::Result;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub struct Opts {
    pub pos: Vec<String>,
    vals: BTreeMap<&'static str, String>,
    bools: BTreeSet<&'static str>,
}

impl Opts {
    pub fn val(&self, long: &str) -> Option<&str> {
        self.vals.get(long).map(|s| s.as_str())
    }
    pub fn flag(&self, long: &str) -> bool {
        self.bools.contains(long)
    }
}

/// `val_specs` / `bool_specs`: (short, long) pairs; short may be "".
pub fn parse(
    args: &[String],
    val_specs: &[(&'static str, &'static str)],
    bool_specs: &[(&'static str, &'static str)],
) -> Result<Opts> {
    let mut opts = Opts {
        pos: Vec::new(),
        vals: BTreeMap::new(),
        bools: BTreeSet::new(),
    };
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg.starts_with('-') && arg.len() > 1 {
            if let Some((_, long)) = val_specs
                .iter()
                .find(|(s, l)| (!s.is_empty() && *s == arg) || *l == arg)
            {
                i += 1;
                let Some(value) = args.get(i) else {
                    bail!(msg::opt_needs_value(arg));
                };
                opts.vals.insert(long, value.clone());
            } else if let Some((_, long)) = bool_specs
                .iter()
                .find(|(s, l)| (!s.is_empty() && *s == arg) || *l == arg)
            {
                opts.bools.insert(long);
            } else {
                bail!(msg::unknown_option(arg));
            }
        } else {
            opts.pos.push(arg.to_string());
        }
        i += 1;
    }
    Ok(opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALS: &[(&'static str, &'static str)] = &[("-a", "--agent"), ("", "--from")];
    const BOOLS: &[(&'static str, &'static str)] = &[("-f", "--force"), ("", "--stat")];

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn positionals_values_and_flags_mix() {
        let o = parse(&s(&["name", "-a", "claude", "--from", "prev", "--force", "extra"]), VALS, BOOLS).unwrap();
        assert_eq!(o.pos, vec!["name", "extra"]);
        assert_eq!(o.val("--agent"), Some("claude"));
        assert_eq!(o.val("--from"), Some("prev"));
        assert!(o.flag("--force"));
        assert!(!o.flag("--stat"));
        assert_eq!(o.val("--repo"), None);
    }

    #[test]
    fn short_and_long_forms_are_equivalent() {
        let a = parse(&s(&["-a", "x"]), VALS, BOOLS).unwrap();
        let b = parse(&s(&["--agent", "x"]), VALS, BOOLS).unwrap();
        assert_eq!(a.val("--agent"), b.val("--agent"));
        assert!(parse(&s(&["-f"]), VALS, BOOLS).unwrap().flag("--force"));
    }

    #[test]
    fn value_option_may_consume_dash_prefixed_value() {
        // Documented quirk: the token after a value option is taken verbatim.
        let o = parse(&s(&["-a", "-f"]), VALS, BOOLS).unwrap();
        assert_eq!(o.val("--agent"), Some("-f"));
        assert!(!o.flag("--force"));
    }

    #[test]
    fn rejects_missing_value_and_unknown_option() {
        assert!(parse(&s(&["-a"]), VALS, BOOLS).unwrap_err().to_string().contains("-a"));
        assert!(parse(&s(&["--nope"]), VALS, BOOLS).unwrap_err().to_string().contains("--nope"));
    }

    #[test]
    fn bare_dash_is_positional() {
        let o = parse(&s(&["-"]), VALS, BOOLS).unwrap();
        assert_eq!(o.pos, vec!["-"]);
    }
}
