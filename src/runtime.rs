use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use github_copilot_sdk::{
    handler::PermissionHandler,
    types::{DeliveryMode, MessageOptions, Model, ResumeSessionConfig, SessionEvent, SessionId},
    Client, ClientOptions, Error as SdkError,
};
use serde_json::Value;

use crate::config::{AppConfig, ConfigError};
use crate::permissions::{permission_handler, ApprovalRequest};
use crate::provider::{ProviderError, ProviderRegistry};

pub const MAX_RECOVERY_ATTEMPTS: usize = 3;
const RECOVERY_INSTRUCTION: &str = "The client transport failed while a tool call may have been in flight. Its outcome is unknown. Before continuing, inspect the relevant state; do not assume success and do not retry the operation blindly.";
const RECOVERY_DISPLAY_PROMPT: &str =
    "Connection recovered. Verify any in-flight tool outcome before continuing.";

fn recovery_message() -> MessageOptions {
    MessageOptions::new(RECOVERY_INSTRUCTION)
        .with_display_prompt(RECOVERY_DISPLAY_PROMPT)
        .with_mode(DeliveryMode::Immediate)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionIdentity {
    session_id: SessionId,
    start_time: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActiveModelOptions {
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub context_tier: Option<String>,
}

pub struct AppRuntime {
    pub client: Client,
    pub permission_requests: tokio::sync::mpsc::UnboundedReceiver<ApprovalRequest>,
    permission_handler: Arc<dyn PermissionHandler>,
    pub session: github_copilot_sdk::session::Session,
    pub models: Vec<Model>,
    pub provider_registry: Option<ProviderRegistry>,
    pub working_directory: PathBuf,
    pub active_model_options: ActiveModelOptions,
    startup_config: AppConfig,
    session_start_time: Option<String>,
}

fn apply_active_model_options(config: &mut ResumeSessionConfig, options: &ActiveModelOptions) {
    config.model = options.model.clone();
    config.reasoning_effort = options.reasoning_effort.clone();
    config.context_tier = options.context_tier.clone();
}

fn apply_provider_registry(
    config: ResumeSessionConfig,
    registry: Option<&ProviderRegistry>,
) -> ResumeSessionConfig {
    match registry {
        Some(registry) => config
            .with_providers(registry.providers().to_vec())
            .with_models(registry.models().to_vec()),
        None => config,
    }
}

#[cfg(test)]
mod tests {
    use github_copilot_sdk::types::{ResumeSessionConfig, SessionId};

    use super::{
        apply_active_model_options, apply_provider_registry, models_from_session_catalog,
        recovery_backoff, recovery_message, verify_session_identity, ActiveModelOptions,
        CatalogError, SessionIdentity, RECOVERY_DISPLAY_PROMPT, RECOVERY_INSTRUCTION,
    };
    use crate::provider::{ProviderRegistry, ProviderSettings};

    #[test]
    fn resume_configuration_restores_active_model_options() {
        let mut config = ResumeSessionConfig::new(SessionId::from("session-1"));
        let options = ActiveModelOptions {
            model: Some("gpt-5".to_string()),
            reasoning_effort: Some("high".to_string()),
            context_tier: Some("long_context".to_string()),
        };

        apply_active_model_options(&mut config, &options);

        assert_eq!(config.model.as_deref(), Some("gpt-5"));
        assert_eq!(config.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(config.context_tier.as_deref(), Some("long_context"));
    }

    #[test]
    fn resume_identity_requires_the_same_session_and_start_time() {
        let expected = SessionIdentity {
            session_id: SessionId::from("session-1"),
            start_time: Some("2026-08-31T12:00:00Z".to_string()),
        };
        let actual = expected.clone();

        verify_session_identity(&expected, &actual)
            .expect("matching session identity should be accepted");
        assert!(verify_session_identity(
            &expected,
            &SessionIdentity {
                session_id: SessionId::from("session-2"),
                ..actual.clone()
            }
        )
        .is_err());
        assert!(verify_session_identity(
            &expected,
            &SessionIdentity {
                start_time: Some("2026-08-31T12:01:00Z".to_string()),
                ..actual
            }
        )
        .is_err());
    }

    #[test]
    fn recovery_backoff_grows_with_each_attempt() {
        assert_eq!(recovery_backoff(1).as_millis(), 250);
        assert_eq!(recovery_backoff(2).as_millis(), 500);
        assert_eq!(recovery_backoff(3).as_millis(), 1_000);
    }

    #[test]
    fn recovery_message_tells_the_agent_to_verify_unknown_tool_state_immediately() {
        let message = recovery_message();

        assert_eq!(message.prompt, RECOVERY_INSTRUCTION);
        assert_eq!(
            message.display_prompt.as_deref(),
            Some(RECOVERY_DISPLAY_PROMPT)
        );
        assert_eq!(
            message.mode,
            Some(github_copilot_sdk::types::DeliveryMode::Immediate)
        );
    }

    #[test]
    fn resume_configuration_reuses_the_additive_provider_registry() {
        let settings = ProviderSettings::default_for("http://localhost:11434/v1").unwrap();
        let registry = ProviderRegistry::from_model_ids(&settings, ["qwen:7b"]).unwrap();
        let config = apply_provider_registry(
            ResumeSessionConfig::new(SessionId::from("session-1")),
            Some(&registry),
        );

        assert_eq!(config.providers.as_ref().map(Vec::len), Some(1));
        assert_eq!(config.models.as_ref().map(Vec::len), Some(1));
        assert_eq!(config.models.as_ref().unwrap()[0].provider, "local");
        assert_eq!(config.models.as_ref().unwrap()[0].id, "qwen:7b");
    }

    #[test]
    fn decodes_hosted_and_provider_models_from_the_session_catalog() {
        let models = models_from_session_catalog(vec![
            serde_json::json!({
                "id": "gpt-5",
                "name": "GPT-5",
                "capabilities": {}
            }),
            serde_json::json!({"id": "local/qwen:7b", "name": "local/qwen:7b"}),
        ])
        .expect("representative session catalog should decode");

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gpt-5");
        assert_eq!(models[1].id, "local/qwen:7b");
        assert!(models[1].billing.is_none());
        assert!(models[1].supported_reasoning_efforts.is_none());
        assert!(models[1].supported_context_tiers.is_none());
        assert!(models[1].capabilities.limits.is_none());
    }

    #[test]
    fn rejects_a_session_catalog_entry_without_an_id() {
        assert!(matches!(
            models_from_session_catalog(vec![serde_json::json!({"name": "missing"})]),
            Err(CatalogError::MissingModelId { index: 0 })
        ));
    }
}

#[derive(Debug)]
pub enum ResumeError {
    Session(SdkError),
    MissingSession {
        session_id: SessionId,
    },
    IdentityMismatch {
        expected: SessionId,
        actual: SessionId,
        expected_start_time: Option<String>,
        actual_start_time: Option<String>,
    },
}

impl fmt::Display for ResumeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session(error) => write!(formatter, "could not resume session: {error}"),
            Self::MissingSession { session_id } => {
                write!(formatter, "session '{session_id}' could not be found")
            }
            Self::IdentityMismatch {
                expected,
                actual,
                expected_start_time,
                actual_start_time,
            } => write!(
                formatter,
                "resume returned session '{actual}' instead of requested session '{expected}' (start time expected {expected_start_time:?}, actual {actual_start_time:?})"
            ),
        }
    }
}

