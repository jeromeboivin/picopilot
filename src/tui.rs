use crate::events::{BannerSeverity, EventUpdate, UsageSnapshot};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    None,
    Quit,
    Send(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEntry {
    User(String),
    Assistant {
        message_id: String,
        content: String,
        agent_id: Option<String>,
    },
    Reasoning {
        reasoning_id: String,
        content: String,
        agent_id: Option<String>,
    },
    Tool {
        tool_call_id: String,
        tool_name: String,
        output: String,
        success: Option<bool>,
        agent_id: Option<String>,
    },
    Subagent {
        name: String,
        display_name: String,
        status: SubagentStatus,
        error: Option<String>,
        agent_id: Option<String>,
    },
    Banner {
        severity: BannerSeverity,
        message: String,
        url: Option<String>,
    },
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubagentStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusState {
    pub model: Option<String>,
    pub usage: Option<UsageSnapshot>,
    pub busy: bool,
}

#[derive(Debug, Default)]
pub struct App {
    entries: Vec<ChatEntry>,
    status: StatusState,
    input: String,
    should_quit: bool,
}

impl App {
    pub fn new(model: Option<String>) -> Self {
        Self {
            status: StatusState {
                model,
                ..StatusState::default()
            },
            ..Self::default()
        }
    }

    pub fn entries(&self) -> &[ChatEntry] {
        &self.entries
    }

    pub fn status(&self) -> &StatusState {
        &self.status
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn push_input(&mut self, character: char) {
        self.input.push(character);
    }

    pub fn pop_input(&mut self) {
        self.input.pop();
    }

    pub fn take_input(&mut self) -> String {
        std::mem::take(&mut self.input)
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn add_user_message(&mut self, content: String) {
        self.entries.push(ChatEntry::User(content));
        self.status.busy = true;
    }

    pub fn apply(&mut self, update: EventUpdate) {
        match update {
            EventUpdate::AssistantDelta {
                message_id,
                content,
                agent_id,
            } => self.append_assistant(message_id, content, agent_id),
            EventUpdate::AssistantMessage {
                message_id,
                content,
                agent_id,
            } => self.replace_assistant(message_id, content, agent_id),
            EventUpdate::ReasoningDelta {
                reasoning_id,
                content,
                agent_id,
            } => self.append_reasoning(reasoning_id, content, agent_id),
            EventUpdate::Reasoning {
                reasoning_id,
                content,
                agent_id,
            } => self.replace_reasoning(reasoning_id, content, agent_id),
            EventUpdate::ToolStarted {
                tool_call_id,
                tool_name,
                agent_id,
            } => self.entries.push(ChatEntry::Tool {
                tool_call_id,
                tool_name,
                output: String::new(),
                success: None,
                agent_id,
            }),
            EventUpdate::ToolOutput {
                tool_call_id,
                content,
                agent_id: _,
            } => {
                if let Some(ChatEntry::Tool { output, .. }) = self
                    .entries
                    .iter_mut()
                    .find(|entry| matches!(entry, ChatEntry::Tool { tool_call_id: id, .. } if id == &tool_call_id))
                {
                    output.push_str(&content);
                }
            }
            EventUpdate::ToolCompleted {
                tool_call_id,
                success,
                message,
                agent_id: _,
            } => {
                if let Some(ChatEntry::Tool {
                    output,
                    success: state,
                    ..
                }) = self
                    .entries
                    .iter_mut()
                    .find(|entry| matches!(entry, ChatEntry::Tool { tool_call_id: id, .. } if id == &tool_call_id))
                {
                    *state = Some(success);
                    if let Some(message) = message {
                        output.push_str(&message);
                    }
                }
            }
            EventUpdate::SubagentStarted {
                name,
                display_name,
                agent_id,
            } => self.entries.push(ChatEntry::Subagent {
                name,
                display_name,
                status: SubagentStatus::Running,
                error: None,
                agent_id,
            }),
            EventUpdate::SubagentCompleted { name, agent_id: _ } => {
                if let Some(ChatEntry::Subagent { status, .. }) = self
                    .entries
                    .iter_mut()
                    .find(|entry| matches!(entry, ChatEntry::Subagent { name: current, .. } if current == &name))
                {
                    *status = SubagentStatus::Completed;
                }
            }
            EventUpdate::SubagentFailed {
                name,
                error,
                agent_id: _,
            } => {
                if let Some(ChatEntry::Subagent {
                    status,
                    error: current_error,
                    ..
                }) = self
                    .entries
                    .iter_mut()
                    .find(|entry| matches!(entry, ChatEntry::Subagent { name: current, .. } if current == &name))
                {
                    *status = SubagentStatus::Failed;
                    *current_error = Some(error);
                }
            }
            EventUpdate::Usage(usage) => self.status.usage = Some(usage),
            EventUpdate::Banner {
                severity,
                message,
                url,
            } => self.entries.push(ChatEntry::Banner {
                severity,
                message,
                url,
            }),
            EventUpdate::ModelChanged { model } => self.status.model = Some(model),
            EventUpdate::Idle | EventUpdate::TaskComplete => {
                self.status.busy = false;
                if !matches!(self.entries.last(), Some(ChatEntry::Completed)) {
                    self.entries.push(ChatEntry::Completed);
                }
            }
        }
    }

    fn append_assistant(&mut self, message_id: String, content: String, agent_id: Option<String>) {
        if let Some(ChatEntry::Assistant {
            content: current,
            ..
        }) = self.entries.iter_mut().rev().find(|entry| {
            matches!(entry, ChatEntry::Assistant { message_id: id, .. } if id == &message_id)
        }) {
            current.push_str(&content);
        } else {
            self.entries.push(ChatEntry::Assistant {
                message_id,
                content,
                agent_id,
            });
        }
    }

    fn replace_assistant(&mut self, message_id: String, content: String, agent_id: Option<String>) {
        if let Some(ChatEntry::Assistant {
            content: current,
            agent_id: current_agent,
            ..
        }) = self.entries.iter_mut().rev().find(|entry| {
            matches!(entry, ChatEntry::Assistant { message_id: id, .. } if id == &message_id)
        }) {
            *current = content;
            if current_agent.is_none() {
                *current_agent = agent_id;
            }
        } else {
            self.entries.push(ChatEntry::Assistant {
                message_id,
                content,
                agent_id,
            });
        }
    }

    fn append_reasoning(
        &mut self,
        reasoning_id: String,
        content: String,
        agent_id: Option<String>,
    ) {
        if let Some(ChatEntry::Reasoning {
            content: current,
            ..
        }) = self.entries.iter_mut().rev().find(|entry| {
            matches!(entry, ChatEntry::Reasoning { reasoning_id: id, .. } if id == &reasoning_id)
        }) {
            current.push_str(&content);
        } else {
            self.entries.push(ChatEntry::Reasoning {
                reasoning_id,
                content,
                agent_id,
            });
        }
    }

    fn replace_reasoning(
        &mut self,
        reasoning_id: String,
        content: String,
        agent_id: Option<String>,
    ) {
        if let Some(ChatEntry::Reasoning {
            content: current,
            agent_id: current_agent,
            ..
        }) = self.entries.iter_mut().rev().find(|entry| {
            matches!(entry, ChatEntry::Reasoning { reasoning_id: id, .. } if id == &reasoning_id)
        }) {
            *current = content;
            if current_agent.is_none() {
                *current_agent = agent_id;
            }
        } else {
            self.entries.push(ChatEntry::Reasoning {
                reasoning_id,
                content,
                agent_id,
            });
        }
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> UiAction {
    if key.kind != KeyEventKind::Press {
        return UiAction::None;
    }

    match key.code {
        KeyCode::Esc => UiAction::Quit,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => UiAction::Quit,
        KeyCode::Char('q') if app.input().is_empty() => UiAction::Quit,
        KeyCode::Enter => {
            let input = app.take_input();
            if input.trim().is_empty() {
                UiAction::None
            } else {
                UiAction::Send(input)
            }
        }
        KeyCode::Backspace => {
            app.pop_input();
            UiAction::None
        }
        KeyCode::Char(character) => {
            app.push_input(character);
            UiAction::None
        }
        _ => UiAction::None,
    }
}

pub fn draw(frame: &mut Frame, app: &App) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(frame.area());

    frame.render_widget(status_bar(app), layout[0]);
    draw_chat(frame, app, layout[1]);
    frame.render_widget(input_box(app), layout[2]);
}

fn status_bar(app: &App) -> Paragraph<'static> {
    let status = app.status();
    let model = status.model.as_deref().unwrap_or("auto");
    let mode = if status.busy { "working" } else { "ready" };
    let context = status
        .usage
        .as_ref()
        .map(|usage| format_tokens(usage.current_tokens, usage.token_limit))
        .unwrap_or_else(|| "--/--".to_string());
    let label = format!(
        " picopilot | model: {model} | mode: autopilot/{mode} | context: {context} | cost: -- "
    );

    Paragraph::new(label).style(Style::default().fg(Color::White).bg(Color::Rgb(28, 38, 50)))
}

fn input_box(app: &App) -> Paragraph<'static> {
    Paragraph::new(Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::Rgb(240, 177, 94))),
        Span::raw(app.input().to_string()),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(70, 88, 104)))
            .title("message"),
    )
}

