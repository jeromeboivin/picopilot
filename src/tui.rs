use crate::events::{BannerSeverity, EventUpdate, UsageSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEntry {
    User(String),
    Assistant {
        message_id: String,
        content: String,
        agent_id: Option<String>,
    },
    Reasoning {
        reasoning_id: String,
        content: String,
        agent_id: Option<String>,
    },
    Tool {
        tool_call_id: String,
        tool_name: String,
        output: String,
        success: Option<bool>,
        agent_id: Option<String>,
    },
    Subagent {
        name: String,
        display_name: String,
        status: SubagentStatus,
        error: Option<String>,
        agent_id: Option<String>,
    },
    Banner {
        severity: BannerSeverity,
        message: String,
        url: Option<String>,
    },
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubagentStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusState {
    pub model: Option<String>,
    pub usage: Option<UsageSnapshot>,
    pub busy: bool,
}

#[derive(Debug, Default)]
pub struct App {
    entries: Vec<ChatEntry>,
    status: StatusState,
    input: String,
    should_quit: bool,
}

impl App {
    pub fn new(model: Option<String>) -> Self {
        Self {
            status: StatusState {
                model,
                ..StatusState::default()
            },
            ..Self::default()
        }
    }

    pub fn entries(&self) -> &[ChatEntry] {
        &self.entries
    }

    pub fn status(&self) -> &StatusState {
        &self.status
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn push_input(&mut self, character: char) {
        self.input.push(character);
    }

    pub fn pop_input(&mut self) {
        self.input.pop();
    }

    pub fn take_input(&mut self) -> String {
        std::mem::take(&mut self.input)
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn add_user_message(&mut self, content: String) {
        self.entries.push(ChatEntry::User(content));
        self.status.busy = true;
    }

    pub fn apply(&mut self, update: EventUpdate) {
        match update {
            EventUpdate::AssistantDelta {
                message_id,
                content,
                agent_id,
            } => self.append_assistant(message_id, content, agent_id),
            EventUpdate::AssistantMessage {
                message_id,
                content,
                agent_id,
            } => self.replace_assistant(message_id, content, agent_id),
            EventUpdate::ReasoningDelta {
                reasoning_id,
                content,
                agent_id,
            } => self.append_reasoning(reasoning_id, content, agent_id),
            EventUpdate::Reasoning {
                reasoning_id,
                content,
                agent_id,
            } => self.replace_reasoning(reasoning_id, content, agent_id),
            EventUpdate::ToolStarted {
                tool_call_id,
                tool_name,
                agent_id,
            } => self.entries.push(ChatEntry::Tool {
                tool_call_id,
                tool_name,
                output: String::new(),
                success: None,
                agent_id,
            }),
            EventUpdate::ToolOutput {
                tool_call_id,
                content,
                agent_id: _,
            } => {
                if let Some(ChatEntry::Tool { output, .. }) = self
                    .entries
                    .iter_mut()
                    .find(|entry| matches!(entry, ChatEntry::Tool { tool_call_id: id, .. } if id == &tool_call_id))
                {
                    output.push_str(&content);
                }
            }
            EventUpdate::ToolCompleted {
                tool_call_id,
                success,
                message,
                agent_id: _,
            } => {
                if let Some(ChatEntry::Tool {
                    output,
                    success: state,
                    ..
                }) = self
                    .entries
                    .iter_mut()
                    .find(|entry| matches!(entry, ChatEntry::Tool { tool_call_id: id, .. } if id == &tool_call_id))
                {
                    *state = Some(success);
                    if let Some(message) = message {
                        output.push_str(&message);
                    }
                }
            }
            EventUpdate::SubagentStarted {
                name,
                display_name,
                agent_id,
            } => self.entries.push(ChatEntry::Subagent {
                name,
                display_name,
                status: SubagentStatus::Running,
                error: None,
                agent_id,
            }),
            EventUpdate::SubagentCompleted { name, agent_id: _ } => {
                if let Some(ChatEntry::Subagent { status, .. }) = self
                    .entries
                    .iter_mut()
                    .find(|entry| matches!(entry, ChatEntry::Subagent { name: current, .. } if current == &name))
                {
                    *status = SubagentStatus::Completed;
                }
            }
            EventUpdate::SubagentFailed {
                name,
                error,
                agent_id: _,
            } => {
                if let Some(ChatEntry::Subagent {
                    status,
                    error: current_error,
                    ..
                }) = self
                    .entries
                    .iter_mut()
                    .find(|entry| matches!(entry, ChatEntry::Subagent { name: current, .. } if current == &name))
                {
                    *status = SubagentStatus::Failed;
                    *current_error = Some(error);
                }
            }
            EventUpdate::Usage(usage) => self.status.usage = Some(usage),
            EventUpdate::Banner {
                severity,
                message,
                url,
            } => self.entries.push(ChatEntry::Banner {
                severity,
                message,
                url,
            }),
            EventUpdate::ModelChanged { model } => self.status.model = Some(model),
            EventUpdate::Idle | EventUpdate::TaskComplete => {
                self.status.busy = false;
                if !matches!(self.entries.last(), Some(ChatEntry::Completed)) {
                    self.entries.push(ChatEntry::Completed);
                }
            }
        }
    }

    fn append_assistant(&mut self, message_id: String, content: String, agent_id: Option<String>) {
        if let Some(ChatEntry::Assistant {
            content: current,
            ..
        }) = self.entries.iter_mut().rev().find(|entry| {
            matches!(entry, ChatEntry::Assistant { message_id: id, .. } if id == &message_id)
        }) {
            current.push_str(&content);
        } else {
            self.entries.push(ChatEntry::Assistant {
                message_id,
                content,
                agent_id,
            });
        }
    }

    fn replace_assistant(&mut self, message_id: String, content: String, agent_id: Option<String>) {
        if let Some(ChatEntry::Assistant {
            content: current,
            agent_id: current_agent,
            ..
        }) = self.entries.iter_mut().rev().find(|entry| {
            matches!(entry, ChatEntry::Assistant { message_id: id, .. } if id == &message_id)
        }) {
            *current = content;
            if current_agent.is_none() {
                *current_agent = agent_id;
            }
        } else {
            self.entries.push(ChatEntry::Assistant {
                message_id,
                content,
                agent_id,
            });
        }
    }

    fn append_reasoning(
        &mut self,
        reasoning_id: String,
        content: String,
        agent_id: Option<String>,
    ) {
        if let Some(ChatEntry::Reasoning {
            content: current,
            ..
        }) = self.entries.iter_mut().rev().find(|entry| {
            matches!(entry, ChatEntry::Reasoning { reasoning_id: id, .. } if id == &reasoning_id)
        }) {
            current.push_str(&content);
        } else {
            self.entries.push(ChatEntry::Reasoning {
                reasoning_id,
                content,
                agent_id,
            });
        }
    }

    fn replace_reasoning(
        &mut self,
        reasoning_id: String,
        content: String,
        agent_id: Option<String>,
    ) {
        if let Some(ChatEntry::Reasoning {
            content: current,
            agent_id: current_agent,
            ..
        }) = self.entries.iter_mut().rev().find(|entry| {
            matches!(entry, ChatEntry::Reasoning { reasoning_id: id, .. } if id == &reasoning_id)
        }) {
            *current = content;
            if current_agent.is_none() {
                *current_agent = agent_id;
            }
        } else {
            self.entries.push(ChatEntry::Reasoning {
                reasoning_id,
                content,
                agent_id,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{App, ChatEntry};
    use crate::events::EventUpdate;

    #[test]
    fn accumulates_streamed_assistant_deltas_into_one_entry() {
        let mut app = App::new(Some("gpt-5".to_string()));

        app.apply(EventUpdate::AssistantDelta {
            message_id: "message-1".to_string(),
            content: "The patch".to_string(),
            agent_id: None,
        });
        app.apply(EventUpdate::AssistantDelta {
            message_id: "message-1".to_string(),
            content: " is ready.".to_string(),
            agent_id: None,
        });

        assert_eq!(
            app.entries(),
            &[ChatEntry::Assistant {
                message_id: "message-1".to_string(),
                content: "The patch is ready.".to_string(),
                agent_id: None,
            }]
        );
    }
}
