//! M1a TUI: read-only dashboard (list + live preview + attach + quit).
//! Design doc §8.1. The TUI is a hub — heavy views (attach) suspend the
//! TUI and delegate to tmux, then resume. Actions (new/diff/rm) are M1b.

use crate::msg as m;
use krill_core::error::Result;
use krill_core::git;
use krill_core::session::{self, Health, SessionMeta};
use krill_core::tmux;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use ratatui::Frame;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const REFRESH_EVERY: Duration = Duration::from_millis(2000);
const DIFF_CACHE_TTL: Duration = Duration::from_secs(5);

struct Row {
    meta: SessionMeta,
    health: Health,
    age: Option<u64>,
    diff: String,
}

/// Left-pane display items: repo group headers interleaved with rows
/// (indices into the sorted row vec).
#[derive(Debug, PartialEq)]
enum Item {
    Header(String),
    Row(usize),
}

fn state_rank(h: Health) -> u8 {
    match h {
        Health::Active => 0,
        Health::Quiet => 1,
        Health::Dead => 2,
    }
}

fn icon(h: Health) -> (&'static str, Color) {
    match h {
        Health::Active => ("●", Color::Green),
        Health::Quiet => ("◌", Color::Yellow),
        Health::Dead => ("✖", Color::Red),
    }
}

/// §8.1 sort rule: repo group → state priority → most recent activity
/// (dead sessions: newest created first).
fn sort_rows(rows: &mut [Row]) {
    rows.sort_by(|a, b| {
        (
            &a.meta.repo_name,
            state_rank(a.health),
            a.age.unwrap_or(u64::MAX),
            std::cmp::Reverse(a.meta.created_unix),
        )
            .cmp(&(
                &b.meta.repo_name,
                state_rank(b.health),
                b.age.unwrap_or(u64::MAX),
                std::cmp::Reverse(b.meta.created_unix),
            ))
    });
}

/// Group headers only when more than one repo is present.
fn build_items(rows: &[Row]) -> Vec<Item> {
    let mut repos: Vec<&str> = rows.iter().map(|r| r.meta.repo_name.as_str()).collect();
    repos.dedup();
    let multi = {
        let mut uniq = repos.clone();
        uniq.sort_unstable();
        uniq.dedup();
        uniq.len() > 1
    };
    let mut items = Vec::new();
    let mut last_repo: Option<&str> = None;
    for (i, r) in rows.iter().enumerate() {
        if multi && last_repo != Some(r.meta.repo_name.as_str()) {
            items.push(Item::Header(r.meta.repo_name.clone()));
            last_repo = Some(r.meta.repo_name.as_str());
        }
        items.push(Item::Row(i));
    }
    items
}

/// Terminal display width: CJK/Hangul/fullwidth chars take 2 columns.
/// (A tiny subset of UAX #11 — enough for ko/en text.)
fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| match c as u32 {
            0x1100..=0x115F // Hangul Jamo
            | 0x2E80..=0xA4CF // CJK radicals..Yi
            | 0xAC00..=0xD7A3 // Hangul syllables
            | 0xF900..=0xFAFF // CJK compat ideographs
            | 0xFF00..=0xFF60 // fullwidth forms
            | 0xFFE0..=0xFFE6 => 2,
            _ => 1,
        })
        .sum()
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
    }
}

enum Action {
    None,
    Quit,
    Attach(String),
}

struct App {
    rows: Vec<Row>,
    items: Vec<Item>,
    selected: usize,
    preview: String,
    scroll: u16,
    show_help: bool,
    flash: Option<String>,
    last_refresh: Instant,
    diff_cache: HashMap<String, (Instant, String)>,
}

impl App {
    fn new() -> Result<App> {
        let mut app = App {
            rows: Vec::new(),
            items: Vec::new(),
            selected: 0,
            preview: String::new(),
            scroll: 0,
            show_help: false,
            flash: None,
            last_refresh: Instant::now(),
            diff_cache: HashMap::new(),
        };
        app.refresh()?;
        Ok(app)
    }

    fn refresh(&mut self) -> Result<()> {
        let metas = session::load_all()?;
        let live = tmux::server_sessions();
        let mut rows = Vec::new();
        for meta in metas {
            let (health, age) = session::health(&meta, &live);
            let diff = if health == Health::Dead {
                "-".into()
            } else {
                self.diff_stat(&meta)
            };
            rows.push(Row { meta, health, age, diff });
        }
        sort_rows(&mut rows);
        self.items = build_items(&rows);
        self.rows = rows;
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        self.update_preview();
        self.last_refresh = Instant::now();
        Ok(())
    }

    /// shortstat spawns git, so cache per session for a few seconds.
    fn diff_stat(&mut self, meta: &SessionMeta) -> String {
        let id = meta.id();
        if let Some((at, cached)) = self.diff_cache.get(&id) {
            if at.elapsed() < DIFF_CACHE_TTL {
                return cached.clone();
            }
        }
        let stat = git::shortstat(&meta.worktree, &meta.base);
        self.diff_cache.insert(id, (Instant::now(), stat.clone()));
        stat
    }

