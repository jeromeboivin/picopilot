use crate::events::{
    context_attribution_snapshot, todo_snapshot, usage_metrics_snapshot, BannerSeverity,
    ContextAttributionSnapshot, EventUpdate, TodoSnapshot, UsageMetricsSnapshot, UsageSnapshot,
};
use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use pulldown_cmark::{Alignment, Event as MarkdownEvent, Options, Parser, Tag, TagEnd};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use ratatui::Terminal;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

use github_copilot_sdk::rpc::{FleetStartRequest, FleetStartResult, TasksStartAgentRequest};
use github_copilot_sdk::subscription::EventSubscription;
use github_copilot_sdk::subscription::RecvErrorKind;
use github_copilot_sdk::types::{ContextTier, Model, SessionId, SessionMetadata, SetModelOptions};

use crate::input_editor::InputEditor;
use crate::markdown::assistant_markdown_lines;
use crate::palette;
use crate::permissions::{ApprovalDecision, ApprovalRequest};
use crate::runtime::{
    recovery_backoff, AppRuntime, RecoveryError, ResumeError, MAX_RECOVERY_ATTEMPTS,
};
use crate::screen_model::{
    enter_main_screen, render_transcript_payload, restore_main_screen, terminal_options,
    LiveEntryKind, Platform, ScreenChange, ScreenEntry, ScreenModel, ToolCallState,
    ToolHeaderPayload, ToolProgressKind, ToolProgressPayload, ToolResultPayload, ToolResultState,
    TranscriptPayload,
};
use crate::skills::{Skill, SkillCatalog, SkillSelection};
use crate::toolset::{Toolset, CANONICAL_TOOLS, TOOL_COUNT};

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
    StartFleet(String),
    NewConversation,
    Approval(ApprovalDecision),
    LoadSessions,
    LoadModels,
    LoadUsage,
    LoadTodos,
    LoadTools,
    LoadSkills,
    Resume(SessionId),
    SwitchModel(ModelSelection),
    ApplyToolset(Toolset),
    ApplySkills(SkillSelection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModalKind {
    Sessions,
    Models,
    Usage,
    Todos,
    Tools,
    Skills,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEntry {
    User(String),
    Diagnostic(String),
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
        arguments: Option<serde_json::Value>,
        success: Option<bool>,
        state: ToolCallState,
        unknown: bool,
        agent_id: Option<String>,
        started_at: Instant,
        cwd: PathBuf,
    },
    ToolProgress {
        tool_call_id: String,
        tool_name: String,
        content: String,
        kind: ToolProgressKind,
        agent_id: Option<String>,
    },
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        arguments: Option<serde_json::Value>,
        content: String,
        state: ToolResultState,
        agent_id: Option<String>,
        cwd: PathBuf,
    },
    Subagent {
        name: String,
        tool_call_id: String,
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
    Approval {
        category: String,
        tool_name: String,
        details: String,
        status: ApprovalStatus,
    },
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubagentStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalStatus {
    Pending,
    ApprovedOnce,
    Denied,
    Trusted,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatusState {
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub usage: Option<UsageSnapshot>,
    pub usage_metrics: Option<UsageMetricsSnapshot>,
    pub context_attribution: Option<ContextAttributionSnapshot>,
    pub busy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletionCandidate {
    command: String,
    description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletionState {
    candidates: Vec<CompletionCandidate>,
    selected_item: usize,
    token_start: usize,
    token_end: usize,
}

const BUILTIN_COMMANDS: &[(&str, &str)] = &[("/fleet", "run work through Fleet")];

static NEXT_SCREEN_NAMESPACE: AtomicU64 = AtomicU64::new(1);

fn next_screen_namespace() -> u64 {
    NEXT_SCREEN_NAMESPACE.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Default)]
pub struct App {
    entries: Vec<ChatEntry>,
    entry_ids: Vec<String>,
    screen_namespace: u64,
    next_entry_sequence: u64,
    pending_screen_changes: VecDeque<ScreenChange>,
    pending_user_messages: VecDeque<String>,
    working_directory: PathBuf,
    project_name: Option<String>,
    status: StatusState,
    input: InputEditor,
    pending_approvals: VecDeque<ApprovalRequest>,
    show_approval_details: bool,
    modal: Option<ModalKind>,
    sessions: Vec<SessionMetadata>,
    models: Vec<Model>,
    local_model_ids: HashSet<String>,
    selected_item: usize,
    picker_reasoning_effort: Option<String>,
    picker_context_tier: Option<String>,
    toolset: Toolset,
    picker_toolset: Toolset,
    skill_catalog: SkillCatalog,
    skill_selection: SkillSelection,
    picker_skill_selection: SkillSelection,
    completion: Option<CompletionState>,
    fleet_active: bool,
    todos: Option<TodoSnapshot>,
    todo_refresh_requested: bool,
    reconnecting: bool,
    blocked: bool,
    show_internals: bool,
    assistant_live_ids: HashSet<String>,
    reasoning_live_ids: HashSet<String>,
    should_quit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendPath {
    Fleet,
    Single,
}

async fn send_with_fleet_fallback<
    FleetStart,
    FleetFuture,
    SingleStart,
    SingleFuture,
    Error,
    IsTransportFailure,
>(
    prompt: String,
    fleet_start: FleetStart,
    single_start: SingleStart,
    is_transport_failure: IsTransportFailure,
) -> Result<SendPath, Error>
where
    FleetStart: FnOnce(FleetStartRequest) -> FleetFuture,
    FleetFuture: Future<Output = Result<FleetStartResult, Error>>,
    SingleStart: FnOnce(String) -> SingleFuture,
    SingleFuture: Future<Output = Result<(), Error>>,
    IsTransportFailure: Fn(&Error) -> bool,
{
    match fleet_start(FleetStartRequest {
        prompt: Some(prompt.clone()),
    })
    .await
    {
        Ok(result) if result.started => Ok(SendPath::Fleet),
        Err(error) if is_transport_failure(&error) => Err(error),
        Ok(_) | Err(_) => {
            single_start(prompt).await?;
            Ok(SendPath::Single)
        }
    }
}

impl App {
    pub fn new(model: Option<String>) -> Self {
        let mut app = Self {
            status: StatusState {
                model,
                ..StatusState::default()
            },
            working_directory: std::env::current_dir().unwrap_or_default(),
            ..Self::default()
        };
        app.screen_namespace = next_screen_namespace();
        app
    }

    pub fn new_with_working_directory(model: Option<String>, working_directory: &Path) -> Self {
        let mut app = Self::new(model);
        app.project_name = Some(working_directory_name(working_directory));
        app.working_directory = working_directory.to_path_buf();
        app
    }

    pub fn entries(&self) -> &[ChatEntry] {
        &self.entries
    }

    pub fn take_screen_changes(&mut self) -> Vec<ScreenChange> {
        self.pending_screen_changes.drain(..).collect()
    }

    fn allocate_entry_id(&mut self) -> String {
        if self.screen_namespace == 0 {
            self.screen_namespace = next_screen_namespace();
        }
        let id = format!(
            "screen-{}-{}",
            self.screen_namespace, self.next_entry_sequence
        );
        self.next_entry_sequence += 1;
        id
    }

    fn push_entry(&mut self, entry: ChatEntry) {
        let id = self.allocate_entry_id();
        self.entries.push(entry);
        self.entry_ids.push(id);
        self.queue_screen_change(self.entries.len() - 1);
    }

    fn screen_entry_at(&self, index: usize) -> Option<ScreenEntry> {
        let entry = self.entries.get(index)?;
        let (kind, completed) = match entry {
            ChatEntry::User(_) => (LiveEntryKind::User, true),
            ChatEntry::Diagnostic(_) | ChatEntry::Banner { .. } => (LiveEntryKind::Other, true),
            ChatEntry::Assistant {
                message_id,
                agent_id,
                ..
            } => (
                if agent_id.is_some() {
                    LiveEntryKind::AssistantNested
                } else {
                    LiveEntryKind::Assistant
                },
                !self.assistant_live_ids.contains(message_id),
            ),
            ChatEntry::Reasoning { reasoning_id, .. } => (
                LiveEntryKind::Other,
                !self.reasoning_live_ids.contains(reasoning_id),
            ),
            ChatEntry::Tool {
                state,
                unknown,
                agent_id,
                ..
            } => (
                if agent_id.is_some() {
                    LiveEntryKind::ToolNested
                } else {
                    LiveEntryKind::Tool
                },
                !matches!(state, ToolCallState::Queued | ToolCallState::Running) || *unknown,
            ),
            ChatEntry::ToolProgress { agent_id, .. } => (
                if agent_id.is_some() {
                    LiveEntryKind::ToolNested
                } else {
                    LiveEntryKind::Tool
                },
                false,
            ),
            ChatEntry::ToolResult { agent_id, .. } => (
                if agent_id.is_some() {
                    LiveEntryKind::ToolNested
                } else {
                    LiveEntryKind::Tool
                },
                true,
            ),
            ChatEntry::Subagent { status, .. } => (
                LiveEntryKind::Other,
                !matches!(status, SubagentStatus::Running),
            ),
            ChatEntry::Approval { status, .. } => (
                LiveEntryKind::Other,
                !matches!(status, ApprovalStatus::Pending),
            ),
            ChatEntry::Completed => return None,
        };
        let payload = entry_payload(entry, self.show_internals)?;
        Some(ScreenEntry::with_payload(
            self.entry_ids[index].clone(),
            kind,
            payload,
            completed,
        ))
    }

    fn queue_screen_change(&mut self, index: usize) {
        let id = self.entry_ids[index].clone();
        if let Some(entry) = self.screen_entry_at(index) {
            self.pending_screen_changes
                .push_back(ScreenChange::Upsert(entry));
        } else {
            self.pending_screen_changes
                .push_back(ScreenChange::Remove(id));
        }
    }

    fn queue_all_screen_changes(&mut self) {
        for index in 0..self.entries.len() {
            self.queue_screen_change(index);
        }
    }

    pub fn status(&self) -> &StatusState {
        &self.status
    }

    pub fn input(&self) -> &str {
        self.input.text()
    }

    fn input_cursor_byte_offset(&self) -> usize {
        self.input.cursor_byte_offset()
    }

    pub fn pending_approval(&self) -> Option<&ApprovalRequest> {
        self.pending_approvals.front()
    }

    pub fn modal_is_open(&self) -> bool {
        self.modal.is_some()
    }

    pub fn toolset(&self) -> Toolset {
        self.toolset
    }

    pub fn set_toolset(&mut self, toolset: Toolset) {
        self.toolset = toolset;
        self.picker_toolset = toolset;
    }

    pub fn set_model(&mut self, model: Option<String>) {
        self.status.model = model;
    }

    pub fn open_tool_picker(&mut self) {
        self.show_approval_details = false;
        self.picker_toolset = self.toolset;
        self.selected_item = 0;
        self.completion = None;
        self.modal = Some(ModalKind::Tools);
    }

    pub fn set_skill_catalog(&mut self, catalog: SkillCatalog) {
        self.skill_catalog = catalog;
        self.picker_skill_selection = self.skill_selection.clone();
        self.refresh_completion();
    }

    pub fn set_skill_selection(&mut self, selection: SkillSelection) {
        self.skill_selection = SkillSelection::from_names(
            &self.skill_catalog,
            selection.selected_names().iter().map(String::as_str),
        );
        self.picker_skill_selection = self.skill_selection.clone();
    }

    pub fn skill_selection(&self) -> &SkillSelection {
        &self.skill_selection
    }

    pub fn open_skill_picker(&mut self) {
        self.show_approval_details = false;
        self.picker_skill_selection = self.skill_selection.clone();
        self.selected_item = 0;
        self.completion = None;
        self.modal = Some(ModalKind::Skills);
    }

    fn todo_modal_is_open(&self) -> bool {
        matches!(self.modal, Some(ModalKind::Todos))
    }

    pub fn set_sessions(&mut self, sessions: Vec<SessionMetadata>) {
        self.sessions = sessions;
        self.selected_item = 0;
        self.completion = None;
        self.modal = Some(ModalKind::Sessions);
    }

    pub fn set_models(&mut self, models: Vec<Model>) {
        self.models = models;
        self.selected_item = self
            .status
            .model
            .as_ref()
            .and_then(|active| self.models.iter().position(|model| &model.id == active))
            .unwrap_or(0);
        self.reset_picker_options();
        self.completion = None;
        self.modal = Some(ModalKind::Models);
    }

    pub fn set_local_model_ids<I>(&mut self, model_ids: I)
    where
        I: IntoIterator<Item = String>,
    {
        self.local_model_ids = model_ids.into_iter().collect();
    }

    pub fn set_usage(
        &mut self,
        metrics: UsageMetricsSnapshot,
        context_attribution: Option<ContextAttributionSnapshot>,
    ) {
        self.status.usage_metrics = Some(metrics);
        self.status.context_attribution = context_attribution;
        self.selected_item = 0;
        self.completion = None;
        self.modal = Some(ModalKind::Usage);
    }

    pub fn set_usage_metrics(&mut self, metrics: UsageMetricsSnapshot) {
        self.status.usage_metrics = Some(metrics);
    }

    pub fn set_reasoning_effort(&mut self, reasoning_effort: Option<String>) {
        self.status.reasoning_effort = reasoning_effort;
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
        self.completion = None;
        self.modal = Some(ModalKind::Todos);
    }

    pub fn take_todo_refresh_request(&mut self) -> bool {
        std::mem::take(&mut self.todo_refresh_requested)
    }

    pub fn set_reconnecting(&mut self, reconnecting: bool) {
        self.reconnecting = reconnecting;
    }

    pub fn mark_in_flight_tools_unknown(&mut self) {
        for index in 0..self.entries.len() {
            if let ChatEntry::Tool {
                success,
                state,
                unknown,
                ..
            } = &mut self.entries[index]
            {
                if success.is_none() {
                    *state = ToolCallState::Unknown;
                    *unknown = true;
                    self.queue_screen_change(index);
                }
            }
        }
    }

    fn close_modal(&mut self) {
        self.modal = None;
        self.selected_item = 0;
    }

    fn move_selection(&mut self, delta: isize) {
        let item_count = match self.modal {
            Some(ModalKind::Sessions) => self.sessions.len(),
            Some(ModalKind::Models) => self.models.len(),
            Some(ModalKind::Tools) => TOOL_COUNT,
            Some(ModalKind::Skills) => self.skill_catalog.skills().len(),
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

    fn is_local_model(&self, model_id: &str) -> bool {
        self.local_model_ids.contains(model_id)
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
            Some(ModalKind::Tools) => Some(UiAction::ApplyToolset(self.picker_toolset)),
            Some(ModalKind::Skills) => {
                Some(UiAction::ApplySkills(self.picker_skill_selection.clone()))
            }
            Some(ModalKind::Usage | ModalKind::Todos) => None,
            None => None,
        };
        self.close_modal();
        action.unwrap_or(UiAction::None)
    }

    fn toggle_selected_tool(&mut self) {
        if matches!(self.modal, Some(ModalKind::Tools)) {
            let _ = self.picker_toolset.toggle_at(self.selected_item);
        }
    }

    fn choose_shell_only(&mut self) {
        if matches!(self.modal, Some(ModalKind::Tools)) {
            self.picker_toolset = Toolset::shell_only();
        }
    }

    fn choose_all_tools(&mut self) {
        if matches!(self.modal, Some(ModalKind::Tools)) {
            self.picker_toolset = Toolset::all();
        }
    }

    fn toggle_selected_skill(&mut self) {
        if let Some(skill) = self.skill_catalog.skills().get(self.selected_item) {
            self.picker_skill_selection
                .toggle(&self.skill_catalog, &skill.name);
        }
    }

    fn choose_no_skills(&mut self) {
        if matches!(self.modal, Some(ModalKind::Skills)) {
            self.picker_skill_selection.clear();
        }
    }

    fn choose_all_skills(&mut self) {
        if matches!(self.modal, Some(ModalKind::Skills)) {
            self.picker_skill_selection.select_all(&self.skill_catalog);
        }
    }

    fn toolset_change_is_blocked(&self) -> bool {
        self.blocked || self.reconnecting || self.status.busy || self.pending_approval().is_some()
    }

    fn skill_selection_change_is_blocked(&self) -> bool {
        self.blocked || self.reconnecting || self.status.busy || self.pending_approval().is_some()
    }

    pub fn enqueue_approval(&mut self, request: ApprovalRequest) {
        self.push_entry(ChatEntry::Approval {
            category: request.category.label().to_string(),
            tool_name: request.tool_name.clone(),
            details: request.details.clone(),
            status: ApprovalStatus::Pending,
        });
        self.pending_approvals.push_back(request);
    }

    fn resolve_approval(&mut self, decision: ApprovalDecision) -> Option<ApprovalRequest> {
        let request = self.pending_approvals.pop_front()?;
        if let Some(index) = self.entries.iter().position(|entry| {
            matches!(
                entry,
                ChatEntry::Approval {
                    status: ApprovalStatus::Pending,
                    ..
                }
            )
        }) {
            if let ChatEntry::Approval { status, .. } = &mut self.entries[index] {
                *status = match decision {
                    ApprovalDecision::ApproveOnce => ApprovalStatus::ApprovedOnce,
                    ApprovalDecision::Deny => ApprovalStatus::Denied,
                    ApprovalDecision::Trust => ApprovalStatus::Trusted,
                };
            }
            self.queue_screen_change(index);
        }
        self.show_approval_details = false;
        Some(request)
    }

    fn reject_pending_approvals(&mut self) {
        while let Some(request) = self.pending_approvals.pop_front() {
            let _ = request.respond_to.send(ApprovalDecision::Deny);
        }
        for index in 0..self.entries.len() {
            if matches!(
                self.entries[index],
                ChatEntry::Approval {
                    status: ApprovalStatus::Pending,
                    ..
                }
            ) {
                if let ChatEntry::Approval { status, .. } = &mut self.entries[index] {
                    *status = ApprovalStatus::Denied;
                }
                self.queue_screen_change(index);
            }
        }
        self.show_approval_details = false;
    }

    pub fn push_input(&mut self, character: char) {
        self.input.insert_char(character);
        self.refresh_completion();
    }

    pub fn pop_input(&mut self) {
        self.input.backspace();
        self.refresh_completion();
    }

    fn insert_newline(&mut self) {
        self.input.insert_newline();
        self.refresh_completion();
    }

    fn insert_paste(&mut self, pasted: &str) {
        self.input.insert_paste(pasted);
        self.refresh_completion();
    }

    fn move_input_left(&mut self) {
        self.input.move_left();
        self.refresh_completion();
    }

    fn move_input_right(&mut self) {
        self.input.move_right();
        self.refresh_completion();
    }

    fn move_input_up(&mut self) {
        self.input.move_up();
        self.refresh_completion();
    }

    fn move_input_down(&mut self) {
        self.input.move_down();
        self.refresh_completion();
    }

    fn move_input_home(&mut self, all_lines: bool) {
        self.input.move_home(all_lines);
        self.refresh_completion();
    }

    fn move_input_end(&mut self, all_lines: bool) {
        self.input.move_end(all_lines);
        self.refresh_completion();
    }

    fn delete_input(&mut self) {
        self.input.delete();
        self.refresh_completion();
    }

    pub fn take_input(&mut self) -> String {
        self.completion = None;
        self.input.take()
    }

    pub fn reset_for_new_conversation(&mut self) {
        self.reject_pending_approvals();
        self.reset_screen_lifecycle();
        self.pending_user_messages.clear();
        self.input.clear();
        self.status.usage = None;
        self.status.usage_metrics = None;
        self.status.context_attribution = None;
        self.status.busy = false;
        self.close_modal();
        self.picker_reasoning_effort = None;
        self.picker_context_tier = None;
        self.picker_toolset = self.toolset;
        self.skill_selection.clear();
        self.picker_skill_selection.clear();
        self.completion = None;
        self.fleet_active = false;
        self.todos = None;
        self.todo_refresh_requested = false;
        self.reconnecting = false;
        self.blocked = false;
        self.assistant_live_ids.clear();
        self.reasoning_live_ids.clear();
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn add_user_message(&mut self, content: String) {
        self.pending_user_messages.push_back(content.clone());
        self.push_entry(ChatEntry::User(content));
        self.status.busy = true;
    }

    fn add_diagnostic(&mut self, message: impl Into<String>) {
        self.push_entry(ChatEntry::Diagnostic(message.into()));
    }

    pub fn replace_history(&mut self, events: &[github_copilot_sdk::types::SessionEvent]) {
        self.reset_screen_lifecycle();
        self.pending_user_messages.clear();
        self.assistant_live_ids.clear();
        self.reasoning_live_ids.clear();
        self.status.busy = false;
        self.blocked = false;
        self.completion = None;
        for event in events {
            if let Some(update) = crate::events::event_update(event) {
                self.apply(update);
            }
        }
    }

    fn reset_screen_lifecycle(&mut self) {
        self.entries.clear();
        self.entry_ids.clear();
        self.screen_namespace = next_screen_namespace();
        self.next_entry_sequence = 0;
        self.pending_screen_changes.clear();
        self.pending_screen_changes.push_back(ScreenChange::Reset);
    }

    pub fn apply(&mut self, update: EventUpdate) {
        match update {
            EventUpdate::UserMessage { content } => {
                if let Some(pending) = self.pending_user_messages.pop_front() {
                    if let Some(index) = self
                        .entries
                        .iter()
                        .rev()
                        .position(|entry| matches!(entry, ChatEntry::User(value) if value == &pending))
                        .map(|offset| self.entries.len() - 1 - offset)
                    {
                        if let ChatEntry::User(current) = &mut self.entries[index] {
                            *current = content;
                        }
                        self.queue_screen_change(index);
                    } else {
                        self.push_entry(ChatEntry::User(content));
                    }
                } else {
                    self.push_entry(ChatEntry::User(content));
                }
            }
            EventUpdate::AssistantDelta {
                message_id,
                content,
                agent_id,
            } => {
                self.assistant_live_ids.insert(message_id.clone());
                self.append_assistant(message_id, content, agent_id);
            }
            EventUpdate::AssistantMessage {
                message_id,
                content,
                agent_id,
            } => {
                self.assistant_live_ids.remove(&message_id);
                self.replace_assistant(message_id, content, agent_id);
            }
            EventUpdate::ReasoningDelta {
                reasoning_id,
                content,
                agent_id,
            } => {
                self.reasoning_live_ids.insert(reasoning_id.clone());
                self.append_reasoning(reasoning_id, content, agent_id);
            }
            EventUpdate::Reasoning {
                reasoning_id,
                content,
                agent_id,
            } => {
                self.reasoning_live_ids.remove(&reasoning_id);
                self.replace_reasoning(reasoning_id, content, agent_id);
            }
            EventUpdate::ToolStarted {
                tool_call_id,
                tool_name,
                arguments,
                agent_id,
            } => {
                if self.tool_header_index(&tool_call_id).is_none() {
                    let started_at = Instant::now();
                    self.push_entry(ChatEntry::Tool {
                        tool_call_id: tool_call_id.clone(),
                        tool_name: tool_name.clone(),
                        arguments,
                        success: None,
                        state: ToolCallState::Running,
                        unknown: false,
                        agent_id: agent_id.clone(),
                        started_at,
                        cwd: self.working_directory.clone(),
                    });
                    self.push_entry(ChatEntry::ToolProgress {
                        tool_call_id,
                        tool_name,
                        content: String::new(),
                        kind: ToolProgressKind::Tool,
                        agent_id,
                    });
                }
            }
            EventUpdate::ToolOutput {
                tool_call_id,
                content,
                agent_id: _,
            } => {
                if let Some(index) = self
                    .entries
                    .iter()
                    .position(|entry| matches!(entry, ChatEntry::ToolProgress { tool_call_id: id, .. } if id == &tool_call_id))
                {
                    if let ChatEntry::ToolProgress { content: current, .. } =
                        &mut self.entries[index]
                    {
                        current.push_str(&content);
                    }
                    self.queue_screen_change(index);
                }
            }
            EventUpdate::ToolProgress {
                tool_call_id,
                content,
                agent_id: _,
            } => {
                if let Some(index) = self
                    .entries
                    .iter()
                    .position(|entry| matches!(entry, ChatEntry::ToolProgress { tool_call_id: id, .. } if id == &tool_call_id))
                {
                    if let ChatEntry::ToolProgress { content: current, .. } =
                        &mut self.entries[index]
                    {
                        *current = content;
                    }
                    self.queue_screen_change(index);
                }
            }
            EventUpdate::ToolCompleted {
                tool_call_id,
                success,
                message,
                agent_id: _,
            } => self.complete_tool(tool_call_id, success, message, false),
            EventUpdate::ToolCancelled {
                tool_call_id,
                message,
                agent_id: _,
            } => self.complete_tool(tool_call_id, false, message, true),
            EventUpdate::SubagentStarted {
                name,
                display_name,
                tool_call_id,
                agent_id,
            } => self.push_entry(ChatEntry::Subagent {
                name,
                tool_call_id,
                display_name,
                status: SubagentStatus::Running,
                error: None,
                agent_id,
            }),
            EventUpdate::SubagentCompleted {
                name,
                tool_call_id,
                agent_id,
            } => {
                if let Some(index) =
                    self.subagent_index(&name, &tool_call_id, agent_id.as_deref())
                {
                    if let ChatEntry::Subagent { status, .. } = &mut self.entries[index] {
                        *status = SubagentStatus::Completed;
                    }
                    self.queue_screen_change(index);
                }
            }
            EventUpdate::SubagentFailed {
                name,
                tool_call_id,
                error,
                agent_id,
            } => {
                if let Some(index) =
                    self.subagent_index(&name, &tool_call_id, agent_id.as_deref())
                {
                    if let ChatEntry::Subagent {
                        status,
                        error: current_error,
                        ..
                    } = &mut self.entries[index]
                    {
                        *status = SubagentStatus::Failed;
                        *current_error = Some(error);
                    }
                    self.queue_screen_change(index);
                }
            }
            EventUpdate::Usage(usage) => self.status.usage = Some(usage),
            EventUpdate::Banner {
                severity,
                message,
                url,
            } => {
                if severity == BannerSeverity::BlockingError {
                    self.blocked = true;
                    self.status.busy = false;
                    self.reject_pending_approvals();
                }
                self.push_entry(ChatEntry::Banner {
                    severity,
                    message,
                    url,
                });
            }
            EventUpdate::ModelChanged { model } => self.status.model = Some(model),
            EventUpdate::TodosChanged => {
                if self.fleet_active && matches!(self.modal, Some(ModalKind::Todos)) {
                    self.todo_refresh_requested = true;
                }
            }
            EventUpdate::Idle | EventUpdate::TaskComplete => {
                let completed_indices = self
                    .entries
                    .iter()
                    .enumerate()
                    .filter_map(|(index, entry)| match entry {
                        ChatEntry::Assistant { message_id, .. }
                            if self.assistant_live_ids.contains(message_id) => Some(index),
                        ChatEntry::Reasoning { reasoning_id, .. }
                            if self.reasoning_live_ids.contains(reasoning_id) => Some(index),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                self.assistant_live_ids.clear();
                self.reasoning_live_ids.clear();
                self.set_fleet_active(false);
                self.status.busy = false;
                for index in completed_indices {
                    self.queue_screen_change(index);
                }
                if !matches!(self.entries.last(), Some(ChatEntry::Completed)) {
                    self.push_entry(ChatEntry::Completed);
                }
            }
        }
    }

    fn tool_header_index(&self, tool_call_id: &str) -> Option<usize> {
        self.entries.iter().position(|entry| {
            matches!(
                entry,
                ChatEntry::Tool {
                    tool_call_id: current,
                    ..
                } if current == tool_call_id
            )
        })
    }

    fn complete_tool(
        &mut self,
        tool_call_id: String,
        success: bool,
        message: Option<String>,
        cancelled: bool,
    ) {
        let Some(index) = self.tool_header_index(&tool_call_id) else {
            return;
        };
        let Some((tool_name, arguments, agent_id, cwd)) =
            self.entries.get(index).and_then(|entry| {
                let ChatEntry::Tool {
                    tool_name,
                    arguments,
                    agent_id,
                    state,
                    cwd,
                    ..
                } = entry
                else {
                    return None;
                };
                if matches!(
                    state,
                    ToolCallState::Success | ToolCallState::Error | ToolCallState::Cancelled
                ) {
                    return None;
                }
                Some((
                    tool_name.clone(),
                    arguments.clone(),
                    agent_id.clone(),
                    cwd.clone(),
                ))
            })
        else {
            return;
        };

        if let ChatEntry::Tool {
            success: current_success,
            state,
            unknown,
            ..
        } = &mut self.entries[index]
        {
            *current_success = Some(success);
            *state = if cancelled {
                ToolCallState::Cancelled
            } else if success {
                ToolCallState::Success
            } else {
                ToolCallState::Error
            };
            *unknown = false;
        }
        self.queue_screen_change(index);

        for progress_index in (0..self.entries.len()).rev() {
            if matches!(
                self.entries.get(progress_index),
                Some(ChatEntry::ToolProgress {
                    tool_call_id: current,
                    ..
                }) if current == &tool_call_id
            ) {
                self.remove_entry_at(progress_index);
            }
        }

        self.push_entry(ChatEntry::ToolResult {
            tool_call_id,
            tool_name,
            arguments,
            content: message.unwrap_or_default(),
            state: if cancelled {
                ToolResultState::Cancelled
            } else if success {
                ToolResultState::Success
            } else {
                ToolResultState::Error
            },
            agent_id,
            cwd,
        });
    }

    fn remove_entry_at(&mut self, index: usize) {
        if index >= self.entries.len() {
            return;
        }
        self.entries.remove(index);
        let id = self.entry_ids.remove(index);
        self.pending_screen_changes
            .push_back(ScreenChange::Remove(id));
    }

    fn subagent_index(
        &self,
        name: &str,
        tool_call_id: &str,
        agent_id: Option<&str>,
    ) -> Option<usize> {
        self.entries
            .iter()
            .enumerate()
            .rev()
            .find(|(_, entry)| {
                matches!(
                    entry,
                    ChatEntry::Subagent {
                        name: current,
                        tool_call_id: current_tool_call_id,
                        status: SubagentStatus::Running,
                        agent_id: current_agent,
                        ..
                    } if current == name
                        && current_tool_call_id == tool_call_id
                        && (agent_id.is_none() || current_agent.as_deref() == agent_id)
                )
            })
            .map(|(index, _)| index)
    }

    fn append_assistant(&mut self, message_id: String, content: String, agent_id: Option<String>) {
        if let Some(index) = self
            .entries
            .iter()
            .rev()
            .position(|entry| {
                matches!(entry, ChatEntry::Assistant { message_id: id, .. } if id == &message_id)
            })
            .map(|offset| self.entries.len() - 1 - offset)
        {
            if let ChatEntry::Assistant {
                content: current,
                ..
            } = &mut self.entries[index]
            {
                current.push_str(&content);
            }
            self.queue_screen_change(index);
        } else {
            self.push_entry(ChatEntry::Assistant {
                message_id,
                content,
                agent_id,
            });
        }
    }

    fn replace_assistant(&mut self, message_id: String, content: String, agent_id: Option<String>) {
        if let Some(index) = self
            .entries
            .iter()
            .rev()
            .position(|entry| {
                matches!(entry, ChatEntry::Assistant { message_id: id, .. } if id == &message_id)
            })
            .map(|offset| self.entries.len() - 1 - offset)
        {
            if let ChatEntry::Assistant {
                content: current,
                agent_id: current_agent,
                ..
            } = &mut self.entries[index]
            {
                *current = content;
                if current_agent.is_none() {
                    *current_agent = agent_id;
                }
            }
            self.queue_screen_change(index);
        } else {
            self.push_entry(ChatEntry::Assistant {
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
        if let Some(index) = self
            .entries
            .iter()
            .rev()
            .position(|entry| {
                matches!(entry, ChatEntry::Reasoning { reasoning_id: id, .. } if id == &reasoning_id)
            })
            .map(|offset| self.entries.len() - 1 - offset)
        {
            if let ChatEntry::Reasoning {
                content: current,
                ..
            } = &mut self.entries[index]
            {
                current.push_str(&content);
            }
            self.queue_screen_change(index);
        } else {
            self.push_entry(ChatEntry::Reasoning {
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
        if let Some(index) = self
            .entries
            .iter()
            .rev()
            .position(|entry| {
                matches!(entry, ChatEntry::Reasoning { reasoning_id: id, .. } if id == &reasoning_id)
            })
            .map(|offset| self.entries.len() - 1 - offset)
        {
            if let ChatEntry::Reasoning {
                content: current,
                agent_id: current_agent,
                ..
            } = &mut self.entries[index]
            {
                *current = content;
                if current_agent.is_none() {
                    *current_agent = agent_id;
                }
            }
            self.queue_screen_change(index);
        } else {
            self.push_entry(ChatEntry::Reasoning {
                reasoning_id,
                content,
                agent_id,
            });
        }
    }

    fn refresh_completion(&mut self) {
        let Some((token_start, token_end, prefix_end)) =
            slash_command_context(self.input(), self.input_cursor_byte_offset())
        else {
            self.completion = None;
            return;
        };

        let prefix = &self.input()[token_start..prefix_end];
        let prefix = prefix.to_ascii_lowercase();
        let mut candidates = self
            .command_candidates()
            .into_iter()
            .filter(|candidate| candidate.command.to_ascii_lowercase().starts_with(&prefix))
            .collect::<Vec<_>>();
        candidates.sort_by_key(|candidate| !candidate.command.eq_ignore_ascii_case(&prefix));
        if candidates.is_empty() {
            self.completion = None;
            return;
        }

        let selected_command = self.completion.as_ref().and_then(|completion| {
            completion
                .candidates
                .get(completion.selected_item)
                .map(|candidate| candidate.command.as_str())
        });
        let selected_item = candidates
            .iter()
            .position(|candidate| candidate.command.eq_ignore_ascii_case(&prefix))
            .or_else(|| {
                selected_command.and_then(|command| {
                    candidates
                        .iter()
                        .position(|candidate| candidate.command == command)
                })
            })
            .unwrap_or(0)
            .min(candidates.len().saturating_sub(1));
        self.completion = Some(CompletionState {
            candidates,
            selected_item,
            token_start,
            token_end,
        });
    }

    fn command_candidates(&self) -> Vec<CompletionCandidate> {
        let mut candidates = BUILTIN_COMMANDS
            .iter()
            .map(|(command, description)| CompletionCandidate {
                command: (*command).to_string(),
                description: (*description).to_string(),
            })
            .collect::<Vec<_>>();
        for skill in self.skill_catalog.user_invocable() {
            let command = format!("/{}", skill.name);
            if candidates
                .iter()
                .all(|candidate| candidate.command != command)
            {
                candidates.push(CompletionCandidate {
                    command,
                    description: skill.description.clone(),
                });
            }
        }
        candidates
    }

    fn move_completion(&mut self, delta: isize) {
        let Some(completion) = self.completion.as_mut() else {
            return;
        };
        completion.selected_item = (completion.selected_item as isize + delta)
            .rem_euclid(completion.candidates.len() as isize)
            as usize;
    }

    fn dismiss_completion(&mut self) {
        self.completion = None;
    }

    fn completion_is_incomplete(&self) -> bool {
        let Some(completion) = self.completion.as_ref() else {
            return false;
        };
        let token = &self.input()[completion.token_start..completion.token_end];
        let cursor = self.input_cursor_byte_offset();
        cursor != completion.token_end
            || completion
                .candidates
                .get(completion.selected_item)
                .is_some_and(|candidate| candidate.command != token)
    }

    fn accept_completion(&mut self) {
        let Some(completion) = self.completion.take() else {
            return;
        };
        let Some(candidate) = completion.candidates.get(completion.selected_item) else {
            return;
        };
        self.input.replace_range(
            completion.token_start,
            completion.token_end,
            &candidate.command,
        );
    }
}

fn slash_command_context(text: &str, cursor: usize) -> Option<(usize, usize, usize)> {
    if !text.starts_with('/') || cursor < 1 || cursor > text.len() {
        return None;
    }
    let token_end = text
        .find(|character: char| character.is_whitespace())
        .unwrap_or(text.len());
    if cursor > token_end {
        return None;
    }
    Some((0, token_end, cursor))
}

fn invoked_skill<'a>(catalog: &'a SkillCatalog, prompt: &str) -> Option<&'a Skill> {
    let token_end = prompt
        .find(|character: char| character.is_whitespace())
        .unwrap_or(prompt.len());
    let token = prompt.get(..token_end)?;
    let name = token.strip_prefix('/')?;
    if name.is_empty() {
        return None;
    }
    catalog.find(name).filter(|skill| skill.user_invocable)
}

fn skill_selection_for_invocation(
    catalog: &SkillCatalog,
    active: &SkillSelection,
    prompt: &str,
) -> Option<SkillSelection> {
    let skill = invoked_skill(catalog, prompt)?;
    if active.contains(&skill.name) {
        return None;
    }
    let mut selection = active.clone();
    selection.toggle(catalog, &skill.name);
    Some(selection)
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> UiAction {
    if key.kind != KeyEventKind::Press {
        return UiAction::None;
    }

    if key.code == KeyCode::Char('k') && key.modifiers == KeyModifiers::CONTROL {
        if !app.blocked {
            return UiAction::LoadTools;
        }
        return UiAction::None;
    }

    if key.code == KeyCode::Char('s')
        && key.modifiers == KeyModifiers::CONTROL
        && app.modal.is_none()
    {
        if !app.blocked {
            return UiAction::LoadSkills;
        }
        return UiAction::None;
    }

    if matches!(app.modal, Some(ModalKind::Tools)) {
        return match key.code {
            KeyCode::Char(' ') => {
                app.toggle_selected_tool();
                UiAction::None
            }
            KeyCode::Char('s') => {
                app.choose_shell_only();
                UiAction::None
            }
            KeyCode::Char('a') => {
                app.choose_all_tools();
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
            KeyCode::Enter => app.choose_selected(),
            _ => UiAction::None,
        };
    }

    if matches!(app.modal, Some(ModalKind::Skills)) {
        return match key.code {
            KeyCode::Char(' ') => {
                app.toggle_selected_skill();
                UiAction::None
            }
            KeyCode::Char('n') if key.modifiers == KeyModifiers::NONE => {
                app.choose_no_skills();
                UiAction::None
            }
            KeyCode::Char('a') if key.modifiers == KeyModifiers::NONE => {
                app.choose_all_skills();
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
            KeyCode::Enter => app.choose_selected(),
            _ => UiAction::None,
        };
    }

    if app.reconnecting {
        return UiAction::None;
    }

    if app.blocked {
        return match key.code {
            KeyCode::Char('x') if key.modifiers == KeyModifiers::CONTROL => UiAction::Quit,
            KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => UiAction::Quit,
            _ => UiAction::None,
        };
    }

    if app.pending_approval().is_some() {
        return match key.code {
            KeyCode::Char('y') => UiAction::Approval(ApprovalDecision::ApproveOnce),
            KeyCode::Char('n') if key.modifiers == KeyModifiers::NONE => {
                UiAction::Approval(ApprovalDecision::Deny)
            }
            KeyCode::Char('a')
                if app
                    .pending_approval()
                    .is_some_and(|request| request.category.supports_trust()) =>
            {
                UiAction::Approval(ApprovalDecision::Trust)
            }
            KeyCode::Char('v') => {
                app.show_approval_details = !app.show_approval_details;
                UiAction::None
            }
            _ => UiAction::None,
        };
    }

    if app.completion.is_some() {
        match key.code {
            KeyCode::Esc => {
                app.dismiss_completion();
                return UiAction::None;
            }
            KeyCode::Up => {
                app.move_completion(-1);
                return UiAction::None;
            }
            KeyCode::Down => {
                app.move_completion(1);
                return UiAction::None;
            }
            KeyCode::Tab => {
                app.accept_completion();
                return UiAction::None;
            }
            KeyCode::Enter
                if !is_multiline_enter(key, shift_is_pressed())
                    && app.completion_is_incomplete() =>
            {
                app.accept_completion();
                return UiAction::None;
            }
            _ => {}
        }
    }

    if app.modal.is_some() {
        return match key.code {
            KeyCode::Char('u')
                if key.modifiers == KeyModifiers::CONTROL
                    && matches!(app.modal, Some(ModalKind::Usage)) =>
            {
                app.close_modal();
                UiAction::None
            }
            KeyCode::Char('t')
                if key.modifiers == KeyModifiers::CONTROL
                    && matches!(app.modal, Some(ModalKind::Todos)) =>
            {
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
        KeyCode::Char('x') if key.modifiers == KeyModifiers::CONTROL => UiAction::Quit,
        KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => UiAction::Quit,
        KeyCode::Char('n') if key.modifiers == KeyModifiers::CONTROL && !app.status.busy => {
            UiAction::NewConversation
        }
        KeyCode::Char('o') if key.modifiers == KeyModifiers::CONTROL => UiAction::LoadSessions,
        KeyCode::Char('p') if key.modifiers == KeyModifiers::CONTROL => UiAction::LoadModels,
        KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => UiAction::LoadUsage,
        KeyCode::Char('t') if key.modifiers == KeyModifiers::CONTROL && app.fleet_active => {
            UiAction::LoadTodos
        }
        KeyCode::Char('i') if key.modifiers == KeyModifiers::CONTROL => {
            app.show_internals = !app.show_internals;
            app.queue_all_screen_changes();
            UiAction::None
        }
        KeyCode::Up => {
            app.move_input_up();
            UiAction::None
        }
        KeyCode::Down => {
            app.move_input_down();
            UiAction::None
        }
        KeyCode::Left => {
            app.move_input_left();
            UiAction::None
        }
        KeyCode::Right => {
            app.move_input_right();
            UiAction::None
        }
        KeyCode::Home => {
            app.move_input_home(false);
            UiAction::None
        }
        KeyCode::End => {
            app.move_input_end(false);
            UiAction::None
        }
        KeyCode::Delete => {
            app.delete_input();
            UiAction::None
        }
        KeyCode::Enter if is_multiline_enter(key, shift_is_pressed()) => {
            app.insert_newline();
            UiAction::None
        }
        KeyCode::Enter => {
            let input = app.take_input();
            if input.trim().is_empty() {
                UiAction::None
            } else if let Some(prompt) = input.strip_prefix("/fleet ") {
                let prompt = prompt.trim();
                if prompt.is_empty() {
                    UiAction::None
                } else {
                    UiAction::StartFleet(prompt.to_string())
                }
            } else {
                UiAction::Send(input)
            }
        }
        KeyCode::Backspace => {
            app.pop_input();
            UiAction::None
        }
        KeyCode::Char(_)
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            UiAction::None
        }
        KeyCode::Char(character) => {
            app.push_input(character);
            UiAction::None
        }
        _ => UiAction::None,
    }
}

fn is_multiline_enter(key: KeyEvent, shift_pressed: bool) -> bool {
    key.code == KeyCode::Enter && (key.modifiers.contains(KeyModifiers::SHIFT) || shift_pressed)
}

#[cfg(windows)]
fn shift_is_pressed() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SHIFT};

    unsafe { GetAsyncKeyState(VK_SHIFT as i32) < 0 }
}

#[cfg(not(windows))]
fn shift_is_pressed() -> bool {
    false
}

pub fn draw(frame: &mut Frame, app: &App) {
    draw_frame(frame, app, None, 0);
}

fn draw_with_screen(
    frame: &mut Frame,
    app: &App,
    screen: &mut ScreenModel,
    animation_started_at: Instant,
) {
    draw_frame(
        frame,
        app,
        Some(screen),
        animation_started_at.elapsed().as_millis() as u64,
    );
}

fn draw_frame(
    frame: &mut Frame,
    app: &App,
    screen: Option<&mut ScreenModel>,
    animation_elapsed_ms: u64,
) {
    let input_height = input_height(app, frame.area());
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(input_height),
            Constraint::Length(1),
        ])
        .split(frame.area());

    frame.render_widget(status_bar(app), layout[0]);
    if let Some(screen) = screen {
        draw_live_chat(frame, app, screen, layout[1], animation_elapsed_ms);
    } else {
        draw_chat(frame, app, layout[1]);
    }
    frame.render_widget(input_box(app, layout[2]), layout[2]);
    draw_completion(frame, app, layout[2]);
    frame.render_widget(shortcut_bar(), layout[3]);
    draw_modal(frame, app);

    if app.modal.is_none() && app.pending_approval().is_none() && !app.blocked && !app.reconnecting
    {
        let wrapped = wrap_input(
            app.input(),
            app.input_cursor_byte_offset(),
            layout[2].width as usize,
        );
        let visible_lines = layout[2].height.saturating_sub(2).max(1) as usize;
        let scroll = wrapped
            .cursor_row
            .saturating_sub(visible_lines.saturating_sub(1));
        let cursor_x = layout[2].x.saturating_add(
            wrapped
                .cursor_column
                .min(layout[2].width.saturating_sub(1) as usize) as u16,
        );
        let cursor_y = layout[2]
            .y
            .saturating_add(1)
            .saturating_add(
                wrapped
                    .cursor_row
                    .saturating_sub(scroll)
                    .min(u16::MAX as usize) as u16,
            )
            .min(layout[2].bottom().saturating_sub(1));
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

pub async fn run(runtime: AppRuntime, model: Option<String>) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    if let Err(error) = enter_main_screen(&mut stdout) {
        let _ = disable_raw_mode();
        let _ = restore_main_screen(&mut stdout);
        return Err(error);
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = match Terminal::with_options(backend, terminal_options()) {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = disable_raw_mode();
            let _ = restore_main_screen(&mut io::stdout());
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
    let reasoning_effort = displayed_reasoning_effort(
        &runtime.models,
        model.as_deref(),
        runtime.active_model_options.reasoning_effort.as_deref(),
    );
    let local_model_ids = runtime
        .provider_registry
        .as_ref()
        .map(|registry| registry.qualified_model_ids())
        .unwrap_or_default();
    let mut app = App::new_with_working_directory(model, &runtime.working_directory);
    app.set_local_model_ids(local_model_ids);
    app.set_toolset(runtime.active_toolset);
    app.set_skill_catalog(runtime.skill_catalog.clone());
    app.set_skill_selection(runtime.active_skill_selection.clone());
    for diagnostic in runtime.skill_catalog.diagnostics() {
        app.add_diagnostic(format!(
            "skill discovery: {} ({})",
            diagnostic.message,
            diagnostic.path.display()
        ));
    }
    app.set_reasoning_effort(reasoning_effort);
    let mut events = runtime.session.subscribe();
    let mut screen_model = ScreenModel::default();
    let mut permission_requests_open = true;
    let animation_started_at = Instant::now();
    let mut usage_refresh = tokio::time::interval(Duration::from_secs(2));
    usage_refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    while !app.should_quit() {
        for change in app.take_screen_changes() {
            screen_model.apply_change(terminal, change)?;
        }
        terminal
            .draw(|frame| draw_with_screen(frame, &app, &mut screen_model, animation_started_at))?;
        let tick = tokio::time::sleep(Duration::from_millis(50));
        tokio::pin!(tick);

        tokio::select! {
            result = events.recv() => match result {
                Ok(event) => {
                    if let Some(update) = crate::events::event_update(&event) {
                        if let EventUpdate::ModelChanged { model } = &update {
                            app.set_reasoning_effort(displayed_reasoning_effort(
                                &runtime.models,
                                Some(model),
                                runtime.active_model_options.reasoning_effort.as_deref(),
                            ));
                        }
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
                        recover_connection(&mut app, &mut runtime, &mut events).await?;
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
            _ = usage_refresh.tick() => {
                refresh_status_cost(&mut app, &mut runtime, &mut events).await?;
            },
            _ = &mut tick => {
                process_terminal_events(&mut app, &mut runtime, &mut events).await?;
                refresh_todos_if_requested(&mut app, &mut runtime, &mut events).await?;
            },
        }
    }

    Ok(())
}

async fn refresh_status_cost(
    app: &mut App,
    runtime: &mut AppRuntime,
    events: &mut EventSubscription,
) -> io::Result<()> {
    match runtime.session.rpc().usage().get_metrics().await {
        Ok(metrics) => app.set_usage_metrics(usage_metrics_snapshot(&metrics)),
        Err(error) if error.is_transport_failure() => {
            recover_connection(app, runtime, events).await?;
        }
        Err(_) => {}
    }
    Ok(())
}

async fn process_terminal_events(
    app: &mut App,
    runtime: &mut AppRuntime,
    events: &mut EventSubscription,
) -> io::Result<()> {
    while event::poll(Duration::ZERO)? {
        let event = event::read()?;
        let action = match event {
            Event::Paste(pasted)
                if app.modal.is_none()
                    && app.pending_approval().is_none()
                    && !app.blocked
                    && !app.reconnecting =>
            {
                app.insert_paste(&pasted);
                continue;
            }
            Event::Key(key) => handle_key(app, key),
            _ => continue,
        };

        match action {
            UiAction::None => {}
            UiAction::Quit => app.quit(),
            UiAction::Approval(decision) => {
                if let Some(request) = app.resolve_approval(decision) {
                    let _ = request.respond_to.send(decision);
                }
            }
            UiAction::LoadSessions => match runtime.client.list_sessions(None).await {
                Ok(sessions) => app.set_sessions(sessions),
                Err(error) if error.is_transport_failure() => {
                    recover_connection(app, runtime, events).await?;
                }
                Err(error) => app.apply(crate::events::EventUpdate::Banner {
                    severity: crate::events::BannerSeverity::RecoverableError,
                    message: format!("could not list sessions: {error}"),
                    url: None,
                }),
            },
            UiAction::NewConversation => match runtime.new_conversation().await {
                Ok(()) => {
                    *events = runtime.session.subscribe();
                    app.reset_for_new_conversation();
                    app.set_skill_selection(runtime.active_skill_selection.clone());
                    app.add_diagnostic("new conversation started");
                }
                Err(error) if error.is_transport_failure() => {
                    recover_connection(app, runtime, events).await?;
                }
                Err(error) => app.apply(crate::events::EventUpdate::Banner {
                    severity: crate::events::BannerSeverity::RecoverableError,
                    message: format!("could not start new conversation: {error}"),
                    url: None,
                }),
            },
            UiAction::LoadModels => {
                app.set_models(runtime.models.clone());
            }
            UiAction::LoadTools => {
                app.open_tool_picker();
            }
            UiAction::LoadSkills => {
                app.open_skill_picker();
            }
            UiAction::LoadUsage => {
                let metrics = match runtime.session.rpc().usage().get_metrics().await {
                    Ok(metrics) => metrics,
                    Err(error) if error.is_transport_failure() => {
                        recover_connection(app, runtime, events).await?;
                        continue;
                    }
                    Err(error) => {
                        app.apply(crate::events::EventUpdate::Banner {
                            severity: crate::events::BannerSeverity::RecoverableError,
                            message: format!("could not load usage metrics: {error}"),
                            url: None,
                        });
                        continue;
                    }
                };
                let context_attribution = match runtime
                    .session
                    .rpc()
                    .metadata()
                    .get_context_attribution()
                    .await
                {
                    Ok(result) => context_attribution_snapshot(&result),
                    Err(error) if error.is_transport_failure() => {
                        recover_connection(app, runtime, events).await?;
                        continue;
                    }
                    Err(_) => None,
                };
                app.set_usage(usage_metrics_snapshot(&metrics), context_attribution);
            }
            UiAction::LoadTodos => {
                load_todos(app, runtime, events).await?;
            }
            UiAction::Resume(session_id) => match runtime.resume(session_id).await {
                Ok(history) => {
                    *events = runtime.session.subscribe();
                    app.replace_history(&history);
                    app.set_toolset(runtime.active_toolset);
                    app.set_skill_selection(runtime.active_skill_selection.clone());
                    app.set_model(runtime.active_model_options.model.clone());
                    if let Some(model) = runtime.active_model_options.model.clone() {
                        app.apply(crate::events::EventUpdate::ModelChanged {
                            model: model.clone(),
                        });
                        app.set_reasoning_effort(displayed_reasoning_effort(
                            &runtime.models,
                            Some(&model),
                            runtime.active_model_options.reasoning_effort.as_deref(),
                        ));
                    }
                    app.add_diagnostic("session resumed");
                }
                Err(error) if error.is_transport_failure() => {
                    recover_connection(app, runtime, events).await?;
                }
                Err(error) => app.apply(crate::events::EventUpdate::Banner {
                    severity: crate::events::BannerSeverity::RecoverableError,
                    message: error.to_string(),
                    url: None,
                }),
            },
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
                if let Err(error) = runtime
                    .switch_model(
                        model.clone(),
                        options,
                        selection.reasoning_effort.clone(),
                        selection.context_tier.clone(),
                    )
                    .await
                {
                    if error.is_transport_failure() {
                        recover_connection(app, runtime, events).await?;
                    } else {
                        app.apply(crate::events::EventUpdate::Banner {
                            severity: crate::events::BannerSeverity::RecoverableError,
                            message: error.to_string(),
                            url: None,
                        });
                    }
                } else {
                    *events = runtime.session.subscribe();
                    let displayed_reasoning = displayed_reasoning_effort(
                        &runtime.models,
                        Some(&model),
                        selection.reasoning_effort.as_deref(),
                    );
                    app.set_toolset(runtime.active_toolset);
                    app.set_reasoning_effort(displayed_reasoning);
                    app.apply(crate::events::EventUpdate::ModelChanged { model });
                }
            }
            UiAction::ApplyToolset(toolset) => {
                if app.toolset_change_is_blocked() {
                    app.apply(crate::events::EventUpdate::Banner {
                        severity: crate::events::BannerSeverity::RecoverableError,
                        message: "tool selection can only change while the session is idle"
                            .to_string(),
                        url: None,
                    });
                    continue;
                }

                app.set_reconnecting(true);
                let result = runtime.set_toolset(toolset).await;
                app.set_reconnecting(false);
                match result {
                    Ok(()) => {
                        *events = runtime.session.subscribe();
                        app.set_toolset(runtime.active_toolset);
                    }
                    Err(error) if error.is_transport_failure() => {
                        recover_connection(app, runtime, events).await?;
                    }
                    Err(error) => app.apply(crate::events::EventUpdate::Banner {
                        severity: crate::events::BannerSeverity::RecoverableError,
                        message: error.to_string(),
                        url: None,
                    }),
                }
            }
            UiAction::ApplySkills(selection) => {
                if app.skill_selection_change_is_blocked() {
                    app.apply(crate::events::EventUpdate::Banner {
                        severity: crate::events::BannerSeverity::RecoverableError,
                        message: "skill selection can only change while the session is idle"
                            .to_string(),
                        url: None,
                    });
                    continue;
                }

                app.set_reconnecting(true);
                let result = runtime.set_skills(selection).await;
                app.set_reconnecting(false);
                match result {
                    Ok(()) => {
                        *events = runtime.session.subscribe();
                        app.set_skill_selection(runtime.active_skill_selection.clone());
                    }
                    Err(error) if error.is_transport_failure() => {
                        recover_connection(app, runtime, events).await?;
                    }
                    Err(error) => app.apply(crate::events::EventUpdate::Banner {
                        severity: crate::events::BannerSeverity::RecoverableError,
                        message: error.to_string(),
                        url: None,
                    }),
                }
            }
            UiAction::Send(prompt) => {
                if let Some(selection) = skill_selection_for_invocation(
                    &runtime.skill_catalog,
                    &runtime.active_skill_selection,
                    &prompt,
                ) {
                    if app.skill_selection_change_is_blocked() {
                        app.apply(crate::events::EventUpdate::Banner {
                            severity: crate::events::BannerSeverity::RecoverableError,
                            message: "the requested skill can only be activated while the session is idle"
                                .to_string(),
                            url: None,
                        });
                        continue;
                    }
                    app.set_reconnecting(true);
                    let result = runtime.set_skills(selection).await;
                    app.set_reconnecting(false);
                    match result {
                        Ok(()) => {
                            *events = runtime.session.subscribe();
                            app.set_skill_selection(runtime.active_skill_selection.clone());
                        }
                        Err(error) if error.is_transport_failure() => {
                            recover_connection(app, runtime, events).await?;
                            continue;
                        }
                        Err(error) => {
                            app.apply(crate::events::EventUpdate::Banner {
                                severity: crate::events::BannerSeverity::RecoverableError,
                                message: error.to_string(),
                                url: None,
                            });
                            continue;
                        }
                    }
                }
                app.add_user_message(prompt.clone());
                runtime.mark_conversation_started();
                match runtime.session.send(prompt).await {
                    Ok(_) => app.set_fleet_active(false),
                    Err(error) if error.is_transport_failure() => {
                        recover_connection(app, runtime, events).await?;
                    }
                    Err(error) => app.apply(crate::events::EventUpdate::Banner {
                        severity: crate::events::BannerSeverity::BlockingError,
                        message: format!("message could not be sent: {error}"),
                        url: None,
                    }),
                }
            }
            UiAction::StartFleet(prompt) => {
                app.add_user_message(format!("/fleet {prompt}"));
                runtime.mark_conversation_started();
                let session = &runtime.session;
                match send_with_fleet_fallback(
                    prompt,
                    |request| async move { session.rpc().fleet().start(request).await },
                    |prompt| async move {
                        session
                            .rpc()
                            .tasks()
                            .start_agent(TasksStartAgentRequest {
                                agent_type: "general-purpose".to_string(),
                                description: Some("Single-agent Fleet fallback".to_string()),
                                model: None,
                                name: "picopilot-task".to_string(),
                                prompt,
                            })
                            .await
                            .map(|_| ())
                    },
                    |error| error.is_transport_failure(),
                )
                .await
                {
                    Ok(SendPath::Fleet) => app.set_fleet_active(true),
                    Ok(SendPath::Single) => app.set_fleet_active(false),
                    Err(error) if error.is_transport_failure() => {
                        recover_connection(app, runtime, events).await?;
                    }
                    Err(error) => app.apply(crate::events::EventUpdate::Banner {
                        severity: crate::events::BannerSeverity::BlockingError,
                        message: format!("message could not be sent: {error}"),
                        url: None,
                    }),
                }
            }
        }
    }
    Ok(())
}

async fn recover_connection(
    app: &mut App,
    runtime: &mut AppRuntime,
    events: &mut EventSubscription,
) -> io::Result<()> {
    let session_id = runtime.session.id().clone();
    app.mark_in_flight_tools_unknown();
    app.reject_pending_approvals();
    app.set_reconnecting(true);

    for attempt in 1..=MAX_RECOVERY_ATTEMPTS {
        app.apply(crate::events::EventUpdate::Banner {
            severity: crate::events::BannerSeverity::Warning,
            message: format!(
                "connection lost; reconnecting session '{session_id}' (attempt {attempt}/{MAX_RECOVERY_ATTEMPTS})"
            ),
            url: None,
        });

        match runtime.recover_transport().await {
            Ok(()) => {
                *events = runtime.session.subscribe();
                app.set_reconnecting(false);
                app.apply(crate::events::EventUpdate::Banner {
                    severity: crate::events::BannerSeverity::Warning,
                    message: "connection restored; in-flight tool outcomes remain unknown"
                        .to_string(),
                    url: None,
                });
                return Ok(());
            }
            Err(error) if is_fatal_recovery_error(&error) || attempt == MAX_RECOVERY_ATTEMPTS => {
                app.set_reconnecting(false);
                app.apply(crate::events::EventUpdate::Banner {
                    severity: crate::events::BannerSeverity::BlockingError,
                    message: format!(
                        "could not reconnect session '{session_id}' after {attempt} attempt(s): {error}"
                    ),
                    url: None,
                });
                app.quit();
                return Ok(());
            }
            Err(_) => tokio::time::sleep(recovery_backoff(attempt)).await,
        }
    }

    Ok(())
}

fn is_fatal_recovery_error(error: &RecoveryError) -> bool {
    matches!(
        error,
        RecoveryError::Resume(
            ResumeError::MissingSession { .. } | ResumeError::IdentityMismatch { .. }
        )
    )
}

async fn load_todos(
    app: &mut App,
    runtime: &mut AppRuntime,
    events: &mut EventSubscription,
) -> io::Result<()> {
    match runtime
        .session
        .rpc()
        .plan()
        .read_sql_todos_with_dependencies()
        .await
    {
        Ok(result) => app.set_todos(todo_snapshot(&result)),
        Err(error) if error.is_transport_failure() => {
            recover_connection(app, runtime, events).await?;
        }
        Err(error) => app.apply(crate::events::EventUpdate::Banner {
            severity: crate::events::BannerSeverity::RecoverableError,
            message: format!("could not load Fleet todos: {error}"),
            url: None,
        }),
    }
    Ok(())
}

async fn refresh_todos_if_requested(
    app: &mut App,
    runtime: &mut AppRuntime,
    events: &mut EventSubscription,
) -> io::Result<()> {
    if !app.take_todo_refresh_request() || !app.fleet_active || !app.todo_modal_is_open() {
        return Ok(());
    }
    load_todos(app, runtime, events).await
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let raw_mode_result = disable_raw_mode();
    let terminal_result = restore_main_screen(terminal.backend_mut());
    let cursor_result = terminal.show_cursor();
    raw_mode_result?;
    terminal_result?;
    cursor_result
}

fn status_bar(app: &App) -> Paragraph<'static> {
    let status = app.status();
    let project = app
        .project_name
        .as_deref()
        .map(|project| format!("{project}  ·  "))
        .unwrap_or_default();
    let model = status.model.as_deref().unwrap_or("auto");
    let reasoning = status.reasoning_effort.as_deref().unwrap_or("default");
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
        " {project}{model}  ·  {reasoning} reasoning  ·  autopilot {mode}  ·  tools {}/{}  ·  skills {}/{}  ·  {context} tokens  ·  {cost} ",
        app.toolset.len(),
        TOOL_COUNT,
        app.skill_selection.len(),
        app.skill_catalog.skills().len(),
    );

    Paragraph::new(label).style(Style::default().fg(Color::DarkGray))
}

fn working_directory_name(working_directory: &Path) -> String {
    working_directory
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "project".to_string())
}

fn displayed_reasoning_effort(
    models: &[Model],
    model_id: Option<&str>,
    configured_effort: Option<&str>,
) -> Option<String> {
    configured_effort.map(str::to_owned).or_else(|| {
        let model_id = model_id?;
        models
            .iter()
            .find(|model| model.id == model_id)
            .and_then(|model| model.default_reasoning_effort.clone())
    })
}

fn shortcut_bar() -> Paragraph<'static> {
    let shortcut = |key| {
        Span::styled(
            key,
            Style::default()
                .fg(Color::Rgb(240, 177, 94))
                .add_modifier(Modifier::BOLD),
        )
    };

    Paragraph::new(Line::from(vec![
        Span::raw(" "),
        shortcut("^N"),
        Span::raw(" new "),
        shortcut("^O"),
        Span::raw(" sessions "),
        shortcut("^P"),
        Span::raw(" models "),
        shortcut("^U"),
        Span::raw(" usage "),
        shortcut("^K"),
        Span::raw(" tools "),
        shortcut("^S"),
        Span::raw(" skills "),
        shortcut("^T"),
        Span::raw(" todo "),
        shortcut("^I"),
        Span::raw(" internals "),
        shortcut("^X"),
        Span::raw(" exit"),
    ]))
    .style(Style::default().fg(Color::DarkGray))
}

const INPUT_PROMPT: &str = "  ❯ ";
const INPUT_CONTINUATION: &str = "    ";
const MAX_INPUT_CONTENT_LINES: usize = 8;

struct WrappedInput {
    lines: Vec<Line<'static>>,
    cursor_row: usize,
    cursor_column: usize,
}

struct WrapState {
    lines: Vec<Line<'static>>,
    cursor_position: Option<(usize, usize)>,
    first_visual_line: bool,
}

fn input_height(app: &App, area: Rect) -> u16 {
    if app.blocked || app.reconnecting || app.pending_approval().is_some() {
        return 3.min(area.height.saturating_sub(3).max(1));
    }

    let wrapped = wrap_input(
        app.input(),
        app.input_cursor_byte_offset(),
        area.width as usize,
    );
    let desired = wrapped.lines.len().clamp(1, MAX_INPUT_CONTENT_LINES) as u16 + 2;
    desired.min(area.height.saturating_sub(3).max(1))
}

fn wrap_input(text: &str, cursor: usize, width: usize) -> WrappedInput {
    let width = width.max(1);
    let cursor = cursor.min(text.len());
    let mut state = WrapState {
        lines: Vec::new(),
        cursor_position: None,
        first_visual_line: true,
    };
    let mut line_start = 0;

    for (line_end, character) in text.char_indices() {
        if character == '\n' {
            wrap_input_line(text, line_start, line_end, cursor, width, &mut state);
            line_start = line_end + character.len_utf8();
        }
    }
    wrap_input_line(text, line_start, text.len(), cursor, width, &mut state);

    let (cursor_row, cursor_column) = state.cursor_position.unwrap_or_else(|| {
        let row = state.lines.len().saturating_sub(1);
        (row, display_width(INPUT_CONTINUATION))
    });
    WrappedInput {
        lines: state.lines,
        cursor_row,
        cursor_column,
    }
}

fn wrap_input_line(
    text: &str,
    line_start: usize,
    line_end: usize,
    cursor: usize,
    width: usize,
    state: &mut WrapState,
) {
    let mut segment_start = line_start;
    loop {
        let is_first_line = state.first_visual_line;
        let prefix = if is_first_line {
            INPUT_PROMPT
        } else {
            INPUT_CONTINUATION
        };
        let prefix_width = display_width(prefix);
        let content_width = width.saturating_sub(prefix_width).max(1);
        let segment_end =
            segment_start + wrapped_segment_end(&text[segment_start..line_end], content_width);
        let segment = &text[segment_start..segment_end];
        let row = state.lines.len();

        if cursor >= segment_start
            && (cursor < segment_end || (cursor == segment_end && segment_end == line_end))
        {
            state.cursor_position = Some((
                row,
                prefix_width + display_width(&text[segment_start..cursor]),
            ));
        }

        let prefix_span = if is_first_line {
            Span::styled(prefix, Style::default().fg(Color::Rgb(240, 177, 94)))
        } else {
            Span::raw(prefix)
        };
        state.lines.push(Line::from(vec![
            prefix_span,
            Span::raw(segment.to_string()),
        ]));
        state.first_visual_line = false;

        if segment_end == line_end {
            break;
        }
        segment_start = segment_end;
    }
}

fn wrapped_segment_end(text: &str, width: usize) -> usize {
    let mut display_width: usize = 0;
    let mut end = 0;
    for (index, character) in text.char_indices() {
        let character_width = input_character_width(character);
        if end != 0 && display_width.saturating_add(character_width) > width {
            break;
        }
        end = index + character.len_utf8();
        display_width = display_width.saturating_add(character_width);
        if display_width >= width {
            break;
        }
    }
    end
}

fn display_width(text: &str) -> usize {
    text.chars().map(input_character_width).sum()
}

fn input_character_width(character: char) -> usize {
    if character == '\t' {
        4
    } else {
        UnicodeWidthChar::width(character).unwrap_or(1)
    }
}

fn input_box(app: &App, area: Rect) -> Paragraph<'static> {
    if app.blocked {
        return Paragraph::new("Session ended. Press Ctrl+X to close.")
            .style(Style::default().fg(Color::Rgb(255, 117, 117)))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(255, 117, 117)))
                    .title("session ended"),
            );
    }

    if app.reconnecting {
        return Paragraph::new("Connection lost; reconnecting. Input is paused.")
            .style(Style::default().fg(Color::Rgb(242, 204, 96)))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(242, 177, 94)))
                    .title("reconnecting"),
            )
            .wrap(Wrap { trim: false });
    }

    if let Some(request) = app.pending_approval() {
        let choices = if request.category.supports_trust() {
            "y allow once, n deny, a trust for session, v details"
        } else {
            "y allow once, n deny, v details"
        };
        let prompt = format!(
            "{} ({}): {} | {choices}",
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

    let wrapped = wrap_input(
        app.input(),
        app.input_cursor_byte_offset(),
        area.width as usize,
    );
    let visible_lines = area.height.saturating_sub(2).max(1) as usize;
    let scroll = wrapped
        .cursor_row
        .saturating_sub(visible_lines.saturating_sub(1));

    Paragraph::new(wrapped.lines)
        .block(
            Block::default()
                .borders(Borders::TOP | Borders::BOTTOM)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .scroll((scroll.min(u16::MAX as usize) as u16, 0))
}

fn draw_completion(frame: &mut Frame, app: &App, input_area: Rect) {
    let Some(completion) = app.completion.as_ref() else {
        return;
    };
    if completion.candidates.is_empty() || input_area.y == 0 {
        return;
    }

    let visible_count = completion.candidates.len().min(7);
    let height = visible_count as u16 + 2;
    let first_visible = completion
        .selected_item
        .saturating_sub(visible_count.saturating_sub(1))
        .min(completion.candidates.len().saturating_sub(visible_count));
    let desired_width = completion
        .candidates
        .iter()
        .map(|candidate| candidate.command.len() + candidate.description.len() + 5)
        .max()
        .unwrap_or(20)
        .min(100) as u16;
    let x = input_area.x.saturating_add(1);
    let width = desired_width
        .min(frame.area().right().saturating_sub(x))
        .max(1);
    let y = input_area.y.saturating_sub(height);
    let area = Rect::new(x, y, width, height);
    frame.render_widget(ratatui::widgets::Clear, area);

    let items = completion
        .candidates
        .iter()
        .skip(first_visible)
        .take(visible_count)
        .map(|candidate| {
            ListItem::new(format!(
                " {:<width$} {}",
                candidate.command,
                candidate.description,
                width = completion
                    .candidates
                    .iter()
                    .map(|item| item.command.len())
                    .max()
                    .unwrap_or(1)
            ))
        })
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(240, 177, 94)))
                .title("commands"),
        )
        .highlight_symbol("› ")
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(240, 177, 94)),
        );
    let mut state = ListState::default()
        .with_selected(Some(completion.selected_item.saturating_sub(first_visible)));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_modal(frame: &mut Frame, app: &App) {
    if app.show_approval_details {
        let area = centered_rect(80, 80, frame.area());
        frame.render_widget(ratatui::widgets::Clear, area);
        let details = app
            .pending_approval()
            .map(|request| request.details.as_str())
            .unwrap_or("No approval details available.");
        frame.render_widget(
            Paragraph::new(details.to_string())
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Rgb(240, 177, 94)))
                        .title("approval details | v to close"),
                )
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }

    let Some(modal) = app.modal else {
        return;
    };
    let area = modal_area(modal, frame.area());
    frame.render_widget(ratatui::widgets::Clear, area);

    if matches!(modal, ModalKind::Tools) {
        draw_tool_picker(frame, app, area);
        return;
    }

    if matches!(modal, ModalKind::Skills) {
        draw_skill_picker(frame, app, area);
        return;
    }

    if matches!(modal, ModalKind::Usage) {
        frame.render_widget(
            Paragraph::new(usage_detail_lines(app))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Rgb(240, 177, 94)))
                        .title("usage and context | ^U or Esc to close"),
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
                        .title("fleet todos | ^T or Esc to close"),
                )
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }

    if matches!(modal, ModalKind::Models) {
        draw_model_picker(frame, app, area);
        return;
    }

    let title = match modal {
        ModalKind::Sessions => "resume session",
        ModalKind::Models => "choose model",
        ModalKind::Usage => "usage and context | ^U or Esc to close",
        ModalKind::Todos => "fleet todos | ^T or Esc to close",
        ModalKind::Tools => "tools",
        ModalKind::Skills => "skills",
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
        ModalKind::Models => Vec::new(),
        ModalKind::Usage | ModalKind::Todos => Vec::new(),
        ModalKind::Tools => Vec::new(),
        ModalKind::Skills => Vec::new(),
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

fn draw_model_picker(frame: &mut Frame, app: &App, area: Rect) {
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(240, 177, 94)))
        .title("choose model");
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .split(inner);
    let items: Vec<ListItem<'static>> = app
        .models
        .iter()
        .map(|model| ListItem::new(model_picker_row_for(model, app.is_local_model(&model.id))))
        .collect();
    let list = List::new(items).highlight_symbol("› ").highlight_style(
        Style::default()
            .fg(Color::Black)
            .bg(Color::Rgb(240, 177, 94)),
    );
    let mut state =
        ListState::default().with_selected((!app.models.is_empty()).then_some(app.selected_item));
    frame.render_stateful_widget(list, layout[0], &mut state);

    frame.render_widget(
        Paragraph::new(model_picker_detail_lines(app))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(Color::Rgb(70, 88, 104)))
                    .title("selected model"),
            )
            .wrap(Wrap { trim: false }),
        layout[1],
    );
    frame.render_widget(
        Paragraph::new("↑/↓ choose   r reasoning   c context   Enter apply   Esc cancel")
            .style(Style::default().fg(Color::Rgb(165, 174, 187))),
        layout[2],
    );
}

