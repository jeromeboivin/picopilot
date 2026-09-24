use std::path::{Path, PathBuf};

use clap::Parser;
use github_copilot_sdk::types::{Model, SessionConfig, SystemMessageConfig};
use github_copilot_sdk::ClientOptions;

use crate::provider::ProviderRegistry;
use crate::toolset::{Toolset, CANONICAL_TOOLS, EXCLUDED_TOOLS};

pub const V1_AVAILABLE_TOOLS: &[&str] = CANONICAL_TOOLS;
pub const V1_EXCLUDED_TOOLS: &[&str] = EXCLUDED_TOOLS;

/// The SDK's bundled `copilot-runtime` is a standalone server that cannot
/// resolve custom agent prompts, so `task` calls to custom agents fail on it.
/// Prefer an installed Copilot CLI, which hosts those session effects.
/// Returns `None` when `COPILOT_CLI_PATH` already selects a CLI (the SDK
/// honors it) or when no `copilot` is found on `PATH`.
pub(crate) fn installed_copilot_cli() -> Option<PathBuf> {
    if std::env::var_os("COPILOT_CLI_PATH").is_some_and(|path| Path::new(&path).is_file()) {
        return None;
    }
    find_copilot_on_path(std::env::var_os("PATH")?.as_os_str())
}

pub(crate) fn uses_bundled_runtime() -> bool {
    installed_copilot_cli().is_none()
        && !std::env::var_os("COPILOT_CLI_PATH").is_some_and(|path| Path::new(&path).is_file())
}

fn find_copilot_on_path(path: &std::ffi::OsStr) -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["copilot.exe", "copilot.cmd"]
    } else {
        &["copilot"]
    };
    names.iter().find_map(|name| {
        std::env::split_paths(path)
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    })
}

pub(crate) fn system_message_config() -> SystemMessageConfig {
    SystemMessageConfig::new()
        .with_mode("replace")
        .with_content("")
}

#[derive(Debug, Clone, Parser, PartialEq, Eq)]
#[command(
    name = "picopilot",
    version,
    about = "A minimalist Copilot coding agent"
)]
pub struct AppConfig {
    #[arg(value_name = "PROJECT", conflicts_with = "project")]
    pub project_path: Option<PathBuf>,

    #[arg(long, value_name = "PROJECT", conflicts_with = "project_path")]
    pub project: Option<PathBuf>,

    #[arg(long, env = "PICOPILOT_REDUCED_MOTION")]
    pub reduced_motion: bool,

    /// Open the setup wizard on top of whatever provider configuration already exists, then
    /// exit without starting a session (spec §2.1/§2.4).
    #[arg(long)]
    pub configure: bool,
}

impl AppConfig {
    pub fn working_directory(&self) -> Result<PathBuf, std::io::Error> {
        let current_directory = std::env::current_dir()?;
        Ok(
            match self.project.as_deref().or(self.project_path.as_deref()) {
                Some(project) if project.is_absolute() => project.to_path_buf(),
                Some(project) => current_directory.join(project),
                None => current_directory,
            },
        )
    }

    pub fn client_options_in(&self, working_directory: &Path) -> ClientOptions {
        let options = ClientOptions::new().with_cwd(working_directory);
        match installed_copilot_cli() {
            Some(program) => options.with_program(program),
            None => options,
        }
    }

    pub fn session_config(&self) -> SessionConfig {
        self.session_config_with_registry(None)
    }

    pub fn session_config_with_registry(
        &self,
        registry: Option<&ProviderRegistry>,
    ) -> SessionConfig {
        self.session_config_with_registry_and_toolset(registry, Toolset::all())
    }

    pub fn session_config_with_registry_and_toolset(
        &self,
        registry: Option<&ProviderRegistry>,
        toolset: Toolset,
    ) -> SessionConfig {
        let mut session = SessionConfig::default()
            .with_client_name("picopilot")
            .with_streaming(true)
            .with_available_tools(toolset.available_tools())
            .with_excluded_tools(EXCLUDED_TOOLS.iter().copied())
            .with_system_message(system_message_config());
        crate::agents::configure_session(&mut session);
        if let Some(registry) = registry {
            session = session
                .with_providers(registry.providers().to_vec())
                .with_models(registry.models().to_vec());
        }
        session
    }

    pub fn session_config_in(&self, working_directory: impl Into<PathBuf>) -> SessionConfig {
        self.session_config_in_with_registry(working_directory, None)
    }

    pub fn session_config_in_with_registry(
        &self,
        working_directory: impl Into<PathBuf>,
        registry: Option<&ProviderRegistry>,
    ) -> SessionConfig {
        self.session_config_in_with_registry_and_toolset(
            working_directory,
            registry,
            Toolset::all(),
        )
    }

