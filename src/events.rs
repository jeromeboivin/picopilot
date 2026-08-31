use github_copilot_sdk::rpc::{MetadataContextAttributionResult, UsageGetMetricsResult};
use github_copilot_sdk::session_events::{
    AssistantMessageData, AssistantMessageDeltaData, AssistantReasoningData,
    AssistantReasoningDeltaData, SessionErrorData, SessionModelChangeData, SessionUsageInfoData,
    SessionWarningData, SubagentCompletedData, SubagentFailedData, SubagentStartedData,
    ToolExecutionCompleteData, ToolExecutionPartialResultData, ToolExecutionProgressData,
    ToolExecutionStartData,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventUpdate {
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
        agent_id: Option<String>,
    },
    ToolOutput {
        tool_call_id: String,
        content: String,
        agent_id: Option<String>,
    },
    ToolCompleted {
        tool_call_id: String,
        success: bool,
        message: Option<String>,
        agent_id: Option<String>,
    },
    SubagentStarted {
        name: String,
        display_name: String,
        agent_id: Option<String>,
    },
    SubagentCompleted {
        name: String,
        agent_id: Option<String>,
    },
    SubagentFailed {
        name: String,
        error: String,
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
    Idle,
    TaskComplete,
}

pub fn event_update(event: &SessionEvent) -> Option<EventUpdate> {
    let agent_id = event.agent_id.clone();
    match event.event_type.as_str() {
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
                .map(|data| EventUpdate::ToolOutput {
                    tool_call_id: data.tool_call_id,
                    content: data.progress_message,
                    agent_id,
                })
        }
        "tool.execution_complete" => {
            event
                .typed_data::<ToolExecutionCompleteData>()
                .map(|data| EventUpdate::ToolCompleted {
                    tool_call_id: data.tool_call_id,
                    success: data.success,
                    message: data.error.map(|error| error.message),
                    agent_id,
                })
        }
        "subagent.started" => {
            event
                .typed_data::<SubagentStartedData>()
                .map(|data| EventUpdate::SubagentStarted {
                    name: data.agent_name,
                    display_name: data.agent_display_name,
                    agent_id,
                })
        }
        "subagent.completed" => {
            event
                .typed_data::<SubagentCompletedData>()
                .map(|data| EventUpdate::SubagentCompleted {
                    name: data.agent_name,
                    agent_id,
                })
        }
        "subagent.failed" => {
            event
                .typed_data::<SubagentFailedData>()
                .map(|data| EventUpdate::SubagentFailed {
                    name: data.agent_name,
                    error: data.error,
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
        "session.idle" => Some(EventUpdate::Idle),
        "session.task_complete" => Some(EventUpdate::TaskComplete),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use github_copilot_sdk::rpc::{
        MetadataContextAttributionResult, MetadataContextAttributionResultContextAttribution,
        MetadataContextAttributionResultContextAttributionCategories,
        MetadataContextAttributionResultContextAttributionCompactions, UsageGetMetricsResult,
    };
    use github_copilot_sdk::types::SessionEvent;
    use serde_json::json;

    use super::{context_attribution_snapshot, event_update, usage_metrics_snapshot, EventUpdate};

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
}
