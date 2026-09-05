use std::collections::HashSet;
use std::fmt;
use std::io::{self, Write};

use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::execute;
use ratatui::backend::Backend;
use ratatui::buffer::Buffer;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::{Paragraph, Widget};
use ratatui::{Frame, Terminal, TerminalOptions, Viewport};

use crate::ansi::parse_sanitized_ansi;
use crate::markdown::assistant_markdown_lines_for_widths;
use crate::palette;
use crate::tool_rendering::{tool_summary, tool_user_facing_name};
pub use crate::tool_rendering::{
    ToolCallState, ToolHeaderPayload, ToolPlatform, ToolProgressKind, ToolProgressPayload,
    ToolResultPayload, ToolResultState,
};
use crate::transcript_wrap::{wrap_lines, WrapSpec};
use unicode_width::UnicodeWidthStr;

pub const FIXED_LIVE_REGION_HEIGHT: u16 = 1 + 9 + 3 + 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveEntryKind {
    User,
    Assistant,
    AssistantNested,
    Bash,
    Tool,
    ToolNested,
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
        LiveEntryKind::Assistant | LiveEntryKind::AssistantNested => {
            !platform.is_windows && !platform.wt_session
        }
        LiveEntryKind::User
        | LiveEntryKind::Bash
        | LiveEntryKind::Tool
        | LiveEntryKind::ToolNested
        | LiveEntryKind::Other => true,
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
    id: TranscriptEntryId,
    height: u16,
}

impl CommittedEntry {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn height(&self) -> u16 {
        self.height
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveEntry {
    id: TranscriptEntryId,
    kind: LiveEntryKind,
    payload: TranscriptPayload,
    completed: bool,
    revision: u64,
    cached_render: Option<CachedRender>,
}

impl LiveEntry {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn kind(&self) -> LiveEntryKind {
        self.kind
    }

    pub fn lines(&self) -> &[Line<'static>] {
        match &self.payload {
            TranscriptPayload::PreRendered(lines) => lines,
            TranscriptPayload::AssistantMarkdown(_)
            | TranscriptPayload::ToolHeader(_)
            | TranscriptPayload::ToolProgress(_)
            | TranscriptPayload::ToolResult(_) => &[],
        }
    }

    pub fn payload(&self) -> &TranscriptPayload {
        &self.payload
    }