fn draw_tool_picker(frame: &mut Frame, app: &App, area: Rect) {
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(240, 177, 94)))
        .title("choose tools | Space toggle, s shell only, a all, Enter apply, Esc cancel");
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let items: Vec<ListItem<'static>> = CANONICAL_TOOLS
        .iter()
        .enumerate()
        .map(|(index, tool)| {
            let checkbox = if app.picker_toolset.contains_at(index) {
                "[x]"
            } else {
                "[ ]"
            };
            ListItem::new(format!(" {checkbox} {tool}"))
        })
        .collect();
    let list = List::new(items).highlight_symbol("› ").highlight_style(
        Style::default()
            .fg(Color::Black)
            .bg(Color::Rgb(240, 177, 94)),
    );
    let mut state = ListState::default().with_selected(Some(app.selected_item));
    frame.render_stateful_widget(list, inner, &mut state);
}

fn draw_skill_picker(frame: &mut Frame, app: &App, area: Rect) {
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(240, 177, 94)))
        .title("choose skills | Space toggle, a all, n none, Enter apply, Esc cancel");
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(4),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .split(inner);
    let items = app
        .skill_catalog
        .skills()
        .iter()
        .map(|skill| {
            let checkbox = if app.picker_skill_selection.contains(&skill.name) {
                "[x]"
            } else {
                "[ ]"
            };
            ListItem::new(format!(" {checkbox} {}", skill.name))
        })
        .collect::<Vec<_>>();
    let list = List::new(items).highlight_symbol("› ").highlight_style(
        Style::default()
            .fg(Color::Black)
            .bg(Color::Rgb(240, 177, 94)),
    );
    let selected = (!app.skill_catalog.skills().is_empty()).then_some(app.selected_item);
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(list, layout[0], &mut state);

    frame.render_widget(
        Paragraph::new(skill_picker_detail_lines(app))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(Color::Rgb(70, 88, 104)))
                    .title("selected skill"),
            )
            .wrap(Wrap { trim: false }),
        layout[1],
    );
    frame.render_widget(
        Paragraph::new("↑/↓ choose   Space toggle   a all   n none   Enter apply   Esc cancel")
            .style(Style::default().fg(Color::Rgb(165, 174, 187))),
        layout[2],
    );
}

