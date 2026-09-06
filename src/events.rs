use github_copilot_sdk::rpc::{
    MetadataContextAttributionResult, PlanReadSqlTodosWithDependenciesResult, UsageGetMetricsResult,
};
use github_copilot_sdk::session_events::{
    AssistantMessageData, AssistantMessageDeltaData, AssistantReasoningData,
    AssistantReasoningDeltaData, SessionErrorData, SessionModelChangeData, SessionTodosChangedData,
    SessionUsageInfoData, SessionWarningData, SubagentCompletedData, SubagentFailedData,
    SubagentStartedData, ToolExecutionCompleteContent, ToolExecutionCompleteData,
    ToolExecutionCompleteResult, ToolExecutionPartialResultData, ToolExecutionProgressData,
    ToolExecutionStartData, UserMessageData,
};
use github_copilot_sdk::types::SessionEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BannerSeverity {
    Warning,
    RecoverableError,
    BlockingError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageSnapshot {
    pub current_tokens: i64,
    pub token_limit: i64,
    pub messages: i64,
    pub conversation_tokens: Option<i64>,
    pub system_tokens: Option<i64>,
    pub tool_definitions_tokens: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageMetricsSnapshot {
    pub total_nano_aiu: Option<f64>,
    pub total_premium_request_cost: f64,
    pub total_user_requests: i64,
    pub total_api_duration_ms: i64,
    pub current_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCategorySnapshot {
    pub label: String,
    pub tokens: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextAttributionSnapshot {
    pub model_id: String,
    pub total_tokens: i64,
    pub prompt_token_limit: i64,
    pub categories: Vec<ContextCategorySnapshot>,
    pub compactions: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoRowSnapshot {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoDependencySnapshot {
    pub todo_id: String,
    pub depends_on: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoSnapshot {
    pub rows: Vec<TodoRowSnapshot>,
    pub dependencies: Vec<TodoDependencySnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellExitMetadata {
    pub cwd: Option<String>,
    pub exit_code: i64,
    pub output_file_path: Option<String>,
    pub output_preview: Option<String>,
    pub output_truncated: Option<bool>,
    pub shell_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellCompletion {
    pub exit: Option<ShellExitMetadata>,
    pub output: Option<String>,
    pub image_detected: bool,
}

pub fn usage_metrics_snapshot(metrics: &UsageGetMetricsResult) -> UsageMetricsSnapshot {
    UsageMetricsSnapshot {
        total_nano_aiu: metrics.total_nano_aiu,
        total_premium_request_cost: metrics.total_premium_request_cost,
        total_user_requests: metrics.total_user_requests,
        total_api_duration_ms: metrics.total_api_duration_ms,
        current_model: metrics.current_model.clone(),
    }
}

pub fn context_attribution_snapshot(
    result: &MetadataContextAttributionResult,
) -> Option<ContextAttributionSnapshot> {
    let context = result.context_attribution.as_ref()?;
    let categories = vec![
        ContextCategorySnapshot {
            label: "System instructions".to_string(),
            tokens: context.categories.system_prompt,
        },
        ContextCategorySnapshot {
            label: "Custom instructions".to_string(),
            tokens: context.categories.custom_instructions,
        },
        ContextCategorySnapshot {
            label: "Tool definitions".to_string(),
            tokens: context.categories.system_tools,
        },
        ContextCategorySnapshot {
            label: "MCP tool definitions".to_string(),
            tokens: context.categories.mcp_tools,
        },
        ContextCategorySnapshot {
            label: "Messages and tool results".to_string(),
            tokens: context.categories.messages,
        },
    ];

    Some(ContextAttributionSnapshot {
        model_id: context.model_id.clone(),
        total_tokens: context.total_tokens,
        prompt_token_limit: context.prompt_token_limit,
        categories,
        compactions: context.compactions.count,
    })
}

pub fn todo_snapshot(result: &PlanReadSqlTodosWithDependenciesResult) -> TodoSnapshot {
    TodoSnapshot {
        rows: result
            .rows
            .iter()
            .map(|row| TodoRowSnapshot {
                id: row.id.clone().unwrap_or_else(|| "<unknown>".to_string()),
                title: row
                    .title
                    .clone()
                    .unwrap_or_else(|| "(untitled)".to_string()),
                description: row.description.clone().unwrap_or_default(),
                status: row.status.clone().unwrap_or_else(|| "unknown".to_string()),
            })
            .collect(),
        dependencies: result
            .dependencies
            .iter()
            .map(|dependency| TodoDependencySnapshot {
                todo_id: dependency.todo_id.clone(),
                depends_on: dependency.depends_on.clone(),
            })
            .collect(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventUpdate {
    UserMessage {
        content: String,
    },
    AssistantDelta {
        message_id: String,
        content: String,
        agent_id: Option<String>,
    },
    AssistantMessage {
        message_id: String,
        content: String,
        agent_id: Option<String>,
    },
    ReasoningDelta {
        reasoning_id: String,
        content: String,
        agent_id: Option<String>,
    },
    Reasoning {
        reasoning_id: String,
        content: String,
        agent_id: Option<String>,
    },
    ToolStarted {
        tool_call_id: String,
        tool_name: String,
        arguments: Option<serde_json::Value>,
        agent_id: Option<String>,
    },
    ToolOutput {
        tool_call_id: String,
        content: String,
        agent_id: Option<String>,
    },
    ToolProgress {
        tool_call_id: String,
        content: String,
        agent_id: Option<String>,
    },
    ToolCompleted {
        tool_call_id: String,
        success: bool,
        message: Option<String>,
        shell_completion: Option<ShellCompletion>,
        agent_id: Option<String>,
    },
    ToolCancelled {
        tool_call_id: String,
        message: Option<String>,
        agent_id: Option<String>,
    },
    SubagentStarted {
        name: String,
        description: String,
        display_name: String,
        tool_call_id: String,
        agent_id: Option<String>,
    },
    SubagentCompleted {
        name: String,
        tool_call_id: String,
        cancelled: bool,
        duration_ms: Option<i64>,
        total_tool_calls: Option<i64>,
        total_tokens: Option<i64>,
        agent_id: Option<String>,
    },
    SubagentFailed {
        name: String,
        tool_call_id: String,
        error: String,
        duration_ms: Option<i64>,
        total_tool_calls: Option<i64>,
        total_tokens: Option<i64>,
        agent_id: Option<String>,
    },
    Usage(UsageSnapshot),
    Banner {
        severity: BannerSeverity,
        message: String,
        url: Option<String>,
    },
    ModelChanged {
        model: String,
    },
    TodosChanged,
    Idle,
    TaskComplete,
}

#[allow(deprecated)]
fn map_tool_completion(
    result: ToolExecutionCompleteResult,
) -> (Option<String>, Option<ShellCompletion>) {
    let message = Some(result.detailed_content.unwrap_or(result.content));
    let Some(contents) = result.contents else {
        return (message, None);
    };
    let mut shell_exit = None;
    let mut terminal_exit = None;
    let mut output = Vec::new();
    let mut image_detected = false;

    for content in contents {
        match content {
            ToolExecutionCompleteContent::Text(content) => output.push(content.text),
            ToolExecutionCompleteContent::Terminal(content) => {
                if let Some(exit_code) = content.exit_code {
                    terminal_exit = Some(ShellExitMetadata {
                        cwd: content.cwd,
                        exit_code,
                        output_file_path: None,
                        output_preview: None,
                        output_truncated: None,
                        shell_id: String::new(),
                    });
                }
                output.push(content.text);
            }
            ToolExecutionCompleteContent::ShellExit(content) => {
                shell_exit = Some(ShellExitMetadata {
                    cwd: content.cwd,
                    exit_code: content.exit_code,
                    output_file_path: content.output_file_path,
                    output_preview: content.output_preview,
                    output_truncated: content.output_truncated,
                    shell_id: content.shell_id,
                });
            }
            ToolExecutionCompleteContent::Image(_) => image_detected = true,
            ToolExecutionCompleteContent::Audio(_)
            | ToolExecutionCompleteContent::ResourceLink(_)
            | ToolExecutionCompleteContent::Resource(_) => {}
        }
    }

    (
        message,
        Some(ShellCompletion {
            exit: shell_exit.or(terminal_exit),
            output: (!output.is_empty()).then(|| output.join("\n")),
            image_detected,
        }),
    )
}

pub fn event_update(event: &SessionEvent) -> Option<EventUpdate> {
    let agent_id = event.agent_id.clone();
    match event.event_type.as_str() {
        "user.message" => event
            .typed_data::<UserMessageData>()
            .filter(|data| {
                !data.is_autopilot_continuation.unwrap_or(false)
                    && data.source.as_deref().is_none_or(|source| source == "user")
            })
            .map(|data| EventUpdate::UserMessage {
                content: data.content,
            }),
        "assistant.message_delta" => event.typed_data::<AssistantMessageDeltaData>().map(|data| {
            EventUpdate::AssistantDelta {
                message_id: data.message_id,
                content: data.delta_content,
                agent_id,
            }
        }),
        "assistant.message" => {
            event
                .typed_data::<AssistantMessageData>()
                .map(|data| EventUpdate::AssistantMessage {
                    message_id: data.message_id,
                    content: data.content,
                    agent_id,
                })
        }
        "assistant.reasoning_delta" => {
            event
                .typed_data::<AssistantReasoningDeltaData>()
                .map(|data| EventUpdate::ReasoningDelta {
                    reasoning_id: data.reasoning_id,
                    content: data.delta_content,
                    agent_id,
                })
        }
        "assistant.reasoning" => {
            event
                .typed_data::<AssistantReasoningData>()
                .map(|data| EventUpdate::Reasoning {
                    reasoning_id: data.reasoning_id,
                    content: data.content,
                    agent_id,
                })
        }
        "tool.execution_start" => {
            event
                .typed_data::<ToolExecutionStartData>()
                .map(|data| EventUpdate::ToolStarted {
                    tool_call_id: data.tool_call_id,
                    tool_name: data.tool_name,
                    arguments: data.arguments,
                    agent_id,
                })
        }
        "tool.execution_partial_result" => event
            .typed_data::<ToolExecutionPartialResultData>()
            .map(|data| EventUpdate::ToolOutput {
                tool_call_id: data.tool_call_id,
                content: data.partial_output,
                agent_id,
            }),
        "tool.execution_progress" => {
            event
                .typed_data::<ToolExecutionProgressData>()
                .map(|data| EventUpdate::ToolProgress {
                    tool_call_id: data.tool_call_id,
                    content: data.progress_message,
                    agent_id,
                })
        }
        "tool.execution_complete" => event.typed_data::<ToolExecutionCompleteData>().map(|data| {
            let (message, shell_completion) = data
                .result
                .map(map_tool_completion)
                .unwrap_or_else(|| (None, None));
            EventUpdate::ToolCompleted {
                tool_call_id: data.tool_call_id,
                success: data.success,
                message: message.or_else(|| data.error.map(|error| error.message)),
                shell_completion,
                agent_id,
            }
        }),
        "subagent.started" => {
            event
                .typed_data::<SubagentStartedData>()
                .map(|data| EventUpdate::SubagentStarted {
                    name: data.agent_name,
                    description: data.agent_description,
                    display_name: data.agent_display_name,
                    tool_call_id: data.tool_call_id,
                    agent_id,
                })
        }
        "subagent.completed" => {
            event
                .typed_data::<SubagentCompletedData>()
                .map(|data| EventUpdate::SubagentCompleted {
                    name: data.agent_name,
                    tool_call_id: data.tool_call_id,
                    cancelled: data.cancelled.unwrap_or(false),
                    duration_ms: data.duration_ms,
                    total_tool_calls: data.total_tool_calls,
                    total_tokens: data.total_tokens,
                    agent_id,
                })
        }
        "subagent.failed" => {
            event
                .typed_data::<SubagentFailedData>()
                .map(|data| EventUpdate::SubagentFailed {
                    name: data.agent_name,
                    tool_call_id: data.tool_call_id,
                    error: data.error,
                    duration_ms: data.duration_ms,
                    total_tool_calls: data.total_tool_calls,
                    total_tokens: data.total_tokens,
                    agent_id,
                })
        }
        "session.usage_info" => event.typed_data::<SessionUsageInfoData>().map(|data| {
            EventUpdate::Usage(UsageSnapshot {
                current_tokens: data.current_tokens,
                token_limit: data.token_limit,
                messages: data.messages_length,
                conversation_tokens: data.conversation_tokens,
                system_tokens: data.system_tokens,
                tool_definitions_tokens: data.tool_definitions_tokens,
            })
        }),
        "session.warning" => {
            event
                .typed_data::<SessionWarningData>()
                .map(|data| EventUpdate::Banner {
                    severity: BannerSeverity::Warning,
                    message: data.message,
                    url: data.url,
                })
        }
        "session.error" => event
            .typed_data::<SessionErrorData>()
            .map(|data| EventUpdate::Banner {
                severity: if event.is_transient_error() {
                    BannerSeverity::RecoverableError
                } else {
                    BannerSeverity::BlockingError
                },
                message: data.message,
                url: data.url,
            }),
        "session.model_change" => {
            event
                .typed_data::<SessionModelChangeData>()
                .map(|data| EventUpdate::ModelChanged {
                    model: data.new_model,
                })
        }
        "session.todos_changed" => event
            .typed_data::<SessionTodosChangedData>()
            .map(|_| EventUpdate::TodosChanged),
        "session.idle" => Some(EventUpdate::Idle),
        "session.task_complete" => Some(EventUpdate::TaskComplete),
        _ => None,
    }
}

pub fn latest_message_preview(events: &[SessionEvent]) -> Option<String> {
    events
        .iter()
        .rev()
        .find_map(|event| match event_update(event) {
            Some(EventUpdate::UserMessage { content })
            | Some(EventUpdate::AssistantMessage { content, .. }) => {
                let preview: String = content.chars().take(240).collect();
                (!preview.is_empty()).then_some(preview)
            }
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use github_copilot_sdk::rpc::{
        MetadataContextAttributionResult, MetadataContextAttributionResultContextAttribution,
        MetadataContextAttributionResultContextAttributionCategories,
        MetadataContextAttributionResultContextAttributionCompactions,
        PlanReadSqlTodosWithDependenciesResult, PlanSqlTodoDependency, PlanSqlTodosRow,
        UsageGetMetricsResult,
    };
    use github_copilot_sdk::types::SessionEvent;
    use serde_json::json;

    use super::{
        context_attribution_snapshot, event_update, latest_message_preview, todo_snapshot,
        usage_metrics_snapshot, EventUpdate, ShellCompletion, ShellExitMetadata,
    };

    #[test]
    fn maps_assistant_message_delta_to_a_stream_update() {
        let event = SessionEvent {
            id: "event-1".to_string(),
            timestamp: "2026-08-31T12:00:00Z".to_string(),
            parent_id: None,
            ephemeral: None,
            agent_id: None,
            debug_cli_received_at_ms: None,
            debug_ws_forwarded_at_ms: None,
            event_type: "assistant.message_delta".to_string(),
            data: json!({
                "messageId": "message-1",
                "deltaContent": "The patch is ready."
            }),
        };

        assert_eq!(
            event_update(&event),
            Some(EventUpdate::AssistantDelta {
                message_id: "message-1".to_string(),
                content: "The patch is ready.".to_string(),
                agent_id: None,
            })
        );
    }

    #[test]
    fn maps_subagent_description_metrics_and_cancellation() {
        let started = SessionEvent {
            id: "event-subagent-start".to_string(),
            timestamp: "2026-08-31T12:00:00Z".to_string(),
            parent_id: None,
            ephemeral: None,
            agent_id: None,
            debug_cli_received_at_ms: None,
            debug_ws_forwarded_at_ms: None,
            event_type: "subagent.started".to_string(),
            data: json!({
                "agentName": "explore",
                "agentDisplayName": "Explore",
                "agentDescription": "Inspect the repository",
                "toolCallId": "task-1"
            }),
        };
        let completed = SessionEvent {
            event_type: "subagent.completed".to_string(),
            data: json!({
                "agentName": "explore",
                "agentDisplayName": "Explore",
                "toolCallId": "task-1",
                "cancelled": true,
                "durationMs": 1200,
                "totalToolCalls": 3,
                "totalTokens": 450
            }),
            ..started.clone()
        };

        assert_eq!(
            event_update(&started),
            Some(EventUpdate::SubagentStarted {
                name: "explore".to_string(),
                description: "Inspect the repository".to_string(),
                display_name: "Explore".to_string(),
                tool_call_id: "task-1".to_string(),
                agent_id: None,
            })
        );
        assert_eq!(
            event_update(&completed),
            Some(EventUpdate::SubagentCompleted {
                name: "explore".to_string(),
                tool_call_id: "task-1".to_string(),
                cancelled: true,
                duration_ms: Some(1200),
                total_tool_calls: Some(3),
                total_tokens: Some(450),
                agent_id: None,
            })
        );
    }

    #[test]
    fn maps_shell_arguments_and_completed_output() {
        let start = SessionEvent {
            id: "event-shell-start".to_string(),
            timestamp: "2026-08-31T12:00:00Z".to_string(),
            parent_id: None,
            ephemeral: None,
            agent_id: None,
            debug_cli_received_at_ms: None,
            debug_ws_forwarded_at_ms: None,
            event_type: "tool.execution_start".to_string(),
            data: json!({
                "toolCallId": "tool-1",
                "toolName": "powershell",
                "arguments": { "command": "Get-CimInstance Win32_OperatingSystem" }
            }),
        };
        let complete = SessionEvent {
            id: "event-shell-complete".to_string(),
            event_type: "tool.execution_complete".to_string(),
            data: json!({
                "toolCallId": "tool-1",
                "success": true,
                "result": {
                    "content": "Microsoft Windows 11 Pro",
                    "detailedContent": "Caption: Microsoft Windows 11 Pro\nVersion: 10.0.26100"
                }
            }),
            ..start.clone()
        };

        assert_eq!(
            event_update(&start),
            Some(EventUpdate::ToolStarted {
                tool_call_id: "tool-1".to_string(),
                tool_name: "powershell".to_string(),
                arguments: Some(json!({
                    "command": "Get-CimInstance Win32_OperatingSystem"
                })),
                agent_id: None,
            })
        );
        assert_eq!(
            event_update(&complete),
            Some(EventUpdate::ToolCompleted {
                tool_call_id: "tool-1".to_string(),
                success: true,
                message: Some("Caption: Microsoft Windows 11 Pro\nVersion: 10.0.26100".to_string()),
                agent_id: None,
                shell_completion: None,
            })
        );
    }

    #[test]
    fn maps_structured_shell_completion_without_retaining_binary_data() {
        let event = SessionEvent {
            id: "event-shell-structured".to_string(),
            timestamp: "2026-08-31T12:00:00Z".to_string(),
            parent_id: None,
            ephemeral: None,
            agent_id: None,
            debug_cli_received_at_ms: None,
            debug_ws_forwarded_at_ms: None,
            event_type: "tool.execution_complete".to_string(),
            data: json!({
                "toolCallId": "tool-structured",
                "success": true,
                "result": {
                    "content": "concise fallback",
                    "detailedContent": "detailed fallback",
                    "contents": [
                        {
                            "type": "shell_exit",
                            "cwd": "C:/work",
                            "exitCode": 0,
                            "outputFilePath": "C:/work/output.txt",
                            "outputPreview": "preview",
                            "outputTruncated": true,
                            "shellId": "shell-1"
                        },
                        {
                            "type": "terminal",
                            "cwd": "C:/terminal-fallback",
                            "exitCode": 9,
                            "text": "terminal output"
                        },
                        {"type": "text", "text": "text output"},
                        {
                            "type": "image",
                            "data": "base64-secret",
                            "mimeType": "image/png"
                        }
                    ]
                }
            }),
        };

        assert_eq!(
            event_update(&event),
            Some(EventUpdate::ToolCompleted {
                tool_call_id: "tool-structured".to_string(),
                success: true,
                message: Some("detailed fallback".to_string()),
                shell_completion: Some(ShellCompletion {
                    exit: Some(ShellExitMetadata {
                        cwd: Some("C:/work".to_string()),
                        exit_code: 0,
                        output_file_path: Some("C:/work/output.txt".to_string()),
                        output_preview: Some("preview".to_string()),
                        output_truncated: Some(true),
                        shell_id: "shell-1".to_string(),
                    }),
                    output: Some("terminal output\ntext output".to_string()),
                    image_detected: true,
                }),
                agent_id: None,
            })
        );
    }

    #[test]
    fn maps_terminal_exit_metadata_when_shell_exit_is_absent() {
        let event = SessionEvent {
            id: "event-terminal-only".to_string(),
            timestamp: "2026-08-31T12:00:00Z".to_string(),
            parent_id: None,
            ephemeral: None,
            agent_id: None,
            debug_cli_received_at_ms: None,
            debug_ws_forwarded_at_ms: None,
            event_type: "tool.execution_complete".to_string(),
            data: json!({
                "toolCallId": "tool-terminal-only",
                "success": false,
                "result": {
                    "content": "",
                    "contents": [
                        {
                            "type": "terminal",
                            "cwd": "C:/terminal",
                            "exitCode": 2,
                            "text": "terminal failure"
                        }
                    ]
                }
            }),
        };

        assert_eq!(
            event_update(&event),
            Some(EventUpdate::ToolCompleted {
                tool_call_id: "tool-terminal-only".to_string(),
                success: false,
                message: Some("".to_string()),
                shell_completion: Some(ShellCompletion {
                    exit: Some(ShellExitMetadata {
                        cwd: Some("C:/terminal".to_string()),
                        exit_code: 2,
                        output_file_path: None,
                        output_preview: None,
                        output_truncated: None,
                        shell_id: String::new(),
                    }),
                    output: Some("terminal failure".to_string()),
                    image_detected: false,
                }),
                agent_id: None,
            })
        );
    }

    #[test]
    fn keeps_tool_progress_distinct_from_partial_output() {
        let partial = SessionEvent {
            id: "event-partial".to_string(),
            timestamp: "2026-08-31T12:00:00Z".to_string(),
            parent_id: None,
            ephemeral: None,
            agent_id: None,
            debug_cli_received_at_ms: None,
            debug_ws_forwarded_at_ms: None,
            event_type: "tool.execution_partial_result".to_string(),
            data: json!({
                "toolCallId": "tool-1",
                "partialOutput": "stdout chunk"
            }),
        };
        let progress = SessionEvent {
            id: "event-progress".to_string(),
            event_type: "tool.execution_progress".to_string(),
            data: json!({
                "toolCallId": "tool-1",
                "progressMessage": "waiting"
            }),
            ..partial.clone()
        };

        assert_eq!(
            event_update(&partial),
            Some(EventUpdate::ToolOutput {
                tool_call_id: "tool-1".to_string(),
                content: "stdout chunk".to_string(),
                agent_id: None,
            })
        );
        assert_eq!(
            event_update(&progress),
            Some(EventUpdate::ToolProgress {
                tool_call_id: "tool-1".to_string(),
                content: "waiting".to_string(),
                agent_id: None,
            })
        );
    }

    #[test]
    fn maps_visible_user_messages_but_not_autopilot_continuations() {
        let visible = SessionEvent {
            id: "event-user".to_string(),
            timestamp: "2026-08-31T12:00:00Z".to_string(),
            parent_id: None,
            ephemeral: None,
            agent_id: None,
            debug_cli_received_at_ms: None,
            debug_ws_forwarded_at_ms: None,
            event_type: "user.message".to_string(),
            data: json!({ "content": "Fix the parser", "source": "user" }),
        };
        let continuation = SessionEvent {
            id: "event-continuation".to_string(),
            data: json!({
                "content": "Continue working",
                "isAutopilotContinuation": true,
                "source": "system"
            }),
            ..visible.clone()
        };

        assert_eq!(
            event_update(&visible),
            Some(EventUpdate::UserMessage {
                content: "Fix the parser".to_string(),
            })
        );
        assert_eq!(event_update(&continuation), None);
    }

    #[test]
    fn previews_the_latest_complete_visible_message() {
        let user = SessionEvent {
            id: "event-user".to_string(),
            timestamp: "2026-08-31T12:00:00Z".to_string(),
            parent_id: None,
            ephemeral: None,
            agent_id: None,
            debug_cli_received_at_ms: None,
            debug_ws_forwarded_at_ms: None,
            event_type: "user.message".to_string(),
            data: json!({ "content": "Fix the parser", "source": "user" }),
        };
        let assistant = SessionEvent {
            id: "event-assistant".to_string(),
            event_type: "assistant.message".to_string(),
            data: json!({
                "messageId": "message-1",
                "content": "The parser is fixed."
            }),
            ..user.clone()
        };

        assert_eq!(
            latest_message_preview(&[user, assistant]).as_deref(),
            Some("The parser is fixed.")
        );
    }

    #[test]
    fn maps_session_usage_metrics_to_a_renderable_snapshot() {
        let metrics = UsageGetMetricsResult {
            total_nano_aiu: Some(3.5),
            total_premium_request_cost: 2.0,
            total_user_requests: 4,
            total_api_duration_ms: 1250,
            current_model: Some("gpt-5".to_string()),
            ..UsageGetMetricsResult::default()
        };

        let snapshot = usage_metrics_snapshot(&metrics);

        assert_eq!(snapshot.total_nano_aiu, Some(3.5));
        assert_eq!(snapshot.total_premium_request_cost, 2.0);
        assert_eq!(snapshot.total_user_requests, 4);
        assert_eq!(snapshot.total_api_duration_ms, 1250);
        assert_eq!(snapshot.current_model.as_deref(), Some("gpt-5"));
    }

    #[test]
    fn maps_context_attribution_categories_and_compactions() {
        let attribution = MetadataContextAttributionResult {
            context_attribution: Some(MetadataContextAttributionResultContextAttribution {
                categories: MetadataContextAttributionResultContextAttributionCategories {
                    custom_instructions: 2,
                    mcp_tools: 4,
                    messages: 20,
                    system_prompt: 10,
                    system_tools: 4,
                    ..Default::default()
                },
                compactions: MetadataContextAttributionResultContextAttributionCompactions {
                    count: 3,
                },
                model_id: "gpt-5".to_string(),
                prompt_token_limit: 100,
                total_tokens: 40,
                ..Default::default()
            }),
        };

        let snapshot = context_attribution_snapshot(&attribution)
            .expect("initialized attribution should produce a snapshot");

        assert_eq!(snapshot.model_id, "gpt-5");
        assert_eq!(snapshot.total_tokens, 40);
        assert_eq!(snapshot.prompt_token_limit, 100);
        assert_eq!(snapshot.categories[0].tokens, 10);
        assert_eq!(snapshot.categories[3].tokens, 4);
        assert_eq!(snapshot.compactions, 3);
    }

    #[test]
    fn maps_todos_changed_to_a_refresh_signal() {
        let event = SessionEvent {
            id: "event-todos".to_string(),
            timestamp: "2026-08-31T12:00:00Z".to_string(),
            parent_id: None,
            ephemeral: None,
            agent_id: None,
            debug_cli_received_at_ms: None,
            debug_ws_forwarded_at_ms: None,
            event_type: "session.todos_changed".to_string(),
            data: json!({}),
        };

        assert_eq!(event_update(&event), Some(EventUpdate::TodosChanged));
    }

    #[test]
    fn maps_todo_rows_and_dependencies_to_a_renderable_snapshot() {
        let todos = PlanReadSqlTodosWithDependenciesResult {
            dependencies: vec![PlanSqlTodoDependency {
                depends_on: "todo-1".to_string(),
                todo_id: "todo-2".to_string(),
            }],
            rows: vec![
                PlanSqlTodosRow {
                    description: Some("Inspect the transport path".to_string()),
                    id: Some("todo-1".to_string()),
                    status: Some("completed".to_string()),
                    title: Some("Read the uploader".to_string()),
                    ..Default::default()
                },
                PlanSqlTodosRow {
                    description: Some("Add bounded retries".to_string()),
                    id: Some("todo-2".to_string()),
                    status: Some("in_progress".to_string()),
                    title: Some("Patch the uploader".to_string()),
                    ..Default::default()
                },
            ],
        };

        let snapshot = todo_snapshot(&todos);

        assert_eq!(snapshot.rows[1].title, "Patch the uploader");
        assert_eq!(snapshot.rows[1].status, "in_progress");
        assert_eq!(snapshot.dependencies[0].depends_on, "todo-1");
        assert_eq!(snapshot.dependencies[0].todo_id, "todo-2");
    }
}
