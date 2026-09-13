//! Interactive first-run / `--configure` setup wizard (spec §3).
//!
//! The module is split into two halves on purpose:
//!
//! - [`WizardState`] and its transition methods are pure — no terminal I/O, no clock, no network
//!   — so the screen sequence, inline validation, and the `default_provider` "most-recently-wins"
//!   tie-break (spec §3.5) can be unit tested without a real terminal.
//! - [`run_setup_wizard`] is the thin terminal driver: it reuses `tui.rs`'s low-level raw-mode /
//!   main-screen primitives and its `ratatui` rendering technique (real widgets drawn via
//!   `Terminal::draw`, not hand-rolled `write!`/`execute!` line printing), reads key events, and
//!   (for the Copilot-connect screen) drives the real async Copilot client startup — but it
//!   deliberately does not reuse `App`'s full draw loop or state machine (out of scope per the
//!   ticket).

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use crossterm::event::{Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use crate::config::AppConfig;
use crate::provider::{normalize_base_url, validate_provider_name};
use crate::provider_config::{
    self, CopilotDefaults, ProviderConfigFile, ProviderIdentity, ProviderProfile,
    RESERVED_COPILOT_PROVIDER_NAME,
};
use crate::screen_model::{enter_main_screen, restore_main_screen, terminal_options};

// ============================================================================
// Pure state machine — unit-testable without a terminal.
// ============================================================================

/// Which screen the wizard is currently showing (spec §3.1-§3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Entry,
    PickProviderPreset,
    AddProvider,
    ConnectingCopilot,
    Finish,
}

/// The entry screen's three options (spec §3.1). "Finish" is unselectable/disabled until
/// [`WizardState::can_finish`] is true.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntrySelection {
    ConnectCopilot,
    AddProvider,
    Finish,
}

const ENTRY_OPTIONS: [EntrySelection; 3] = [
    EntrySelection::ConnectCopilot,
    EntrySelection::AddProvider,
    EntrySelection::Finish,
];

/// Which field of the add-a-provider form is focused (spec §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    BaseUrl,
    WireApi,
    ApiKey,
}

const FIELD_ORDER: [Field; 4] = [Field::Name, Field::BaseUrl, Field::WireApi, Field::ApiKey];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireApiChoice {
    Completions,
    Responses,
}

impl WireApiChoice {
    fn as_str(self) -> &'static str {
        match self {
            Self::Completions => "completions",
            Self::Responses => "responses",
        }
    }

    fn toggled(self) -> Self {
        match self {
            Self::Completions => Self::Responses,
            Self::Responses => Self::Completions,
        }
    }
}

/// One well-known OpenAI-API-compatible provider offered on the provider-picker screen, plus a
/// trailing "Custom" entry (`name`/`base_url` both `None`) that goes straight to a blank form.
/// All of the named presets speak the OpenAI `/v1/chat/completions` + `/v1/models` shape, so none
/// of them need to touch `wire_api` — it keeps its `completions` default either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderPreset {
    pub display_name: &'static str,
    pub name: Option<&'static str>,
    pub base_url: Option<&'static str>,
}

/// The provider-picker screen's options: 8 well-known presets (researched against each provider's
/// docs) followed by "Custom", which pre-fills nothing.
pub const PROVIDER_PRESETS: [ProviderPreset; 9] = [
    ProviderPreset {
        display_name: "OpenRouter",
        name: Some("openrouter"),
        base_url: Some("https://openrouter.ai/api/v1"),
    },
    ProviderPreset {
        display_name: "Groq",
        name: Some("groq"),
        base_url: Some("https://api.groq.com/openai/v1"),
    },
    ProviderPreset {
        display_name: "Together AI",
        name: Some("together"),
        base_url: Some("https://api.together.xyz/v1"),
    },
    ProviderPreset {
        display_name: "Mistral AI",
        name: Some("mistral"),
        base_url: Some("https://api.mistral.ai/v1"),
    },
    ProviderPreset {
        display_name: "DeepSeek",
        name: Some("deepseek"),
        base_url: Some("https://api.deepseek.com/v1"),
    },
    ProviderPreset {
        display_name: "Ollama (local)",
        name: Some("ollama"),
        base_url: Some("http://localhost:11434/v1"),
    },
    ProviderPreset {
        display_name: "LM Studio (local)",
        name: Some("lmstudio"),
        base_url: Some("http://localhost:1234/v1"),
    },
    ProviderPreset {
        display_name: "vLLM (local)",
        name: Some("vllm"),
        base_url: Some("http://localhost:8000/v1"),
    },
    ProviderPreset {
        display_name: "Custom",
        name: None,
        base_url: None,
    },
];

/// The in-progress add-a-provider form (spec §3.2). `wire_api` defaults to `completions`,
/// pre-selected, per spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub name: String,
    pub base_url: String,
    pub wire_api: WireApiChoice,
    pub api_key: String,
    pub focus: Field,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            name: String::new(),
            base_url: String::new(),
            wire_api: WireApiChoice::Completions,
            api_key: String::new(),
            focus: Field::Name,
        }
    }
}

/// Inline validation errors under the Name/Base URL fields (spec §3.7). Cleared as soon as that
/// field is edited again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldErrors {
    pub name: Option<String>,
    pub base_url: Option<String>,
}

/// What the terminal driver should do after a key/event is handled by the pure state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardOutcome {
    /// Nothing state-changing happened (or state changed but no persistence/exit is needed yet).
    Continue,
    /// Esc was pressed on the entry screen: quit without saving (spec §3.1).
    Quit,
    /// The user selected "Connect to GitHub Copilot" and it wasn't already connected: the driver
    /// must now actually start the Copilot client (spec §3.3).
    ConnectCopilot,
    /// A provider profile (or a Copilot connection) was just saved: the driver must persist the
    /// config file now (spec §3.7 item 7).
    Persist,
    /// "Finish" was chosen: the driver must persist once more and end the wizard (spec §3.4).
    Finished,
}

/// The wizard's full in-progress state (spec §3). Pure and cheaply cloneable, with no I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardState {
    screen: Screen,
    entry_selection: EntrySelection,
    providers: BTreeMap<String, ProviderProfile>,
    copilot_connected: bool,
    copilot_defaults: Option<CopilotDefaults>,
    default_provider: Option<String>,
    draft: Draft,
    errors: FieldErrors,
    connect_error: Option<String>,
    preset_selection: usize,
}

impl WizardState {
    /// A genuine first run: nothing configured yet.
    pub fn new() -> Self {
        Self {
            screen: Screen::Entry,
            entry_selection: EntrySelection::ConnectCopilot,
            providers: BTreeMap::new(),
            copilot_connected: false,
            copilot_defaults: None,
            default_provider: None,
            draft: Draft::default(),
            errors: FieldErrors::default(),
            connect_error: None,
            preset_selection: 0,
        }
    }

