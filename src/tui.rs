use crate::events::{
    context_attribution_snapshot, todo_snapshot, usage_metrics_snapshot, BannerSeverity,
    ContextAttributionSnapshot, EventUpdate, TodoSnapshot, UsageMetricsSnapshot, UsageSnapshot,
};
use std::collections::VecDeque;
use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use ratatui::Terminal;

use github_copilot_sdk::subscription::EventSubscription;
use github_copilot_sdk::subscription::RecvErrorKind;
use github_copilot_sdk::types::{ContextTier, Model, SessionId, SessionMetadata, SetModelOptions};

use crate::permissions::{ApprovalDecision, ApprovalRequest};
use crate::runtime::AppRuntime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSelection {
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub context_tier: Option<String>,
}

impl ModelSelection {
    fn sdk_options(&self) -> Result<Option<SetModelOptions>, String> {
        let mut options = SetModelOptions::default();
        let mut has_options = false;

        if let Some(reasoning_effort) = self.reasoning_effort.as_deref() {
            options = options.with_reasoning_effort(reasoning_effort);
            has_options = true;
        }

        if let Some(context_tier) = self.context_tier.as_deref() {
            let context_tier = match context_tier {
                "default" => ContextTier::Default,
                "long_context" => ContextTier::LongContext,
                _ => return Err(format!("unsupported context tier '{context_tier}'")),
            };
            options = options.with_context_tier(context_tier);
            has_options = true;
        }

        Ok(has_options.then_some(options))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    None,
    Quit,
    Send(String),
    Approval(ApprovalDecision),
    LoadSessions,
    LoadModels,
    LoadUsage,
    LoadTodos,
    Resume(SessionId),
    SwitchModel(ModelSelection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModalKind {
    Sessions,
    Models,
    Usage,
    Todos,
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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatusState {
    pub model: Option<String>,
    pub usage: Option<UsageSnapshot>,
    pub usage_metrics: Option<UsageMetricsSnapshot>,
    pub context_attribution: Option<ContextAttributionSnapshot>,
    pub busy: bool,
}

#[derive(Debug, Default)]
pub struct App {
    entries: Vec<ChatEntry>,
    status: StatusState,
    input: String,
    pending_approvals: VecDeque<ApprovalRequest>,
    modal: Option<ModalKind>,
    sessions: Vec<SessionMetadata>,
    models: Vec<Model>,
    selected_item: usize,
    picker_reasoning_effort: Option<String>,
    picker_context_tier: Option<String>,
    fleet_active: bool,
    todos: Option<TodoSnapshot>,
    todo_refresh_requested: bool,
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

    pub fn pending_approval(&self) -> Option<&ApprovalRequest> {
        self.pending_approvals.front()
    }

    pub fn modal_is_open(&self) -> bool {
        self.modal.is_some()
    }

    fn todo_modal_is_open(&self) -> bool {
        matches!(self.modal, Some(ModalKind::Todos))
    }

    pub fn set_sessions(&mut self, sessions: Vec<SessionMetadata>) {
        self.sessions = sessions;
        self.selected_item = 0;
        self.modal = Some(ModalKind::Sessions);
    }

    pub fn set_models(&mut self, models: Vec<Model>) {
        self.models = models;
        self.selected_item = 0;
        self.reset_picker_options();
        self.modal = Some(ModalKind::Models);
    }

    pub fn set_usage(
        &mut self,
        metrics: UsageMetricsSnapshot,
        context_attribution: Option<ContextAttributionSnapshot>,
    ) {
        self.status.usage_metrics = Some(metrics);
        self.status.context_attribution = context_attribution;
        self.selected_item = 0;
        self.modal = Some(ModalKind::Usage);
    }

    pub fn set_fleet_active(&mut self, active: bool) {
        self.fleet_active = active;
        if active {
            self.todos = None;
            self.todo_refresh_requested = false;
        } else if matches!(self.modal, Some(ModalKind::Todos)) {
            self.close_modal();
        }
    }

    pub fn set_todos(&mut self, todos: TodoSnapshot) {
        self.todos = Some(todos);
        self.todo_refresh_requested = false;
        self.selected_item = 0;
        self.modal = Some(ModalKind::Todos);
    }

    pub fn take_todo_refresh_request(&mut self) -> bool {
        std::mem::take(&mut self.todo_refresh_requested)
    }

    fn close_modal(&mut self) {
        self.modal = None;
        self.selected_item = 0;
    }

    fn move_selection(&mut self, delta: isize) {
        let item_count = match self.modal {
            Some(ModalKind::Sessions) => self.sessions.len(),
            Some(ModalKind::Models) => self.models.len(),
            Some(ModalKind::Usage | ModalKind::Todos) => 0,
            None => 0,
        };
        if item_count == 0 {
            return;
        }
        let previous = self.selected_item;
        self.selected_item =
            (self.selected_item as isize + delta).rem_euclid(item_count as isize) as usize;
        if matches!(self.modal, Some(ModalKind::Models)) && self.selected_item != previous {
            self.reset_picker_options();
        }
    }

    fn reset_picker_options(&mut self) {
        self.picker_reasoning_effort = None;
        self.picker_context_tier = None;
    }

    fn cycle_picker_option(&mut self, reasoning: bool) {
        let Some(model) = self.models.get(self.selected_item) else {
            return;
        };
        let mut values = vec![None];
        if reasoning {
            values.extend(
                model
                    .supported_reasoning_efforts
                    .clone()
                    .unwrap_or_default()
                    .into_iter()
                    .map(Some),
            );
        } else {
            values.extend(
                crate::config::supported_context_tiers(model)
                    .into_iter()
                    .filter_map(|tier| match tier.as_str() {
                        "default" | "long_context" => Some(Some(tier)),
                        _ => None,
                    }),
            );
        }

        let current = if reasoning {
            self.picker_reasoning_effort.clone()
        } else {
            self.picker_context_tier.clone()
        };
        let current_index = values
            .iter()
            .position(|value| value == &current)
            .unwrap_or(0);
        let next = values[(current_index + 1) % values.len()].clone();
        if reasoning {
            self.picker_reasoning_effort = next;
        } else {
            self.picker_context_tier = next;
        }
    }

    fn choose_selected(&mut self) -> UiAction {
        let action = match self.modal {
            Some(ModalKind::Sessions) => self
                .sessions
                .get(self.selected_item)
                .map(|session| UiAction::Resume(session.session_id.clone())),
            Some(ModalKind::Models) => self.models.get(self.selected_item).map(|model| {
                UiAction::SwitchModel(ModelSelection {
                    model: model.id.clone(),
                    reasoning_effort: self.picker_reasoning_effort.clone(),
                    context_tier: self.picker_context_tier.clone(),
                })
            }),
            Some(ModalKind::Usage | ModalKind::Todos) => None,
            None => None,
        };
        self.close_modal();
        action.unwrap_or(UiAction::None)
    }

    pub fn enqueue_approval(&mut self, request: ApprovalRequest) {
        self.pending_approvals.push_back(request);
    }

    fn take_approval(&mut self) -> Option<ApprovalRequest> {
        self.pending_approvals.pop_front()
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
            EventUpdate::TodosChanged => {
                if self.fleet_active && matches!(self.modal, Some(ModalKind::Todos)) {
                    self.todo_refresh_requested = true;
                }
            }
            EventUpdate::Idle | EventUpdate::TaskComplete => {
                self.set_fleet_active(false);
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

    if app.pending_approval().is_some() {
        return match key.code {
            KeyCode::Char('y') => UiAction::Approval(ApprovalDecision::ApproveOnce),
            KeyCode::Char('n') => UiAction::Approval(ApprovalDecision::Deny),
            KeyCode::Char('a') => UiAction::Approval(ApprovalDecision::Trust),
            _ => UiAction::None,
        };
    }

    if app.modal.is_some() {
        return match key.code {
            KeyCode::Char('u') if matches!(app.modal, Some(ModalKind::Usage)) => {
                app.close_modal();
                UiAction::None
            }
            KeyCode::Char('t') if matches!(app.modal, Some(ModalKind::Todos)) => {
                app.close_modal();
                UiAction::None
            }
            KeyCode::Esc => {
                app.close_modal();
                UiAction::None
            }
            KeyCode::Up => {
                app.move_selection(-1);
                UiAction::None
            }
            KeyCode::Down => {
                app.move_selection(1);
                UiAction::None
            }
            KeyCode::Char('r') => {
                app.cycle_picker_option(true);
                UiAction::None
            }
            KeyCode::Char('c') => {
                app.cycle_picker_option(false);
                UiAction::None
            }
            KeyCode::Enter => app.choose_selected(),
            _ => UiAction::None,
        };
    }

    match key.code {
        KeyCode::Esc => UiAction::Quit,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => UiAction::Quit,
        KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            UiAction::LoadSessions
        }
        KeyCode::Char('m') => UiAction::LoadModels,
        KeyCode::Char('u') => UiAction::LoadUsage,
        KeyCode::Char('t') if app.fleet_active => UiAction::LoadTodos,
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
    draw_modal(frame, app);
}

pub async fn run(runtime: AppRuntime, model: Option<String>) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    if let Err(error) = execute!(stdout, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(error);
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            return Err(error);
        }
    };

    let result = run_loop(&mut terminal, runtime, model).await;
    let restore_result = restore_terminal(&mut terminal);
    result.and(restore_result)
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut runtime: AppRuntime,
    model: Option<String>,
) -> io::Result<()> {
    let mut app = App::new(model);
    let mut events = runtime.session.subscribe();
    let mut permission_requests_open = true;

    while !app.should_quit() {
        terminal.draw(|frame| draw(frame, &app))?;
        let tick = tokio::time::sleep(Duration::from_millis(50));
        tokio::pin!(tick);

        tokio::select! {
            result = events.recv() => match result {
                Ok(event) => {
                    if let Some(update) = crate::events::event_update(&event) {
                        app.apply(update);
                    }
                }
                Err(error) => match error.kind() {
                    RecvErrorKind::Lagged(lagged) => app.apply(crate::events::EventUpdate::Banner {
                        severity: crate::events::BannerSeverity::Warning,
                        message: format!("event stream lagged by {} events", lagged.skipped()),
                        url: None,
                    }),
                    RecvErrorKind::Closed => {
                        app.apply(crate::events::EventUpdate::Banner {
                            severity: crate::events::BannerSeverity::BlockingError,
                            message: "Copilot event stream closed".to_string(),
                            url: None,
                        });
                        app.quit();
                    }
                    _ => app.apply(crate::events::EventUpdate::Banner {
                        severity: crate::events::BannerSeverity::Warning,
                        message: format!("event stream error: {error}"),
                        url: None,
                    }),
                }
            },
            request = runtime.permission_requests.recv(), if permission_requests_open => match request {
                Some(request) => app.enqueue_approval(request),
                None => permission_requests_open = false,
            },
            _ = &mut tick => {
                process_terminal_events(&mut app, &mut runtime, &mut events).await?;
                refresh_todos_if_requested(&mut app, &runtime).await?;
            },
        }
    }

    Ok(())
}

async fn process_terminal_events(
    app: &mut App,
    runtime: &mut AppRuntime,
    events: &mut EventSubscription,
) -> io::Result<()> {
    while event::poll(Duration::ZERO)? {
        let Event::Key(key) = event::read()? else {
            continue;
        };

        match handle_key(app, key) {
            UiAction::None => {}
            UiAction::Quit => app.quit(),
            UiAction::Approval(decision) => {
                if let Some(request) = app.take_approval() {
                    let _ = request.respond_to.send(decision);
                }
            }
            UiAction::LoadSessions => {
                let sessions = runtime.client.list_sessions(None).await.map_err(|error| {
                    io::Error::other(format!("could not list sessions: {error}"))
                })?;
                app.set_sessions(sessions);
            }
            UiAction::LoadModels => {
                app.set_models(runtime.models.clone());
            }
            UiAction::LoadUsage => {
                let metrics = match runtime.session.rpc().usage().get_metrics().await {
                    Ok(metrics) => metrics,
                    Err(error) => {
                        app.apply(crate::events::EventUpdate::Banner {
                            severity: crate::events::BannerSeverity::RecoverableError,
                            message: format!("could not load usage metrics: {error}"),
                            url: None,
                        });
                        continue;
                    }
                };
                let context_attribution = runtime
                    .session
                    .rpc()
                    .metadata()
                    .get_context_attribution()
                    .await
                    .ok()
                    .and_then(|result| context_attribution_snapshot(&result));
                app.set_usage(usage_metrics_snapshot(&metrics), context_attribution);
            }
            UiAction::LoadTodos => {
                load_todos(app, runtime).await?;
            }
            UiAction::Resume(session_id) => {
                if let Err(error) = runtime.resume(session_id).await {
                    app.apply(crate::events::EventUpdate::Banner {
                        severity: crate::events::BannerSeverity::RecoverableError,
                        message: error.to_string(),
                        url: None,
                    });
                } else {
                    *events = runtime.session.subscribe();
                    app.apply(crate::events::EventUpdate::Banner {
                        severity: crate::events::BannerSeverity::Warning,
                        message: "session resumed".to_string(),
                        url: None,
                    });
                }
            }
            UiAction::SwitchModel(selection) => {
                let model = selection.model.clone();
                let options = match selection.sdk_options() {
                    Ok(options) => options,
                    Err(error) => {
                        app.apply(crate::events::EventUpdate::Banner {
                            severity: crate::events::BannerSeverity::RecoverableError,
                            message: error,
                            url: None,
                        });
                        continue;
                    }
                };
                if let Err(error) = runtime.session.set_model(&model, options).await {
                    app.apply(crate::events::EventUpdate::Banner {
                        severity: crate::events::BannerSeverity::RecoverableError,
                        message: format!("could not switch model: {error}"),
                        url: None,
                    });
                } else {
                    runtime.set_active_model_options(
                        model.clone(),
                        selection.reasoning_effort,
                        selection.context_tier,
                    );
                    app.apply(crate::events::EventUpdate::ModelChanged { model });
                }
            }
            UiAction::Send(prompt) => {
                app.add_user_message(prompt.clone());
                if let Err(error) = runtime.session.send(prompt).await {
                    app.apply(crate::events::EventUpdate::Banner {
                        severity: crate::events::BannerSeverity::BlockingError,
                        message: format!("message could not be sent: {error}"),
                        url: None,
                    });
                }
            }
        }
    }
    Ok(())
}

async fn load_todos(app: &mut App, runtime: &AppRuntime) -> io::Result<()> {
    match runtime
        .session
        .rpc()
        .plan()
        .read_sql_todos_with_dependencies()
        .await
    {
        Ok(result) => app.set_todos(todo_snapshot(&result)),
        Err(error) => app.apply(crate::events::EventUpdate::Banner {
            severity: crate::events::BannerSeverity::RecoverableError,
            message: format!("could not load Fleet todos: {error}"),
            url: None,
        }),
    }
    Ok(())
}

async fn refresh_todos_if_requested(app: &mut App, runtime: &AppRuntime) -> io::Result<()> {
    if !app.take_todo_refresh_request() || !app.fleet_active || !app.todo_modal_is_open() {
        return Ok(());
    }
    load_todos(app, runtime).await
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()
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
    let cost = status
        .usage_metrics
        .as_ref()
        .map(format_cost)
        .unwrap_or_else(|| "--".to_string());
    let label = format!(
        " picopilot | model: {model} | mode: autopilot/{mode} | context: {context} | cost: {cost} "
    );

    Paragraph::new(label).style(Style::default().fg(Color::White).bg(Color::Rgb(28, 38, 50)))
}

fn input_box(app: &App) -> Paragraph<'static> {
    if let Some(request) = app.pending_approval() {
        let prompt = format!(
            "{} ({}): {} | y allow once, n deny, a trust for session",
            request.category.label(),
            request.tool_name,
            request.details
        );
        return Paragraph::new(prompt)
            .style(Style::default().fg(Color::Rgb(255, 219, 129)))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(242, 177, 94)))
                    .title("approval"),
            )
            .wrap(Wrap { trim: false });
    }

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

fn draw_modal(frame: &mut Frame, app: &App) {
    let Some(modal) = app.modal else {
        return;
    };
    let area = centered_rect(70, 70, frame.area());
    frame.render_widget(ratatui::widgets::Clear, area);

    if matches!(modal, ModalKind::Usage) {
        frame.render_widget(
            Paragraph::new(usage_detail_lines(app))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Rgb(240, 177, 94)))
                        .title("usage and context | u or esc to close"),
                )
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }

    if matches!(modal, ModalKind::Todos) {
        frame.render_widget(
            Paragraph::new(todo_detail_lines(app))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Rgb(240, 177, 94)))
                        .title("fleet todos | t or esc to close"),
                )
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }

    let title = match modal {
        ModalKind::Sessions => "resume session",
        ModalKind::Models => "choose model | r reasoning, c context, enter, esc",
        ModalKind::Usage => "usage and context | u or esc to close",
        ModalKind::Todos => "fleet todos | t or esc to close",
    };
    let items: Vec<String> = match modal {
        ModalKind::Sessions => app
            .sessions
            .iter()
            .map(|session| {
                format!(
                    "{} | {}",
                    session.modified_time,
                    session.summary.as_deref().unwrap_or("untitled session")
                )
            })
            .collect(),
        ModalKind::Models => app
            .models
            .iter()
            .enumerate()
            .map(|(index, model)| {
                let reasoning = if index == app.selected_item {
                    app.picker_reasoning_effort
                        .clone()
                        .unwrap_or_else(|| "default".to_string())
                } else {
                    model
                        .supported_reasoning_efforts
                        .as_ref()
                        .map(|values| values.join("/"))
                        .unwrap_or_else(|| "default".to_string())
                };
                let context = if index == app.selected_item {
                    app.picker_context_tier.as_deref().unwrap_or("default")
                } else {
                    "default"
                };
                format!(
                    "{} | {} | reasoning: {reasoning} | context: {context}",
                    model.id, model.name
                )
            })
            .collect(),
        ModalKind::Usage | ModalKind::Todos => Vec::new(),
    };
    let lines: Vec<Line<'static>> = if items.is_empty() {
        vec![Line::from("No entries available.")]
    } else {
        items
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                let style = if index == app.selected_item {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Rgb(240, 177, 94))
                } else {
                    Style::default().fg(Color::White)
                };
                Line::from(Span::styled(format!(" {item}"), style))
            })
            .collect()
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(240, 177, 94)))
                    .title(format!("{title} | up/down, enter, esc")),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn todo_detail_lines(app: &App) -> Vec<Line<'static>> {
    let Some(todos) = app.todos.as_ref() else {
        return vec![Line::from("Fleet todo data unavailable.")];
    };
    if todos.rows.is_empty() {
        return vec![Line::from("No Fleet todos available.")];
    }

    todos
        .rows
        .iter()
        .map(|row| {
            let blocked_by: Vec<String> = todos
                .dependencies
                .iter()
                .filter(|dependency| dependency.todo_id == row.id)
                .map(|dependency| {
                    todos
                        .rows
                        .iter()
                        .find(|candidate| candidate.id == dependency.depends_on)
                        .map(|candidate| candidate.title.clone())
                        .unwrap_or_else(|| dependency.depends_on.clone())
                })
                .collect();
            let dependency_label = if blocked_by.is_empty() {
                String::new()
            } else {
                format!(" | blocked by: {}", blocked_by.join(", "))
            };
            Line::from(format!(
                "[{}] {}{}",
                row.status, row.title, dependency_label
            ))
        })
        .collect()
}

