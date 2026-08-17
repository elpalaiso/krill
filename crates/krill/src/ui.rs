//! M1 TUI. M1a: read-only dashboard (list + live preview + attach).
//! M1b: actions — `n` new-session prompt, `d` diff, `x` remove modal.
//! M1c: polish — `/` name filter, colored preview (capture-pane -e).
//! Design doc §8.1. The TUI is a hub — heavy views (attach, diff)
//! suspend the TUI and delegate to tmux / git's pager, then resume.

use crate::ansi;
use crate::commands;
use crate::msg as m;
use krill_core::config::Config;
use krill_core::error::Result;
use krill_core::git;
use krill_core::session::{self, SessionMeta, Status};
use krill_core::tmux;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use ratatui::Frame;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const REFRESH_EVERY: Duration = Duration::from_millis(2000);
const DIFF_CACHE_TTL: Duration = Duration::from_secs(5);

struct Row {
    meta: SessionMeta,
    health: Status,
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

/// §8.1 sort priority: needs-you first, dead last.
fn state_rank(h: Status) -> u8 {
    match h {
        Status::NeedsYou => 0,
        Status::Active => 1,
        Status::Quiet => 2,
        Status::Done => 3,
        Status::Dead => 4,
    }
}

fn icon(h: Status) -> (&'static str, Color) {
    match h {
        Status::NeedsYou => ("◆", Color::Magenta),
        Status::Active => ("●", Color::Green),
        Status::Quiet => ("◌", Color::Yellow),
        Status::Done => ("✓", Color::Blue),
        Status::Dead => ("✖", Color::Red),
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

/// Case-insensitive substring filter on the session name ("" = all).
fn matches_filter(name: &str, query: &str) -> bool {
    query.is_empty() || name.to_lowercase().contains(&query.to_lowercase())
}

/// Group headers only when more than one repo is present.
fn build_items(rows: &[Row]) -> Vec<Item> {
    let multi = {
        let mut uniq: Vec<&str> = rows.iter().map(|r| r.meta.repo_name.as_str()).collect();
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

// ---- new-session prompt (3 steps, bottom line) ------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum NewStep {
    Name,
    Agent,
    Message,
}

#[derive(Debug)]
struct NewPrompt {
    step: NewStep,
    name: String,
    agents: Vec<String>,
    agent_idx: usize,
    message: String,
}

#[derive(Debug, PartialEq)]
enum PromptOutcome {
    /// Name failed validation — stay on the name step.
    Invalid,
    /// Advanced to the next step.
    Next,
    /// All inputs collected: (name, agent, message).
    Done(String, String, Option<String>),
}

impl NewPrompt {
    fn start() -> Result<NewPrompt> {
        let config = Config::load()?;
        // Surface "no agents" through the same error resolve_agent gives.
        config.resolve_agent(None).or_else(|e| {
            if config.agents.is_empty() { Err(e) } else { config.resolve_agent(config.agents.keys().next().map(String::as_str)) }
        })?;
        let agents: Vec<String> = config.agents.keys().cloned().collect();
        let agent_idx = config
            .default_agent
            .as_ref()
            .and_then(|d| agents.iter().position(|a| a == d))
            .unwrap_or(0);
        Ok(NewPrompt {
            step: NewStep::Name,
            name: String::new(),
            agents,
            agent_idx,
            message: String::new(),
        })
    }

    fn on_char(&mut self, c: char) {
        match self.step {
            NewStep::Name => self.name.push(c),
            NewStep::Agent => {} // pick with Tab
            NewStep::Message => self.message.push(c),
        }
    }

    fn backspace(&mut self) {
        match self.step {
            NewStep::Name => {
                self.name.pop();
            }
            NewStep::Agent => {}
            NewStep::Message => {
                self.message.pop();
            }
        }
    }

    fn tab(&mut self) {
        if self.step == NewStep::Agent && !self.agents.is_empty() {
            self.agent_idx = (self.agent_idx + 1) % self.agents.len();
        }
    }

    fn enter(&mut self) -> PromptOutcome {
        match self.step {
            NewStep::Name => {
                if krill_core::valid_name(&self.name) {
                    self.step = NewStep::Agent;
                    PromptOutcome::Next
                } else {
                    PromptOutcome::Invalid
                }
            }
            NewStep::Agent => {
                self.step = NewStep::Message;
                PromptOutcome::Next
            }
            NewStep::Message => {
                let message = if self.message.trim().is_empty() {
                    None
                } else {
                    Some(self.message.clone())
                };
                PromptOutcome::Done(
                    self.name.clone(),
                    self.agents[self.agent_idx].clone(),
                    message,
                )
            }
        }
    }

    /// (label, value, key-hint) for the bottom prompt line.
    fn line_parts(&self) -> (String, String, String) {
        match self.step {
            NewStep::Name => (m::tui_new_name(), self.name.clone(), m::tui_new_esc()),
            NewStep::Agent => (
                m::tui_new_agent(),
                self.agents.get(self.agent_idx).cloned().unwrap_or_default(),
                m::tui_new_tab(),
            ),
            NewStep::Message => (m::tui_new_message(), self.message.clone(), m::tui_new_enter()),
        }
    }
}

// ---- app --------------------------------------------------------------------

enum Mode {
    Normal,
    Help,
    ConfirmRm,
    New(NewPrompt),
    Filter,
}

enum Action {
    None,
    Quit,
    Attach(String),
    Diff { worktree: PathBuf, base: String, stat: bool },
}

struct App {
    rows: Vec<Row>,
    items: Vec<Item>,
    selected: usize,
    filter: String,
    preview: Vec<Line<'static>>,
    scroll: u16,
    mode: Mode,
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
            filter: String::new(),
            preview: Vec::new(),
            scroll: 0,
            mode: Mode::Normal,
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
            if !matches_filter(&meta.name, &self.filter) {
                continue;
            }
            let (health, age) = session::status(&meta, &live);
            let diff = if health == Status::Dead {
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
            None => Vec::new(),
            Some(r) if r.health == Status::Dead => vec![Line::raw(m::attach_dead(&r.meta.name))],
            Some(r) => match tmux::capture_pane_ansi(&r.meta.tmux) {
                Ok(text) if !text.is_empty() => ansi::parse(&text),
                _ => vec![Line::raw(m::tui_no_output())],
            },
        };
        self.scroll = self.scroll.min((self.preview.len() as u16).saturating_sub(1));
    }

    fn select(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() - 1;
        self.selected = self.selected.saturating_add_signed(delta).min(last);
        self.scroll = 0;
        self.update_preview();
    }

    fn select_by_name(&mut self, name: &str) {
        if let Some(i) = self.rows.iter().position(|r| r.meta.name == name) {
            self.selected = i;
            self.scroll = 0;
            self.update_preview();
        }
    }

    fn create_new(&mut self, name: &str, agent: &str, message: Option<&str>) {
        match commands::create_session(name, Some(agent), None, message, None) {
            Ok(meta) => {
                self.flash = Some(format!("{} {}", meta.name, m::session_started()));
                let _ = self.refresh();
                self.select_by_name(name);
            }
            Err(e) => self.flash = Some(e.to_string()),
        }
    }

    fn do_remove(&mut self, force: bool) {
        let Some(r) = self.rows.get(self.selected) else {
            return;
        };
        let meta = r.meta.clone();
        match commands::remove_session(&meta, force) {
            Ok(warning) => {
                self.flash = Some(warning.unwrap_or_else(|| m::rm_done(&meta.name)));
                self.diff_cache.remove(&meta.id());
                let _ = self.refresh();
            }
            Err(e) => self.flash = Some(e.to_string()),
        }
    }

    fn handle_events(&mut self) -> Result<Action> {
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    return self.on_key(key.code, key.modifiers);
                }
            }
        }
        if self.last_refresh.elapsed() >= REFRESH_EVERY {
            self.refresh()?;
        }
        Ok(Action::None)
    }