    /// Seeds the entry screen's "Configured so far" list from an existing config file, as if
    /// everything in it had been added earlier in this same pass (spec §3.6, `--configure`).
    pub fn seeded_from(config: &ProviderConfigFile) -> Self {
        let mut state = Self::new();
        state.providers = config.providers.clone();
        state.copilot_connected = config.copilot.is_some();
        state.copilot_defaults = config.copilot.clone();
        state.default_provider = Some(config.default_provider.clone());
        state
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn entry_selection(&self) -> EntrySelection {
        self.entry_selection
    }

    pub fn providers(&self) -> &BTreeMap<String, ProviderProfile> {
        &self.providers
    }

    pub fn copilot_connected(&self) -> bool {
        self.copilot_connected
    }

    pub fn default_provider(&self) -> Option<&str> {
        self.default_provider.as_deref()
    }

    pub fn draft(&self) -> &Draft {
        &self.draft
    }

    pub fn errors(&self) -> &FieldErrors {
        &self.errors
    }

    pub fn connect_error(&self) -> Option<&str> {
        self.connect_error.as_deref()
    }

    /// The currently highlighted row on the provider-picker screen (an index into
    /// [`PROVIDER_PRESETS`]).
    pub fn preset_selection(&self) -> usize {
        self.preset_selection
    }

    /// "Finish" only becomes selectable once at least one provider or Copilot is configured this
    /// pass (spec §3.1).
    pub fn can_finish(&self) -> bool {
        !self.providers.is_empty() || self.copilot_connected
    }

    /// Builds the [`ProviderConfigFile`] to persist from the current state (spec §5.5). Falls back
    /// to `"copilot"` if nothing has set `default_provider` yet — unreachable in the real flow
    /// (Finish requires [`Self::can_finish`], which always implies a `default_provider` was set),
    /// but keeps this function total.
    pub fn to_config_file(&self) -> ProviderConfigFile {
        let default_provider = self
            .default_provider
            .clone()
            .unwrap_or_else(|| RESERVED_COPILOT_PROVIDER_NAME.to_string());
        let mut config = ProviderConfigFile::new(default_provider);
        config.providers = self.providers.clone();
        config.copilot = self
            .copilot_connected
            .then(|| self.copilot_defaults.clone().unwrap_or_default());
        config
    }

    // -- Entry screen transitions --------------------------------------------------------------

    pub fn move_entry_selection(&mut self, delta: i32) {
        let current_index = ENTRY_OPTIONS
            .iter()
            .position(|option| *option == self.entry_selection)
            .unwrap_or(0) as i32;
        let last_index = ENTRY_OPTIONS.len() as i32 - 1;
        let new_index = (current_index + delta).clamp(0, last_index) as usize;
        self.entry_selection = ENTRY_OPTIONS[new_index];
    }

    /// Handles a key press on the entry screen, returning what the driver should do next.
    pub fn handle_entry_key(&mut self, key: KeyCode) -> WizardOutcome {
        match key {
            KeyCode::Up => {
                self.move_entry_selection(-1);
                WizardOutcome::Continue
            }
            KeyCode::Down => {
                self.move_entry_selection(1);
                WizardOutcome::Continue
            }
            KeyCode::Esc => WizardOutcome::Quit,
            KeyCode::Enter => self.activate_entry_selection(),
            _ => WizardOutcome::Continue,
        }
    }

    fn activate_entry_selection(&mut self) -> WizardOutcome {
        match self.entry_selection {
            EntrySelection::ConnectCopilot => {
                if self.copilot_connected {
                    // Already connected this pass — nothing to do (mirrors the prototype).
                    WizardOutcome::Continue
                } else {
                    self.connect_error = None;
                    self.screen = Screen::ConnectingCopilot;
                    WizardOutcome::ConnectCopilot
                }
            }
            EntrySelection::AddProvider => {
                self.go_to_pick_provider_preset();
                WizardOutcome::Continue
            }
            EntrySelection::Finish => {
                if self.can_finish() {
                    self.screen = Screen::Finish;
                    WizardOutcome::Finished
                } else {
                    WizardOutcome::Continue
                }
            }
        }
    }

    // -- Provider-picker sub-flow (in front of the add-a-provider form) -------------------------

    /// Enters the provider-picker screen, always starting on the first preset row.
    pub fn go_to_pick_provider_preset(&mut self) {
        self.screen = Screen::PickProviderPreset;
        self.preset_selection = 0;
    }

    pub fn move_preset_selection(&mut self, delta: i32) {
        let last_index = PROVIDER_PRESETS.len() as i32 - 1;
        let new_index = (self.preset_selection as i32 + delta).clamp(0, last_index) as usize;
        self.preset_selection = new_index;
    }

    /// Handles a key press on the provider-picker screen.
    pub fn handle_pick_preset_key(&mut self, key: KeyCode) -> WizardOutcome {
        match key {
            KeyCode::Up => {
                self.move_preset_selection(-1);
                WizardOutcome::Continue
            }
            KeyCode::Down => {
                self.move_preset_selection(1);
                WizardOutcome::Continue
            }
            KeyCode::Esc => {
                self.screen = Screen::Entry;
                WizardOutcome::Continue
            }
            KeyCode::Enter => {
                self.select_preset(self.preset_selection);
                WizardOutcome::Continue
            }
            _ => WizardOutcome::Continue,
        }
    }

    /// Applies `PROVIDER_PRESETS[index]` (pre-filling `draft.name`/`draft.base_url` through the
    /// same `Draft` the blank form uses — a preset is just a starting point, not a separate
    /// representation) and transitions to `Screen::AddProvider`. Focus starts on `Field::Name`:
    /// the pre-filled name is a *suggestion* (e.g. two OpenRouter accounts would collide on
    /// "openrouter" already being taken this pass), so the field the user is most likely to want
    /// to change first should be the one already focused. Selecting "Custom" (`name`/`base_url`
    /// both `None`) behaves exactly like today's blank `go_to_add_provider`.
    fn select_preset(&mut self, index: usize) {
        let preset = PROVIDER_PRESETS
            .get(index)
            .copied()
            .unwrap_or(PROVIDER_PRESETS[PROVIDER_PRESETS.len() - 1]);
        self.go_to_add_provider();
        self.draft.name = preset.name.unwrap_or_default().to_string();
        self.draft.base_url = preset.base_url.unwrap_or_default().to_string();
        self.draft.focus = Field::Name;
    }

    // -- Add-a-provider sub-flow ----------------------------------------------------------------

    pub fn go_to_add_provider(&mut self) {
        self.screen = Screen::AddProvider;
        self.draft = Draft::default();
        self.errors = FieldErrors::default();
    }

    fn cancel_add_provider(&mut self) {
        self.screen = Screen::Entry;
        self.errors = FieldErrors::default();
    }

    fn advance_focus(&mut self) {
        let current_index = FIELD_ORDER
            .iter()
            .position(|field| *field == self.draft.focus)
            .unwrap_or(0);
        self.draft.focus = FIELD_ORDER[(current_index + 1) % FIELD_ORDER.len()];
    }

    fn push_char(&mut self, character: char) {
        match self.draft.focus {
            Field::Name => {
                self.draft.name.push(character);
                self.errors.name = None;
            }
            Field::BaseUrl => {
                self.draft.base_url.push(character);
                self.errors.base_url = None;
            }
            Field::ApiKey => self.draft.api_key.push(character),
            Field::WireApi => {}
        }
    }

    fn backspace(&mut self) {
        match self.draft.focus {
            Field::Name => {
                self.draft.name.pop();
                self.errors.name = None;
            }
            Field::BaseUrl => {
                self.draft.base_url.pop();
                self.errors.base_url = None;
            }
            Field::ApiKey => {
                self.draft.api_key.pop();
            }
            Field::WireApi => {}
        }
    }

    fn toggle_wire_api(&mut self) {
        self.draft.wire_api = self.draft.wire_api.toggled();
    }

    /// Handles a key press on the add-a-provider screen.
    pub fn handle_add_provider_key(&mut self, key: KeyCode) -> WizardOutcome {
        match key {
            KeyCode::Esc => {
                self.cancel_add_provider();
                WizardOutcome::Continue
            }
            KeyCode::Tab => {
                self.advance_focus();
                WizardOutcome::Continue
            }
            KeyCode::Left | KeyCode::Right if self.draft.focus == Field::WireApi => {
                self.toggle_wire_api();
                WizardOutcome::Continue
            }
            KeyCode::Char(' ') if self.draft.focus == Field::WireApi => {
                self.toggle_wire_api();
                WizardOutcome::Continue
            }
            KeyCode::Enter => {
                if self.draft.focus == Field::WireApi {
                    self.toggle_wire_api();
                    WizardOutcome::Continue
                } else if self.submit_provider_form() {
                    WizardOutcome::Persist
                } else {
                    WizardOutcome::Continue
                }
            }
            KeyCode::Backspace => {
                self.backspace();
                WizardOutcome::Continue
            }
            KeyCode::Char(character) => {
                self.push_char(character);
                WizardOutcome::Continue
            }
            _ => WizardOutcome::Continue,
        }
    }

    /// Validates and, if valid, saves the draft as a new provider profile, sets it as
    /// `default_provider` (spec §3.5's "last configured wins"), and returns to the entry screen.
    /// Returns `false` (leaving inline errors set) on a failed submit (spec §3.7).
    pub fn submit_provider_form(&mut self) -> bool {
        let name_result = validate_name(&self.draft.name, &self.providers);
        let url_result = validate_base_url(&self.draft.base_url);

        let (name, base_url) = match (&name_result, &url_result) {
            (Ok(name), Ok(base_url)) => (name.clone(), base_url.clone()),
            _ => {
                self.errors = FieldErrors {
                    name: name_result.err(),
                    base_url: url_result.err(),
                };
                return false;
            }
        };

        let mut profile = ProviderProfile::new(base_url);
        profile.wire_api = self.draft.wire_api.as_str().to_string();
        let api_key = self.draft.api_key.trim();
        if !api_key.is_empty() {
            profile.api_key = Some(api_key.to_string());
        }

        self.providers.insert(name.clone(), profile);
        self.default_provider = Some(name);
        self.screen = Screen::Entry;
        self.errors = FieldErrors::default();
        true
    }

    // -- Connecting to Copilot -------------------------------------------------------------------

    /// Called once the real Copilot client has started successfully (spec §3.3): marks Copilot
    /// `[connected]`, applies the `default_provider` tie-break (spec §3.5), and returns to the
    /// entry screen.
    pub fn confirm_copilot_connected(&mut self) {
        self.copilot_connected = true;
        if self.copilot_defaults.is_none() {
            self.copilot_defaults = Some(CopilotDefaults::default());
        }
        self.default_provider = Some(RESERVED_COPILOT_PROVIDER_NAME.to_string());
        self.connect_error = None;
        self.screen = Screen::Entry;
    }

    /// Called when the real Copilot client failed to start: records the error and returns to the
    /// entry screen without marking Copilot connected.
    pub fn fail_copilot_connect(&mut self, message: impl Into<String>) {
        self.connect_error = Some(message.into());
        self.screen = Screen::Entry;
    }
}

impl Default for WizardState {
    fn default() -> Self {
        Self::new()
    }
}

/// Name validation (spec §3.7): non-empty, no `/`, not already used by another profile in this
/// pass, and not the reserved word `copilot`. Reuses `provider.rs::validate_provider_name` for the
/// non-empty/no-`/` rule rather than reimplementing it.
fn validate_name(raw: &str, existing: &BTreeMap<String, ProviderProfile>) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("name is required".to_string());
    }
    if trimmed.eq_ignore_ascii_case(RESERVED_COPILOT_PROVIDER_NAME) {
        return Err(format!("'{RESERVED_COPILOT_PROVIDER_NAME}' is reserved"));
    }
    validate_provider_name(trimmed).map_err(|error| error.to_string())?;
    if existing.contains_key(trimmed) {
        return Err("a provider with this name already exists".to_string());
    }
    Ok(trimmed.to_string())
}

