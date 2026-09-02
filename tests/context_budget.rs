use clap::Parser;
use github_copilot_sdk::rpc::MetadataContextInfoRequest;
use github_copilot_sdk::session_events::ToolExecutionStartData;
use github_copilot_sdk::types::{SessionEvent, SetModelOptions};
use picopilot::config::AppConfig;
use picopilot::runtime::{connect, connect_with_toolset, AppRuntime};
use picopilot::toolset::{Toolset, SHELL_TOOL};
use serde_json::Value;

const ALL_TOOLS_CEILING: i64 = 5_500;
const SHELL_ONLY_CEILING: i64 = 1_500;

#[derive(Debug, Clone, Copy)]
struct ContextBudget {
    tool_tokens: i64,
    tool_count: Option<i64>,
}

async fn context_budget(runtime: &AppRuntime) -> Result<ContextBudget, Box<dyn std::error::Error>> {
    let events = runtime.session.get_events().await?;
    assert!(
        has_empty_system_message(&events),
        "system instructions were not empty"
    );

    let result = runtime
        .session
        .rpc()
        .metadata()
        .get_context_attribution()
        .await?;
    if let Some(context) = result
        .context_attribution
        .filter(|context| context.total_tokens > 0 && !context.model_id.is_empty())
    {
        assert_eq!(context.categories.system_prompt, 0);
        assert_eq!(context.categories.custom_instructions, 0);
        let checkpoint = latest_checkpoint(&events, runtime.active_model_options.model.as_deref());
        return Ok(ContextBudget {
            tool_tokens: context.categories.system_tools,
            tool_count: checkpoint.map(|checkpoint| checkpoint.tool_count),
        });
    }

    let context = runtime
        .session
        .rpc()
        .metadata()
        .context_info(MetadataContextInfoRequest {
            output_token_limit: 0,
            prompt_token_limit: 0,
            selected_model: runtime.active_model_options.model.clone(),
        })
        .await?
        .context_info;
    if let Some(context) = context.filter(|context| {
        context.total_tokens > 0 && !context.model_name.is_empty() && context.system_tokens == 0
    }) {
        assert_eq!(context.system_tokens, 0);
        let checkpoint = latest_checkpoint(&events, runtime.active_model_options.model.as_deref());
        return Ok(ContextBudget {
            tool_tokens: context.tool_definitions_tokens,
            tool_count: checkpoint.map(|checkpoint| checkpoint.tool_count),
        });
    }

    let checkpoint = latest_checkpoint(&events, runtime.active_model_options.model.as_deref())
        .ok_or("context budget was not initialized")?;
    assert_eq!(checkpoint.system_segments, 0);
    Ok(ContextBudget {
        tool_tokens: checkpoint.tool_tokens,
        tool_count: Some(checkpoint.tool_count),
    })
}

struct CheckpointBudget {
    tool_tokens: i64,
    tool_count: i64,
    system_segments: usize,
}

fn has_empty_system_message(events: &[SessionEvent]) -> bool {
    events.iter().any(|event| {
        event.event_type == "system.message"
            && event.data.get("content").and_then(Value::as_str) == Some("")
    })
}

fn latest_checkpoint(events: &[SessionEvent], model: Option<&str>) -> Option<CheckpointBudget> {
    events
        .iter()
        .rev()
        .filter(|event| event.event_type == "session.usage_checkpoint")
        .find_map(|event| {
            let states = event.data.get("promptCacheBreakState")?.as_array()?;
            states.iter().rev().find_map(|state| {
                let models = state.get("models")?.as_object()?;
                let model_state = model
                    .and_then(|model| models.get(model))
                    .or_else(|| models.values().next())?;
                Some(CheckpointBudget {
                    tool_tokens: model_state.get("tool_tokens")?.as_i64()?,
                    tool_count: model_state.get("tool_count")?.as_i64()?,
                    system_segments: model_state.get("system_segments")?.as_array()?.len(),
                })
            })
        })
}

fn has_shell_tool_call(events: &[SessionEvent]) -> bool {
    events.iter().any(|event| {
        event.event_type == "tool.execution_start"
            && event
                .typed_data::<ToolExecutionStartData>()
                .is_some_and(|data| data.tool_name == SHELL_TOOL)
    })
}

#[tokio::test]
#[ignore = "requires a live Copilot CLI, authentication, and local provider"]
async fn empty_session_switches_to_a_local_model_without_resuming(
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("PICOPILOT_CONTEXT_BUDGET_E2E").as_deref() != Ok("1") {
        eprintln!("set PICOPILOT_CONTEXT_BUDGET_E2E=1 to run the live local-model check");
        return Ok(());
    }

    let config = AppConfig::try_parse_from(["picopilot"])?;
    if config.provider_url.is_none() {
        eprintln!("skipping local-model check because PICOPILOT_PROVIDER_URL is not configured");
        return Ok(());
    }
    let mut runtime = connect(&config).await?;
    let initial_session_id = runtime.session.id().clone();
    let local_model = runtime
        .models
        .iter()
        .find(|model| model.id.ends_with("/qwen3.5:4b"))
        .or_else(|| {
            runtime
                .models
                .iter()
                .find(|model| runtime.is_local_model(&model.id))
        })
        .map(|model| model.id.clone())
        .ok_or("provider did not expose a local model")?;

    runtime
        .switch_model(local_model.clone(), None::<SetModelOptions>, None, None)
        .await?;

    assert_ne!(runtime.session.id(), &initial_session_id);
    assert_eq!(
        runtime.active_model_options.model.as_deref(),
        Some(local_model.as_str())
    );
    assert_eq!(runtime.active_toolset, Toolset::shell_only());

    runtime.session.disconnect().await?;
    runtime.client.force_stop();
    Ok(())
}