fn skill_picker_detail_lines(app: &App) -> Vec<Line<'static>> {
    let Some(skill) = app.skill_catalog.skills().get(app.selected_item) else {
        return vec![Line::from("No skills discovered.")];
    };
    vec![
        Line::from(Span::styled(
            skill.name.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("Description: {}", skill.description)),
        Line::from(format!(
            "Source: {} | {}",
            skill.root.source,
            skill.root.path.display()
        )),
        Line::from(format!("Directory: {}", skill.directory.display())),
    ]
}

fn model_picker_row_for(model: &Model, is_local: bool) -> String {
    format!(
        "{:<28}  {:<9}  {} tokens",
        model.name,
        model_cost_label_for(model, is_local),
        model_context_label(model)
    )
}

fn model_picker_detail_lines(app: &App) -> Vec<Line<'static>> {
    let Some(model) = app.models.get(app.selected_item) else {
        return vec![Line::from("No models available.")];
    };
    let is_local = app.is_local_model(&model.id);
    let reasoning = app
        .picker_reasoning_effort
        .as_deref()
        .unwrap_or("model default");
    let reasoning_values = model
        .supported_reasoning_efforts
        .as_ref()
        .filter(|values| !values.is_empty())
        .map(|values| values.join(" · "))
        .unwrap_or_else(|| "unavailable".to_string());
    let context = app
        .picker_context_tier
        .as_deref()
        .unwrap_or("model default");
    let context_values = crate::config::supported_context_tiers(model);
    let context_values = if context_values.is_empty() {
        "unavailable".to_string()
    } else {
        context_values.join(" · ")
    };

    let mut lines = vec![
        Line::from(Span::styled(
            format!("{}  ({})", model.name, model.id),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            "Reasoning: {reasoning}   Available: {reasoning_values}"
        )),
        Line::from(format!(
            "Context:   {context}   Available: {context_values}"
        )),
    ];
    if is_local {
        lines.push(Line::from("Billing:   local inference (provider-tracked)"));
    }
    lines
}

