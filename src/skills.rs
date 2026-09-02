use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use github_copilot_sdk::types::{ResumeSessionConfig, SessionConfig};
use serde::Deserialize;
use serde_json::Value;

const SKILL_FILE_NAME: &str = "SKILL.md";
const AGENT_SKILLS_SETTINGS_KEY: &str = "chat.agentSkillsLocations";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillRootSource {
    Project,
    User,
    VisualStudioCode,
}

impl fmt::Display for SkillRootSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Project => formatter.write_str("project"),
            Self::User => formatter.write_str("user"),
            Self::VisualStudioCode => formatter.write_str("VS Code"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillRoot {
    pub path: PathBuf,
    pub source: SkillRootSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub user_invocable: bool,
    pub directory: PathBuf,
    pub root: SkillRoot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDiagnostic {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillCatalog {
    roots: Vec<SkillRoot>,
    skills: Vec<Skill>,
    diagnostics: Vec<SkillDiagnostic>,
}

impl SkillCatalog {
    #[cfg(test)]
    pub(crate) fn from_parts(
        roots: Vec<SkillRoot>,
        skills: Vec<Skill>,
        diagnostics: Vec<SkillDiagnostic>,
    ) -> Self {
        Self {
            roots,
            skills,
            diagnostics,
        }
    }

    pub fn discover(working_directory: &Path) -> Self {
        let home = home_directory();
        let user_settings = home.as_deref().and_then(vscode_user_settings_path);
        let workspace_settings = working_directory.join(".vscode").join("settings.json");
        Self::discover_from_settings(
            working_directory,
            home.as_deref(),
            user_settings.as_deref(),
            Some(&workspace_settings),
        )
    }

    fn discover_from_settings(
        working_directory: &Path,
        home: Option<&Path>,
        user_settings: Option<&Path>,
        workspace_settings: Option<&Path>,
    ) -> Self {
        let mut catalog = Self::default();
        for (path, source) in standard_roots(working_directory, home) {
            catalog.add_root(path, source);
        }

        let custom_roots = merge_vscode_skill_locations(
            user_settings,
            workspace_settings,
            working_directory,
            home,
            &mut catalog.diagnostics,
        );
        for path in custom_roots {
            catalog.add_root(path, SkillRootSource::VisualStudioCode);
        }

        catalog.scan_roots();
        catalog
    }

    pub fn roots(&self) -> &[SkillRoot] {
        &self.roots
    }

    pub fn skill_directories(&self) -> Vec<PathBuf> {
        self.roots.iter().map(|root| root.path.clone()).collect()
    }

    pub fn skills(&self) -> &[Skill] {
        &self.skills
    }

    pub fn diagnostics(&self) -> &[SkillDiagnostic] {
        &self.diagnostics
    }

    pub fn find(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|skill| skill.name == name)
    }

    pub fn user_invocable(&self) -> impl Iterator<Item = &Skill> {
        self.skills.iter().filter(|skill| skill.user_invocable)
    }

    pub fn skill_names(&self) -> Vec<String> {
        self.skills.iter().map(|skill| skill.name.clone()).collect()
    }

    fn add_root(&mut self, path: PathBuf, source: SkillRootSource) {
        if self
            .roots
            .iter()
            .any(|root| path_key(&root.path) == path_key(&path))
        {
            return;
        }
        self.roots.push(SkillRoot { path, source });
    }

    fn scan_roots(&mut self) {
        let roots = self.roots.clone();
        let mut seen_names = BTreeSet::new();
        for root in roots {
            scan_directory(
                &root.path,
                &root,
                &mut seen_names,
                &mut self.skills,
                &mut self.diagnostics,
            );
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillSelection {
    selected: BTreeSet<String>,
}

impl SkillSelection {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn from_names<I, S>(catalog: &SkillCatalog, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let available = catalog
            .skills()
            .iter()
            .map(|skill| skill.name.as_str())
            .collect::<BTreeSet<_>>();
        let selected = names
            .into_iter()
            .map(|name| name.as_ref().to_string())
            .filter(|name| available.contains(name.as_str()))
            .collect();
        Self { selected }
    }

    pub fn selected_names(&self) -> &BTreeSet<String> {
        &self.selected
    }

    pub fn contains(&self, name: &str) -> bool {
        self.selected.contains(name)
    }

    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    pub fn len(&self) -> usize {
        self.selected.len()
    }

    pub fn toggle(&mut self, catalog: &SkillCatalog, name: &str) {
        if catalog.find(name).is_none() {
            return;
        }
        if !self.selected.remove(name) {
            self.selected.insert(name.to_string());
        }
    }

    pub fn select_all(&mut self, catalog: &SkillCatalog) {
        self.selected = catalog.skill_names().into_iter().collect();
    }

    pub fn clear(&mut self) {
        self.selected.clear();
    }

    pub fn apply_session_config(&self, catalog: &SkillCatalog, config: &mut SessionConfig) {
        config.enable_skills = Some(!self.is_empty());
        config.skill_directories = Some(catalog.skill_directories());
        config.disabled_skills = Some(disabled_skill_names(catalog, self));
    }

    pub fn apply_resume_config(&self, catalog: &SkillCatalog, config: &mut ResumeSessionConfig) {
        config.enable_skills = Some(!self.is_empty());
        config.skill_directories = Some(catalog.skill_directories());
        config.disabled_skills = Some(disabled_skill_names(catalog, self));
    }
}

fn disabled_skill_names(catalog: &SkillCatalog, selection: &SkillSelection) -> Vec<String> {
    let mut names = catalog
        .skills()
        .iter()
        .filter(|skill| !selection.contains(&skill.name))
        .map(|skill| skill.name.clone())
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    #[serde(rename = "user-invocable")]
    user_invocable: Option<bool>,
}

fn scan_directory(
    directory: &Path,
    root: &SkillRoot,
    seen_names: &mut BTreeSet<String>,
    skills: &mut Vec<Skill>,
    diagnostics: &mut Vec<SkillDiagnostic>,
) {
    let entries = match read_directory(directory, diagnostics) {
        Some(entries) => entries,
        None => return,
    };

    for child in entries {
        if child.join(SKILL_FILE_NAME).is_file() {
            match parse_skill(&child, root) {
                Ok(skill) => {
                    if seen_names.insert(skill.name.clone()) {
                        skills.push(skill);
                    } else {
                        diagnostics.push(SkillDiagnostic {
                            path: child.join(SKILL_FILE_NAME),
                            message: "duplicate skill name ignored; an earlier root wins"
                                .to_string(),
                        });
                    }
                }
                Err(message) => diagnostics.push(SkillDiagnostic {
                    path: child.join(SKILL_FILE_NAME),
                    message,
                }),
            }
            continue;
        }

        if child.is_dir() {
            scan_directory(&child, root, seen_names, skills, diagnostics);
        }
    }
}

fn read_directory(
    directory: &Path,
    diagnostics: &mut Vec<SkillDiagnostic>,
) -> Option<Vec<PathBuf>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
        Err(error) => {
            diagnostics.push(SkillDiagnostic {
                path: directory.to_path_buf(),
                message: format!("could not read skill directory: {error}"),
            });
            return None;
        }
    };

    let mut paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => paths.push(entry.path()),
            Err(error) => diagnostics.push(SkillDiagnostic {
                path: directory.to_path_buf(),
                message: format!("could not read skill directory entry: {error}"),
            }),
        }
    }
    paths.sort_by_key(|path| path_key(path));
    Some(paths)
}

fn parse_skill(directory: &Path, root: &SkillRoot) -> Result<Skill, String> {
    let path = directory.join(SKILL_FILE_NAME);
    let content =
        fs::read_to_string(&path).map_err(|error| format!("could not read skill: {error}"))?;
    let frontmatter = extract_frontmatter(&content)
        .ok_or_else(|| "missing YAML frontmatter delimited by '---'".to_string())?;
    let metadata: SkillFrontmatter = serde_yaml::from_str(frontmatter)
        .map_err(|error| format!("invalid YAML frontmatter: {error}"))?;
    let name = metadata
        .name
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| "skill frontmatter is missing 'name'".to_string())?;
    let directory_name = directory
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "skill directory name is not valid UTF-8".to_string())?;
    if name != directory_name {
        return Err(format!(
            "skill name '{name}' must match directory '{directory_name}'"
        ));
    }
    if !valid_skill_name(&name) {
        return Err(
            "skill name must contain only lowercase letters, numbers, and hyphens".to_string(),
        );
    }
    let description = metadata
        .description
        .filter(|description| !description.trim().is_empty())
        .ok_or_else(|| "skill frontmatter is missing 'description'".to_string())?;

    Ok(Skill {
        name,
        description,
        user_invocable: metadata.user_invocable.unwrap_or(true),
        directory: directory.to_path_buf(),
        root: root.clone(),
    })
}