fn draw_chat(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(70, 88, 104)))
        .title("conversation");
    let inner_width = area.width.saturating_sub(2);
    let visible_height = area.height.saturating_sub(2);
    let lines = chat_lines(app);
    let total_lines = wrapped_line_count(&lines, inner_width);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    let scroll = total_lines
        .saturating_sub(visible_height as usize)
        .min(u16::MAX as usize) as u16;

    frame.render_widget(paragraph.scroll((scroll, 0)), area);
}

fn wrapped_line_count(lines: &[Line<'static>], width: u16) -> usize {
    let width = usize::from(width.max(1));
    lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum()
}

fn chat_lines(app: &App) -> Vec<Line<'static>> {
    if app.entries().is_empty() {
        return vec![Line::from(Span::styled(
            "Waiting for a prompt.",
            Style::default().fg(Color::Rgb(132, 147, 160)),
        ))];
    }

    app.entries().iter().flat_map(entry_lines).collect()
}

fn entry_lines(entry: &ChatEntry) -> Vec<Line<'static>> {
    match entry {
        ChatEntry::User(content) => labeled_lines(
            "you",
            content,
            Style::default()
                .fg(Color::Rgb(240, 177, 94))
                .add_modifier(Modifier::BOLD),
        ),
        ChatEntry::Assistant {
            content, agent_id, ..
        } => labeled_lines(
            &speaker_label("agent", agent_id.as_deref()),
            content,
            Style::default().fg(Color::Rgb(154, 230, 180)),
        ),
        ChatEntry::Reasoning {
            content, agent_id, ..
        } => labeled_lines(
            &speaker_label("think", agent_id.as_deref()),
            content,
            Style::default()
                .fg(Color::Rgb(165, 174, 187))
                .add_modifier(Modifier::ITALIC),
        ),
        ChatEntry::Tool {
            tool_name,
            output,
            success,
            agent_id,
            ..
        } => {
            let state = match success {
                None => "running",
                Some(true) => "done",
                Some(false) => "failed",
            };
            let label = format!(
                "tool {} [{}]{}",
                tool_name,
                state,
                agent_suffix(agent_id.as_deref())
            );
            let mut lines = labeled_lines(
                &label,
                if output.is_empty() { "" } else { output },
                Style::default().fg(Color::Rgb(139, 181, 255)),
            );
            if output.is_empty() {
                lines.truncate(1);
            }
            lines
        }
        ChatEntry::Subagent {
            display_name,
            status,
            error,
            agent_id,
            ..
        } => {
            let state = match status {
                SubagentStatus::Running => "running",
                SubagentStatus::Completed => "done",
                SubagentStatus::Failed => "failed",
            };
            let content = error.as_deref().unwrap_or("");
            let label = format!(
                "agent {} [{}]{}",
                display_name,
                state,
                agent_suffix(agent_id.as_deref())
            );
            let mut lines = labeled_lines(
                &label,
                content,
                Style::default().fg(Color::Rgb(204, 166, 255)),
            );
            if content.is_empty() {
                lines.truncate(1);
            }
            lines
        }
        ChatEntry::Banner {
            severity,
            message,
            url,
        } => {
            let (label, style) = match severity {
                BannerSeverity::Warning => (
                    "warn",
                    Style::default()
                        .fg(Color::Rgb(242, 204, 96))
                        .add_modifier(Modifier::BOLD),
                ),
                BannerSeverity::RecoverableError => (
                    "retry",
                    Style::default()
                        .fg(Color::Rgb(255, 169, 122))
                        .add_modifier(Modifier::BOLD),
                ),
                BannerSeverity::BlockingError => (
                    "error",
                    Style::default()
                        .fg(Color::Rgb(255, 117, 117))
                        .add_modifier(Modifier::BOLD),
                ),
            };
            let content = match url {
                Some(url) => format!("{message} ({url})"),
                None => message.clone(),
            };
            labeled_lines(label, &content, style)
        }
        ChatEntry::Completed => vec![Line::from(Span::styled(
            "done",
            Style::default().fg(Color::Rgb(132, 147, 160)),
        ))],
    }
}

