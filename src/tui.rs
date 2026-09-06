use crate::events::{
    context_attribution_snapshot, todo_snapshot, usage_metrics_snapshot, BannerSeverity,
    ContextAttributionSnapshot, EventUpdate, ShellCompletion, ShellExitMetadata, TodoSnapshot,
    UsageMetricsSnapshot, UsageSnapshot,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::hash::{Hash, Hasher};
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
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use ratatui::Terminal;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use github_copilot_sdk::rpc::{FleetStartRequest, FleetStartResult, TasksStartAgentRequest};
use github_copilot_sdk::subscription::EventSubscription;
use github_copilot_sdk::subscription::RecvErrorKind;
use github_copilot_sdk::types::{ContextTier, Model, SessionId, SessionMetadata, SetModelOptions};

use crate::ansi::{plain_from_sanitized, sanitize_ansi, sanitize_plain, AnsiSanitizer};
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
    LoadUsageCommand,
    LoadStatus,
    LocalCommandError(String),
    LoadTodos,
    LoadTools,
    LoadSkills,
    Resume(SessionId),
    SwitchModel(ModelSelection),
    ApplyToolset(Toolset),
    ApplySkills(SkillSelection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerKind {
    Sessions,
    Models,
    Tools,
    Skills,
    Approval,
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
        started_at: u64,
        cwd: PathBuf,
    },
    ToolProgress {
        tool_call_id: String,
        tool_name: String,
        output: String,
        status: String,
        kind: ToolProgressKind,
        agent_id: Option<String>,
        started_at: Option<u64>,
        timeout: Option<String>,
    },
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        arguments: Option<serde_json::Value>,
        content: String,
        partial_output: Option<String>,
        shell_completion: Option<ShellCompletion>,
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
    LocalOutput(Vec<Line<'static>>),
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

const BUILTIN_COMMANDS: &[(&str, &str)] = &[
    ("/fleet", "run work through Fleet"),
    ("/status", "show session and configuration status"),
    ("/usage", "show session usage and context attribution"),
];

static NEXT_SCREEN_NAMESPACE: AtomicU64 = AtomicU64::new(1);

const SPINNER_FRAME_MS: u64 = 120;
const SPINNER_TICK_MS: u64 = 50;
const SPINNER_STALL_AFTER_MS: u64 = 3_000;
const SPINNER_STALL_RAMP_MS: u64 = 2_000;
const SPINNER_STATUS_AFTER_MS: u64 = 30_000;
const SPINNER_THINKING_MIN_MS: u64 = 2_000;
const SPINNER_THOUGHT_HOLD_MS: u64 = 2_000;
const SPINNER_ERROR_COLOR: (u8, u8, u8) = (171, 43, 63);
const SPINNER_THINKING_INACTIVE: (u8, u8, u8) = (153, 153, 153);
const SPINNER_THINKING_SHIMMER: (u8, u8, u8) = (185, 185, 185);

const BUILTIN_SPINNER_VERBS: &[&str] = &[
    "Accomplishing",
    "Actioning",
    "Actualizing",
    "Architecting",
    "Baking",
    "Beaming",
    "Beboppin'",
    "Befuddling",
    "Billowing",
    "Blanching",
    "Bloviating",
    "Boogieing",
    "Boondoggling",
    "Booping",
    "Bootstrapping",
    "Brewing",
    "Bunning",
    "Burrowing",
    "Calculating",
    "Canoodling",
    "Caramelizing",
    "Cascading",
    "Catapulting",
    "Cerebrating",
    "Channeling",
    "Channelling",
    "Choreographing",
    "Churning",
    "Clauding",
    "Coalescing",
    "Cogitating",
    "Combobulating",
    "Composing",
    "Computing",
    "Concocting",
    "Considering",
    "Contemplating",
    "Cooking",
    "Crafting",
    "Creating",
    "Crunching",
    "Crystallizing",
    "Cultivating",
    "Deciphering",
    "Deliberating",
    "Determining",
    "Dilly-dallying",
    "Discombobulating",
    "Doing",
    "Doodling",
    "Drizzling",
    "Ebbing",
    "Effecting",
    "Elucidating",
    "Embellishing",
    "Enchanting",
    "Envisioning",
    "Evaporating",
    "Fermenting",
    "Fiddle-faddling",
    "Finagling",
    "Flambéing",
    "Flibbertigibbeting",
    "Flowing",
    "Flummoxing",
    "Fluttering",
    "Forging",
    "Forming",
    "Frolicking",
    "Frosting",
    "Gallivanting",
    "Galloping",
    "Garnishing",
    "Generating",
    "Gesticulating",
    "Germinating",
    "Gitifying",
    "Grooving",
    "Gusting",
    "Harmonizing",
    "Hashing",
    "Hatching",
    "Herding",
    "Honking",
    "Hullaballooing",
    "Hyperspacing",
    "Ideating",
    "Imagining",
    "Improvising",
    "Incubating",
    "Inferring",
    "Infusing",
    "Ionizing",
    "Jitterbugging",
    "Julienning",
    "Kneading",
    "Leavening",
    "Levitating",
    "Lollygagging",
    "Manifesting",
    "Marinating",
    "Meandering",
    "Metamorphosing",
    "Misting",
    "Moonwalking",
    "Moseying",
    "Mulling",
    "Mustering",
    "Musing",
    "Nebulizing",
    "Nesting",
    "Newspapering",
    "Noodling",
    "Nucleating",
    "Orbiting",
    "Orchestrating",
    "Osmosing",
    "Perambulating",
    "Percolating",
    "Perusing",
    "Philosophising",
    "Photosynthesizing",
    "Pollinating",
    "Pondering",
    "Pontificating",
    "Pouncing",
    "Precipitating",
    "Prestidigitating",
    "Processing",
    "Proofing",
    "Propagating",
    "Puttering",
    "Puzzling",
    "Quantumizing",
    "Razzle-dazzling",
    "Razzmatazzing",
    "Recombobulating",
    "Reticulating",
    "Roosting",
    "Ruminating",
    "Sautéing",
    "Scampering",
    "Schlepping",
    "Scurrying",
    "Seasoning",
    "Shenaniganing",
    "Shimmying",
    "Simmering",
    "Skedaddling",
    "Sketching",
    "Slithering",
    "Smooshing",
    "Sock-hopping",
    "Spelunking",
    "Spinning",
    "Sprouting",
    "Stewing",
    "Sublimating",
    "Swirling",
    "Swooping",
    "Symbioting",
    "Synthesizing",
    "Tempering",
    "Thinking",
    "Thundering",
    "Tinkering",
    "Tomfoolering",
    "Topsy-turvying",
    "Transfiguring",
    "Transmuting",
    "Twisting",
    "Undulating",
    "Unfurling",
    "Unravelling",
    "Vibing",
    "Waddling",
    "Wandering",
    "Warping",
    "Whatchamacalliting",
    "Whirlpooling",
    "Whirring",
    "Whisking",
    "Wibbling",
    "Working",
    "Wrangling",
    "Zesting",
    "Zigzagging",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpinnerMode {
    Requesting,
    Reasoning,
    LiveResponse,
    ToolUse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpinnerPlatform {
    Macos,
    WindowsLinux,
    Ghostty,
}

#[derive(Debug, Clone, Default)]
struct SpinnerState {
    active: bool,
    started_at_ms: u64,
    verb: String,
    assistant_characters: usize,
    last_output_at_ms: u64,
    reasoning_started_at_ms: Option<u64>,
    thinking_until_ms: Option<u64>,
    thought_until_ms: Option<u64>,
    thought_duration_ms: u64,
    displayed_characters: usize,
    last_advance_at_ms: u64,
}

fn next_screen_namespace() -> u64 {
    NEXT_SCREEN_NAMESPACE.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum AnsiStreamId {
    Assistant(String),
    Reasoning(String),
    ToolOutput(String),
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
    session_id: Option<String>,
    status: StatusState,
    input: InputEditor,
    pending_approvals: VecDeque<ApprovalRequest>,
    picker: Option<PickerKind>,
    picker_window_start: usize,
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
    show_todos: bool,
    todo_refresh_requested: bool,
    context_warning_suppressed: bool,
    observed_compactions: Option<i64>,
    reconnecting: bool,
    blocked: bool,
    show_internals: bool,
    assistant_live_ids: HashSet<String>,
    reasoning_live_ids: HashSet<String>,
    ansi_streams: HashMap<AnsiStreamId, AnsiSanitizer>,
    animation_started_at: Option<Instant>,
    spinner: SpinnerState,
    spinner_turn: u64,
    spinner_override: Option<String>,
    reduced_motion: bool,
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
            animation_started_at: Some(Instant::now()),
            ..Self::default()
        };
        app.screen_namespace = next_screen_namespace();
        app
    }

    pub fn new_with_working_directory(model: Option<String>, working_directory: &Path) -> Self {
        let mut app = Self::new(model);
        app.working_directory = working_directory.to_path_buf();
        app
    }

    pub fn entries(&self) -> &[ChatEntry] {
        &self.entries
    }

    pub fn animation_elapsed_ms(&self) -> u64 {
        self.animation_started_at
            .map(|started_at| started_at.elapsed().as_millis() as u64)
            .unwrap_or_default()
    }

    pub fn set_reduced_motion(&mut self, reduced_motion: bool) {
        self.reduced_motion = reduced_motion;
        if reduced_motion {
            self.spinner.displayed_characters = self.spinner.assistant_characters;
        }
    }

    pub fn reduced_motion(&self) -> bool {
        self.reduced_motion
    }

    pub fn set_spinner_override(&mut self, override_message: Option<String>) {
        self.spinner_override = override_message;
    }

    fn spinner_visible(&self) -> bool {
        self.status.busy && self.spinner.active
    }

    fn spinner_mode(&self) -> SpinnerMode {
        if self.has_active_tool() {
            SpinnerMode::ToolUse
        } else if !self.reasoning_live_ids.is_empty() {
            SpinnerMode::Reasoning
        } else if !self.assistant_live_ids.is_empty() {
            SpinnerMode::LiveResponse
        } else {
            SpinnerMode::Requesting
        }
    }

    fn has_active_tool(&self) -> bool {
        self.entries.iter().any(|entry| {
            matches!(
                entry,
                ChatEntry::Tool {
                    state: ToolCallState::Queued | ToolCallState::Running,
                    ..
                } | ChatEntry::Subagent {
                    status: SubagentStatus::Running,
                    ..
                }
            )
        })
    }

    fn spinner_elapsed_ms(&self, animation_elapsed_ms: u64) -> u64 {
        animation_elapsed_ms.saturating_sub(self.spinner.started_at_ms)
    }

    fn start_spinner_turn(&mut self, prompt: &str) {
        self.spinner_turn = self.spinner_turn.wrapping_add(1);
        let started_at_ms = self.animation_elapsed_ms();
        let verb = self
            .spinner_override
            .as_deref()
            .and_then(normalize_spinner_verb)
            .or_else(|| self.active_todo_spinner_verb())
            .unwrap_or_else(|| builtin_spinner_verb(self.spinner_turn, prompt).to_string());
        self.spinner = SpinnerState {
            active: true,
            started_at_ms,
            verb: format!("{verb}…"),
            last_output_at_ms: started_at_ms,
            last_advance_at_ms: started_at_ms,
            ..SpinnerState::default()
        };
    }

    fn clear_spinner(&mut self) {
        self.spinner = SpinnerState::default();
    }

    fn active_todo_spinner_verb(&self) -> Option<String> {
        let todos = self.todos.as_ref()?;
        todos
            .rows
            .iter()
            .find(|row| is_in_progress_todo_status(&row.status))
            .and_then(|row| {
                normalize_spinner_verb(&row.description)
                    .or_else(|| normalize_spinner_verb(&row.title))
            })
    }

    fn note_assistant_output(&mut self, content: &str) {
        if !self.spinner.active || content.is_empty() {
            return;
        }
        self.spinner.assistant_characters += content.encode_utf16().count();
        self.spinner.last_output_at_ms = self.animation_elapsed_ms();
        if self.reduced_motion {
            self.spinner.displayed_characters = self.spinner.assistant_characters;
        }
    }

    fn note_reasoning_started(&mut self) {
        if self.spinner.active && self.spinner.reasoning_started_at_ms.is_none() {
            self.spinner.reasoning_started_at_ms = Some(self.animation_elapsed_ms());
        }
    }

    fn note_reasoning_finished(&mut self) {
        let Some(started_at_ms) = self.spinner.reasoning_started_at_ms.take() else {
            return;
        };
        let now = self.animation_elapsed_ms();
        let duration = now.saturating_sub(started_at_ms);
        let thinking_until_ms = now.max(started_at_ms.saturating_add(SPINNER_THINKING_MIN_MS));
        self.spinner.thinking_until_ms = Some(thinking_until_ms);
        self.spinner.thought_until_ms = Some(thinking_until_ms + SPINNER_THOUGHT_HOLD_MS);
        self.spinner.thought_duration_ms = duration;
    }

    fn advance_spinner(&mut self, animation_elapsed_ms: u64) {
        if !self.spinner_visible() {
            return;
        }
        if self.spinner_mode() == SpinnerMode::ToolUse {
            self.spinner.last_output_at_ms = animation_elapsed_ms;
        }
        if animation_elapsed_ms <= self.spinner.last_advance_at_ms {
            return;
        }
        let target_characters = self.spinner.assistant_characters;
        if self.reduced_motion {
            self.spinner.displayed_characters = target_characters;
            self.spinner.last_advance_at_ms = animation_elapsed_ms;
            return;
        }

        let ticks =
            ((animation_elapsed_ms - self.spinner.last_advance_at_ms) / SPINNER_TICK_MS).min(128);
        for _ in 0..ticks {
            let gap = target_characters.saturating_sub(self.spinner.displayed_characters);
            if gap == 0 {
                break;
            }
            let increment = if gap < 70 {
                3usize
            } else if gap < 200 {
                ((gap as f64 * 0.15).ceil() as usize).max(8)
            } else {
                50usize
            };
            self.spinner.displayed_characters = self
                .spinner
                .displayed_characters
                .saturating_add(increment)
                .min(target_characters);
        }
        self.spinner.last_advance_at_ms = animation_elapsed_ms;
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
            ChatEntry::LocalOutput(_) => (LiveEntryKind::Other, true),
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
        self.picker.is_some()
    }

    fn todos_visible(&self) -> bool {
        self.show_todos
    }

    pub fn picker_is_open(&self) -> bool {
        self.picker.is_some()
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

    pub fn set_session_id(&mut self, session_id: impl Into<String>) {
        self.session_id = Some(sanitize_plain(&session_id.into()));
    }

    pub fn open_tool_picker(&mut self) {
        self.picker_toolset = self.toolset;
        self.open_picker(PickerKind::Tools);
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
        self.picker_skill_selection = self.skill_selection.clone();
        self.open_picker(PickerKind::Skills);
    }

    fn open_picker(&mut self, picker: PickerKind) {
        if !matches!(picker, PickerKind::Approval) && self.pending_approval().is_some() {
            return;
        }
        self.picker = Some(picker);
        self.picker_window_start = 0;
        self.selected_item = if matches!(picker, PickerKind::Models) {
            self.selected_item
        } else {
            0
        };
        self.completion = None;
    }

    pub fn set_sessions(&mut self, sessions: Vec<SessionMetadata>) {
        self.sessions = sessions;
        self.selected_item = 0;
        self.completion = None;
        self.open_picker(PickerKind::Sessions);
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
        self.open_picker(PickerKind::Models);
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
        self.add_local_command("/usage");
        self.set_usage_snapshot(metrics, context_attribution);
        self.push_entry(ChatEntry::LocalOutput(usage_detail_lines(self)));
    }

    fn set_usage_snapshot(
        &mut self,
        metrics: UsageMetricsSnapshot,
        context_attribution: Option<ContextAttributionSnapshot>,
    ) {
        self.status.usage_metrics = Some(metrics);
        self.observe_context_attribution(context_attribution.as_ref());
        self.status.context_attribution = context_attribution;
    }

    fn observe_context_attribution(&mut self, context: Option<&ContextAttributionSnapshot>) {
        let Some(context) = context else {
            return;
        };
        if self
            .observed_compactions
            .is_some_and(|observed| context.compactions > observed)
        {
            self.context_warning_suppressed = true;
        }
        self.observed_compactions = Some(context.compactions);
    }

    pub fn set_usage_metrics(&mut self, metrics: UsageMetricsSnapshot) {
        self.status.usage_metrics = Some(metrics);
    }

    pub fn set_context_attribution(&mut self, context: Option<ContextAttributionSnapshot>) {
        self.observe_context_attribution(context.as_ref());
        self.status.context_attribution = context;
    }

    pub fn set_reasoning_effort(&mut self, reasoning_effort: Option<String>) {
        self.status.reasoning_effort = reasoning_effort;
    }

    pub fn set_fleet_active(&mut self, active: bool) {
        self.fleet_active = active;
        if active {
            self.todos = None;
            self.show_todos = false;
            self.todo_refresh_requested = false;
        } else {
            self.show_todos = false;
        }
        self.close_picker();
    }

    pub fn set_todos(&mut self, todos: TodoSnapshot) {
        self.todos = Some(todos);
        self.show_todos = true;
        self.todo_refresh_requested = false;
    }

    pub fn take_todo_refresh_request(&mut self) -> bool {
        std::mem::take(&mut self.todo_refresh_requested)
    }

    pub fn set_reconnecting(&mut self, reconnecting: bool) {
        self.reconnecting = reconnecting;
        if reconnecting {
            self.ansi_streams.clear();
        }
    }

    fn sanitize_streamed_ansi(&mut self, stream_id: AnsiStreamId, content: &str) -> String {
        self.ansi_streams
            .entry(stream_id)
            .or_default()
            .push(content)
    }

    fn sanitize_streamed_plain(&mut self, stream_id: AnsiStreamId, content: &str) -> String {
        let sanitized = self.sanitize_streamed_ansi(stream_id, content);
        plain_from_sanitized(&sanitized)
    }

    fn finish_ansi_stream(&mut self, stream_id: AnsiStreamId) {
        if let Some(mut sanitizer) = self.ansi_streams.remove(&stream_id) {
            let _ = sanitizer.finish();
        }
    }

    fn reset_ansi_streams(&mut self) {
        self.ansi_streams.clear();
    }

    pub fn mark_in_flight_tools_unknown(&mut self) {
        for index in 0..self.entries.len() {
            let tool_call_id = match self.entries.get(index) {
                Some(ChatEntry::Tool {
                    tool_call_id,
                    success: None,
                    ..
                }) => Some(tool_call_id.clone()),
                _ => None,
            };
            if let Some(tool_call_id) = tool_call_id {
                self.finish_ansi_stream(AnsiStreamId::ToolOutput(tool_call_id));
                if let ChatEntry::Tool { state, unknown, .. } = &mut self.entries[index] {
                    *state = ToolCallState::Unknown;
                    *unknown = true;
                    self.queue_screen_change(index);
                }
            }
        }
    }

    fn close_picker(&mut self) {
        self.picker = None;
        self.picker_window_start = 0;
        self.selected_item = 0;
        self.reset_picker_options();
    }

    fn add_local_output(&mut self, message: impl Into<String>) {
        let message = sanitize_plain(&message.into());
        self.push_entry(ChatEntry::LocalOutput(vec![Line::from(message)]));
    }

    fn add_local_output_lines(&mut self, lines: Vec<Line<'static>>) {
        self.push_entry(ChatEntry::LocalOutput(lines));
    }

    fn add_local_command(&mut self, command: &str) {
        self.push_entry(ChatEntry::User(sanitize_plain(command)));
    }

    fn cancel_picker(&mut self) {
        let outcome = match self.picker {
            Some(PickerKind::Sessions) => Some("Kept current session".to_string()),
            Some(PickerKind::Models) => Some(format!(
                "Kept model as {}",
                self.status
                    .model
                    .as_deref()
                    .and_then(|model_id| {
                        self.models
                            .iter()
                            .find(|model| model.id == model_id)
                            .map(|model| sanitize_plain(&model.name))
                    })
                    .or_else(|| self.status.model.as_deref().map(sanitize_plain))
                    .unwrap_or_else(|| "auto".to_string())
            )),
            Some(PickerKind::Tools) => Some(format!(
                "Kept tools: {}/{} enabled",
                self.toolset.len(),
                TOOL_COUNT
            )),
            Some(PickerKind::Skills) => Some(format!(
                "Kept skills: {} enabled",
                self.skill_selection.len()
            )),
            Some(PickerKind::Approval) | None => None,
        };
        self.close_picker();
        if let Some(outcome) = outcome {
            self.add_local_output(outcome);
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let item_count = match self.picker {
            Some(PickerKind::Sessions) => self.sessions.len(),
            Some(PickerKind::Models) => self.models.len(),
            Some(PickerKind::Tools) => TOOL_COUNT,
            Some(PickerKind::Skills) => self.skill_catalog.skills().len(),
            Some(PickerKind::Approval) => self.approval_choice_count(),
            None => 0,
        };
        if item_count == 0 {
            return;
        }
        let previous = self.selected_item;
        self.selected_item =
            (self.selected_item as isize + delta).rem_euclid(item_count as isize) as usize;
        self.adjust_picker_window(previous, MAX_PICKER_ROWS);
        if matches!(self.picker, Some(PickerKind::Models)) && self.selected_item != previous {
            self.reset_picker_options();
        }
    }

    fn adjust_picker_window(&mut self, previous: usize, visible_rows: usize) {
        let visible_rows = visible_rows.max(1);
        if self.selected_item < self.picker_window_start {
            self.picker_window_start = self.selected_item;
        } else if self.selected_item >= self.picker_window_start + visible_rows {
            self.picker_window_start = self.selected_item + 1 - visible_rows;
        }
        if self.selected_item < previous && previous == 0 {
            self.picker_window_start = 0;
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
        let action = match self.picker {
            Some(PickerKind::Sessions) => self
                .sessions
                .get(self.selected_item)
                .map(|session| UiAction::Resume(session.session_id.clone())),
            Some(PickerKind::Models) => self.models.get(self.selected_item).map(|model| {
                UiAction::SwitchModel(ModelSelection {
                    model: model.id.clone(),
                    reasoning_effort: self.picker_reasoning_effort.clone(),
                    context_tier: self.picker_context_tier.clone(),
                })
            }),
            Some(PickerKind::Tools) => Some(UiAction::ApplyToolset(self.picker_toolset)),
            Some(PickerKind::Skills) => {
                Some(UiAction::ApplySkills(self.picker_skill_selection.clone()))
            }
            Some(PickerKind::Approval) => return self.approval_action_for_selection(),
            None => None,
        };
        self.close_picker();
        action.unwrap_or(UiAction::None)
    }

    fn toggle_selected_tool(&mut self) {
        if matches!(self.picker, Some(PickerKind::Tools)) {
            let _ = self.picker_toolset.toggle_at(self.selected_item);
        }
    }

    fn choose_shell_only(&mut self) {
        if matches!(self.picker, Some(PickerKind::Tools)) {
            self.picker_toolset = Toolset::shell_only();
        }
    }

    fn choose_all_tools(&mut self) {
        if matches!(self.picker, Some(PickerKind::Tools)) {
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
        if matches!(self.picker, Some(PickerKind::Skills)) {
            self.picker_skill_selection.clear();
        }
    }

    fn choose_all_skills(&mut self) {
        if matches!(self.picker, Some(PickerKind::Skills)) {
            self.picker_skill_selection.select_all(&self.skill_catalog);
        }
    }

    fn approval_choice_count(&self) -> usize {
        if self
            .pending_approval()
            .is_some_and(|request| request.category.supports_trust())
        {
            3
        } else if self.pending_approval().is_some() {
            2
        } else {
            0
        }
    }

    fn approval_action_for_selection(&self) -> UiAction {
        let decision = match self.selected_item {
            0 => ApprovalDecision::ApproveOnce,
            1 => ApprovalDecision::Deny,
            2 if self
                .pending_approval()
                .is_some_and(|request| request.category.supports_trust()) =>
            {
                ApprovalDecision::Trust
            }
            _ => return UiAction::None,
        };
        UiAction::Approval(decision)
    }

    fn toolset_change_is_blocked(&self) -> bool {
        self.blocked || self.reconnecting || self.status.busy || self.pending_approval().is_some()
    }

    fn skill_selection_change_is_blocked(&self) -> bool {
        self.blocked || self.reconnecting || self.status.busy || self.pending_approval().is_some()
    }

    pub fn enqueue_approval(&mut self, request: ApprovalRequest) {
        self.push_entry(ChatEntry::Approval {
            category: sanitize_plain(request.category.label()),
            tool_name: sanitize_plain(&request.tool_name),
            details: sanitize_plain(&request.details),
            status: ApprovalStatus::Pending,
        });
        self.pending_approvals.push_back(request);
        self.open_picker(PickerKind::Approval);
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
        if self.pending_approvals.is_empty() {
            self.close_picker();
        } else {
            self.open_picker(PickerKind::Approval);
        }
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
        self.close_picker();
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
        self.reset_context_warning_state();
        self.status.busy = false;
        self.close_picker();
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
        let content = sanitize_plain(&content);
        self.context_warning_suppressed = false;
        self.start_spinner_turn(&content);
        self.pending_user_messages.push_back(content.clone());
        self.push_entry(ChatEntry::User(content));
        self.status.busy = true;
    }

    fn add_diagnostic(&mut self, message: impl Into<String>) {
        self.push_entry(ChatEntry::Diagnostic(sanitize_plain(&message.into())));
    }

    pub fn replace_history(&mut self, events: &[github_copilot_sdk::types::SessionEvent]) {
        self.reset_screen_lifecycle();
        self.pending_user_messages.clear();
        self.assistant_live_ids.clear();
        self.reasoning_live_ids.clear();
        self.status.busy = false;
        self.status.usage = None;
        self.status.usage_metrics = None;
        self.status.context_attribution = None;
        self.reset_context_warning_state();
        self.blocked = false;
        self.completion = None;
        for event in events {
            if let Some(update) = crate::events::event_update(event) {
                self.apply(update);
            }
        }
    }

    fn reset_screen_lifecycle(&mut self) {
        self.clear_spinner();
        self.entries.clear();
        self.entry_ids.clear();
        self.reset_ansi_streams();
        self.screen_namespace = next_screen_namespace();
        self.next_entry_sequence = 0;
        self.pending_screen_changes.clear();
        self.pending_screen_changes.push_back(ScreenChange::Reset);
    }

    fn reset_context_warning_state(&mut self) {
        self.context_warning_suppressed = false;
        self.observed_compactions = None;
    }

    pub fn apply(&mut self, update: EventUpdate) {
        match update {
            EventUpdate::UserMessage { content } => {
                let content = sanitize_plain(&content);
                if let Some(pending) = self.pending_user_messages.pop_front() {
                    if let Some(index) = self
                        .entries
                        .iter()
                        .rev()
                        .position(
                            |entry| matches!(entry, ChatEntry::User(value) if value == &pending),
                        )
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
                let content = self
                    .sanitize_streamed_plain(AnsiStreamId::Assistant(message_id.clone()), &content);
                self.note_assistant_output(&content);
                self.assistant_live_ids.insert(message_id.clone());
                self.append_assistant(message_id, content, agent_id);
            }
            EventUpdate::AssistantMessage {
                message_id,
                content,
                agent_id,
            } => {
                self.finish_ansi_stream(AnsiStreamId::Assistant(message_id.clone()));
                self.assistant_live_ids.remove(&message_id);
                self.replace_assistant(message_id, sanitize_plain(&content), agent_id);
            }
            EventUpdate::ReasoningDelta {
                reasoning_id,
                content,
                agent_id,
            } => {
                let was_idle = self.reasoning_live_ids.is_empty();
                let content = self.sanitize_streamed_plain(
                    AnsiStreamId::Reasoning(reasoning_id.clone()),
                    &content,
                );
                self.reasoning_live_ids.insert(reasoning_id.clone());
                if was_idle {
                    self.note_reasoning_started();
                }
                self.append_reasoning(reasoning_id, content, agent_id);
            }
            EventUpdate::Reasoning {
                reasoning_id,
                content,
                agent_id,
            } => {
                self.finish_ansi_stream(AnsiStreamId::Reasoning(reasoning_id.clone()));
                self.reasoning_live_ids.remove(&reasoning_id);
                if self.reasoning_live_ids.is_empty() {
                    self.note_reasoning_finished();
                }
                self.replace_reasoning(reasoning_id, sanitize_plain(&content), agent_id);
            }
            EventUpdate::ToolStarted {
                tool_call_id,
                tool_name,
                arguments,
                agent_id,
            } => {
                if self.tool_header_index(&tool_call_id).is_none() {
                    self.finish_ansi_stream(AnsiStreamId::ToolOutput(tool_call_id.clone()));
                    let tool_name = sanitize_plain(&tool_name);
                    let arguments = arguments.map(|mut arguments| {
                        sanitize_json_value(&mut arguments);
                        arguments
                    });
                    let timeout = shell_timeout(arguments.as_ref());
                    let started_at = self.animation_elapsed_ms();
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
                        output: String::new(),
                        status: String::new(),
                        kind: ToolProgressKind::Tool,
                        agent_id,
                        started_at: Some(started_at),
                        timeout,
                    });
                }
            }
            EventUpdate::ToolOutput {
                tool_call_id,
                content,
                agent_id: _,
            } => {
                let Some(index) = self
                    .entries
                    .iter()
                    .position(|entry| matches!(entry, ChatEntry::ToolProgress { tool_call_id: id, .. } if id == &tool_call_id))
                else {
                    self.finish_ansi_stream(AnsiStreamId::ToolOutput(tool_call_id));
                    return;
                };
                let content = self.sanitize_streamed_ansi(
                    AnsiStreamId::ToolOutput(tool_call_id.clone()),
                    &content,
                );
                if let ChatEntry::ToolProgress {
                    output: current, ..
                } = &mut self.entries[index]
                {
                    current.push_str(&content);
                }
                self.queue_screen_change(index);
            }
            EventUpdate::ToolProgress {
                tool_call_id,
                content,
                agent_id: _,
            } => {
                let Some(index) = self
                    .entries
                    .iter()
                    .position(|entry| matches!(entry, ChatEntry::ToolProgress { tool_call_id: id, .. } if id == &tool_call_id))
                else {
                    self.finish_ansi_stream(AnsiStreamId::ToolOutput(tool_call_id));
                    return;
                };
                let content = sanitize_ansi(&content);
                if let ChatEntry::ToolProgress {
                    status: current, ..
                } = &mut self.entries[index]
                {
                    *current = content;
                }
                self.queue_screen_change(index);
            }
            EventUpdate::ToolCompleted {
                tool_call_id,
                success,
                message,
                agent_id: _,
                shell_completion,
            } => self.complete_tool(tool_call_id, success, message, shell_completion, false),
            EventUpdate::ToolCancelled {
                tool_call_id,
                message,
                agent_id: _,
            } => self.complete_tool(tool_call_id, false, message, None, true),
            EventUpdate::SubagentStarted {
                name,
                display_name,
                tool_call_id,
                agent_id,
            } => self.push_entry(ChatEntry::Subagent {
                name,
                tool_call_id,
                display_name: sanitize_plain(&display_name),
                status: SubagentStatus::Running,
                error: None,
                agent_id,
            }),
            EventUpdate::SubagentCompleted {
                name,
                tool_call_id,
                agent_id,
            } => {
                if let Some(index) = self.subagent_index(&name, &tool_call_id, agent_id.as_deref())
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
                if let Some(index) = self.subagent_index(&name, &tool_call_id, agent_id.as_deref())
                {
                    if let ChatEntry::Subagent {
                        status,
                        error: current_error,
                        ..
                    } = &mut self.entries[index]
                    {
                        *status = SubagentStatus::Failed;
                        *current_error = Some(sanitize_plain(&error));
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
                    self.clear_spinner();
                    self.reject_pending_approvals();
                }
                self.push_entry(ChatEntry::Banner {
                    severity,
                    message: sanitize_plain(&message),
                    url: url.map(|url| sanitize_plain(&url)),
                });
            }
            EventUpdate::ModelChanged { model } => self.status.model = Some(model),
            EventUpdate::TodosChanged => {
                if self.fleet_active && self.show_todos {
                    self.todo_refresh_requested = true;
                }
            }
            EventUpdate::Idle | EventUpdate::TaskComplete => {
                self.reset_ansi_streams();
                let completed_indices = self
                    .entries
                    .iter()
                    .enumerate()
                    .filter_map(|(index, entry)| match entry {
                        ChatEntry::Assistant { message_id, .. }
                            if self.assistant_live_ids.contains(message_id) =>
                        {
                            Some(index)
                        }
                        ChatEntry::Reasoning { reasoning_id, .. }
                            if self.reasoning_live_ids.contains(reasoning_id) =>
                        {
                            Some(index)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                self.assistant_live_ids.clear();
                self.reasoning_live_ids.clear();
                self.set_fleet_active(false);
                self.status.busy = false;
                self.clear_spinner();
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
        shell_completion: Option<ShellCompletion>,
        cancelled: bool,
    ) {
        self.finish_ansi_stream(AnsiStreamId::ToolOutput(tool_call_id.clone()));
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

        let shell_completion = shell_completion.map(sanitize_shell_completion);
        let partial_output = self.entries.iter().find_map(|entry| {
            let ChatEntry::ToolProgress {
                tool_call_id: current,
                output,
                ..
            } = entry
            else {
                return None;
            };
            (current == &tool_call_id && !output.is_empty()).then(|| output.clone())
        });
        let success = success
            && shell_completion
                .as_ref()
                .and_then(|completion| completion.exit.as_ref())
                .is_none_or(|exit| exit.exit_code == 0);

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
            content: message
                .map(|message| sanitize_ansi(&message))
                .unwrap_or_default(),
            partial_output,
            shell_completion,
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

    if app.picker.is_some() {
        return handle_picker_key(app, key);
    }

    if key.code == KeyCode::Char('k') && key.modifiers == KeyModifiers::CONTROL {
        if !app.blocked {
            return UiAction::LoadTools;
        }
        return UiAction::None;
    }

    if key.code == KeyCode::Char('s')
        && key.modifiers == KeyModifiers::CONTROL
        && app.picker.is_none()
    {
        if !app.blocked {
            return UiAction::LoadSkills;
        }
        return UiAction::None;
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
            _ => {}
        }
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
            if app.show_todos {
                app.show_todos = false;
                app.todo_refresh_requested = false;
                UiAction::None
            } else {
                UiAction::LoadTodos
            }
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
            } else if input == "/status" {
                UiAction::LoadStatus
            } else if input == "/usage" {
                UiAction::LoadUsageCommand
            } else if matches!(input.split_whitespace().next(), Some("/status" | "/usage")) {
                let command = input.split_whitespace().next().unwrap_or_default();
                UiAction::LocalCommandError(format!(
                    "{command} does not accept arguments. Use {command} without arguments."
                ))
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

fn handle_picker_key(app: &mut App, key: KeyEvent) -> UiAction {
    let picker = app.picker;
    if matches!(picker, Some(PickerKind::Approval)) {
        match key.code {
            KeyCode::Char('y') => return UiAction::Approval(ApprovalDecision::ApproveOnce),
            KeyCode::Char('n') if key.modifiers == KeyModifiers::NONE => {
                return UiAction::Approval(ApprovalDecision::Deny)
            }
            KeyCode::Char('a')
                if app
                    .pending_approval()
                    .is_some_and(|request| request.category.supports_trust()) =>
            {
                return UiAction::Approval(ApprovalDecision::Trust);
            }
            KeyCode::Char('k') if key.modifiers == KeyModifiers::CONTROL => return UiAction::None,
            _ => {}
        }
        if key.code == KeyCode::Esc {
            return UiAction::Approval(ApprovalDecision::Deny);
        }
    }
    match key.code {
        KeyCode::Esc => {
            app.cancel_picker();
            UiAction::None
        }
        KeyCode::Up => {
            app.move_selection(-1);
            UiAction::None
        }
        KeyCode::Char('k') | KeyCode::Char('j') if key.modifiers == KeyModifiers::NONE => {
            app.move_selection(if matches!(key.code, KeyCode::Char('k')) {
                -1
            } else {
                1
            });
            UiAction::None
        }
        KeyCode::Char('p') if key.modifiers == KeyModifiers::CONTROL => {
            app.move_selection(-1);
            UiAction::None
        }
        KeyCode::Down => {
            app.move_selection(1);
            UiAction::None
        }
        KeyCode::Char('n') if key.modifiers == KeyModifiers::CONTROL => {
            app.move_selection(1);
            UiAction::None
        }
        KeyCode::PageUp => {
            app.move_selection(-(MAX_PICKER_ROWS as isize));
            UiAction::None
        }
        KeyCode::PageDown => {
            app.move_selection(MAX_PICKER_ROWS as isize);
            UiAction::None
        }
        KeyCode::Left if matches!(picker, Some(PickerKind::Models)) => {
            app.cycle_picker_option(true);
            UiAction::None
        }
        KeyCode::Right if matches!(picker, Some(PickerKind::Models)) => {
            app.cycle_picker_option(false);
            UiAction::None
        }
        KeyCode::Char(' ') if matches!(picker, Some(PickerKind::Tools)) => {
            app.toggle_selected_tool();
            UiAction::None
        }
        KeyCode::Char(' ') if matches!(picker, Some(PickerKind::Skills)) => {
            app.toggle_selected_skill();
            UiAction::None
        }
        KeyCode::Char('s') if matches!(picker, Some(PickerKind::Tools)) => {
            app.choose_shell_only();
            UiAction::None
        }
        KeyCode::Char('a') if matches!(picker, Some(PickerKind::Tools)) => {
            app.choose_all_tools();
            UiAction::None
        }
        KeyCode::Char('n')
            if matches!(picker, Some(PickerKind::Skills))
                && key.modifiers == KeyModifiers::NONE =>
        {
            app.choose_no_skills();
            UiAction::None
        }
        KeyCode::Char('a')
            if matches!(picker, Some(PickerKind::Skills))
                && key.modifiers == KeyModifiers::NONE =>
        {
            app.choose_all_skills();
            UiAction::None
        }
        KeyCode::Char(character) if ('1'..='9').contains(&character) => {
            let index = character as usize - '1' as usize;
            if index >= picker_item_count(app) {
                return UiAction::None;
            }
            app.selected_item = index;
            app.adjust_picker_window(index, MAX_PICKER_ROWS);
            match picker {
                Some(PickerKind::Tools) | Some(PickerKind::Skills) => {
                    if matches!(picker, Some(PickerKind::Tools)) {
                        app.toggle_selected_tool();
                    } else {
                        app.toggle_selected_skill();
                    }
                    UiAction::None
                }
                Some(PickerKind::Approval) => app.choose_selected(),
                Some(PickerKind::Sessions) | Some(PickerKind::Models) => app.choose_selected(),
                None => UiAction::None,
            }
        }
        KeyCode::Enter => app.choose_selected(),
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
    animation_elapsed_ms: u64,
) {
    draw_frame(frame, app, Some(screen), animation_elapsed_ms);
}

fn draw_frame(
    frame: &mut Frame,
    app: &App,
    screen: Option<&mut ScreenModel>,
    animation_elapsed_ms: u64,
) {
    let prompt_layout = prompt_layout(app, frame.area());
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(prompt_layout.total_height),
        ])
        .split(frame.area());

    if let Some(screen) = screen {
        draw_live_chat(frame, app, screen, layout[0], animation_elapsed_ms);
    } else {
        draw_chat(frame, app, layout[0], animation_elapsed_ms);
    }
    draw_prompt(frame, app, layout[1], prompt_layout);
}

pub async fn run(runtime: AppRuntime, model: Option<String>) -> io::Result<()> {
    run_with_settings(runtime, model, false).await
}

pub async fn run_with_settings(
    runtime: AppRuntime,
    model: Option<String>,
    reduced_motion: bool,
) -> io::Result<()> {
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
    if let Err(error) = terminal.hide_cursor() {
        let _ = restore_terminal(&mut terminal);
        return Err(error);
    }

    let result = run_loop(&mut terminal, runtime, model, reduced_motion).await;
    let restore_result = restore_terminal(&mut terminal);
    result.and(restore_result)
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut runtime: AppRuntime,
    model: Option<String>,
    reduced_motion: bool,
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
    app.set_reduced_motion(reduced_motion);
    app.set_local_model_ids(local_model_ids);
    app.set_toolset(runtime.active_toolset);
    app.set_skill_catalog(runtime.skill_catalog.clone());
    app.set_skill_selection(runtime.active_skill_selection.clone());
    app.set_session_id(runtime.session.id().to_string());
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
    let mut usage_refresh = tokio::time::interval(Duration::from_secs(2));
    usage_refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    while !app.should_quit() {
        for change in app.take_screen_changes() {
            screen_model.apply_change(terminal, change)?;
        }
        let animation_elapsed_ms = app.animation_elapsed_ms();
        app.advance_spinner(animation_elapsed_ms);
        terminal
            .draw(|frame| draw_with_screen(frame, &app, &mut screen_model, animation_elapsed_ms))?;
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
    match runtime
        .session
        .rpc()
        .metadata()
        .get_context_attribution()
        .await
    {
        Ok(result) => app.set_context_attribution(context_attribution_snapshot(&result)),
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
                if app.picker.is_none()
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
                    app.set_session_id(runtime.session.id().to_string());
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
            UiAction::LoadStatus => {
                app.add_local_command("/status");
                app.add_local_output_lines(status_detail_lines(app));
            }
            UiAction::LocalCommandError(message) => {
                app.add_local_output(message);
            }
            UiAction::LoadUsage | UiAction::LoadUsageCommand => {
                app.add_local_command("/usage");
                let metrics = match runtime.session.rpc().usage().get_metrics().await {
                    Ok(metrics) => metrics,
                    Err(error) if error.is_transport_failure() => {
                        recover_connection(app, runtime, events).await?;
                        app.add_local_output(format!("Usage unavailable: {error}"));
                        continue;
                    }
                    Err(error) => {
                        app.add_local_output(format!("Usage unavailable: {error}"));
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
                        app.add_local_output(format!("Usage unavailable: {error}"));
                        continue;
                    }
                    Err(_) => None,
                };
                app.set_usage_snapshot(usage_metrics_snapshot(&metrics), context_attribution);
                app.add_local_output_lines(usage_detail_lines(app));
            }
            UiAction::LoadTodos => {
                load_todos(app, runtime, events).await?;
            }
            UiAction::Resume(session_id) => match runtime.resume(session_id).await {
                Ok(history) => {
                    *events = runtime.session.subscribe();
                    app.replace_history(&history);
                    app.set_session_id(runtime.session.id().to_string());
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
                    app.set_session_id(runtime.session.id().to_string());
                    let displayed_reasoning = displayed_reasoning_effort(
                        &runtime.models,
                        Some(&model),
                        selection.reasoning_effort.as_deref(),
                    );
                    app.set_toolset(runtime.active_toolset);
                    app.set_reasoning_effort(displayed_reasoning);
                    app.apply(crate::events::EventUpdate::ModelChanged {
                        model: model.clone(),
                    });
                    let display_model = runtime
                        .models
                        .iter()
                        .find(|candidate| candidate.id == model)
                        .map(|candidate| sanitize_plain(&candidate.name))
                        .unwrap_or_else(|| sanitize_plain(&model));
                    app.add_local_output(format!("Set model to {display_model}"));
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
                        app.set_session_id(runtime.session.id().to_string());
                        app.set_toolset(runtime.active_toolset);
                        app.add_local_output(format!(
                            "Set tools to {}/{} enabled",
                            app.toolset.len(),
                            TOOL_COUNT
                        ));
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
                        app.set_session_id(runtime.session.id().to_string());
                        app.set_skill_selection(runtime.active_skill_selection.clone());
                        app.add_local_output(format!(
                            "Set skills: {} enabled",
                            app.skill_selection.len()
                        ));
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
                            app.set_session_id(runtime.session.id().to_string());
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
                app.set_session_id(runtime.session.id().to_string());
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
    if !app.take_todo_refresh_request() || !app.fleet_active || !app.todos_visible() {
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

const INPUT_PROMPT: &str = "❯ ";
const INPUT_CONTINUATION: &str = "  ";
const MIN_INPUT_ROWS: usize = 3;
const PROMPT_CHROME_ROWS: u16 = 3;
const MAX_COMPLETION_ROWS: usize = 6;
const MAX_PICKER_ROWS: usize = 5;
const MAX_TODO_ROWS: usize = 5;
const CONTEXT_WARNING_BUFFER_TOKENS: i64 = 20_000;

struct WrappedInput {
    lines: Vec<Line<'static>>,
    cursor_row: usize,
}

struct WrapState {
    lines: Vec<Line<'static>>,
    cursor_row: Option<usize>,
    first_visual_line: bool,
}

#[derive(Debug, Clone, Copy)]
struct PromptLayout {
    todo_rows: u16,
    input_rows: u16,
    footer_rows: u16,
    total_height: u16,
}

fn context_warning_text(app: &App) -> Option<String> {
    if app.context_warning_suppressed {
        return None;
    }
    let usage = app.status.usage.as_ref()?;
    if usage.token_limit <= 0 {
        return None;
    }

    let warning_threshold = usage
        .token_limit
        .saturating_sub(CONTEXT_WARNING_BUFFER_TOKENS);
    if usage.current_tokens < warning_threshold {
        return None;
    }

    let percent = ((usage.token_limit.saturating_sub(usage.current_tokens) as f64
        / usage.token_limit as f64
        * 100.0)
        .round()
        .max(0.0)) as i64;
    Some(format!("{percent}% until auto-compact"))
}

fn prompt_layout(app: &App, area: Rect) -> PromptLayout {
    let prompt_budget = area.height.saturating_sub(1);
    if prompt_budget == 0 {
        return PromptLayout {
            todo_rows: 0,
            input_rows: 0,
            footer_rows: 0,
            total_height: 0,
        };
    }

    if app.picker.is_some() {
        let picker_rows = picker_item_count(app).clamp(1, MAX_PICKER_ROWS) as u16;
        let total_height = (picker_rows + 4).min(prompt_budget);
        return PromptLayout {
            todo_rows: 0,
            input_rows: 0,
            footer_rows: 0,
            total_height,
        };
    }

    let completion_rows = app
        .completion
        .as_ref()
        .filter(|completion| !completion.candidates.is_empty())
        .map(|completion| completion.candidates.len().min(MAX_COMPLETION_ROWS) as u16);
    let requested_footer_rows =
        if app.blocked || app.reconnecting || app.pending_approval().is_some() {
            0
        } else if completion_rows.is_some() {
            completion_rows.unwrap_or_default()
        } else {
            let left_footer_rows = u16::from(app.status.busy || app.input().is_empty());
            let warning_rows =
                if context_warning_text(app).is_some() && area.width < 80 && left_footer_rows > 0 {
                    1
                } else {
                    0
                };
            (left_footer_rows + warning_rows).max(u16::from(context_warning_text(app).is_some()))
        };
    let wrapped_rows = wrap_input(
        app.input(),
        app.input_cursor_byte_offset(),
        area.width as usize,
    )
    .lines
    .len();
    let desired_input_rows = wrapped_rows.max(MIN_INPUT_ROWS) as u16;

    let desired_todo_rows = todo_live_lines(app).len().min(MAX_TODO_ROWS + 1) as u16;
    let todo_budget = prompt_budget.saturating_sub(PROMPT_CHROME_ROWS + MIN_INPUT_ROWS as u16);
    let todo_rows = if app.show_todos {
        desired_todo_rows.min(todo_budget)
    } else {
        0
    };
    let input_and_footer_budget = prompt_budget
        .saturating_sub(PROMPT_CHROME_ROWS)
        .saturating_sub(todo_rows);
    if input_and_footer_budget == 0 {
        return PromptLayout {
            todo_rows,
            input_rows: 0,
            footer_rows: 0,
            total_height: prompt_budget,
        };
    }

    let footer_rows =
        requested_footer_rows.min(input_and_footer_budget.saturating_sub(MIN_INPUT_ROWS as u16));
    let input_budget = input_and_footer_budget - footer_rows;
    let minimum_input_rows = if input_budget >= MIN_INPUT_ROWS as u16 {
        MIN_INPUT_ROWS as u16
    } else {
        1
    };
    let input_rows = desired_input_rows.min(input_budget).max(minimum_input_rows);
    PromptLayout {
        todo_rows,
        input_rows,
        footer_rows,
        total_height: todo_rows + PROMPT_CHROME_ROWS + input_rows + footer_rows,
    }
}

fn wrap_input(text: &str, cursor: usize, width: usize) -> WrappedInput {
    wrap_input_with_busy(text, cursor, width, false)
}

fn wrap_input_with_busy(text: &str, cursor: usize, width: usize, busy: bool) -> WrappedInput {
    let width = width.max(1);
    let cursor = cursor.min(text.len());
    let mut state = WrapState {
        lines: Vec::new(),
        cursor_row: None,
        first_visual_line: true,
    };
    let mut line_start = 0;

    for (line_end, character) in text.char_indices() {
        if character == '\n' {
            wrap_input_line(text, line_start, line_end, cursor, width, busy, &mut state);
            line_start = line_end + character.len_utf8();
        }
    }
    wrap_input_line(
        text,
        line_start,
        text.len(),
        cursor,
        width,
        busy,
        &mut state,
    );

    let cursor_row = state
        .cursor_row
        .unwrap_or_else(|| state.lines.len().saturating_sub(1));
    WrappedInput {
        lines: state.lines,
        cursor_row,
    }
}

fn wrap_input_line(
    text: &str,
    line_start: usize,
    line_end: usize,
    cursor: usize,
    width: usize,
    busy: bool,
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
            state.cursor_row = Some(row);
        }

        let prefix_span = if is_first_line {
            let style = if busy {
                Style::default().add_modifier(Modifier::DIM)
            } else {
                Style::default()
            };
            Span::styled(prefix, style)
        } else {
            Span::raw(prefix)
        };
        let mut spans = vec![prefix_span];
        let cursor_in_segment = if cursor >= segment_start
            && (cursor < segment_end || (cursor == segment_end && segment_end == line_end))
        {
            Some(cursor - segment_start)
        } else {
            None
        };
        spans.extend(input_content_spans(segment, cursor_in_segment));
        state.lines.push(Line::from(spans));
        state.first_visual_line = false;

        if segment_end == line_end {
            break;
        }
        segment_start = segment_end;
    }
}

fn input_content_spans(text: &str, cursor: Option<usize>) -> Vec<Span<'static>> {
    let Some(cursor) = cursor else {
        return vec![Span::raw(text.to_string())];
    };

    let mut spans = vec![Span::raw(text[..cursor].to_string())];
    let remaining = &text[cursor..];
    if let Some(grapheme) = remaining.graphemes(true).next() {
        let grapheme_end = grapheme.len();
        if UnicodeWidthStr::width(grapheme) == 0 {
            spans.push(Span::styled(
                " ",
                Style::default().add_modifier(Modifier::REVERSED),
            ));
            spans.push(Span::raw(remaining.to_string()));
        } else {
            spans.push(Span::styled(
                grapheme.to_string(),
                Style::default().add_modifier(Modifier::REVERSED),
            ));
            spans.push(Span::raw(remaining[grapheme_end..].to_string()));
        }
    } else {
        spans.push(Span::styled(
            " ",
            Style::default().add_modifier(Modifier::REVERSED),
        ));
    }
    spans
}

fn wrapped_segment_end(text: &str, width: usize) -> usize {
    let mut display_width: usize = 0;
    let mut end = 0;
    for grapheme in text.graphemes(true) {
        let grapheme_width = input_grapheme_width(grapheme);
        if end != 0 && display_width.saturating_add(grapheme_width) > width {
            break;
        }
        end += grapheme.len();
        display_width = display_width.saturating_add(grapheme_width);
        if display_width >= width {
            break;
        }
    }
    end
}

fn display_width(text: &str) -> usize {
    text.graphemes(true).map(input_grapheme_width).sum()
}

fn input_grapheme_width(grapheme: &str) -> usize {
    if grapheme == "\t" {
        4
    } else {
        UnicodeWidthStr::width(grapheme)
    }
}

fn cursor_scroll_start(cursor_row: usize, total_rows: usize, visible_rows: usize) -> usize {
    let visible_rows = visible_rows.max(1);
    cursor_row
        .saturating_sub(visible_rows / 2)
        .min(total_rows.saturating_sub(visible_rows))
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

    let wrapped = wrap_input_with_busy(
        app.input(),
        app.input_cursor_byte_offset(),
        area.width as usize,
        app.status.busy,
    );
    let visible_lines = area.height.saturating_sub(2).max(1) as usize;
    let scroll = cursor_scroll_start(wrapped.cursor_row, wrapped.lines.len(), visible_lines);

    Paragraph::new(wrapped.lines)
        .block(
            Block::default()
                .borders(Borders::TOP | Borders::BOTTOM)
                .border_style(Style::default().fg(palette::PROMPT_BORDER)),
        )
        .scroll((scroll.min(u16::MAX as usize) as u16, 0))
}

fn draw_prompt(frame: &mut Frame, app: &App, area: Rect, layout: PromptLayout) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if app.picker.is_some() {
        draw_inline_picker(frame, app, area);
        return;
    }

    if layout.todo_rows > 0 {
        let todo_area = Rect::new(area.x, area.y, area.width, layout.todo_rows);
        frame.render_widget(Paragraph::new(todo_live_lines(app)), todo_area);
    }
    let prompt_area = Rect::new(
        area.x,
        area.y.saturating_add(layout.todo_rows),
        area.width,
        area.height.saturating_sub(layout.todo_rows),
    );

    let input_y = prompt_area.y.saturating_add(1).min(prompt_area.bottom());
    let input_height = layout
        .input_rows
        .saturating_add(2)
        .min(prompt_area.bottom().saturating_sub(input_y));
    if input_height == 0 {
        return;
    }
    let input_area = Rect::new(area.x, input_y, area.width, input_height);
    frame.render_widget(input_box(app, input_area), input_area);

    let footer_y = input_area.bottom();
    let footer_height = layout
        .footer_rows
        .min(prompt_area.bottom().saturating_sub(footer_y));
    if footer_height == 0 {
        return;
    }
    let footer_area = Rect::new(area.x, footer_y, area.width, footer_height);
    if app
        .completion
        .as_ref()
        .is_some_and(|completion| !completion.candidates.is_empty())
    {
        draw_completion(frame, app, footer_area);
    } else {
        frame.render_widget(prompt_footer(app, footer_area), footer_area);
    }
}

fn todo_live_lines(app: &App) -> Vec<Line<'static>> {
    let Some(todos) = app.todos.as_ref() else {
        return vec![Line::from(Span::styled(
            "Fleet todos unavailable.",
            Style::default().fg(palette::INACTIVE),
        ))];
    };

    let rows = todos.rows.iter().collect::<Vec<_>>();
    if rows.is_empty() {
        return vec![Line::from(Span::styled(
            "Fleet todos: no pending work.",
            Style::default().fg(palette::INACTIVE),
        ))];
    }

    let mut lines = vec![Line::from(Span::styled(
        "Fleet todos",
        Style::default()
            .fg(palette::TEXT)
            .add_modifier(Modifier::BOLD),
    ))];
    let visible_rows = if rows.len() > MAX_TODO_ROWS {
        MAX_TODO_ROWS.saturating_sub(1)
    } else {
        MAX_TODO_ROWS
    };
    for row in rows.iter().take(visible_rows) {
        let (marker, style) = todo_status_marker(&row.status);
        lines.push(Line::from(vec![
            Span::styled(format!("  {marker} "), style),
            Span::styled(
                sanitize_plain(&row.title),
                Style::default().fg(palette::TEXT),
            ),
        ]));
    }

    if rows.len() > visible_rows {
        let hidden = &rows[visible_rows..];
        let pending = hidden
            .iter()
            .filter(|row| {
                !todo_status_is_finished(&row.status) && !todo_status_is_in_progress(&row.status)
            })
            .count();
        let in_progress = hidden
            .iter()
            .filter(|row| todo_status_is_in_progress(&row.status))
            .count();
        if pending > 0 || in_progress > 0 {
            lines.push(Line::from(Span::styled(
                format!(" … +{pending} pending, {in_progress} in progress"),
                Style::default().fg(palette::INACTIVE),
            )));
        }
    }
    lines
}

fn todo_status_is_finished(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "completed" | "complete" | "done" | "cancelled" | "canceled"
    )
}

fn todo_status_is_in_progress(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "in_progress" | "in-progress" | "in progress" | "working"
    )
}

fn todo_status_marker(status: &str) -> (&'static str, Style) {
    if todo_status_is_finished(status) {
        ("✓", Style::default().fg(palette::SUCCESS))
    } else if todo_status_is_in_progress(status) {
        ("◼", Style::default().fg(palette::SUGGESTION))
    } else {
        ("◻", Style::default().fg(palette::INACTIVE))
    }
}

fn picker_item_count(app: &App) -> usize {
    match app.picker {
        Some(PickerKind::Sessions) => app.sessions.len(),
        Some(PickerKind::Models) => app.models.len(),
        Some(PickerKind::Tools) => TOOL_COUNT,
        Some(PickerKind::Skills) => app.skill_catalog.skills().len(),
        Some(PickerKind::Approval) => app.approval_choice_count(),
        None => 0,
    }
}

fn draw_inline_picker(frame: &mut Frame, app: &App, area: Rect) {
    let Some(picker) = app.picker else {
        return;
    };
    let item_count = picker_item_count(app);
    let position = if item_count == 0 {
        0
    } else {
        app.selected_item.saturating_add(1).min(item_count)
    };
    let title = match picker {
        PickerKind::Sessions => format!(
            "Select a session to resume ({} of {}):",
            position, item_count
        ),
        PickerKind::Models => format!("Select a model ({} of {}):", position, item_count),
        PickerKind::Tools if item_count > MAX_PICKER_ROWS => {
            format!("Select tools ({} of {}):", position, item_count)
        }
        PickerKind::Tools => "Select tools:".to_string(),
        PickerKind::Skills if item_count > MAX_PICKER_ROWS => {
            format!("Select skills ({} of {}):", position, item_count)
        }
        PickerKind::Skills => "Select skills:".to_string(),
        PickerKind::Approval => {
            let request = app.pending_approval();
            let category = request
                .map(|request| sanitize_plain(request.category.label()))
                .unwrap_or_else(|| "Tool".to_string());
            let tool_name = request
                .map(|request| sanitize_plain(&request.tool_name))
                .unwrap_or_else(|| "request".to_string());
            format!("{category} approval required for {tool_name}")
        }
    };

    let mut lines = vec![Line::from(Span::styled(
        title,
        Style::default()
            .fg(palette::TEXT)
            .add_modifier(Modifier::BOLD),
    ))];
    if matches!(picker, PickerKind::Sessions) {
        let prefix_width = 6 + item_count.max(1).to_string().len();
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}Updated", " ".repeat(prefix_width)),
                Style::default().fg(palette::INACTIVE),
            ),
            Span::raw("                   "),
            Span::styled("Session Title", Style::default().fg(palette::INACTIVE)),
        ]));
    }
    if let PickerKind::Approval = picker {
        if let Some(request) = app.pending_approval() {
            lines.push(Line::from(Span::styled(
                format!("  ⎿  {}", sanitize_plain(&request.details)),
                Style::default().fg(palette::INACTIVE),
            )));
            lines.push(Line::from(Span::styled(
                "  Allow this tool call?",
                Style::default().fg(palette::TEXT),
            )));
        }
    }

    let visible_count = item_count.clamp(1, MAX_PICKER_ROWS);
    let first_visible = app
        .picker_window_start
        .min(item_count.saturating_sub(visible_count));
    let last_visible = (first_visible + visible_count).min(item_count);
    let index_width = item_count.max(1).to_string().len();
    for index in first_visible..last_visible {
        let indicator = if index == app.selected_item {
            "❯"
        } else if index == first_visible && first_visible > 0 {
            "↑"
        } else if index + 1 == last_visible && last_visible < item_count {
            "↓"
        } else {
            " "
        };
        let indicator_style = if indicator == "❯" {
            Style::default().fg(palette::SUGGESTION)
        } else {
            Style::default().fg(palette::INACTIVE)
        };
        let label_style = if index == app.selected_item {
            Style::default().fg(palette::SUGGESTION)
        } else {
            Style::default().fg(palette::TEXT)
        };
        let index_prefix = format!("{:>index_width$}.", index + 1);
        let mut spans = vec![
            Span::styled(indicator, indicator_style),
            Span::raw("  "),
            Span::styled(index_prefix, Style::default().fg(palette::INACTIVE)),
            Span::raw("  "),
        ];
        match picker {
            PickerKind::Sessions => {
                let session = &app.sessions[index];
                let updated = truncate_tail(&sanitize_plain(&session.modified_time), 24);
                let summary = session
                    .summary
                    .as_deref()
                    .map(sanitize_plain)
                    .unwrap_or_else(|| "untitled session".to_string());
                spans.push(Span::styled(format!("{updated:<24}"), label_style));
                spans.push(Span::raw("  "));
                spans.push(Span::styled(summary, label_style));
            }
            PickerKind::Models => {
                let model = &app.models[index];
                spans.push(Span::styled(
                    model_picker_row_for(model, app.is_local_model(&model.id)),
                    label_style,
                ));
                if app.status.model.as_deref() == Some(model.id.as_str()) {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled("✓", Style::default().fg(palette::SUCCESS)));
                }
            }
            PickerKind::Tools => {
                let selected = app.picker_toolset.contains_at(index);
                spans.push(Span::styled(
                    if selected { "[✓]" } else { "[ ]" },
                    if selected {
                        Style::default().fg(palette::SUCCESS)
                    } else {
                        Style::default().fg(palette::TEXT)
                    },
                ));
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    CANONICAL_TOOLS.get(index).copied().unwrap_or_default(),
                    label_style,
                ));
            }
            PickerKind::Skills => {
                let skill = &app.skill_catalog.skills()[index];
                let selected = app.picker_skill_selection.contains(&skill.name);
                spans.push(Span::styled(
                    if selected { "[✓]" } else { "[ ]" },
                    if selected {
                        Style::default().fg(palette::SUCCESS)
                    } else {
                        Style::default().fg(palette::TEXT)
                    },
                ));
                spans.push(Span::raw("  "));
                spans.push(Span::styled(sanitize_plain(&skill.name), label_style));
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    sanitize_plain(&skill.description),
                    Style::default().fg(palette::INACTIVE),
                ));
            }
            PickerKind::Approval => {
                let label = match index {
                    0 => "Allow once",
                    1 => "Deny",
                    2 => "Trust for this session",
                    _ => "",
                };
                spans.push(Span::styled(label, label_style));
            }
        }
        lines.push(Line::from(spans));
    }

    if item_count == 0 {
        lines.push(Line::from(Span::styled(
            "  No entries available.",
            Style::default().fg(palette::INACTIVE),
        )));
    }
    if area.height as usize > lines.len() {
        lines.push(Line::from(Span::styled(
            "  ↑/↓ to select · Enter to confirm · Esc to cancel",
            Style::default().fg(palette::INACTIVE),
        )));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn prompt_footer(app: &App, area: Rect) -> Paragraph<'static> {
    let left = if app.status.busy {
        Some("  esc to interrupt".to_string())
    } else if app.input().is_empty() {
        Some("  ? for shortcuts".to_string())
    } else {
        None
    };
    let has_left = left.is_some();
    let dim = Style::default().add_modifier(Modifier::DIM);
    let warning = context_warning_text(app);

    if area.width < 80 {
        let mut lines = Vec::new();
        if let Some(left) = left {
            lines.push(Line::from(Span::styled(
                truncate_tail(&left, area.width as usize),
                dim,
            )));
        }
        if let Some(warning) = warning.filter(|_| area.height >= 2 || !has_left) {
            lines.push(Line::from(Span::styled(
                truncate_tail(&format!("  {warning}"), area.width as usize),
                dim,
            )));
        }
        return Paragraph::new(lines);
    }

    let right_padding = 2usize.min(area.width as usize);
    let warning = warning.map(|warning| {
        truncate_tail(
            &warning,
            (area.width as usize).saturating_sub(right_padding),
        )
    });
    let warning_width = warning.as_deref().map(display_width).unwrap_or_default();
    let left = left
        .map(|left| {
            truncate_tail(
                &left,
                (area.width as usize)
                    .saturating_sub(warning_width)
                    .saturating_sub(right_padding),
            )
        })
        .unwrap_or_default();
    let left_width = display_width(&left);
    let padding = (area.width as usize)
        .saturating_sub(left_width)
        .saturating_sub(warning_width)
        .saturating_sub(right_padding);
    let mut spans = vec![Span::styled(left, dim), Span::raw(" ".repeat(padding))];
    if let Some(warning) = warning {
        spans.push(Span::styled(warning, dim));
    }
    spans.push(Span::raw(" ".repeat(right_padding)));
    Paragraph::new(Line::from(spans))
}

fn draw_completion(frame: &mut Frame, app: &App, footer_area: Rect) {
    let Some(completion) = app.completion.as_ref() else {
        return;
    };
    if completion.candidates.is_empty() || footer_area.width == 0 || footer_area.height == 0 {
        return;
    }

    let visible_count = completion
        .candidates
        .len()
        .min(MAX_COMPLETION_ROWS)
        .min(footer_area.height as usize);
    if visible_count == 0 {
        return;
    }
    let first_visible = completion
        .selected_item
        .saturating_sub(visible_count / 2)
        .min(completion.candidates.len().saturating_sub(visible_count));
    let available_width = footer_area.width as usize;
    let command_column_width = (available_width.saturating_sub(2).saturating_mul(40) / 100).max(1);
    let lines = completion
        .candidates
        .iter()
        .enumerate()
        .skip(first_visible)
        .take(visible_count)
        .map(|(index, candidate)| {
            let command = truncate_tail(&sanitize_plain(&candidate.command), command_column_width);
            let command_padding = command_column_width.saturating_sub(display_width(&command));
            let mut row = format!("  {command}{}", " ".repeat(command_padding));
            let description = collapsed_whitespace(&sanitize_plain(&candidate.description));
            let separator_width = usize::from(!description.is_empty());
            let description_width = available_width
                .saturating_sub(display_width(&row))
                .saturating_sub(separator_width);
            if !description.is_empty() && description_width > 0 {
                row.push(' ');
                row.push_str(&truncate_tail(&description, description_width));
            }
            let style = if index == completion.selected_item {
                Style::default().fg(palette::SUGGESTION)
            } else {
                Style::default().fg(palette::INACTIVE)
            };
            Line::from(Span::styled(row, style))
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), footer_area);
}

fn collapsed_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_tail(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if display_width(text) <= width {
        return text.to_string();
    }
    if width == 1 {
        return "…".to_string();
    }

    let content_width = width - 1;
    let mut result = String::new();
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        let grapheme_width = UnicodeWidthStr::width(grapheme);
        if used + grapheme_width > content_width {
            break;
        }
        result.push_str(grapheme);
        used += grapheme_width;
    }
    result.push('…');
    result
}

fn model_picker_row_for(model: &Model, is_local: bool) -> String {
    format!(
        "{:<28}  {:<9}  {} tokens",
        sanitize_plain(&model.name),
        model_cost_label_for(model, is_local),
        model_context_label(model)
    )
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

fn status_detail_lines(app: &App) -> Vec<Line<'static>> {
    let session_id = app.session_id.as_deref().unwrap_or("unknown");
    let working_directory = app.working_directory.display().to_string();
    let model = app
        .status
        .model
        .as_deref()
        .map(sanitize_plain)
        .unwrap_or_else(|| "auto".to_string());
    let enabled_tools = app.toolset.len();
    let disabled_tools = TOOL_COUNT.saturating_sub(enabled_tools);
    let enabled_skills = app.skill_selection.len();
    let disabled_skills = app
        .skill_catalog
        .skills()
        .len()
        .saturating_sub(enabled_skills);

    vec![
        status_property_line("Version", env!("CARGO_PKG_VERSION")),
        status_property_line("Session ID", session_id),
        status_property_line("cwd", &working_directory),
        Line::default(),
        status_property_line("Model", &model),
        status_count_line("Tools", enabled_tools, disabled_tools, "/tools"),
        status_count_line("Skills", enabled_skills, disabled_skills, "/skills"),
    ]
}

fn status_property_line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label}:"),
            Style::default()
                .fg(palette::TEXT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(sanitize_plain(value), Style::default().fg(palette::TEXT)),
    ])
}