    fn on_key(&mut self, code: KeyCode, mods: KeyModifiers) -> Result<Action> {
        self.flash = None;
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            // Any key closes the help overlay.
            Mode::Help => Ok(Action::None),

            Mode::ConfirmRm => {
                match code {
                    KeyCode::Char('y') => self.do_remove(false),
                    KeyCode::Char('f') => self.do_remove(true),
                    KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('q') => {}
                    _ => self.mode = Mode::ConfirmRm, // ignore other keys
                }
                Ok(Action::None)
            }

            Mode::New(mut p) => {
                match code {
                    KeyCode::Esc => {} // cancel: mode already back to Normal
                    KeyCode::Enter => match p.enter() {
                        PromptOutcome::Invalid => {
                            self.flash = Some(m::invalid_session_name(&p.name));
                            self.mode = Mode::New(p);
                        }
                        PromptOutcome::Next => self.mode = Mode::New(p),
                        PromptOutcome::Done(name, agent, message) => {
                            self.create_new(&name, &agent, message.as_deref())
                        }
                    },
                    KeyCode::Tab => {
                        p.tab();
                        self.mode = Mode::New(p);
                    }
                    KeyCode::Backspace => {
                        p.backspace();
                        self.mode = Mode::New(p);
                    }
                    KeyCode::Char(c) => {
                        p.on_char(c);
                        self.mode = Mode::New(p);
                    }
                    _ => self.mode = Mode::New(p),
                }
                Ok(Action::None)
            }

            Mode::Filter => {
                match code {
                    KeyCode::Enter => {} // keep the filter, back to Normal
                    KeyCode::Esc => {
                        self.filter.clear();
                        self.refresh()?;
                    }
                    KeyCode::Backspace => {
                        self.filter.pop();
                        self.mode = Mode::Filter;
                        self.refresh()?;
                    }
                    KeyCode::Char(c) => {
                        self.filter.push(c);
                        self.mode = Mode::Filter;
                        self.refresh()?;
                    }
                    _ => self.mode = Mode::Filter,
                }
                Ok(Action::None)
            }

            Mode::Normal => {
                match code {
                    KeyCode::Char('q') => return Ok(Action::Quit),
                    KeyCode::Char('c') if mods.contains(KeyModifiers::CONTROL) => {
                        return Ok(Action::Quit)
                    }
                    KeyCode::Char('?') => self.mode = Mode::Help,
                    KeyCode::Char('j') | KeyCode::Down => self.select(1),
                    KeyCode::Char('k') | KeyCode::Up => self.select(-1),
                    KeyCode::Char('J') | KeyCode::PageDown => {
                        let max = self.preview.len() as u16;
                        self.scroll = self.scroll.saturating_add(3).min(max.saturating_sub(1));
                    }
                    KeyCode::Char('K') | KeyCode::PageUp => {
                        self.scroll = self.scroll.saturating_sub(3)
                    }
                    KeyCode::Char('r') => self.refresh()?,
                    KeyCode::Char('/') => self.mode = Mode::Filter,
                    KeyCode::Esc => {
                        if !self.filter.is_empty() {
                            self.filter.clear();
                            self.refresh()?;
                        }
                    }
                    KeyCode::Char('n') => match NewPrompt::start() {
                        Ok(p) => self.mode = Mode::New(p),
                        Err(e) => self.flash = Some(e.to_string()),
                    },
                    KeyCode::Char('x') => {
                        if !self.rows.is_empty() {
                            self.mode = Mode::ConfirmRm;
                        }
                    }
                    KeyCode::Char(c @ ('d' | 'D')) => {
                        if let Some(r) = self.rows.get(self.selected) {
                            if r.meta.worktree.exists() {
                                return Ok(Action::Diff {
                                    worktree: r.meta.worktree.clone(),
                                    base: r.meta.base.clone(),
                                    stat: c == 'D',
                                });
                            }
                            self.flash = Some(m::worktree_missing(
                                &r.meta.worktree.display().to_string(),
                            ));
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(r) = self.rows.get(self.selected) {
                            if r.health != Status::Dead {
                                return Ok(Action::Attach(r.meta.tmux.clone()));
                            }
                        }
                    }
                    _ => {}
                }
                Ok(Action::None)
            }
        }
    }

