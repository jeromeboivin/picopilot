//! Startup orchestration for the provider configuration file (spec §2.4).
//!
//! Resolves what to do with `config.yaml` on process startup: run the setup wizard (or not),
//! write a `.bak` safety copy first when the existing file turns out to be unusable, and decide
//! whether picopilot should continue straight into a session afterward or exit.

use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};

use crate::provider_config::{self, ProviderConfigError, ProviderConfigFile};

/// Suffix appended to the config file's name for its pre-wizard safety copy.
pub const BACKUP_SUFFIX: &str = ".bak";

/// What startup should do once the provider configuration is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupOutcome {
    /// Continue straight into a session using this configuration — either it was already valid,
    /// or the wizard just produced it (spec §2.4 conditions 1 and 2).
    Continue(ProviderConfigFile),
    /// `--configure` was passed explicitly: the wizard ran and picopilot must exit now instead of
    /// starting a session (spec §2.4 condition 3).
    ExitAfterConfigure,
}

/// Resolves startup behavior for the config file at `path`, per spec §2.4.
///
/// `run_wizard` is injected so tests can supply a deterministic stub instead of driving a real
/// interactive TUI; production code passes a closure wrapping
/// [`crate::wizard::run_setup_wizard`]. It returns a future rather than a value directly because
/// the real wizard's Copilot-connect screen (spec §3.3) performs real async client startup.
pub async fn resolve_startup<F>(
    path: &Path,
    configure_requested: bool,
    run_wizard: impl FnOnce(Option<ProviderConfigFile>) -> F,
) -> Result<StartupOutcome, ProviderConfigError>
where
    F: Future<Output = ProviderConfigFile>,
{
    let load_result = provider_config::load(path);

    if configure_requested {
        // Case 3: run the wizard over whatever exists (or doesn't), but always exit afterward.
        // No `.bak` here — the user asked for this, so there is nothing surprising to protect
        // against (unlike cases below, where an auto-launched wizard could otherwise clobber a
        // file the user did not expect to be judged invalid).
        let existing = load_result.unwrap_or(None);
        let produced = run_wizard(existing).await;
        provider_config::save(path, &produced)?;
        return Ok(StartupOutcome::ExitAfterConfigure);
    }

    match load_result {
        // Case 1: genuine first run. No warning, no backup (nothing to back up), run the wizard,
        // continue into a session with what it produced.
        Ok(None) => {
            let produced = run_wizard(None).await;
            provider_config::save(path, &produced)?;
            Ok(StartupOutcome::Continue(produced))
        }
        // Case 2 (parses, but `default_provider` is unresolved): warn, back up, run the wizard
        // over the parsed-but-invalid config, continue.
        Ok(Some(config)) => match provider_config::validate(&config) {
            Ok(()) => Ok(StartupOutcome::Continue(config)),
            Err(error) => {
                let produced =
                    warn_backup_and_rerun_wizard(path, &error, Some(config), run_wizard).await?;
                Ok(StartupOutcome::Continue(produced))
            }
        },
        // Case 2 (fails to parse as YAML at all): warn, back up, run the wizard with nothing to
        // seed it from, continue.
        Err(error) => {
            let produced = warn_backup_and_rerun_wizard(path, &error, None, run_wizard).await?;
            Ok(StartupOutcome::Continue(produced))
        }
    }
}

/// Shared recovery path for both "unusable existing config" cases (spec §2.4 case 2): warn about
/// `error`, write a `.bak` safety copy of the file at `path` *before* the wizard can touch it,
/// run the wizard seeded with `seed`, save what it produces, and return that.
async fn warn_backup_and_rerun_wizard<F>(
    path: &Path,
    error: &ProviderConfigError,
    seed: Option<ProviderConfigFile>,
    run_wizard: impl FnOnce(Option<ProviderConfigFile>) -> F,
) -> Result<ProviderConfigFile, ProviderConfigError>
where
    F: Future<Output = ProviderConfigFile>,
{
    warn(error);
    backup_existing_file(path)?;
    let produced = run_wizard(seed).await;
    provider_config::save(path, &produced)?;
    Ok(produced)
}

fn warn(error: &ProviderConfigError) {
    eprintln!("picopilot: warning: {error}");
}