fn status_count_line(label: &str, enabled: usize, disabled: usize, command: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label}:"),
            Style::default()
                .fg(palette::TEXT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!("{enabled} enabled"),
            Style::default().fg(palette::SUCCESS),
        ),
        Span::raw(", "),
        Span::styled(
            format!("{disabled} disabled"),
            Style::default().fg(palette::INACTIVE),
        ),
        Span::styled(
            format!(" · {command}"),
            Style::default()
                .fg(palette::INACTIVE)
                .add_modifier(Modifier::DIM),
        ),
    ])
}

fn usage_detail_lines(app: &App) -> Vec<Line<'static>> {
    let Some(metrics) = app.status.usage_metrics.as_ref() else {
        return vec![usage_line("Usage metrics unavailable.")];
    };

    let mut lines = vec![
        usage_line(format!("Session cost: {}", format_cost(metrics))),
        usage_line(format!(
            "Premium request cost: {:.2}",
            metrics.total_premium_request_cost
        )),
        usage_line(format!("Requests: {}", metrics.total_user_requests)),
        usage_line(format!("API time: {} ms", metrics.total_api_duration_ms)),
    ];

    if let Some(usage) = app.status.usage.as_ref() {
        lines.push(usage_line(format!(
            "Context window: {} / {} tokens",
            format_count(usage.current_tokens),
            format_count(usage.token_limit)
        )));
    }

    if let Some(context) = app.status.context_attribution.as_ref() {
        lines.push(usage_line(format!(
            "Attribution: {} / {} tokens ({})",
            format_count(context.total_tokens),
            format_count(context.prompt_token_limit),
            sanitize_plain(&context.model_id)
        )));
        for category in &context.categories {
            let percentage = if context.total_tokens > 0 {
                category.tokens as f64 / context.total_tokens as f64 * 100.0
            } else {
                0.0
            };
            lines.push(usage_line(format!(
                "  {}: {} ({percentage:.1}%)",
                sanitize_plain(&category.label),
                format_count(category.tokens)
            )));
        }
        lines.push(usage_line(format!("Compactions: {}", context.compactions)));
    } else {
        lines.push(usage_line("Context attribution unavailable."));
    }

    lines
}

