//! On-disk multi-provider configuration file: schema, path resolution, load, validate, and save.
//!
//! Implements `docs/provider-config-spec.md` §1 (Configuration File) and §5.1-5.2, 5.4
//! (persistence mechanics). This module owns the schema types and the load/validate/save
//! machinery in isolation; wiring it into startup, the wizard, or `AppRuntime` is later work.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::provider::{
    normalize_base_url, validate_provider_name, validate_wire_api, ProviderError,
    DEFAULT_PROVIDER_WIRE_API,
};
use crate::skills::home_directory;

/// `"copilot"` is reserved: legal as `default_provider` or the top-level `copilot` block key,
/// never as a `providers` map key.
pub const RESERVED_COPILOT_PROVIDER_NAME: &str = "copilot";

/// Which provider a `default_provider` string (or a switched-to provider name) refers to: the
/// reserved hosted Copilot provider, or a named entry in `providers`. Replaces the repeated
/// `if name == RESERVED_COPILOT_PROVIDER_NAME { .. } else { .. }` cascade with a real type callers
/// can match on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderIdentity {
    Copilot,
    Named(String),
}

impl ProviderIdentity {
    /// Classifies `name` as `Copilot` or `Named`.
    pub fn parse(name: &str) -> Self {
        if name == RESERVED_COPILOT_PROVIDER_NAME {
            Self::Copilot
        } else {
            Self::Named(name.to_string())
        }
    }

    pub fn is_copilot(&self) -> bool {
        matches!(self, Self::Copilot)
    }
}

impl fmt::Display for ProviderIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Copilot => write!(formatter, "{RESERVED_COPILOT_PROVIDER_NAME}"),
            Self::Named(name) => write!(formatter, "{name}"),
        }
    }
}

/// The full deserialized shape of `config.yaml`, per spec §1.2.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct ProviderConfigFile {
    pub default_provider: String,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub copilot: Option<CopilotDefaults>,

    #[serde(default)]
    pub providers: BTreeMap<String, ProviderProfile>,
}

impl ProviderConfigFile {
    pub fn new(default_provider: impl Into<String>) -> Self {
        Self {
            default_provider: default_provider.into(),
            copilot: None,
            providers: BTreeMap::new(),
        }
    }
}

/// Remembered per-provider defaults for the hosted Copilot provider (spec §1.2). Structurally
/// separate from `ProviderProfile` because Copilot has no URL or API key.
#[derive(Clone, Serialize, Deserialize, Debug, Default, PartialEq, Eq)]
pub struct CopilotDefaults {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub default_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub default_reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub default_context_tier: Option<String>,
}

/// One OpenAI-compatible provider profile, keyed by name in `ProviderConfigFile::providers`.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderProfile {
    pub base_url: String,

    #[serde(default = "default_wire_api")]
    pub wire_api: String,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub api_key: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub default_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub default_reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub default_context_tier: Option<String>,
}

impl ProviderProfile {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            wire_api: default_wire_api(),
            api_key: None,
            default_model: None,
            default_reasoning_effort: None,
            default_context_tier: None,
        }
    }
}

fn default_wire_api() -> String {
    DEFAULT_PROVIDER_WIRE_API.to_string()
}

// Mirrors `ProviderSettings`'s existing `Debug` redaction pattern (`provider.rs`): `api_key` is
// never printed, only whether one is set.
impl fmt::Debug for ProviderProfile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderProfile")
            .field("base_url", &self.base_url)
            .field("wire_api", &self.wire_api)
            .field("api_key", &self.api_key.as_ref().map(|_| "<set>"))
            .field("default_model", &self.default_model)
            .field("default_reasoning_effort", &self.default_reasoning_effort)
            .field("default_context_tier", &self.default_context_tier)
            .finish()
    }
}

/// Errors from loading, validating, or saving the provider configuration file.
#[derive(Debug)]
pub enum ProviderConfigError {
    Io(io::Error),
    Parse(serde_yaml::Error),
    /// `default_provider` names neither `"copilot"` nor a key present in `providers`.
    UnresolvedDefaultProvider { default_provider: String },
    /// `providers` contains a `"copilot"` key, which is reserved (spec §1.2/§2.3).
    ReservedProviderName,
    /// A provider profile failed one of `provider.rs`'s validators.
    InvalidProvider { name: String, source: ProviderError },
}