/// Copies the existing file at `path` to its `.bak` sibling, before the wizard can write
/// anything. A missing file is not an error here (nothing to back up).
fn backup_existing_file(path: &Path) -> Result<(), ProviderConfigError> {
    match fs::read(path) {
        Ok(bytes) => fs::write(backup_path_for(path), bytes).map_err(ProviderConfigError::Io),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ProviderConfigError::Io(error)),
    }
}

fn backup_path_for(path: &Path) -> PathBuf {
    let mut file_name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    file_name.push(BACKUP_SUFFIX);
    path.with_file_name(file_name)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{backup_path_for, resolve_startup, warn_backup_and_rerun_wizard, StartupOutcome};
    use crate::provider_config::{load, save, ProviderConfigError, ProviderConfigFile, ProviderProfile};

    fn temp_directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "picopilot-startup-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp directory should be created");
        path
    }

    #[tokio::test]
    async fn no_config_file_runs_the_wizard_and_continues_with_no_backup() {
        let directory = temp_directory("no-file");
        let path = directory.join("config.yaml");
        let wizard_called_with = std::rc::Rc::new(std::cell::RefCell::new(None));
        let wizard_called_with_handle = wizard_called_with.clone();

        let outcome = resolve_startup(&path, false, |existing| {
            *wizard_called_with_handle.borrow_mut() = Some(existing.clone());
            async move { existing.unwrap_or_else(|| ProviderConfigFile::new("copilot")) }
        })
        .await
        .expect("startup should resolve for a missing file");

        assert_eq!(*wizard_called_with.borrow(), Some(None));
        assert_eq!(
            outcome,
            StartupOutcome::Continue(ProviderConfigFile::new("copilot"))
        );
        assert!(path.exists(), "the wizard's output should be saved");
        assert!(
            !backup_path_for(&path).exists(),
            "a genuine first run must not create a .bak file"
        );
    }

    #[tokio::test]
    async fn unparseable_config_warns_backs_up_and_continues_after_the_wizard() {
        let directory = temp_directory("bad-yaml");
        let path = directory.join("config.yaml");
        fs::write(&path, "default_provider: [not valid yaml").unwrap();
        let original_bytes = fs::read(&path).unwrap();

        let outcome = resolve_startup(&path, false, |existing| {
            assert_eq!(existing, None, "an unparseable file has nothing to seed the wizard with");
            async move { ProviderConfigFile::new("copilot") }
        })
        .await
        .expect("startup should resolve after backing up and running the wizard");

        assert_eq!(
            outcome,
            StartupOutcome::Continue(ProviderConfigFile::new("copilot"))
        );
        let backup_bytes = fs::read(backup_path_for(&path)).unwrap();
        assert_eq!(backup_bytes, original_bytes);
        let saved = load(&path).unwrap().unwrap();
        assert_eq!(saved, ProviderConfigFile::new("copilot"));
    }

    #[tokio::test]
    async fn unresolved_default_provider_warns_backs_up_and_continues_after_the_wizard() {
        let directory = temp_directory("bad-default-provider");
        let path = directory.join("config.yaml");
        let invalid = ProviderConfigFile::new("does-not-exist");
        save(&path, &invalid).unwrap();
        let original_bytes = fs::read(&path).unwrap();

        let outcome = resolve_startup(&path, false, |existing| {
            assert_eq!(existing, Some(invalid.clone()));
            async move { ProviderConfigFile::new("copilot") }
        })
        .await
        .expect("startup should resolve after backing up and running the wizard");

        assert_eq!(
            outcome,
            StartupOutcome::Continue(ProviderConfigFile::new("copilot"))
        );
        let backup_bytes = fs::read(backup_path_for(&path)).unwrap();
        assert_eq!(backup_bytes, original_bytes);
    }

    #[tokio::test]
    async fn valid_config_runs_no_wizard_and_continues_directly() {
        let directory = temp_directory("valid");
        let path = directory.join("config.yaml");
        let mut valid = ProviderConfigFile::new("ollama");
        valid.providers.insert(
            "ollama".to_string(),
            ProviderProfile::new("http://localhost:11434/v1"),
        );
        save(&path, &valid).unwrap();

        let outcome = resolve_startup(&path, false, |_existing| async move {
            panic!("the wizard must not run over an already-valid config")
        })
        .await
        .expect("a valid config should resolve without running the wizard");

        assert_eq!(outcome, StartupOutcome::Continue(valid));
        assert!(!backup_path_for(&path).exists());
    }

    #[tokio::test]
    async fn configure_flag_runs_the_wizard_over_a_valid_config_and_exits() {
        let directory = temp_directory("configure-valid");
        let path = directory.join("config.yaml");
        let mut valid = ProviderConfigFile::new("ollama");
        valid.providers.insert(
            "ollama".to_string(),
            ProviderProfile::new("http://localhost:11434/v1"),
        );
        save(&path, &valid).unwrap();

        let outcome = resolve_startup(&path, true, |existing| {
            assert_eq!(existing, Some(valid.clone()));
            async move { ProviderConfigFile::new("copilot") }
        })
        .await
        .expect("--configure should resolve by running the wizard and exiting");

        assert_eq!(outcome, StartupOutcome::ExitAfterConfigure);
        assert!(
            !backup_path_for(&path).exists(),
            "--configure is user-requested and must not create a .bak file"
        );
        let saved = load(&path).unwrap().unwrap();
        assert_eq!(saved, ProviderConfigFile::new("copilot"));
    }

    #[tokio::test]
    async fn configure_flag_runs_the_wizard_with_no_existing_config() {
        let directory = temp_directory("configure-missing");
        let path = directory.join("config.yaml");

        let outcome = resolve_startup(&path, true, |existing| {
            assert_eq!(existing, None);
            async move { ProviderConfigFile::new("copilot") }
        })
        .await
        .expect("--configure should resolve even with no existing config");

        assert_eq!(outcome, StartupOutcome::ExitAfterConfigure);
        assert!(!backup_path_for(&path).exists());
    }

    #[tokio::test]
    async fn warn_backup_and_rerun_wizard_backs_up_before_the_wizard_runs_and_seeds_it() {
        let directory = temp_directory("helper-direct");
        let path = directory.join("config.yaml");
        let existing = ProviderConfigFile::new("does-not-exist");
        save(&path, &existing).unwrap();
        let original_bytes = fs::read(&path).unwrap();
        let error = ProviderConfigError::UnresolvedDefaultProvider {
            default_provider: "does-not-exist".to_string(),
        };

        let produced = warn_backup_and_rerun_wizard(&path, &error, Some(existing.clone()), |seed| {
            // The backup must already exist by the time the wizard is invoked, and the wizard
            // must be seeded with exactly what the caller passed in.
            assert!(
                backup_path_for(&path).exists(),
                ".bak must be written before the wizard runs"
            );
            assert_eq!(seed, Some(existing.clone()));
            async move { ProviderConfigFile::new("copilot") }
        })
        .await
        .expect("the helper should back up, run the wizard, and save its output");

        assert_eq!(produced, ProviderConfigFile::new("copilot"));
        let backup_bytes = fs::read(backup_path_for(&path)).unwrap();
        assert_eq!(backup_bytes, original_bytes, "the backup must hold the pre-wizard content");
        let saved = load(&path).unwrap().unwrap();
        assert_eq!(saved, ProviderConfigFile::new("copilot"), "the wizard's output must be saved");
    }

    #[tokio::test]
    async fn warn_backup_and_rerun_wizard_with_no_seed_passes_none_to_the_wizard() {
        let directory = temp_directory("helper-direct-no-seed");
        let path = directory.join("config.yaml");
        fs::write(&path, "default_provider: [not valid yaml").unwrap();
        let error = ProviderConfigError::UnresolvedDefaultProvider {
            default_provider: "unused".to_string(),
        };

        let produced = warn_backup_and_rerun_wizard(&path, &error, None, |seed| {
            assert_eq!(seed, None, "an unparseable file has nothing to seed the wizard with");
            async move { ProviderConfigFile::new("copilot") }
        })
        .await
        .expect("the helper should back up, run the wizard, and save its output");

        assert_eq!(produced, ProviderConfigFile::new("copilot"));
        assert!(backup_path_for(&path).exists());
    }
}