fn usage_line(text: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(
        sanitize_plain(&text.into()),
        Style::default()
            .fg(palette::INACTIVE)
            .add_modifier(Modifier::DIM),
    ))
}

fn format_cost(metrics: &UsageMetricsSnapshot) -> String {
    match metrics.total_nano_aiu {
        Some(cost) => format!("{:.3} AIU", cost / 1_000_000_000.0),
        None => format!("{:.1} premium", metrics.total_premium_request_cost),
    }
}

fn draw_live_chat(
    frame: &mut Frame,
    app: &App,
    screen: &mut ScreenModel,
    area: Rect,
    animation_elapsed_ms: u64,
) {
    let spinner_visible = app.spinner_visible();
    let transcript_height = if spinner_visible {
        area.height.saturating_sub(2) as usize
    } else {
        area.height as usize
    };
    let mut lines = screen.visible_live_lines_at_width_with_clock(
        Platform::current(),
        area.width as usize,
        transcript_height,
        animation_elapsed_ms,
    );
    if spinner_visible {
        if area.height >= 2 {
            lines.push(Line::default());
        }
        if area.height >= 1 {
            lines.extend(spinner_lines_at_width(
                app,
                area.width as usize,
                animation_elapsed_ms,
            ));
        }
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_chat(frame: &mut Frame, app: &App, area: Rect, animation_elapsed_ms: u64) {
    let lines = chat_lines_at_width_with_clock(app, area.width as usize, animation_elapsed_ms);
    let scroll = lines
        .len()
        .saturating_sub(area.height as usize)
        .min(u16::MAX as usize) as u16;
    let paragraph = Paragraph::new(lines).scroll((scroll, 0));
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
fn chat_lines(app: &App) -> Vec<Line<'static>> {
    chat_lines_at_width_with_clock(app, 80, app.animation_elapsed_ms())
}

#[cfg(test)]
fn chat_lines_at_width(app: &App, width: usize) -> Vec<Line<'static>> {
    chat_lines_at_width_with_clock(app, width, app.animation_elapsed_ms())
}

fn chat_lines_at_width_with_clock(
    app: &App,
    width: usize,
    animation_elapsed_ms: u64,
) -> Vec<Line<'static>> {
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
    if app.spinner_visible() {
        lines.push(Line::default());
        lines.extend(spinner_lines_at_width(app, width, animation_elapsed_ms));
    }
    lines
}