/// Base URL validation (spec §3.7): non-empty and shaped like an absolute `http`/`https` URL with
/// no embedded credentials, query, or fragment. Reuses `provider.rs::normalize_base_url` for the
/// shape check (and its normalized return value) rather than reimplementing it. Reachability is
/// deliberately not checked here (that's startup's job, spec §4.2).
fn validate_base_url(raw: &str) -> Result<String, String> {
    if raw.trim().is_empty() {
        return Err("base URL is required".to_string());
    }
    normalize_base_url(raw).map_err(|error| error.to_string())
}

// ============================================================================
// Terminal driver — reuses `tui.rs`'s raw-mode/main-screen primitives, drives its own minimal
// render loop (not `App`'s), and performs the real async Copilot connect (spec §3.3).
// ============================================================================

/// Runs the setup wizard over `existing` configuration and returns the `ProviderConfigFile` to
/// persist. `existing` is `None` on a genuine first run, or when the on-disk file could not be
/// parsed at all; it is `Some` when a parsed-but-invalid file is available to seed the wizard
/// (spec §3.6).
///
/// `config` supplies the same `ClientOptions`/working-directory construction the Copilot-connect
/// screen uses (spec §3.3 reuses `connect_inner`'s Copilot branch unmodified). `config_path` is
/// where the wizard writes the config file directly after every state-changing screen (spec §5.5
/// item 7), in addition to the caller's own final save.
///
/// Esc on the entry screen quits without saving (spec §3.1). Because the caller
/// (`startup::resolve_startup`) unconditionally persists whatever this function returns, "without
/// saving" is implemented by exiting the process directly from here, before ever returning.
pub async fn run_setup_wizard(
    config: &AppConfig,
    config_path: &Path,
    existing: Option<ProviderConfigFile>,
) -> ProviderConfigFile {
    let mut state = match existing {
        Some(existing) => WizardState::seeded_from(&existing),
        None => WizardState::new(),
    };

    let raw_mode_enabled = enable_raw_mode().is_ok();
    let mut stdout = io::stdout();
    let _ = enter_main_screen(&mut stdout);

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = match Terminal::with_options(backend, terminal_options()) {
        Ok(terminal) => terminal,
        Err(_) => {
            // No usable terminal: fall back to just persisting whatever state we have (mirrors
            // `tui.rs`'s non-interactive short-circuit) rather than panicking.
            if raw_mode_enabled {
                let _ = disable_raw_mode();
            }
            let _ = restore_main_screen(&mut io::stdout());
            return state.to_config_file();
        }
    };
    let _ = reset_visible_viewport(terminal.backend_mut());

    loop {
        let _ = terminal.draw(|frame| draw(frame, &state));

        let outcome = match state.screen() {
            Screen::ConnectingCopilot => match connect_to_copilot(config).await {
                Ok(()) => {
                    state.confirm_copilot_connected();
                    WizardOutcome::Persist
                }
                Err(message) => {
                    state.fail_copilot_connect(message);
                    WizardOutcome::Continue
                }
            },
            Screen::Entry => match next_key() {
                Some(key) => state.handle_entry_key(key),
                None => WizardOutcome::Quit,
            },
            Screen::PickProviderPreset => match next_key() {
                Some(key) => state.handle_pick_preset_key(key),
                None => WizardOutcome::Quit,
            },
            Screen::AddProvider => match next_key() {
                Some(key) => state.handle_add_provider_key(key),
                None => WizardOutcome::Quit,
            },
            Screen::Finish => {
                // Unreachable: reaching `Screen::Finish` always comes from a `Finished` outcome,
                // handled below before the next loop iteration.
                WizardOutcome::Finished
            }
        };

        match outcome {
            WizardOutcome::Continue => {}
            WizardOutcome::Persist => {
                let _ = provider_config::save(config_path, &state.to_config_file());
            }
            WizardOutcome::Finished => {
                let _ = provider_config::save(config_path, &state.to_config_file());
                let _ = terminal.draw(|frame| draw(frame, &state));
                let _ = next_key();
                break;
            }
            WizardOutcome::ConnectCopilot => {
                // Nothing to do here: the next loop iteration's `Screen::ConnectingCopilot` arm
                // drives the actual connect.
            }
            WizardOutcome::Quit => {
                restore_terminal(&mut terminal, raw_mode_enabled);
                std::process::exit(0);
            }
        }
    }

    restore_terminal(&mut terminal, raw_mode_enabled);
    state.to_config_file()
}

