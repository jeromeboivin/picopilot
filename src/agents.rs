//! Extra user agent definitions outside the Copilot CLI's standard discovery roots.
use std::fs;
use std::path::Path;

use github_copilot_sdk::types::{CustomAgentConfig, ResumeSessionConfig, SessionConfig};
use serde::Deserialize;

#[derive(Default, Deserialize)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
    #[serde(rename = "display-name")]
    display_name: Option<String>,
    #[serde(rename = "user-invocable")]
    user_invocable: Option<bool>,
    tools: Option<Vec<String>>,
    model: Option<String>,
}

pub(crate) fn discover_extra(home: Option<&Path>) -> Vec<CustomAgentConfig> {
    let Some(home) = home else { return Vec::new() };
    let Ok(entries) = fs::read_dir(home.join(".agents").join("agents")) else {
        return Vec::new();
    };
    let mut paths = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let text = fs::read_to_string(&path).ok()?;
            let body = text
                .strip_prefix("---\n")
                .or_else(|| text.strip_prefix("---\r\n"))?;
            let (header, prompt) = body.split_once("\n---")?;
            let prompt = prompt
                .strip_prefix("\r\n")
                .or_else(|| prompt.strip_prefix('\n'))?;
            let metadata: Frontmatter = serde_yaml::from_str(header).ok()?;
            if metadata.user_invocable == Some(false) {
                return None;
            }
            let name = metadata.name.or_else(|| {
                let file_name = path.file_name()?.to_string_lossy().into_owned();
                let stem = file_name
                    .strip_suffix(".agent.md")
                    .or_else(|| file_name.strip_suffix(".md"))
                    .unwrap_or(&file_name);
                Some(stem.to_string())
            })?;
            let mut agent = CustomAgentConfig::new(name, prompt.trim().to_string());
            agent.description = metadata.description;
            agent.display_name = metadata.display_name;
            agent.tools = metadata.tools;
            agent.model = metadata.model;
            Some(agent)
        })
        .collect()
}

pub(crate) fn configure_session(config: &mut SessionConfig) {
    config.enable_config_discovery = Some(true);
    let agents = discover_extra(crate::skills::home_directory().as_deref());
    if !agents.is_empty() {
        config.custom_agents = Some(agents);
    }
}

pub(crate) fn configure_resume(config: &mut ResumeSessionConfig) {
    config.enable_config_discovery = Some(true);
    let agents = discover_extra(crate::skills::home_directory().as_deref());
    if !agents.is_empty() {
        config.custom_agents = Some(agents);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_extra_user_agents() {
        let home =
            std::env::temp_dir().join(format!("picopilot-agent-test-{}", std::process::id()));
        let directory = home.join(".agents/agents");
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("helper.md"),
            "---\ndescription: A helper\n---\nHelp the user.\n",
        )
        .unwrap();
        fs::write(
            directory.join("qa.agent.md"),
            "---\ndescription: QA\n---\nTest things.\n",
        )
        .unwrap();
        fs::write(
            directory.join("hidden.md"),
            "---\nuser-invocable: false\n---\nHidden\n",
        )
        .unwrap();
        let agents = discover_extra(Some(&home));
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].name, "helper");
        assert_eq!(agents[0].prompt, "Help the user.");
        assert_eq!(agents[1].name, "qa");
        fs::remove_dir_all(home).unwrap();
    }
}