impl fmt::Display for ProviderConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "could not access config.yaml: {error}"),
            Self::Parse(error) => write!(formatter, "config.yaml is not valid YAML: {error}"),
            Self::UnresolvedDefaultProvider { default_provider } => write!(
                formatter,
                "default_provider '{default_provider}' is not configured"
            ),
            Self::ReservedProviderName => write!(
                formatter,
                "'{RESERVED_COPILOT_PROVIDER_NAME}' is reserved and must not appear as a providers key"
            ),
            Self::InvalidProvider { name, source } => {
                write!(formatter, "provider '{name}' is invalid: {source}")
            }
        }
    }
}

impl std::error::Error for ProviderConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Parse(error) => Some(error),
            Self::InvalidProvider { source, .. } => Some(source),
            Self::UnresolvedDefaultProvider { .. } | Self::ReservedProviderName => None,
        }
    }
}

/// Resolves the per-OS config file path (spec §1.1), reusing `skills.rs`'s `home_directory()`
/// helper rather than duplicating its `USERPROFILE`/`HOME` fallback. Returns `None` only if no
/// home directory can be determined at all.
pub fn config_file_path() -> Option<PathBuf> {
    home_directory().map(|home| config_file_path_for(&home))
}

fn config_file_path_for(home: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Roaming"))
            .join("picopilot")
            .join("config.yaml")
    }

    #[cfg(target_os = "macos")]
    {
        home.join("Library")
            .join("Application Support")
            .join("picopilot")
            .join("config.yaml")
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        home.join(".config").join("picopilot").join("config.yaml")
    }
}

/// Loads and parses the config file at `path`. A missing file is a distinct, non-error "no config
/// yet" case (`Ok(None)`) — this function MUST NOT create the containing directory (spec §1.1).
pub fn load(path: &Path) -> Result<Option<ProviderConfigFile>, ProviderConfigError> {
    match fs::read_to_string(path) {
        Ok(content) => {
            let config = serde_yaml::from_str(&content).map_err(ProviderConfigError::Parse)?;
            Ok(Some(config))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ProviderConfigError::Io(error)),
    }
}

/// Validates a parsed config per spec §1.4. Does NOT validate `default_model`/
/// `default_reasoning_effort`/`default_context_tier` against a live model catalog — that is a
/// runtime concern handled elsewhere (`config.rs`'s `ConfigError`).
pub fn validate(config: &ProviderConfigFile) -> Result<(), ProviderConfigError> {
    if config
        .providers
        .contains_key(RESERVED_COPILOT_PROVIDER_NAME)
    {
        return Err(ProviderConfigError::ReservedProviderName);
    }

    if let ProviderIdentity::Named(name) = ProviderIdentity::parse(&config.default_provider) {
        if !config.providers.contains_key(&name) {
            return Err(ProviderConfigError::UnresolvedDefaultProvider {
                default_provider: name,
            });
        }
    }

    for (name, profile) in &config.providers {
        validate_provider_name(name)
            .map_err(|source| ProviderConfigError::InvalidProvider {
                name: name.clone(),
                source,
            })?;
        normalize_base_url(&profile.base_url).map_err(|source| {
            ProviderConfigError::InvalidProvider {
                name: name.clone(),
                source,
            }
        })?;
        validate_wire_api(&profile.wire_api).map_err(|source| {
            ProviderConfigError::InvalidProvider {
                name: name.clone(),
                source,
            }
        })?;
    }

    Ok(())
}

/// Full read-modify-write, write-to-temp-then-rename save (spec §5.1-5.2). Creates the containing
/// directory (`create_dir_all`) if needed — unlike `load`, this MUST happen on write.
pub fn save(path: &Path, config: &ProviderConfigFile) -> Result<(), ProviderConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(ProviderConfigError::Io)?;
    }

    let yaml = serde_yaml::to_string(config).map_err(ProviderConfigError::Parse)?;
    let temp_path = temp_path_for(path);
    fs::write(&temp_path, yaml).map_err(ProviderConfigError::Io)?;
    fs::rename(&temp_path, path).map_err(ProviderConfigError::Io)?;

    Ok(())
}

