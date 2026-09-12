//! Interactive first-run / `--configure` setup wizard (spec §3).
//!
//! TODO(ticket 4): replace this stub with the real interactive TUI wizard described in
//! `docs/provider-config-spec.md` §3. This stub exists so that ticket 2's startup control flow —
//! exit-vs-continue behavior and the `config.yaml.bak` safety copy — is fully wired and testable
//! now, without building the real wizard screens.

use crate::provider_config::ProviderConfigFile;

/// Runs the setup wizard over `existing` configuration and returns the `ProviderConfigFile` to
/// persist. `existing` is `None` on a genuine first run, or when the on-disk file could not be
/// parsed at all; it is `Some` when a parsed-but-invalid file is available to seed the wizard.
///
/// TODO(ticket 4): build the real interactive screens. For now this is a no-op stub that hands
/// back `existing` unchanged, falling back to a bare `"copilot"` default when nothing exists yet.
pub fn run_setup_wizard(existing: Option<ProviderConfigFile>) -> ProviderConfigFile {
    existing.unwrap_or_else(|| ProviderConfigFile::new("copilot"))
}

#[cfg(test)]
mod tests {
    use super::run_setup_wizard;
    use crate::provider_config::{ProviderConfigFile, ProviderProfile};

    #[test]
    fn stub_wizard_preserves_existing_configuration() {
        let mut existing = ProviderConfigFile::new("ollama");
        existing.providers.insert(
            "ollama".to_string(),
            ProviderProfile::new("http://localhost:11434/v1"),
        );

        let result = run_setup_wizard(Some(existing.clone()));

        assert_eq!(result, existing);
    }

    #[test]
    fn stub_wizard_defaults_to_copilot_when_nothing_exists() {
        let result = run_setup_wizard(None);

        assert_eq!(result.default_provider, "copilot");
        assert!(result.providers.is_empty());
    }
}