/// Clears whatever is currently on screen and homes the cursor before `ratatui` takes over
/// drawing the viewport, mirroring `tui.rs`'s `clear_visible_viewport` (kept private there, so
/// reimplemented here rather than reused) — otherwise stale prompt/output from before the wizard
/// started would bleed into the first frame on a "main screen" (non-alternate-screen) terminal.
fn reset_visible_viewport<W: io::Write>(writer: &mut W) -> io::Result<()> {
    crossterm::execute!(
        writer,
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
        crossterm::cursor::MoveTo(0, 0),
    )
}

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    raw_mode_enabled: bool,
) {
    if raw_mode_enabled {
        let _ = disable_raw_mode();
    }
    let _ = restore_main_screen(terminal.backend_mut());
    let _ = terminal.show_cursor();
}

/// Starts the Copilot client exactly like today's `connect_inner` Copilot branch (spec §3.3): no
/// custom login screen, just today's ambient/interactive auto-login, unmodified. `list_models()` is
/// what actually issues the RPC that requires the bundled CLI to be signed in, so awaiting it is
/// what makes this call "resolve" once sign-in completes.
async fn connect_to_copilot(config: &AppConfig) -> Result<(), String> {
    let working_directory = config.working_directory().map_err(|error| error.to_string())?;
    let client_options = config.client_options_in(&working_directory);
    let client = github_copilot_sdk::Client::start(client_options)
        .await
        .map_err(|error| error.to_string())?;
    client
        .list_models()
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Blocks for the next key-press event, ignoring key-release events. Blocking is acceptable here:
/// the wizard runs before any other async work is scheduled (before `connect()`/`AppRuntime`
/// exist), so there is nothing else on this process for the read to contend with.
fn next_key() -> Option<KeyCode> {
    loop {
        match crossterm::event::read() {
            Ok(Event::Key(key_event)) if key_event.kind != KeyEventKind::Release => {
                return Some(key_event.code)
            }
            Ok(_) => continue,
            Err(_) => return None,
        }
    }
}

/// Style applied to the currently focused field's label/border on the add-provider form, and to
/// the highlighted entry-screen list item. Bold + accent color rather than reverse video, so it
/// reads clearly on both light and dark terminal themes.
fn focus_style() -> Style {
    Style::default()
        .fg(crate::palette::CLAUDE)
        .add_modifier(Modifier::BOLD)
}

fn subtle_style() -> Style {
    Style::default().fg(crate::palette::SUBTLE)
}

fn error_style() -> Style {
    Style::default().fg(crate::palette::ERROR)
}

/// Renders the current screen with real `ratatui` widgets (spec §3.1-§3.4/§3.7). The ticket treats
/// exact wording/layout as unspecified polish (spec "Not Yet Specified"), so this keeps the same
/// information as the original plain-text renderer but makes focus/selection actually visible.
fn draw(frame: &mut Frame, state: &WizardState) {
    match state.screen() {
        Screen::Entry => draw_entry(frame, state),
        Screen::PickProviderPreset => draw_pick_provider_preset(frame, state),
        Screen::AddProvider => draw_add_provider(frame, state),
        Screen::ConnectingCopilot => draw_connecting_copilot(frame),
        Screen::Finish => draw_finish(frame, state),
    }
}

/// Renders the provider-picker screen: a `List` of the 8 well-known presets plus "Custom", with
/// the current selection highlighted — same visual style (`focus_style`/highlight symbol) as the
/// entry screen's own list.
fn draw_pick_provider_preset(frame: &mut Frame, state: &WizardState) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    frame.render_widget(
        Paragraph::new("Add a provider — pick a well-known provider, or Custom"),
        chunks[0],
    );

    let items: Vec<ListItem> = PROVIDER_PRESETS
        .iter()
        .map(|preset| match preset.base_url {
            Some(base_url) => ListItem::new(format!("{}  ({base_url})", preset.display_name)),
            None => ListItem::new(preset.display_name),
        })
        .collect();
    let mut list_state = ListState::default();
    list_state.select(Some(state.preset_selection()));
    let list = List::new(items).highlight_style(focus_style()).highlight_symbol("\u{2771} ");
    frame.render_stateful_widget(list, chunks[2], &mut list_state);

    frame.render_widget(
        Paragraph::new(Line::styled(
            "  \u{2191}/\u{2193} to select \u{b7} Enter to choose \u{b7} Esc to go back",
            subtle_style(),
        )),
        chunks[3],
    );
}

