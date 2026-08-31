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
    let runtime = connect(&config).await?;
    tui::run(runtime, model).await?;
    drop(runtime);
    Ok(())
}
