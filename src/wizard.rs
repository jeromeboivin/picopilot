//! Interactive first-run / `--configure` setup wizard (spec §3).
//!
//! The module is split into two halves on purpose:
//!
//! - [`WizardState`] and its transition methods are pure — no terminal I/O, no clock, no network
//!   — so the screen sequence, inline validation, and the `default_provider` "most-recently-wins"
//!   tie-break (spec §3.5) can be unit tested without a real terminal.
//! - [`run_setup_wizard`] is the thin terminal driver: it reuses `tui.rs`'s low-level raw-mode /
//!   main-screen primitives, renders each screen as plain text, reads key events, and (for the
//!   Copilot-connect screen) drives the real async Copilot client startup, but it deliberately does
//!   not reuse `App`'s full draw loop or state machine (out of scope per the ticket).

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;

use crossterm::cursor::MoveTo;
use crossterm::event::{Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType};

use crate::config::AppConfig;
use crate::provider::{normalize_base_url, validate_provider_name};
use crate::provider_config::{
    self, CopilotDefaults, ProviderConfigFile, ProviderProfile, RESERVED_COPILOT_PROVIDER_NAME,
};
use crate::screen_model::{enter_main_screen, restore_main_screen};

// ============================================================================
// Pure state machine — unit-testable without a terminal.
// ============================================================================

/// Which screen the wizard is currently showing (spec §3.1-§3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Entry,
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
                self.go_to_add_provider();
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

    loop {
        render(&mut stdout, &state);

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
                render(&mut stdout, &state);
                let _ = next_key();
                break;
            }
            WizardOutcome::ConnectCopilot => {
                // Nothing to do here: the next loop iteration's `Screen::ConnectingCopilot` arm
                // drives the actual connect.
            }
            WizardOutcome::Quit => {
                restore_terminal(&mut stdout, raw_mode_enabled);
                std::process::exit(0);
            }
        }
    }

    restore_terminal(&mut stdout, raw_mode_enabled);
    state.to_config_file()
}

