use std::collections::HashSet;
use std::fmt;
use std::io::{self, Write};

use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::execute;
use ratatui::backend::Backend;
use ratatui::buffer::Buffer;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget};
use ratatui::{Frame, Terminal, TerminalOptions, Viewport};

pub const FIXED_LIVE_REGION_HEIGHT: u16 = 1 + 9 + 3 + 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveEntryKind {
    Assistant,
    Bash,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platform {
    pub is_windows: bool,
    pub wt_session: bool,
}

impl Default for Platform {
    fn default() -> Self {
        Self::current()
    }
}

impl Platform {
    pub fn current() -> Self {
        Self {
            is_windows: cfg!(windows),
            wt_session: std::env::var_os("WT_SESSION").is_some(),
        }
    }
}

pub fn live_preview_enabled(kind: LiveEntryKind, platform: Platform) -> bool {
    match kind {
        LiveEntryKind::Assistant => !platform.is_windows && !platform.wt_session,
        LiveEntryKind::Bash | LiveEntryKind::Other => true,
    }
}

pub fn terminal_options() -> TerminalOptions {
    TerminalOptions {
        viewport: Viewport::Inline(FIXED_LIVE_REGION_HEIGHT),
    }
}

pub fn enter_main_screen<W: Write>(writer: &mut W) -> io::Result<()> {
    execute!(writer, EnableBracketedPaste)
}

pub fn restore_main_screen<W: Write>(writer: &mut W) -> io::Result<()> {
    execute!(writer, DisableBracketedPaste)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenModelError {
    DuplicateLiveEntry(String),
    AlreadyCommitted(String),
}

impl fmt::Display for ScreenModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateLiveEntry(id) => write!(formatter, "live entry '{id}' already exists"),
            Self::AlreadyCommitted(id) => write!(formatter, "entry '{id}' is already committed"),
        }
    }
}

impl std::error::Error for ScreenModelError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedEntry {
    id: String,
    lines: Vec<Line<'static>>,
}

impl CommittedEntry {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn lines(&self) -> &[Line<'static>] {
        &self.lines
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveEntry {
    id: String,
    kind: LiveEntryKind,
    lines: Vec<Line<'static>>,
}

impl LiveEntry {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn kind(&self) -> LiveEntryKind {
        self.kind
    }

    pub fn lines(&self) -> &[Line<'static>] {
        &self.lines
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenEntry {
    id: String,
    kind: LiveEntryKind,
    lines: Vec<Line<'static>>,
    completed: bool,
}

impl ScreenEntry {
    pub fn new(
        id: impl Into<String>,
        kind: LiveEntryKind,
        lines: Vec<Line<'static>>,
        completed: bool,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            lines: non_empty_lines(lines),
            completed,
        }
    }
}

#[derive(Debug, Default)]
pub struct ScreenModel {
    committed: Vec<CommittedEntry>,
    live: Vec<LiveEntry>,
}

impl ScreenModel {
    pub fn committed_entries(&self) -> &[CommittedEntry] {
        &self.committed
    }

    pub fn live_entries(&self) -> &[LiveEntry] {
        &self.live
    }

    pub fn start_live(
        &mut self,
        id: impl Into<String>,
        kind: LiveEntryKind,
        lines: Vec<Line<'static>>,
    ) -> Result<(), ScreenModelError> {
        let id = id.into();
        if self.committed.iter().any(|entry| entry.id == id) {
            return Err(ScreenModelError::AlreadyCommitted(id));
        }
        if self.live.iter().any(|entry| entry.id == id) {
            return Err(ScreenModelError::DuplicateLiveEntry(id));
        }
        self.live.push(LiveEntry {
            id,
            kind,
            lines: non_empty_lines(lines),
        });
        Ok(())
    }

    pub fn update_live(&mut self, id: &str, lines: Vec<Line<'static>>) -> bool {
        let Some(entry) = self.live.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        entry.lines = non_empty_lines(lines);
        true
    }

    pub fn sync<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
        entries: &[ScreenEntry],
    ) -> io::Result<()> {
        let active_ids = entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<HashSet<_>>();
        self.live
            .retain(|entry| active_ids.contains(entry.id.as_str()));

        for entry in entries {
            if self
                .committed
                .iter()
                .any(|committed| committed.id == entry.id)
            {
                continue;
            }

            if !self.update_live(&entry.id, entry.lines.clone()) {
                self.start_live(&entry.id, entry.kind, entry.lines.clone())
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
            }
            if entry.completed {
                self.commit_live(terminal, &entry.id)?;
            }
        }
        Ok(())
    }

    pub fn commit_live<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
        id: &str,
    ) -> io::Result<bool> {
        let Some(index) = self.live.iter().position(|entry| entry.id == id) else {
            return Ok(false);
        };
        let entry = self.live[index].clone();
        let height = u16::try_from(entry.lines.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "a transcript entry is too tall for the terminal viewport",
            )
        })?;
        terminal.insert_before(height, |buffer| render_lines(&entry.lines, buffer))?;
        self.live.remove(index);
        self.committed.push(CommittedEntry {
            id: entry.id,
            lines: entry.lines,
        });
        Ok(true)
    }

    pub fn draw_live(&self, frame: &mut Frame, platform: Platform) {
        frame.render_widget(
            Paragraph::new(self.visible_live_lines(platform, frame.area().height as usize)),
            frame.area(),
        );
    }

    pub fn live_lines(&self, platform: Platform) -> Vec<Line<'static>> {
        self.live
            .iter()
            .filter(|entry| live_preview_enabled(entry.kind, platform))
            .flat_map(|entry| entry.lines.iter().cloned())
            .collect()
    }

    pub fn visible_live_lines(&self, platform: Platform, max_rows: usize) -> Vec<Line<'static>> {
        self.live_lines(platform)
            .into_iter()
            .take(max_rows)
            .collect()
    }
}

fn non_empty_lines(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    if lines.is_empty() {
        vec![Line::default()]
    } else {
        lines
    }
}

fn render_lines(lines: &[Line<'static>], buffer: &mut Buffer) {
    Paragraph::new(lines.to_vec()).render(buffer.area, buffer);
}