fn modal_area(modal: ModalKind, terminal_area: Rect) -> Rect {
    match modal {
        ModalKind::Sessions | ModalKind::Models | ModalKind::Tools | ModalKind::Skills => {
            terminal_area
        }
        ModalKind::Usage | ModalKind::Todos => centered_rect(70, 70, terminal_area),
    }
}

fn model_cost_label_for(model: &Model, is_local: bool) -> String {
    if is_local {
        return "local".to_string();
    }

    let category = serde_json::to_value(model).ok().and_then(|value| {
        value
            .get("modelPickerPriceCategory")
            .and_then(|category| category.as_str())
            .map(str::to_owned)
    });
    match category.as_deref() {
        Some("low" | "medium" | "high") => category.expect("matched category is present"),
        Some("very_high") => "very high".to_string(),
        _ => model
            .billing
            .as_ref()
            .and_then(|billing| billing.multiplier)
            .map(|multiplier| format!("{multiplier}x"))
            .unwrap_or_else(|| "unknown".to_string()),
    }
}

fn model_context_label(model: &Model) -> String {
    model
        .capabilities
        .limits
        .as_ref()
        .and_then(|limits| limits.max_context_window_tokens)
        .map(format_count)
        .unwrap_or_else(|| "unknown".to_string())
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
        Some(cost) => format!("{:.3} AIU", cost / 1_000_000_000.0),
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

fn draw_live_chat(
    frame: &mut Frame,
    app: &App,
    screen: &mut ScreenModel,
    area: Rect,
    animation_elapsed_ms: u64,
) {
    let mut lines = screen.visible_live_lines_at_width_with_clock(
        Platform::current(),
        area.width as usize,
        area.height as usize,
        animation_elapsed_ms,
    );
    if app.status.busy {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.push(Line::from(vec![
            Span::styled(
                "✻ ",
                Style::default()
                    .fg(Color::Rgb(240, 177, 94))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Copilot is responding…",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_chat(frame: &mut Frame, app: &App, area: Rect) {
    let lines = chat_lines_at_width(app, area.width as usize);
    let scroll = lines
        .len()
        .saturating_sub(area.height as usize)
        .min(u16::MAX as usize) as u16;
    let paragraph = Paragraph::new(lines).scroll((scroll, 0));
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
fn chat_lines(app: &App) -> Vec<Line<'static>> {
    chat_lines_at_width(app, 80)
}

fn chat_lines_at_width(app: &App, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for entry in app.entries() {
        let kind = match entry {
            ChatEntry::User(_) => LiveEntryKind::User,
            ChatEntry::Assistant { agent_id, .. } => {
                if agent_id.is_some() {
                    LiveEntryKind::AssistantNested
                } else {
                    LiveEntryKind::Assistant
                }
            }
            _ => LiveEntryKind::Other,
        };
        let Some(payload) = entry_payload(entry, app.show_internals) else {
            continue;
        };
        lines.extend(render_transcript_payload(kind, &payload, width));
    }
    if app.status.busy {
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::styled(
                "✻ ",
                Style::default()
                    .fg(Color::Rgb(240, 177, 94))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Copilot is responding…",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ),
        ]));
    }
    lines
}

fn entry_payload(entry: &ChatEntry, show_internals: bool) -> Option<TranscriptPayload> {
    match entry {
        ChatEntry::Assistant { content, .. } => {
            Some(TranscriptPayload::AssistantMarkdown(content.clone()))
        }
        ChatEntry::Tool {
            tool_call_id,
            tool_name,
            arguments,
            success,
            state,
            agent_id,
            started_at,
            cwd,
            ..
        } => Some(TranscriptPayload::ToolHeader(ToolHeaderPayload {
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            arguments: arguments.clone(),
            agent_id: agent_id.clone(),
            started_at: *started_at,
            state: if *success == Some(true) {
                ToolCallState::Success
            } else if *success == Some(false) {
                if matches!(state, ToolCallState::Cancelled) {
                    ToolCallState::Cancelled
                } else {
                    ToolCallState::Error
                }
            } else {
                *state
            },
            cwd: cwd.clone(),
        })),
        ChatEntry::ToolProgress {
            tool_call_id,
            tool_name,
            content,
            kind,
            agent_id,
        } => Some(TranscriptPayload::ToolProgress(ToolProgressPayload {
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            content: content.clone(),
            kind: *kind,
            agent_id: agent_id.clone(),
        })),
        ChatEntry::ToolResult {
            tool_call_id,
            tool_name,
            arguments,
            content,
            state,
            agent_id,
            cwd,
        } => Some(TranscriptPayload::ToolResult(ToolResultPayload {
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            arguments: arguments.clone(),
            content: content.clone(),
            state: *state,
            agent_id: agent_id.clone(),
            cwd: cwd.clone(),
        })),
        _ => {
            let lines = entry_lines(entry, show_internals);
            (!lines.is_empty()).then_some(TranscriptPayload::PreRendered(lines))
        }
    }
}

fn entry_lines(entry: &ChatEntry, show_internals: bool) -> Vec<Line<'static>> {
    match entry {
        ChatEntry::User(content) => {
            let content = truncate_user_content(content);
            markdown_lines(&content, Style::default().fg(palette::TEXT))
        }
        ChatEntry::Diagnostic(message) if show_internals => labeled_lines(
            "debug",
            message,
            Style::default().fg(Color::Rgb(165, 174, 187)),
        ),
        ChatEntry::Diagnostic(_) => Vec::new(),
        ChatEntry::Assistant { content, .. } => {
            assistant_markdown_lines(content, Style::default().fg(palette::TEXT))
        }
        ChatEntry::Reasoning {
            content, agent_id, ..
        } => markdown_prefixed_lines(
            &speaker_prefix("  ", agent_id.as_deref()),
            content,
            Style::default().fg(Color::DarkGray),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        ),
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
        ChatEntry::Approval {
            category,
            tool_name,
            details,
            status,
        } => {
            let state = match status {
                ApprovalStatus::Pending => "pending",
                ApprovalStatus::ApprovedOnce => "approved once",
                ApprovalStatus::Denied => "denied",
                ApprovalStatus::Trusted => "trusted",
            };
            labeled_lines(
                &format!("approve [{state}]"),
                &format!("{category} ({tool_name}): {details}"),
                Style::default()
                    .fg(Color::Rgb(255, 219, 129))
                    .add_modifier(Modifier::BOLD),
            )
        }
        ChatEntry::Tool { .. } | ChatEntry::ToolProgress { .. } | ChatEntry::ToolResult { .. } => {
            Vec::new()
        }
        ChatEntry::Completed => Vec::new(),
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

fn markdown_prefixed_lines(
    prefix: &str,
    content: &str,
    prefix_style: Style,
    body_style: Style,
) -> Vec<Line<'static>> {
    let mut body_lines = markdown_lines(content, body_style);
    if body_lines.is_empty() {
        body_lines.push(Line::default());
    }

    body_lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let prefix = if index == 0 {
                prefix.to_string()
            } else {
                "  ".to_string()
            };
            let mut spans = vec![Span::styled(prefix, prefix_style)];
            spans.extend(line.spans);
            Line::from(spans)
        })
        .collect()
}

fn markdown_lines(content: &str, base_style: Style) -> Vec<Line<'static>> {
    let mut renderer = MarkdownRenderer::new(base_style);
    let parser = Parser::new_ext(
        content,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES,
    );
    for event in parser {
        renderer.push(event);
    }
    renderer.finish()
}

fn truncate_user_content(content: &str) -> String {
    const MAX_GRAPHEMES: usize = 10_000;
    const EDGE_GRAPHEMES: usize = 2_500;

    let graphemes = content.graphemes(true).collect::<Vec<_>>();
    if graphemes.len() <= MAX_GRAPHEMES {
        return content.to_string();
    }

    let first_end = graphemes
        .iter()
        .take(EDGE_GRAPHEMES)
        .map(|grapheme| grapheme.len())
        .sum::<usize>();
    let last_start = content.len()
        - graphemes
            .iter()
            .rev()
            .take(EDGE_GRAPHEMES)
            .map(|grapheme| grapheme.len())
            .sum::<usize>();
    let omitted_newlines = content[first_end..last_start]
        .chars()
        .filter(|character| *character == '\n')
        .count();

    format!(
        "{}\n… +{omitted_newlines} lines …\n{}",
        &content[..first_end],
        &content[last_start..]
    )
}

struct MarkdownRenderer {
    lines: Vec<Line<'static>>,
    spans: Vec<Span<'static>>,
    styles: Vec<Style>,
    muted: bool,
    list_depth: usize,
    code_block: bool,
    table: Option<MarkdownTable>,
}

struct MarkdownTable {
    alignments: Vec<Alignment>,
    rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    current_cell: String,
    header_rows: usize,
}

impl MarkdownRenderer {
    fn new(base_style: Style) -> Self {
        Self {
            lines: Vec::new(),
            spans: Vec::new(),
            muted: base_style.fg == Some(Color::DarkGray),
            styles: vec![base_style],
            list_depth: 0,
            code_block: false,
            table: None,
        }
    }

    fn push(&mut self, event: MarkdownEvent<'_>) {
        match event {
            MarkdownEvent::Start(tag) => self.start(tag),
            MarkdownEvent::End(tag) => self.end(tag),
            MarkdownEvent::Text(text) => self.push_text(&text),
            MarkdownEvent::Code(code) if self.table.is_some() => {
                self.push_table_text(&code);
            }
            MarkdownEvent::Code(code) => self.spans.push(Span::styled(
                code.into_string(),
                self.accent(Color::Rgb(242, 204, 96)),
            )),
            MarkdownEvent::SoftBreak if self.table.is_some() => self.push_table_text(" "),
            MarkdownEvent::SoftBreak => self.spans.push(Span::raw(" ")),
            MarkdownEvent::HardBreak => self.flush_line(),
            MarkdownEvent::Rule => {
                self.flush_line();
                self.spans.push(Span::styled(
                    "----------------------------------------",
                    self.accent(Color::DarkGray),
                ));
                self.flush_line();
            }
            MarkdownEvent::TaskListMarker(checked) => self.spans.push(Span::styled(
                if checked { "[x] " } else { "[ ] " },
                self.accent(Color::Rgb(240, 177, 94)),
            )),
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Table(alignments) => {
                self.flush_line();
                self.table = Some(MarkdownTable {
                    alignments,
                    rows: Vec::new(),
                    current_row: Vec::new(),
                    current_cell: String::new(),
                    header_rows: 0,
                });
            }
            Tag::TableHead | Tag::TableRow | Tag::TableCell => {}
            Tag::Paragraph => {}
            Tag::Heading { .. } => self.push_style(
                self.accent(Color::Rgb(139, 181, 255))
                    .add_modifier(Modifier::BOLD),
            ),
            Tag::Strong => self.push_style(self.style().add_modifier(Modifier::BOLD)),
            Tag::Emphasis => self.push_style(self.style().add_modifier(Modifier::ITALIC)),
            Tag::Strikethrough => self.push_style(self.style().add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { .. } => self.push_style(
                self.accent(Color::Rgb(139, 181, 255))
                    .add_modifier(Modifier::UNDERLINED),
            ),
            Tag::BlockQuote(_) => self
                .spans
                .push(Span::styled("> ", self.accent(Color::Rgb(132, 147, 160)))),
            Tag::List(_) => self.list_depth += 1,
            Tag::Item => self.spans.push(Span::styled(
                format!("{}* ", "  ".repeat(self.list_depth.saturating_sub(1))),
                self.accent(Color::Rgb(240, 177, 94)),
            )),
            Tag::CodeBlock(_) => {
                self.flush_line();
                self.code_block = true;
                self.push_style(self.accent(Color::Rgb(180, 190, 200)));
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::TableCell => {
                if let Some(table) = &mut self.table {
                    table
                        .current_row
                        .push(std::mem::take(&mut table.current_cell));
                }
            }
            TagEnd::TableRow => {
                if let Some(table) = &mut self.table {
                    table.rows.push(std::mem::take(&mut table.current_row));
                }
            }
            TagEnd::TableHead => {
                if let Some(table) = &mut self.table {
                    if !table.current_row.is_empty() {
                        table.rows.push(std::mem::take(&mut table.current_row));
                    }
                    table.header_rows = table.rows.len();
                }
            }
            TagEnd::Table => self.render_table(),
            TagEnd::Paragraph | TagEnd::Item | TagEnd::BlockQuote(_) => self.flush_line(),
            TagEnd::Heading(_) => {
                self.flush_line();
                self.pop_style();
            }
            TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough | TagEnd::Link => {
                self.pop_style()
            }
            TagEnd::List(_) => self.list_depth = self.list_depth.saturating_sub(1),
            TagEnd::CodeBlock => {
                self.flush_line();
                self.code_block = false;
                self.pop_style();
            }
            _ => {}
        }
    }

    fn push_text(&mut self, text: &str) {
        if self.table.is_some() {
            self.push_table_text(text);
        } else if self.code_block {
            for (index, line) in text.split('\n').enumerate() {
                if index > 0 {
                    self.flush_line();
                }
                if !line.is_empty() {
                    self.spans
                        .push(Span::styled(format!("  {line}"), self.style()));
                }
            }
        } else {
            self.spans
                .push(Span::styled(text.to_string(), self.style()));
        }
    }

    fn push_table_text(&mut self, text: &str) {
        if let Some(table) = &mut self.table {
            table.current_cell.push_str(text);
        }
    }

    fn render_table(&mut self) {
        let Some(table) = self.table.take() else {
            return;
        };
        let column_count = table.rows.iter().map(Vec::len).max().unwrap_or(0);
        let widths: Vec<usize> = (0..column_count)
            .map(|column| {
                table
                    .rows
                    .iter()
                    .filter_map(|row| row.get(column))
                    .map(|cell| cell.chars().count())
                    .max()
                    .unwrap_or(0)
            })
            .collect();

        for (row_index, row) in table.rows.iter().enumerate() {
            if row_index == table.header_rows {
                let divider = widths
                    .iter()
                    .map(|width| "-".repeat(width + 1))
                    .collect::<Vec<_>>()
                    .join("+");
                self.lines.push(Line::from(Span::styled(
                    divider,
                    self.accent(Color::DarkGray),
                )));
            }
            let cells = (0..column_count)
                .map(|column| {
                    let cell = row.get(column).map(String::as_str).unwrap_or("");
                    match table
                        .alignments
                        .get(column)
                        .copied()
                        .unwrap_or(Alignment::None)
                    {
                        Alignment::Right => format!("{cell:>width$}", width = widths[column]),
                        Alignment::Center => {
                            let padding = widths[column].saturating_sub(cell.chars().count());
                            let left = padding / 2;
                            format!("{}{cell}{}", " ".repeat(left), " ".repeat(padding - left))
                        }
                        Alignment::None | Alignment::Left => {
                            format!("{cell:<width$}", width = widths[column])
                        }
                    }
                })
                .collect::<Vec<_>>()
                .join(" | ");
            let style = if row_index < table.header_rows {
                self.style().add_modifier(Modifier::BOLD)
            } else {
                self.style()
            };
            self.lines.push(Line::from(Span::styled(cells, style)));
        }
    }

    fn style(&self) -> Style {
        *self.styles.last().expect("base Markdown style is present")
    }

    fn accent(&self, color: Color) -> Style {
        if self.muted {
            self.style()
        } else {
            self.style().fg(color)
        }
    }

    fn push_style(&mut self, style: Style) {
        self.styles.push(style);
    }

    fn pop_style(&mut self) {
        if self.styles.len() > 1 {
            self.styles.pop();
        }
    }

    fn flush_line(&mut self) {
        if !self.spans.is_empty() {
            self.lines.push(Line::from(std::mem::take(&mut self.spans)));
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_line();
        self.lines
    }
}

fn speaker_prefix(symbol: &str, agent_id: Option<&str>) -> String {
    match agent_id {
        Some(agent_id) => format!("{symbol} {agent_id} "),
        None => format!("{symbol} "),
    }
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
    use std::path::{Path, PathBuf};

    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use github_copilot_sdk::rpc::FleetStartResult;
    use github_copilot_sdk::types::{ContextTier, Model, SessionId, SessionMetadata};
    use ratatui::backend::TestBackend;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::Terminal;
    use serde_json::json;

    use super::{
        displayed_reasoning_effort, draw, draw_model_picker, draw_skill_picker, draw_tool_picker,
        handle_key, modal_area, model_context_label, model_cost_label_for,
        model_picker_detail_lines, model_picker_row_for, send_with_fleet_fallback,
        skill_selection_for_invocation, status_bar, todo_detail_lines, App, ChatEntry, ModalKind,
        ModelSelection, SendPath, UiAction,
    };
    use crate::events::{EventUpdate, TodoDependencySnapshot, TodoRowSnapshot, TodoSnapshot};
    use crate::screen_model::{render_entry_lines, render_transcript_payload, ScreenChange};
    use crate::skills::{Skill, SkillCatalog, SkillRoot, SkillRootSource, SkillSelection};
    use crate::toolset::{Toolset, CANONICAL_TOOLS, TOOL_COUNT};

    fn test_skill_catalog() -> SkillCatalog {
        let root = SkillRoot {
            path: PathBuf::from("C:\\project\\.agents\\skills"),
            source: SkillRootSource::Project,
        };
        SkillCatalog::from_parts(
            vec![root.clone()],
            vec![
                Skill {
                    name: "rust-review".to_string(),
                    description: "Review Rust code".to_string(),
                    user_invocable: true,
                    directory: root.path.join("rust-review"),
                    root: root.clone(),
                },
                Skill {
                    name: "runbook-extended".to_string(),
                    description: "Follow an extended incident runbook".to_string(),
                    user_invocable: true,
                    directory: root.path.join("runbook-extended"),
                    root: root.clone(),
                },
                Skill {
                    name: "runbook".to_string(),
                    description: "Follow an incident runbook".to_string(),
                    user_invocable: true,
                    directory: root.path.join("runbook"),
                    root: root.clone(),
                },
                Skill {
                    name: "internal-helper".to_string(),
                    description: "Internal-only helper".to_string(),
                    user_invocable: false,
                    directory: root.path.join("internal-helper"),
                    root,
                },
            ],
            Vec::new(),
        )
    }

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
    fn arrows_edit_the_prompt_and_shift_enter_inserts_a_newline() {
        let mut app = App::new(None);
        for character in "ab\\cd".chars() {
            app.push_input(character);
        }

        handle_key(&mut app, key(KeyCode::Left, KeyEventKind::Press));
        handle_key(&mut app, key(KeyCode::Left, KeyEventKind::Press));
        handle_key(
            &mut app,
            key_with_modifiers(KeyCode::Char('X'), KeyModifiers::NONE),
        );
        assert_eq!(app.input(), "ab\\Xcd");

        handle_key(&mut app, key(KeyCode::Backspace, KeyEventKind::Press));
        handle_key(
            &mut app,
            key_with_modifiers(KeyCode::Enter, KeyModifiers::SHIFT),
        );
        app.push_input('n');
        assert_eq!(app.input(), "ab\\\nncd");
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::Send("ab\\\nncd".to_string())
        );
    }

    #[test]
    fn altgr_style_control_alt_characters_are_kept_as_input() {
        let mut app = App::new(None);

        handle_key(
            &mut app,
            key_with_modifiers(
                KeyCode::Char('\\'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
        );

        assert_eq!(app.input(), "\\");
    }

    #[test]
    fn an_unmodified_enter_is_multiline_when_physical_shift_is_held() {
        assert!(super::is_multiline_enter(
            key(KeyCode::Enter, KeyEventKind::Press),
            true,
        ));
    }

    #[test]
    fn pasted_multiline_text_is_one_normalized_prompt() {
        let mut app = App::new(None);

        app.insert_paste("first\r\nsecond\rthird");

        assert_eq!(
            handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::Send("first\nsecond\nthird".to_string())
        );
    }

    #[test]
    fn input_wraps_by_terminal_cells_and_tracks_wide_cursor_position() {
        let wrapped = super::wrap_input("123456789", 9, 10);

        assert_eq!(wrapped.lines.len(), 2);
        assert_eq!(wrapped.lines[0].to_string(), "  ❯ 123456");
        assert_eq!(wrapped.lines[1].to_string(), "    789");
        assert_eq!(wrapped.cursor_row, 1);
        assert_eq!(wrapped.cursor_column, 7);

        let wide = super::wrap_input("ab🙂", "ab🙂".len(), 10);
        assert_eq!(wide.cursor_column, 8);
    }

    #[test]
    fn reasoning_is_visible_while_tool_telemetry_requires_internals() {
        let mut app = App::new(None);
        app.apply(EventUpdate::Reasoning {
            reasoning_id: "reasoning-1".to_string(),
            content: "internal chain".to_string(),
            agent_id: None,
        });
        app.apply(EventUpdate::ToolStarted {
            tool_call_id: "tool-1".to_string(),
            tool_name: "grep".to_string(),
            arguments: None,
            agent_id: None,
        });
        app.add_diagnostic("session resumed");

        let rendered: Vec<String> = super::chat_lines(&app)
            .iter()
            .map(ToString::to_string)
            .filter(|line| !line.is_empty())
            .collect();
        assert!(rendered.iter().any(|line| line.contains("internal chain")));
        assert!(rendered
            .iter()
            .any(|line| line.to_ascii_lowercase().contains("grep")));
        assert!(!rendered.iter().any(|line| line.contains("session resumed")));
        assert_eq!(
            handle_key(
                &mut app,
                KeyEvent {
                    code: KeyCode::Char('i'),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                    state: crossterm::event::KeyEventState::NONE,
                }
            ),
            UiAction::None
        );
        let rendered = super::chat_lines(&app);
        let rendered: Vec<String> = rendered
            .iter()
            .map(ToString::to_string)
            .filter(|line| !line.is_empty())
            .collect();
        assert!(rendered.iter().any(|line| line.contains("internal chain")));
        assert!(rendered
            .iter()
            .any(|line| line.to_ascii_lowercase().contains("grep")));
        assert!(rendered.iter().any(|line| line.starts_with("debug")));
        assert!(rendered.iter().any(|line| line.contains("session resumed")));
    }

    #[test]
    fn transcript_uses_compact_claude_style_prefixes() {
        let mut app = App::new(None);
        app.add_user_message("Inspect this".to_string());
        app.apply(EventUpdate::Reasoning {
            reasoning_id: "reasoning-1".to_string(),
            content: "Checking the files".to_string(),
            agent_id: None,
        });
        app.apply(EventUpdate::AssistantMessage {
            message_id: "message-1".to_string(),
            content: "Done".to_string(),
            agent_id: None,
        });

        let lines = super::chat_lines(&app);
        assert!(lines[1].to_string().starts_with("❯ Inspect this"));
        assert_eq!(lines[3].to_string(), "   Checking the files");
        assert_eq!(lines[5].to_string(), "● Done");
        assert_eq!(lines[3].spans[1].style.fg, Some(Color::DarkGray));
        assert_eq!(lines[7].to_string(), "✻ Copilot is responding…");
    }

    #[test]
    fn entry_lines_return_body_content_without_transcript_prefixes() {
        let user = super::entry_lines(&ChatEntry::User("Inspect this".to_string()), false);
        let assistant = super::entry_lines(
            &ChatEntry::Assistant {
                message_id: "message-1".to_string(),
                content: "Done".to_string(),
                agent_id: None,
            },
            false,
        );

        assert_eq!(
            user.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["Inspect this"]
        );
        assert_eq!(
            assistant
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["Done"]
        );
        assert!(user
            .iter()
            .chain(&assistant)
            .all(|line| !line.to_string().starts_with(['❯', '⏺', '●'])));
        assert!(user
            .iter()
            .chain(&assistant)
            .flat_map(|line| &line.spans)
            .all(|span| span.style.fg == Some(crate::palette::TEXT)));
    }

    #[test]
    fn long_user_messages_keep_grapheme_edges_and_count_omitted_newlines() {
        let first = "e\u{301}".repeat(2_500);
        let middle = format!("\n{}\n", "m".repeat(5_001));
        let last = "👩\u{200d}💻".repeat(2_500);
        let content = format!("{first}{middle}{last}");

        let truncated = super::truncate_user_content(&content);
        let lines = truncated.split('\n').collect::<Vec<_>>();

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], first);
        assert_eq!(lines[1], "… +2 lines …");
        assert_eq!(lines[2], last);
    }

    #[test]
    fn user_truncation_marker_survives_the_visual_buffer() {
        let mut app = App::new(None);
        app.add_user_message("x".repeat(10_001));

        let lines = super::chat_lines_at_width(&app, 80);
        let marker_rows = lines
            .iter()
            .filter(|line| line.to_string().contains("… +0 lines …"))
            .count();
        let mut terminal = Terminal::new(TestBackend::new(80, lines.len() as u16))
            .expect("test terminal should initialize");
        terminal
            .draw(|frame| {
                frame.render_widget(ratatui::widgets::Paragraph::new(lines), frame.area())
            })
            .expect("truncated user message should render");

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert_eq!(marker_rows, 1);
        assert!(rendered.contains("… +0 lines …"));
    }

    #[test]
    fn bottom_follow_shows_the_last_word_wrapped_response_row() {
        let mut app = App::new(None);
        app.apply(EventUpdate::AssistantMessage {
            message_id: "message-long".to_string(),
            content: [
                "aaaaaaaaaaaaaaaaaaaa",
                "bbbbbbbbbbbbbbbbbbbb",
                "cccccccccccccccccccc",
                "dddddddddddddddddddd",
                "eeeeeeeeeeeeeeeeeeee",
                "FINAL_RESPONSE_MARKER",
            ]
            .join(" "),
            agent_id: None,
        });
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal.draw(|frame| draw(frame, &app)).expect("draw TUI");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("FINAL_RESPONSE_MARKER"));
    }

    #[test]
    fn reasoning_markdown_stays_gray() {
        let lines = super::markdown_lines(
            "# Plan\n\nUse **care** and `inspect`.",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        );

        assert_eq!(lines.len(), 2);
        assert!(lines.iter().flat_map(|line| &line.spans).all(|span| {
            span.style.fg == Some(Color::DarkGray)
                && span.style.add_modifier.contains(Modifier::ITALIC)
        }));
        assert!(lines[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
    }

    #[test]
    fn markdown_formats_lists_emphasis_and_code() {
        let lines =
            super::markdown_lines("## Result\n\n- **ready**\n- `cargo test`", Style::default());

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].to_string(), "Result");
        assert_eq!(lines[1].to_string(), "* ready");
        assert_eq!(lines[2].to_string(), "* cargo test");
        assert!(lines[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
        assert!(lines[1].spans[1]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
        assert_eq!(lines[1].spans[1].style.fg, None);
        assert_eq!(lines[2].spans[1].style.fg, Some(Color::Rgb(242, 204, 96)));
    }

    #[test]
    fn assistant_h1_uses_inherited_heading_modifiers_and_two_trailing_newlines() {
        let lines = super::entry_lines(
            &ChatEntry::Assistant {
                message_id: "message-heading".to_string(),
                content: "# Result".to_string(),
                agent_id: None,
            },
            false,
        );

        assert_eq!(
            lines.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["Result", ""]
        );
        let style = lines[0].spans[0].style;
        assert_eq!(style.fg, Some(crate::palette::TEXT));
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert!(style.add_modifier.contains(Modifier::ITALIC));
        assert!(style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn assistant_markdown_changes_do_not_change_user_markdown() {
        let lines = super::entry_lines(
            &ChatEntry::User("~~literal~~ and `code`".to_string()),
            false,
        );

        assert_eq!(lines[0].to_string(), "literal and code");
        assert!(lines[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::CROSSED_OUT));
        let code = lines[0]
            .spans
            .iter()
            .find(|span| span.content.as_ref() == "code")
            .expect("user inline code span");
        assert_eq!(code.style.fg, Some(Color::Rgb(242, 204, 96)));
    }

    #[test]
    fn nested_assistant_markdown_keeps_semantics_without_transcript_prefixes() {
        let lines = super::entry_lines(
            &ChatEntry::Assistant {
                message_id: "nested-heading".to_string(),
                content: "# Nested".to_string(),
                agent_id: Some("agent-1".to_string()),
            },
            false,
        );
        let rendered = render_entry_lines(
            crate::screen_model::LiveEntryKind::AssistantNested,
            &lines,
            40,
        );

        assert_eq!(
            rendered.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["Nested", ""]
        );
        assert!(rendered
            .iter()
            .flat_map(|line| &line.spans)
            .all(|span| !span.content.contains('●')));
    }

    #[test]
    fn live_and_committed_assistant_code_blocks_have_identical_rows() {
        let content = "```rust\nfn main() { let value = 42; }\n```";
        let mut app = App::new(None);
        app.apply(EventUpdate::AssistantDelta {
            message_id: "message-code".to_string(),
            content: content.to_string(),
            agent_id: None,
        });
        let live_changes = app.take_screen_changes();

        app.apply(EventUpdate::AssistantMessage {
            message_id: "message-code".to_string(),
            content: content.to_string(),
            agent_id: None,
        });
        let committed_changes = app.take_screen_changes();

        let rows = |changes: &[ScreenChange]| {
            let entry = changes.iter().find_map(|change| match change {
                ScreenChange::Upsert(entry) => Some(entry),
                ScreenChange::Reset | ScreenChange::Remove(_) => None,
            });
            let entry = entry.expect("assistant screen change");
            render_transcript_payload(entry.kind(), entry.payload(), 40)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        };

        assert_eq!(rows(&live_changes), rows(&committed_changes));
    }

    #[test]
    fn markdown_renders_tables_as_aligned_terminal_rows() {
        let lines = super::markdown_lines(
            "| OS | Version |\n| --- | ---: |\n| Windows | 11 |\n| Ubuntu | 24.04 |",
            Style::default(),
        );
        let rendered: Vec<String> = lines.iter().map(ToString::to_string).collect();

        assert_eq!(
            rendered,
            vec![
                "OS      | Version",
                "--------+--------",
                "Windows |      11",
                "Ubuntu  |   24.04",
            ]
        );
        assert!(lines[0]
            .spans
            .iter()
            .all(|span| span.style.add_modifier.contains(Modifier::BOLD)));
    }

    #[test]
    fn shell_commands_and_results_are_visible_without_internals() {
        let mut app = App::new(None);
        app.apply(EventUpdate::ToolStarted {
            tool_call_id: "tool-shell".to_string(),
            tool_name: "powershell".to_string(),
            arguments: Some(serde_json::json!({ "command": "Get-Date" })),
            agent_id: None,
        });
        app.apply(EventUpdate::ToolCompleted {
            tool_call_id: "tool-shell".to_string(),
            success: true,
            message: Some("Monday, August 31, 2026".to_string()),
            agent_id: None,
        });

        let rendered: Vec<String> = super::chat_lines(&app)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(rendered.iter().any(|line| line.contains("Get-Date")));
        assert!(rendered
            .iter()
            .any(|line| line.contains("Monday, August 31, 2026")));
    }

    #[test]
    fn status_bar_shows_compact_model_and_reasoning_metadata() {
        let mut app = App::new_with_working_directory(
            Some("gpt-5".to_string()),
            Path::new("C:\\dev\\picopilot"),
        );
        app.set_reasoning_effort(Some("high".to_string()));
        let backend = TestBackend::new(100, 1);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| frame.render_widget(status_bar(&app), frame.area()))
            .expect("status bar renders");
        let rendered =
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .fold(String::new(), |mut text, cell| {
                    text.push_str(cell.symbol());
                    text
                });

        assert!(rendered
            .contains("picopilot  ·  gpt-5  ·  high reasoning  ·  autopilot ready  ·  tools 7/7"));
        assert!(!rendered.contains("C:\\dev\\picopilot"));
    }

    #[test]
    fn tool_picker_supports_toggle_shell_only_all_and_apply() {
        let mut app = App::new(None);

        assert_eq!(handle_key(&mut app, ctrl_key('k')), UiAction::LoadTools);
        app.open_tool_picker();
        assert!(app.modal_is_open());
        assert_eq!(app.toolset(), Toolset::all());

        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char(' '), KeyEventKind::Press)),
            UiAction::None
        );
        assert!(!app.picker_toolset.contains(CANONICAL_TOOLS[0]));
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('s'), KeyEventKind::Press)),
            UiAction::None
        );
        assert_eq!(app.picker_toolset, Toolset::shell_only());
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('a'), KeyEventKind::Press)),
            UiAction::None
        );
        assert_eq!(app.picker_toolset, Toolset::all());
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::ApplyToolset(Toolset::all())
        );
        assert!(!app.modal_is_open());
    }

    #[test]
    fn tool_picker_navigation_toggles_the_selected_tool() {
        let mut app = App::new(None);
        app.open_tool_picker();

        handle_key(&mut app, key(KeyCode::Down, KeyEventKind::Press));
        handle_key(&mut app, key(KeyCode::Char(' '), KeyEventKind::Press));

        assert_eq!(app.selected_item, 1);
        assert!(!app.picker_toolset.contains_at(1));
        assert_eq!(TOOL_COUNT, CANONICAL_TOOLS.len());
    }

    #[test]
    fn tool_picker_fills_the_terminal_and_shows_checkbox_state() {
        let mut app = App::new(None);
        app.set_toolset(Toolset::shell_only());
        app.open_tool_picker();
        let mut terminal = Terminal::new(TestBackend::new(80, 12)).expect("test terminal");

        terminal
            .draw(|frame| draw_tool_picker(frame, &app, frame.area()))
            .expect("tool picker should render");

        let rendered = (0..terminal.backend().buffer().area.height)
            .map(|y| {
                (0..terminal.backend().buffer().area.width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rendered.iter().any(|line| line.contains("[x]")));
        assert!(rendered.iter().any(|line| line.contains("[ ]")));
        assert!(rendered
            .iter()
            .any(|line| line.contains("powershell") || line.contains("bash")));
    }

    #[test]
    fn skill_picker_supports_toggle_none_all_and_apply() {
        let mut app = App::new(None);
        app.set_skill_catalog(test_skill_catalog());

        assert_eq!(handle_key(&mut app, ctrl_key('s')), UiAction::LoadSkills);
        app.open_skill_picker();
        assert!(app.modal_is_open());

        handle_key(&mut app, key(KeyCode::Char(' '), KeyEventKind::Press));
        assert!(app.picker_skill_selection.contains("rust-review"));
        handle_key(&mut app, key(KeyCode::Down, KeyEventKind::Press));
        handle_key(&mut app, key(KeyCode::Char(' '), KeyEventKind::Press));
        assert!(app.picker_skill_selection.contains("runbook-extended"));

        handle_key(&mut app, key(KeyCode::Char('n'), KeyEventKind::Press));
        assert!(app.picker_skill_selection.is_empty());
        handle_key(&mut app, key(KeyCode::Char('a'), KeyEventKind::Press));
        assert_eq!(app.picker_skill_selection.len(), 4);
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::ApplySkills(SkillSelection::from_names(
                &test_skill_catalog(),
                [
                    "rust-review",
                    "runbook-extended",
                    "runbook",
                    "internal-helper",
                ],
            ))
        );
        assert!(!app.modal_is_open());
    }

    #[test]
    fn skill_picker_cancel_and_blocked_state_leave_active_selection_unchanged() {
        let mut app = App::new(None);
        let catalog = test_skill_catalog();
        app.set_skill_catalog(catalog.clone());
        app.set_skill_selection(SkillSelection::from_names(&catalog, ["rust-review"]));
        app.open_skill_picker();
        handle_key(&mut app, key(KeyCode::Char(' '), KeyEventKind::Press));
        handle_key(&mut app, key(KeyCode::Esc, KeyEventKind::Press));

        assert_eq!(
            app.skill_selection(),
            &SkillSelection::from_names(&catalog, ["rust-review"])
        );
        app.status.busy = true;
        assert!(app.skill_selection_change_is_blocked());
    }

    #[test]
    fn skill_picker_shortcut_ignores_key_release_events() {
        let mut app = App::new(None);

        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('s'), KeyEventKind::Release)),
            UiAction::None
        );
        assert_eq!(handle_key(&mut app, ctrl_key('s')), UiAction::LoadSkills);
    }

    #[test]
    fn skill_picker_renders_description_and_discovery_source() {
        let mut app = App::new(None);
        app.set_skill_catalog(test_skill_catalog());
        app.open_skill_picker();
        let mut terminal = Terminal::new(TestBackend::new(100, 16)).expect("test terminal");

        terminal
            .draw(|frame| draw_skill_picker(frame, &app, frame.area()))
            .expect("skill picker should render");

        let rendered = (0..terminal.backend().buffer().area.height)
            .map(|y| {
                (0..terminal.backend().buffer().area.width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rendered.iter().any(|line| line.contains("[ ]")));
        assert!(rendered.iter().any(|line| line.contains("rust-review")));
        assert!(rendered
            .iter()
            .any(|line| line.contains("Review Rust code")));
        assert!(rendered
            .iter()
            .any(|line| line.contains("project") && line.contains(".agents")));
    }

    #[test]
    fn slash_completion_filters_invocable_skills_and_accepts_with_enter() {
        let mut app = App::new(None);
        app.set_skill_catalog(test_skill_catalog());
        for character in "/r".chars() {
            app.push_input(character);
        }

        let candidates = app
            .completion
            .as_ref()
            .expect("slash completion should open")
            .candidates
            .iter()
            .map(|candidate| candidate.command.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            candidates,
            vec!["/rust-review", "/runbook-extended", "/runbook"]
        );

        handle_key(&mut app, key(KeyCode::Down, KeyEventKind::Press));
        handle_key(&mut app, key(KeyCode::Down, KeyEventKind::Press));
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::None
        );
        assert_eq!(app.input(), "/runbook");
        assert!(app.completion.is_none());

        let mut hidden = App::new(None);
        hidden.set_skill_catalog(test_skill_catalog());
        for character in "/internal".chars() {
            hidden.push_input(character);
        }
        assert!(hidden.completion.is_none());
    }

    #[test]
    fn exact_slash_completion_outranks_longer_prefixes_and_submits() {
        let mut app = App::new(None);
        app.set_skill_catalog(test_skill_catalog());
        for character in "/runbook".chars() {
            app.push_input(character);
        }

        assert_eq!(
            app.completion.as_ref().unwrap().candidates[0].command,
            "/runbook"
        );
        assert_eq!(app.completion.as_ref().unwrap().candidates.len(), 2);
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::Send("/runbook".to_string())
        );
    }

    #[test]
    fn slash_completion_tab_preserves_trailing_arguments_and_escape_dismisses() {
        let mut app = App::new(None);
        app.set_skill_catalog(test_skill_catalog());
        for character in "/rus extra".chars() {
            app.push_input(character);
        }
        for _ in 0..6 {
            app.move_input_left();
        }
        assert!(app.completion.is_some());

        assert_eq!(
            handle_key(&mut app, key(KeyCode::Tab, KeyEventKind::Press)),
            UiAction::None
        );
        assert_eq!(app.input(), "/rust-review extra");
        assert!(app.completion.is_none());

        let mut dismissed = App::new(None);
        dismissed.set_skill_catalog(test_skill_catalog());
        for character in "/r".chars() {
            dismissed.push_input(character);
        }
        assert_eq!(
            handle_key(&mut dismissed, key(KeyCode::Esc, KeyEventKind::Press)),
            UiAction::None
        );
        assert_eq!(dismissed.input(), "/r");
        assert!(dismissed.completion.is_none());
    }

    #[test]
    fn exact_slash_skill_is_submitted_literally() {
        let mut app = App::new(None);
        app.set_skill_catalog(test_skill_catalog());
        for character in "/rust-review inspect this".chars() {
            app.push_input(character);
        }

        assert_eq!(
            handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::Send("/rust-review inspect this".to_string())
        );
        assert!(app.input().is_empty());
    }

    #[test]
    fn slash_invocation_auto_checks_only_known_invocable_skills() {
        let catalog = test_skill_catalog();
        let active = SkillSelection::none();

        let selected =
            skill_selection_for_invocation(&catalog, &active, "/rust-review inspect this")
                .expect("known invocable skill should be selected");
        assert!(selected.contains("rust-review"));
        assert!(skill_selection_for_invocation(&catalog, &selected, "/rust-review").is_none());
        assert!(skill_selection_for_invocation(&catalog, &active, "/internal-helper").is_none());
        assert!(skill_selection_for_invocation(&catalog, &active, "/unknown command").is_none());
    }

    #[test]
    fn unknown_and_non_invocable_slash_commands_are_ordinary_messages() {
        for prompt in ["/unknown context", "/internal-helper context"] {
            let mut app = App::new(None);
            app.set_skill_catalog(test_skill_catalog());
            for character in prompt.chars() {
                app.push_input(character);
            }

            assert_eq!(
                handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
                UiAction::Send(prompt.to_string())
            );
        }
    }

    #[test]
    fn skill_selection_status_and_new_conversation_reset_are_visible() {
        let mut app = App::new(None);
        let catalog = test_skill_catalog();
        app.set_skill_catalog(catalog.clone());
        app.set_skill_selection(SkillSelection::from_names(&catalog, ["rust-review"]));
        let mut terminal = Terminal::new(TestBackend::new(140, 1)).expect("test terminal");
        terminal
            .draw(|frame| frame.render_widget(status_bar(&app), frame.area()))
            .expect("status bar renders");
        let line = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(line.contains("skills 1/4"));

        app.reset_for_new_conversation();
        assert!(app.skill_selection().is_empty());
    }

    #[test]
    fn status_bar_displays_the_selected_tool_count() {
        let mut app = App::new(None);
        app.set_toolset(Toolset::shell_only());
        let mut terminal = Terminal::new(TestBackend::new(120, 1)).expect("test terminal");
        terminal
            .draw(|frame| frame.render_widget(status_bar(&app), frame.area()))
            .expect("status bar renders");
        let line = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(line.contains("tools 1/7"));
    }

    #[test]
    fn displayed_reasoning_uses_the_model_default_without_an_override() {
        let model = Model {
            id: "gpt-5".to_string(),
            default_reasoning_effort: Some("medium".to_string()),
            ..Model::default()
        };

        assert_eq!(
            displayed_reasoning_effort(std::slice::from_ref(&model), Some("gpt-5"), None)
                .as_deref(),
            Some("medium")
        );
        assert_eq!(
            displayed_reasoning_effort(&[model], Some("gpt-5"), Some("high")).as_deref(),
            Some("high")
        );
    }

    #[test]
    fn resumed_history_replaces_the_previous_transcript() {
        let mut app = App::new(None);
        app.add_user_message("old session".to_string());
        let events = vec![github_copilot_sdk::types::SessionEvent {
            id: "event-user".to_string(),
            timestamp: "2026-08-31T12:00:00Z".to_string(),
            parent_id: None,
            ephemeral: None,
            agent_id: None,
            debug_cli_received_at_ms: None,
            debug_ws_forwarded_at_ms: None,
            event_type: "user.message".to_string(),
            data: json!({ "content": "resumed session", "source": "user" }),
        }];

        app.replace_history(&events);

        assert_eq!(
            app.entries(),
            &[ChatEntry::User("resumed session".to_string())]
        );
    }

    #[test]
    fn new_conversation_reset_clears_transcript_state_and_keeps_preferences() {
        let mut app = App::new(Some("gpt-5".to_string()));
        app.set_reasoning_effort(Some("high".to_string()));
        app.set_toolset(Toolset::shell_only());
        app.show_internals = true;
        app.add_user_message("old session".to_string());
        app.push_input('x');
        app.set_usage_metrics(crate::events::UsageMetricsSnapshot {
            total_nano_aiu: Some(2.0),
            total_premium_request_cost: 1.0,
            total_user_requests: 1,
            total_api_duration_ms: 250,
            current_model: Some("gpt-5".to_string()),
        });
        app.set_fleet_active(true);
        app.set_todos(TodoSnapshot {
            rows: Vec::new(),
            dependencies: Vec::new(),
        });
        app.reconnecting = true;
        app.blocked = true;

        app.reset_for_new_conversation();

        assert!(app.entries().is_empty());
        assert!(app.pending_user_messages.is_empty());
        assert!(app.input().is_empty());
        assert_eq!(app.status().model.as_deref(), Some("gpt-5"));
        assert_eq!(app.status().reasoning_effort.as_deref(), Some("high"));
        assert!(app.status().usage_metrics.is_none());
        assert!(app.status().context_attribution.is_none());
        assert!(!app.status().busy);
        assert!(!app.modal_is_open());
        assert_eq!(app.toolset(), Toolset::shell_only());
        assert!(!app.fleet_active);
        assert!(app.todos.is_none());
        assert!(!app.reconnecting);
        assert!(!app.blocked);
        assert!(app.show_internals);
    }

    #[test]
    fn new_conversation_requires_an_idle_control_press() {
        let mut app = App::new(None);

        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('n'), KeyEventKind::Release)),
            UiAction::None
        );
        assert_eq!(
            handle_key(&mut app, ctrl_key('n')),
            UiAction::NewConversation
        );

        app.status.busy = true;
        assert_eq!(handle_key(&mut app, ctrl_key('n')), UiAction::None);
        app.status.busy = false;

        app.set_reconnecting(true);
        assert_eq!(handle_key(&mut app, ctrl_key('n')), UiAction::None);
        app.set_reconnecting(false);

        app.blocked = true;
        assert_eq!(handle_key(&mut app, ctrl_key('n')), UiAction::None);
        app.blocked = false;

        app.open_tool_picker();
        assert_eq!(handle_key(&mut app, ctrl_key('n')), UiAction::None);
        app.close_modal();

        let (respond_to, _response) = tokio::sync::oneshot::channel();
        app.enqueue_approval(crate::permissions::ApprovalRequest {
            category: crate::permissions::ApprovalCategory::Shell,
            tool_name: "bash".to_string(),
            details: "pwd".to_string(),
            respond_to,
        });
        assert_eq!(handle_key(&mut app, ctrl_key('n')), UiAction::None);
    }

    #[test]
    fn live_user_event_reconciles_the_optimistic_message() {
        let mut app = App::new(None);
        app.add_user_message("Hi".to_string());

        app.apply(EventUpdate::UserMessage {
            content: "Hi".to_string(),
        });

        assert_eq!(app.entries(), &[ChatEntry::User("Hi".to_string())]);
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

    #[test]
    fn fleet_requires_an_explicit_command() {
        let mut app = App::new(None);
        for character in "/fleet inspect the parser".chars() {
            app.push_input(character);
        }

        assert_eq!(
            handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::StartFleet("inspect the parser".to_string())
        );

        for character in "Hi".chars() {
            app.push_input(character);
        }
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::Send("Hi".to_string())
        );
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
            .resolve_approval(crate::permissions::ApprovalDecision::ApproveOnce)
            .expect("approval should still be queued");
        request
            .respond_to
            .send(crate::permissions::ApprovalDecision::ApproveOnce)
            .expect("test response receiver should still be open");
        assert_eq!(
            response.await.expect("approval response should arrive"),
            crate::permissions::ApprovalDecision::ApproveOnce
        );
        assert!(matches!(
            app.entries().last(),
            Some(ChatEntry::Approval {
                status: super::ApprovalStatus::ApprovedOnce,
                ..
            })
        ));
    }

    #[test]
    fn approval_details_toggle_while_the_input_is_hijacked() {
        let mut app = App::new(None);
        let (respond_to, _response) = tokio::sync::oneshot::channel();
        app.enqueue_approval(crate::permissions::ApprovalRequest {
            category: crate::permissions::ApprovalCategory::Shell,
            tool_name: "bash".to_string(),
            details: "cargo test --all-targets".to_string(),
            respond_to,
        });

        assert!(matches!(
            app.entries().last(),
            Some(ChatEntry::Approval {
                status: super::ApprovalStatus::Pending,
                details,
                ..
            }) if details == "cargo test --all-targets"
        ));
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('v'), KeyEventKind::Press)),
            UiAction::None
        );
        assert!(app.show_approval_details);
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('v'), KeyEventKind::Press)),
            UiAction::None
        );
        assert!(!app.show_approval_details);
    }

    #[test]
    fn session_and_model_pickers_fill_the_terminal() {
        let terminal_area = ratatui::layout::Rect::new(0, 0, 120, 40);

        assert_eq!(
            modal_area(ModalKind::Sessions, terminal_area),
            terminal_area
        );
        assert_eq!(modal_area(ModalKind::Models, terminal_area), terminal_area);
        assert_eq!(modal_area(ModalKind::Tools, terminal_area), terminal_area);
        assert_ne!(modal_area(ModalKind::Usage, terminal_area), terminal_area);
    }

    #[test]
    fn tool_picker_can_open_while_an_approval_is_pending_but_not_apply_while_busy() {
        let mut app = App::new(None);
        let (respond_to, _response) = tokio::sync::oneshot::channel();
        app.enqueue_approval(crate::permissions::ApprovalRequest {
            category: crate::permissions::ApprovalCategory::Shell,
            tool_name: "bash".to_string(),
            details: "pwd".to_string(),
            respond_to,
        });

        assert_eq!(handle_key(&mut app, ctrl_key('k')), UiAction::LoadTools);
        app.open_tool_picker();
        assert!(app.toolset_change_is_blocked());
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char(' '), KeyEventKind::Press)),
            UiAction::None
        );
        assert!(app.modal_is_open());
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Esc, KeyEventKind::Press)),
            UiAction::None
        );
        assert!(!app.modal_is_open());

        app.open_tool_picker();
        assert!(matches!(
            handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::ApplyToolset(_)
        ));

        app.pending_approvals.clear();
        app.status.busy = true;
        app.open_tool_picker();
        assert!(matches!(
            handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::ApplyToolset(_)
        ));
        assert!(app.toolset_change_is_blocked());
    }

    #[test]
    fn session_navigation_stays_local_and_modal_selection_emits_actions() {
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
        assert_eq!(app.selected_item, 1);
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Esc, KeyEventKind::Press)),
            UiAction::None
        );
        assert!(!app.modal_is_open());

        app.set_sessions(vec![SessionMetadata {
            session_id: SessionId::from("session-2"),
            start_time: "2026-08-31T12:00:00Z".to_string(),
            modified_time: "2026-08-31T12:02:00Z".to_string(),
            summary: Some("second".to_string()),
            is_remote: false,
        }]);
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
    fn model_selection_converts_supported_options_for_the_sdk() {
        let selection = ModelSelection {
            model: "gpt-5".to_string(),
            reasoning_effort: Some("high".to_string()),
            context_tier: Some("long_context".to_string()),
        };

        let options = selection
            .sdk_options()
            .expect("supported context tier should convert")
            .expect("selected options should be forwarded");

        assert_eq!(options.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(options.context_tier, Some(ContextTier::LongContext));
    }

    #[test]
    fn model_picker_formats_cost_and_context_metadata() {
        let model: Model = serde_json::from_value(json!({
            "billing": { "multiplier": 1.5 },
            "capabilities": {
                "limits": { "max_context_window_tokens": 200000 }
            },
            "id": "gpt-5",
            "modelPickerPriceCategory": "high",
            "name": "GPT-5"
        }))
        .expect("model metadata should deserialize");

        assert_eq!(model_cost_label_for(&model, false), "high");
        assert_eq!(model_context_label(&model), "200,000");
        assert_eq!(
            model_picker_row_for(&model, false),
            "GPT-5                         high       200,000 tokens"
        );
    }

    #[test]
    fn local_model_rows_show_provider_tracked_cost_and_unknown_context() {
        let model = Model {
            id: "local/qwen:7b".to_string(),
            name: "local/qwen:7b".to_string(),
            ..Model::default()
        };
        let mut app = App::new(Some(model.id.clone()));
        app.set_local_model_ids(vec![model.id.clone()]);
        app.set_models(vec![model.clone()]);

        assert_eq!(model_cost_label_for(&model, true), "local");
        assert_eq!(model_context_label(&model), "unknown");
        assert!(model_picker_row_for(&model, true).contains("local"));
        let details: Vec<String> = model_picker_detail_lines(&app)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(details.iter().any(|line| line.contains("local inference")));
        assert!(details
            .iter()
            .any(|line| line.contains("Context:   model default   Available: unavailable")));

        handle_key(&mut app, key(KeyCode::Char('r'), KeyEventKind::Press));
        handle_key(&mut app, key(KeyCode::Char('c'), KeyEventKind::Press));
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::SwitchModel(ModelSelection {
                model: "local/qwen:7b".to_string(),
                reasoning_effort: None,
                context_tier: None,
            })
        );
    }

    #[test]
    fn model_picker_opens_on_the_active_model_and_isolates_its_options() {
        let mut app = App::new(Some("gpt-5".to_string()));
        app.set_models(vec![
            Model {
                id: "auto".to_string(),
                name: "Auto".to_string(),
                ..Default::default()
            },
            Model {
                id: "gpt-5".to_string(),
                name: "GPT-5".to_string(),
                supported_reasoning_efforts: Some(vec!["low".to_string(), "high".to_string()]),
                supported_context_tiers: Some(vec!["default".to_string()]),
                ..Default::default()
            },
        ]);

        assert_eq!(app.selected_item, 1);
        let details: Vec<String> = model_picker_detail_lines(&app)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(details[0], "GPT-5  (gpt-5)");
        assert_eq!(
            details[1],
            "Reasoning: model default   Available: low · high"
        );
        assert_eq!(details[2], "Context:   model default   Available: default");
    }

    #[test]
    fn model_picker_keeps_rows_compact_at_a_typical_terminal_size() {
        let mut app = App::new(Some("gpt-5".to_string()));
        app.set_models(vec![
            serde_json::from_value(json!({
                "billing": { "multiplier": 0.0 },
                "capabilities": { "limits": { "max_context_window_tokens": 128000 } },
                "id": "auto",
                "name": "Auto"
            }))
            .expect("auto model metadata"),
            serde_json::from_value(json!({
                "billing": { "multiplier": 1.0 },
                "capabilities": { "limits": { "max_context_window_tokens": 200000 } },
                "id": "gpt-5",
                "name": "GPT-5",
                "supportedReasoningEfforts": ["low", "high"],
                "supportedContextTiers": ["default"]
            }))
            .expect("gpt-5 model metadata"),
        ]);
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).expect("test terminal");

        terminal
            .draw(|frame| draw_model_picker(frame, &app, frame.area()))
            .expect("picker should render");

        let buffer = terminal.backend().buffer();
        let rendered = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rendered.iter().any(|line| line.contains("Auto")));
        assert!(rendered.iter().any(|line| line.contains("GPT-5")));
        assert_eq!(
            rendered
                .iter()
                .filter(|line| line.contains("low · high"))
                .count(),
            1
        );
        assert_eq!(
            rendered
                .iter()
                .filter(|line| line.contains("Available: default"))
                .count(),
            1
        );
    }

    #[test]
    fn usage_key_requests_the_usage_detail_modal() {
        let mut app = App::new(None);

        assert_eq!(handle_key(&mut app, ctrl_key('u')), UiAction::LoadUsage);
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
        assert_eq!(handle_key(&mut app, ctrl_key('u')), UiAction::None);
        assert!(!app.modal_is_open());
    }

    #[test]
    fn status_cost_updates_without_opening_the_usage_modal() {
        let mut app = App::new(None);

        app.set_usage_metrics(crate::events::UsageMetricsSnapshot {
            total_nano_aiu: Some(7.25),
            total_premium_request_cost: 0.0,
            total_user_requests: 2,
            total_api_duration_ms: 500,
            current_model: Some("gpt-5".to_string()),
        });

        assert_eq!(
            app.status()
                .usage_metrics
                .as_ref()
                .and_then(|metrics| metrics.total_nano_aiu),
            Some(7.25)
        );
        assert!(!app.modal_is_open());
    }

    #[test]
    fn formats_nano_aiu_as_readable_aiu() {
        let metrics = crate::events::UsageMetricsSnapshot {
            total_nano_aiu: Some(8_063_475_000.0),
            total_premium_request_cost: 0.0,
            total_user_requests: 1,
            total_api_duration_ms: 100,
            current_model: Some("gpt-5".to_string()),
        };

        assert_eq!(super::format_cost(&metrics), "8.063 AIU");
    }

    #[test]
    fn todo_modal_is_only_available_for_an_active_fleet() {
        let mut app = App::new(None);

        assert_eq!(handle_key(&mut app, ctrl_key('t')), UiAction::None);
        assert!(app.input().is_empty());

        app.set_fleet_active(true);
        assert_eq!(handle_key(&mut app, ctrl_key('t')), UiAction::LoadTodos);

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

        assert_eq!(handle_key(&mut app, ctrl_key('t')), UiAction::None);
        assert!(!app.modal_is_open());
    }

    #[test]
    fn main_window_commands_require_control_and_plain_letters_remain_input() {
        let mut app = App::new(None);
        for character in "quantum".chars() {
            assert_eq!(
                handle_key(&mut app, key(KeyCode::Char(character), KeyEventKind::Press)),
                UiAction::None
            );
        }
        assert_eq!(app.input(), "quantum");

        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('n'), KeyEventKind::Press)),
            UiAction::None
        );
        assert_eq!(app.input(), "quantumn");
        assert_eq!(
            handle_key(&mut app, ctrl_key('n')),
            UiAction::NewConversation
        );
        assert_eq!(handle_key(&mut app, ctrl_key('o')), UiAction::LoadSessions);
        assert_eq!(handle_key(&mut app, ctrl_key('p')), UiAction::LoadModels);
        assert_eq!(handle_key(&mut app, ctrl_key('u')), UiAction::LoadUsage);
        assert_eq!(handle_key(&mut app, ctrl_key('x')), UiAction::Quit);
        assert_eq!(app.input(), "quantumn");
    }

    #[test]
    fn main_window_renders_a_borderless_transcript_and_one_line_shortcuts() {
        let app = App::new(None);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &app))
            .expect("main window should render");

        let buffer = terminal.backend().buffer();
        let row = |y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };
        assert!(row(29).contains("^O sessions ^P models ^U usage"));
        assert!(row(29).contains("^N new ^O sessions"));
        assert!(row(29).contains("^K tools ^S skills"));
        assert!(row(29).contains("^I internals ^X exit"));
        assert!(!row(1).contains('┌'));
        assert!(!row(1).contains('│'));
    }

    #[tokio::test]
    async fn fleet_start_owns_the_prompt_when_supported() {
        let path = send_with_fleet_fallback(
            "parallelize this".to_string(),
            |request| async move {
                assert_eq!(request.prompt.as_deref(), Some("parallelize this"));
                Ok::<_, String>(FleetStartResult { started: true })
            },
            |_prompt| async { panic!("single-agent fallback should not send") },
            |_error: &String| false,
        )
        .await
        .expect("Fleet start should succeed");

        assert_eq!(path, SendPath::Fleet);
    }

    #[tokio::test]
    async fn fleet_start_falls_back_to_one_single_agent_delegation() {
        let path = send_with_fleet_fallback(
            "inspect this".to_string(),
            |_request| async { Ok::<_, String>(FleetStartResult { started: false }) },
            |prompt| async move {
                assert_eq!(prompt, "inspect this");
                Ok::<_, String>(())
            },
            |_error: &String| false,
        )
        .await
        .expect("single-agent fallback should succeed");

        assert_eq!(path, SendPath::Single);
    }

    #[tokio::test]
    async fn fleet_start_errors_are_silently_fallbacked() {
        let path = send_with_fleet_fallback(
            "repair this".to_string(),
            |_request| async { Err::<FleetStartResult, _>("unsupported".to_string()) },
            |prompt| async move {
                assert_eq!(prompt, "repair this");
                Ok::<_, String>(())
            },
            |_error: &String| false,
        )
        .await
        .expect("single-agent fallback should handle a Fleet error");

        assert_eq!(path, SendPath::Single);
    }

    #[tokio::test]
    async fn fleet_transport_errors_are_not_retried_as_single_agent_prompts() {
        let result = send_with_fleet_fallback(
            "inspect this".to_string(),
            |_request| async { Err::<FleetStartResult, _>("transport".to_string()) },
            |_prompt| async { panic!("a transport failure must not duplicate the prompt") },
            |error: &String| error == "transport",
        )
        .await;

        assert_eq!(result, Err("transport".to_string()));
    }

    #[test]
    fn reconnecting_hijacks_input_and_marks_in_flight_tools_unknown() {
        let mut app = App::new(None);
        app.apply(EventUpdate::ToolStarted {
            tool_call_id: "tool-1".to_string(),
            tool_name: "edit".to_string(),
            arguments: None,
            agent_id: None,
        });
        app.set_reconnecting(true);

        app.push_input('x');
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('y'), KeyEventKind::Press)),
            UiAction::None
        );
        assert_eq!(app.input(), "x");
        app.mark_in_flight_tools_unknown();
        assert!(matches!(
            app.entries().first(),
            Some(ChatEntry::Tool {
                unknown: true,
                success: None,
                ..
            })
        ));

        app.apply(EventUpdate::ToolCompleted {
            tool_call_id: "tool-1".to_string(),
            success: true,
            message: None,
            agent_id: None,
        });
        assert!(matches!(
            app.entries().first(),
            Some(ChatEntry::Tool {
                unknown: false,
                success: Some(true),
                ..
            })
        ));
    }

    #[test]
    fn blocking_error_ends_input_but_leaves_the_final_message_visible() {
        let mut app = App::new(None);
        app.add_user_message("keep working".to_string());

        app.apply(EventUpdate::Banner {
            severity: crate::events::BannerSeverity::BlockingError,
            message: "quota exhausted".to_string(),
            url: None,
        });

        assert!(!app.status().busy);
        assert!(matches!(
            app.entries().last(),
            Some(ChatEntry::Banner {
                severity: crate::events::BannerSeverity::BlockingError,
                message,
                ..
            }) if message == "quota exhausted"
        ));
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('x'), KeyEventKind::Press)),
            UiAction::None
        );
        assert!(app.input().is_empty());
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('q'), KeyEventKind::Press)),
            UiAction::None
        );
        assert_eq!(handle_key(&mut app, ctrl_key('x')), UiAction::Quit);
    }

    #[test]
    fn todo_modal_renders_dependency_titles() {
        let mut app = App::new(None);
        app.set_fleet_active(true);
        app.set_todos(TodoSnapshot {
            rows: vec![
                TodoRowSnapshot {
                    id: "todo-1".to_string(),
                    title: "Inspect transport".to_string(),
                    description: String::new(),
                    status: "completed".to_string(),
                },
                TodoRowSnapshot {
                    id: "todo-2".to_string(),
                    title: "Patch transport".to_string(),
                    description: String::new(),
                    status: "in_progress".to_string(),
                },
            ],
            dependencies: vec![TodoDependencySnapshot {
                todo_id: "todo-2".to_string(),
                depends_on: "todo-1".to_string(),
            }],
        });

        let rendered: Vec<String> = todo_detail_lines(&app)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect();

        assert_eq!(
            rendered[1],
            "[in_progress] Patch transport | blocked by: Inspect transport"
        );
    }

    fn key(code: KeyCode, kind: KeyEventKind) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    fn key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    fn ctrl_key(character: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(character),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }
}