fn usage_detail_lines(app: &App) -> Vec<Line<'static>> {
    let Some(metrics) = app.status.usage_metrics.as_ref() else {
        return vec![Line::from("Usage metrics unavailable.")];
    };

    let mut lines = vec![
        Line::from(format!("Session cost: {}", format_cost(metrics))),
        Line::from(format!(
            "Premium request cost: {:.2}",
            metrics.total_premium_request_cost
        )),
        Line::from(format!("Requests: {}", metrics.total_user_requests)),
        Line::from(format!("API time: {} ms", metrics.total_api_duration_ms)),
    ];

    if let Some(usage) = app.status.usage.as_ref() {
        lines.push(Line::from(format!(
            "Context window: {} / {} tokens",
            format_count(usage.current_tokens),
            format_count(usage.token_limit)
        )));
    }

    if let Some(context) = app.status.context_attribution.as_ref() {
        lines.push(Line::from(format!(
            "Attribution: {} / {} tokens ({})",
            format_count(context.total_tokens),
            format_count(context.prompt_token_limit),
            context.model_id
        )));
        for category in &context.categories {
            let percentage = if context.total_tokens > 0 {
                category.tokens as f64 / context.total_tokens as f64 * 100.0
            } else {
                0.0
            };
            lines.push(Line::from(format!(
                "  {}: {} ({percentage:.1}%)",
                category.label,
                format_count(category.tokens)
            )));
        }
        lines.push(Line::from(format!("Compactions: {}", context.compactions)));
    } else {
        lines.push(Line::from("Context attribution unavailable."));
    }

    lines
}