    pub fn completed(&self) -> bool {
        self.completed
    }
}

pub type TranscriptEntryId = String;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptPayload {
    PreRendered(Vec<Line<'static>>),
    AssistantMarkdown(String),
    ToolHeader(ToolHeaderPayload),
    ToolProgress(ToolProgressPayload),
    ToolResult(ToolResultPayload),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CachedRender {
    revision: u64,
    width: usize,
    animation_phase: u64,
    lines: Vec<Line<'static>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenEntry {
    id: TranscriptEntryId,
    kind: LiveEntryKind,
    payload: TranscriptPayload,
    completed: bool,
}

impl ScreenEntry {
    pub fn new(
        id: impl Into<String>,
        kind: LiveEntryKind,
        lines: Vec<Line<'static>>,
        completed: bool,
    ) -> Self {
        Self::with_payload(
            id,
            kind,
            TranscriptPayload::PreRendered(non_empty_lines(lines)),
            completed,
        )
    }

    pub fn with_payload(
        id: impl Into<String>,
        kind: LiveEntryKind,
        payload: TranscriptPayload,
        completed: bool,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            payload,
            completed,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn kind(&self) -> LiveEntryKind {
        self.kind
    }

    pub fn lines(&self) -> &[Line<'static>] {
        match &self.payload {
            TranscriptPayload::PreRendered(lines) => lines,
            TranscriptPayload::AssistantMarkdown(_)
            | TranscriptPayload::ToolHeader(_)
            | TranscriptPayload::ToolProgress(_)
            | TranscriptPayload::ToolResult(_) => &[],
        }
    }

    pub fn payload(&self) -> &TranscriptPayload {
        &self.payload
    }

    pub fn completed(&self) -> bool {
        self.completed
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum ScreenChange {
    Reset,
    Upsert(ScreenEntry),
    Remove(TranscriptEntryId),
}

#[derive(Debug, Default)]
pub struct ScreenModel {
    committed: Vec<CommittedEntry>,
    committed_ids: HashSet<TranscriptEntryId>,
    live: Vec<LiveEntry>,
}

impl ScreenModel {
    pub fn committed_entries(&self) -> &[CommittedEntry] {
        &self.committed
    }

    pub fn live_entries(&self) -> &[LiveEntry] {
        &self.live
    }

    pub fn committed_count(&self) -> usize {
        self.committed.len()
    }

    pub fn is_committed(&self, id: &str) -> bool {
        self.committed_ids.contains(id)
    }

    pub fn reset(&mut self) {
        self.committed.clear();
        self.committed_ids.clear();
        self.live.clear();
    }

    pub fn start_live(
        &mut self,
        id: impl Into<String>,
        kind: LiveEntryKind,
        lines: Vec<Line<'static>>,
    ) -> Result<(), ScreenModelError> {
        let id = id.into();
        if self.committed_ids.contains(&id) {
            return Err(ScreenModelError::AlreadyCommitted(id));
        }
        if self.live.iter().any(|entry| entry.id == id) {
            return Err(ScreenModelError::DuplicateLiveEntry(id));
        }
        self.live.push(LiveEntry {
            id,
            kind,
            payload: TranscriptPayload::PreRendered(non_empty_lines(lines)),
            completed: false,
            revision: 0,
            cached_render: None,
        });
        Ok(())
    }

    pub fn update_live(&mut self, id: &str, lines: Vec<Line<'static>>) -> bool {
        let Some(entry) = self.live.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        entry.payload = TranscriptPayload::PreRendered(non_empty_lines(lines));
        entry.revision = entry.revision.wrapping_add(1);
        entry.cached_render = None;
        true
    }

    pub fn apply_change<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
        change: ScreenChange,
    ) -> io::Result<()> {
        match change {
            ScreenChange::Reset => self.reset(),
            ScreenChange::Remove(id) => {
                if !self.committed_ids.contains(&id) {
                    self.live.retain(|entry| entry.id != id);
                }
                self.commit_ready(terminal)?;
            }
            ScreenChange::Upsert(entry) => {
                if self.committed_ids.contains(&entry.id) {
                    return Ok(());
                }

                if let Some(current) = self.live.iter_mut().find(|current| current.id == entry.id) {
                    current.kind = entry.kind;
                    current.payload = entry.payload;
                    current.completed = entry.completed;
                    current.revision = current.revision.wrapping_add(1);
                    current.cached_render = None;
                } else {
                    self.live.push(LiveEntry {
                        id: entry.id,
                        kind: entry.kind,
                        payload: entry.payload,
                        completed: entry.completed,
                        revision: 0,
                        cached_render: None,
                    });
                }
                self.commit_ready(terminal)?;
            }
        }
        Ok(())
    }

    pub fn commit_live<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
        id: &str,
    ) -> io::Result<bool> {
        let Some(entry) = self.live.first() else {
            return Ok(false);
        };
        if entry.id != id {
            return Ok(false);
        }
        self.commit_index(terminal, 0)?;
        Ok(true)
    }

    fn commit_ready<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> io::Result<()> {
        while self.live.first().is_some_and(|entry| entry.completed) {
            self.commit_index(terminal, 0)?;
        }
        Ok(())
    }

    fn commit_index<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
        index: usize,
    ) -> io::Result<()> {
        let width = terminal.size()?.width as usize;
        let lines = self.live[index]
            .rendered_at_width(width, ToolPlatform::current(), 0)
            .to_vec();
        let height = u16::try_from(lines.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "a transcript entry is too tall for the terminal viewport",
            )
        })?;
        terminal.insert_before(height, |buffer| render_lines(&lines, buffer))?;
        let entry = self.live.remove(index);
        self.committed_ids.insert(entry.id.clone());
        self.committed.push(CommittedEntry {
            id: entry.id,
            height,
        });
        Ok(())
    }

    pub fn draw_live(&mut self, frame: &mut Frame, platform: Platform) {
        self.draw_live_at(frame, platform, 0);
    }

    pub fn draw_live_at(
        &mut self,
        frame: &mut Frame,
        platform: Platform,
        animation_elapsed_ms: u64,
    ) {
        frame.render_widget(
            Paragraph::new(self.visible_live_lines_at_width_with_clock(
                platform,
                frame.area().width as usize,
                frame.area().height as usize,
                animation_elapsed_ms,
            )),
            frame.area(),
        );
    }

    pub fn live_lines(&self, platform: Platform) -> Vec<Line<'static>> {
        self.live
            .iter()
            .filter(|entry| live_preview_enabled(entry.kind, platform))
            .flat_map(|entry| entry.lines().iter().cloned())
            .collect()
    }