fn restore_terminal(stdout: &mut io::Stdout, raw_mode_enabled: bool) {
    let _ = restore_main_screen(stdout);
    if raw_mode_enabled {
        let _ = disable_raw_mode();
    }
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

fn render(stdout: &mut io::Stdout, state: &WizardState) {
    let _ = execute!(stdout, Clear(ClearType::All), MoveTo(0, 0));
    for line in render_lines(state) {
        let _ = write!(stdout, "{line}\r\n");
    }
    let _ = stdout.flush();
}

/// Renders the current screen as plain text lines (spec §3.1-§3.4/§3.7). Pure and testable, though
/// the ticket treats exact wording/layout as unspecified polish (spec "Not Yet Specified").
fn render_lines(state: &WizardState) -> Vec<String> {
    match state.screen() {
        Screen::Entry => render_entry_lines(state),
        Screen::AddProvider => render_add_provider_lines(state),
        Screen::ConnectingCopilot => render_connecting_copilot_lines(),
        Screen::Finish => render_finish_lines(state),
    }
}

fn render_entry_lines(state: &WizardState) -> Vec<String> {
    let mut lines = Vec::new();
    if state.can_finish() {
        lines.push("Setup — add another provider, or finish.".to_string());
    } else {
        lines.push("Welcome to picopilot. Let's set up a model provider.".to_string());
    }
    lines.push(String::new());

    let cursor = |selection: EntrySelection| -> &str {
        if state.entry_selection() == selection {
            "\u{2771} "
        } else {
            "  "
        }
    };

    let copilot_suffix = if state.copilot_connected() {
        "  [connected]"
    } else {
        ""
    };
    lines.push(format!(
        "{}1.  Connect to GitHub Copilot{copilot_suffix}",
        cursor(EntrySelection::ConnectCopilot)
    ));
    lines.push(format!(
        "{}2.  Add a provider (OpenRouter, Ollama, vLLM, LM Studio, \u{2026})",
        cursor(EntrySelection::AddProvider)
    ));
    if state.can_finish() {
        lines.push(format!("{}3.  Finish", cursor(EntrySelection::Finish)));
    } else {
        lines.push(format!(
            "{}3.  Finish                        (configure at least one to finish)",
            cursor(EntrySelection::Finish)
        ));
    }

    if !state.providers().is_empty() || state.copilot_connected() {
        lines.push(String::new());
        lines.push("Configured so far:".to_string());
        for (name, profile) in state.providers() {
            let is_default = state.default_provider() == Some(name.as_str());
            let marker = if is_default { "\u{25cf}" } else { "\u{25cb}" };
            let default_label = if is_default { "  \u{2190} default" } else { "" };
            lines.push(format!(
                "   {marker} {name}  ({}){default_label}",
                profile.base_url
            ));
        }
        if state.copilot_connected() {
            let is_default = state.default_provider() == Some(RESERVED_COPILOT_PROVIDER_NAME);
            let marker = if is_default { "\u{25cf}" } else { "\u{25cb}" };
            let default_label = if is_default { "  \u{2190} default" } else { "" };
            lines.push(format!("   {marker} copilot{default_label}"));
        }
    }

    if let Some(error) = state.connect_error() {
        lines.push(String::new());
        lines.push(format!("  Could not connect to GitHub Copilot: {error}"));
    }

    lines.push(String::new());
    lines.push("  \u{2191}/\u{2193} to select \u{b7} Enter to choose \u{b7} Esc to quit without saving".to_string());
    lines
}

fn render_add_provider_lines(state: &WizardState) -> Vec<String> {
    let draft = state.draft();
    let errors = state.errors();
    let mut lines = vec!["Add a provider".to_string(), String::new()];

    lines.push(format!("Name        {}", draft.name));
    if let Some(error) = &errors.name {
        lines.push(format!("            {error}"));
    }
    lines.push(String::new());

    lines.push(format!("Base URL    {}", draft.base_url));
    if let Some(error) = &errors.base_url {
        lines.push(format!("            {error}"));
    }
    lines.push(String::new());

    let (completions_mark, responses_mark) = match draft.wire_api {
        WireApiChoice::Completions => ("\u{25cf}", "\u{25cb}"),
        WireApiChoice::Responses => ("\u{25cb}", "\u{25cf}"),
    };
    lines.push(format!(
        "Wire API    [{completions_mark}] completions   [{responses_mark}] responses"
    ));
    lines.push(String::new());

    let masked_key = "*".repeat(draft.api_key.chars().count());
    if masked_key.is_empty() {
        lines.push("API key     (optional, not echoed)".to_string());
    } else {
        lines.push(format!("API key     {masked_key}"));
    }
    lines.push(String::new());
    lines.push("  Tab between fields \u{b7} Enter to save \u{b7} Esc to cancel".to_string());
    lines
}

fn render_connecting_copilot_lines() -> Vec<String> {
    vec![
        "Connecting to GitHub Copilot\u{2026}".to_string(),
        String::new(),
        "  This runs exactly like picopilot's startup does today: the bundled".to_string(),
        "  Copilot CLI performs its own ambient/interactive sign-in. picopilot".to_string(),
        "  builds no custom login screen — whatever that process shows or asks".to_string(),
        "  for happens here, unmodified.".to_string(),
    ]
}

fn render_finish_lines(state: &WizardState) -> Vec<String> {
    let default_provider = state.default_provider().unwrap_or(RESERVED_COPILOT_PROVIDER_NAME);
    vec![
        "Setup complete.".to_string(),
        String::new(),
        "  config.yaml written. Starting picopilot with default_provider =".to_string(),
        format!("  '{default_provider}'."),
        String::new(),
        "  Press Enter to continue.".to_string(),
    ]
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
}
