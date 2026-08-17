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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_roundtrip() {
        for v in ["", "plain", "a=b=c", "line1\nline2", "back\\slash", "\\n literal", "mix\\\nboth", "trail\\"] {
            assert_eq!(unescape(&escape(v)), v, "value: {v:?}");
        }
        assert_eq!(escape("a\nb"), "a\\nb");
        assert_eq!(unescape("bare\\qkeep"), "bare\\qkeep"); // unknown escape preserved
        assert_eq!(unescape("end\\"), "end\\"); // dangling backslash preserved
    }

    #[test]
    fn file_roundtrip() {
        let dir = std::env::temp_dir().join(format!("krill-test-kv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.kv");

        let mut m = BTreeMap::new();
        m.insert("cmd".to_string(), "claude 'do it'\nline2".to_string());
        m.insert("empty".to_string(), String::new());
        m.insert("eq".to_string(), "a=b".to_string());
        write_file(&path, &m).unwrap();
        assert_eq!(read_file(&path).unwrap(), m);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_is_lenient_about_stray_lines() {
        assert!(read_file(Path::new("/nonexistent/krill-test.kv")).is_err());

        let dir = std::env::temp_dir().join(format!("krill-test-kv2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.kv");
        std::fs::write(&path, "\n a = 1\nno-equals-sign\n").unwrap();
        let m = read_file(&path).unwrap();
        assert_eq!(m.len(), 1); // the no-equals line is skipped
        assert_eq!(m.get("a").map(String::as_str), Some(" 1")); // key trimmed, value kept verbatim

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
