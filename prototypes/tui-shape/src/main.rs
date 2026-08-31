//! THROWAWAY prototype for wayfinder ticket 003-tui-shape. Not part of picopilot.
//! Three structurally different TUI layouts, cycled with Left/Right.
//! Run: `cargo run` from prototypes/tui-shape.
//!
//! Keys:
//!   Left/Right  cycle variant (wraps)
//!   p           toggle the session-history picker
//!   a           toggle a pending tool-approval (simulated)
//!   u           toggle the context/cost usage detail modal
//!   Up/Down     move selection in whichever list has focus
//!   q / Esc     quit

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use ratatui::Terminal;

const VARIANTS: [&str; 3] = [
    "A: Single-column overlay",
    "B: Persistent sidebar + modal",
    "C: Command-center split",
];

/// Fake transcript: (speaker, text). "tool" entries render distinctly per variant.
fn fake_transcript() -> Vec<(&'static str, &'static str)> {
    vec![
        ("user", "Add retry logic to the SFTP uploader"),
        ("assistant", "I'll look at the uploader first."),
        ("tool", "read src/sftp/uploader.rs"),
        ("assistant", "Found it. I'll wrap the send in a bounded retry."),
        ("tool", "edit src/sftp/uploader.rs (+18 -2)"),
        ("assistant", "Now let's make sure the build is clean."),
        ("tool", "shell: cargo check"),
    ]
}

/// The one tool call currently awaiting approval, shown when `show_approval` is on.
const PENDING_TOOL: &str = "shell: cargo test -- --include-ignored";

fn fake_sessions() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("2h ago", "Add retry logic to the SFTP uploader", "picopilot"),
        ("yesterday", "Trim the default system message", "picopilot"),
        ("3 days ago", "Fix flaky resume test", "picopilot"),
        ("last week", "Prototype the session picker", "picopilot"),
    ]
}

/// Fake numbers matching the SDK's `session.metadata.getContextAttribution()` shape:
/// session-wide cost, current/limit tokens, and a percent-of-window breakdown by category.
fn fake_usage() -> (f64, u32, u32, Vec<(&'static str, f64)>) {
    let cost_credits = 811.8;
    let current_tokens = 169_300;
    let token_limit = 264_000;
    let categories = vec![
        ("System Instructions", 9.6),
        ("Tool Definitions", 9.0),
        ("Messages", 30.1),
        ("Tool Results", 15.4),
    ];
    (cost_credits, current_tokens, token_limit, categories)
}

struct App {
    variant: usize,
    show_picker: bool,
    show_approval: bool,
    show_usage: bool,
    session_list: ListState,
}

impl App {
    fn new() -> Self {
        let mut session_list = ListState::default();
        session_list.select(Some(0));
        Self {
            variant: 0,
            show_picker: false,
            show_approval: true,
            show_usage: false,
            session_list,
        }
    }

    fn cycle(&mut self, delta: i32) {
        let len = VARIANTS.len() as i32;
        self.variant = ((self.variant as i32 + delta).rem_euclid(len)) as usize;
    }

    fn move_selection(&mut self, delta: i32) {
        let sessions = fake_sessions();
        let i = self.session_list.selected().unwrap_or(0) as i32;
        let next = (i + delta).rem_euclid(sessions.len() as i32);
        self.session_list.select(Some(next as usize));
    }
}

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    loop {
        terminal.draw(|f| draw(f, &app))?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                // Windows Terminal/VS Code report both press and release; only act on press
                // or every toggle key fires twice and appears to do nothing.
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Left => app.cycle(-1),
                    KeyCode::Right => app.cycle(1),
                    KeyCode::Char('p') => app.show_picker = !app.show_picker,
                    KeyCode::Char('a') => app.show_approval = !app.show_approval,
                    KeyCode::Char('u') => app.show_usage = !app.show_usage,
                    KeyCode::Down => app.move_selection(1),
                    KeyCode::Up => app.move_selection(-1),
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

fn draw(f: &mut Frame, app: &App) {
    match app.variant {
        0 => draw_variant_a(f, app),
        1 => draw_variant_b(f, app),
        _ => draw_variant_c(f, app),
    }
    draw_switcher(f, app);
    if app.show_usage {
        draw_usage_modal(f);
    }
}

/// Bottom switcher strip, present in every variant so flipping never loses context.
fn draw_switcher(f: &mut Frame, app: &App) {
    let area = f.area();
    let bar = Rect::new(0, area.height.saturating_sub(1), area.width, 1);
    let label = format!(
        " ← {} → | p: picker={} | a: approval={} | u: usage={} | q: quit ",
        VARIANTS[app.variant],
        if app.show_picker { "on" } else { "off" },
        if app.show_approval { "pending" } else { "clear" },
        if app.show_usage { "on" } else { "off" }
    );
    f.render_widget(
        Paragraph::new(label).style(Style::default().bg(Color::Blue).fg(Color::White)),
        bar,
    );
}

fn transcript_lines<'a>(show_pending: bool) -> Vec<Line<'a>> {
    let mut lines: Vec<Line> = fake_transcript()
        .into_iter()
        .map(|(who, text)| match who {
            "user" => Line::from(vec![Span::styled("you  ", Style::default().fg(Color::Cyan)), Span::raw(text)]),
            "assistant" => Line::from(vec![Span::styled("agent", Style::default().fg(Color::Green)), Span::raw(format!(" {text}"))]),
            _ => Line::from(vec![Span::styled("tool ", Style::default().fg(Color::DarkGray)), Span::styled(text, Style::default().add_modifier(Modifier::ITALIC))]),
        })
        .collect();
    if show_pending {
        lines.push(Line::from(Span::styled(
            format!("tool  {PENDING_TOOL}  [awaiting approval — inline]"),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )));
    }
    lines
}

fn status_bar_text() -> String {
    " model: claude-sonnet-4.5 · autopilot · 12,480 tokens · $0.18 this session ".to_string()
}

/// A: everything is an overlay over one full-width chat column — status bar on top,
/// input pinned at bottom, picker and approval both pop over the transcript.
fn draw_variant_a(f: &mut Frame, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0), Constraint::Length(3)])
        .split(f.area());

    f.render_widget(
        Paragraph::new(status_bar_text()).style(Style::default().bg(Color::DarkGray).fg(Color::White)),
        root[0],
    );

    let inline_pending = app.show_approval; // A renders the pending call inline, in-stream
    let lines = transcript_lines(inline_pending);
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(Block::default().borders(Borders::ALL).title("picopilot")),
        root[1],
    );

    f.render_widget(
        Paragraph::new("> ").block(Block::default().borders(Borders::ALL).title("message")),
        root[2],
    );

    if app.show_picker {
        draw_picker_modal(f, app, "Ctrl+O opened this full-screen");
    }
}