fn normalize_spinner_verb(value: &str) -> Option<String> {
    let sanitized = sanitize_plain(value);
    let value = sanitized.trim().trim_end_matches('…').trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn is_in_progress_todo_status(status: &str) -> bool {
    matches!(
        status
            .trim()
            .to_ascii_lowercase()
            .replace(['-', ' '], "_")
            .as_str(),
        "in_progress" | "inprogress" | "active" | "doing"
    )
}

fn builtin_spinner_verb(turn: u64, prompt: &str) -> &'static str {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    turn.hash(&mut hasher);
    prompt.hash(&mut hasher);
    let index = (hasher.finish() as usize) % BUILTIN_SPINNER_VERBS.len();
    BUILTIN_SPINNER_VERBS[index]
}

fn spinner_platform_for(term: Option<&str>, is_macos: bool) -> SpinnerPlatform {
    if term == Some("xterm-ghostty") {
        SpinnerPlatform::Ghostty
    } else if is_macos {
        SpinnerPlatform::Macos
    } else {
        SpinnerPlatform::WindowsLinux
    }
}

fn current_spinner_platform() -> SpinnerPlatform {
    spinner_platform_for(
        std::env::var("TERM").ok().as_deref(),
        cfg!(target_os = "macos"),
    )
}

fn spinner_frames(platform: SpinnerPlatform) -> [&'static str; 12] {
    let base = match platform {
        SpinnerPlatform::Macos => ["·", "✢", "✳", "✶", "✻", "✽"],
        SpinnerPlatform::WindowsLinux => ["·", "✢", "*", "✶", "✻", "✽"],
        SpinnerPlatform::Ghostty => ["·", "✢", "✳", "✶", "✻", "*"],
    };
    let mut frames = [""; 12];
    for (index, frame) in base.iter().enumerate() {
        frames[index] = frame;
    }
    for (index, frame) in base.iter().rev().enumerate() {
        frames[base.len() + index] = frame;
    }
    frames
}

fn spinner_lines_at_width(
    app: &App,
    width: usize,
    animation_elapsed_ms: u64,
) -> Vec<Line<'static>> {
    vec![render_spinner_line(app, width, animation_elapsed_ms)]
}

fn render_spinner_line(app: &App, width: usize, animation_elapsed_ms: u64) -> Line<'static> {
    render_spinner_line_for_platform(app, width, animation_elapsed_ms, current_spinner_platform())
}

fn render_spinner_line_for_platform(
    app: &App,
    width: usize,
    animation_elapsed_ms: u64,
    platform: SpinnerPlatform,
) -> Line<'static> {
    let elapsed_ms = app.spinner_elapsed_ms(animation_elapsed_ms);
    let mode = app.spinner_mode();
    let stall_intensity = spinner_stall_intensity(app, animation_elapsed_ms, mode);
    let flash_opacity = if app.reduced_motion || mode != SpinnerMode::ToolUse {
        0.0
    } else {
        ((elapsed_ms as f64 / 1_000.0 * std::f64::consts::PI).sin() + 1.0) / 2.0
    };
    let message_color = if stall_intensity > 0.0 {
        interpolate_rgb(palette::CLAUDE, SPINNER_ERROR_COLOR, stall_intensity)
    } else if mode == SpinnerMode::ToolUse {
        interpolate_rgb(
            palette::CLAUDE,
            color_components(palette::CLAUDE_SHIMMER),
            flash_opacity,
        )
    } else {
        palette::CLAUDE
    };
    let shimmer_color = if stall_intensity > 0.0 {
        message_color
    } else {
        palette::CLAUDE_SHIMMER
    };
    let glyph = if app.reduced_motion {
        "●"
    } else {
        let frames = spinner_frames(platform);
        frames[((elapsed_ms / SPINNER_FRAME_MS) as usize) % frames.len()]
    };
    let message = &app.spinner.verb;
    let message_width = UnicodeWidthStr::width(message.as_str());
    let prefix_width = 2usize.saturating_add(message_width).saturating_add(1);
    let mut spans = vec![Span::styled(
        format!("{glyph} "),
        Style::default().fg(message_color),
    )];
    spans.extend(spinner_message_spans(
        message,
        message_color,
        shimmer_color,
        mode,
        elapsed_ms,
        stall_intensity,
        app.reduced_motion,
    ));
    let status_parts = spinner_status_parts(app, mode, animation_elapsed_ms, width, prefix_width);
    if !status_parts.is_empty() {
        let status_text = status_parts
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<Vec<_>>()
            .join(" · ");
        let thinking_only = status_parts.len() == 1 && status_parts[0].1;
        let style = if thinking_only {
            thinking_status_style(app, elapsed_ms)
        } else {
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM)
        };
        spans.push(Span::styled(format!(" ({status_text})"), style));
    }
    clip_spinner_line(Line::from(spans), width)
}