fn temp_path_for(path: &Path) -> PathBuf {
    let mut file_name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    file_name.push(".tmp");
    path.with_file_name(file_name)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{
        config_file_path_for, load, save, temp_path_for, validate, CopilotDefaults,
        ProviderConfigError, ProviderConfigFile, ProviderIdentity, ProviderProfile,
        RESERVED_COPILOT_PROVIDER_NAME,
    };

    #[test]
    fn provider_identity_parses_the_reserved_name_as_copilot() {
        assert_eq!(
            ProviderIdentity::parse(RESERVED_COPILOT_PROVIDER_NAME),
            ProviderIdentity::Copilot
        );
        assert!(ProviderIdentity::parse("copilot").is_copilot());
    }

    #[test]
    fn provider_identity_parses_any_other_name_as_named() {
        let identity = ProviderIdentity::parse("openrouter");
        assert_eq!(identity, ProviderIdentity::Named("openrouter".to_string()));
        assert!(!identity.is_copilot());
    }

    #[test]
    fn provider_identity_display_round_trips_the_original_name() {
        assert_eq!(ProviderIdentity::parse("copilot").to_string(), "copilot");
        assert_eq!(ProviderIdentity::parse("ollama").to_string(), "ollama");
    }

    fn temp_directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "picopilot-provider-config-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp directory should be created");
        path
    }

    fn sample_config() -> ProviderConfigFile {
        let mut config = ProviderConfigFile::new("openrouter");
        config.copilot = Some(CopilotDefaults {
            default_model: Some("gpt-5".to_string()),
            default_reasoning_effort: Some("high".to_string()),
            default_context_tier: Some("long_context".to_string()),
        });
        let mut openrouter = ProviderProfile::new("https://openrouter.ai/api/v1");
        openrouter.api_key = Some("sk-or-v1-secret".to_string());
        openrouter.default_model = Some("anthropic/claude-3.5-sonnet".to_string());
        config.providers.insert("openrouter".to_string(), openrouter);
        let ollama = ProviderProfile::new("http://localhost:11434/v1");
        config.providers.insert("ollama".to_string(), ollama);
        config
    }

    #[test]
    fn resolves_the_documented_per_os_path() {
        let home = PathBuf::from("/home/example");
        let path = config_file_path_for(&home);
        let path_string = path.to_string_lossy().replace('\\', "/");

        #[cfg(target_os = "macos")]
        assert!(path_string.ends_with("Library/Application Support/picopilot/config.yaml"));
        #[cfg(all(not(windows), not(target_os = "macos")))]
        assert!(path_string.ends_with(".config/picopilot/config.yaml"));
        #[cfg(windows)]
        assert!(path_string.ends_with("picopilot/config.yaml"));
    }

    #[test]
    fn round_trips_a_valid_config_through_save_and_load() {
        let directory = temp_directory("round-trip");
        let path = directory.join("config.yaml");
        let config = sample_config();

        save(&path, &config).expect("valid config should save");
        let loaded = load(&path)
            .expect("saved config should load")
            .expect("saved config should be present");

        assert_eq!(loaded, config);
        validate(&loaded).expect("round-tripped config should still validate");
    }

    #[test]
    fn missing_file_is_a_distinct_absent_result_and_creates_no_directory() {
        let directory = temp_directory("missing");
        let missing_subdirectory = directory.join("does-not-exist");
        let path = missing_subdirectory.join("config.yaml");

        let result = load(&path).expect("a missing file should not be an error");

        assert!(result.is_none());
        assert!(!missing_subdirectory.exists());
    }

    #[test]
    fn malformed_yaml_is_a_parse_error() {
        let directory = temp_directory("malformed");
        let path = directory.join("config.yaml");
        fs::write(&path, "default_provider: [this is not valid: yaml").unwrap();

        let error = load(&path).expect_err("malformed YAML should fail to parse");

        assert!(matches!(error, ProviderConfigError::Parse(_)));
    }

    #[test]
    fn rejects_a_default_provider_pointing_nowhere() {
        let config = ProviderConfigFile::new("missing-provider");

        let error = validate(&config).expect_err("an unresolved default_provider should fail");

        assert!(matches!(
            error,
            ProviderConfigError::UnresolvedDefaultProvider { default_provider }
                if default_provider == "missing-provider"
        ));
    }

    #[test]
    fn default_provider_may_resolve_to_copilot_with_no_providers_configured() {
        let config = ProviderConfigFile::new("copilot");

        validate(&config).expect("'copilot' should always be a valid default_provider");
    }

    #[test]
    fn rejects_a_reserved_copilot_key_under_providers() {
        let mut config = ProviderConfigFile::new("copilot");
        config
            .providers
            .insert("copilot".to_string(), ProviderProfile::new("https://example.test"));

        let error = validate(&config).expect_err("a providers.copilot key should be rejected");

        assert!(matches!(error, ProviderConfigError::ReservedProviderName));
    }

    #[test]
    fn rejects_a_bad_provider_name_surfaced_from_provider_rs() {
        let mut config = ProviderConfigFile::new("bad/name");
        config
            .providers
            .insert("bad/name".to_string(), ProviderProfile::new("https://example.test"));

        let error = validate(&config).expect_err("a provider name containing '/' should fail");

        assert!(matches!(
            error,
            ProviderConfigError::InvalidProvider { name, .. } if name == "bad/name"
        ));
    }

    #[test]
    fn rejects_a_bad_base_url_surfaced_from_provider_rs() {
        let mut config = ProviderConfigFile::new("openrouter");
        config.providers.insert(
            "openrouter".to_string(),
            ProviderProfile::new("http://user:pass@example.test/v1"),
        );

        let error = validate(&config).expect_err("a base_url with embedded credentials should fail");

        assert!(matches!(
            error,
            ProviderConfigError::InvalidProvider { name, .. } if name == "openrouter"
        ));
    }

    #[test]
    fn rejects_a_bad_wire_api_surfaced_from_provider_rs() {
        let mut config = ProviderConfigFile::new("openrouter");
        let mut profile = ProviderProfile::new("https://openrouter.ai/api/v1");
        profile.wire_api = "graphql".to_string();
        config.providers.insert("openrouter".to_string(), profile);

        let error = validate(&config).expect_err("an unsupported wire_api should fail");

        assert!(matches!(
            error,
            ProviderConfigError::InvalidProvider { name, .. } if name == "openrouter"
        ));
    }

    #[test]
    fn wire_api_defaults_to_completions_when_omitted() {
        let yaml = "default_provider: openrouter\nproviders:\n  openrouter:\n    base_url: https://openrouter.ai/api/v1\n";
        let directory = temp_directory("wire-api-default");
        let path = directory.join("config.yaml");
        fs::write(&path, yaml).unwrap();

        let config = load(&path).unwrap().unwrap();

        assert_eq!(config.providers["openrouter"].wire_api, "completions");
    }

    #[test]
    fn save_creates_the_containing_directory_but_load_does_not() {
        let directory = temp_directory("create-on-write");
        let nested = directory.join("nested").join("dir");
        let path = nested.join("config.yaml");
        assert!(load(&path).unwrap().is_none());
        assert!(!nested.exists());

        save(&path, &ProviderConfigFile::new("copilot")).expect("save should create directories");

        assert!(nested.exists());
        assert!(path.exists());
    }

    #[test]
    fn save_writes_atomically_via_a_temp_file_that_does_not_survive() {
        let directory = temp_directory("atomic-save");
        let path = directory.join("config.yaml");
        let temp_path = temp_path_for(&path);
        assert_eq!(temp_path, directory.join("config.yaml.tmp"));

        save(&path, &ProviderConfigFile::new("copilot")).expect("save should succeed");

        assert!(path.exists());
        assert!(
            !temp_path.exists(),
            "the temp file must be renamed onto the real path, not left behind"
        );
    }

    #[test]
    fn save_overwrites_an_existing_file_with_a_fresh_read_modify_write() {
        let directory = temp_directory("overwrite");
        let path = directory.join("config.yaml");
        save(&path, &sample_config()).unwrap();

        let mut updated = sample_config();
        updated.default_provider = "ollama".to_string();
        save(&path, &updated).unwrap();

        let loaded = load(&path).unwrap().unwrap();
        assert_eq!(loaded.default_provider, "ollama");
    }

    #[test]
    fn debug_output_never_contains_a_real_api_key() {
        let config = sample_config();

        let debug = format!("{config:?}");

        assert!(!debug.contains("sk-or-v1-secret"));
        assert!(debug.contains("<set>"));
    }

    #[test]
    fn debug_output_shows_absent_when_no_api_key_is_set() {
        let profile = ProviderProfile::new("http://localhost:11434/v1");

        let debug = format!("{profile:?}");

        assert!(!debug.contains("<set>"));
        assert!(debug.contains("api_key: None"));
    }
}