impl std::error::Error for ResumeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Session(error) => Some(error),
            Self::MissingSession { .. } | Self::IdentityMismatch { .. } => None,
        }
    }
}

impl ResumeError {
    pub fn is_transport_failure(&self) -> bool {
        matches!(self, Self::Session(error) if error.is_transport_failure())
    }
}

#[derive(Debug)]
pub enum RecoveryError {
    Client(SdkError),
    Resume(ResumeError),
    Notify(SdkError),
}

impl fmt::Display for RecoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Client(error) => write!(formatter, "could not restart Copilot: {error}"),
            Self::Resume(error) => write!(formatter, "could not reconnect session: {error}"),
            Self::Notify(error) => {
                write!(formatter, "could not notify the resumed agent: {error}")
            }
        }
    }
}

impl std::error::Error for RecoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Client(error) => Some(error),
            Self::Resume(error) => Some(error),
            Self::Notify(error) => Some(error),
        }
    }
}

fn verify_session_identity(
    expected: &SessionIdentity,
    actual: &SessionIdentity,
) -> Result<(), ResumeError> {
    let start_time_mismatch =
        expected.start_time.is_some() && expected.start_time != actual.start_time;
    if expected.session_id != actual.session_id || start_time_mismatch {
        return Err(ResumeError::IdentityMismatch {
            expected: expected.session_id.clone(),
            actual: actual.session_id.clone(),
            expected_start_time: expected.start_time.clone(),
            actual_start_time: actual.start_time.clone(),
        });
    }
    Ok(())
}

pub fn recovery_backoff(attempt: usize) -> Duration {
    let exponent = attempt.saturating_sub(1).min(2);
    Duration::from_millis(250 * (1_u64 << exponent))
}

impl AppRuntime {
    pub fn set_active_model_options(
        &mut self,
        model: String,
        reasoning_effort: Option<String>,
        context_tier: Option<String>,
    ) {
        self.active_model_options = ActiveModelOptions {
            model: Some(model),
            reasoning_effort,
            context_tier,
        };
    }

