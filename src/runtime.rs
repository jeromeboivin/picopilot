use std::fmt;
use std::path::PathBuf;

use github_copilot_sdk::{Client, Error as SdkError, Model};

use crate::config::{AppConfig, ConfigError};

pub struct AppRuntime {
    pub client: Client,
    pub session: github_copilot_sdk::session::Session,
    pub models: Vec<Model>,
    pub working_directory: PathBuf,
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
    let client = Client::start(config.client_options_in(&working_directory))
        .await
        .map_err(StartupError::Client)?;
    let models = client.list_models().await.map_err(StartupError::Client)?;

    config
        .validate_against(&models)
        .map_err(StartupError::Configuration)?;

    let session = client
        .create_session(config.session_config_in(&working_directory))
        .await
        .map_err(StartupError::Session)?;

    Ok(AppRuntime {
        client,
        session,
        models,
        working_directory,
    })
}
