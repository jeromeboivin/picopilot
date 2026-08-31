use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use github_copilot_sdk::{
    handler::PermissionHandler,
    types::{Model, ResumeSessionConfig, SessionId},
    Client, Error as SdkError,
};

use crate::config::{AppConfig, ConfigError};
use crate::permissions::{permission_handler, ApprovalRequest};

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
    pub working_directory: PathBuf,
    pub active_model_options: ActiveModelOptions,
}

fn apply_active_model_options(config: &mut ResumeSessionConfig, options: &ActiveModelOptions) {
    config.model = options.model.clone();
    config.reasoning_effort = options.reasoning_effort.clone();
    config.context_tier = options.context_tier.clone();
}

#[cfg(test)]
mod tests {
    use github_copilot_sdk::types::{ResumeSessionConfig, SessionId};

    use super::{apply_active_model_options, ActiveModelOptions};

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
}

#[derive(Debug)]
pub enum ResumeError {
    Session(SdkError),
    IdentityMismatch {
        expected: SessionId,
        actual: SessionId,
    },
}

impl fmt::Display for ResumeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session(error) => write!(formatter, "could not resume session: {error}"),
            Self::IdentityMismatch { expected, actual } => write!(
                formatter,
                "resume returned session '{actual}' instead of requested session '{expected}'"
            ),
        }
    }
}

impl std::error::Error for ResumeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Session(error) => Some(error),
            Self::IdentityMismatch { .. } => None,
        }
    }
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

    pub async fn resume(&mut self, session_id: SessionId) -> Result<(), ResumeError> {
        self.session
            .disconnect()
            .await
            .map_err(ResumeError::Session)?;

        let resumed = self
            .client
            .resume_session(self.resume_config(session_id.clone()))
            .await
            .map_err(ResumeError::Session)?;
        if resumed.id() != &session_id {
            let actual = resumed.id().clone();
            let _ = resumed.disconnect().await;
            return Err(ResumeError::IdentityMismatch {
                expected: session_id,
                actual,
            });
        }

        self.session = resumed;
        Ok(())
    }

    fn resume_config(&self, session_id: SessionId) -> ResumeSessionConfig {
        let mut config = ResumeSessionConfig::new(session_id)
            .with_client_name("picopilot")
            .with_streaming(true)
            .with_available_tools(crate::config::V1_AVAILABLE_TOOLS.iter().copied())
            .with_excluded_tools(crate::config::V1_EXCLUDED_TOOLS.iter().copied())
            .with_working_directory(self.working_directory.clone())
            .with_permission_handler(self.permission_handler.clone())
            .with_suppress_resume_event(true);
        apply_active_model_options(&mut config, &self.active_model_options);
        config
    }
}

#[derive(Debug)]
pub enum StartupError {
    CurrentDirectory(std::io::Error),
    Client(SdkError),
    Configuration(ConfigError),
    Session(SdkError),
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
            Self::Session(error) => write!(formatter, "could not create Copilot session: {error}"),
        }
    }
}

impl std::error::Error for StartupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CurrentDirectory(error) => Some(error),
            Self::Client(error) | Self::Session(error) => Some(error),
            Self::Configuration(error) => Some(error),
        }
    }
}

pub async fn connect(config: &AppConfig) -> Result<AppRuntime, StartupError> {
    let working_directory = std::env::current_dir().map_err(StartupError::CurrentDirectory)?;
    let (permission_handler, permission_requests) = permission_handler(working_directory.clone());
    let client = Client::start(config.client_options_in(&working_directory))
        .await
        .map_err(StartupError::Client)?;
    let models = client.list_models().await.map_err(StartupError::Client)?;

    config
        .validate_against(&models)
        .map_err(StartupError::Configuration)?;

    let session = client
        .create_session(
            config
                .session_config_in(&working_directory)
                .with_permission_handler(permission_handler.clone()),
        )
        .await
        .map_err(StartupError::Session)?;

    Ok(AppRuntime {
        client,
        permission_requests,
        permission_handler,
        session,
        models,
        working_directory,
        active_model_options: ActiveModelOptions {
            model: config.model.clone(),
            reasoning_effort: config.reasoning_effort.clone(),
            context_tier: config.context_tier.clone(),
        },
    })
}