    fn update_preview(&mut self) {
        self.preview = match self.rows.get(self.selected) {
            None => String::new(),
            Some(r) if r.health == Health::Dead => m::attach_dead(&r.meta.name),
            Some(r) => match tmux::capture_pane(&r.meta.tmux) {
                Ok(text) if !text.is_empty() => text,
                _ => m::tui_no_output(),
            },
        };
        let lines = self.preview.lines().count() as u16;
        self.scroll = self.scroll.min(lines.saturating_sub(1));
    }

    fn select(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() - 1;
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(last);
        self.scroll = 0;
        self.update_preview();
    }

    fn handle_events(&mut self) -> Result<Action> {
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    self.flash = None;
                    if self.show_help {
                        self.show_help = false;
                        return Ok(Action::None);
                    }
                    match key.code {
                        KeyCode::Char('q') => return Ok(Action::Quit),
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            return Ok(Action::Quit)
                        }
                        KeyCode::Char('?') => self.show_help = true,
                        KeyCode::Char('j') | KeyCode::Down => self.select(1),
                        KeyCode::Char('k') | KeyCode::Up => self.select(-1),
                        KeyCode::Char('J') | KeyCode::PageDown => {
                            self.scroll = self.scroll.saturating_add(3);
                            let max = self.preview.lines().count() as u16;
                            self.scroll = self.scroll.min(max.saturating_sub(1));
                        }
                        KeyCode::Char('K') | KeyCode::PageUp => {
                            self.scroll = self.scroll.saturating_sub(3)
                        }
                        KeyCode::Char('r') => self.refresh()?,
                        KeyCode::Enter => {
                            if let Some(r) = self.rows.get(self.selected) {
                                if r.health != Health::Dead {
                                    return Ok(Action::Attach(r.meta.tmux.clone()));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if self.last_refresh.elapsed() >= REFRESH_EVERY {
            self.refresh()?;
        }
        Ok(Action::None)
    }

    fn render(&self, f: &mut Frame) {
        let [body, hint] =
            Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(f.area());

        if self.rows.is_empty() {
            let text = format!("\n  {}\n\n  {}", m::ls_empty(), m::ls_hint());
            f.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .block(Block::bordered().title(" krill ")),
                body,
            );
        } else {
            let [left, right] =
                Layout::horizontal([Constraint::Length(38), Constraint::Min(24)]).areas(body);
            self.render_list(f, left);
            self.render_preview(f, right);
        }

        let hint_line = match &self.flash {
            Some(err) => Line::styled(format!(" {err}"), Style::new().fg(Color::Red)),
            None => Line::styled(
                format!(" {}", m::tui_hint()),
                Style::new().add_modifier(Modifier::DIM),
            ),
        };
        f.render_widget(Paragraph::new(hint_line), hint);

        if self.show_help {
            self.render_help(f);
        }
    }

    fn render_list(&self, f: &mut Frame, area: Rect) {
        let mut lines = Vec::new();
        for item in &self.items {
            match item {
                Item::Header(repo) => lines.push(Line::styled(
                    format!(" {repo}"),
                    Style::new().add_modifier(Modifier::BOLD | Modifier::DIM),
                )),
                Item::Row(i) => {
                    let r = &self.rows[*i];
                    let sel = *i == self.selected;
                    let (dot, color) = icon(r.health);
                    let base = if sel {
                        Style::new().add_modifier(Modifier::REVERSED)
                    } else {
                        Style::new()
                    };
                    let age = r.age.map(krill_core::fmt_age).unwrap_or_else(|| "-".into());
                    lines.push(Line::from(vec![
                        Span::styled(if sel { "▸" } else { " " }, base),
                        Span::styled(dot, base.fg(color)),
                        Span::styled(
                            format!(
                                " {:<13} {:<7} {:>4}  {}",
                                clip(&r.meta.name, 13),
                                clip(&r.meta.agent, 7),
                                age,
                                r.diff
                            ),
                            base,
                        ),
                    ]));
                }
            }
        }
        f.render_widget(
            Paragraph::new(lines).block(Block::bordered().title(" krill ")),
            area,
        );
    }

    fn render_preview(&self, f: &mut Frame, area: Rect) {
        let Some(r) = self.rows.get(self.selected) else {
            return;
        };
        let title = format!(" {} · {} · {} ", r.meta.name, r.meta.agent, r.diff);
        let mut footer = format!(" {} ← {}", r.meta.branch, r.meta.base);
        if let Some(age) = r.age {
            footer.push_str(&format!(" · {}", m::tui_last_output(&krill_core::fmt_age(age))));
        }
        footer.push(' ');
        let block = Block::bordered()
            .title(title)
            .title_bottom(Line::styled(footer, Style::new().add_modifier(Modifier::DIM)));
        f.render_widget(
            Paragraph::new(self.preview.as_str())
                .scroll((self.scroll, 0))
                .block(block),
            area,
        );
    }

    fn render_help(&self, f: &mut Frame) {
        let body = m::tui_help_body();
        let w = (body.lines().map(display_width).max().unwrap_or(0) as u16 + 4)
            .min(f.area().width);
        let h = (body.lines().count() as u16 + 2).min(f.area().height);
        let area = f.area();
        let rect = Rect {
            x: area.x + (area.width.saturating_sub(w)) / 2,
            y: area.y + (area.height.saturating_sub(h)) / 2,
            width: w,
            height: h,
        };
        f.render_widget(Clear, rect);
        f.render_widget(
            Paragraph::new(body)
                .block(Block::bordered().title(format!(" {} ", m::tui_help_title()))),
            rect,
        );
    }
}

pub fn run() -> Result<()> {
    let mut app = App::new()?;
    let mut terminal = ratatui::init();
    let result = loop {
        if let Err(e) = terminal.draw(|f| app.render(f)) {
            break Err(e.into());
        }
        match app.handle_events() {
            Ok(Action::None) => {}
            Ok(Action::Quit) => break Ok(()),
            Ok(Action::Attach(tmux_name)) => {
                // Suspend the TUI, hand the terminal to tmux, resume on detach.
                ratatui::restore();
                let attached = tmux::attach_wait(&tmux_name);
                terminal = ratatui::init();
                let _ = terminal.clear();
                if let Err(e) = attached {
                    app.flash = Some(e.to_string());
                }
                let _ = app.refresh();
            }
            Err(e) => break Err(e),
        }
    };
    ratatui::restore();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn row(name: &str, repo: &str, health: Health, age: Option<u64>, created: u64) -> Row {
        Row {
            meta: SessionMeta {
                name: name.into(),
                repo_name: repo.into(),
                repo_path: PathBuf::from("/r"),
                base: "main".into(),
                branch: format!("krill/{name}"),
                worktree: PathBuf::from("/w").join(name),
                agent: "shell".into(),
                cmd: String::new(),
                tmux: format!("krill_{repo}_{name}"),
                created_unix: created,
            },
            health,
            age,
            diff: "clean".into(),
        }
    }

    fn names(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.meta.name.as_str()).collect()
    }

    #[test]
    fn sort_state_priority_then_recent_activity() {
        let mut rows = vec![
            row("dead-new", "a", Health::Dead, None, 300),
            row("quiet", "a", Health::Quiet, Some(120), 100),
            row("active-old", "a", Health::Active, Some(20), 50),
            row("active-hot", "a", Health::Active, Some(2), 10),
            row("dead-old", "a", Health::Dead, None, 200),
        ];
        sort_rows(&mut rows);
        assert_eq!(names(&rows), ["active-hot", "active-old", "quiet", "dead-new", "dead-old"]);
    }

    #[test]
    fn sort_groups_by_repo_first() {
        let mut rows = vec![
            row("z", "web", Health::Active, Some(1), 1),
            row("a", "api", Health::Dead, None, 1),
        ];
        sort_rows(&mut rows);
        assert_eq!(names(&rows), ["a", "z"]); // repo "api" group before "web"
    }

    #[test]
    fn headers_only_with_multiple_repos() {
        let mut multi = vec![
            row("s1", "api", Health::Active, Some(1), 1),
            row("s2", "web", Health::Active, Some(1), 1),
        ];
        sort_rows(&mut multi);
        assert_eq!(
            build_items(&multi),
            vec![
                Item::Header("api".into()),
                Item::Row(0),
                Item::Header("web".into()),
                Item::Row(1)
            ]
        );

        let single = vec![row("s1", "api", Health::Active, Some(1), 1)];
        assert_eq!(build_items(&single), vec![Item::Row(0)]);
    }

    #[test]
    fn icons_and_ranks_cover_all_states() {
        assert!(state_rank(Health::Active) < state_rank(Health::Quiet));
        assert!(state_rank(Health::Quiet) < state_rank(Health::Dead));
        assert_eq!(icon(Health::Active).0, "●");
        assert_eq!(icon(Health::Quiet).0, "◌");
        assert_eq!(icon(Health::Dead).0, "✖");
    }

    #[test]
    fn clip_truncates_with_ellipsis() {
        assert_eq!(clip("short", 13), "short");
        assert_eq!(clip("exactly-13-ch", 13), "exactly-13-ch");
        assert_eq!(clip("much-longer-than-that", 13), "much-longer-…");
    }

    #[test]
    fn display_width_counts_cjk_as_two() {
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("한글"), 4);
        assert_eq!(display_width("a한b글c"), 7);
        assert_eq!(display_width(""), 0);
    }
}