fn draw_entry(frame: &mut Frame, state: &WizardState) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    let title = if state.can_finish() {
        "Setup — add another provider, or finish."
    } else {
        "Welcome to picopilot. Let's set up a model provider."
    };
    frame.render_widget(Paragraph::new(title), chunks[0]);

    let copilot_suffix = if state.copilot_connected() {
        "  [connected]"
    } else {
        ""
    };
    let finish_suffix = if state.can_finish() {
        ""
    } else {
        "   (configure at least one to finish)"
    };
    let items = vec![
        ListItem::new(format!("1.  Connect to GitHub Copilot{copilot_suffix}")),
        ListItem::new("2.  Add a provider (OpenRouter, Ollama, vLLM, LM Studio, \u{2026})"),
        ListItem::new(format!("3.  Finish{finish_suffix}")).style(if state.can_finish() {
            Style::default()
        } else {
            subtle_style()
        }),
    ];
    let selected_index = ENTRY_OPTIONS
        .iter()
        .position(|option| *option == state.entry_selection())
        .unwrap_or(0);
    let mut list_state = ListState::default();
    list_state.select(Some(selected_index));
    let list = List::new(items).highlight_style(focus_style()).highlight_symbol("\u{2771} ");
    frame.render_stateful_widget(list, chunks[2], &mut list_state);

    let mut summary_lines: Vec<Line> = Vec::new();
    if !state.providers().is_empty() || state.copilot_connected() {
        summary_lines.push(Line::from("Configured so far:"));
        for (name, profile) in state.providers() {
            let is_default = state.default_provider() == Some(name.as_str());
            let marker = if is_default { "\u{25cf}" } else { "\u{25cb}" };
            let default_label = if is_default { "  \u{2190} default" } else { "" };
            summary_lines.push(Line::from(format!(
                "   {marker} {name}  ({}){default_label}",
                profile.base_url
            )));
        }
        if state.copilot_connected() {
            let is_default = state
                .default_provider()
                .map(ProviderIdentity::parse)
                .is_some_and(|identity| identity.is_copilot());
            let marker = if is_default { "\u{25cf}" } else { "\u{25cb}" };
            let default_label = if is_default { "  \u{2190} default" } else { "" };
            summary_lines.push(Line::from(format!("   {marker} copilot{default_label}")));
        }
    }
    if let Some(error) = state.connect_error() {
        summary_lines.push(Line::from(""));
        summary_lines.push(Line::styled(
            format!("  Could not connect to GitHub Copilot: {error}"),
            error_style(),
        ));
    }
    frame.render_widget(Paragraph::new(summary_lines).wrap(Wrap { trim: false }), chunks[4]);

    frame.render_widget(
        Paragraph::new(Line::styled(
            "  \u{2191}/\u{2193} to select \u{b7} Enter to choose \u{b7} Esc to quit without saving",
            subtle_style(),
        )),
        chunks[5],
    );
}

/// Builds the bordered `Block` for one add-provider field: highlighted (accent, bold) when
/// focused, subdued otherwise, so focus is actually visible — this is the missing feedback that
/// made Tab look broken.
fn field_block(title: &str, focused: bool) -> Block<'static> {
    let style = if focused { focus_style() } else { subtle_style() };
    Block::default()
        .borders(Borders::ALL)
        .border_style(style)
        .title(Span::styled(format!(" {title} "), style))
}

fn draw_add_provider(frame: &mut Frame, state: &WizardState) {
    let draft = state.draft();
    let errors = state.errors();
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(Paragraph::new("Add a provider"), chunks[0]);

    let name_focused = draft.focus == Field::Name;
    render_text_field(frame, chunks[2], "Name", &draft.name, errors.name.as_deref(), name_focused);

    let base_url_focused = draft.focus == Field::BaseUrl;
    render_text_field(
        frame,
        chunks[3],
        "Base URL",
        &draft.base_url,
        errors.base_url.as_deref(),
        base_url_focused,
    );

    let wire_api_focused = draft.focus == Field::WireApi;
    let (completions_mark, responses_mark) = match draft.wire_api {
        WireApiChoice::Completions => ("\u{25cf}", "\u{25cb}"),
        WireApiChoice::Responses => ("\u{25cb}", "\u{25cf}"),
    };
    let wire_api_block = field_block("Wire API", wire_api_focused);
    let wire_api_inner = wire_api_block.inner(chunks[4]);
    frame.render_widget(wire_api_block, chunks[4]);
    frame.render_widget(
        Paragraph::new(format!(
            "[{completions_mark}] completions   [{responses_mark}] responses"
        )),
        wire_api_inner,
    );

    let api_key_focused = draft.focus == Field::ApiKey;
    let masked_key = "*".repeat(draft.api_key.chars().count());
    let api_key_display = if masked_key.is_empty() {
        "(optional, not echoed)".to_string()
    } else {
        masked_key.clone()
    };
    let api_key_block = field_block("API key", api_key_focused);
    let api_key_inner = api_key_block.inner(chunks[5]);
    frame.render_widget(api_key_block, chunks[5]);
    frame.render_widget(Paragraph::new(api_key_display), api_key_inner);
    if api_key_focused {
        frame.set_cursor_position((api_key_inner.x + masked_key.chars().count() as u16, api_key_inner.y));
    }

    frame.render_widget(
        Paragraph::new(Line::styled(
            "  Tab between fields \u{b7} Enter to save \u{b7} Esc to cancel",
            subtle_style(),
        )),
        chunks[7],
    );
}

/// Renders one text field (Name/Base URL) as a bordered block with its value, an inline error line
/// underneath when present (spec §3.7), and places the terminal cursor at the end of the typed
/// text when this field has focus.
fn render_text_field(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    error: Option<&str>,
    focused: bool,
) {
    let block = field_block(label, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    frame.render_widget(Paragraph::new(value), layout[0]);
    if let Some(error) = error {
        frame.render_widget(Paragraph::new(Span::styled(error, error_style())), layout[1]);
    }
    if focused {
        frame.set_cursor_position((layout[0].x + value.chars().count() as u16, layout[0].y));
    }
}

fn draw_connecting_copilot(frame: &mut Frame) {
    let lines = vec![
        Line::from("Connecting to GitHub Copilot\u{2026}"),
        Line::from(""),
        Line::from("  This runs exactly like picopilot's startup does today: the bundled"),
        Line::from("  Copilot CLI performs its own ambient/interactive sign-in. picopilot"),
        Line::from("  builds no custom login screen — whatever that process shows or asks"),
        Line::from("  for happens here, unmodified."),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), frame.area());
}