    pub fn visible_live_lines(&self, platform: Platform, max_rows: usize) -> Vec<Line<'static>> {
        self.live_lines(platform)
            .into_iter()
            .take(max_rows)
            .collect()
    }

    pub fn live_lines_at_width(&mut self, platform: Platform, width: usize) -> Vec<Line<'static>> {
        self.live_lines_at_width_with_clock(platform, width, 0)
    }

    pub fn live_lines_at_width_with_clock(
        &mut self,
        platform: Platform,
        width: usize,
        animation_elapsed_ms: u64,
    ) -> Vec<Line<'static>> {
        self.live
            .iter_mut()
            .filter(|entry| live_preview_enabled(entry.kind, platform))
            .flat_map(|entry| {
                entry
                    .rendered_at_width(width, ToolPlatform::current(), animation_elapsed_ms)
                    .to_vec()
            })
            .collect()
    }

    pub fn visible_live_lines_at_width(
        &mut self,
        platform: Platform,
        width: usize,
        max_rows: usize,
    ) -> Vec<Line<'static>> {
        self.visible_live_lines_at_width_with_clock(platform, width, max_rows, 0)
    }

    pub fn visible_live_lines_at_width_with_clock(
        &mut self,
        platform: Platform,
        width: usize,
        max_rows: usize,
        animation_elapsed_ms: u64,
    ) -> Vec<Line<'static>> {
        self.live_lines_at_width_with_clock(platform, width, animation_elapsed_ms)
            .into_iter()
            .take(max_rows)
            .collect()
    }
}

impl LiveEntry {
    fn rendered_at_width(
        &mut self,
        width: usize,
        platform: ToolPlatform,
        animation_elapsed_ms: u64,
    ) -> &[Line<'static>] {
        let animation_phase = animation_elapsed_ms / 600;
        let cache_is_current = self.cached_render.as_ref().is_some_and(|cache| {
            cache.revision == self.revision
                && cache.width == width
                && cache.animation_phase == animation_phase
        });
        if !cache_is_current {
            let lines = render_transcript_payload_with_clock(
                self.kind,
                &self.payload,
                width,
                platform,
                animation_elapsed_ms,
            );
            self.cached_render = Some(CachedRender {
                revision: self.revision,
                width,
                animation_phase,
                lines,
            });
        }
        &self
            .cached_render
            .as_ref()
            .expect("live render cache is populated")
            .lines
    }
}

pub fn render_transcript_payload(
    kind: LiveEntryKind,
    payload: &TranscriptPayload,
    width: usize,
) -> Vec<Line<'static>> {
    render_transcript_payload_with_options(kind, payload, width, ToolPlatform::current(), 0, false)
}

pub fn render_transcript_payload_with_clock(
    kind: LiveEntryKind,
    payload: &TranscriptPayload,
    width: usize,
    platform: ToolPlatform,
    animation_elapsed_ms: u64,
) -> Vec<Line<'static>> {
    render_transcript_payload_with_options(
        kind,
        payload,
        width,
        platform,
        animation_elapsed_ms,
        false,
    )
}

pub fn render_transcript_payload_with_options(
    kind: LiveEntryKind,
    payload: &TranscriptPayload,
    width: usize,
    platform: ToolPlatform,
    animation_elapsed_ms: u64,
    verbose: bool,
) -> Vec<Line<'static>> {
    match payload {
        TranscriptPayload::PreRendered(lines) => render_entry_lines(kind, lines, width),
        TranscriptPayload::AssistantMarkdown(content) => {
            let content_width = width.saturating_sub(assistant_prefix_width(kind));
            let lines = assistant_markdown_lines_for_widths(
                content,
                Style::default().fg(palette::TEXT),
                content_width,
                width,
            );
            render_entry_lines(kind, &lines, width)
        }
        TranscriptPayload::ToolHeader(header) => {
            render_tool_header(kind, header, width, platform, animation_elapsed_ms, verbose)
        }
        TranscriptPayload::ToolProgress(progress) => render_tool_progress(kind, progress, width),
        TranscriptPayload::ToolResult(result) => render_tool_result(kind, result, width, verbose),
    }
}