    pub fn session_config_in_with_registry_and_toolset(
        &self,
        working_directory: impl Into<PathBuf>,
        registry: Option<&ProviderRegistry>,
        toolset: Toolset,
    ) -> SessionConfig {
        let mut session = self.session_config_with_registry_and_toolset(registry, toolset);
        session.working_directory = Some(working_directory.into());
        session
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

    use crate::provider::ProviderRegistry;
    use crate::toolset::Toolset;

    use super::{system_message_config, AppConfig};

    #[test]
    fn parses_the_configure_flag() {
        let config = AppConfig::try_parse_from(["picopilot", "--configure"])
            .expect("--configure should parse");

        assert!(config.configure);
    }

    #[test]
    fn configure_defaults_to_false() {
        let config =
            AppConfig::try_parse_from(["picopilot"]).expect("default options should parse");

        assert!(!config.configure);
    }

    #[test]
    fn rejects_the_removed_model_flag() {
        let error = AppConfig::try_parse_from(["picopilot", "--model", "gpt-5"])
            .expect_err("--model was removed in favor of the config file");

        assert!(error.to_string().contains("unexpected argument"));
    }

    #[test]
    fn rejects_the_removed_reasoning_effort_flag() {
        let error = AppConfig::try_parse_from(["picopilot", "--reasoning-effort", "high"])
            .expect_err("--reasoning-effort was removed in favor of the config file");

        assert!(error.to_string().contains("unexpected argument"));
    }

    #[test]
    fn rejects_the_removed_context_tier_flag() {
        let error = AppConfig::try_parse_from(["picopilot", "--context-tier", "long_context"])
            .expect_err("--context-tier was removed in favor of the config file");

        assert!(error.to_string().contains("unexpected argument"));
    }

    #[test]
    fn rejects_the_removed_provider_flags() {
        for flag in ["--provider-url", "--provider-name", "--provider-wire-api"] {
            let error = AppConfig::try_parse_from(["picopilot", flag, "value"])
                .expect_err("provider CLI flags were removed in favor of the config file");

            assert!(error.to_string().contains("unexpected argument"));
        }
    }

    #[test]
    fn parses_a_positional_project_path() {
        let config = AppConfig::try_parse_from(["picopilot", "projects/demo"])
            .expect("a positional project path should parse");

        assert_eq!(
            config.project_path.as_deref(),
            Some(Path::new("projects/demo"))
        );
        assert!(config.project.is_none());
    }

    #[test]
    fn parses_and_resolves_the_named_project_path() {
        let config = AppConfig::try_parse_from(["picopilot", "--project", "projects/demo"])
            .expect("the named project path should parse");
        let expected = std::env::current_dir()
            .expect("the test should have a current directory")
            .join("projects/demo");

        assert_eq!(config.working_directory().unwrap(), expected);
        assert!(config.project_path.is_none());
    }

    #[test]
    fn finds_an_installed_copilot_cli_on_path() {
        let directory =
            std::env::temp_dir().join(format!("picopilot-cli-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let name = if cfg!(windows) { "copilot.exe" } else { "copilot" };
        let empty = std::env::join_paths([directory.join("missing")]).unwrap();
        assert_eq!(super::find_copilot_on_path(&empty), None);
        std::fs::write(directory.join(name), "").unwrap();
        let path = std::env::join_paths([directory.join("missing"), directory.clone()]).unwrap();
        assert_eq!(
            super::find_copilot_on_path(&path),
            Some(directory.join(name))
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_both_project_path_forms() {
        let error = AppConfig::try_parse_from([
            "picopilot",
            "projects/demo",
            "--project",
            "projects/other",
        ])
        .expect_err("the positional and named project paths should conflict");

        assert!(error.to_string().contains("cannot be used with"));
    }

    #[test]
    fn builds_a_streaming_session_with_the_v1_tool_policy() {
        let config =
            AppConfig::try_parse_from(["picopilot"]).expect("default options should parse");

        let session = config.session_config();

        assert_eq!(session.streaming, Some(true));
        assert_eq!(session.enable_config_discovery, Some(true));
        let shell_tool = if cfg!(windows) { "powershell" } else { "bash" };
        let list_tool = if cfg!(windows) { "list_powershell" } else { "list_bash" };
        let read_tool = if cfg!(windows) { "read_powershell" } else { "read_bash" };
        let stop_tool = if cfg!(windows) { "stop_powershell" } else { "stop_bash" };
        let write_tool = if cfg!(windows) { "write_powershell" } else { "write_bash" };
        assert_eq!(
            session.available_tools,
            Some(
                [shell_tool, list_tool, read_tool, stop_tool, write_tool, "view", "edit", "create", "apply_patch", "grep", "glob", "task", "list_agents", "read_agent", "write_agent", "ask_user", "skill"]
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
            Some("replace")
        );
        assert_eq!(
            session
                .system_message
                .as_ref()
                .and_then(|config| config.content.as_deref()),
            Some("")
        );
        assert!(session
            .system_message
            .as_ref()
            .and_then(|config| config.sections.as_ref())
            .is_none());
        assert!(session.system_message_transform.is_none());
    }

    #[test]
    fn serializes_shell_only_and_empty_toolsets_as_explicit_allowlists() {
        let config =
            AppConfig::try_parse_from(["picopilot"]).expect("default options should parse");

        let shell_only =
            config.session_config_with_registry_and_toolset(None, Toolset::shell_only());
        assert_eq!(
            shell_only.available_tools,
            Some(vec![if cfg!(windows) {
                "powershell".to_string()
            } else {
                "bash".to_string()
            }])
        );

        let empty = config.session_config_with_registry_and_toolset(None, Toolset::empty());
        assert_eq!(empty.available_tools, Some(Vec::new()));
    }

    #[test]
    fn replaces_the_system_message_with_empty_content() {
        let config = system_message_config();
        assert_eq!(config.mode.as_deref(), Some("replace"));
        assert_eq!(config.content.as_deref(), Some(""));
        assert!(config.sections.is_none());
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

    #[test]
    fn applies_a_registry_to_session_creation_without_changing_hosted_defaults() {
        let config =
            AppConfig::try_parse_from(["picopilot"]).expect("default options should parse");
        let settings = crate::provider::ProviderSettings::default_for("http://localhost:11434/v1")
            .unwrap();
        let registry = ProviderRegistry::from_model_ids(&settings, ["qwen:7b"]).unwrap();

        let session = config.session_config_with_registry(Some(&registry));

        assert_eq!(session.providers.as_ref().map(Vec::len), Some(1));
        assert_eq!(session.models.as_ref().map(Vec::len), Some(1));
        assert_eq!(session.models.as_ref().unwrap()[0].id, "qwen:7b");
        assert!(session.provider.is_none());
    }
}
