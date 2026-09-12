use std::process::ExitCode;

use clap::Parser;

use picopilot::config::AppConfig;
use picopilot::provider_config;
use picopilot::runtime::connect;
use picopilot::startup::{resolve_startup, StartupOutcome};
use picopilot::tui;
use picopilot::wizard::run_setup_wizard;

#[tokio::main]
async fn main() -> ExitCode {
    let config = AppConfig::parse();

    match run(config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("picopilot: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(config: AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let Some(config_path) = provider_config::config_file_path() else {
        return Err("could not determine a home directory for the provider config file".into());
    };

    let outcome = resolve_startup(&config_path, config.configure, run_setup_wizard)?;
    let provider_config = match outcome {
        StartupOutcome::ExitAfterConfigure => return Ok(()),
        StartupOutcome::Continue(provider_config) => provider_config,
    };

    let reduced_motion = config.reduced_motion;
    let runtime = connect(&config, &provider_config.default_provider).await?;
    tui::run_with_settings(runtime, None, reduced_motion).await?;
    Ok(())
}