fn render_tool_header(
    kind: LiveEntryKind,
    header: &ToolHeaderPayload,
    width: usize,
    platform: ToolPlatform,
    animation_elapsed_ms: u64,
    verbose: bool,
) -> Vec<Line<'static>> {
    let name = tool_user_facing_name(&header.tool_name);
    if name.is_empty() {
        return Vec::new();
    }
    let summary = tool_summary(
        &header.tool_name,
        header.arguments.as_ref(),
        &header.cwd,
        verbose,
    );
    let dot = match header.state {
        ToolCallState::Running if !tool_dot_visible(animation_elapsed_ms) => " ",
        _ => platform.dot(),
    };
    let dot_style = match header.state {
        ToolCallState::Success => Style::default().fg(palette::SUCCESS),
        ToolCallState::Error | ToolCallState::Cancelled => Style::default().fg(palette::ERROR),
        ToolCallState::Running | ToolCallState::Queued | ToolCallState::Unknown => Style::default()
            .fg(palette::INACTIVE)
            .add_modifier(ratatui::style::Modifier::DIM),
    };
    let mut lines = if name == "Bash" {
        if let Some((first_command, second_command)) = summary.split_once('\n') {
            let mut first_spans = tool_header_prefix(dot, dot_style, &name);
            first_spans.push(Span::raw("("));
            first_spans.push(Span::raw(single_line_content(first_command)));
            let first_line = truncate_line_with_ellipsis(Line::from(first_spans), width);
            let second_line = truncate_line_with_suffix(
                Line::from(vec![
                    Span::raw("  "),
                    Span::raw(single_line_content(second_command)),
                ]),
                width,
                ")",
            );
            vec![first_line, second_line]
        } else {
            vec![render_single_tool_header_line(
                dot, dot_style, &name, &summary, width,
            )]
        }
    } else {
        vec![render_single_tool_header_line(
            dot, dot_style, &name, &summary, width,
        )]
    };
    if tool_is_nested(kind, header.nested()) {
        lines
    } else {
        lines.insert(0, Line::default());
        lines
    }
}

fn tool_header_prefix(dot: &str, dot_style: Style, name: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(dot.to_string(), dot_style),
        Span::styled(" ", dot_style),
        Span::styled(
            name.to_string(),
            Style::default()
                .fg(palette::TEXT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
    ]
}

fn render_single_tool_header_line(
    dot: &str,
    dot_style: Style,
    name: &str,
    summary: &str,
    width: usize,
) -> Line<'static> {
    let mut spans = tool_header_prefix(dot, dot_style, name);
    let summary = single_line_content(summary);
    if !summary.is_empty() {
        spans.push(Span::raw(format!("({summary})")));
    }
    truncate_line(Line::from(spans), width)
}

fn render_tool_progress(
    kind: LiveEntryKind,
    progress: &ToolProgressPayload,
    width: usize,
) -> Vec<Line<'static>> {
    let content = if progress.content.is_empty() {
        match progress.kind {
            ToolProgressKind::Permission => "Waiting for permission…",
            ToolProgressKind::Tool => "",
            ToolProgressKind::Classifier => "",
        }
    } else {
        progress.content.as_str()
    };
    if content.is_empty() {
        return Vec::new();
    }
    let nested = tool_is_nested(kind, progress.nested());
    let prefix = if nested { "" } else { TOOL_BODY_GUTTER };
    let content_width = width.saturating_sub(UnicodeWidthStr::width(prefix));
    let content_line = truncate_line(
        single_line_ansi_content(content, Style::default().fg(palette::INACTIVE)),
        content_width,
    );
    let gutter_style = Style::default()
        .fg(palette::INACTIVE)
        .add_modifier(ratatui::style::Modifier::DIM);
    let mut spans = vec![Span::styled(prefix, gutter_style)];
    spans.extend(content_line.spans);
    vec![Line::from(spans)]
}