    // ---- rendering ----------------------------------------------------------

    fn title(&self) -> String {
        if self.filter.is_empty() {
            " krill ".into()
        } else {
            format!(" krill /{} ", self.filter)
        }
    }

    fn render(&self, f: &mut Frame) {
        let [body, bottom] =
            Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(f.area());

        if self.rows.is_empty() {
            let text = format!("\n  {}\n\n  {}", m::ls_empty(), m::ls_hint());
            f.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .block(Block::bordered().title(self.title())),
                body,
            );
        } else {
            let [left, right] =
                Layout::horizontal([Constraint::Length(38), Constraint::Min(24)]).areas(body);
            self.render_list(f, left);
            self.render_preview(f, right);
        }

        self.render_bottom(f, bottom);

        match &self.mode {
            Mode::Help => self.render_help(f),
            Mode::ConfirmRm => self.render_confirm(f),
            _ => {}
        }
    }

    fn render_bottom(&self, f: &mut Frame, area: Rect) {
        let line = if let Mode::New(p) = &self.mode {
            let (label, value, hint) = p.line_parts();
            Line::from(vec![
                Span::styled(format!(" {label}"), Style::new().add_modifier(Modifier::BOLD)),
                Span::raw(value),
                Span::styled("▏", Style::new().add_modifier(Modifier::SLOW_BLINK)),
                Span::styled(format!("  {hint}"), Style::new().add_modifier(Modifier::DIM)),
            ])
        } else if matches!(self.mode, Mode::Filter) {
            Line::from(vec![
                Span::styled(" /", Style::new().add_modifier(Modifier::BOLD)),
                Span::raw(self.filter.clone()),
                Span::styled("▏", Style::new().add_modifier(Modifier::SLOW_BLINK)),
                Span::styled(
                    format!("  {}", m::tui_filter_hint()),
                    Style::new().add_modifier(Modifier::DIM),
                ),
            ])
        } else if let Some(err) = &self.flash {
            Line::styled(format!(" {err}"), Style::new().fg(Color::Red))
        } else {
            Line::styled(
                format!(" {}", m::tui_hint()),
                Style::new().add_modifier(Modifier::DIM),
            )
        };
        f.render_widget(Paragraph::new(line), area);
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
            Paragraph::new(lines).block(Block::bordered().title(self.title())),
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
            Paragraph::new(self.preview.clone())
                .scroll((self.scroll, 0))
                .block(block),
            area,
        );
    }

    fn render_overlay(&self, f: &mut Frame, title: String, body: Vec<Line>) {
        let w = (body
            .iter()
            .map(|l| l.iter().map(|s| display_width(&s.content)).sum::<usize>())
            .max()
            .unwrap_or(0) as u16
            + 4)
            .min(f.area().width);
        let h = (body.len() as u16 + 2).min(f.area().height);
        let area = f.area();
        let rect = Rect {
            x: area.x + (area.width.saturating_sub(w)) / 2,
            y: area.y + (area.height.saturating_sub(h)) / 2,
            width: w,
            height: h,
        };
        f.render_widget(Clear, rect);
        f.render_widget(
            Paragraph::new(body).block(Block::bordered().title(format!(" {title} "))),
            rect,
        );
    }

    fn render_help(&self, f: &mut Frame) {
        let body: Vec<Line> = m::tui_help_body().lines().map(|l| Line::raw(l.to_string())).collect();
        self.render_overlay(f, m::tui_help_title(), body);
    }

    fn render_confirm(&self, f: &mut Frame) {
        let Some(r) = self.rows.get(self.selected) else {
            return;
        };
        let mut body = vec![
            Line::raw(""),
            Line::raw(format!(" {}", m::tui_rm_body(&r.meta.name, &r.meta.branch))),
        ];
        if r.diff != "clean" && r.diff != "-" {
            body.push(Line::raw(""));
            body.push(Line::styled(
                format!(" {}", m::tui_rm_dirty(&r.diff)),
                Style::new().fg(Color::Yellow),
            ));
        }
        body.push(Line::raw(""));
        body.push(Line::styled(
            format!("   {}", m::tui_rm_keys()),
            Style::new().add_modifier(Modifier::BOLD),
        ));
        body.push(Line::raw(""));
        self.render_overlay(f, m::tui_rm_title(), body);
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
            Ok(Action::Diff { worktree, base, stat }) => {
                // Suspend and let git render the diff with its own pager.
                ratatui::restore();
                let diffed = commands::diff_worktree(&worktree, &base, stat, true);
                terminal = ratatui::init();
                let _ = terminal.clear();
                if let Err(e) = diffed {
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

    fn row(name: &str, repo: &str, health: Status, age: Option<u64>, created: u64) -> Row {
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
            row("dead-new", "a", Status::Dead, None, 300),
            row("quiet", "a", Status::Quiet, Some(120), 100),
            row("active-old", "a", Status::Active, Some(20), 50),
            row("active-hot", "a", Status::Active, Some(2), 10),
            row("dead-old", "a", Status::Dead, None, 200),
        ];
        sort_rows(&mut rows);
        assert_eq!(names(&rows), ["active-hot", "active-old", "quiet", "dead-new", "dead-old"]);
    }

    #[test]
    fn sort_groups_by_repo_first() {
        let mut rows = vec![
            row("z", "web", Status::Active, Some(1), 1),
            row("a", "api", Status::Dead, None, 1),
        ];
        sort_rows(&mut rows);
        assert_eq!(names(&rows), ["a", "z"]); // repo "api" group before "web"
    }

    #[test]
    fn headers_only_with_multiple_repos() {
        let mut multi = vec![
            row("s1", "api", Status::Active, Some(1), 1),
            row("s2", "web", Status::Active, Some(1), 1),
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

        let single = vec![row("s1", "api", Status::Active, Some(1), 1)];
        assert_eq!(build_items(&single), vec![Item::Row(0)]);
    }

    #[test]
    fn icons_and_ranks_cover_all_states() {
        assert!(state_rank(Status::NeedsYou) < state_rank(Status::Active));
        assert!(state_rank(Status::Active) < state_rank(Status::Quiet));
        assert!(state_rank(Status::Quiet) < state_rank(Status::Done));
        assert!(state_rank(Status::Done) < state_rank(Status::Dead));
        assert_eq!(icon(Status::NeedsYou).0, "◆");
        assert_eq!(icon(Status::Active).0, "●");
        assert_eq!(icon(Status::Quiet).0, "◌");
        assert_eq!(icon(Status::Done).0, "✓");
        assert_eq!(icon(Status::Dead).0, "✖");
    }

    #[test]
    fn clip_truncates_with_ellipsis() {
        assert_eq!(clip("short", 13), "short");
        assert_eq!(clip("exactly-13-ch", 13), "exactly-13-ch");
        assert_eq!(clip("much-longer-than-that", 13), "much-longer-…");
    }

    #[test]
    fn filter_is_case_insensitive_substring() {
        assert!(matches_filter("fix-login", ""));
        assert!(matches_filter("fix-login", "log"));
        assert!(matches_filter("Fix-Login", "fix-l"));
        assert!(!matches_filter("fix-login", "xyz"));
    }

    #[test]
    fn display_width_counts_cjk_as_two() {
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("한글"), 4);
        assert_eq!(display_width("a한b글c"), 7);
        assert_eq!(display_width(""), 0);
    }

    // ---- new-session prompt state machine ----

    fn prompt() -> NewPrompt {
        NewPrompt {
            step: NewStep::Name,
            name: String::new(),
            agents: vec!["claude".into(), "codex".into(), "shell".into()],
            agent_idx: 0,
            message: String::new(),
        }
    }

    #[test]
    fn prompt_walks_name_agent_message() {
        let mut p = prompt();
        for c in "fix-1".chars() {
            p.on_char(c);
        }
        assert_eq!(p.enter(), PromptOutcome::Next);
        assert_eq!(p.step, NewStep::Agent);

        p.on_char('z'); // typing is ignored on the agent step
        p.tab();
        p.tab();
        assert_eq!(p.agents[p.agent_idx], "shell");
        assert_eq!(p.enter(), PromptOutcome::Next);

        for c in "do it".chars() {
            p.on_char(c);
        }
        assert_eq!(
            p.enter(),
            PromptOutcome::Done("fix-1".into(), "shell".into(), Some("do it".into()))
        );
    }

    #[test]
    fn prompt_rejects_invalid_name_and_allows_fix() {
        let mut p = prompt();
        for c in "bad.name".chars() {
            p.on_char(c);
        }
        assert_eq!(p.enter(), PromptOutcome::Invalid);
        assert_eq!(p.step, NewStep::Name); // still on the name step
        for _ in 0..5 {
            p.backspace();
        }
        assert_eq!(p.name, "bad");
        assert_eq!(p.enter(), PromptOutcome::Next);
    }

    #[test]
    fn prompt_empty_message_means_shell_only() {
        let mut p = prompt();
        for c in "s1".chars() {
            p.on_char(c);
        }
        assert_eq!(p.enter(), PromptOutcome::Next);
        assert_eq!(p.enter(), PromptOutcome::Next);
        p.on_char(' '); // whitespace-only counts as empty
        assert_eq!(
            p.enter(),
            PromptOutcome::Done("s1".into(), "claude".into(), None)
        );
    }

    #[test]
    fn prompt_tab_wraps_around() {
        let mut p = prompt();
        p.step = NewStep::Agent;
        p.tab();
        p.tab();
        p.tab();
        assert_eq!(p.agent_idx, 0); // cycled back to the first agent
    }
}
