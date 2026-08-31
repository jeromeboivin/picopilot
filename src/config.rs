use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use clap::Parser;
use github_copilot_sdk::{
    transforms::{SystemMessageTransform, TransformContext},
    types::{Model, SectionOverride, SessionConfig, SystemMessageConfig},
    ClientOptions,
};

#[cfg(windows)]
pub const V1_AVAILABLE_TOOLS: &[&str] = &[
    "powershell",
    "view",
    "edit",
    "create",
    "grep",
    "glob",
    "task",
];

#[cfg(not(windows))]
pub const V1_AVAILABLE_TOOLS: &[&str] = &["bash", "view", "edit", "create", "grep", "glob", "task"];
pub const V1_EXCLUDED_TOOLS: &[&str] = &["web_fetch", "web_search"];
const CONCISE_TONE: &str = "Be concise, direct, and professional.";

struct PicopilotSystemMessageTransform;

#[async_trait]
impl SystemMessageTransform for PicopilotSystemMessageTransform {
    fn section_ids(&self) -> Vec<String> {
        vec!["tone".to_string()]
    }

    async fn transform_section(
        &self,
        section_id: &str,
        _content: &str,
        _context: TransformContext,
    ) -> Option<String> {
        match section_id {
            "tone" => Some(CONCISE_TONE.to_string()),
            _ => None,
        }
    }
}

pub(crate) fn system_message_config() -> SystemMessageConfig {
    let sections = ["guidelines", "custom_instructions"]
        .into_iter()
        .map(|section_id| {
            (
                section_id.to_string(),
                SectionOverride {
                    action: Some("remove".to_string()),
                    content: None,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    SystemMessageConfig::new()
        .with_mode("customize")
        .with_sections(sections)
}

pub(crate) fn system_message_transform() -> Arc<dyn SystemMessageTransform> {
    Arc::new(PicopilotSystemMessageTransform)
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

#[derive(Debug, Clone, Parser, PartialEq, Eq)]
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
            .with_excluded_tools(V1_EXCLUDED_TOOLS.iter().copied())
            .with_system_message(system_message_config())
            .with_system_message_transform(system_message_transform());
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
    use github_copilot_sdk::transforms::{SystemMessageTransform, TransformContext};
    use github_copilot_sdk::types::{Model, ModelCapabilities, SessionId};

    use super::{system_message_config, AppConfig, PicopilotSystemMessageTransform, CONCISE_TONE};

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
            Some("customize")
        );
        assert!(session.system_message_transform.is_some());
    }

    #[test]
    fn removes_only_the_planned_system_message_sections() {
        let config = system_message_config();
        let sections = config
            .sections
            .expect("section overrides should be configured");

        assert_eq!(sections.len(), 2);
        assert_eq!(sections["guidelines"].action.as_deref(), Some("remove"));
        assert_eq!(
            sections["custom_instructions"].action.as_deref(),
            Some("remove")
        );
    }

    #[tokio::test]
    async fn rewrites_tone_but_preserves_runtime_instructions() {
        let transform = PicopilotSystemMessageTransform;
        let context = TransformContext {
            session_id: SessionId::from("session-1"),
        };

        assert_eq!(transform.section_ids(), vec!["tone"]);
        assert_eq!(
            transform
                .transform_section("tone", "long default tone", context.clone())
                .await
                .as_deref(),
            Some(CONCISE_TONE)
        );
        assert_eq!(
            transform
                .transform_section(
                    "runtime_instructions",
                    "long default runtime instructions",
                    context,
                )
                .await,
            None
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