fn labeled_lines(label: &str, content: &str, label_style: Style) -> Vec<Line<'static>> {
    let mut lines = content.lines();
    let first = lines.next().unwrap_or_default();
    let mut rendered = vec![Line::from(vec![
        Span::styled(format!("{label:<18} "), label_style),
        Span::raw(first.to_string()),
    ])];
    rendered.extend(lines.map(|line| {
        Line::from(vec![
            Span::styled("                   ", label_style),
            Span::raw(line.to_string()),
        ])
    }));
    rendered
}

fn speaker_label(label: &str, agent_id: Option<&str>) -> String {
    format!("{}{}", label, agent_suffix(agent_id))
}

fn agent_suffix(agent_id: Option<&str>) -> String {
    agent_id.map(|id| format!(" ({id})")).unwrap_or_default()
}

fn format_tokens(current: i64, limit: i64) -> String {
    format!("{}/{}", format_count(current), format_count(limit))
}

fn format_count(value: i64) -> String {
    let digits = value.to_string();
    let mut result = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(character);
    }
    result
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    use super::{handle_key, App, ChatEntry, UiAction};
    use crate::events::EventUpdate;

    #[test]
    fn accumulates_streamed_assistant_deltas_into_one_entry() {
        let mut app = App::new(Some("gpt-5".to_string()));

        app.apply(EventUpdate::AssistantDelta {
            message_id: "message-1".to_string(),
            content: "The patch".to_string(),
            agent_id: None,
        });
        app.apply(EventUpdate::AssistantDelta {
            message_id: "message-1".to_string(),
            content: " is ready.".to_string(),
            agent_id: None,
        });

        assert_eq!(
            app.entries(),
            &[ChatEntry::Assistant {
                message_id: "message-1".to_string(),
                content: "The patch is ready.".to_string(),
                agent_id: None,
            }]
        );
    }

    #[test]
    fn submits_input_on_enter_and_ignores_key_release_events() {
        let mut app = App::new(None);
        app.push_input('h');
        app.push_input('i');

        assert_eq!(
            handle_key(
                &mut app,
                KeyEvent {
                    code: KeyCode::Enter,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: crossterm::event::KeyEventState::NONE,
                }
            ),
            UiAction::Send("hi".to_string())
        );
        assert!(app.input().is_empty());

        app.push_input('x');
        assert_eq!(
            handle_key(
                &mut app,
                KeyEvent {
                    code: KeyCode::Enter,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Release,
                    state: crossterm::event::KeyEventState::NONE,
                }
            ),
            UiAction::None
        );
        assert_eq!(app.input(), "x");
    }
}