fn spinner_message_spans(
    message: &str,
    message_color: Color,
    shimmer_color: Color,
    mode: SpinnerMode,
    elapsed_ms: u64,
    stall_intensity: f64,
    reduced_motion: bool,
) -> Vec<Span<'static>> {
    let base_style = Style::default().fg(message_color);
    if reduced_motion || stall_intensity > 0.0 || mode == SpinnerMode::ToolUse {
        return vec![Span::styled(format!("{message} "), base_style)];
    }

    let message_width = UnicodeWidthStr::width(message);
    let speed_ms = if mode == SpinnerMode::Requesting {
        50
    } else {
        200
    };
    let cycle_length = message_width.saturating_add(20).max(1);
    let cycle_position = (elapsed_ms / speed_ms) as usize % cycle_length;
    let glimmer_index = if mode == SpinnerMode::Requesting {
        cycle_position as isize - 10
    } else {
        message_width as isize + 10 - cycle_position as isize
    };
    let band_start = glimmer_index - 1;
    let band_end = glimmer_index + 2;
    let shimmer_style = Style::default().fg(shimmer_color);
    let mut spans = Vec::new();
    let mut column = 0isize;
    for grapheme in message.graphemes(true) {
        let grapheme_width = UnicodeWidthStr::width(grapheme) as isize;
        let style = if column < band_end && column + grapheme_width > band_start {
            shimmer_style
        } else {
            base_style
        };
        spans.push(Span::styled(grapheme.to_string(), style));
        column += grapheme_width;
    }
    spans.push(Span::styled(" ", base_style));
    spans
}

fn spinner_status_parts(
    app: &App,
    mode: SpinnerMode,
    animation_elapsed_ms: u64,
    width: usize,
    prefix_width: usize,
) -> Vec<(String, bool)> {
    let elapsed_ms = app.spinner_elapsed_ms(animation_elapsed_ms);
    let mut parts = Vec::new();
    if let Some(thinking) = thinking_status(app, animation_elapsed_ms) {
        let with_effort = thinking.0;
        let bare = "thinking".to_string();
        if spinner_status_fits(prefix_width, width, std::slice::from_ref(&with_effort)) {
            parts.push((with_effort, true));
        } else if spinner_status_fits(prefix_width, width, std::slice::from_ref(&bare)) {
            parts.push((bare, true));
        }
    }

    let wants_timer_and_tokens = app.show_internals || elapsed_ms > SPINNER_STATUS_AFTER_MS;
    if wants_timer_and_tokens {
        let timer = format_elapsed(elapsed_ms);
        if spinner_status_fits(
            prefix_width,
            width,
            &parts
                .iter()
                .map(|(text, _)| text.clone())
                .chain(std::iter::once(timer.clone()))
                .collect::<Vec<_>>(),
        ) {
            parts.push((timer, false));
        }
        let tokens = ((app.spinner.displayed_characters + 2) / 4) as u64;
        if tokens > 0 {
            let token_text = format!(
                "{} {} tokens",
                if mode == SpinnerMode::Requesting {
                    "↑"
                } else {
                    "↓"
                },
                format_spinner_tokens(tokens)
            );
            let candidate = parts
                .iter()
                .map(|(text, _)| text.clone())
                .chain(std::iter::once(token_text.clone()))
                .collect::<Vec<_>>();
            if spinner_status_fits(prefix_width, width, &candidate) {
                parts.push((token_text, false));
            }
        }
    }
    parts
}

fn spinner_status_fits(prefix_width: usize, width: usize, parts: &[String]) -> bool {
    if parts.is_empty() {
        return false;
    }
    let status_width = 2usize.saturating_add(UnicodeWidthStr::width(parts.join(" · ").as_str()));
    prefix_width.saturating_add(status_width) <= width
}

fn thinking_status(app: &App, animation_elapsed_ms: u64) -> Option<(String, bool)> {
    if !app.reasoning_live_ids.is_empty() {
        let text = app
            .status
            .reasoning_effort
            .as_deref()
            .filter(|effort| !effort.trim().is_empty())
            .map(|effort| format!("thinking with {} effort", sanitize_plain(effort)))
            .unwrap_or_else(|| "thinking".to_string());
        return Some((text, true));
    }

    let elapsed_ms = app.spinner_elapsed_ms(animation_elapsed_ms);
    let absolute_ms = app.spinner.started_at_ms.saturating_add(elapsed_ms);
    if let Some(thinking_until_ms) = app.spinner.thinking_until_ms {
        if absolute_ms < thinking_until_ms {
            return Some(("thinking".to_string(), true));
        }
        if app
            .spinner
            .thought_until_ms
            .is_some_and(|until| absolute_ms < until)
        {
            let seconds =
                ((app.spinner.thought_duration_ms as f64 / 1_000.0).round() as u64).max(1);
            return Some((format!("thought for {seconds}s"), false));
        }
    }
    None
}

fn thinking_status_style(app: &App, elapsed_ms: u64) -> Style {
    if app.reduced_motion {
        return Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM);
    }
    let opacity = ((elapsed_ms as f64 / 1_000.0 * std::f64::consts::PI).sin() + 1.0) / 2.0;
    Style::default().fg(interpolate_rgb(
        Color::Rgb(
            SPINNER_THINKING_INACTIVE.0,
            SPINNER_THINKING_INACTIVE.1,
            SPINNER_THINKING_INACTIVE.2,
        ),
        (
            SPINNER_THINKING_SHIMMER.0,
            SPINNER_THINKING_SHIMMER.1,
            SPINNER_THINKING_SHIMMER.2,
        ),
        opacity,
    ))
}

fn spinner_stall_intensity(app: &App, animation_elapsed_ms: u64, mode: SpinnerMode) -> f64 {
    if mode == SpinnerMode::ToolUse {
        return 0.0;
    }
    let elapsed_since_output = animation_elapsed_ms.saturating_sub(app.spinner.last_output_at_ms);
    if elapsed_since_output <= SPINNER_STALL_AFTER_MS {
        return 0.0;
    }
    let ramp_elapsed = elapsed_since_output - SPINNER_STALL_AFTER_MS;
    let raw = (ramp_elapsed as f64 / SPINNER_STALL_RAMP_MS as f64).min(1.0);
    if app.reduced_motion {
        return raw;
    }

    let steps = ramp_elapsed / SPINNER_TICK_MS;
    let initial_steps = steps.min(SPINNER_STALL_RAMP_MS / SPINNER_TICK_MS);
    let mut smoothed = 0.0;
    for step in 1..=initial_steps {
        let step_target = (step * SPINNER_TICK_MS) as f64 / SPINNER_STALL_RAMP_MS as f64;
        smoothed += (step_target.min(1.0) - smoothed) * 0.1;
    }
    if steps > initial_steps {
        smoothed = 1.0 - (1.0 - smoothed) * 0.9_f64.powf((steps - initial_steps) as f64);
    }
    smoothed
}

fn format_elapsed(milliseconds: u64) -> String {
    if milliseconds < 60_000 {
        return format!("{}s", milliseconds / 1_000);
    }
    let total_seconds = (milliseconds as f64 / 1_000.0).round() as u64;
    let total_minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    if total_minutes < 60 {
        return format!("{total_minutes}m {seconds}s");
    }
    let total_hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    if total_hours < 24 {
        return format!("{total_hours}h {minutes}m {seconds}s");
    }
    let days = total_hours / 24;
    let hours = total_hours % 24;
    format!("{days}d {hours}h {minutes}m")
}

fn format_spinner_tokens(tokens: u64) -> String {
    if tokens < 1_000 {
        return tokens.to_string();
    }
    if tokens < 1_000_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        format!("{:.1}m", tokens as f64 / 1_000_000.0)
    }
}

fn color_components(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Rgb(red, green, blue) => (red, green, blue),
        _ => (255, 255, 255),
    }
}

fn interpolate_rgb(start: Color, end: (u8, u8, u8), amount: f64) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    let (start_red, start_green, start_blue) = color_components(start);
    let channel = |from: u8, to: u8| {
        (from as f64 + (to as f64 - from as f64) * amount)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color::Rgb(
        channel(start_red, end.0),
        channel(start_green, end.1),
        channel(start_blue, end.2),
    )
}

