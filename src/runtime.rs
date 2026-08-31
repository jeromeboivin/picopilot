use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use github_copilot_sdk::{
    handler::PermissionHandler,
    types::{Model, ResumeSessionConfig, SessionEvent, SessionId},
    Client, ClientOptions, Error as SdkError,
};

use crate::config::{AppConfig, ConfigError};
use crate::permissions::{permission_handler, ApprovalRequest};

pub const MAX_RECOVERY_ATTEMPTS: usize = 3;

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

#[cfg(test)]
mod tests {
    use github_copilot_sdk::types::{ResumeSessionConfig, SessionId};

    use super::{
        apply_active_model_options, recovery_backoff, verify_session_identity, ActiveModelOptions,
        SessionIdentity,
    };

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
}

impl fmt::Display for RecoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Client(error) => write!(formatter, "could not restart Copilot: {error}"),
            Self::Resume(error) => write!(formatter, "could not reconnect session: {error}"),
        }
    }
}

impl std::error::Error for RecoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Client(error) => Some(error),
            Self::Resume(error) => Some(error),
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

    pub async fn resume(&mut self, session_id: SessionId) -> Result<Vec<SessionEvent>, ResumeError> {
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
        self.session.get_events().await.map_err(ResumeError::Session)
    }

    pub async fn preview_session(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<SessionEvent>, ResumeError> {
        if self.session.id() == &session_id {
            return self.session.get_events().await.map_err(ResumeError::Session);
        }

        let expected_metadata = self
            .client
            .get_session_metadata(&session_id)
            .await
            .map_err(ResumeError::Session)?
            .ok_or_else(|| ResumeError::MissingSession {
                session_id: session_id.clone(),
            })?;
        let resumed = self
            .client
            .resume_session(self.base_resume_config(session_id.clone()))
            .await
            .map_err(ResumeError::Session)?;
        let events = async {
            let actual_metadata = self
                .client
                .get_session_metadata(resumed.id())
                .await
                .map_err(ResumeError::Session)?;
            verify_session_identity(
                &SessionIdentity {
                    session_id,
                    start_time: Some(expected_metadata.start_time),
                },
                &SessionIdentity {
                    session_id: resumed.id().clone(),
                    start_time: actual_metadata.map(|metadata| metadata.start_time),
                },
            )?;
            resumed.get_events().await.map_err(ResumeError::Session)
        }
        .await;
        let _ = resumed.disconnect().await;
        events
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
        ResumeSessionConfig::new(session_id)
            .with_client_name("picopilot")
            .with_streaming(true)
            .with_available_tools(crate::config::V1_AVAILABLE_TOOLS.iter().copied())
            .with_excluded_tools(crate::config::V1_EXCLUDED_TOOLS.iter().copied())
            .with_working_directory(self.working_directory.clone())
            .with_permission_handler(self.permission_handler.clone())
            .with_suppress_resume_event(true)
    }
}

#[derive(Debug)]
pub enum StartupError {
    CurrentDirectory(std::io::Error),
    Client(SdkError),
    Configuration(ConfigError),
    Session(SdkError),
    SessionMetadataMissing { session_id: SessionId },
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
            Self::SessionMetadataMissing { session_id } => write!(
                formatter,
                "could not determine the start time for Copilot session '{session_id}'"
            ),
        }
    }
}

impl std::error::Error for StartupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CurrentDirectory(error) => Some(error),
            Self::Client(error) | Self::Session(error) => Some(error),
            Self::Configuration(error) => Some(error),
            Self::SessionMetadataMissing { .. } => None,
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
    let session_start_time = client
        .get_session_metadata(session.id())
        .await
        .map_err(StartupError::Client)?
        .map(|metadata| metadata.start_time)
        .ok_or_else(|| StartupError::SessionMetadataMissing {
            session_id: session.id().clone(),
        })?;

    Ok(AppRuntime {
        client,
        permission_requests,
        permission_handler,
        session,
        models,
        working_directory,
        startup_config: config.clone(),
        session_start_time: Some(session_start_time),
        active_model_options: ActiveModelOptions {
            model: config.model.clone(),
            reasoning_effort: config.reasoning_effort.clone(),
            context_tier: config.context_tier.clone(),
        },
    })
}
