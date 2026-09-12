use std::fmt;

#[cfg(windows)]
pub const SHELL_TOOL: &str = "powershell";
#[cfg(not(windows))]
pub const SHELL_TOOL: &str = "bash";

#[cfg(windows)]
pub const CANONICAL_TOOLS: &[&str] = &[
    "powershell",
    "list_powershell",
    "read_powershell",
    "stop_powershell",
    "write_powershell",
    "view",
    "edit",
    "create",
    "apply_patch",
    "grep",
    "glob",
    "task",
    "list_agents",
    "read_agent",
    "write_agent",
    "ask_user",
    "skill"
];

#[cfg(not(windows))]
pub const CANONICAL_TOOLS: &[&str] = &[
    "bash",
    "list_bash",
    "read_bash",
    "stop_bash",
    "write_bash",
    "view",
    "edit",
    "create",
    "apply_patch",
    "grep",
    "glob",
    "task",
    "list_agents",
    "read_agent",
    "write_agent",
    "ask_user",
    "skill"
];

pub const EXCLUDED_TOOLS: &[&str] = &["web_fetch", "web_search"];
pub const TOOL_COUNT: usize = CANONICAL_TOOLS.len();

const ALL_TOOLS_MASK: u32 = (1_u32 << TOOL_COUNT) - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Toolset {
    selected: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToolsetProvenance {
    #[default]
    Default,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolsetError {
    UnknownTool { tool: String },
}

impl fmt::Display for ToolsetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTool { tool } => write!(formatter, "unknown built-in tool '{tool}'"),
        }
    }
}

impl std::error::Error for ToolsetError {}

impl Toolset {
    pub const fn empty() -> Self {
        Self { selected: 0 }
    }

    pub const fn all() -> Self {
        Self {
            selected: ALL_TOOLS_MASK,
        }
    }

    pub const fn shell_only() -> Self {
        Self { selected: 1 }
    }

    pub const fn default_for_model(is_local: bool) -> Self {
        if is_local {
            Self::shell_only()
        } else {
            Self::all()
        }
    }

    pub fn from_tools<I, S>(tools: I) -> Result<Self, ToolsetError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut toolset = Self::empty();
        for tool in tools {
            toolset.select(tool.as_ref())?;
        }
        Ok(toolset)
    }

    pub const fn is_empty(self) -> bool {
        self.selected == 0
    }

    pub const fn len(self) -> usize {
        self.selected.count_ones() as usize
    }

    pub const fn contains_at(self, index: usize) -> bool {
        index < TOOL_COUNT && self.selected & (1_u32 << index) != 0
    }

    pub fn contains(self, tool: &str) -> bool {
        self.index_of(tool)
            .is_some_and(|index| self.contains_at(index))
    }

    pub fn toggle_at(&mut self, index: usize) -> bool {
        if index >= TOOL_COUNT {
            return false;
        }
        self.selected ^= 1_u32 << index;
        true
    }

    pub fn available_tools(self) -> Vec<&'static str> {
        CANONICAL_TOOLS
            .iter()
            .enumerate()
            .filter_map(|(index, tool)| self.contains_at(index).then_some(*tool))
            .collect()
    }

    pub const fn tool_at(index: usize) -> Option<&'static str> {
        if index < TOOL_COUNT {
            Some(CANONICAL_TOOLS[index])
        } else {
            None
        }
    }

    fn select(&mut self, tool: &str) -> Result<(), ToolsetError> {
        let Some(index) = self.index_of(tool) else {
            return Err(ToolsetError::UnknownTool {
                tool: tool.to_string(),
            });
        };
        self.selected |= 1_u32 << index;
        Ok(())
    }

    fn index_of(self, tool: &str) -> Option<usize> {
        CANONICAL_TOOLS
            .iter()
            .position(|candidate| *candidate == tool)
    }
}

impl Default for Toolset {
    fn default() -> Self {
        Self::all()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Toolset, ToolsetError, ToolsetProvenance, CANONICAL_TOOLS, SHELL_TOOL, TOOL_COUNT,
    };