/// B: a permanent left sidebar for session history; the pending tool call interrupts
/// with a centered modal dialog instead of appearing inline.
fn draw_variant_b(f: &mut Frame, app: &App) {
    let root = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(f.area());

    let sessions = fake_sessions();
    let items: Vec<ListItem> = sessions
        .iter()
        .map(|(when, title, _)| ListItem::new(format!("{when}\n{title}")))
        .collect();
    let mut list_state = app.session_list.clone();
    f.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title("history (always visible)"))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        root[0],
        &mut list_state,
    );

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0), Constraint::Length(3)])
        .split(root[1]);

    f.render_widget(
        Paragraph::new(status_bar_text()).style(Style::default().bg(Color::DarkGray).fg(Color::White)),
        right[0],
    );
    f.render_widget(
        Paragraph::new(transcript_lines(false)).wrap(Wrap { trim: false }).block(Block::default().borders(Borders::ALL)),
        right[1],
    );
    f.render_widget(
        Paragraph::new("> ").block(Block::default().borders(Borders::ALL).title("message")),
        right[2],
    );

    if app.show_approval {
        draw_approval_modal(f);
    }
    if app.show_picker {
        // sidebar is always visible in B; 'p' just calls out that it already has focus
        let hint = Rect::new(root[0].x, root[0].y + root[0].height.saturating_sub(1), root[0].width, 1);
        f.render_widget(Paragraph::new("↑↓ + Enter").style(Style::default().fg(Color::Yellow)), hint);
    }
}

/// C: dual pane — chat on the left, a running tool-activity log on the right that
/// owns approvals (no inline interruption, no modal). Picker is a command palette.
fn draw_variant_c(f: &mut Frame, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(f.area());

    f.render_widget(
        Paragraph::new(status_bar_text()).style(Style::default().bg(Color::DarkGray).fg(Color::White)),
        root[0],
    );

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(root[1]);

    let chat = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(body[0]);
    f.render_widget(
        Paragraph::new(transcript_lines(false)).wrap(Wrap { trim: false }).block(Block::default().borders(Borders::ALL)),
        chat[0],
    );
    f.render_widget(
        Paragraph::new("> ").block(Block::default().borders(Borders::ALL).title("message")),
        chat[1],
    );

    let mut log_lines: Vec<Line> = fake_transcript()
        .into_iter()
        .filter(|(who, _)| *who == "tool")
        .map(|(_, text)| Line::from(Span::styled(text, Style::default().fg(Color::DarkGray))))
        .collect();
    if app.show_approval {
        log_lines.push(Line::from(Span::styled(
            format!("▶ {PENDING_TOOL}  [y/n/a]"),
            Style::default().fg(Color::Yellow).bg(Color::Black).add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )));
    }
    f.render_widget(
        Paragraph::new(log_lines).wrap(Wrap { trim: false }).block(Block::default().borders(Borders::ALL).title("tool activity")),
        body[1],
    );

    if app.show_picker {
        draw_picker_palette(f, app);
    }
}