fn extract_frontmatter(content: &str) -> Option<&str> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let first_line_end = content.find('\n')?;
    if content[..first_line_end].trim_end_matches('\r').trim() != "---" {
        return None;
    }
    let body_start = first_line_end + 1;
    let body = &content[body_start..];
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let line_without_newline = line.strip_suffix('\n').unwrap_or(line);
        let line_without_newline = line_without_newline
            .strip_suffix('\r')
            .unwrap_or(line_without_newline);
        if line_without_newline.trim() == "---" {
            return Some(&body[..offset]);
        }
        offset += line.len();
    }
    None
}

fn valid_skill_name(name: &str) -> bool {
    name.len() <= 64
        && !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

fn standard_roots(
    working_directory: &Path,
    home: Option<&Path>,
) -> Vec<(PathBuf, SkillRootSource)> {
    let mut roots = vec![
        (
            working_directory.join(".agents").join("skills"),
            SkillRootSource::Project,
        ),
        (
            working_directory.join(".github").join("skills"),
            SkillRootSource::Project,
        ),
        (
            working_directory.join(".claude").join("skills"),
            SkillRootSource::Project,
        ),
    ];
    if let Some(home) = home {
        roots.extend([
            (home.join(".agents").join("skills"), SkillRootSource::User),
            (home.join(".copilot").join("skills"), SkillRootSource::User),
            (home.join(".claude").join("skills"), SkillRootSource::User),
        ]);
    }
    roots
}

fn merge_vscode_skill_locations(
    user_settings: Option<&Path>,
    workspace_settings: Option<&Path>,
    working_directory: &Path,
    home: Option<&Path>,
    diagnostics: &mut Vec<SkillDiagnostic>,
) -> Vec<PathBuf> {
    let mut locations: Vec<(String, bool)> = Vec::new();
    let mut location_indexes: BTreeMap<String, usize> = BTreeMap::new();
    for settings_path in [user_settings, workspace_settings].into_iter().flatten() {
        let Some(settings) = read_jsonc_settings(settings_path, diagnostics) else {
            continue;
        };
        let Some(entries) = settings
            .get(AGENT_SKILLS_SETTINGS_KEY)
            .and_then(Value::as_object)
        else {
            continue;
        };
        for (path, enabled) in entries {
            let Some(enabled) = enabled.as_bool() else {
                diagnostics.push(SkillDiagnostic {
                    path: settings_path.to_path_buf(),
                    message: format!("'{AGENT_SKILLS_SETTINGS_KEY}.{path}' must be a boolean"),
                });
                continue;
            };
            if let Some(index) = location_indexes.get(path).copied() {
                locations[index].1 = enabled;
            } else {
                location_indexes.insert(path.clone(), locations.len());
                locations.push((path.clone(), enabled));
            }
        }
    }

    locations
        .into_iter()
        .filter_map(|(path, enabled)| {
            enabled.then(|| resolve_location(&path, working_directory, home))
        })
        .collect()
}

fn read_jsonc_settings(path: &Path, diagnostics: &mut Vec<SkillDiagnostic>) -> Option<Value> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
        Err(error) => {
            diagnostics.push(SkillDiagnostic {
                path: path.to_path_buf(),
                message: format!("could not read VS Code settings: {error}"),
            });
            return None;
        }
    };

    match json5::from_str(&content) {
        Ok(settings) => Some(settings),
        Err(error) => {
            diagnostics.push(SkillDiagnostic {
                path: path.to_path_buf(),
                message: format!("could not parse VS Code JSONC settings: {error}"),
            });
            None
        }
    }
}

