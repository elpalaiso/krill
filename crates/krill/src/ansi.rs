//! Tiny ANSI/SGR → ratatui parser for the TUI preview (M1c).
//!
//! `tmux capture-pane -e` emits pane content with SGR color codes; this
//! turns them into styled `Line`s. Only SGR (`ESC[…m`) is interpreted —
//! every other CSI sequence and OSC string is stripped. That is enough
//! for captured pane *content* (cursor movement never appears in it).

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub fn parse(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut style = Style::new();

    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.peek() {
                Some('[') => {
                    chars.next();
                    // CSI: parameter/intermediate bytes, then a final byte @..~
                    let mut params = String::new();
                    let mut fin = None;
                    for c in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c) {
                            fin = Some(c);
                            break;
                        }
                        params.push(c);
                    }
                    if fin == Some('m') {
                        flush(&mut spans, &mut buf, style);
                        style = apply_sgr(style, &params);
                    }
                    // any other CSI (cursor moves, erases…) is dropped
                }
                Some(']') => {
                    chars.next();
                    // OSC: skip until BEL or ST (ESC \)
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {
                    chars.next(); // two-byte escape (ESC c, ESC = …): drop
                }
            },
            '\n' => {
                flush(&mut spans, &mut buf, style);
                lines.push(Line::from(std::mem::take(&mut spans)));
            }
            '\r' => {}
            _ => buf.push(c),
        }
    }
    flush(&mut spans, &mut buf, style);
    if !spans.is_empty() || lines.is_empty() {
        lines.push(Line::from(spans));
    }
    lines
}

fn flush(spans: &mut Vec<Span<'static>>, buf: &mut String, style: Style) {
    if !buf.is_empty() {
        spans.push(Span::styled(std::mem::take(buf), style));
    }
}

/// Apply an SGR parameter list ("1;32", "38;5;208", "" = reset) to a style.
fn apply_sgr(mut style: Style, params: &str) -> Style {
    let mut it = params
        .split(';')
        .map(|p| p.parse::<u16>().unwrap_or(0))
        .peekable();
    if params.is_empty() {
        return Style::new();
    }
    while let Some(p) = it.next() {
        style = match p {
            0 => Style::new(),
            1 => style.add_modifier(Modifier::BOLD),
            2 => style.add_modifier(Modifier::DIM),
            3 => style.add_modifier(Modifier::ITALIC),
            4 => style.add_modifier(Modifier::UNDERLINED),
            7 => style.add_modifier(Modifier::REVERSED),
            22 => style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => style.remove_modifier(Modifier::ITALIC),
            24 => style.remove_modifier(Modifier::UNDERLINED),
            27 => style.remove_modifier(Modifier::REVERSED),
            30..=37 => style.fg(basic(p - 30)),
            39 => style.fg(Color::Reset),
            40..=47 => style.bg(basic(p - 40)),
            49 => style.bg(Color::Reset),
            90..=97 => style.fg(bright(p - 90)),
            100..=107 => style.bg(bright(p - 100)),
            38 | 48 => {
                let color = match it.next() {
                    Some(5) => it.next().map(|n| Color::Indexed(n as u8)),
                    Some(2) => {
                        let (r, g, b) = (it.next(), it.next(), it.next());
                        match (r, g, b) {
                            (Some(r), Some(g), Some(b)) => {
                                Some(Color::Rgb(r as u8, g as u8, b as u8))
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };
                match (p, color) {
                    (38, Some(c)) => style.fg(c),
                    (48, Some(c)) => style.bg(c),
                    _ => style,
                }
            }
            _ => style, // unknown SGR: ignore
        };
    }
    style
}

fn basic(n: u16) -> Color {
    [
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::Gray,
    ][n as usize]
}

fn bright(n: u16) -> Color {
    [
        Color::DarkGray,
        Color::LightRed,
        Color::LightGreen,
        Color::LightYellow,
        Color::LightBlue,
        Color::LightMagenta,
        Color::LightCyan,
        Color::White,
    ][n as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span_texts(line: &Line) -> Vec<String> {
        line.iter().map(|s| s.content.to_string()).collect()
    }

    #[test]
    fn plain_text_passes_through() {
        let lines = parse("hello\nworld");
        assert_eq!(lines.len(), 2);
        assert_eq!(span_texts(&lines[0]), ["hello"]);
        assert_eq!(span_texts(&lines[1]), ["world"]);
        assert_eq!(lines[0].spans[0].style, Style::new());
    }

    #[test]
    fn empty_input_is_one_empty_line() {
        assert_eq!(parse("").len(), 1);
    }

    #[test]
    fn sgr_colors_and_reset() {
        let lines = parse("\x1b[32mgreen\x1b[0m plain");
        assert_eq!(span_texts(&lines[0]), ["green", " plain"]);
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Green));
        assert_eq!(lines[0].spans[1].style, Style::new());
    }

    #[test]
    fn sgr_combos_accumulate() {
        let lines = parse("\x1b[1;91mhot\x1b[22m still-red");
        let s0 = lines[0].spans[0].style;
        assert_eq!(s0.fg, Some(Color::LightRed));
        assert!(s0.add_modifier.contains(Modifier::BOLD));
        let s1 = lines[0].spans[1].style;
        assert_eq!(s1.fg, Some(Color::LightRed));
        assert!(!s1.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn sgr_indexed_and_truecolor() {
        let lines = parse("\x1b[38;5;208morange\x1b[48;2;10;20;30mbg");
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Indexed(208)));
        assert_eq!(lines[0].spans[1].style.bg, Some(Color::Rgb(10, 20, 30)));
    }

    #[test]
    fn non_sgr_sequences_are_stripped() {
        let lines = parse("a\x1b[2Jb\x1b[12;40Hc\x1b]0;title\x07d\re");
        assert_eq!(span_texts(&lines[0]), ["abcde"]);
    }

    #[test]
    fn empty_sgr_params_mean_reset() {
        let lines = parse("\x1b[31mred\x1b[mplain");
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Red));
        assert_eq!(lines[0].spans[1].style, Style::new());
    }
}
