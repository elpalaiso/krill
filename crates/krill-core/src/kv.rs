//! Line-based `key=value` persistence for session metadata.
//! (M0 has no serde; this format is trivial to read, diff and hand-edit.)
//!
//! Values escape `\` → `\\` and newline → `\n`.

use crate::error::Result;
use std::collections::BTreeMap;
use std::path::Path;

fn escape(v: &str) -> String {
    v.replace('\\', "\\\\").replace('\n', "\\n")
}

fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn write_file(path: &Path, map: &BTreeMap<String, String>) -> Result<()> {
    let mut body = String::new();
    for (k, v) in map {
        body.push_str(k);
        body.push('=');
        body.push_str(&escape(v));
        body.push('\n');
    }
    std::fs::write(path, body)?;
    Ok(())
}

pub fn read_file(path: &Path) -> Result<BTreeMap<String, String>> {
    let raw = std::fs::read_to_string(path)?;
    let mut map = BTreeMap::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), unescape(v));
        }
    }
    Ok(map)
}