fn clip_spinner_line(line: Line<'static>, width: usize) -> Line<'static> {
    if width == 0 || line.width() <= width {
        return if width == 0 { Line::default() } else { line };
    }
    let mut remaining = width;
    let mut spans = Vec::new();
    for span in line.spans {
        let mut content = String::new();
        for grapheme in span.content.graphemes(true) {
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

fn sanitize_json_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(content) => *content = sanitize_plain(content),
        serde_json::Value::Array(values) => {
            for value in values {
                sanitize_json_value(value);
            }
        }
        serde_json::Value::Object(values) => {
            let sanitized = std::mem::take(values)
                .into_iter()
                .map(|(key, mut value)| {
                    sanitize_json_value(&mut value);
                    (sanitize_plain(&key), value)
                })
                .collect();
            *values = sanitized;
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

fn sanitize_shell_completion(completion: ShellCompletion) -> ShellCompletion {
    ShellCompletion {
        exit: completion.exit.map(|exit| ShellExitMetadata {
            cwd: exit.cwd.map(|value| sanitize_plain(&value)),
            exit_code: exit.exit_code,
            output_file_path: exit.output_file_path.map(|value| sanitize_plain(&value)),
            output_preview: exit.output_preview.map(|value| sanitize_ansi(&value)),
            output_truncated: exit.output_truncated,
            shell_id: sanitize_plain(&exit.shell_id),
        }),
        output: completion.output.map(|value| sanitize_ansi(&value)),
        image_detected: completion.image_detected,
    }
}

fn shell_timeout(arguments: Option<&serde_json::Value>) -> Option<String> {
    let arguments = arguments?.as_object()?;
    ["timeout", "timeout_ms", "timeoutMs"]
        .iter()
        .find_map(|key| arguments.get(*key))
        .and_then(normalize_timeout_value)
}

fn normalize_timeout_value(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) => parse_timeout_ms(value)
            .map(format_timeout)
            .or_else(|| (!value.trim().is_empty()).then(|| value.trim().to_string())),
        serde_json::Value::Number(value) => value.as_u64().map(format_timeout),
        _ => None,
    }
}

fn parse_timeout_ms(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    let bytes = value.as_bytes();
    let mut index = 0;
    let mut total = 0u64;
    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let number_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if number_start == index {
            return None;
        }
        let number = value[number_start..index].parse::<u64>().ok()?;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let (multiplier, unit_length) = if bytes
            .get(index..)
            .is_some_and(|unit| unit.starts_with(b"ms"))
        {
            (1, 2)
        } else if bytes.get(index) == Some(&b's') {
            (1_000, 1)
        } else if bytes.get(index) == Some(&b'm') {
            (60_000, 1)
        } else if bytes.get(index) == Some(&b'h') {
            (3_600_000, 1)
        } else if bytes.get(index) == Some(&b'd') {
            (86_400_000, 1)
        } else if index == bytes.len() {
            (1, 0)
        } else {
            return None;
        };
        total = total.checked_add(number.checked_mul(multiplier)?)?;
        index += unit_length;
    }
    Some(total)
}

fn format_timeout(milliseconds: u64) -> String {
    let total_seconds = milliseconds.saturating_add(999) / 1_000;
    let days = total_seconds / 86_400;
    let hours = (total_seconds % 86_400) / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{seconds}s"));
    }
    parts.join(" ")
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
            output,
            status,
            kind,
            agent_id,
            started_at,
            timeout,
        } => Some(TranscriptPayload::ToolProgress(ToolProgressPayload {
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            output: output.clone(),
            status: status.clone(),
            kind: *kind,
            agent_id: agent_id.clone(),
            started_at: *started_at,
            timeout: timeout.clone(),
        })),
        ChatEntry::ToolResult {
            tool_call_id,
            tool_name,
            arguments,
            content,
            partial_output,
            shell_completion,
            state,
            agent_id,
            cwd,
        } => Some(TranscriptPayload::ToolResult(ToolResultPayload {
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            arguments: arguments.clone(),
            content: content.clone(),
            partial_output: partial_output.clone(),
            shell_completion: shell_completion.clone(),
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
        ChatEntry::LocalOutput(lines) => local_output_lines(lines),
        ChatEntry::Completed => Vec::new(),
    }
}

fn local_output_lines(lines: &[Line<'static>]) -> Vec<Line<'static>> {
    let lines = if lines.is_empty() {
        vec![Line::from(Span::styled(
            "(no content)",
            Style::default()
                .fg(palette::INACTIVE)
                .add_modifier(Modifier::DIM),
        ))]
    } else {
        lines.to_vec()
    };
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let gutter = if index == 0 { "  ⎿  " } else { "     " };
            let gutter_style = if index == 0 {
                Style::default()
                    .fg(palette::INACTIVE)
                    .add_modifier(Modifier::DIM)
            } else {
                Style::default()
            };
            let mut spans = vec![Span::styled(gutter, gutter_style)];
            spans.extend(line.spans.clone());
            Line::from(spans)
        })
        .collect()
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
        Some(agent_id) => format!("{symbol} {} ", sanitize_plain(agent_id)),
        None => format!("{symbol} "),
    }
}

fn agent_suffix(agent_id: Option<&str>) -> String {
    agent_id
        .map(|id| format!(" ({})", sanitize_plain(id)))
        .unwrap_or_default()
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
    use ratatui::text::Line;
    use ratatui::Terminal;
    use serde_json::json;
    use unicode_segmentation::UnicodeSegmentation;

    use super::{
        builtin_spinner_verb, displayed_reasoning_effort, draw, draw_live_chat, format_elapsed,
        format_spinner_tokens, handle_key, model_context_label, model_cost_label_for,
        model_picker_row_for, render_spinner_line_for_platform, send_with_fleet_fallback,
        skill_selection_for_invocation, spinner_frames, spinner_message_spans,
        spinner_platform_for, spinner_stall_intensity, thinking_status, App, ChatEntry,
        ModelSelection, SendPath, SpinnerMode, SpinnerPlatform, UiAction, MAX_PICKER_ROWS,
        SPINNER_STATUS_AFTER_MS,
    };
    use crate::events::{
        ContextAttributionSnapshot, ContextCategorySnapshot, EventUpdate, TodoDependencySnapshot,
        TodoRowSnapshot, TodoSnapshot, UsageMetricsSnapshot, UsageSnapshot,
    };
    use crate::palette;
    use crate::permissions::{ApprovalCategory, ApprovalDecision, ApprovalRequest};
    use crate::screen_model::{
        render_entry_lines, render_transcript_payload, LiveEntryKind, ScreenChange, ScreenModel,
    };
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

    fn long_skill_catalog(count: usize) -> SkillCatalog {
        let root = SkillRoot {
            path: PathBuf::from("C:\\project\\.agents\\skills"),
            source: SkillRootSource::Project,
        };
        let skills = (0..count)
            .map(|index| Skill {
                name: format!("skill-{index}"),
                description: format!("Description {index}"),
                user_invocable: true,
                directory: root.path.join(format!("skill-{index}")),
                root: root.clone(),
            })
            .collect();
        SkillCatalog::from_parts(vec![root], skills, Vec::new())
    }

    fn rendered_rows(app: &App, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|frame| draw(frame, app))
            .expect("surface should render");
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    fn test_sessions(count: usize) -> Vec<SessionMetadata> {
        (0..count)
            .map(|index| SessionMetadata {
                session_id: SessionId::from(format!("session-{index}")),
                start_time: "2026-08-31T12:00:00Z".to_string(),
                modified_time: format!("2026-08-31T12:{index:02}:00Z"),
                summary: Some(format!("session-{index}")),
                is_remote: false,
            })
            .collect()
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
    fn prompt_uses_full_width_rules_hint_and_no_shortcut_bar() {
        let app = App::new(None);
        let mut terminal = Terminal::new(TestBackend::new(60, 12)).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &app))
            .expect("prompt should render");

        let rows = (0..terminal.backend().buffer().area.height)
            .map(|y| {
                (0..terminal.backend().buffer().area.width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let top_rule = rows
            .iter()
            .position(|row| row.chars().all(|character| character == '─'))
            .expect("prompt top rule should be visible");

        assert!(top_rule > 0);
        assert!(rows[top_rule - 1].trim().is_empty());
        assert_eq!(rows[top_rule].chars().count(), 60);
        assert!(rows[top_rule + 1].starts_with("❯ "));
        assert!(rows.iter().any(|row| row.starts_with("  ? for shortcuts")));
        assert!(!rows.iter().any(|row| row.contains("^N")));
    }

    #[test]
    fn session_picker_replaces_prompt_without_overlay_or_history_rows() {
        let mut app = App::new(None);
        app.set_sessions(vec![SessionMetadata {
            session_id: SessionId::from("session-1"),
            start_time: "2026-08-31T12:00:00Z".to_string(),
            modified_time: "2026-08-31T12:01:00Z".to_string(),
            summary: Some("first session".to_string()),
            is_remote: false,
        }]);
        let mut terminal = Terminal::new(TestBackend::new(80, 14)).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &app))
            .expect("picker should render");

        let rows = (0..terminal.backend().buffer().area.height)
            .map(|y| {
                (0..terminal.backend().buffer().area.width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rows
            .iter()
            .any(|row| row.contains("Select a session to resume")));
        assert!(rows.iter().any(|row| row.contains("❯")));
        assert!(!rows.iter().any(|row| row.contains('┌')));
        assert!(!rows.iter().any(|row| row.contains('│')));
        assert!(!rows.iter().any(|row| row.contains("? for shortcuts")));
        assert!(app.entries().is_empty());
    }

    #[test]
    fn usage_is_static_transcript_output_without_a_picker_overlay() {
        let mut app = App::new(Some("gpt-5".to_string()));
        app.set_usage(
            UsageMetricsSnapshot {
                total_nano_aiu: Some(1.0),
                total_premium_request_cost: 2.0,
                total_user_requests: 3,
                total_api_duration_ms: 4,
                current_model: Some("gpt-5".to_string()),
            },
            None,
        );
        let mut terminal = Terminal::new(TestBackend::new(100, 18)).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &app))
            .expect("usage should render in the transcript");

        let rows = (0..terminal.backend().buffer().area.height)
            .map(|y| {
                (0..terminal.backend().buffer().area.width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rows.iter().any(|row| row.contains("/usage")));
        assert!(rows.iter().any(|row| row.contains("⎿")));
        assert!(rows.iter().any(|row| row.contains("Session cost:")));
        assert!(!rows.iter().any(|row| row.contains("usage and context")));
        assert!(!app.picker_is_open());
    }

    #[test]
    fn todos_are_a_capped_live_block_above_input() {
        let mut app = App::new(None);
        app.set_fleet_active(true);
        app.set_todos(TodoSnapshot {
            rows: (0..8)
                .map(|index| TodoRowSnapshot {
                    id: format!("todo-{index}"),
                    title: format!("Task {index}"),
                    description: String::new(),
                    status: if index % 2 == 0 {
                        "pending".to_string()
                    } else {
                        "in_progress".to_string()
                    },
                })
                .collect(),
            dependencies: Vec::new(),
        });
        let mut terminal = Terminal::new(TestBackend::new(100, 18)).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &app))
            .expect("todos should render live");

        let rows = (0..terminal.backend().buffer().area.height)
            .map(|y| {
                (0..terminal.backend().buffer().area.width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rows.iter().any(|row| row.contains("Fleet todos")));
        assert!(rows
            .iter()
            .any(|row| row.contains("… +2 pending, 2 in progress")));
        assert!(!rows.iter().any(|row| row.contains('┌')));
        assert!(app.entries().is_empty());
    }

    #[test]
    fn todo_live_block_uses_reference_status_glyphs() {
        let mut app = App::new(None);
        app.set_fleet_active(true);
        app.set_todos(TodoSnapshot {
            rows: vec![
                TodoRowSnapshot {
                    id: "done".to_string(),
                    title: "Done task".to_string(),
                    description: String::new(),
                    status: "completed".to_string(),
                },
                TodoRowSnapshot {
                    id: "working".to_string(),
                    title: "Working task".to_string(),
                    description: String::new(),
                    status: "in_progress".to_string(),
                },
                TodoRowSnapshot {
                    id: "pending".to_string(),
                    title: "Pending task".to_string(),
                    description: String::new(),
                    status: "pending".to_string(),
                },
            ],
            dependencies: Vec::new(),
        });
        let mut terminal = Terminal::new(TestBackend::new(100, 18)).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &app))
            .expect("todos should render");

        let rows = (0..terminal.backend().buffer().area.height)
            .map(|y| {
                (0..terminal.backend().buffer().area.width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rows.iter().any(|row| row.contains("✓ Done task")));
        assert!(rows.iter().any(|row| row.contains("◼ Working task")));
        assert!(rows.iter().any(|row| row.contains("◻ Pending task")));
    }

    #[test]
    fn approval_replaces_input_with_borderless_choices() {
        let mut app = App::new(None);
        let (respond_to, _response) = tokio::sync::oneshot::channel();
        app.enqueue_approval(ApprovalRequest {
            category: ApprovalCategory::Shell,
            tool_name: "bash".to_string(),
            details: "cargo test".to_string(),
            respond_to,
        });
        let mut terminal = Terminal::new(TestBackend::new(100, 18)).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &app))
            .expect("approval should render inline");

        let rows = (0..terminal.backend().buffer().area.height)
            .map(|y| {
                (0..terminal.backend().buffer().area.width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rows.iter().any(|row| row.contains("approval required")));
        assert!(rows.iter().any(|row| row.contains("⎿  cargo test")));
        assert!(rows.iter().any(|row| row.contains("Allow once")));
        assert!(rows.iter().any(|row| row.contains("Deny")));
        assert!(!rows.iter().any(|row| row.contains('┌')));
        assert!(!rows.iter().any(|row| row.contains("y allow once")));
    }

    #[tokio::test]
    async fn approval_escape_denies_the_pending_request() {
        let mut app = App::new(None);
        let (respond_to, response) = tokio::sync::oneshot::channel();
        app.enqueue_approval(ApprovalRequest {
            category: ApprovalCategory::Shell,
            tool_name: "bash".to_string(),
            details: "cargo test".to_string(),
            respond_to,
        });

        assert_eq!(
            handle_key(&mut app, key(KeyCode::Esc, KeyEventKind::Press)),
            UiAction::Approval(crate::permissions::ApprovalDecision::Deny)
        );
        let request = app
            .resolve_approval(crate::permissions::ApprovalDecision::Deny)
            .expect("approval should still be queued");
        request
            .respond_to
            .send(crate::permissions::ApprovalDecision::Deny)
            .expect("test response receiver should still be open");
        assert_eq!(
            response.await.expect("approval response should arrive"),
            crate::permissions::ApprovalDecision::Deny
        );
        assert!(app.pending_approval().is_none());
        assert!(!app.picker_is_open());
    }

    #[tokio::test]
    async fn approval_picker_remains_authoritative_until_fifo_resolution() {
        let mut app = App::new(None);
        let (first_sender, first_response) = tokio::sync::oneshot::channel();
        let (second_sender, second_response) = tokio::sync::oneshot::channel();
        app.enqueue_approval(ApprovalRequest {
            category: ApprovalCategory::Shell,
            tool_name: "first".to_string(),
            details: "first command".to_string(),
            respond_to: first_sender,
        });
        app.enqueue_approval(ApprovalRequest {
            category: ApprovalCategory::Shell,
            tool_name: "second".to_string(),
            details: "second command".to_string(),
            respond_to: second_sender,
        });

        assert_eq!(handle_key(&mut app, ctrl_key('k')), UiAction::None);
        assert!(app.picker_is_open());
        assert_eq!(
            app.pending_approval()
                .expect("front approval should remain pending")
                .tool_name,
            "first"
        );

        let request = app
            .resolve_approval(ApprovalDecision::ApproveOnce)
            .expect("first approval should resolve");
        request
            .respond_to
            .send(ApprovalDecision::ApproveOnce)
            .expect("first response receiver should remain open");
        assert!(app.picker_is_open());
        assert_eq!(
            app.pending_approval()
                .expect("second approval should advance to the front")
                .tool_name,
            "second"
        );

        let request = app
            .resolve_approval(ApprovalDecision::Deny)
            .expect("second approval should resolve");
        request
            .respond_to
            .send(ApprovalDecision::Deny)
            .expect("second response receiver should remain open");
        assert!(!app.picker_is_open());
        assert_eq!(
            first_response.await.expect("first response should arrive"),
            ApprovalDecision::ApproveOnce
        );
        assert_eq!(
            second_response
                .await
                .expect("second response should arrive"),
            ApprovalDecision::Deny
        );
    }

    #[test]
    fn long_picker_titles_show_position_and_keep_five_rows() {
        let mut tools = App::new(None);
        tools.open_tool_picker();
        let rows = rendered_rows(&tools, 100, 16);
        assert!(rows
            .iter()
            .any(|row| row.contains(&format!("Select tools (1 of {TOOL_COUNT}):"))));
        assert_eq!(
            rows.iter()
                .filter(|row| row.contains("[✓]") || row.contains("[ ]"))
                .count(),
            MAX_PICKER_ROWS
        );

        let mut skills = App::new(None);
        skills.set_skill_catalog(long_skill_catalog(6));
        skills.open_skill_picker();
        let rows = rendered_rows(&skills, 100, 16);
        assert!(rows
            .iter()
            .any(|row| row.contains("Select skills (1 of 6):")));
        assert_eq!(
            rows.iter()
                .filter(|row| row.contains("[✓]") || row.contains("[ ]"))
                .count(),
            MAX_PICKER_ROWS
        );

        handle_key(&mut skills, key(KeyCode::Down, KeyEventKind::Press));
        let rows = rendered_rows(&skills, 100, 16);
        assert!(rows
            .iter()
            .any(|row| row.contains("Select skills (2 of 6):")));
    }

    #[test]
    fn five_hundred_sessions_cover_shared_window_navigation_and_arrows() {
        let mut app = App::new(None);
        app.set_sessions(test_sessions(500));

        let rows = rendered_rows(&app, 120, 18);
        assert!(rows.iter().any(|row| row.contains("(1 of 500)")));
        let option_rows = rows
            .iter()
            .filter(|row| row.contains("session-"))
            .collect::<Vec<_>>();
        assert_eq!(option_rows.len(), MAX_PICKER_ROWS);
        assert!(option_rows.iter().any(|row| row.contains("❯")));
        assert!(option_rows.iter().any(|row| row.contains("↓")));

        for _ in 0..6 {
            handle_key(&mut app, key(KeyCode::Down, KeyEventKind::Press));
        }
        handle_key(&mut app, key(KeyCode::Up, KeyEventKind::Press));
        let rows = rendered_rows(&app, 120, 18);
        let option_rows = rows
            .iter()
            .filter(|row| row.contains("session-"))
            .collect::<Vec<_>>();
        assert!(option_rows.iter().any(|row| row.contains("↑")));
        assert!(option_rows.iter().any(|row| row.contains("↓")));
        assert!(option_rows.iter().any(|row| row.contains("❯")));

        app.selected_item = 0;
        app.picker_window_start = 0;
        handle_key(&mut app, key(KeyCode::Up, KeyEventKind::Press));
        assert_eq!(app.selected_item, 499);
        assert_eq!(app.picker_window_start, 495);
        let rows = rendered_rows(&app, 120, 18);
        let option_rows = rows
            .iter()
            .filter(|row| row.contains("session-"))
            .collect::<Vec<_>>();
        assert!(option_rows.iter().any(|row| row.contains("❯")));
        assert!(!option_rows.iter().any(|row| row.contains("↓")));

        handle_key(&mut app, key(KeyCode::Down, KeyEventKind::Press));
        assert_eq!(app.selected_item, 0);
        assert_eq!(app.picker_window_start, 0);

        handle_key(&mut app, key(KeyCode::PageDown, KeyEventKind::Press));
        assert_eq!(app.selected_item, MAX_PICKER_ROWS);
        handle_key(&mut app, key(KeyCode::PageUp, KeyEventKind::Press));
        assert_eq!(app.selected_item, 0);

        handle_key(&mut app, key(KeyCode::Char('j'), KeyEventKind::Press));
        assert_eq!(app.selected_item, 1);
        handle_key(&mut app, ctrl_key('p'));
        assert_eq!(app.selected_item, 0);
        handle_key(&mut app, ctrl_key('n'));
        assert_eq!(app.selected_item, 1);
        handle_key(&mut app, key(KeyCode::Char('k'), KeyEventKind::Press));
        assert_eq!(app.selected_item, 0);
    }

    #[test]
    fn numeric_selection_and_multi_select_toggle_are_immediate() {
        let mut sessions = App::new(None);
        sessions.set_sessions(test_sessions(3));
        assert_eq!(
            handle_key(&mut sessions, key(KeyCode::Char('2'), KeyEventKind::Press)),
            UiAction::Resume(SessionId::from("session-1"))
        );
        assert!(!sessions.picker_is_open());

        let mut tools = App::new(None);
        tools.open_tool_picker();
        assert!(tools.picker_toolset.contains_at(0));
        assert_eq!(
            handle_key(&mut tools, key(KeyCode::Char('1'), KeyEventKind::Press)),
            UiAction::None
        );
        assert!(!tools.picker_toolset.contains_at(0));
        assert!(tools.picker_is_open());
    }

    #[test]
    fn picker_rendering_handles_zero_and_one_cell_viewports_without_history_rows() {
        let mut app = App::new(None);
        app.set_sessions(test_sessions(1));

        for (width, height) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            let mut terminal =
                Terminal::new(TestBackend::new(width, height)).expect("test terminal");
            terminal
                .draw(|frame| draw(frame, &app))
                .expect("narrow picker should render");
        }
        assert!(app.entries().is_empty());
    }

    #[test]
    fn prompt_software_cursor_reverses_middle_and_end_cells() {
        let mut app = App::new(None);
        for character in "abcd".chars() {
            app.push_input(character);
        }
        handle_key(&mut app, key(KeyCode::Left, KeyEventKind::Press));
        handle_key(&mut app, key(KeyCode::Left, KeyEventKind::Press));

        let mut terminal = Terminal::new(TestBackend::new(60, 12)).expect("test terminal");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("middle cursor should render");
        let top_rule = (0..terminal.backend().buffer().area.height)
            .find(|y| {
                (0..terminal.backend().buffer().area.width)
                    .all(|x| terminal.backend().buffer()[(x, *y)].symbol() == "─")
            })
            .expect("prompt top rule should be visible");
        assert_eq!(terminal.backend().buffer()[(4, top_rule + 1)].symbol(), "c");
        assert!(terminal.backend().buffer()[(4, top_rule + 1)]
            .style()
            .add_modifier
            .contains(Modifier::REVERSED));

        handle_key(&mut app, key(KeyCode::End, KeyEventKind::Press));
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("end cursor should render");
        assert_eq!(terminal.backend().buffer()[(6, top_rule + 1)].symbol(), " ");
        assert!(terminal.backend().buffer()[(6, top_rule + 1)]
            .style()
            .add_modifier
            .contains(Modifier::REVERSED));
    }

    #[test]
    fn prompt_hides_ordinary_hint_after_first_typed_character() {
        let mut app = App::new(None);
        app.push_input('x');
        let mut terminal = Terminal::new(TestBackend::new(60, 12)).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &app))
            .expect("typed prompt should render");

        let rows = (0..terminal.backend().buffer().area.height)
            .map(|y| {
                (0..terminal.backend().buffer().area.width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(!rows.iter().any(|row| row.contains("? for shortcuts")));
    }

    #[test]
    fn prompt_budget_keeps_three_rows_and_centers_long_input_cursor() {
        let mut app = App::new(None);
        app.insert_paste("one\ntwo\nthree\nfour\nfive\nsix\nseven\neight");
        app.move_input_up();
        app.move_input_up();

        let layout = super::prompt_layout(&app, ratatui::layout::Rect::new(0, 0, 80, 12));
        assert_eq!(layout.input_rows, 8);
        assert_eq!(layout.footer_rows, 0);
        assert_eq!(
            super::cursor_scroll_start(5, 8, layout.input_rows as usize),
            0
        );

        let empty = App::new(None);
        let empty_layout = super::prompt_layout(&empty, ratatui::layout::Rect::new(0, 0, 80, 12));
        assert_eq!(empty_layout.input_rows, 3);
    }

    #[test]
    fn busy_prompt_dims_marker_keeps_busy_hint_and_uses_prompt_palette() {
        let mut app = App::new(None);
        app.add_user_message("work".to_string());
        let mut terminal = Terminal::new(TestBackend::new(60, 12)).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &app))
            .expect("busy prompt should render");

        let top_rule = (0..terminal.backend().buffer().area.height)
            .find(|y| {
                (0..terminal.backend().buffer().area.width)
                    .all(|x| terminal.backend().buffer()[(x, *y)].symbol() == "─")
            })
            .expect("prompt top rule should be visible");
        assert_eq!(
            terminal.backend().buffer()[(0, top_rule)].style().fg,
            Some(palette::PROMPT_BORDER)
        );
        assert!(terminal.backend().buffer()[(0, top_rule + 1)]
            .style()
            .add_modifier
            .contains(Modifier::DIM));
        let rows = (0..terminal.backend().buffer().area.height)
            .map(|y| {
                (0..terminal.backend().buffer().area.width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rows.iter().any(|row| row.contains("esc to interrupt")));
    }

    #[test]
    fn completions_are_below_the_rule_borderless_capped_and_color_selected() {
        let mut app = App::new(None);
        app.completion = Some(super::CompletionState {
            candidates: (0..8)
                .map(|index| super::CompletionCandidate {
                    command: format!("/command-{index}"),
                    description: "description   with\n collapsed whitespace".to_string(),
                })
                .collect(),
            selected_item: 7,
            token_start: 0,
            token_end: 0,
        });
        let mut terminal = Terminal::new(TestBackend::new(80, 16)).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &app))
            .expect("completion surface should render");

        let rows = (0..terminal.backend().buffer().area.height)
            .map(|y| {
                (0..terminal.backend().buffer().area.width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let top_rule = rows
            .iter()
            .position(|row| row.chars().all(|character| character == '─'))
            .expect("prompt top rule should be visible");
        let bottom_rule = rows
            .iter()
            .enumerate()
            .skip(top_rule + 1)
            .find(|(_, row)| row.chars().all(|character| character == '─'))
            .map(|(index, _)| index)
            .expect("prompt bottom rule should be visible");
        let completion_rows = &rows[bottom_rule + 1..];
        assert_eq!(completion_rows.len(), 6);
        assert!(completion_rows.iter().all(|row| row.starts_with("  ")));
        assert!(!completion_rows.iter().any(|row| row.contains("commands")));
        assert!(!completion_rows.iter().any(|row| row.contains('›')));
        assert!(completion_rows
            .iter()
            .any(|row| row.contains("collapsed whitespace")));

        let selected_row = bottom_rule + 1 + (7 - 2);
        assert_eq!(
            terminal.backend().buffer()[(2, selected_row as u16)]
                .style()
                .fg,
            Some(palette::SUGGESTION)
        );
        assert_eq!(
            terminal.backend().buffer()[(2, (bottom_rule + 1) as u16)]
                .style()
                .fg,
            Some(palette::INACTIVE)
        );
    }

    #[test]
    fn completions_use_remaining_budget_in_a_twelve_row_frame() {
        let mut app = App::new(None);
        app.completion = Some(super::CompletionState {
            candidates: (0..8)
                .map(|index| super::CompletionCandidate {
                    command: format!("/command-{index}"),
                    description: format!("description {index}"),
                })
                .collect(),
            selected_item: 4,
            token_start: 0,
            token_end: 0,
        });

        let layout = super::prompt_layout(&app, ratatui::layout::Rect::new(0, 0, 80, 12));
        assert_eq!(layout.input_rows, 3);
        assert_eq!(layout.footer_rows, 5);

        let mut terminal = Terminal::new(TestBackend::new(80, 12)).expect("test terminal");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("partial completion surface should render");

        let rows = (0..terminal.backend().buffer().area.height)
            .map(|y| {
                (0..terminal.backend().buffer().area.width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let top_rule = rows
            .iter()
            .position(|row| row.chars().all(|character| character == '─'))
            .expect("prompt top rule should be visible");
        let bottom_rule = rows
            .iter()
            .enumerate()
            .skip(top_rule + 1)
            .find(|(_, row)| row.chars().all(|character| character == '─'))
            .map(|(index, _)| index)
            .expect("prompt bottom rule should be visible");
        let completion_rows = &rows[bottom_rule + 1..];
        assert_eq!(completion_rows.len(), 5);
        assert!(completion_rows[2].contains("/command-4"));
        assert_eq!(
            terminal.backend().buffer()[(2, (bottom_rule + 1 + 2) as u16)]
                .style()
                .fg,
            Some(palette::SUGGESTION)
        );
    }

    #[test]
    fn prompt_degrades_without_panicking_in_zero_and_one_row_areas() {
        let app = App::new(None);
        for height in [0, 1] {
            let mut terminal = Terminal::new(TestBackend::new(40, height)).expect("test terminal");
            terminal
                .draw(|frame| draw(frame, &app))
                .expect("tiny prompt should render");
        }
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
        assert_eq!(wrapped.lines[0].to_string(), "❯ 12345678");
        assert_eq!(wrapped.lines[1].to_string(), "  9 ");
        assert_eq!(wrapped.cursor_row, 1);

        let wide = super::wrap_input("ab🙂", "ab🙂".len(), 10);
        assert_eq!(wide.cursor_row, 0);

        let combining = super::wrap_input("e\u{301}x", "e\u{301}".len(), 10);
        assert_eq!(combining.cursor_row, 0);
        assert!(combining.lines[0].to_string().contains("e\u{301}"));
    }

    #[test]
    fn input_wrap_preserves_combining_and_zwj_graphemes_at_narrow_boundaries() {
        let combining_text = "e\u{301}x";
        let combining_end = combining_text.graphemes(true).next().unwrap().len();
        let combining = super::wrap_input(combining_text, combining_end, 3);
        assert_eq!(combining.lines.len(), 2);
        assert_eq!(combining.lines[0].to_string(), "❯ e\u{301}");
        assert_eq!(combining.lines[1].to_string(), "  x");
        assert_eq!(combining.cursor_row, 1);

        let zwj_text = "👩‍💻x";
        let zwj_end = zwj_text.graphemes(true).next().unwrap().len();
        let zwj = super::wrap_input(zwj_text, zwj_end, 3);
        assert_eq!(zwj.lines.len(), 2);
        assert_eq!(zwj.lines[0].to_string(), "❯ 👩‍💻");
        assert_eq!(zwj.lines[1].to_string(), "  x");
        assert_eq!(zwj.cursor_row, 1);
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
        assert!(lines[7].to_string().ends_with(' '));
    }

    #[test]
    fn busy_chat_uses_a_source_derived_spinner_row() {
        let mut app = App::new(None);
        app.add_user_message("Inspect this".to_string());

        let lines = super::chat_lines_at_width(&app, 80);
        let spinner_rows = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.to_string().contains('…'))
            .collect::<Vec<_>>();

        assert_eq!(spinner_rows.len(), 1);
        let (index, spinner) = spinner_rows[0];
        assert!(index > 0 && lines[index - 1].to_string().is_empty());
        assert!(spinner.to_string().ends_with(' '));
        assert!(!spinner.to_string().contains("Copilot is responding"));
    }

    #[test]
    fn live_spinner_reserves_the_bottom_rows_in_a_saturated_viewport() {
        let mut app = App::new(None);
        app.add_user_message("Inspect this".to_string());
        let mut screen = ScreenModel::default();
        for index in 0..8 {
            screen
                .start_live(
                    format!("live-{index}"),
                    LiveEntryKind::Other,
                    vec![Line::from(format!("transcript-{index}"))],
                )
                .expect("live entry should be accepted");
        }

        let mut terminal = Terminal::new(TestBackend::new(80, 4)).expect("test terminal");
        terminal
            .draw(|frame| {
                draw_live_chat(
                    frame,
                    &app,
                    &mut screen,
                    frame.area(),
                    app.spinner.started_at_ms,
                )
            })
            .expect("live chat should render");

        let rendered = (0..terminal.backend().buffer().area.height)
            .map(|y| {
                (0..terminal.backend().buffer().area.width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rendered[3].contains('…'));
        assert!(rendered[2].trim().is_empty());
        assert!(!rendered[3].contains("transcript-"));
        assert_eq!(screen.committed_count(), 0);
    }

    #[test]
    fn live_spinner_handles_zero_and_one_row_viewports() {
        let mut app = App::new(None);
        app.add_user_message("Inspect this".to_string());

        for height in [0, 1] {
            let mut screen = ScreenModel::default();
            let mut terminal = Terminal::new(TestBackend::new(80, height)).expect("test terminal");
            terminal
                .draw(|frame| {
                    draw_live_chat(
                        frame,
                        &app,
                        &mut screen,
                        frame.area(),
                        app.spinner.started_at_ms,
                    )
                })
                .expect("tiny live chat should render");

            if height == 1 {
                let row = (0..terminal.backend().buffer().area.width)
                    .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
                    .collect::<String>();
                assert!(row.contains('…'));
            }
        }
    }

    #[test]
    fn spinner_frames_match_each_platform_and_double_the_endpoints() {
        assert_eq!(
            spinner_frames(SpinnerPlatform::Macos),
            ["·", "✢", "✳", "✶", "✻", "✽", "✽", "✻", "✶", "✳", "✢", "·"]
        );
        assert_eq!(
            spinner_frames(SpinnerPlatform::WindowsLinux),
            ["·", "✢", "*", "✶", "✻", "✽", "✽", "✻", "✶", "*", "✢", "·"]
        );
        assert_eq!(
            spinner_frames(SpinnerPlatform::Ghostty),
            ["·", "✢", "✳", "✶", "✻", "*", "*", "✻", "✶", "✳", "✢", "·"]
        );
        assert_eq!(
            spinner_platform_for(Some("xterm-ghostty"), true),
            SpinnerPlatform::Ghostty
        );
        assert_eq!(
            spinner_platform_for(Some("xterm-256color"), true),
            SpinnerPlatform::Macos
        );
        assert_eq!(
            spinner_platform_for(Some("xterm-256color"), false),
            SpinnerPlatform::WindowsLinux
        );
    }

    #[test]
    fn spinner_frame_changes_only_at_the_120_ms_boundaries() {
        let mut app = App::new(None);
        app.add_user_message("Inspect this".to_string());
        let start = app.spinner.started_at_ms;
        let expected = ["·", "✢", "*", "✶", "✻", "✽", "✽", "✻", "✶", "*", "✢", "·"];

        for (index, glyph) in expected.iter().enumerate() {
            let before = render_spinner_line_for_platform(
                &app,
                80,
                start + index as u64 * 120 + 119,
                SpinnerPlatform::WindowsLinux,
            );
            assert!(before.to_string().starts_with(&format!("{glyph} ")));
            let at_boundary = render_spinner_line_for_platform(
                &app,
                80,
                start + (index as u64 + 1) * 120,
                SpinnerPlatform::WindowsLinux,
            );
            let next_glyph = expected[(index + 1) % expected.len()];
            assert!(at_boundary
                .to_string()
                .starts_with(&format!("{next_glyph} ")));
        }
    }

    #[test]
    fn spinner_verb_precedence_is_stable_for_a_turn_and_resets_on_idle() {
        let mut app = App::new(None);
        app.set_spinner_override(Some("Explicit…".to_string()));
        app.add_user_message("first".to_string());
        assert_eq!(app.spinner.verb, "Explicit…");
        let first_verb = app.spinner.verb.clone();
        let start = app.spinner.started_at_ms;
        assert_eq!(
            render_spinner_line_for_platform(&app, 80, start, SpinnerPlatform::WindowsLinux)
                .to_string()
                .trim_start_matches("· ")
                .split('…')
                .next(),
            Some("Explicit")
        );

        app.apply(EventUpdate::Idle);
        assert!(!app.spinner_visible());
        app.add_user_message("second".to_string());
        assert_eq!(app.spinner.verb, first_verb);
        assert_eq!(
            builtin_spinner_verb(1, "first"),
            builtin_spinner_verb(1, "first")
        );
    }

    #[test]
    fn spinner_verb_uses_the_active_todo_before_the_builtin_list() {
        let mut app = App::new(None);
        app.set_todos(TodoSnapshot {
            rows: vec![TodoRowSnapshot {
                id: "todo-1".to_string(),
                title: "Fallback title".to_string(),
                description: "Inspecting the source".to_string(),
                status: "in_progress".to_string(),
            }],
            dependencies: Vec::new(),
        });
        app.add_user_message("inspect".to_string());
        assert_eq!(app.spinner.verb, "Inspecting the source…");

        let mut title_app = App::new(None);
        title_app.set_todos(TodoSnapshot {
            rows: vec![TodoRowSnapshot {
                id: "todo-2".to_string(),
                title: "Fallback title".to_string(),
                description: "  ".to_string(),
                status: "in-progress".to_string(),
            }],
            dependencies: Vec::new(),
        });
        title_app.add_user_message("inspect".to_string());
        assert_eq!(title_app.spinner.verb, "Fallback title…");
    }

    #[test]
    fn spinner_mode_precedence_is_tool_then_reasoning_then_response_then_requesting() {
        let mut app = App::new(None);
        app.add_user_message("inspect".to_string());
        assert_eq!(app.spinner_mode(), SpinnerMode::Requesting);
        app.apply(EventUpdate::AssistantDelta {
            message_id: "message-1".to_string(),
            content: "response".to_string(),
            agent_id: None,
        });
        assert_eq!(app.spinner_mode(), SpinnerMode::LiveResponse);
        app.apply(EventUpdate::ReasoningDelta {
            reasoning_id: "reasoning-1".to_string(),
            content: "thinking".to_string(),
            agent_id: None,
        });
        assert_eq!(app.spinner_mode(), SpinnerMode::Reasoning);
        app.apply(EventUpdate::ToolStarted {
            tool_call_id: "tool-1".to_string(),
            tool_name: "Read".to_string(),
            arguments: None,
            agent_id: None,
        });
        assert_eq!(app.spinner_mode(), SpinnerMode::ToolUse);
    }

    #[test]
    fn reduced_motion_is_static_and_uses_immediate_stall_color() {
        let mut app = App::new(None);
        app.set_reduced_motion(true);
        app.add_user_message("inspect".to_string());
        let start = app.spinner.started_at_ms;
        let line =
            render_spinner_line_for_platform(&app, 80, start + 500, SpinnerPlatform::Ghostty);
        assert!(line.to_string().starts_with("● "));
        assert!(line.spans.iter().all(|span| {
            span.style.fg != Some(palette::CLAUDE_SHIMMER)
                && span.style.fg != Some(Color::Rgb(185, 185, 185))
        }));
        let stalled =
            render_spinner_line_for_platform(&app, 80, start + 5_001, SpinnerPlatform::Ghostty);
        assert_eq!(stalled.spans[0].style.fg, Some(Color::Rgb(171, 43, 63)));
        assert_eq!(app.spinner.displayed_characters, 0);
    }

    #[test]
    fn shimmer_is_grapheme_aware_and_moves_by_mode() {
        let spans = spinner_message_spans(
            "a🙂b",
            palette::CLAUDE,
            palette::CLAUDE_SHIMMER,
            SpinnerMode::Requesting,
            500,
            0.0,
            false,
        );
        assert_eq!(spans[0].style.fg, Some(palette::CLAUDE_SHIMMER));
        assert_eq!(spans[1].style.fg, Some(palette::CLAUDE_SHIMMER));
        assert_eq!(spans[2].style.fg, Some(palette::CLAUDE));

        let reverse = spinner_message_spans(
            "abc",
            palette::CLAUDE,
            palette::CLAUDE_SHIMMER,
            SpinnerMode::LiveResponse,
            2_000,
            0.0,
            false,
        );
        assert!(reverse
            .iter()
            .any(|span| span.style.fg == Some(palette::CLAUDE_SHIMMER)));
    }

    #[test]
    fn tool_flash_is_whole_message_interpolation_and_stall_excludes_tools() {
        let mut app = App::new(None);
        app.add_user_message("inspect".to_string());
        let start = app.spinner.started_at_ms;
        app.apply(EventUpdate::ToolStarted {
            tool_call_id: "tool-1".to_string(),
            tool_name: "Read".to_string(),
            arguments: None,
            agent_id: None,
        });
        let beginning =
            render_spinner_line_for_platform(&app, 80, start, SpinnerPlatform::WindowsLinux);
        let middle =
            render_spinner_line_for_platform(&app, 80, start + 500, SpinnerPlatform::WindowsLinux);
        assert_ne!(beginning.spans[1].style.fg, middle.spans[1].style.fg);
        assert_eq!(
            spinner_stall_intensity(&app, start + 20_000, SpinnerMode::ToolUse),
            0.0
        );
    }

    #[test]
    fn stall_starts_after_three_seconds_smooths_and_resets_on_output() {
        let mut app = App::new(None);
        app.add_user_message("inspect".to_string());
        let start = app.spinner.started_at_ms;
        assert_eq!(
            spinner_stall_intensity(&app, start + 3_000, SpinnerMode::Requesting),
            0.0
        );
        assert!(spinner_stall_intensity(&app, start + 3_050, SpinnerMode::Requesting) > 0.0);
        assert!(spinner_stall_intensity(&app, start + 5_000, SpinnerMode::Requesting) < 1.0);
        app.apply(EventUpdate::AssistantDelta {
            message_id: "message-1".to_string(),
            content: "new output".to_string(),
            agent_id: None,
        });
        let now = app.animation_elapsed_ms();
        assert_eq!(
            spinner_stall_intensity(&app, now + 100, SpinnerMode::LiveResponse),
            0.0
        );
    }

    #[test]
    fn long_active_tools_reset_stall_timing_before_response_resumes() {
        let mut app = App::new(None);
        app.add_user_message("inspect".to_string());
        let start = app.spinner.started_at_ms;
        app.spinner.last_output_at_ms = start;
        app.apply(EventUpdate::ToolStarted {
            tool_call_id: "tool-1".to_string(),
            tool_name: "Read".to_string(),
            arguments: None,
            agent_id: None,
        });

        app.advance_spinner(start + 10_000);
        assert_eq!(app.spinner.last_output_at_ms, start + 10_000);

        app.apply(EventUpdate::ToolCompleted {
            tool_call_id: "tool-1".to_string(),
            success: true,
            message: None,
            agent_id: None,
            shell_completion: None,
        });
        assert_eq!(
            spinner_stall_intensity(&app, start + 10_001, SpinnerMode::Requesting),
            0.0
        );
    }

    #[test]
    fn streamed_astral_response_counts_utf16_code_units_for_tokens() {
        let mut app = App::new(None);
        app.add_user_message("inspect".to_string());
        app.apply(EventUpdate::AssistantDelta {
            message_id: "message-1".to_string(),
            content: "🙂a".to_string(),
            agent_id: None,
        });

        assert_eq!(app.spinner.assistant_characters, 3);
    }

    #[test]
    fn spinner_status_gate_formatting_and_token_animation_match_the_spec() {
        assert_eq!(format_elapsed(0), "0s");
        assert_eq!(format_elapsed(12_000), "12s");
        assert_eq!(format_elapsed(65_000), "1m 5s");
        assert_eq!(format_elapsed(3_723_000), "1h 2m 3s");
        assert_eq!(format_elapsed(93_783_000), "1d 2h 3m");
        assert_eq!(format_spinner_tokens(900), "900");
        assert_eq!(format_spinner_tokens(1_000), "1.0k");
        assert_eq!(format_spinner_tokens(12_400), "12.4k");
        assert_eq!(format_spinner_tokens(1_200_000), "1.2m");

        let mut app = App::new(None);
        app.add_user_message("inspect".to_string());
        let start = app.spinner.started_at_ms;
        app.spinner.displayed_characters = 4_936;
        let before_gate = render_spinner_line_for_platform(
            &app,
            80,
            start + SPINNER_STATUS_AFTER_MS,
            SpinnerPlatform::WindowsLinux,
        );
        assert!(!before_gate.to_string().contains("30s"));
        let after_gate = render_spinner_line_for_platform(
            &app,
            80,
            start + SPINNER_STATUS_AFTER_MS + 1,
            SpinnerPlatform::WindowsLinux,
        );
        assert!(after_gate.to_string().contains("30s"));
        assert!(after_gate.to_string().contains("1.2k tokens"));

        app.spinner.displayed_characters = 0;
        app.spinner.assistant_characters = 400;
        app.advance_spinner(start + 50);
        assert_eq!(app.spinner.displayed_characters, 50);
        app.set_reduced_motion(true);
        app.spinner.assistant_characters = 4_000;
        app.advance_spinner(start + 100);
        assert_eq!(app.spinner.displayed_characters, 4_000);
    }

    #[test]
    fn thinking_status_holds_then_reports_duration_and_glows() {
        let mut app = App::new(None);
        app.add_user_message("inspect".to_string());
        app.set_reasoning_effort(Some("high".to_string()));
        let start = app.spinner.started_at_ms;
        app.reasoning_live_ids.insert("reasoning-1".to_string());
        assert_eq!(
            thinking_status(&app, start + 500),
            Some(("thinking with high effort".to_string(), true))
        );
        app.reasoning_live_ids.clear();
        app.spinner.thinking_until_ms = Some(start + 2_000);
        app.spinner.thought_until_ms = Some(start + 4_000);
        app.spinner.thought_duration_ms = 1_500;
        assert_eq!(
            thinking_status(&app, start + 1_999),
            Some(("thinking".to_string(), true))
        );
        assert_eq!(
            thinking_status(&app, start + 2_001),
            Some(("thought for 2s".to_string(), false))
        );
        let glowing = super::thinking_status_style(&app, 500).fg;
        let dimmer = super::thinking_status_style(&app, 1_500).fg;
        assert_ne!(glowing, dimmer);
    }

    #[test]
    fn spinner_width_never_overflows_and_idle_cleanup_leaves_no_spinner_history() {
        let mut app = App::new(None);
        app.add_user_message("inspect".to_string());
        app.show_internals = true;
        app.spinner.displayed_characters = 49_600;
        let start = app.spinner.started_at_ms;
        for width in 0..=40 {
            let line = render_spinner_line_for_platform(
                &app,
                width,
                start + 31_000,
                SpinnerPlatform::WindowsLinux,
            );
            assert!(line.width() <= width, "spinner overflow at width {width}");
        }
        app.apply(EventUpdate::Idle);
        assert!(super::chat_lines_at_width(&app, 80)
            .iter()
            .all(|line| !line.to_string().contains("tokens")));
        app.add_user_message("blocked".to_string());
        app.apply(EventUpdate::Banner {
            severity: crate::events::BannerSeverity::BlockingError,
            message: "blocked".to_string(),
            url: None,
        });
        assert!(!app.spinner_visible());
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
            shell_completion: None,
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
    fn frame_has_no_persistent_status_row_or_status_metadata() {
        let mut app = App::new_with_working_directory(
            Some("gpt-5".to_string()),
            Path::new("C:\\dev\\picopilot"),
        );
        app.set_reasoning_effort(Some("high".to_string()));

        let rows = rendered_rows(&app, 100, 18);

        assert!(!rows.iter().any(|row| row.contains("gpt-5")));
        assert!(!rows.iter().any(|row| row.contains("high reasoning")));
        assert!(!rows.iter().any(|row| row.contains("autopilot")));
        assert!(!rows.iter().any(|row| row.contains("tools 7/7")));
    }

    #[test]
    fn hostile_display_surfaces_never_write_controls_to_the_backend() {
        const HOSTILE: &str = "hostile \u{1b}[31mred\u{1b}]0;title\u{07} bell\u{009b}2J";

        let assert_safe = |surface: &str, app: &App| {
            let mut terminal = Terminal::new(TestBackend::new(160, 30)).expect("test terminal");
            terminal
                .draw(|frame| draw(frame, app))
                .expect("surface should render");
            assert!(
                terminal.backend().buffer().content().iter().all(|cell| {
                    cell.symbol().chars().all(|character| {
                        !character.is_control() && !(('\u{0080}'..='\u{009f}').contains(&character))
                    })
                }),
                "surface {surface} wrote a prohibited control"
            );
        };

        let mut status = App::new_with_working_directory(
            Some(HOSTILE.to_string()),
            &PathBuf::from(format!("C:\\project\\{HOSTILE}")),
        );
        status.set_reasoning_effort(Some(HOSTILE.to_string()));
        assert_safe("status", &status);

        let mut sessions = App::new(None);
        sessions.set_sessions(vec![SessionMetadata {
            session_id: SessionId::from(HOSTILE),
            start_time: HOSTILE.to_string(),
            modified_time: HOSTILE.to_string(),
            summary: Some(HOSTILE.to_string()),
            is_remote: false,
        }]);
        assert_safe("sessions", &sessions);

        let mut models = App::new(None);
        models.set_models(vec![Model {
            id: HOSTILE.to_string(),
            name: HOSTILE.to_string(),
            default_reasoning_effort: Some(HOSTILE.to_string()),
            supported_reasoning_efforts: Some(vec![HOSTILE.to_string()]),
            supported_context_tiers: Some(vec![HOSTILE.to_string()]),
            ..Model::default()
        }]);
        assert_safe("models", &models);

        let mut usage = App::new(None);
        usage.set_usage(
            UsageMetricsSnapshot {
                total_nano_aiu: Some(1.0),
                total_premium_request_cost: 2.0,
                total_user_requests: 3,
                total_api_duration_ms: 4,
                current_model: Some(HOSTILE.to_string()),
            },
            Some(ContextAttributionSnapshot {
                model_id: HOSTILE.to_string(),
                total_tokens: 10,
                prompt_token_limit: 20,
                categories: vec![ContextCategorySnapshot {
                    label: HOSTILE.to_string(),
                    tokens: 10,
                }],
                compactions: 1,
            }),
        );
        assert_safe("usage", &usage);

        let mut todos = App::new(None);
        todos.set_fleet_active(true);
        todos.set_todos(TodoSnapshot {
            rows: vec![TodoRowSnapshot {
                id: "todo-1".to_string(),
                title: HOSTILE.to_string(),
                description: HOSTILE.to_string(),
                status: HOSTILE.to_string(),
            }],
            dependencies: vec![TodoDependencySnapshot {
                todo_id: "todo-1".to_string(),
                depends_on: HOSTILE.to_string(),
            }],
        });
        assert_safe("todos", &todos);

        let skill_root = SkillRoot {
            path: PathBuf::from(HOSTILE),
            source: SkillRootSource::Project,
        };
        let catalog = SkillCatalog::from_parts(
            vec![skill_root.clone()],
            vec![Skill {
                name: HOSTILE.to_string(),
                description: HOSTILE.to_string(),
                user_invocable: true,
                directory: PathBuf::from(format!("{HOSTILE}\\skill")),
                root: skill_root,
            }],
            Vec::new(),
        );
        let mut skills = App::new(None);
        skills.set_skill_catalog(catalog.clone());
        skills.open_skill_picker();
        assert_safe("skills", &skills);

        let mut completion = App::new(None);
        completion.set_skill_catalog(catalog);
        completion.push_input('/');
        assert_safe("completion", &completion);

        let (respond_to, _response) = tokio::sync::oneshot::channel();
        let mut approval = App::new(None);
        approval.enqueue_approval(ApprovalRequest {
            category: ApprovalCategory::Shell,
            tool_name: HOSTILE.to_string(),
            details: HOSTILE.to_string(),
            respond_to,
        });
        assert_safe("approval", &approval);
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
    fn tool_picker_renders_inline_checkbox_state() {
        let mut app = App::new(None);
        app.set_toolset(Toolset::shell_only());
        app.open_tool_picker();
        let mut terminal = Terminal::new(TestBackend::new(80, 12)).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &app))
            .expect("tool picker should render");

        let rendered = (0..terminal.backend().buffer().area.height)
            .map(|y| {
                (0..terminal.backend().buffer().area.width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rendered.iter().any(|line| line.contains("[✓]")));
        assert!(rendered.iter().any(|line| line.contains("[ ]")));
        assert!(rendered
            .iter()
            .any(|line| line.contains("powershell") || line.contains("bash")));
        assert!(!rendered.iter().any(|line| line.contains('┌')));
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
    fn skill_picker_renders_inline_description() {
        let mut app = App::new(None);
        app.set_skill_catalog(test_skill_catalog());
        app.open_skill_picker();
        let mut terminal = Terminal::new(TestBackend::new(100, 16)).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &app))
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
        assert!(!rendered.iter().any(|line| line.contains('┌')));
    }

    #[test]
    fn slash_completion_filters_invocable_skills_and_submits_with_enter() {
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
            UiAction::Send("/r".to_string())
        );
        assert!(app.input().is_empty());
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
        let lines = super::status_detail_lines(&app);
        assert!(lines
            .iter()
            .any(|line| line.to_string().contains("Skills: 1 enabled, 3 disabled")));

        app.reset_for_new_conversation();
        assert!(app.skill_selection().is_empty());
    }

    #[test]
    fn status_block_displays_the_selected_tool_count() {
        let mut app = App::new(None);
        app.set_toolset(Toolset::shell_only());
        let lines = super::status_detail_lines(&app);
        assert!(lines
            .iter()
            .any(|line| line.to_string().contains("Tools: 1 enabled, 6 disabled")));
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
        app.close_picker();

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
    fn approval_picker_blocks_tool_replacement_and_busy_tool_apply_stays_guarded() {
        let mut app = App::new(None);
        let (respond_to, _response) = tokio::sync::oneshot::channel();
        app.enqueue_approval(crate::permissions::ApprovalRequest {
            category: crate::permissions::ApprovalCategory::Shell,
            tool_name: "bash".to_string(),
            details: "pwd".to_string(),
            respond_to,
        });

        assert_eq!(handle_key(&mut app, ctrl_key('k')), UiAction::None);
        assert!(app.picker_is_open());
        assert_eq!(
            app.pending_approval()
                .expect("approval should remain at the front")
                .tool_name,
            "bash"
        );
        app.open_tool_picker();
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::Approval(crate::permissions::ApprovalDecision::ApproveOnce)
        );
        let request = app
            .resolve_approval(crate::permissions::ApprovalDecision::ApproveOnce)
            .expect("approval should resolve from its own picker");
        let _ = request
            .respond_to
            .send(crate::permissions::ApprovalDecision::ApproveOnce);
        assert!(!app.picker_is_open());

        let mut busy = App::new(None);
        busy.status.busy = true;
        busy.open_tool_picker();
        assert!(matches!(
            handle_key(&mut busy, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::ApplyToolset(_)
        ));
        assert!(busy.toolset_change_is_blocked());
    }

    #[test]
    fn session_navigation_stays_local_and_picker_selection_emits_actions() {
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
            handle_key(&mut app, key(KeyCode::Left, KeyEventKind::Press)),
            UiAction::None
        );
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Right, KeyEventKind::Press)),
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
    fn session_resume_selection_has_no_picker_trace_before_history_replacement() {
        let mut app = App::new(None);
        app.set_sessions(vec![SessionMetadata {
            session_id: SessionId::from("session-1"),
            start_time: "2026-08-31T12:00:00Z".to_string(),
            modified_time: "2026-08-31T12:01:00Z".to_string(),
            summary: Some("first".to_string()),
            is_remote: false,
        }]);

        assert_eq!(
            handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::Resume(SessionId::from("session-1"))
        );
        assert!(app.entries().is_empty());
    }

    #[test]
    fn model_picker_uses_only_arrow_keys_for_option_adjustment() {
        let mut app = App::new(None);
        app.set_models(vec![Model {
            id: "gpt-5".to_string(),
            name: "GPT-5".to_string(),
            supported_context_tiers: Some(vec!["default".to_string()]),
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
                reasoning_effort: None,
                context_tier: None,
            })
        );

        app.set_models(vec![Model {
            id: "gpt-5".to_string(),
            name: "GPT-5".to_string(),
            supported_context_tiers: Some(vec!["default".to_string()]),
            supported_reasoning_efforts: Some(vec!["low".to_string(), "high".to_string()]),
            ..Model::default()
        }]);
        handle_key(&mut app, key(KeyCode::Left, KeyEventKind::Press));
        handle_key(&mut app, key(KeyCode::Right, KeyEventKind::Press));
        assert!(matches!(
            handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::SwitchModel(ModelSelection {
                reasoning_effort: Some(_),
                context_tier: Some(_),
                ..
            })
        ));
    }

    #[test]
    fn picker_cancellation_commits_a_truthful_local_outcome() {
        let mut app = App::new(Some("gpt-5".to_string()));
        app.set_models(vec![Model {
            id: "gpt-5".to_string(),
            name: "GPT-5".to_string(),
            ..Model::default()
        }]);

        assert_eq!(
            handle_key(&mut app, key(KeyCode::Esc, KeyEventKind::Press)),
            UiAction::None
        );
        assert!(matches!(
            app.entries().last(),
            Some(ChatEntry::LocalOutput(lines))
                if lines.iter().any(|line| line.spans.iter().any(|span| span.content.contains("Kept model as")))
        ));
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

        let mut terminal = Terminal::new(TestBackend::new(100, 12)).expect("test terminal");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("model picker should render");
        let rendered = (0..terminal.backend().buffer().area.height)
            .map(|y| {
                (0..terminal.backend().buffer().area.width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rendered.iter().any(|line| line.contains("local")));
        assert!(rendered.iter().any(|line| line.contains("unknown tokens")));
        assert!(!rendered.iter().any(|line| line.contains('┌')));

        handle_key(&mut app, key(KeyCode::Left, KeyEventKind::Press));
        handle_key(&mut app, key(KeyCode::Right, KeyEventKind::Press));
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
    fn model_picker_opens_on_the_active_model() {
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
        let mut terminal = Terminal::new(TestBackend::new(100, 12)).expect("test terminal");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("model picker should render");
        let rendered = (0..terminal.backend().buffer().area.height)
            .map(|y| {
                (0..terminal.backend().buffer().area.width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rendered.iter().any(|line| line.contains("GPT-5")));
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
            .draw(|frame| draw(frame, &app))
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
        assert!(rendered.iter().any(|line| line.contains("128,000 tokens")));
        assert!(rendered.iter().any(|line| line.contains("200,000 tokens")));
        assert!(!rendered.iter().any(|line| line.contains('┌')));
    }

    #[test]
    fn usage_key_requests_a_usage_refresh() {
        let mut app = App::new(None);

        assert_eq!(handle_key(&mut app, ctrl_key('u')), UiAction::LoadUsage);
    }

    #[test]
    fn exact_status_and_usage_commands_are_local_actions() {
        let mut status = App::new(None);
        for character in "/status".chars() {
            status.push_input(character);
        }
        assert_eq!(
            handle_key(&mut status, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::LoadStatus
        );

        let mut usage = App::new(None);
        for character in "/usage".chars() {
            usage.push_input(character);
        }
        assert_eq!(
            handle_key(&mut usage, key(KeyCode::Enter, KeyEventKind::Press)),
            UiAction::LoadUsageCommand
        );
    }

    #[test]
    fn local_commands_reject_extra_arguments_without_becoming_sdk_prompts() {
        for (input, command) in [("/status extra", "/status"), ("/usage\tmore", "/usage")] {
            let mut app = App::new(None);
            for character in input.chars() {
                app.push_input(character);
            }

            assert_eq!(
                handle_key(&mut app, key(KeyCode::Enter, KeyEventKind::Press)),
                UiAction::LocalCommandError(format!(
                    "{command} does not accept arguments. Use {command} without arguments."
                ))
            );
        }
    }

    #[test]
    fn status_lines_use_full_identity_and_colored_count_fields() {
        let mut app = App::new_with_working_directory(
            Some("gpt-5".to_string()),
            Path::new("C:\\dev\\picopilot"),
        );
        app.set_session_id("session-123");
        app.set_toolset(crate::toolset::Toolset::shell_only());
        app.set_skill_catalog(long_skill_catalog(3));
        app.set_skill_selection(SkillSelection::from_names(&app.skill_catalog, ["skill-0"]));

        let lines = super::status_detail_lines(&app);
        let rendered = lines.iter().map(ToString::to_string).collect::<Vec<_>>();

        assert_eq!(
            rendered[0],
            format!("Version: {}", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(rendered[1], "Session ID: session-123");
        assert_eq!(rendered[2], "cwd: C:\\dev\\picopilot");
        assert_eq!(rendered[3], "");
        assert_eq!(rendered[4], "Model: gpt-5");
        assert!(rendered[5].contains("Tools: "));
        assert!(rendered[5].contains("· /tools"));
        assert_eq!(rendered[6], "Skills: 1 enabled, 2 disabled · /skills");
        assert!(!rendered.iter().any(|line| line.contains("reasoning")));
        assert!(!rendered.iter().any(|line| line.contains("cost")));
        assert!(lines[5].spans.iter().any(|span| {
            span.content.contains("enabled") && span.style.fg == Some(palette::SUCCESS)
        }));
        assert!(lines[5].spans.iter().any(|span| {
            span.content.contains("disabled") && span.style.fg == Some(palette::INACTIVE)
        }));
    }

    #[test]
    fn empty_local_output_keeps_the_dim_five_cell_gutter() {
        let lines = super::local_output_lines(&[]);

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].to_string(), "  ⎿  (no content)");
        assert_eq!(lines[0].spans[0].content, "  ⎿  ");
        assert_eq!(lines[0].spans[0].style.fg, Some(palette::INACTIVE));
        assert!(lines[0].spans[0].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(lines[0].spans[1].style.fg, Some(palette::INACTIVE));
    }

    #[test]
    fn context_warning_uses_the_twenty_thousand_token_threshold_and_safe_rounding() {
        let mut app = App::new(None);

        app.apply(EventUpdate::Usage(UsageSnapshot {
            current_tokens: 79_999,
            token_limit: 100_000,
            messages: 0,
            conversation_tokens: None,
            system_tokens: None,
            tool_definitions_tokens: None,
        }));
        assert_eq!(super::context_warning_text(&app), None);

        app.apply(EventUpdate::Usage(UsageSnapshot {
            current_tokens: 80_000,
            token_limit: 100_000,
            messages: 0,
            conversation_tokens: None,
            system_tokens: None,
            tool_definitions_tokens: None,
        }));
        assert_eq!(
            super::context_warning_text(&app).as_deref(),
            Some("20% until auto-compact")
        );

        app.apply(EventUpdate::Usage(UsageSnapshot {
            current_tokens: 150_001,
            token_limit: 100_000,
            messages: 0,
            conversation_tokens: None,
            system_tokens: None,
            tool_definitions_tokens: None,
        }));
        assert_eq!(
            super::context_warning_text(&app).as_deref(),
            Some("0% until auto-compact")
        );

        for token_limit in [0, -1] {
            app.apply(EventUpdate::Usage(UsageSnapshot {
                current_tokens: token_limit,
                token_limit,
                messages: 0,
                conversation_tokens: None,
                system_tokens: None,
                tool_definitions_tokens: None,
            }));
            assert_eq!(super::context_warning_text(&app), None);
        }
    }

    #[test]
    fn compaction_suppresses_warning_until_the_next_user_turn() {
        let mut app = App::new(None);
        app.apply(EventUpdate::Usage(UsageSnapshot {
            current_tokens: 90_000,
            token_limit: 100_000,
            messages: 0,
            conversation_tokens: None,
            system_tokens: None,
            tool_definitions_tokens: None,
        }));
        let context = |compactions| ContextAttributionSnapshot {
            model_id: "gpt-5".to_string(),
            total_tokens: 90_000,
            prompt_token_limit: 100_000,
            categories: Vec::new(),
            compactions,
        };
        let metrics = UsageMetricsSnapshot {
            total_nano_aiu: None,
            total_premium_request_cost: 0.0,
            total_user_requests: 0,
            total_api_duration_ms: 0,
            current_model: Some("gpt-5".to_string()),
        };

        app.set_usage_snapshot(metrics.clone(), Some(context(1)));
        assert_eq!(
            super::context_warning_text(&app).as_deref(),
            Some("10% until auto-compact")
        );
        app.set_usage_snapshot(metrics.clone(), Some(context(2)));
        assert_eq!(super::context_warning_text(&app), None);
        app.set_usage_snapshot(metrics, Some(context(2)));
        assert_eq!(super::context_warning_text(&app), None);

        app.add_user_message("next turn".to_string());
        assert_eq!(
            super::context_warning_text(&app).as_deref(),
            Some("10% until auto-compact")
        );
    }

    fn app_with_context_warning() -> App {
        let mut app = App::new(None);
        app.apply(EventUpdate::Usage(UsageSnapshot {
            current_tokens: 90_000,
            token_limit: 100_000,
            messages: 0,
            conversation_tokens: None,
            system_tokens: None,
            tool_definitions_tokens: None,
        }));
        app
    }

    #[test]
    fn context_warning_is_right_aligned_wide_and_stacked_narrow() {
        let wide_rows = rendered_rows(&app_with_context_warning(), 100, 14);
        let wide_warning = wide_rows
            .iter()
            .find(|row| row.contains("10% until auto-compact"))
            .expect("wide warning should render");
        assert!(wide_warning.ends_with("10% until auto-compact  "));
        assert!(wide_warning.find("10%").unwrap_or_default() > 60);

        let narrow_rows = rendered_rows(&app_with_context_warning(), 60, 14);
        let narrow_warning_index = narrow_rows
            .iter()
            .position(|row| row.contains("10% until auto-compact"))
            .expect("narrow warning should render");
        assert!(narrow_warning_index > 0);
        assert!(narrow_rows[narrow_warning_index - 1].contains("? for shortcuts"));
        assert!(narrow_rows[narrow_warning_index].starts_with("  "));
    }

    #[test]
    fn context_warning_yields_to_completion_and_picker_surfaces() {
        let mut completion = app_with_context_warning();
        completion.push_input('/');
        let completion_rows = rendered_rows(&completion, 100, 14);
        assert!(completion_rows.iter().any(|row| row.contains("/status")));
        assert!(!completion_rows
            .iter()
            .any(|row| row.contains("until auto-compact")));

        let mut picker = app_with_context_warning();
        picker.open_tool_picker();
        let picker_rows = rendered_rows(&picker, 100, 14);
        assert!(!picker_rows
            .iter()
            .any(|row| row.contains("until auto-compact")));
    }

    #[test]
    fn narrow_footer_drops_optional_warning_before_three_row_input_minimum() {
        let app = app_with_context_warning();
        let layout = super::prompt_layout(&app, ratatui::layout::Rect::new(0, 0, 60, 8));

        assert_eq!(layout.input_rows, 3);
        assert_eq!(layout.footer_rows, 1);
        let rows = rendered_rows(&app, 60, 8);
        assert!(!rows.iter().any(|row| row.contains("until auto-compact")));
    }

    #[test]
    fn usage_metrics_are_static_transcript_output() {
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

        assert!(!app.modal_is_open());
        assert_eq!(
            app.status()
                .usage_metrics
                .as_ref()
                .and_then(|metrics| metrics.total_nano_aiu),
            Some(3.5)
        );
        assert_eq!(handle_key(&mut app, ctrl_key('u')), UiAction::LoadUsage);
        assert!(!app.modal_is_open());
    }

    #[test]
    fn usage_transcript_echoes_once_and_commits_one_immutable_output_block() {
        let mut app = App::new(None);
        app.apply(EventUpdate::Usage(UsageSnapshot {
            current_tokens: 12_345,
            token_limit: 100_000,
            messages: 0,
            conversation_tokens: None,
            system_tokens: None,
            tool_definitions_tokens: None,
        }));
        app.set_usage(
            UsageMetricsSnapshot {
                total_nano_aiu: Some(3.5),
                total_premium_request_cost: 2.0,
                total_user_requests: 4,
                total_api_duration_ms: 1250,
                current_model: Some("gpt-5".to_string()),
            },
            None,
        );

        assert_eq!(app.entries().len(), 2);
        assert_eq!(app.entries()[0], ChatEntry::User("/usage".to_string()));
        let output = app.entries()[1].clone();
        assert!(matches!(output, ChatEntry::LocalOutput(_)));

        app.set_usage_metrics(UsageMetricsSnapshot {
            total_nano_aiu: Some(9.0),
            total_premium_request_cost: 4.0,
            total_user_requests: 8,
            total_api_duration_ms: 2500,
            current_model: Some("gpt-5".to_string()),
        });
        assert_eq!(app.entries()[1], output);
    }

    #[test]
    fn usage_body_is_dimmed_without_losing_context_attribution_fields() {
        let mut app = App::new(None);
        app.apply(EventUpdate::Usage(UsageSnapshot {
            current_tokens: 12_345,
            token_limit: 100_000,
            messages: 0,
            conversation_tokens: None,
            system_tokens: None,
            tool_definitions_tokens: None,
        }));
        app.set_usage_snapshot(
            UsageMetricsSnapshot {
                total_nano_aiu: Some(3.5),
                total_premium_request_cost: 2.0,
                total_user_requests: 4,
                total_api_duration_ms: 1250,
                current_model: Some("gpt-5".to_string()),
            },
            Some(ContextAttributionSnapshot {
                model_id: "gpt-5".to_string(),
                total_tokens: 12_345,
                prompt_token_limit: 100_000,
                categories: vec![ContextCategorySnapshot {
                    label: "Messages".to_string(),
                    tokens: 12_345,
                }],
                compactions: 0,
            }),
        );

        let lines = super::usage_detail_lines(&app);
        let rendered = lines.iter().map(ToString::to_string).collect::<Vec<_>>();

        assert!(rendered.iter().any(|line| line.contains("Session cost:")));
        assert!(rendered
            .iter()
            .any(|line| line.contains("Context window: 12,345 / 100,000 tokens")));
        assert!(rendered.iter().any(|line| line.contains("Attribution:")));
        assert!(rendered.iter().any(|line| line.contains("Compactions: 0")));
        assert!(lines[0].spans[0].style.fg == Some(palette::INACTIVE));
        assert!(lines[0].spans[0].style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn status_cost_updates_without_opening_a_picker() {
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
    fn todo_visibility_is_only_available_for_an_active_fleet() {
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
        assert!(!app.modal_is_open());

        app.apply(EventUpdate::TodosChanged);
        assert!(app.take_todo_refresh_request());
        assert!(!app.take_todo_refresh_request());

        assert_eq!(handle_key(&mut app, ctrl_key('t')), UiAction::None);
        assert!(!app.show_todos);
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
    fn main_window_renders_a_borderless_transcript_and_prompt_footer() {
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
        assert!(row(29).starts_with("  ? for shortcuts"));
        assert!(!row(29).contains("^N"));
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
            shell_completion: None,
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