fn render_tool_result(
    kind: LiveEntryKind,
    result: &ToolResultPayload,
    width: usize,
    verbose: bool,
) -> Vec<Line<'static>> {
    let (content, style) = match result.state {
        ToolResultState::Success => (None, Style::default().fg(palette::TEXT)),
        ToolResultState::Error => (None, Style::default().fg(palette::ERROR)),
        ToolResultState::Cancelled => (
            Some(vec![Line::from(Span::raw(
                "Interrupted · What should Claude do instead?",
            ))]),
            Style::default()
                .fg(palette::INACTIVE)
                .add_modifier(ratatui::style::Modifier::DIM),
        ),
    };
    let content_lines = match result.state {
        ToolResultState::Success => success_result_lines(&result.tool_name, &result.content),
        ToolResultState::Error => error_result_lines(&result.content, verbose),
        ToolResultState::Cancelled => content,
    };
    let Some(content_lines) = content_lines else {
        return Vec::new();
    };
    if content_lines.is_empty() {
        return Vec::new();
    }
    render_tool_body(
        &content_lines,
        width,
        tool_is_nested(kind, result.nested()),
        style,
    )
}

fn tool_is_nested(kind: LiveEntryKind, payload_is_nested: bool) -> bool {
    matches!(kind, LiveEntryKind::ToolNested) || payload_is_nested
}

fn success_result_lines(tool_name: &str, content: &str) -> Option<Vec<Line<'static>>> {
    let mut lines = trimmed_styled_lines(parse_sanitized_ansi(content, Style::default()));
    if lines.is_empty() {
        return None;
    }
    if tool_user_facing_name(tool_name) == "Write" && lines.len() > 10 {
        let remaining = lines.len() - 10;
        lines.truncate(10);
        lines.push(Line::from(vec![
            Span::styled("… +", Style::default().fg(palette::INACTIVE)),
            Span::styled(
                remaining.to_string(),
                Style::default().fg(palette::INACTIVE),
            ),
            Span::styled(
                if remaining == 1 {
                    " line ("
                } else {
                    " lines ("
                },
                Style::default().fg(palette::INACTIVE),
            ),
            Span::styled(
                "ctrl+o",
                Style::default()
                    .fg(palette::INACTIVE)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(" to expand)", Style::default().fg(palette::INACTIVE)),
        ]));
    }
    Some(lines)
}

