use clap::Parser;

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

#[cfg(test)]
mod tests {
    use clap::Parser;

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
}