fn draw_finish(frame: &mut Frame, state: &WizardState) {
    let default_provider = state.default_provider().unwrap_or(RESERVED_COPILOT_PROVIDER_NAME);
    let lines = vec![
        Line::from("Setup complete."),
        Line::from(""),
        Line::from("  config.yaml written. Starting picopilot with default_provider ="),
        Line::from(format!("  '{default_provider}'.")),
        Line::from(""),
        Line::from("  Press Enter to continue."),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), frame.area());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider_with_url(url: &str) -> ProviderProfile {
        ProviderProfile::new(url)
    }

    // -- Entry screen navigation -----------------------------------------------------------------

    #[test]
    fn finish_is_not_selectable_until_something_is_configured() {
        let state = WizardState::new();
        assert!(!state.can_finish());
    }

    #[test]
    fn finish_becomes_selectable_after_adding_a_provider() {
        let mut state = WizardState::new();
        state.go_to_add_provider();
        state.draft = Draft {
            name: "ollama".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            ..Draft::default()
        };
        assert!(state.submit_provider_form());
        assert!(state.can_finish());
    }

    #[test]
    fn entry_selection_moves_up_and_down_and_clamps() {
        let mut state = WizardState::new();
        assert_eq!(state.entry_selection(), EntrySelection::ConnectCopilot);

        state.move_entry_selection(-1);
        assert_eq!(state.entry_selection(), EntrySelection::ConnectCopilot);

        state.move_entry_selection(1);
        assert_eq!(state.entry_selection(), EntrySelection::AddProvider);
        state.move_entry_selection(1);
        assert_eq!(state.entry_selection(), EntrySelection::Finish);
        state.move_entry_selection(1);
        assert_eq!(state.entry_selection(), EntrySelection::Finish);
    }

    #[test]
    fn esc_on_entry_screen_quits() {
        let mut state = WizardState::new();
        assert_eq!(state.handle_entry_key(KeyCode::Esc), WizardOutcome::Quit);
    }

    #[test]
    fn selecting_add_provider_opens_the_sub_flow() {
        let mut state = WizardState::new();
        state.entry_selection = EntrySelection::AddProvider;
        assert_eq!(
            state.handle_entry_key(KeyCode::Enter),
            WizardOutcome::Continue
        );
        // Now opens the provider-picker screen first, not the form directly (see
        // `selecting_add_provider_opens_the_picker_not_the_form_directly` for the picker-specific
        // assertions, and `selecting_custom_lands_on_the_blank_form` for reaching the form).
        assert_eq!(state.screen(), Screen::PickProviderPreset);
    }

    // -- Provider-picker screen --------------------------------------------------------------

    #[test]
    fn selecting_add_provider_opens_the_picker_not_the_form_directly() {
        let mut state = WizardState::new();
        state.entry_selection = EntrySelection::AddProvider;
        state.handle_entry_key(KeyCode::Enter);
        assert_eq!(state.screen(), Screen::PickProviderPreset);
        assert_eq!(state.preset_selection(), 0);
    }

    #[test]
    fn picker_selection_moves_up_and_down_and_clamps() {
        let mut state = WizardState::new();
        state.go_to_pick_provider_preset();
        assert_eq!(state.preset_selection(), 0);

        state.move_preset_selection(-1);
        assert_eq!(state.preset_selection(), 0);

        state.move_preset_selection(1);
        assert_eq!(state.preset_selection(), 1);

        state.move_preset_selection(100);
        assert_eq!(state.preset_selection(), PROVIDER_PRESETS.len() - 1);
    }

    #[test]
    fn esc_on_picker_returns_to_entry() {
        let mut state = WizardState::new();
        state.go_to_pick_provider_preset();
        assert_eq!(
            state.handle_pick_preset_key(KeyCode::Esc),
            WizardOutcome::Continue
        );
        assert_eq!(state.screen(), Screen::Entry);
    }

    #[test]
    fn selecting_each_named_preset_prefills_name_and_base_url() {
        for (index, preset) in PROVIDER_PRESETS.iter().enumerate() {
            let Some(expected_name) = preset.name else {
                continue;
            };
            let expected_base_url = preset.base_url.expect("named presets have a base URL");

            let mut state = WizardState::new();
            state.go_to_pick_provider_preset();
            state.preset_selection = index;
            state.handle_pick_preset_key(KeyCode::Enter);

            assert_eq!(state.screen(), Screen::AddProvider, "preset {expected_name}");
            assert_eq!(state.draft().name, expected_name, "preset {expected_name}");
            assert_eq!(
                state.draft().base_url,
                expected_base_url,
                "preset {expected_name}"
            );
            assert_eq!(state.draft().focus, Field::Name, "preset {expected_name}");
            // The wire-api default must stay untouched by a preset.
            assert_eq!(
                state.draft().wire_api,
                WireApiChoice::Completions,
                "preset {expected_name}"
            );
        }
    }

    #[test]
    fn selecting_custom_lands_on_the_blank_form() {
        let custom_index = PROVIDER_PRESETS.len() - 1;
        assert_eq!(PROVIDER_PRESETS[custom_index].display_name, "Custom");

        let mut state = WizardState::new();
        state.go_to_pick_provider_preset();
        state.preset_selection = custom_index;
        state.handle_pick_preset_key(KeyCode::Enter);

        assert_eq!(state.screen(), Screen::AddProvider);
        assert_eq!(state.draft(), &Draft::default());
    }

    #[test]
    fn a_prefilled_preset_value_can_still_be_edited_and_revalidated() {
        // Select OpenRouter, then mangle its pre-filled base URL into something invalid, and
        // confirm the existing validation still catches it exactly as it would for a hand-typed
        // value — proving the preset is just a starting point, not a bypass.
        let openrouter_index = PROVIDER_PRESETS
            .iter()
            .position(|preset| preset.name == Some("openrouter"))
            .expect("openrouter preset present");

        let mut state = WizardState::new();
        state.go_to_pick_provider_preset();
        state.preset_selection = openrouter_index;
        state.handle_pick_preset_key(KeyCode::Enter);
        assert_eq!(state.draft().base_url, "https://openrouter.ai/api/v1");

        state.draft.base_url = "not-a-url".to_string();
        assert!(!state.submit_provider_form());
        assert!(state.errors().base_url.is_some());
        assert_eq!(state.screen(), Screen::AddProvider);
    }

    #[test]
    fn selecting_finish_before_anything_is_configured_does_nothing() {
        let mut state = WizardState::new();
        state.entry_selection = EntrySelection::Finish;
        assert_eq!(
            state.handle_entry_key(KeyCode::Enter),
            WizardOutcome::Continue
        );
        assert_eq!(state.screen(), Screen::Entry);
    }

    #[test]
    fn selecting_connect_copilot_triggers_the_connect_outcome() {
        let mut state = WizardState::new();
        state.entry_selection = EntrySelection::ConnectCopilot;
        assert_eq!(
            state.handle_entry_key(KeyCode::Enter),
            WizardOutcome::ConnectCopilot
        );
        assert_eq!(state.screen(), Screen::ConnectingCopilot);
    }

    #[test]
    fn selecting_connect_copilot_again_after_already_connected_is_a_no_op() {
        let mut state = WizardState::new();
        state.confirm_copilot_connected();
        state.entry_selection = EntrySelection::ConnectCopilot;
        assert_eq!(
            state.handle_entry_key(KeyCode::Enter),
            WizardOutcome::Continue
        );
        assert_eq!(state.screen(), Screen::Entry);
    }

    // -- Add-a-provider validation (spec §3.7) ---------------------------------------------------

    #[test]
    fn empty_name_is_rejected() {
        let mut state = WizardState::new();
        state.go_to_add_provider();
        state.draft.base_url = "http://localhost:11434/v1".to_string();
        assert!(!state.submit_provider_form());
        assert_eq!(state.errors().name.as_deref(), Some("name is required"));
    }

    #[test]
    fn name_with_a_slash_is_rejected() {
        let mut state = WizardState::new();
        state.go_to_add_provider();
        state.draft.name = "local/provider".to_string();
        state.draft.base_url = "http://localhost:11434/v1".to_string();
        assert!(!state.submit_provider_form());
        assert!(state.errors().name.is_some());
    }

    #[test]
    fn reserved_name_copilot_is_rejected() {
        let mut state = WizardState::new();
        state.go_to_add_provider();
        state.draft.name = "copilot".to_string();
        state.draft.base_url = "http://localhost:11434/v1".to_string();
        assert!(!state.submit_provider_form());
        assert_eq!(
            state.errors().name.as_deref(),
            Some("'copilot' is reserved")
        );
    }

    #[test]
    fn duplicate_name_this_pass_is_rejected() {
        let mut state = WizardState::new();
        state
            .providers
            .insert("ollama".to_string(), provider_with_url("http://localhost:11434/v1"));
        state.go_to_add_provider();
        state.draft.name = "ollama".to_string();
        state.draft.base_url = "http://localhost:11435/v1".to_string();
        assert!(!state.submit_provider_form());
        assert!(state.errors().name.is_some());
    }

    #[test]
    fn empty_base_url_is_rejected() {
        let mut state = WizardState::new();
        state.go_to_add_provider();
        state.draft.name = "ollama".to_string();
        assert!(!state.submit_provider_form());
        assert_eq!(
            state.errors().base_url.as_deref(),
            Some("base URL is required")
        );
    }

    #[test]
    fn malformed_base_url_is_rejected() {
        let mut state = WizardState::new();
        state.go_to_add_provider();
        state.draft.name = "ollama".to_string();
        state.draft.base_url = "not-a-url".to_string();
        assert!(!state.submit_provider_form());
        assert!(state.errors().base_url.is_some());
    }

    #[test]
    fn errors_clear_as_soon_as_the_field_is_edited_again() {
        let mut state = WizardState::new();
        state.go_to_add_provider();
        assert!(!state.submit_provider_form());
        assert!(state.errors().name.is_some());
        assert!(state.errors().base_url.is_some());

        state.draft.focus = Field::Name;
        state.push_char('o');
        assert!(state.errors().name.is_none());
        assert!(state.errors().base_url.is_some());
    }

    #[test]
    fn valid_submission_adds_the_provider_and_returns_to_entry() {
        let mut state = WizardState::new();
        state.go_to_add_provider();
        state.draft.name = "openrouter".to_string();
        state.draft.base_url = "https://openrouter.ai/api/v1".to_string();
        state.draft.api_key = "sk-demo".to_string();
        assert!(state.submit_provider_form());
        assert_eq!(state.screen(), Screen::Entry);
        let profile = state.providers().get("openrouter").expect("provider saved");
        assert_eq!(profile.base_url, "https://openrouter.ai/api/v1");
        assert_eq!(profile.wire_api, "completions");
        assert_eq!(profile.api_key.as_deref(), Some("sk-demo"));
    }

    #[test]
    fn wire_api_toggles_between_completions_and_responses() {
        let mut state = WizardState::new();
        state.go_to_add_provider();
        assert_eq!(state.draft().wire_api, WireApiChoice::Completions);
        state.draft.focus = Field::WireApi;
        state.toggle_wire_api();
        assert_eq!(state.draft().wire_api, WireApiChoice::Responses);
    }

    #[test]
    fn tab_cycles_focus_through_all_fields_and_wraps() {
        let mut state = WizardState::new();
        state.go_to_add_provider();
        assert_eq!(state.draft().focus, Field::Name);
        for expected in [Field::BaseUrl, Field::WireApi, Field::ApiKey, Field::Name] {
            state.handle_add_provider_key(KeyCode::Tab);
            assert_eq!(state.draft().focus, expected);
        }
    }

    #[test]
    fn esc_cancels_the_add_provider_form_and_clears_errors() {
        let mut state = WizardState::new();
        state.go_to_add_provider();
        assert!(!state.submit_provider_form());
        assert_eq!(
            state.handle_add_provider_key(KeyCode::Esc),
            WizardOutcome::Continue
        );
        assert_eq!(state.screen(), Screen::Entry);
        assert_eq!(state.errors(), &FieldErrors::default());
    }

    // -- default_provider tie-break (spec §3.5) --------------------------------------------------

    #[test]
    fn connecting_copilot_then_adding_a_provider_flips_default_to_the_provider() {
        let mut state = WizardState::new();
        state.confirm_copilot_connected();
        assert_eq!(state.default_provider(), Some("copilot"));

        state.go_to_add_provider();
        state.draft.name = "openrouter".to_string();
        state.draft.base_url = "https://openrouter.ai/api/v1".to_string();
        assert!(state.submit_provider_form());
        assert_eq!(state.default_provider(), Some("openrouter"));
    }

    #[test]
    fn adding_a_provider_then_connecting_copilot_flips_default_to_copilot() {
        let mut state = WizardState::new();
        state.go_to_add_provider();
        state.draft.name = "openrouter".to_string();
        state.draft.base_url = "https://openrouter.ai/api/v1".to_string();
        assert!(state.submit_provider_form());
        assert_eq!(state.default_provider(), Some("openrouter"));

        state.confirm_copilot_connected();
        assert_eq!(state.default_provider(), Some("copilot"));
    }

    #[test]
    fn adding_two_providers_the_second_one_wins() {
        let mut state = WizardState::new();
        state.go_to_add_provider();
        state.draft.name = "ollama".to_string();
        state.draft.base_url = "http://localhost:11434/v1".to_string();
        assert!(state.submit_provider_form());
        assert_eq!(state.default_provider(), Some("ollama"));

        state.go_to_add_provider();
        state.draft.name = "openrouter".to_string();
        state.draft.base_url = "https://openrouter.ai/api/v1".to_string();
        assert!(state.submit_provider_form());
        assert_eq!(state.default_provider(), Some("openrouter"));
    }

    // -- Copilot connect result handling ---------------------------------------------------------

    #[test]
    fn a_failed_copilot_connect_is_recorded_and_does_not_mark_connected() {
        let mut state = WizardState::new();
        state.entry_selection = EntrySelection::ConnectCopilot;
        state.handle_entry_key(KeyCode::Enter);
        state.fail_copilot_connect("boom");
        assert!(!state.copilot_connected());
        assert_eq!(state.connect_error(), Some("boom"));
        assert_eq!(state.screen(), Screen::Entry);
    }

    // -- Seeding from an existing config (spec §3.6, `--configure`) ------------------------------

    #[test]
    fn seeded_from_existing_config_preseeds_providers_copilot_and_default() {
        let mut existing = ProviderConfigFile::new("openrouter");
        existing.copilot = Some(CopilotDefaults {
            default_model: Some("gpt-5".to_string()),
            ..CopilotDefaults::default()
        });
        existing
            .providers
            .insert("openrouter".to_string(), provider_with_url("https://openrouter.ai/api/v1"));

        let state = WizardState::seeded_from(&existing);

        assert!(state.can_finish());
        assert!(state.copilot_connected());
        assert_eq!(state.default_provider(), Some("openrouter"));
        assert!(state.providers().contains_key("openrouter"));
    }

    #[test]
    fn seeded_copilot_defaults_survive_a_reconnect_in_the_same_pass() {
        let mut existing = ProviderConfigFile::new("copilot");
        existing.copilot = Some(CopilotDefaults {
            default_model: Some("gpt-5".to_string()),
            ..CopilotDefaults::default()
        });

        let mut state = WizardState::seeded_from(&existing);
        // Re-confirming Copilot (e.g. re-running --configure) must not wipe remembered defaults.
        state.confirm_copilot_connected();

        let config = state.to_config_file();
        assert_eq!(
            config.copilot.and_then(|copilot| copilot.default_model),
            Some("gpt-5".to_string())
        );
    }

    #[test]
    fn seeding_does_not_reset_when_nothing_existed_yet() {
        let state = WizardState::seeded_from(&ProviderConfigFile::new("copilot"));
        assert!(!state.copilot_connected());
        assert!(state.providers().is_empty());
        assert_eq!(state.default_provider(), Some("copilot"));
    }

    // -- to_config_file --------------------------------------------------------------------------

    #[test]
    fn to_config_file_reflects_current_providers_and_default() {
        let mut state = WizardState::new();
        state.go_to_add_provider();
        state.draft.name = "ollama".to_string();
        state.draft.base_url = "http://localhost:11434/v1".to_string();
        assert!(state.submit_provider_form());

        let config = state.to_config_file();
        assert_eq!(config.default_provider, "ollama");
        assert!(config.providers.contains_key("ollama"));
        assert!(config.copilot.is_none());
    }

    // -- ratatui rendering (TestBackend) -----------------------------------------------------------
    //
    // Mirrors `tui.rs`'s own `TestBackend`-driven rendering tests: draw a screen into an in-memory
    // terminal buffer and assert on the visible cell contents/styles rather than driving a real
    // terminal (which isn't possible in a unit test).

    use ratatui::backend::TestBackend;

    fn rendered_rows(state: &WizardState, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::with_options(TestBackend::new(width, height), terminal_options())
            .expect("test terminal should initialize");
        terminal
            .draw(|frame| draw(frame, state))
            .expect("draw should succeed");
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn entry_screen_renders_options_and_highlights_the_selection() {
        let state = WizardState::new();
        let rows = rendered_rows(&state, 100, 22);
        assert!(rows
            .iter()
            .any(|row| row.contains("Connect to GitHub Copilot")));
        assert!(rows.iter().any(|row| row.contains("Add a provider")));
        assert!(rows.iter().any(|row| row.contains("Finish")));
        assert!(rows
            .iter()
            .any(|row| row.contains("configure at least one to finish")));

        // Default selection is ConnectCopilot; the highlight symbol should mark its row.
        let selected_row = rows
            .iter()
            .find(|row| row.contains("Connect to GitHub Copilot"))
            .expect("selected row present");
        assert!(selected_row.contains('\u{2771}'));
    }

    #[test]
    fn entry_screen_shows_configured_providers_and_default_marker() {
        let mut state = WizardState::new();
        state.go_to_add_provider();
        state.draft.name = "ollama".to_string();
        state.draft.base_url = "http://localhost:11434/v1".to_string();
        assert!(state.submit_provider_form());

        let rows = rendered_rows(&state, 100, 22);
        assert!(rows.iter().any(|row| row.contains("Configured so far")));
        assert!(rows
            .iter()
            .any(|row| row.contains("ollama") && row.contains("default")));
    }

    #[test]
    fn picker_screen_renders_all_options_and_highlights_the_selection() {
        let mut state = WizardState::new();
        state.go_to_pick_provider_preset();
        state.move_preset_selection(2); // highlight "Together AI"

        let rows = rendered_rows(&state, 100, 22);
        for preset in PROVIDER_PRESETS.iter() {
            assert!(
                rows.iter().any(|row| row.contains(preset.display_name)),
                "expected to find '{}' rendered",
                preset.display_name
            );
        }

        let selected_row = rows
            .iter()
            .find(|row| row.contains("Together AI"))
            .expect("selected row present");
        assert!(selected_row.contains('\u{2771}'));
    }

    #[test]
    fn add_provider_screen_shows_all_field_labels() {
        let mut state = WizardState::new();
        state.go_to_add_provider();
        let rows = rendered_rows(&state, 100, 22);
        assert!(rows.iter().any(|row| row.contains("Name")));
        assert!(rows.iter().any(|row| row.contains("Base URL")));
        assert!(rows.iter().any(|row| row.contains("Wire API")));
        assert!(rows.iter().any(|row| row.contains("API key")));
        assert!(rows.iter().any(|row| row.contains("completions")));
    }

    /// The user's original bug report: Tab appeared not to switch focus. The pure `Field` cycle
    /// (`tab_cycles_focus_through_all_fields_and_wraps` above) was already correct, so this test
    /// asserts on the piece that was actually missing — the rendered form gave zero visual
    /// feedback about which field was focused, which is why Tab *looked* broken interactively.
    #[test]
    fn add_provider_screen_highlights_the_focused_field_border() {
        let mut state = WizardState::new();
        state.go_to_add_provider();
        assert_eq!(state.draft().focus, Field::Name);

        let mut terminal = Terminal::with_options(TestBackend::new(100, 22), terminal_options())
            .expect("test terminal should initialize");
        terminal
            .draw(|frame| draw(frame, &state))
            .expect("draw should succeed");
        let name_border_style_before = terminal.backend().buffer()[(0, 2)].style();

        state.handle_add_provider_key(KeyCode::Tab);
        assert_eq!(state.draft().focus, Field::BaseUrl);
        terminal
            .draw(|frame| draw(frame, &state))
            .expect("draw should succeed");
        let name_border_style_after = terminal.backend().buffer()[(0, 2)].style();
        let base_url_border_style_after = terminal.backend().buffer()[(0, 6)].style();

        assert_ne!(name_border_style_before, name_border_style_after);
        assert_eq!(base_url_border_style_after, name_border_style_before);
    }

    #[test]
    fn add_provider_screen_shows_inline_errors_under_offending_fields() {
        let mut state = WizardState::new();
        state.go_to_add_provider();
        assert!(!state.submit_provider_form());

        let rows = rendered_rows(&state, 100, 22);
        assert!(rows.iter().any(|row| row.contains("name is required")));
        assert!(rows.iter().any(|row| row.contains("base URL is required")));
    }

    #[test]
    fn connecting_copilot_screen_renders_via_the_entry_transition() {
        let mut state = WizardState::new();
        state.entry_selection = EntrySelection::ConnectCopilot;
        assert_eq!(
            state.handle_entry_key(KeyCode::Enter),
            WizardOutcome::ConnectCopilot
        );
        assert_eq!(state.screen(), Screen::ConnectingCopilot);

        let rows = rendered_rows(&state, 100, 22);
        assert!(rows
            .iter()
            .any(|row| row.contains("Connecting to GitHub Copilot")));
    }

    #[test]
    fn finish_screen_renders_the_default_provider() {
        let mut state = WizardState::new();
        state.confirm_copilot_connected();
        state.entry_selection = EntrySelection::Finish;
        assert_eq!(
            state.handle_entry_key(KeyCode::Enter),
            WizardOutcome::Finished
        );
        assert_eq!(state.screen(), Screen::Finish);

        let rows = rendered_rows(&state, 100, 22);
        assert!(rows.iter().any(|row| row.contains("Setup complete")));
        assert!(rows.iter().any(|row| row.contains("'copilot'")));
    }
}