fn format_cost(metrics: &UsageMetricsSnapshot) -> String {
    match metrics.total_nano_aiu {
        Some(cost) => format!("{cost:.1} nAIU"),
        None => format!("{:.1} premium", metrics.total_premium_request_cost),
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let width = area.width * percent_x / 100;
    let height = area.height * percent_y / 100;
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
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
    use github_copilot_sdk::types::{Model, SessionId, SessionMetadata};

    use super::{handle_key, App, ChatEntry, ModelSelection, UiAction};
    use crate::events::{EventUpdate, TodoDependencySnapshot, TodoRowSnapshot, TodoSnapshot};

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

    #[tokio::test]
    async fn resolves_the_visible_approval_only_on_a_key_press() {
        let mut app = App::new(None);
        let (respond_to, response) = tokio::sync::oneshot::channel();
        app.enqueue_approval(crate::permissions::ApprovalRequest {
            category: crate::permissions::ApprovalCategory::Shell,
            tool_name: "bash".to_string(),
            details: "cargo test".to_string(),
            respond_to,
        });

        assert_eq!(
            handle_key(
                &mut app,
                KeyEvent {
                    code: KeyCode::Char('y'),
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Release,
                    state: crossterm::event::KeyEventState::NONE,
                }
            ),
            UiAction::None
        );
        assert!(app.pending_approval().is_some());

        assert_eq!(
            handle_key(
                &mut app,
                KeyEvent {
                    code: KeyCode::Char('y'),
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: crossterm::event::KeyEventState::NONE,
                }
            ),
            UiAction::Approval(crate::permissions::ApprovalDecision::ApproveOnce)
        );
        let request = app
            .take_approval()
            .expect("approval should still be queued");
        request
            .respond_to
            .send(crate::permissions::ApprovalDecision::ApproveOnce)
            .expect("test response receiver should still be open");
        assert_eq!(
            response.await.expect("approval response should arrive"),
            crate::permissions::ApprovalDecision::ApproveOnce
        );
    }

    #[test]
    fn modal_selection_emits_resume_and_model_actions() {
        let mut app = App::new(None);
        app.set_sessions(vec![
            SessionMetadata {
                session_id: SessionId::from("session-1"),
                start_time: "2026-08-31T12:00:00Z".to_string(),
                modified_time: "2026-08-31T12:01:00Z".to_string(),
                summary: Some("first".to_string()),
                is_remote: false,
            },
            SessionMetadata {
                session_id: SessionId::from("session-2"),
                start_time: "2026-08-31T12:00:00Z".to_string(),
                modified_time: "2026-08-31T12:02:00Z".to_string(),
                summary: Some("second".to_string()),
                is_remote: false,
            },
        ]);

        assert_eq!(
            handle_key(&mut app, key(KeyCode::Down, KeyEventKind::Press)),
            UiAction::None
        );
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::Resume(SessionId::from("session-2"))
        );
        assert!(!app.modal_is_open());

        app.set_models(vec![Model {
            default_reasoning_effort: Some("low".to_string()),
            id: "gpt-5".to_string(),
            name: "GPT-5".to_string(),
            supported_context_tiers: Some(vec!["default".to_string(), "long_context".to_string()]),
            supported_reasoning_efforts: Some(vec!["low".to_string(), "high".to_string()]),
            ..Model::default()
        }]);
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('r'), KeyEventKind::Press)),
            UiAction::None
        );
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('c'), KeyEventKind::Press)),
            UiAction::None
        );
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::SwitchModel(ModelSelection {
                model: "gpt-5".to_string(),
                reasoning_effort: Some("low".to_string()),
                context_tier: Some("default".to_string()),
            })
        );
    }

    #[test]
    fn usage_key_requests_the_usage_detail_modal() {
        let mut app = App::new(None);

        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('u'), KeyEventKind::Press)),
            UiAction::LoadUsage
        );
    }

    #[test]
    fn usage_metrics_open_the_detail_modal_and_can_be_closed() {
        let mut app = App::new(None);
        app.set_usage(
            crate::events::UsageMetricsSnapshot {
                total_nano_aiu: Some(3.5),
                total_premium_request_cost: 2.0,
                total_user_requests: 4,
                total_api_duration_ms: 1250,
                current_model: Some("gpt-5".to_string()),
            },
            None,
        );

        assert!(app.modal_is_open());
        assert_eq!(
            app.status()
                .usage_metrics
                .as_ref()
                .and_then(|metrics| metrics.total_nano_aiu),
            Some(3.5)
        );
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('u'), KeyEventKind::Press)),
            UiAction::None
        );
        assert!(!app.modal_is_open());
    }

    #[test]
    fn todo_modal_is_only_available_for_an_active_fleet() {
        let mut app = App::new(None);

        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('t'), KeyEventKind::Press)),
            UiAction::None
        );

        app.set_fleet_active(true);
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('t'), KeyEventKind::Press)),
            UiAction::LoadTodos
        );

        app.set_todos(TodoSnapshot {
            rows: vec![TodoRowSnapshot {
                id: "todo-1".to_string(),
                title: "Inspect the transport".to_string(),
                description: String::new(),
                status: "in_progress".to_string(),
            }],
            dependencies: vec![TodoDependencySnapshot {
                todo_id: "todo-1".to_string(),
                depends_on: "todo-0".to_string(),
            }],
        });
        assert!(app.modal_is_open());

        app.apply(EventUpdate::TodosChanged);
        assert!(app.take_todo_refresh_request());
        assert!(!app.take_todo_refresh_request());

        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('t'), KeyEventKind::Press)),
            UiAction::None
        );
        assert!(!app.modal_is_open());
    }

    fn key(code: KeyCode, kind: KeyEventKind) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind,
            state: crossterm::event::KeyEventState::NONE,
        }
    }
}