    pub async fn resume(
        &mut self,
        session_id: SessionId,
    ) -> Result<Vec<SessionEvent>, ResumeError> {
        let expected_metadata = self
            .client
            .get_session_metadata(&session_id)
            .await
            .map_err(ResumeError::Session)?
            .ok_or_else(|| ResumeError::MissingSession {
                session_id: session_id.clone(),
            })?;
        let expected_start_time = Some(expected_metadata.start_time);

        self.session
            .disconnect()
            .await
            .map_err(ResumeError::Session)?;

        let (resumed, actual_start_time) = self
            .resume_on_client(
                &self.client,
                SessionIdentity {
                    session_id,
                    start_time: expected_start_time,
                },
            )
            .await?;

        self.session = resumed;
        self.session_start_time = actual_start_time;
        self.session
            .get_events()
            .await
            .map_err(ResumeError::Session)
    }

    pub async fn recover_transport(&mut self) -> Result<(), RecoveryError> {
        let expected = SessionIdentity {
            session_id: self.session.id().clone(),
            start_time: self.session_start_time.clone(),
        };
        self.session.stop_event_loop().await;
        self.client.force_stop();

        let replacement = Client::start(self.client_options())
            .await
            .map_err(RecoveryError::Client)?;
        let (resumed, actual_start_time) = self
            .resume_on_client(&replacement, expected)
            .await
            .map_err(RecoveryError::Resume)?;
        resumed
            .send(recovery_message())
            .await
            .map_err(RecoveryError::Notify)?;

        let _old_client = std::mem::replace(&mut self.client, replacement);
        self.session = resumed;
        self.session_start_time = actual_start_time;
        Ok(())
    }

    async fn resume_on_client(
        &self,
        client: &Client,
        expected: SessionIdentity,
    ) -> Result<(github_copilot_sdk::session::Session, Option<String>), ResumeError> {
        let resumed = client
            .resume_session(self.resume_config(expected.session_id.clone()))
            .await
            .map_err(ResumeError::Session)?;
        let actual_start_time = match client.get_session_metadata(resumed.id()).await {
            Ok(metadata) => metadata.map(|metadata| metadata.start_time),
            Err(error) => {
                let _ = resumed.disconnect().await;
                return Err(ResumeError::Session(error));
            }
        };
        let actual = SessionIdentity {
            session_id: resumed.id().clone(),
            start_time: actual_start_time.clone(),
        };
        if let Err(error) = verify_session_identity(&expected, &actual) {
            let _ = resumed.disconnect().await;
            return Err(error);
        }
        Ok((resumed, actual_start_time))
    }

    fn client_options(&self) -> ClientOptions {
        self.startup_config
            .client_options_in(&self.working_directory)
    }

    fn resume_config(&self, session_id: SessionId) -> ResumeSessionConfig {
        let mut config = self.base_resume_config(session_id);
        apply_active_model_options(&mut config, &self.active_model_options);
        config
    }

    fn base_resume_config(&self, session_id: SessionId) -> ResumeSessionConfig {
        let config = ResumeSessionConfig::new(session_id)
            .with_client_name("picopilot")
            .with_streaming(true)
            .with_available_tools(crate::config::V1_AVAILABLE_TOOLS.iter().copied())
            .with_excluded_tools(crate::config::V1_EXCLUDED_TOOLS.iter().copied())
            .with_working_directory(self.working_directory.clone())
            .with_permission_handler(self.permission_handler.clone())
            .with_system_message(crate::config::system_message_config())
            .with_system_message_transform(crate::config::system_message_transform())
            .with_suppress_resume_event(true);
        apply_provider_registry(config, self.provider_registry.as_ref())
    }
}

#[derive(Debug)]
pub enum StartupError {
    CurrentDirectory(std::io::Error),
    Client(SdkError),
    Configuration(ConfigError),
    ProviderDiscovery(ProviderError),
    Session(SdkError),
    SessionCatalog(SdkError),
    InvalidSessionCatalog(CatalogError),
}

impl fmt::Display for StartupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CurrentDirectory(error) => {
                write!(
                    formatter,
                    "could not determine the working directory: {error}"
                )
            }
            Self::Client(error) => write!(formatter, "could not start Copilot: {error}"),
            Self::Configuration(error) => {
                write!(formatter, "invalid startup configuration: {error}")
            }
            Self::ProviderDiscovery(error) => {
                write!(formatter, "could not discover provider models: {error}")
            }
            Self::Session(error) => write!(formatter, "could not create Copilot session: {error}"),
            Self::SessionCatalog(error) => {
                write!(formatter, "could not list session models: {error}")
            }
            Self::InvalidSessionCatalog(error) => {
                write!(formatter, "could not decode session model catalog: {error}")
            }
        }
    }
}