    #[test]
    fn exposes_the_canonical_tools_in_stable_order() {
        assert_eq!(CANONICAL_TOOLS.len(), TOOL_COUNT);
        assert_eq!(CANONICAL_TOOLS[0], SHELL_TOOL);
        assert_eq!(Toolset::all().available_tools(), CANONICAL_TOOLS);
    }

    #[test]
    fn builds_arbitrary_subsets_in_canonical_order() {
        let toolset = Toolset::from_tools(["task", "view", SHELL_TOOL])
            .expect("canonical tools should be accepted");

        assert_eq!(toolset.available_tools(), vec![SHELL_TOOL, "view", "task"]);
        assert_eq!(toolset.len(), 3);
        assert!(toolset.contains("view"));
        assert!(!toolset.contains("edit"));
    }

    #[test]
    fn preserves_an_explicit_empty_selection() {
        let toolset = Toolset::from_tools(std::iter::empty::<&str>())
            .expect("an empty selection should be valid");

        assert!(toolset.is_empty());
        assert_eq!(toolset.available_tools(), Vec::<&str>::new());
    }

    #[test]
    fn rejects_tools_outside_the_canonical_set() {
        assert_eq!(
            Toolset::from_tools(["web_search"]),
            Err(ToolsetError::UnknownTool {
                tool: "web_search".to_string()
            })
        );
    }

    #[test]
    fn toggles_tools_by_picker_index() {
        let mut toolset = Toolset::shell_only();

        assert!(toolset.toggle_at(1));
        assert!(toolset.contains_at(1));
        assert!(toolset.toggle_at(0));
        assert!(!toolset.contains(SHELL_TOOL));
        assert!(!toolset.toggle_at(TOOL_COUNT));
    }

    #[test]
    fn chooses_shell_only_for_local_models_and_all_tools_for_hosted_models() {
        assert_eq!(Toolset::default_for_model(true), Toolset::shell_only());
        assert_eq!(Toolset::default_for_model(false), Toolset::all());
    }

    #[test]
    fn defaults_to_hosted_tools_and_default_provenance() {
        assert_eq!(Toolset::default(), Toolset::all());
        assert_eq!(ToolsetProvenance::default(), ToolsetProvenance::Default);
    }

    #[test]
    fn includes_all_shell_tools() {
        let toolset = Toolset::all();
        let tools = toolset.available_tools();
        assert!(tools.contains(&SHELL_TOOL));
        #[cfg(windows)]
        {
            assert!(tools.contains(&"list_powershell"));
            assert!(tools.contains(&"read_powershell"));
            assert!(tools.contains(&"stop_powershell"));
            assert!(tools.contains(&"write_powershell"));
        }
        #[cfg(not(windows))]
        {
            assert!(tools.contains(&"list_bash"));
            assert!(tools.contains(&"read_bash"));
            assert!(tools.contains(&"stop_bash"));
            assert!(tools.contains(&"write_bash"));
        }
    }

    #[test]
    fn includes_all_file_tools() {
        let toolset = Toolset::all();
        let tools = toolset.available_tools();
        assert!(tools.contains(&"view"));
        assert!(tools.contains(&"edit"));
        assert!(tools.contains(&"create"));
        assert!(tools.contains(&"apply_patch"));
    }

    #[test]
    fn includes_all_agent_tools() {
        let toolset = Toolset::all();
        let tools = toolset.available_tools();
        assert!(tools.contains(&"task"));
        assert!(tools.contains(&"list_agents"));
        assert!(tools.contains(&"read_agent"));
        assert!(tools.contains(&"write_agent"));
    }

    #[test]
    fn includes_other_tools() {
        let toolset = Toolset::all();
        let tools = toolset.available_tools();
        assert!(tools.contains(&"grep"));
        assert!(tools.contains(&"glob"));
        assert!(tools.contains(&"ask_user"));
        assert!(tools.contains(&"skill"));
    }
}