fn centered_rect(pct_x: u16, pct_y: u16, area: Rect) -> Rect {
    let popup_h = area.height * pct_y / 100;
    let popup_w = area.width * pct_x / 100;
    Rect::new(
        area.x + (area.width.saturating_sub(popup_w)) / 2,
        area.y + (area.height.saturating_sub(popup_h)) / 2,
        popup_w,
        popup_h,
    )
}

fn draw_picker_modal(f: &mut Frame, app: &App, hint: &str) {
    let area = centered_rect(60, 60, f.area());
    f.render_widget(Clear, area);
    let sessions = fake_sessions();
    let items: Vec<ListItem> = sessions
        .iter()
        .map(|(when, title, _)| ListItem::new(format!("{when} — {title}")))
        .collect();
    let mut list_state = app.session_list.clone();
    f.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title(format!("resume a session ({hint})")))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        area,
        &mut list_state,
    );
}

fn draw_picker_palette(f: &mut Frame, app: &App) {
    let area = centered_rect(50, 40, f.area());
    f.render_widget(Clear, area);
    let sessions = fake_sessions();
    let items: Vec<ListItem> = sessions
        .iter()
        .map(|(when, title, _)| ListItem::new(format!("{when} — {title}")))
        .collect();
    let mut list_state = app.session_list.clone();
    f.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title("⌘K resume session (fuzzy search)"))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        area,
        &mut list_state,
    );
}

/// Reuses the same modal convention as the session/model pickers (ticket 003/006):
/// a centered overlay, opened on demand, not a permanent part of any variant's layout.
fn draw_usage_modal(f: &mut Frame) {
    let outer = f.area();
    let usable = Rect::new(outer.x, outer.y, outer.width, outer.height.saturating_sub(1));
    let area = centered_rect(60, 70, usable);
    f.render_widget(Clear, area);
    f.render_widget(Block::default().borders(Borders::ALL).title("context & cost (u to close)"), area);

    let inner = Rect::new(area.x + 2, area.y + 1, area.width.saturating_sub(4), area.height.saturating_sub(2));
    let (cost_credits, current_tokens, token_limit, categories) = fake_usage();
    let window_pct = (current_tokens as f64 / token_limit as f64 * 100.0).round() as u16;

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // session cost
            Constraint::Length(1), // spacer
            Constraint::Length(1), // context window label
            Constraint::Length(1), // context window gauge
            Constraint::Length(1), // spacer
        ].into_iter().chain(categories.iter().flat_map(|_| [Constraint::Length(1), Constraint::Length(1)])).collect::<Vec<_>>())
        .split(inner);

    f.render_widget(
        Paragraph::new(format!("Session Cost: {cost_credits:.1} credits")),
        rows[0],
    );
    f.render_widget(
        Paragraph::new(format!(
            "Context Window: {:.1}K / {:.0}K tokens",
            current_tokens as f64 / 1000.0,
            token_limit as f64 / 1000.0
        )),
        rows[2],
    );
    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(Color::Cyan))
            .percent(window_pct)
            .label(format!("{window_pct}%")),
        rows[3],
    );

    for (i, (label, pct)) in categories.iter().enumerate() {
        let label_row = rows[5 + i * 2];
        let bar_row = rows[6 + i * 2];
        f.render_widget(Paragraph::new(format!("{label} — {pct:.1}%")), label_row);
        f.render_widget(
            Gauge::default()
                .gauge_style(Style::default().fg(Color::DarkGray))
                .percent(pct.round() as u16)
                .label(""),
            bar_row,
        );
    }
}

fn draw_approval_modal(f: &mut Frame) {
    let area = centered_rect(50, 20, f.area());
    f.render_widget(Clear, area);
    let text = format!("Allow this tool call?\n\n{PENDING_TOOL}\n\n[y] once   [a] always this session   [n] deny");
    f.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("confirm").style(Style::default().fg(Color::Yellow))),
        area,
    );
}
