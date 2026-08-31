use std::fmt;
use std::path::{Path, PathBuf};

use clap::Parser;
use github_copilot_sdk::{
    types::{Model, SessionConfig},
    ClientOptions,
};

pub const V1_AVAILABLE_TOOLS: &[&str] = &["bash", "view", "edit", "create", "grep", "glob", "task"];
pub const V1_EXCLUDED_TOOLS: &[&str] = &["web_fetch", "web_search"];

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
            } => write!(
                formatter,
                "reasoning effort '{effort}' is not supported by model '{model}'; supported values: {}",
                supported.join(", ")
            ),
            Self::ContextTierNotSupported {
                model,
                tier,
                supported,
            } => write!(
                formatter,
                "context tier '{tier}' is not supported by model '{model}'; supported values: {}",
                supported.join(", ")
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Parser, PartialEq, Eq)]
#[command(
    name = "picopilot",
    version,
    about = "A minimalist Copilot coding agent"
)]
pub struct AppConfig {
    #[arg(long, value_name = "MODEL")]
    pub model: Option<String>,

    #[arg(long, value_name = "EFFORT")]
    pub reasoning_effort: Option<String>,

    #[arg(long, value_name = "TIER")]
    pub context_tier: Option<String>,
}

impl AppConfig {
    pub fn client_options_in(&self, working_directory: &Path) -> ClientOptions {
        ClientOptions::new().with_cwd(working_directory)
    }

    pub fn session_config(&self) -> SessionConfig {
        let mut session = SessionConfig::default()
            .with_client_name("picopilot")
            .with_streaming(true)
            .with_available_tools(V1_AVAILABLE_TOOLS.iter().copied())
            .with_excluded_tools(V1_EXCLUDED_TOOLS.iter().copied());
        session.model = self.model.clone();
        session.reasoning_effort = self.reasoning_effort.clone();
        session.context_tier = self.context_tier.clone();
        session
    }

    pub fn session_config_in(&self, working_directory: impl Into<PathBuf>) -> SessionConfig {
        let mut session = self.session_config();
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

    use super::AppConfig;

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
        assert_eq!(
            session.available_tools,
            Some(
                ["bash", "view", "edit", "create", "grep", "glob", "task"]
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
}