fn error_result_lines(content: &str, verbose: bool) -> Option<Vec<Line<'static>>> {
    let mut lines = parse_sanitized_ansi(content, Style::default())
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .flat_map(|span| {
                    let style = span.style;
                    span.content
                        .chars()
                        .map(move |character| (character, style))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    strip_error_wrappers_from_styled(&mut lines);
    trim_styled_lines(&mut lines);
    let visible = styled_lines_to_string(&lines);
    if visible.is_empty() {
        return None;
    }
    if !verbose && visible.contains("InputValidationError") {
        return Some(vec![Line::from("Invalid tool parameters")]);
    }
    if !visible.starts_with("Error: ") && !visible.starts_with("Cancelled: ") {
        let first_line = lines.first_mut().expect("visible text has a first line");
        let mut prefix = "Error: "
            .chars()
            .map(|character| (character, Style::default()))
            .collect::<Vec<_>>();
        prefix.append(first_line);
        *first_line = prefix;
    }

    let mut lines = lines
        .into_iter()
        .map(styled_chars_to_line)
        .collect::<Vec<_>>();
    if !verbose && lines.len() > 10 {
        let remaining = lines.len() - 10;
        lines.truncate(10);
        lines.push(Line::from(vec![
            Span::styled("… +", Style::default().fg(palette::INACTIVE)),
            Span::styled(
                remaining.to_string(),
                Style::default().fg(palette::INACTIVE),
            ),
            Span::styled(
                if remaining == 1 {
                    " line ("
                } else {
                    " lines ("
                },
                Style::default().fg(palette::INACTIVE),
            ),
            Span::styled(
                "ctrl+o",
                Style::default()
                    .fg(palette::INACTIVE)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(" to see all)", Style::default().fg(palette::INACTIVE)),
        ]));
    }
    Some(lines)
}

fn strip_error_wrappers_from_styled(lines: &mut [Vec<(char, Style)>]) {
    for line in lines {
        for tag in [
            "<tool_use_error>",
            "</tool_use_error>",
            "<error>",
            "</error>",
            "<sandbox_violation>",
            "</sandbox_violation>",
        ] {
            loop {
                let text = line
                    .iter()
                    .map(|(character, _)| *character)
                    .collect::<String>();
                let Some(start_byte) = text.find(tag) else {
                    break;
                };
                let start = text[..start_byte].chars().count();
                let end = start + tag.chars().count();
                line.drain(start..end);
            }
        }
    }
}

fn trim_styled_lines(lines: &mut Vec<Vec<(char, Style)>>) {
    while let Some(line) = lines.first_mut() {
        while line
            .first()
            .is_some_and(|(character, _)| character.is_whitespace())
        {
            line.remove(0);
        }
        if line.is_empty() {
            lines.remove(0);
        } else {
            break;
        }
    }
    while let Some(line) = lines.last_mut() {
        while line
            .last()
            .is_some_and(|(character, _)| character.is_whitespace())
        {
            line.pop();
        }
        if line.is_empty() {
            lines.pop();
        } else {
            break;
        }
    }
}

fn styled_lines_to_string(lines: &[Vec<(char, Style)>]) -> String {
    lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|(character, _)| *character)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn styled_chars_to_line(chars: Vec<(char, Style)>) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (character, style) in chars {
        if let Some(span) = spans.last_mut().filter(|span| span.style == style) {
            span.content.to_mut().push(character);
        } else {
            spans.push(Span::styled(character.to_string(), style));
        }
    }
    Line::from(spans)
}

fn trimmed_styled_lines(mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    while lines
        .first()
        .is_some_and(|line| line.to_string().trim().is_empty())
    {
        lines.remove(0);
    }
    while lines
        .last()
        .is_some_and(|line| line.to_string().trim().is_empty())
    {
        lines.pop();
    }
    lines
}

fn single_line_content(content: &str) -> String {
    let mut single_line = String::with_capacity(content.len());
    let mut characters = content.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                single_line.push(' ');
            }
            '\n' => single_line.push(' '),
            character => single_line.push(character),
        }
    }
    single_line
}

fn single_line_ansi_content(content: &str, base_style: Style) -> Line<'static> {
    parse_sanitized_ansi(&single_line_content(content), base_style)
        .into_iter()
        .next()
        .unwrap_or_default()
}

const TOOL_BODY_GUTTER: &str = "  ⎿ \u{00a0}";
const TOOL_BODY_CONTINUATION_GUTTER: &str = "     ";

fn render_tool_body(
    lines: &[Line<'static>],
    width: usize,
    nested: bool,
    body_style: Style,
) -> Vec<Line<'static>> {
    let content_width = width.saturating_sub(if nested {
        0
    } else {
        UnicodeWidthStr::width(TOOL_BODY_GUTTER)
    });
    let wrapped = wrap_lines(
        lines,
        &WrapSpec {
            wrap_width: content_width,
            fill_width: 0,
            first_prefix: Vec::new(),
            continuation_prefix: Vec::new(),
            fill_style: None,
        },
    );
    let mut rendered = Vec::with_capacity(wrapped.len() + usize::from(!nested));
    if !nested {
        rendered.push(Line::default());
    }
    for (index, line) in wrapped.into_iter().enumerate() {
        let prefix = if nested {
            ""
        } else if index == 0 {
            TOOL_BODY_GUTTER
        } else {
            TOOL_BODY_CONTINUATION_GUTTER
        };
        let gutter_style = Style::default()
            .fg(palette::INACTIVE)
            .add_modifier(ratatui::style::Modifier::DIM);
        let mut spans = vec![Span::styled(prefix, gutter_style)];
        spans.extend(
            line.spans
                .into_iter()
                .map(|span| Span::styled(span.content, body_style.patch(span.style))),
        );
        rendered.push(Line::from(spans));
    }
    rendered
}