impl std::error::Error for StartupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CurrentDirectory(error) => Some(error),
            Self::Client(error) | Self::Session(error) | Self::SessionCatalog(error) => Some(error),
            Self::Configuration(error) => Some(error),
            Self::ProviderDiscovery(error) => Some(error),
            Self::InvalidSessionCatalog(error) => Some(error),
        }
    }
}

#[derive(Debug)]
pub enum CatalogError {
    InvalidEntry {
        index: usize,
        source: serde_json::Error,
    },
    MissingModelId {
        index: usize,
    },
    MissingRegisteredModel {
        model: String,
    },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEntry { index, .. } => {
                write!(formatter, "session model catalog entry {index} is invalid")
            }
            Self::MissingModelId { index } => {
                write!(
                    formatter,
                    "session model catalog entry {index} has no model id"
                )
            }
            Self::MissingRegisteredModel { model } => write!(
                formatter,
                "session model catalog did not include registered model '{model}'"
            ),
        }
    }
}

impl std::error::Error for CatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidEntry { source, .. } => Some(source),
            Self::MissingModelId { .. } | Self::MissingRegisteredModel { .. } => None,
        }
    }
}

fn models_from_session_catalog(entries: Vec<Value>) -> Result<Vec<Model>, CatalogError> {
    entries
        .into_iter()
        .enumerate()
        .map(|(index, mut entry)| decode_session_model(index, &mut entry))
        .collect()
}

fn decode_session_model(index: usize, entry: &mut Value) -> Result<Model, CatalogError> {
    let Some(object) = entry.as_object_mut() else {
        let source = serde_json::from_value::<Model>(entry.clone()).unwrap_err();
        return Err(CatalogError::InvalidEntry { index, source });
    };
    let model_id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|model_id| !model_id.trim().is_empty())
        .ok_or(CatalogError::MissingModelId { index })?
        .to_string();
    if matches!(object.get("name"), None | Some(Value::Null)) {
        object.insert("name".to_string(), Value::String(model_id.clone()));
    }
    if matches!(object.get("capabilities"), None | Some(Value::Null)) {
        object.insert(
            "capabilities".to_string(),
            Value::Object(serde_json::Map::new()),
        );
    }
    serde_json::from_value(entry.clone())
        .map_err(|source| CatalogError::InvalidEntry { index, source })
}

pub async fn connect(config: &AppConfig) -> Result<AppRuntime, StartupError> {
    let working_directory = std::env::current_dir().map_err(StartupError::CurrentDirectory)?;
    let (permission_handler, permission_requests) = permission_handler(working_directory.clone());
    let provider_settings = config
        .provider_settings()
        .map_err(StartupError::Configuration)?;
    let client = Client::start(config.client_options_in(&working_directory))
        .await
        .map_err(StartupError::Client)?;
    let hosted_models = client.list_models().await.map_err(StartupError::Client)?;
    let provider_registry = match provider_settings.as_ref() {
        Some(settings) => Some(
            crate::provider::discover(settings)
                .await
                .map_err(StartupError::ProviderDiscovery)?,
        ),
        None => None,
    };

    match provider_registry.as_ref() {
        Some(registry) => config
            .validate_against_registry(&hosted_models, registry)
            .map_err(StartupError::Configuration)?,
        None => config
            .validate_against(&hosted_models)
            .map_err(StartupError::Configuration)?,
    }

    let session = client
        .create_session(
            config
                .session_config_in_with_registry(&working_directory, provider_registry.as_ref())
                .with_permission_handler(permission_handler.clone()),
        )
        .await
        .map_err(StartupError::Session)?;
    let models = match provider_registry.as_ref() {
        Some(registry) => {
            let catalog = session
                .rpc()
                .model()
                .list()
                .await
                .map_err(StartupError::SessionCatalog)?;
            let models = models_from_session_catalog(catalog.list)
                .map_err(StartupError::InvalidSessionCatalog)?;
            for model_id in registry.qualified_model_ids() {
                if !models.iter().any(|model| model.id == model_id) {
                    return Err(StartupError::InvalidSessionCatalog(
                        CatalogError::MissingRegisteredModel { model: model_id },
                    ));
                }
            }
            models
        }
        None => hosted_models,
    };
    let session_start_time = client
        .get_session_metadata(session.id())
        .await
        .map_err(StartupError::Client)?
        .map(|metadata| metadata.start_time);

    Ok(AppRuntime {
        client,
        permission_requests,
        permission_handler,
        session,
        models,
        provider_registry,
        working_directory,
        startup_config: config.clone(),
        session_start_time,
        active_model_options: ActiveModelOptions {
            model: config.model.clone(),
            reasoning_effort: config.reasoning_effort.clone(),
            context_tier: config.context_tier.clone(),
        },
    })
}