#[tokio::test]
#[ignore = "requires a live Copilot CLI and authentication; set PICOPILOT_CONTEXT_BUDGET_E2E=1"]
async fn context_budget_stays_empty_and_toolsets_remain_bounded(
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("PICOPILOT_CONTEXT_BUDGET_E2E").as_deref() != Ok("1") {
        eprintln!("set PICOPILOT_CONTEXT_BUDGET_E2E=1 to run the live context-budget check");
        return Ok(());
    }

    let config = AppConfig::try_parse_from(["picopilot"])?;
    let provider_configured = config.provider_url.is_some();
    let mut runtime = connect(&config).await?;

    let local_model = if provider_configured {
        Some(
            runtime
                .models
                .iter()
                .find(|model| runtime.is_local_model(&model.id))
                .map(|model| model.id.clone())
                .ok_or("provider was configured but no local model was discovered")?,
        )
    } else {
        None
    };
    if let Some(local_model) = local_model.clone() {
        runtime
            .switch_model(local_model, None::<SetModelOptions>, None, None)
            .await?;
    }

    runtime
        .session
        .send_and_wait("Reply with exactly READY. Do not use tools.")
        .await?;
    runtime.mark_conversation_started();
    let created = context_budget(&runtime).await?;

    let mut all_runtime = connect_with_toolset(&config, Toolset::all()).await?;
    all_runtime.set_toolset(Toolset::all()).await?;
    if let Some(local_model) = local_model.clone() {
        all_runtime
            .switch_model(local_model, None::<SetModelOptions>, None, None)
            .await?;
    }
    all_runtime
        .session
        .send_and_wait("Reply with exactly READY. Do not use tools.")
        .await?;
    let all = context_budget(&all_runtime).await?;
    assert!(
        all.tool_count
            .is_some_and(|count| { count > 0 && count <= Toolset::all().len() as i64 }),
        "all-tools session exposed more schemas than requested: {:?}",
        all.tool_count
    );
    assert!(
        all.tool_tokens <= ALL_TOOLS_CEILING,
        "all-tools schema cost {} exceeded ceiling {ALL_TOOLS_CEILING}",
        all.tool_tokens
    );

    let mut shell_runtime = connect_with_toolset(&config, Toolset::shell_only()).await?;
    if let Some(local_model) = local_model {
        shell_runtime
            .switch_model(local_model, None::<SetModelOptions>, None, None)
            .await?;
    }
    shell_runtime
        .session
        .send_and_wait("Reply with exactly READY. Do not use tools.")
        .await?;
    let shell_only = context_budget(&shell_runtime).await?;
    assert_eq!(
        shell_only.tool_count,
        Some(Toolset::shell_only().len() as i64)
    );
    assert!(
        shell_only.tool_tokens <= SHELL_ONLY_CEILING,
        "shell-only schema cost {} exceeded ceiling {SHELL_ONLY_CEILING}",
        shell_only.tool_tokens
    );
    assert!(
        shell_only.tool_tokens * 2 < all.tool_tokens,
        "shell-only schema cost {} was not materially below all-tools cost {}",
        shell_only.tool_tokens,
        all.tool_tokens
    );
    assert!(
        created.tool_tokens <= all.tool_tokens,
        "created-session schema cost {} exceeded all-tools cost {}",
        created.tool_tokens,
        all.tool_tokens
    );

    runtime.set_toolset(Toolset::all()).await?;
    runtime.set_toolset(Toolset::shell_only()).await?;
    let session_id = runtime.session.id().clone();
    runtime.resume(session_id.clone()).await?;
    let expected_resumed_tools = runtime
        .active_model_options
        .model
        .as_deref()
        .is_some_and(|model| runtime.is_hosted_model(model));
    assert_eq!(
        runtime.active_toolset,
        if expected_resumed_tools {
            Toolset::all()
        } else {
            Toolset::shell_only()
        },
        "historical resume should derive the toolset from the restored model"
    );

    let resumed_toolset = runtime.active_toolset;
    let resumed_model_options = runtime.active_model_options.clone();
    runtime.recover_transport().await?;
    assert_eq!(
        runtime.active_toolset, resumed_toolset,
        "transport recovery must preserve the active toolset"
    );
    assert_eq!(runtime.active_model_options, resumed_model_options);

    if provider_configured {
        runtime
            .session
            .send_and_wait(format!(
                "Use the {SHELL_TOOL} tool to run `pwd` and report its output. You must use the shell tool."
            ))
            .await?;
        let events = runtime.session.get_events().await?;
        assert!(
            has_shell_tool_call(&events),
            "the local-provider regression must execute the shell tool"
        );
    }

    runtime.session.disconnect().await?;
    runtime.client.force_stop();
    all_runtime.session.disconnect().await?;
    all_runtime.client.force_stop();
    shell_runtime.session.disconnect().await?;
    shell_runtime.client.force_stop();
    Ok(())
}
