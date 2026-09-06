use std::process::ExitCode;

use clap::Parser;

use picopilot::config::AppConfig;
use picopilot::runtime::connect;
use picopilot::tui;

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
    let model = config.model.clone();
    let reduced_motion = config.reduced_motion;
    let runtime = connect(&config).await?;
    tui::run_with_settings(runtime, model, reduced_motion).await?;
    Ok(())
}
