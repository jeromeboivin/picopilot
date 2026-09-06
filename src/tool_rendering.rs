use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::events::ShellCompletion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallState {
    Queued,
    Running,
    Success,
    Error,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolProgressKind {
    Classifier,
    Permission,
    Tool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolResultState {
    Success,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPlatform {
    MacOs,
    WindowsLinux,
}

impl ToolPlatform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::WindowsLinux
        }
    }

    pub fn dot(self) -> &'static str {
        match self {
            Self::MacOs => "⏺",
            Self::WindowsLinux => "●",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolHeaderPayload {
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: Option<Value>,
    pub agent_id: Option<String>,
    pub started_at: u64,
    pub state: ToolCallState,
    pub cwd: PathBuf,
}

impl ToolHeaderPayload {
    pub fn nested(&self) -> bool {
        self.agent_id.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolProgressPayload {
    pub tool_call_id: String,
    pub tool_name: String,
    pub output: String,
    pub status: String,
    pub kind: ToolProgressKind,
    pub agent_id: Option<String>,
    pub started_at: Option<u64>,
    pub timeout: Option<String>,
}

impl ToolProgressPayload {
    pub fn nested(&self) -> bool {
        self.agent_id.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResultPayload {
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: Option<Value>,
    pub content: String,
    pub partial_output: Option<String>,
    pub shell_completion: Option<ShellCompletion>,
    pub state: ToolResultState,
    pub agent_id: Option<String>,
    pub cwd: PathBuf,
}

impl ToolResultPayload {
    pub fn nested(&self) -> bool {
        self.agent_id.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KnownTool {
    Bash,
    Read,
    Edit,
    Write,
    Grep,
    Glob,
    Unknown,
}

fn known_tool(tool_name: &str) -> KnownTool {
    let normalized = tool_name.to_ascii_lowercase();
    if normalized.contains("bash")
        || normalized.contains("shell")
        || normalized.contains("powershell")
    {
        KnownTool::Bash
    } else if normalized == "read" || normalized.ends_with("_read") {
        KnownTool::Read
    } else if normalized == "edit" || normalized.ends_with("_edit") {
        KnownTool::Edit
    } else if normalized == "write" || normalized.ends_with("_write") {
        KnownTool::Write
    } else if normalized == "grep" || normalized.ends_with("_grep") {
        KnownTool::Grep
    } else if normalized == "glob" || normalized.ends_with("_glob") {
        KnownTool::Glob
    } else {
        KnownTool::Unknown
    }
}

pub fn tool_user_facing_name(tool_name: &str) -> String {
    match known_tool(tool_name) {
        KnownTool::Bash => "Bash".to_string(),
        KnownTool::Read => "Read".to_string(),
        KnownTool::Edit => "Edit".to_string(),
        KnownTool::Write => "Write".to_string(),
        KnownTool::Grep => "Grep".to_string(),
        KnownTool::Glob => "Glob".to_string(),
        KnownTool::Unknown => readable_fallback_name(tool_name),
    }
}

pub fn tool_summary(
    tool_name: &str,
    arguments: Option<&Value>,
    cwd: &Path,
    verbose: bool,
) -> String {
    let Some(arguments) = arguments else {
        return String::new();
    };

    match known_tool(tool_name) {
        KnownTool::Bash => bash_summary(arguments, verbose),
        KnownTool::Read => read_summary(arguments, cwd, verbose),
        KnownTool::Edit | KnownTool::Write => edit_summary(arguments, cwd),
        KnownTool::Grep | KnownTool::Glob => search_summary(arguments, cwd),
        KnownTool::Unknown => unknown_summary(arguments),
    }
}

pub(crate) fn is_silent_shell_command(arguments: Option<&Value>) -> bool {
    let Some(command) = arguments.and_then(|arguments| {
        first_string(arguments, &["command", "cmd", "script", "fullCommandText"])
    }) else {
        return false;
    };
    let Some(program) = command
        .split_whitespace()
        .next()
        .map(|program| program.trim_matches(['\'', '"']))
        .and_then(|program| program.rsplit(['/', '\\']).next())
    else {
        return false;
    };

    matches!(
        program.to_ascii_lowercase().as_str(),
        "mv" | "cp"
            | "rm"
            | "mkdir"
            | "rmdir"
            | "chmod"
            | "chown"
            | "chgrp"
            | "touch"
            | "ln"
            | "cd"
            | "export"
            | "unset"
            | "wait"
    )
}

fn bash_summary(arguments: &Value, verbose: bool) -> String {
    let command = first_string(arguments, &["command", "cmd", "script", "fullCommandText"])
        .or_else(|| arguments.as_str().map(ToString::to_string));
    let Some(command) = command else {
        return String::new();
    };

    if is_sed_in_place(&command) {
        return sed_path(&command).unwrap_or_default();
    }
    if verbose {
        return command;
    }

    let mut retained = command.lines().take(2).collect::<Vec<_>>().join("\n");
    let line_truncated = command.lines().count() > 2;
    let unit_truncated = utf16_units(&retained) > 160;
    if unit_truncated {
        retained = truncate_utf16(&retained, 160);
    }
    if line_truncated || unit_truncated {
        retained = format!("{}…", retained.trim());
    }
    retained
}

fn read_summary(arguments: &Value, cwd: &Path, verbose: bool) -> String {
    let Some(path) = first_string(arguments, &["file_path", "path"]) else {
        return String::new();
    };
    let path = display_path(&path, cwd);

    if verbose {
        if let (Some(start), Some(end)) = (line_start(arguments), line_end(arguments)) {
            return format!("{path} · lines {start}-{end}");
        }
        if let Some(start) = line_start(arguments) {
            return format!("{path} · from line {start}");
        }
    }
    if let Some(pages) = value_as_display_string(arguments.get("pages")) {
        return format!("{path} · pages {pages}");
    }
    path
}

fn edit_summary(arguments: &Value, cwd: &Path) -> String {
    let Some(path) = first_string(arguments, &["file_path", "path"]) else {
        return String::new();
    };
    if is_plan_file(&path) {
        String::new()
    } else {
        display_path(&path, cwd)
    }
}

fn search_summary(arguments: &Value, cwd: &Path) -> String {
    let Some(pattern) = first_string(arguments, &["pattern", "query"]) else {
        return String::new();
    };
    let mut summary = format!("pattern: \"{pattern}\"");
    if let Some(path) = first_string(arguments, &["path", "file_path"]) {
        summary.push_str(&format!(", path: \"{}\"", display_path(&path, cwd)));
    }
    summary
}

fn unknown_summary(arguments: &Value) -> String {
    match arguments {
        Value::Object(values) => values
            .iter()
            .find_map(|(key, value)| {
                (!value.is_null()).then(|| format!("{key}: {}", compact_value(value)))
            })
            .unwrap_or_default(),
        Value::Null => String::new(),
        value => compact_value(value),
    }
}

fn first_string(arguments: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        arguments
            .get(key)
            .and_then(Value::as_str)
            .map(ToString::to_string)
    })
}

fn value_as_display_string(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

fn line_start(arguments: &Value) -> Option<String> {
    ["line_start", "start_line", "offset", "from_line"]
        .iter()
        .find_map(|key| value_as_display_string(arguments.get(key)))
}

fn line_end(arguments: &Value) -> Option<String> {
    ["line_end", "end_line", "to_line"]
        .iter()
        .find_map(|key| value_as_display_string(arguments.get(key)))
}

fn compact_value(value: &Value) -> String {
    match value {
        Value::String(value) => format!("\"{value}\""),
        _ => value.to_string(),
    }
}

fn readable_fallback_name(tool_name: &str) -> String {
    let mut result = String::new();
    for (index, word) in tool_name
        .split(['_', '-'])
        .filter(|word| !word.is_empty())
        .enumerate()
    {
        if index > 0 {
            result.push(' ');
        }
        let mut characters = word.chars();
        if let Some(first) = characters.next() {
            result.extend(first.to_uppercase());
            result.extend(characters);
        }
    }
    if result.is_empty() {
        tool_name.to_string()
    } else {
        result
    }
}

fn display_path(path: &str, cwd: &Path) -> String {
    let path = Path::new(path);
    let display = if let Ok(relative) = path.strip_prefix(cwd) {
        relative.to_path_buf()
    } else if let Some(home) = home_directory().and_then(|home| path.strip_prefix(home).ok()) {
        PathBuf::from("~").join(home)
    } else {
        path.to_path_buf()
    };
    normalize_path(&display)
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn home_directory() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn is_plan_file(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    normalized.contains("/.claude/plans/")
        || normalized.starts_with(".claude/plans/")
        || normalized.contains("/plans/") && normalized.contains(".claude")
}

fn is_sed_in_place(command: &str) -> bool {
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    tokens.first().is_some_and(|token| *token == "sed")
        && tokens.iter().any(|token| token.starts_with("-i"))
}

fn sed_path(command: &str) -> Option<String> {
    command
        .split_whitespace()
        .rev()
        .find(|token| !token.starts_with('-') && !token.contains('='))
        .map(|path| path.trim_matches(['\'', '"']).to_string())
}

fn utf16_units(value: &str) -> usize {
    value.encode_utf16().count()
}

fn truncate_utf16(value: &str, max_units: usize) -> String {
    let mut units = 0;
    let mut end = 0;
    for (index, character) in value.char_indices() {
        let character_units = character.len_utf16();
        if units + character_units > max_units {
            break;
        }
        units += character_units;
        end = index + character.len_utf8();
    }
    value[..end].to_string()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;
    use std::path::Path;

    use super::{tool_summary, tool_user_facing_name};

    #[test]
    fn edit_summary_keeps_a_cwd_relative_path() {
        assert_eq!(
            tool_summary(
                "edit",
                Some(&json!({"file_path": "/workspace/src/main.rs"})),
                Path::new("/workspace"),
                false,
            ),
            "src/main.rs"
        );
    }

    #[test]
    fn malformed_arguments_return_a_safe_empty_summary() {
        assert_eq!(
            tool_summary("read", Some(&json!(null)), Path::new("."), false),
            ""
        );
        assert_eq!(
            tool_summary("read", Some(&json!(42)), Path::new("."), false),
            ""
        );
        assert_eq!(tool_summary("read", None, Path::new("."), false), "");
    }

    #[test]
    fn summary_arguments_are_table_driven_and_ignore_unrelated_fields() {
        let cases = vec![
            ("read", json!({}), false, ""),
            ("read", json!({"file_path": null}), false, ""),
            ("read", json!("malformed"), false, ""),
            (
                "read",
                json!({"file_path": "src/lib.rs", "extra": "ignored"}),
                false,
                "src/lib.rs",
            ),
            (
                "read",
                json!({"file_path": "src/lib.rs", "pages": 3}),
                false,
                "src/lib.rs · pages 3",
            ),
            (
                "read",
                json!({"file_path": "src/lib.rs", "line_start": 10, "line_end": 20}),
                true,
                "src/lib.rs · lines 10-20",
            ),
            (
                "read",
                json!({"file_path": "src/lib.rs", "from_line": 10}),
                true,
                "src/lib.rs · from line 10",
            ),
            (
                "edit",
                json!({"file_path": ".claude/plans/plan.md"}),
                false,
                "",
            ),
            (
                "write",
                json!({"file_path": "src/output.txt"}),
                false,
                "src/output.txt",
            ),
            (
                "grep",
                json!({"pattern": "TODO", "path": "src"}),
                false,
                "pattern: \"TODO\", path: \"src\"",
            ),
            (
                "glob",
                json!({"pattern": "*.rs", "path": "src"}),
                false,
                "pattern: \"*.rs\", path: \"src\"",
            ),
        ];

        for (tool, arguments, verbose, expected) in cases {
            assert_eq!(
                tool_summary(tool, Some(&arguments), Path::new("."), verbose),
                expected,
                "unexpected summary for {tool}"
            );
        }
    }

    #[test]
    fn bash_summary_preserves_normal_and_two_line_commands_but_truncates_long_ones() {
        assert_eq!(
            tool_summary(
                "bash",
                Some(&json!({"command": "printf hi"})),
                Path::new("."),
                false
            ),
            "printf hi"
        );
        assert_eq!(
            tool_summary(
                "bash",
                Some(&json!({"command": "printf one\nprintf two"})),
                Path::new("."),
                false,
            ),
            "printf one\nprintf two"
        );
        assert_eq!(
            tool_summary(
                "bash",
                Some(&json!({"command": "x".repeat(161)})),
                Path::new("."),
                false,
            ),
            format!("{}…", "x".repeat(160))
        );
        assert_eq!(
            tool_summary(
                "bash",
                Some(&json!({"command": "one\ntwo\nthree"})),
                Path::new("."),
                false,
            ),
            "one\ntwo…"
        );
        assert_eq!(
            tool_summary(
                "bash",
                Some(&json!({"command": "sed -i 's/a/b/' src/main.rs"})),
                Path::new("."),
                false,
            ),
            "src/main.rs"
        );
    }

    #[test]
    fn paths_prefer_cwd_then_home_then_absolute() {
        let cwd = std::env::temp_dir().join("picopilot-summary-workspace");
        let cwd_path = cwd.join("src/main.rs");
        assert_eq!(
            tool_summary("read", Some(&json!({"file_path": cwd_path})), &cwd, false,),
            "src/main.rs"
        );

        if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
            let home_path = PathBuf::from(home).join("picopilot-summary.txt");
            assert_eq!(
                tool_summary("read", Some(&json!({"file_path": home_path})), &cwd, false,),
                "~/picopilot-summary.txt"
            );
        }
    }

    #[test]
    fn unknown_tools_keep_a_readable_name_and_summary() {
        assert_eq!(tool_user_facing_name("my_tool"), "My Tool");
        assert_eq!(
            tool_summary(
                "my_tool",
                Some(&json!({"value": "hello"})),
                Path::new("."),
                false
            ),
            "value: \"hello\""
        );
    }
}