fn truncate_line_with_ellipsis(line: Line<'static>, width: usize) -> Line<'static> {
    if line.width() <= width {
        return line;
    }
    if width == 0 {
        return Line::default();
    }
    let mut truncated = truncate_line(line, width.saturating_sub(1));
    truncated.spans.push(Span::raw("…"));
    truncated
}

fn truncate_line_with_suffix(line: Line<'static>, width: usize, suffix: &str) -> Line<'static> {
    let suffix_width = UnicodeWidthStr::width(suffix);
    if line.width().saturating_add(suffix_width) <= width {
        let mut spans = line.spans;
        spans.push(Span::raw(suffix.to_string()));
        return Line::from(spans);
    }
    let mut truncated = truncate_line_with_ellipsis(line, width.saturating_sub(suffix_width));
    truncated.spans.push(Span::raw(suffix.to_string()));
    truncated
}

fn truncate_line(line: Line<'static>, width: usize) -> Line<'static> {
    let mut remaining = width;
    let mut spans = Vec::new();
    for span in line.spans {
        let mut content = String::new();
        for grapheme in
            unicode_segmentation::UnicodeSegmentation::graphemes(span.content.as_ref(), true)
        {
            let grapheme_width = UnicodeWidthStr::width(grapheme);
            if grapheme_width > remaining {
                break;
            }
            content.push_str(grapheme);
            remaining = remaining.saturating_sub(grapheme_width);
        }
        if !content.is_empty() {
            spans.push(Span::styled(content, span.style));
        }
        if remaining == 0 {
            break;
        }
    }
    Line::from(spans)
}

fn tool_dot_visible(animation_elapsed_ms: u64) -> bool {
    (animation_elapsed_ms / 600).is_multiple_of(2)
}

fn assistant_prefix_width(kind: LiveEntryKind) -> usize {
    match kind {
        LiveEntryKind::Assistant => UnicodeWidthStr::width(assistant_dot_with_space()),
        LiveEntryKind::AssistantNested => 0,
        LiveEntryKind::User
        | LiveEntryKind::Bash
        | LiveEntryKind::Tool
        | LiveEntryKind::ToolNested
        | LiveEntryKind::Other => 0,
    }
}

pub fn render_entry_lines(
    kind: LiveEntryKind,
    lines: &[Line<'static>],
    width: usize,
) -> Vec<Line<'static>> {
    let lines = match kind {
        LiveEntryKind::User => wrap_lines(lines, &user_wrap_spec(width)),
        LiveEntryKind::Assistant => wrap_lines(lines, &assistant_wrap_spec(width)),
        LiveEntryKind::AssistantNested => wrap_lines(lines, &assistant_nested_wrap_spec(width)),
        LiveEntryKind::Bash
        | LiveEntryKind::Tool
        | LiveEntryKind::ToolNested
        | LiveEntryKind::Other => lines.to_vec(),
    };
    let has_top_level_spacing = !matches!(kind, LiveEntryKind::AssistantNested);
    let mut rendered = Vec::with_capacity(lines.len() + usize::from(has_top_level_spacing));
    if has_top_level_spacing {
        rendered.push(Line::default());
    }
    rendered.extend(lines);
    rendered
}

fn user_wrap_spec(columns: usize) -> WrapSpec {
    WrapSpec {
        wrap_width: columns.saturating_sub(1),
        fill_width: columns,
        first_prefix: vec![Span::styled("❯ ", Style::default().fg(palette::SUBTLE))],
        continuation_prefix: Vec::new(),
        fill_style: Some(Style::default().bg(palette::USER_MESSAGE_BACKGROUND)),
    }
}

fn assistant_wrap_spec(columns: usize) -> WrapSpec {
    WrapSpec {
        wrap_width: columns,
        fill_width: columns,
        first_prefix: vec![Span::styled(
            assistant_dot_with_space(),
            Style::default().fg(palette::TEXT),
        )],
        continuation_prefix: vec![Span::raw("  ")],
        fill_style: None,
    }
}

fn assistant_nested_wrap_spec(columns: usize) -> WrapSpec {
    WrapSpec {
        wrap_width: columns,
        fill_width: columns,
        first_prefix: Vec::new(),
        continuation_prefix: Vec::new(),
        fill_style: None,
    }
}

fn assistant_dot_with_space() -> &'static str {
    if cfg!(target_os = "macos") {
        "⏺ "
    } else {
        "● "
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
