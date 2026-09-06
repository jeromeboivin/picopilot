use std::fmt;
use std::path::{Path, PathBuf};

use clap::Parser;
use github_copilot_sdk::types::{Model, SessionConfig, SystemMessageConfig};
use github_copilot_sdk::ClientOptions;

use crate::provider::{
    ProviderError, ProviderRegistry, ProviderSettings, DEFAULT_PROVIDER_NAME,
    DEFAULT_PROVIDER_WIRE_API,
};
use crate::toolset::{Toolset, CANONICAL_TOOLS, EXCLUDED_TOOLS};

pub const V1_AVAILABLE_TOOLS: &[&str] = CANONICAL_TOOLS;
pub const V1_EXCLUDED_TOOLS: &[&str] = EXCLUDED_TOOLS;

pub(crate) fn system_message_config() -> SystemMessageConfig {
    SystemMessageConfig::new()
        .with_mode("replace")
        .with_content("")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    ModelNotFound {
        model: String,
        available: Vec<String>,
    },
    OptionRequiresModel {
        option: &'static str,
    },
    ReasoningEffortNotSupported {
        model: String,
        effort: String,
        supported: Vec<String>,
    },
    ContextTierNotSupported {
        model: String,
        tier: String,
        supported: Vec<String>,
    },
    ProviderOptionRequiresUrl {
        option: &'static str,
    },
    InvalidProviderConfiguration {
        message: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModelNotFound { model, available } => write!(
                formatter,
                "model '{model}' was not found; available models: {}",
                available.join(", ")
            ),
            Self::OptionRequiresModel { option } => {
                write!(formatter, "--{option} requires --model for validation")
            }
            Self::ReasoningEffortNotSupported {
                model,
                effort,
                supported,
            } => {
                let supported = if supported.is_empty() {
                    "none".to_string()
                } else {
                    supported.join(", ")
                };
                write!(
                    formatter,
                    "reasoning effort '{effort}' is not supported by model '{model}'; supported values: {supported}"
                )
            }
            Self::ContextTierNotSupported {
                model,
                tier,
                supported,
            } => {
                let supported = if supported.is_empty() {
                    "none".to_string()
                } else {
                    supported.join(", ")
                };
                write!(
                    formatter,
                    "context tier '{tier}' is not supported by model '{model}'; supported values: {supported}"
                )
            }
            Self::ProviderOptionRequiresUrl { option } => {
                write!(formatter, "{option} requires --provider-url")
            }
            Self::InvalidProviderConfiguration { message } => {
                write!(formatter, "provider configuration is invalid: {message}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Clone, Parser, PartialEq, Eq)]
#[command(
    name = "picopilot",
    version,
    about = "A minimalist Copilot coding agent"
)]
pub struct AppConfig {
    #[arg(value_name = "PROJECT", conflicts_with = "project")]
    pub project_path: Option<PathBuf>,

    #[arg(long, value_name = "PROJECT", conflicts_with = "project_path")]
    pub project: Option<PathBuf>,

    #[arg(long, value_name = "MODEL")]
    pub model: Option<String>,

    #[arg(long, value_name = "EFFORT")]
    pub reasoning_effort: Option<String>,

    #[arg(long, value_name = "TIER")]
    pub context_tier: Option<String>,

    #[arg(long, env = "PICOPILOT_REDUCED_MOTION")]
    pub reduced_motion: bool,

    #[arg(long, env = "PICOPILOT_PROVIDER_URL", value_name = "URL")]
    pub provider_url: Option<String>,

    #[arg(long, value_name = "NAME")]
    pub provider_name: Option<String>,

    #[arg(long, value_name = "API")]
    pub provider_wire_api: Option<String>,
}

impl AppConfig {
    pub fn working_directory(&self) -> Result<PathBuf, std::io::Error> {
        let current_directory = std::env::current_dir()?;
        Ok(
            match self.project.as_deref().or(self.project_path.as_deref()) {
                Some(project) if project.is_absolute() => project.to_path_buf(),
                Some(project) => current_directory.join(project),
                None => current_directory,
            },
        )
    }

    pub fn client_options_in(&self, working_directory: &Path) -> ClientOptions {
        ClientOptions::new().with_cwd(working_directory)
    }

    pub fn provider_settings(&self) -> Result<Option<ProviderSettings>, ConfigError> {
        let api_key = std::env::var("PICOPILOT_PROVIDER_API_KEY")
            .ok()
            .filter(|api_key| !api_key.trim().is_empty());
        if self.provider_url.is_none() {
            if self.provider_name.is_some() {
                return Err(ConfigError::ProviderOptionRequiresUrl {
                    option: "--provider-name",
                });
            }
            if self.provider_wire_api.is_some() {
                return Err(ConfigError::ProviderOptionRequiresUrl {
                    option: "--provider-wire-api",
                });
            }
            if api_key.is_some() {
                return Err(ConfigError::ProviderOptionRequiresUrl {
                    option: "PICOPILOT_PROVIDER_API_KEY",
                });
            }
            return Ok(None);
        }

        let name = self
            .provider_name
            .clone()
            .unwrap_or_else(|| DEFAULT_PROVIDER_NAME.to_string());
        let wire_api = self
            .provider_wire_api
            .clone()
            .unwrap_or_else(|| DEFAULT_PROVIDER_WIRE_API.to_string());
        ProviderSettings::new(
            name,
            self.provider_url.as_deref().unwrap_or_default(),
            wire_api,
            api_key,
        )
        .map(Some)
        .map_err(provider_config_error)
    }

    pub fn session_config(&self) -> SessionConfig {
        self.session_config_with_registry(None)
    }

    pub fn session_config_with_registry(
        &self,
        registry: Option<&ProviderRegistry>,
    ) -> SessionConfig {
        self.session_config_with_registry_and_toolset(registry, Toolset::all())
    }

    pub fn session_config_with_registry_and_toolset(
        &self,
        registry: Option<&ProviderRegistry>,
        toolset: Toolset,
    ) -> SessionConfig {
        let mut session = SessionConfig::default()
            .with_client_name("picopilot")
            .with_streaming(true)
            .with_available_tools(toolset.available_tools())
            .with_excluded_tools(EXCLUDED_TOOLS.iter().copied())
            .with_system_message(system_message_config());
        if let Some(registry) = registry {
            session = session
                .with_providers(registry.providers().to_vec())
                .with_models(registry.models().to_vec());
        }
        session.model = self.model.clone();
        session.reasoning_effort = self.reasoning_effort.clone();
        session.context_tier = self.context_tier.clone();
        session
    }

    pub fn session_config_in(&self, working_directory: impl Into<PathBuf>) -> SessionConfig {
        self.session_config_in_with_registry(working_directory, None)
    }

    pub fn session_config_in_with_registry(
        &self,
        working_directory: impl Into<PathBuf>,
        registry: Option<&ProviderRegistry>,
    ) -> SessionConfig {
        self.session_config_in_with_registry_and_toolset(
            working_directory,
            registry,
            Toolset::all(),
        )
    }

    pub fn session_config_in_with_registry_and_toolset(
        &self,
        working_directory: impl Into<PathBuf>,
        registry: Option<&ProviderRegistry>,
        toolset: Toolset,
    ) -> SessionConfig {
        let mut session = self.session_config_with_registry_and_toolset(registry, toolset);
        session.working_directory = Some(working_directory.into());
        session
    }

    pub fn validate_against(&self, models: &[Model]) -> Result<(), ConfigError> {
        let Some(model_id) = self.model.as_deref() else {
            if self.reasoning_effort.is_some() {
                return Err(ConfigError::OptionRequiresModel {
                    option: "reasoning-effort",
                });
            }
            if self.context_tier.is_some() {
                return Err(ConfigError::OptionRequiresModel {
                    option: "context-tier",
                });
            }
            return Ok(());
        };

        let Some(model) = models.iter().find(|candidate| candidate.id == model_id) else {
            return Err(ConfigError::ModelNotFound {
                model: model_id.to_string(),
                available: models
                    .iter()
                    .map(|candidate| candidate.id.clone())
                    .collect(),
            });
        };

        self.validate_model_options(model)
    }

    pub fn validate_against_registry(
        &self,
        hosted_models: &[Model],
        registry: &ProviderRegistry,
    ) -> Result<(), ConfigError> {
        let Some(model_id) = self.model.as_deref() else {
            if self.reasoning_effort.is_some() {
                return Err(ConfigError::OptionRequiresModel {
                    option: "reasoning-effort",
                });
            }
            if self.context_tier.is_some() {
                return Err(ConfigError::OptionRequiresModel {
                    option: "context-tier",
                });
            }
            return Ok(());
        };

        if let Some(model) = hosted_models
            .iter()
            .find(|candidate| candidate.id == model_id)
        {
            return self.validate_model_options(model);
        }

        if registry
            .qualified_model_ids()
            .iter()
            .any(|candidate| candidate == model_id)
        {
            let local_model = Model {
                id: model_id.to_string(),
                name: model_id.to_string(),
                ..Model::default()
            };
            return self.validate_model_options(&local_model);
        }

        let mut available = hosted_models
            .iter()
            .map(|candidate| candidate.id.clone())
            .collect::<Vec<_>>();
        available.extend(registry.qualified_model_ids());
        Err(ConfigError::ModelNotFound {
            model: model_id.to_string(),
            available,
        })
    }

    fn validate_model_options(&self, model: &Model) -> Result<(), ConfigError> {
        if let Some(effort) = self.reasoning_effort.as_deref() {
            let supported = model
                .supported_reasoning_efforts
                .clone()
                .unwrap_or_default();
            if !supported.iter().any(|candidate| candidate == effort) {
                return Err(ConfigError::ReasoningEffortNotSupported {
                    model: model.id.clone(),
                    effort: effort.to_string(),
                    supported,
                });
            }
        }

        if let Some(tier) = self.context_tier.as_deref() {
            let supported = supported_context_tiers(model);
            if !supported.iter().any(|candidate| candidate == tier) {
                return Err(ConfigError::ContextTierNotSupported {
                    model: model.id.clone(),
                    tier: tier.to_string(),
                    supported,
                });
            }
        }

        Ok(())
    }
}

fn provider_config_error(error: ProviderError) -> ConfigError {
    ConfigError::InvalidProviderConfiguration {
        message: error.to_string(),
    }
}

pub(crate) fn supported_context_tiers(model: &Model) -> Vec<String> {
    let mut supported = model.supported_context_tiers.clone().unwrap_or_default();
    if let Some(token_prices) = model
        .billing
        .as_ref()
        .and_then(|billing| billing.token_prices.as_ref())
    {
        if !supported.iter().any(|tier| tier == "default") {
            supported.push("default".to_string());
        }
        if token_prices.long_context.is_some()
            && !supported.iter().any(|tier| tier == "long_context")
        {
            supported.push("long_context".to_string());
        }
    }
    supported
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use clap::Parser;
    use github_copilot_sdk::types::{Model, ModelCapabilities};

    use crate::provider::{ProviderRegistry, ProviderSettings};
    use crate::toolset::Toolset;

    use super::{system_message_config, AppConfig};

    #[test]
    fn parses_startup_model_overrides() {
        let config = AppConfig::try_parse_from([
            "picopilot",
            "--model",
            "claude-sonnet-4.5",
            "--reasoning-effort",
            "high",
            "--context-tier",
            "long_context",
        ])
        .expect("valid startup options should parse");

        assert_eq!(config.model.as_deref(), Some("claude-sonnet-4.5"));
        assert_eq!(config.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(config.context_tier.as_deref(), Some("long_context"));
    }

    #[test]
    fn parses_a_positional_project_path() {
        let config = AppConfig::try_parse_from(["picopilot", "projects/demo"])
            .expect("a positional project path should parse");

        assert_eq!(
            config.project_path.as_deref(),
            Some(Path::new("projects/demo"))
        );
        assert!(config.project.is_none());
    }

    #[test]
    fn parses_and_resolves_the_named_project_path() {
        let config = AppConfig::try_parse_from(["picopilot", "--project", "projects/demo"])
            .expect("the named project path should parse");
        let expected = std::env::current_dir()
            .expect("the test should have a current directory")
            .join("projects/demo");

        assert_eq!(config.working_directory().unwrap(), expected);
        assert!(config.project_path.is_none());
    }

    #[test]
    fn rejects_both_project_path_forms() {
        let error = AppConfig::try_parse_from([
            "picopilot",
            "projects/demo",
            "--project",
            "projects/other",
        ])
        .expect_err("the positional and named project paths should conflict");

        assert!(error.to_string().contains("cannot be used with"));
    }

    #[test]
    fn rejects_reasoning_effort_not_supported_by_selected_model() {
        let config = AppConfig::try_parse_from([
            "picopilot",
            "--model",
            "claude-sonnet-4.5",
            "--reasoning-effort",
            "medium",
        ])
        .expect("valid startup options should parse");
        let model = Model {
            capabilities: ModelCapabilities::default(),
            id: "claude-sonnet-4.5".to_string(),
            name: "Claude Sonnet".to_string(),
            supported_reasoning_efforts: Some(vec!["low".to_string(), "high".to_string()]),
            ..Default::default()
        };

        let error = config
            .validate_against(&[model])
            .expect_err("unsupported reasoning effort should fail validation");

        assert_eq!(
            error.to_string(),
            "reasoning effort 'medium' is not supported by model 'claude-sonnet-4.5'; supported values: low, high"
        );
    }

    #[test]
    fn rejects_an_unknown_startup_model() {
        let config = AppConfig::try_parse_from(["picopilot", "--model", "missing-model"])
            .expect("valid startup options should parse");

        let error = config
            .validate_against(&[Model {
                id: "gpt-5".to_string(),
                name: "GPT-5".to_string(),
                ..Default::default()
            }])
            .expect_err("unknown model should fail validation");

        assert_eq!(
            error.to_string(),
            "model 'missing-model' was not found; available models: gpt-5"
        );
    }

    #[test]
    fn rejects_a_context_tier_not_supported_by_selected_model() {
        let config = AppConfig::try_parse_from([
            "picopilot",
            "--model",
            "gpt-5",
            "--context-tier",
            "long_context",
        ])
        .expect("valid startup options should parse");
        let model = Model {
            id: "gpt-5".to_string(),
            name: "GPT-5".to_string(),
            supported_context_tiers: Some(vec!["default".to_string()]),
            ..Default::default()
        };

        let error = config
            .validate_against(&[model])
            .expect_err("unsupported context tier should fail validation");

        assert_eq!(
            error.to_string(),
            "context tier 'long_context' is not supported by model 'gpt-5'; supported values: default"
        );
    }

    #[test]
    fn builds_a_streaming_session_with_the_v1_tool_policy() {
        let config = AppConfig::try_parse_from([
            "picopilot",
            "--model",
            "claude-sonnet-4.5",
            "--reasoning-effort",
            "high",
            "--context-tier",
            "long_context",
        ])
        .expect("valid startup options should parse");

        let session = config.session_config();

        assert_eq!(session.model.as_deref(), Some("claude-sonnet-4.5"));
        assert_eq!(session.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(session.context_tier.as_deref(), Some("long_context"));
        assert_eq!(session.streaming, Some(true));
        let shell_tool = if cfg!(windows) { "powershell" } else { "bash" };
        assert_eq!(
            session.available_tools,
            Some(
                [shell_tool, "view", "edit", "create", "grep", "glob", "task"]
                    .into_iter()
                    .map(String::from)
                    .collect()
            )
        );
        assert_eq!(
            session.excluded_tools,
            Some(
                ["web_fetch", "web_search"]
                    .into_iter()
                    .map(String::from)
                    .collect()
            )
        );
        assert_eq!(
            session
                .system_message
                .as_ref()
                .and_then(|config| config.mode.as_deref()),
            Some("replace")
        );
        assert_eq!(
            session
                .system_message
                .as_ref()
                .and_then(|config| config.content.as_deref()),
            Some("")
        );
        assert!(session
            .system_message
            .as_ref()
            .and_then(|config| config.sections.as_ref())
            .is_none());
        assert!(session.system_message_transform.is_none());
    }

    #[test]
    fn serializes_shell_only_and_empty_toolsets_as_explicit_allowlists() {
        let config =
            AppConfig::try_parse_from(["picopilot"]).expect("default options should parse");

        let shell_only =
            config.session_config_with_registry_and_toolset(None, Toolset::shell_only());
        assert_eq!(
            shell_only.available_tools,
            Some(vec![if cfg!(windows) {
                "powershell".to_string()
            } else {
                "bash".to_string()
            }])
        );

        let empty = config.session_config_with_registry_and_toolset(None, Toolset::empty());
        assert_eq!(empty.available_tools, Some(Vec::new()));
    }

    #[test]
    fn replaces_the_system_message_with_empty_content() {
        let config = system_message_config();
        assert_eq!(config.mode.as_deref(), Some("replace"));
        assert_eq!(config.content.as_deref(), Some(""));
        assert!(config.sections.is_none());
    }

    #[test]
    fn propagates_the_working_directory_to_client_and_session() {
        let config =
            AppConfig::try_parse_from(["picopilot"]).expect("default options should parse");
        let working_directory = Path::new("C:\\dev\\picopilot");

        let client_options = config.client_options_in(working_directory);
        let session_config = config.session_config_in(working_directory);

        assert_eq!(client_options.working_directory, working_directory);
        assert_eq!(
            session_config.working_directory.as_deref(),
            Some(working_directory)
        );
    }

    #[test]
    fn parses_provider_options_without_exposing_an_api_key_flag() {
        let config = AppConfig::try_parse_from([
            "picopilot",
            "--provider-url",
            "http://localhost:11434/v1/",
            "--provider-name",
            "ollama",
            "--provider-wire-api",
            "responses",
        ])
        .expect("valid provider options should parse");

        let settings = config
            .provider_settings()
            .expect("provider settings should be valid")
            .expect("provider URL should enable provider settings");
        assert_eq!(settings.name, "ollama");
        assert_eq!(settings.base_url, "http://localhost:11434/v1");
        assert_eq!(settings.wire_api, "responses");
    }

    #[test]
    fn rejects_provider_options_without_a_provider_url() {
        let config = AppConfig::try_parse_from(["picopilot", "--provider-name", "ollama"])
            .expect("provider name should parse");

        assert_eq!(
            config.provider_settings().unwrap_err().to_string(),
            "--provider-name requires --provider-url"
        );
    }

    #[test]
    fn applies_a_registry_to_session_creation_without_changing_hosted_defaults() {
        let config =
            AppConfig::try_parse_from(["picopilot"]).expect("default options should parse");
        let settings = ProviderSettings::default_for("http://localhost:11434/v1").unwrap();
        let registry = ProviderRegistry::from_model_ids(&settings, ["qwen:7b"]).unwrap();

        let session = config.session_config_with_registry(Some(&registry));

        assert_eq!(session.providers.as_ref().map(Vec::len), Some(1));
        assert_eq!(session.models.as_ref().map(Vec::len), Some(1));
        assert_eq!(session.models.as_ref().unwrap()[0].id, "qwen:7b");
        assert!(session.provider.is_none());
    }

    #[test]
    fn validates_qualified_local_models_without_inventing_capabilities() {
        let config = AppConfig::try_parse_from([
            "picopilot",
            "--model",
            "local/qwen:7b",
            "--context-tier",
            "long_context",
        ])
        .expect("valid local model options should parse");
        let settings = ProviderSettings::default_for("http://localhost:11434/v1").unwrap();
        let registry = ProviderRegistry::from_model_ids(&settings, ["qwen:7b"]).unwrap();

        let error = config
            .validate_against_registry(
                &[Model {
                    id: "gpt-5".to_string(),
                    name: "GPT-5".to_string(),
                    ..Model::default()
                }],
                &registry,
            )
            .expect_err("local models must not accept unknown context tiers");

        assert_eq!(
            error.to_string(),
            "context tier 'long_context' is not supported by model 'local/qwen:7b'; supported values: none"
        );
    }
}