fn resolve_location(path: &str, working_directory: &Path, home: Option<&Path>) -> PathBuf {
    if let Some(home) = home {
        if path == "~" {
            return home.to_path_buf();
        }
        if let Some(relative) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
            return home.join(normalize_separators(relative));
        }
    }
    let normalized_path = normalize_separators(path);
    let path = Path::new(&normalized_path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        working_directory.join(path)
    }
}

fn normalize_separators(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.replace('\\', "/")
    }
}

fn path_key(path: &Path) -> String {
    let path = fs::canonicalize(path).unwrap_or_else(|_| normalize_path(path));
    let path = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        path.to_ascii_lowercase()
    } else {
        path
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if normalized.file_name().is_some() => {
                normalized.pop();
            }
            Component::ParentDir if !path.is_absolute() => normalized.push(component.as_os_str()),
            Component::ParentDir => {}
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn home_directory() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn vscode_user_settings_path(home: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| Some(home.join("AppData").join("Roaming")))
            .map(|app_data| app_data.join("Code").join("User").join("settings.json"))
    }

    #[cfg(target_os = "macos")]
    {
        Some(
            home.join("Library")
                .join("Application Support")
                .join("Code")
                .join("User")
                .join("settings.json"),
        )
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        Some(
            home.join(".config")
                .join("Code")
                .join("User")
                .join("settings.json"),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use github_copilot_sdk::types::{ResumeSessionConfig, SessionConfig, SessionId};

    use super::{
        extract_frontmatter, merge_vscode_skill_locations, parse_skill, standard_roots,
        SkillCatalog, SkillRoot, SkillRootSource, SkillSelection,
    };

    fn temp_directory(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("picopilot-skills-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temporary directory should be created");
        path
    }

    fn write_skill(root: &Path, name: &str, body: &str) -> PathBuf {
        let directory = root.join(name);
        fs::create_dir_all(&directory).expect("skill directory should be created");
        fs::write(directory.join("SKILL.md"), body).expect("skill should be written");
        directory
    }

    fn write_settings(path: &Path, content: &str) {
        fs::write(path, content).expect("settings should be written");
    }

    #[test]
    fn standard_roots_have_project_then_user_precedence() {
        let roots = standard_roots(Path::new("C:\\project"), Some(Path::new("C:\\home")));

        assert_eq!(roots.len(), 6);
        assert_eq!(roots[0].0, PathBuf::from("C:\\project\\.agents\\skills"));
        assert_eq!(roots[1].0, PathBuf::from("C:\\project\\.github\\skills"));
        assert_eq!(roots[2].0, PathBuf::from("C:\\project\\.claude\\skills"));
        assert!(roots[3..]
            .iter()
            .all(|(_, source)| *source == SkillRootSource::User));
    }

    #[test]
    fn parses_jsonc_and_merges_workspace_values_over_user_values() {
        let directory = temp_directory("settings");
        let user_settings = directory.join("user-settings.json");
        let workspace_settings = directory.join("workspace-settings.json");
        write_settings(
            &user_settings,
            r#"{
                // user setting
                "chat.agentSkillsLocations": {
                    "~/.copilot/skills": true,
                    "shared": true,
                    "disabled": true,
                },
            }"#,
        );
        write_settings(
            &workspace_settings,
            r#"{
                "chat.agentSkillsLocations": {
                    "shared": false,
                    "workspace": true,
                }
            }"#,
        );

        let mut diagnostics = Vec::new();
        let locations = merge_vscode_skill_locations(
            Some(&user_settings),
            Some(&workspace_settings),
            &directory,
            Some(Path::new("C:\\home")),
            &mut diagnostics,
        );

        assert!(diagnostics.is_empty());
        assert_eq!(
            locations,
            vec![
                PathBuf::from("C:\\home\\.copilot\\skills"),
                directory.join("disabled"),
                directory.join("workspace"),
            ]
        );
    }

    #[test]
    fn deduplicates_equivalent_relative_skill_roots() {
        let directory = temp_directory("root-deduplication");
        let workspace_settings = directory.join("workspace-settings.json");
        write_settings(
            &workspace_settings,
            r#"{
                "chat.agentSkillsLocations": {
                    "./.agents/skills": true,
                }
            }"#,
        );

        let catalog =
            SkillCatalog::discover_from_settings(&directory, None, None, Some(&workspace_settings));

        assert_eq!(catalog.roots().len(), 3);
    }

    #[test]
    fn parses_skill_frontmatter_and_defaults_user_invocation_to_true() {
        let directory = temp_directory("frontmatter");
        let skill_directory = write_skill(
            &directory,
            "rust-review",
            "---\nname: rust-review\ndescription: Review Rust code\n---\n\nInstructions\n",
        );
        let root = SkillRoot {
            path: directory.clone(),
            source: SkillRootSource::Project,
        };

        let skill = parse_skill(&skill_directory, &root).expect("skill should parse");

        assert_eq!(skill.name, "rust-review");
        assert_eq!(skill.description, "Review Rust code");
        assert!(skill.user_invocable);
    }

    #[test]
    fn rejects_invalid_skill_name_and_directory_mismatch() {
        let directory = temp_directory("invalid");
        let mismatch = write_skill(
            &directory,
            "directory-name",
            "---\nname: other-name\ndescription: Invalid\n---\n",
        );
        let root = SkillRoot {
            path: directory.clone(),
            source: SkillRootSource::Project,
        };
        assert!(parse_skill(&mismatch, &root)
            .expect_err("mismatched names should fail")
            .contains("must match directory"));

        let invalid = write_skill(
            &directory,
            "UpperCase",
            "---\nname: UpperCase\ndescription: Invalid\n---\n",
        );
        assert!(parse_skill(&invalid, &root)
            .expect_err("invalid names should fail")
            .contains("only lowercase"));

        for invalid_name in ["-leading", "trailing-", "two--hyphens"] {
            let invalid = write_skill(
                &directory,
                invalid_name,
                &format!("---\nname: {invalid_name}\ndescription: Invalid\n---\n"),
            );
            assert!(parse_skill(&invalid, &root).is_err());
        }
    }

    #[test]
    fn reports_malformed_skills_and_honors_non_invocable_metadata() {
        let directory = temp_directory("diagnostics");
        let root = directory.join(".agents").join("skills");
        write_skill(&root, "malformed", "not frontmatter");
        write_skill(
            &root,
            "internal-only",
            "---\nname: internal-only\ndescription: Internal helper\nuser-invocable: false\n---\n",
        );

        let catalog = SkillCatalog::discover_from_settings(&directory, None, None, None);

        assert_eq!(catalog.skills().len(), 1);
        assert!(!catalog.skills()[0].user_invocable);
        assert!(catalog
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.path.ends_with("malformed/SKILL.md")));
    }

    #[test]
    fn catalog_keeps_the_first_duplicate_and_reports_the_second() {
        let directory = temp_directory("duplicates");
        let project_root = directory.join(".github").join("skills");
        let user_home = directory.join("home");
        let user_root = user_home.join(".copilot").join("skills");
        write_skill(
            &project_root,
            "same-skill",
            "---\nname: same-skill\ndescription: Project version\n---\n",
        );
        write_skill(
            &user_root,
            "same-skill",
            "---\nname: same-skill\ndescription: User version\n---\n",
        );

        let catalog =
            SkillCatalog::discover_from_settings(&directory, Some(&user_home), None, None);

        assert_eq!(catalog.skills().len(), 1);
        assert_eq!(catalog.skills()[0].description, "Project version");
        assert!(catalog
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message.contains("duplicate skill name")));
    }

    #[test]
    fn selection_projects_exact_create_and_resume_sdk_fields() {
        let directory = temp_directory("selection");
        let root_path = directory.join("skills");
        write_skill(
            &root_path,
            "selected",
            "---\nname: selected\ndescription: Selected\n---\n",
        );
        write_skill(
            &root_path,
            "not-selected",
            "---\nname: not-selected\ndescription: Not selected\n---\n",
        );
        let root = SkillRoot {
            path: root_path.clone(),
            source: SkillRootSource::Project,
        };
        let catalog = SkillCatalog {
            roots: vec![root],
            skills: vec![
                parse_skill(
                    &root_path.join("selected"),
                    &SkillRoot {
                        path: root_path.clone(),
                        source: SkillRootSource::Project,
                    },
                )
                .unwrap(),
                parse_skill(
                    &root_path.join("not-selected"),
                    &SkillRoot {
                        path: root_path,
                        source: SkillRootSource::Project,
                    },
                )
                .unwrap(),
            ],
            diagnostics: Vec::new(),
        };
        let selection = SkillSelection::from_names(&catalog, ["selected"]);
        let mut session = SessionConfig::default();
        selection.apply_session_config(&catalog, &mut session);
        assert_eq!(session.enable_skills, Some(true));
        assert_eq!(session.skill_directories, Some(catalog.skill_directories()));
        assert_eq!(
            session.disabled_skills,
            Some(vec!["not-selected".to_string()])
        );

        let mut resume = ResumeSessionConfig::new(SessionId::from("session-1"));
        selection.apply_resume_config(&catalog, &mut resume);
        assert_eq!(resume.enable_skills, Some(true));
        assert_eq!(resume.skill_directories, Some(catalog.skill_directories()));
        assert_eq!(
            resume.disabled_skills,
            Some(vec!["not-selected".to_string()])
        );

        let mut empty_session = SessionConfig::default();
        SkillSelection::none().apply_session_config(&catalog, &mut empty_session);
        assert_eq!(empty_session.enable_skills, Some(false));
        assert_eq!(
            empty_session.disabled_skills,
            Some(vec!["not-selected".to_string(), "selected".to_string(),])
        );
    }

    #[test]
    fn extracts_frontmatter_only_between_delimiters() {
        assert_eq!(
            extract_frontmatter("---\nname: test\n---\nbody"),
            Some("name: test\n")
        );
        assert_eq!(extract_frontmatter("name: test\n---\nbody"), None);
    }
}
