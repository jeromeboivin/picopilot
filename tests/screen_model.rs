use picopilot::ansi::sanitize_ansi;
use picopilot::events::EventUpdate;
use picopilot::palette;
use picopilot::screen_model::{
    enter_main_screen, live_preview_enabled, render_entry_lines, render_transcript_payload,
    render_transcript_payload_with_clock, render_transcript_payload_with_options, terminal_options,
    LiveEntryKind, Platform, ScreenChange, ScreenEntry, ScreenModel, ToolCallState,
    ToolHeaderPayload, ToolPlatform, ToolProgressKind, ToolProgressPayload, ToolResultPayload,
    ToolResultState, TranscriptPayload, FIXED_LIVE_REGION_HEIGHT,
};
use picopilot::tui::{App, ChatEntry};
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::{Terminal, Viewport};
use serde_json::json;
use unicode_width::UnicodeWidthStr;

fn apply_pending_changes(
    app: &mut App,
    screen: &mut ScreenModel,
    terminal: &mut Terminal<TestBackend>,
) {
    for change in app.take_screen_changes() {
        screen
            .apply_change(terminal, change)
            .expect("screen change should apply");
    }
}

fn terminal_text(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn assistant_markdown_visual_buffer_fixtures_at_required_widths() {
    let mut app = App::new(None);
    app.apply(EventUpdate::AssistantMessage {
        message_id: "assistant-markdown-fixture".to_string(),
        content: "# Ready\n\nText **bold**.\n\n```rust\nlet value = 42;\n```".to_string(),
        agent_id: None,
    });
    let changes = app.take_screen_changes();
    let entry = changes
        .iter()
        .find_map(|change| match change {
            ScreenChange::Upsert(entry) => Some(entry),
            ScreenChange::Reset | ScreenChange::Remove(_) => None,
        })
        .expect("assistant fixture screen entry");

    let expected = [
        (
            10,
            vec![
                "",
                "● Ready",
                "  ",
                "  Text ",
                "  bold.",
                "  let ",
                "  value = ",
                "  42;",
            ],
        ),
        (
            40,
            vec!["", "● Ready", "  ", "  Text bold.", "  let value = 42;"],
        ),
        (
            80,
            vec!["", "● Ready", "  ", "  Text bold.", "  let value = 42;"],
        ),
    ];
    for (width, expected) in expected {
        let actual = render_transcript_payload(entry.kind(), entry.payload(), width)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            expected.into_iter().map(str::to_string).collect::<Vec<_>>()
        );
    }
}

#[test]
fn assistant_vertical_table_rows_fit_after_the_transcript_prefix_is_added() {
    let payload = TranscriptPayload::AssistantMarkdown(
        "| A | B | C |\n| --- | --- | --- |\n| one | two | three |\n| four | five | six |"
            .to_string(),
    );
    let rendered = render_transcript_payload(LiveEntryKind::Assistant, &payload, 20);

    assert!(rendered
        .iter()
        .all(|line| UnicodeWidthStr::width(line.to_string().as_str()) <= 20));
    assert_eq!(
        rendered
            .iter()
            .filter(|line| line.to_string().contains('─'))
            .count(),
        1
    );
}

#[test]
fn app_queues_raw_assistant_markdown_for_width_aware_rendering() {
    let markdown = "| Header | Value |\n| --- | --- |\n| narrow | wide content |";
    let mut app = App::new(None);

    app.apply(EventUpdate::AssistantDelta {
        message_id: "assistant-table".to_string(),
        content: markdown.to_string(),
        agent_id: None,
    });

    let entry = app
        .take_screen_changes()
        .into_iter()
        .find_map(|change| match change {
            ScreenChange::Upsert(entry) => Some(entry),
            ScreenChange::Reset | ScreenChange::Remove(_) => None,
        })
        .expect("assistant screen entry");

    assert_eq!(
        entry.payload(),
        &TranscriptPayload::AssistantMarkdown(markdown.to_string())
    );
}

#[test]
fn app_sanitizes_assistant_ansi_before_screen_storage() {
    let mut app = App::new(None);

    app.apply(EventUpdate::AssistantMessage {
        message_id: "assistant-ansi".to_string(),
        content: "\u{1b}[31mred\u{1b}[0m".to_string(),
        agent_id: None,
    });

    let entry = app
        .take_screen_changes()
        .into_iter()
        .find_map(|change| match change {
            ScreenChange::Upsert(entry) => Some(entry),
            ScreenChange::Reset | ScreenChange::Remove(_) => None,
        })
        .expect("assistant screen entry");

    assert_eq!(
        entry.payload(),
        &TranscriptPayload::AssistantMarkdown("red".to_string())
    );
}

#[test]
fn wrapped_tool_result_keeps_sgr_style_on_every_fragment_without_escape_bytes() {
    let payload = TranscriptPayload::ToolResult(ToolResultPayload {
        tool_call_id: "tool-ansi".to_string(),
        tool_name: "bash".to_string(),
        arguments: None,
        content: sanitize_ansi("\u{1b}[31mred red red red red\u{1b}[0m"),
        state: ToolResultState::Success,
        agent_id: None,
        cwd: std::path::PathBuf::from("."),
    });

    let lines = render_transcript_payload(LiveEntryKind::Tool, &payload, 16);
    let fragments = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .filter(|span| span.content.contains("red"))
        .collect::<Vec<_>>();

    assert!(fragments.len() > 1);
    assert!(fragments
        .iter()
        .all(|span| span.style.fg == Some(ratatui::style::Color::Red)));
    assert!(lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .all(|span| !span.content.contains('\u{1b}')));
}

#[test]
fn tool_error_normalization_preserves_sgr_styles_without_escape_bytes() {
    let payload = TranscriptPayload::ToolResult(ToolResultPayload {
        tool_call_id: "tool-error-ansi".to_string(),
        tool_name: "bash".to_string(),
        arguments: None,
        content: sanitize_ansi("<error>failure \u{1b}[31mred\u{1b}[0m</error>"),
        state: ToolResultState::Error,
        agent_id: None,
        cwd: std::path::PathBuf::from("."),
    });

    let lines = render_transcript_payload(LiveEntryKind::Tool, &payload, 80);
    let red = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.contains("red"))
        .expect("styled error content");

    assert_eq!(red.style.fg, Some(ratatui::style::Color::Red));
    assert!(lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .all(|span| !span.content.contains('\u{1b}')));
    assert!(lines
        .iter()
        .any(|line| line.to_string().contains("Error: failure red")));
}

#[test]
fn app_keeps_incremental_ansi_state_per_logical_stream() {
    let mut app = App::new(None);

    app.apply(EventUpdate::AssistantDelta {
        message_id: "assistant-red".to_string(),
        content: "\u{1b}[31".to_string(),
        agent_id: None,
    });
    app.apply(EventUpdate::AssistantDelta {
        message_id: "assistant-green".to_string(),
        content: "\u{1b}[32".to_string(),
        agent_id: None,
    });
    app.apply(EventUpdate::AssistantDelta {
        message_id: "assistant-red".to_string(),
        content: "mred".to_string(),
        agent_id: None,
    });
    app.apply(EventUpdate::AssistantDelta {
        message_id: "assistant-green".to_string(),
        content: "mgreen".to_string(),
        agent_id: None,
    });

    let assistants = app
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            ChatEntry::Assistant {
                message_id,
                content,
                ..
            } => Some((message_id.as_str(), content.as_str())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        assistants,
        vec![("assistant-red", "red"), ("assistant-green", "green"),]
    );
}

#[test]
fn reset_drops_unfinished_tool_ansi_state_before_a_reused_id() {
    let mut app = App::new(None);
    app.apply(EventUpdate::ToolStarted {
        tool_call_id: "tool-reused".to_string(),
        tool_name: "bash".to_string(),
        arguments: Some(json!({"command": "echo test"})),
        agent_id: None,
    });
    app.apply(EventUpdate::ToolOutput {
        tool_call_id: "tool-reused".to_string(),
        content: "before \u{1b}[31".to_string(),
        agent_id: None,
    });

    app.reset_for_new_conversation();
    app.apply(EventUpdate::ToolStarted {
        tool_call_id: "tool-reused".to_string(),
        tool_name: "bash".to_string(),
        arguments: Some(json!({"command": "echo test"})),
        agent_id: None,
    });
    app.apply(EventUpdate::ToolOutput {
        tool_call_id: "tool-reused".to_string(),
        content: "mcontinued".to_string(),
        agent_id: None,
    });

    assert!(app.entries().iter().any(|entry| matches!(
        entry,
        ChatEntry::ToolProgress { content, .. } if content == "mcontinued"
    )));
}

#[test]
fn idle_and_reconnect_drop_unfinished_ansi_state() {
    let mut app = App::new(None);
    app.apply(EventUpdate::AssistantDelta {
        message_id: "assistant-lifecycle".to_string(),
        content: "\u{1b}[31".to_string(),
        agent_id: None,
    });
    app.apply(EventUpdate::Idle);
    app.apply(EventUpdate::AssistantDelta {
        message_id: "assistant-lifecycle".to_string(),
        content: "mcontinued".to_string(),
        agent_id: None,
    });

    app.apply(EventUpdate::ToolStarted {
        tool_call_id: "tool-lifecycle".to_string(),
        tool_name: "bash".to_string(),
        arguments: None,
        agent_id: None,
    });
    app.apply(EventUpdate::ToolOutput {
        tool_call_id: "tool-lifecycle".to_string(),
        content: "tool \u{1b}[31".to_string(),
        agent_id: None,
    });
    app.set_reconnecting(true);
    app.set_reconnecting(false);
    app.apply(EventUpdate::ToolOutput {
        tool_call_id: "tool-lifecycle".to_string(),
        content: "mcontinued".to_string(),
        agent_id: None,
    });

    assert!(app.entries().iter().any(|entry| matches!(
        entry,
        ChatEntry::Assistant { content, .. } if content == "mcontinued"
    )));
    assert!(app.entries().iter().any(|entry| matches!(
        entry,
        ChatEntry::ToolProgress { content, .. } if content == "tool mcontinued"
    )));
}

#[test]
fn all_untrusted_display_surfaces_are_plain_or_sanitized_before_storage() {
    let mut app = App::new(None);
    app.add_user_message("user \u{1b}[31mtext".to_string());
    app.apply(EventUpdate::Reasoning {
        reasoning_id: "reasoning-1".to_string(),
        content: "reasoning \u{1b}[2Jtext".to_string(),
        agent_id: None,
    });
    app.apply(EventUpdate::Banner {
        severity: picopilot::events::BannerSeverity::Warning,
        message: "banner \u{1b}]0;title\u{07}text".to_string(),
        url: Some("https://example.test/\u{1b}[31murl".to_string()),
    });
    app.apply(EventUpdate::ToolStarted {
        tool_call_id: "tool-display".to_string(),
        tool_name: "bash".to_string(),
        arguments: Some(json!({"command": "printf '\u{1b}[31marg"})),
        agent_id: None,
    });
    app.apply(EventUpdate::ToolCompleted {
        tool_call_id: "tool-display".to_string(),
        success: false,
        message: Some("<error>failure \u{1b}[31mtext</error>".to_string()),
        agent_id: None,
    });

    assert!(app.entries().iter().all(|entry| match entry {
        ChatEntry::User(content)
        | ChatEntry::Diagnostic(content)
        | ChatEntry::Reasoning { content, .. } => !content.contains('\u{1b}'),
        ChatEntry::Banner { message, url, .. } => {
            !message.contains('\u{1b}') && url.as_deref().is_none_or(|url| !url.contains('\u{1b}'))
        }
        ChatEntry::ToolResult { .. } => true,
        _ => true,
    }));

    let result = app
        .entries()
        .iter()
        .find_map(|entry| match entry {
            ChatEntry::ToolResult {
                tool_call_id,
                tool_name,
                arguments,
                content,
                state,
                agent_id,
                cwd,
            } => Some(TranscriptPayload::ToolResult(ToolResultPayload {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                arguments: arguments.clone(),
                content: content.clone(),
                state: *state,
                agent_id: agent_id.clone(),
                cwd: cwd.clone(),
            })),
            _ => None,
        })
        .expect("tool result should be stored");
    let rendered = render_transcript_payload(LiveEntryKind::Tool, &result, 80);
    assert!(rendered
        .iter()
        .any(|line| line.to_string().contains("Error: failure text")));
}

#[test]
fn test_backend_never_receives_untrusted_escape_or_control_characters() {
    let mut app = App::new(None);
    app.add_user_message("user \u{1b}[2J\u{0007}text".to_string());
    app.apply(EventUpdate::AssistantMessage {
        message_id: "assistant-controls".to_string(),
        content: "assistant \u{1b}]0;title\u{0007}text".to_string(),
        agent_id: None,
    });
    app.apply(EventUpdate::Reasoning {
        reasoning_id: "reasoning-controls".to_string(),
        content: "reasoning \u{009b}2Jtext".to_string(),
        agent_id: Some("agent \u{1b}[31m".to_string()),
    });
    app.apply(EventUpdate::ToolStarted {
        tool_call_id: "tool-controls".to_string(),
        tool_name: "bash".to_string(),
        arguments: Some(json!({"command": "printf '\u{1b}[31marg"})),
        agent_id: None,
    });
    app.apply(EventUpdate::ToolOutput {
        tool_call_id: "tool-controls".to_string(),
        content: "output \u{1b}[31mred\u{1b}[0m\u{0008}".to_string(),
        agent_id: None,
    });
    app.apply(EventUpdate::ToolCompleted {
        tool_call_id: "tool-controls".to_string(),
        success: false,
        message: Some("<error>failure \u{1b}[31mred</error>".to_string()),
        agent_id: None,
    });
    app.apply(EventUpdate::Banner {
        severity: picopilot::events::BannerSeverity::Warning,
        message: "banner \u{1b}7text".to_string(),
        url: None,
    });
    app.apply(EventUpdate::SubagentStarted {
        name: "worker".to_string(),
        display_name: "worker".to_string(),
        tool_call_id: "subagent-controls".to_string(),
        agent_id: Some("agent \u{1b}[31m".to_string()),
    });
    app.apply(EventUpdate::ToolStarted {
        tool_call_id: "tool-argument-controls".to_string(),
        tool_name: "custom \u{1b}[31m".to_string(),
        arguments: Some(json!({"key \u{1b}[31m": "value \u{1b}[2J"})),
        agent_id: None,
    });

    let mut terminal = Terminal::new(TestBackend::new(100, 80)).expect("test terminal");
    let mut screen = ScreenModel::default();
    for change in app.take_screen_changes() {
        screen
            .apply_change(&mut terminal, change)
            .expect("screen change should apply");
    }
    terminal
        .draw(|frame| screen.draw_live(frame, Platform::default()))
        .expect("live screen should render");

    assert!(terminal.backend().buffer().content().iter().all(|cell| {
        cell.symbol().chars().all(|character| {
            !character.is_control() && !(('\u{0080}'..='\u{009f}').contains(&character))
        })
    }));
}

#[test]
fn non_assistant_entries_keep_the_pre_rendered_payload_path() {
    let mut app = App::new(None);
    app.add_user_message("pre-rendered user".to_string());

    let entry = app
        .take_screen_changes()
        .into_iter()
        .find_map(|change| match change {
            ScreenChange::Upsert(entry) => Some(entry),
            ScreenChange::Reset | ScreenChange::Remove(_) => None,
        })
        .expect("user screen entry");

    assert!(matches!(
        entry.payload(),
        TranscriptPayload::PreRendered(lines) if lines.iter().any(|line| line.to_string() == "pre-rendered user")
    ));
}

#[test]
fn live_assistant_tables_rerender_after_a_width_change() {
    let content =
        "| Name | Description |\n| --- | --- |\n| alpha | a long description that needs room |";
    let mut screen = ScreenModel::default();
    let mut terminal = Terminal::with_options(TestBackend::new(80, 24), terminal_options())
        .expect("inline terminal should initialize");
    screen
        .apply_change(
            &mut terminal,
            ScreenChange::Upsert(ScreenEntry::with_payload(
                "assistant-table",
                LiveEntryKind::Assistant,
                TranscriptPayload::AssistantMarkdown(content.to_string()),
                false,
            )),
        )
        .expect("live table should apply");

    let narrow = screen
        .live_lines_at_width(
            Platform {
                is_windows: false,
                wt_session: false,
            },
            24,
        )
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let wide = screen
        .live_lines_at_width(
            Platform {
                is_windows: false,
                wt_session: false,
            },
            80,
        )
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    assert_ne!(narrow, wide);
    assert!(matches!(
        screen.live_entries()[0].payload(),
        TranscriptPayload::AssistantMarkdown(value) if value == content
    ));
}

#[test]
fn completed_assistant_tables_commit_at_the_current_width_and_keep_only_metadata() {
    let content = "| Header | Value |\n| --- | --- |\n| key | a value that wraps |";
    let mut screen = ScreenModel::default();
    let mut terminal = Terminal::with_options(TestBackend::new(40, 24), terminal_options())
        .expect("inline terminal should initialize");
    screen
        .apply_change(
            &mut terminal,
            ScreenChange::Upsert(ScreenEntry::with_payload(
                "assistant-table",
                LiveEntryKind::Assistant,
                TranscriptPayload::AssistantMarkdown(content.to_string()),
                false,
            )),
        )
        .expect("live table should apply");
    let live_rows = screen
        .live_lines_at_width(
            Platform {
                is_windows: false,
                wt_session: false,
            },
            40,
        )
        .len();

    screen
        .apply_change(
            &mut terminal,
            ScreenChange::Upsert(ScreenEntry::with_payload(
                "assistant-table",
                LiveEntryKind::Assistant,
                TranscriptPayload::AssistantMarkdown(content.to_string()),
                true,
            )),
        )
        .expect("completed table should commit");

    assert_eq!(screen.committed_entries()[0].height() as usize, live_rows);
    assert_eq!(screen.committed_count(), 1);
    let metadata = format!("{:?}", screen.committed_entries());
    assert!(!metadata.contains("Header"));
    assert!(!metadata.contains("┌"));
}

#[test]
fn completed_history_is_not_recommitted_or_rerendered_by_later_updates() {
    let mut screen = ScreenModel::default();
    let mut terminal = Terminal::with_options(TestBackend::new(40, 24), terminal_options())
        .expect("inline terminal should initialize");
    let entry = ScreenEntry::with_payload(
        "assistant-table",
        LiveEntryKind::Assistant,
        TranscriptPayload::AssistantMarkdown("| H | V |\n| --- | --- |\n| a | b |".to_string()),
        true,
    );
    screen
        .apply_change(&mut terminal, ScreenChange::Upsert(entry))
        .expect("completed table should commit");
    let committed_height = screen.committed_entries()[0].height();

    terminal
        .resize(ratatui::layout::Rect::new(0, 0, 80, 24))
        .expect("terminal should resize");
    screen
        .apply_change(
            &mut terminal,
            ScreenChange::Upsert(ScreenEntry::with_payload(
                "assistant-table",
                LiveEntryKind::Assistant,
                TranscriptPayload::AssistantMarkdown("late update".to_string()),
                true,
            )),
        )
        .expect("late update should be ignored");

    assert_eq!(screen.committed_count(), 1);
    assert_eq!(screen.committed_entries()[0].height(), committed_height);
}

fn updated_entry_id(change: &ScreenChange) -> Option<picopilot::screen_model::TranscriptEntryId> {
    match change {
        ScreenChange::Upsert(entry) => Some(entry.id().to_string()),
        ScreenChange::Reset | ScreenChange::Remove(_) => None,
    }
}

#[test]
fn main_screen_setup_does_not_enter_the_alternate_screen() {
    let mut output = Vec::new();

    enter_main_screen(&mut output).expect("main-screen setup should succeed");

    assert_eq!(output, b"\x1b[?2004h");
    assert!(!output.windows(8).any(|window| window == b"\x1b[?1049h"));
}

#[test]
fn inline_viewport_uses_the_measured_fixed_live_region() {
    assert_eq!(FIXED_LIVE_REGION_HEIGHT, 14);
    assert_eq!(
        terminal_options().viewport,
        Viewport::Inline(FIXED_LIVE_REGION_HEIGHT)
    );

    let mut terminal = Terminal::with_options(TestBackend::new(80, 24), terminal_options())
        .expect("inline terminal should initialize");
    assert_eq!(terminal.get_frame().area().height, FIXED_LIVE_REGION_HEIGHT);
}

#[test]
fn completing_a_live_entry_inserts_its_exact_lines_before_the_viewport() {
    let mut terminal = Terminal::with_options(TestBackend::new(80, 24), terminal_options())
        .expect("inline terminal should initialize");
    let mut screen = ScreenModel::default();
    screen
        .start_live(
            "message-1",
            LiveEntryKind::Assistant,
            vec![Line::from("first line"), Line::from("second line")],
        )
        .expect("live entry should start");

    screen
        .commit_live(&mut terminal, "message-1")
        .expect("live entry should commit");
    terminal
        .draw(|frame| screen.draw_live(frame, Platform::default()))
        .expect("live viewport should redraw");

    let buffer = terminal.backend().buffer();
    let rows = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert!(rows.iter().any(|row| row.contains("first line")));
    assert!(rows.iter().any(|row| row.contains("second line")));
    assert_eq!(screen.committed_count(), 1);
    assert!(screen.is_committed("message-1"));
}

#[test]
fn committed_entries_cannot_be_mutated_by_late_updates() {
    let mut terminal = Terminal::with_options(TestBackend::new(80, 24), terminal_options())
        .expect("inline terminal should initialize");
    let mut screen = ScreenModel::default();
    screen
        .start_live(
            "message-1",
            LiveEntryKind::Assistant,
            vec![Line::from("original")],
        )
        .expect("live entry should start");
    screen
        .commit_live(&mut terminal, "message-1")
        .expect("live entry should commit");

    assert!(!screen.update_live("message-1", vec![Line::from("late correction")]));
    assert_eq!(screen.committed_count(), 1);
    assert!(screen.is_committed("message-1"));
    assert!(screen
        .start_live(
            "message-1",
            LiveEntryKind::Assistant,
            vec![Line::from("late correction")],
        )
        .is_err());
}

#[test]
fn live_entries_can_update_until_completion() {
    let mut terminal = Terminal::with_options(TestBackend::new(80, 24), terminal_options())
        .expect("inline terminal should initialize");
    let mut screen = ScreenModel::default();
    screen
        .start_live("bash-1", LiveEntryKind::Bash, vec![Line::from("running")])
        .expect("live entry should start");

    assert!(screen.update_live("bash-1", vec![Line::from("progress 1")]));
    assert_eq!(
        screen.live_entries()[0].lines(),
        &[Line::from("progress 1")]
    );

    screen
        .commit_live(&mut terminal, "bash-1")
        .expect("live entry should commit");
    assert!(!screen.update_live("bash-1", vec![Line::from("progress 2")]));
}

#[test]
fn explicit_commit_cannot_bypass_the_front_of_the_queue() {
    let mut terminal = Terminal::with_options(TestBackend::new(80, 24), terminal_options())
        .expect("inline terminal should initialize");
    let mut screen = ScreenModel::default();
    screen
        .start_live("front", LiveEntryKind::Assistant, vec![Line::from("front")])
        .expect("front entry should start");
    screen
        .start_live("behind", LiveEntryKind::Other, vec![Line::from("behind")])
        .expect("behind entry should start");

    assert!(!screen
        .commit_live(&mut terminal, "behind")
        .expect("out-of-order commit should be rejected"));
    assert_eq!(screen.live_entries().len(), 2);
}

#[test]
fn live_lines_are_limited_to_the_available_viewport_rows() {
    let mut screen = ScreenModel::default();
    screen
        .start_live(
            "bash-1",
            LiveEntryKind::Bash,
            vec![
                Line::from("header"),
                Line::from("progress"),
                Line::from("status"),
            ],
        )
        .expect("live entry should start");

    assert_eq!(
        screen.visible_live_lines(Platform::default(), 2),
        vec![Line::from("header"), Line::from("progress")]
    );
}

#[test]
fn new_conversation_resets_screen_identity_and_commits_reused_entries() {
    let mut app = App::new(None);
    let mut screen = ScreenModel::default();
    let mut terminal = Terminal::with_options(TestBackend::new(80, 24), terminal_options())
        .expect("inline terminal should initialize");

    app.add_user_message("conversation A".to_string());
    app.apply(EventUpdate::AssistantMessage {
        message_id: "reused-message-id".to_string(),
        content: "response A".to_string(),
        agent_id: None,
    });
    apply_pending_changes(&mut app, &mut screen, &mut terminal);
    assert_eq!(screen.committed_count(), 2);

    app.reset_for_new_conversation();
    app.add_user_message("conversation B".to_string());
    app.apply(EventUpdate::AssistantMessage {
        message_id: "reused-message-id".to_string(),
        content: "response B".to_string(),
        agent_id: None,
    });
    apply_pending_changes(&mut app, &mut screen, &mut terminal);

    assert_eq!(screen.committed_count(), 2);
    assert!(terminal_text(&terminal).contains("conversation B"));
    assert!(terminal_text(&terminal).contains("response B"));
}

#[test]
fn resuming_a_session_resets_screen_metadata_and_renders_history() {
    let mut app = App::new(None);
    let mut screen = ScreenModel::default();
    let mut terminal = Terminal::with_options(TestBackend::new(80, 24), terminal_options())
        .expect("inline terminal should initialize");

    app.add_user_message("before resume".to_string());
    apply_pending_changes(&mut app, &mut screen, &mut terminal);

    app.replace_history(&[github_copilot_sdk::types::SessionEvent {
        id: "event-user".to_string(),
        timestamp: "2026-09-05T12:00:00Z".to_string(),
        parent_id: None,
        ephemeral: None,
        agent_id: None,
        debug_cli_received_at_ms: None,
        debug_ws_forwarded_at_ms: None,
        event_type: "user.message".to_string(),
        data: json!({ "content": "resumed history", "source": "user" }),
    }]);
    apply_pending_changes(&mut app, &mut screen, &mut terminal);

    assert_eq!(screen.committed_count(), 1);
    assert!(terminal_text(&terminal).contains("resumed history"));
}

#[test]
fn same_display_name_subagents_keep_separate_screen_entries() {
    let mut app = App::new(None);
    let mut screen = ScreenModel::default();
    let mut terminal = Terminal::with_options(TestBackend::new(100, 24), terminal_options())
        .expect("inline terminal should initialize");

    for agent_id in ["agent-1", "agent-2"] {
        app.apply(EventUpdate::SubagentStarted {
            name: "worker".to_string(),
            tool_call_id: format!("tool-{agent_id}"),
            display_name: "Worker".to_string(),
            agent_id: Some(agent_id.to_string()),
        });
    }
    for agent_id in ["agent-1", "agent-2"] {
        app.apply(EventUpdate::SubagentCompleted {
            name: "worker".to_string(),
            tool_call_id: format!("tool-{agent_id}"),
            agent_id: Some(agent_id.to_string()),
        });
    }
    apply_pending_changes(&mut app, &mut screen, &mut terminal);

    assert_eq!(screen.committed_count(), 2);
    assert!(terminal_text(&terminal).matches("Worker").count() >= 2);
}

#[test]
fn entry_ids_are_stable_for_updates_and_fresh_after_transition() {
    let mut app = App::new(None);
    app.add_user_message("first".to_string());
    let first_id = app
        .take_screen_changes()
        .iter()
        .find_map(updated_entry_id)
        .expect("user entry should have an ID");

    app.apply(EventUpdate::UserMessage {
        content: "first canonical".to_string(),
    });
    let updated_id = app
        .take_screen_changes()
        .iter()
        .find_map(updated_entry_id)
        .expect("updated user entry should have an ID");
    assert_eq!(first_id, updated_id);

    app.reset_for_new_conversation();
    app.add_user_message("second conversation".to_string());
    let second_id = app
        .take_screen_changes()
        .iter()
        .find_map(updated_entry_id)
        .expect("new user entry should have an ID");
    assert_ne!(first_id, second_id);
}

#[test]
fn updating_one_entry_emits_one_incremental_screen_change() {
    let mut app = App::new(None);
    app.add_user_message("first".to_string());
    app.apply(EventUpdate::AssistantMessage {
        message_id: "assistant-1".to_string(),
        content: "response".to_string(),
        agent_id: None,
    });
    let _ = app.take_screen_changes();

    app.apply(EventUpdate::AssistantMessage {
        message_id: "assistant-1".to_string(),
        content: "corrected response".to_string(),
        agent_id: None,
    });
    let changes = app.take_screen_changes();

    assert_eq!(changes.len(), 1);
    assert!(matches!(changes[0], ScreenChange::Upsert(_)));
}

#[test]
fn later_completed_entries_wait_for_earlier_live_entries() {
    let mut app = App::new(None);
    let mut screen = ScreenModel::default();
    let mut terminal = Terminal::with_options(TestBackend::new(100, 24), terminal_options())
        .expect("inline terminal should initialize");

    app.apply(EventUpdate::AssistantDelta {
        message_id: "assistant-1".to_string(),
        content: "assistant live".to_string(),
        agent_id: None,
    });
    app.apply(EventUpdate::ToolStarted {
        tool_call_id: "tool-1".to_string(),
        tool_name: "bash".to_string(),
        arguments: Some(json!({ "command": "echo done" })),
        agent_id: None,
    });
    apply_pending_changes(&mut app, &mut screen, &mut terminal);

    app.apply(EventUpdate::ToolCompleted {
        tool_call_id: "tool-1".to_string(),
        success: true,
        message: Some("tool completed".to_string()),
        agent_id: None,
    });
    apply_pending_changes(&mut app, &mut screen, &mut terminal);
    assert_eq!(screen.committed_count(), 0);

    app.apply(EventUpdate::AssistantMessage {
        message_id: "assistant-1".to_string(),
        content: "assistant final".to_string(),
        agent_id: None,
    });
    apply_pending_changes(&mut app, &mut screen, &mut terminal);

    let rendered = terminal_text(&terminal);
    assert_eq!(screen.committed_count(), 3);
    assert!(rendered.find("assistant final") < rendered.find("tool completed"));
}

#[test]
fn tool_result_stays_after_messages_received_between_start_and_completion() {
    let mut app = App::new(None);
    let mut screen = ScreenModel::default();
    let mut terminal = Terminal::with_options(TestBackend::new(100, 24), terminal_options())
        .expect("inline terminal should initialize");

    app.apply(EventUpdate::ToolStarted {
        tool_call_id: "tool-1".to_string(),
        tool_name: "edit".to_string(),
        arguments: Some(json!({"file_path": "src/main.rs", "old_string": "a"})),
        agent_id: None,
    });
    app.apply(EventUpdate::AssistantMessage {
        message_id: "assistant-1".to_string(),
        content: "I will inspect the result next.".to_string(),
        agent_id: None,
    });

    let header = app
        .entries()
        .iter()
        .find(|entry| matches!(entry, ChatEntry::Tool { .. }))
        .expect("tool header should be retained");
    assert!(matches!(
        header,
        ChatEntry::Tool {
            tool_call_id,
            arguments: Some(arguments),
            ..
        } if tool_call_id == "tool-1" && arguments["file_path"] == "src/main.rs"
    ));

    apply_pending_changes(&mut app, &mut screen, &mut terminal);
    app.apply(EventUpdate::ToolCompleted {
        tool_call_id: "tool-1".to_string(),
        success: true,
        message: Some("updated".to_string()),
        agent_id: None,
    });
    apply_pending_changes(&mut app, &mut screen, &mut terminal);

    let rendered = terminal_text(&terminal);
    let header_position = rendered.find("Edit(src/main.rs)").expect("tool header");
    let assistant_position = rendered
        .find("I will inspect the result next.")
        .expect("interleaved assistant message");
    let result_position = rendered.find("updated").expect("tool result");
    assert!(header_position < assistant_position);
    assert!(assistant_position < result_position);
}

#[test]
fn overlapping_tools_keep_reverse_completion_order() {
    let mut app = App::new(None);
    let mut screen = ScreenModel::default();
    let mut terminal = Terminal::with_options(TestBackend::new(100, 24), terminal_options())
        .expect("inline terminal should initialize");

    for tool_call_id in ["tool-a", "tool-b"] {
        app.apply(EventUpdate::ToolStarted {
            tool_call_id: tool_call_id.to_string(),
            tool_name: "bash".to_string(),
            arguments: Some(json!({"command": format!("echo {tool_call_id}")})),
            agent_id: None,
        });
    }
    apply_pending_changes(&mut app, &mut screen, &mut terminal);

    app.apply(EventUpdate::ToolCompleted {
        tool_call_id: "tool-b".to_string(),
        success: true,
        message: Some("result b".to_string()),
        agent_id: None,
    });
    apply_pending_changes(&mut app, &mut screen, &mut terminal);
    app.apply(EventUpdate::ToolCompleted {
        tool_call_id: "tool-a".to_string(),
        success: true,
        message: Some("result a".to_string()),
        agent_id: None,
    });
    apply_pending_changes(&mut app, &mut screen, &mut terminal);

    let rendered = terminal_text(&terminal);
    let header_a = rendered.find("Bash(echo tool-a)").expect("tool a header");
    let header_b = rendered.find("Bash(echo tool-b)").expect("tool b header");
    let result_b = rendered.find("result b").expect("tool b result");
    let result_a = rendered.find("result a").expect("tool a result");
    assert!(header_a < header_b);
    assert!(header_b < result_b);
    assert!(result_b < result_a);
}

#[test]
fn tool_progress_is_call_scoped_and_late_completion_is_ignored() {
    let mut app = App::new(None);
    app.apply(EventUpdate::ToolStarted {
        tool_call_id: "tool-a".to_string(),
        tool_name: "grep".to_string(),
        arguments: Some(json!({"pattern": "alpha"})),
        agent_id: None,
    });
    app.apply(EventUpdate::ToolStarted {
        tool_call_id: "tool-b".to_string(),
        tool_name: "glob".to_string(),
        arguments: Some(json!({"pattern": "*.rs"})),
        agent_id: None,
    });

    app.apply(EventUpdate::ToolOutput {
        tool_call_id: "tool-a".to_string(),
        content: "a output".to_string(),
        agent_id: None,
    });
    app.apply(EventUpdate::ToolProgress {
        tool_call_id: "tool-b".to_string(),
        content: "b progress".to_string(),
        agent_id: None,
    });
    app.apply(EventUpdate::ToolProgress {
        tool_call_id: "tool-a".to_string(),
        content: "a status".to_string(),
        agent_id: None,
    });
    app.apply(EventUpdate::ToolOutput {
        tool_call_id: "tool-a".to_string(),
        content: " tail".to_string(),
        agent_id: None,
    });

    let progress = app
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            ChatEntry::ToolProgress {
                tool_call_id,
                content,
                ..
            } => Some((tool_call_id.as_str(), content.as_str())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        progress,
        vec![("tool-a", "a status tail"), ("tool-b", "b progress")]
    );

    app.apply(EventUpdate::ToolCompleted {
        tool_call_id: "tool-a".to_string(),
        success: true,
        message: Some("first result".to_string()),
        agent_id: None,
    });
    app.apply(EventUpdate::ToolOutput {
        tool_call_id: "tool-a".to_string(),
        content: "late output".to_string(),
        agent_id: None,
    });
    app.apply(EventUpdate::ToolCompleted {
        tool_call_id: "tool-a".to_string(),
        success: true,
        message: Some("duplicate result".to_string()),
        agent_id: None,
    });

    let results = app
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            ChatEntry::ToolResult {
                tool_call_id,
                content,
                ..
            } => Some((tool_call_id.as_str(), content.as_str())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(results, vec![("tool-a", "first result")]);
    assert!(!app.entries().iter().any(|entry| {
        matches!(
            entry,
            ChatEntry::ToolProgress { tool_call_id, content, .. }
                if tool_call_id == "tool-a" && content.contains("late output")
        )
    }));
}

#[test]
fn tool_header_and_progress_ids_stay_stable_until_result_is_created() {
    let mut app = App::new(None);
    app.apply(EventUpdate::ToolStarted {
        tool_call_id: "tool-1".to_string(),
        tool_name: "read".to_string(),
        arguments: Some(json!({"file_path": "README.md"})),
        agent_id: None,
    });

    let start_changes = app.take_screen_changes();
    let header_id = match &start_changes[0] {
        ScreenChange::Upsert(entry) => entry.id().to_string(),
        _ => panic!("tool start should create a header update"),
    };
    let progress_id = match &start_changes[1] {
        ScreenChange::Upsert(entry) => entry.id().to_string(),
        _ => panic!("tool start should create a progress update"),
    };

    app.apply(EventUpdate::ToolProgress {
        tool_call_id: "tool-1".to_string(),
        content: "reading".to_string(),
        agent_id: None,
    });
    let progress_update = app.take_screen_changes();
    assert!(matches!(
        progress_update.as_slice(),
        [ScreenChange::Upsert(entry)] if entry.id() == progress_id
    ));

    app.apply(EventUpdate::ToolCompleted {
        tool_call_id: "tool-1".to_string(),
        success: true,
        message: Some("read it".to_string()),
        agent_id: None,
    });
    let completion_changes = app.take_screen_changes();
    assert!(matches!(
        completion_changes.as_slice(),
        [
            ScreenChange::Upsert(header),
            ScreenChange::Remove(progress),
            ScreenChange::Upsert(result),
        ] if header.id() == header_id
            && progress == &progress_id
            && result.id() != header_id
            && result.id() != progress_id
    ));
}

#[test]
fn conversation_reset_clears_unresolved_tool_presentation_state() {
    let mut app = App::new(None);
    app.apply(EventUpdate::ToolStarted {
        tool_call_id: "tool-1".to_string(),
        tool_name: "glob".to_string(),
        arguments: Some(json!({"pattern": "*.rs"})),
        agent_id: None,
    });
    let _ = app.take_screen_changes();

    app.reset_for_new_conversation();
    assert!(app.entries().is_empty());
    assert!(matches!(
        app.take_screen_changes().as_slice(),
        [ScreenChange::Reset]
    ));

    app.apply(EventUpdate::ToolCompleted {
        tool_call_id: "tool-1".to_string(),
        success: true,
        message: Some("late result".to_string()),
        agent_id: None,
    });
    assert!(app.take_screen_changes().is_empty());
}

fn test_tool_header(
    tool_name: &str,
    arguments: serde_json::Value,
    state: ToolCallState,
    agent_id: Option<&str>,
) -> ToolHeaderPayload {
    ToolHeaderPayload {
        tool_call_id: "tool-test".to_string(),
        tool_name: tool_name.to_string(),
        arguments: Some(arguments),
        agent_id: agent_id.map(ToString::to_string),
        started_at: std::time::Instant::now(),
        state,
        cwd: std::path::PathBuf::from("/workspace"),
    }
}

fn test_tool_result(
    tool_name: &str,
    content: &str,
    state: ToolResultState,
    agent_id: Option<&str>,
) -> ToolResultPayload {
    ToolResultPayload {
        tool_call_id: "tool-test".to_string(),
        tool_name: tool_name.to_string(),
        arguments: None,
        content: content.to_string(),
        state,
        agent_id: agent_id.map(ToString::to_string),
        cwd: std::path::PathBuf::from("/workspace"),
    }
}

#[test]
fn tool_headers_use_platform_glyphs_states_and_a_shared_blink_phase() {
    let queued = TranscriptPayload::ToolHeader(test_tool_header(
        "read",
        json!({"file_path": "README.md"}),
        ToolCallState::Queued,
        None,
    ));
    let running = TranscriptPayload::ToolHeader(test_tool_header(
        "read",
        json!({"file_path": "README.md"}),
        ToolCallState::Running,
        None,
    ));
    let success = TranscriptPayload::ToolHeader(test_tool_header(
        "read",
        json!({"file_path": "README.md"}),
        ToolCallState::Success,
        None,
    ));
    let error = TranscriptPayload::ToolHeader(test_tool_header(
        "read",
        json!({"file_path": "README.md"}),
        ToolCallState::Error,
        None,
    ));

    let queued_lines = render_transcript_payload_with_clock(
        LiveEntryKind::Tool,
        &queued,
        80,
        ToolPlatform::WindowsLinux,
        0,
    );
    assert_eq!(queued_lines[1].to_string(), "● Read(README.md)");

    let running_visible = render_transcript_payload_with_clock(
        LiveEntryKind::Tool,
        &running,
        80,
        ToolPlatform::WindowsLinux,
        599,
    );
    let running_hidden = render_transcript_payload_with_clock(
        LiveEntryKind::Tool,
        &running,
        80,
        ToolPlatform::WindowsLinux,
        600,
    );
    assert_eq!(running_visible[1].to_string(), "● Read(README.md)");
    assert_eq!(running_hidden[1].to_string(), "  Read(README.md)");

    let mac = render_transcript_payload_with_clock(
        LiveEntryKind::Tool,
        &running,
        80,
        ToolPlatform::MacOs,
        0,
    );
    assert_eq!(mac[1].to_string(), "⏺ Read(README.md)");

    let success_lines = render_transcript_payload_with_clock(
        LiveEntryKind::Tool,
        &success,
        80,
        ToolPlatform::WindowsLinux,
        600,
    );
    let error_lines = render_transcript_payload_with_clock(
        LiveEntryKind::Tool,
        &error,
        80,
        ToolPlatform::WindowsLinux,
        600,
    );
    assert_eq!(success_lines[1].to_string(), "● Read(README.md)");
    assert_eq!(error_lines[1].to_string(), "● Read(README.md)");
    assert_eq!(success_lines[1].spans[0].style.fg, Some(palette::SUCCESS));
    assert_eq!(error_lines[1].spans[0].style.fg, Some(palette::ERROR));

    let second_running = TranscriptPayload::ToolHeader(test_tool_header(
        "glob",
        json!({"pattern": "*.rs"}),
        ToolCallState::Running,
        None,
    ));
    let first_phase = render_transcript_payload_with_clock(
        LiveEntryKind::Tool,
        &running,
        80,
        ToolPlatform::WindowsLinux,
        600,
    );
    let second_phase = render_transcript_payload_with_clock(
        LiveEntryKind::Tool,
        &second_running,
        80,
        ToolPlatform::WindowsLinux,
        600,
    );
    assert_eq!(first_phase[1].to_string().chars().next(), Some(' '));
    assert_eq!(second_phase[1].to_string().chars().next(), Some(' '));
}

#[test]
fn tool_headers_omit_empty_parentheses_and_nested_spacing() {
    let empty_summary = TranscriptPayload::ToolHeader(test_tool_header(
        "custom_tool",
        json!(null),
        ToolCallState::Running,
        None,
    ));
    let nested = TranscriptPayload::ToolHeader(test_tool_header(
        "read",
        json!({"file_path": "README.md"}),
        ToolCallState::Running,
        Some("agent-1"),
    ));
    let hidden_name = TranscriptPayload::ToolHeader(test_tool_header(
        "",
        json!({"value": "ignored"}),
        ToolCallState::Running,
        None,
    ));

    let empty_lines = render_transcript_payload_with_clock(
        LiveEntryKind::Tool,
        &empty_summary,
        80,
        ToolPlatform::WindowsLinux,
        0,
    );
    let nested_lines = render_transcript_payload_with_clock(
        LiveEntryKind::ToolNested,
        &nested,
        80,
        ToolPlatform::WindowsLinux,
        0,
    );
    let hidden_lines = render_transcript_payload_with_clock(
        LiveEntryKind::Tool,
        &hidden_name,
        80,
        ToolPlatform::WindowsLinux,
        0,
    );

    assert_eq!(empty_lines[1].to_string(), "● Custom Tool");
    assert_eq!(nested_lines.len(), 1);
    assert_eq!(nested_lines[0].to_string(), "● Read(README.md)");
    assert!(hidden_lines.is_empty());
}

#[test]
fn top_level_tool_progress_follows_its_header_without_a_separator() {
    let mut app = App::new(None);
    let mut screen = ScreenModel::default();
    let mut terminal = Terminal::with_options(TestBackend::new(80, 24), terminal_options())
        .expect("inline terminal should initialize");

    app.apply(EventUpdate::ToolStarted {
        tool_call_id: "tool-progress".to_string(),
        tool_name: "read".to_string(),
        arguments: Some(json!({"file_path": "README.md"})),
        agent_id: None,
    });
    app.apply(EventUpdate::ToolProgress {
        tool_call_id: "tool-progress".to_string(),
        content: "reading".to_string(),
        agent_id: None,
    });
    apply_pending_changes(&mut app, &mut screen, &mut terminal);

    let lines = screen
        .live_lines_at_width(
            Platform {
                is_windows: false,
                wt_session: false,
            },
            80,
        )
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    assert_eq!(
        lines,
        vec![
            "".to_string(),
            "● Read(README.md)".to_string(),
            "  ⎿ \u{00a0}reading".to_string(),
        ]
    );
}

#[test]
fn model_only_tool_progress_states_have_deterministic_single_rows() {
    let permission = TranscriptPayload::ToolProgress(ToolProgressPayload {
        tool_call_id: "permission-1".to_string(),
        tool_name: "read".to_string(),
        content: String::new(),
        kind: ToolProgressKind::Permission,
        agent_id: None,
    });
    let classifier = TranscriptPayload::ToolProgress(ToolProgressPayload {
        tool_call_id: "classifier-1".to_string(),
        tool_name: "read".to_string(),
        content: String::new(),
        kind: ToolProgressKind::Classifier,
        agent_id: None,
    });
    let nested_classifier = TranscriptPayload::ToolProgress(ToolProgressPayload {
        tool_call_id: "classifier-2".to_string(),
        tool_name: "read".to_string(),
        content: "classifying".to_string(),
        kind: ToolProgressKind::Classifier,
        agent_id: Some("agent-1".to_string()),
    });

    let permission_lines = render_transcript_payload_with_clock(
        LiveEntryKind::Tool,
        &permission,
        40,
        ToolPlatform::WindowsLinux,
        0,
    );
    let classifier_lines = render_transcript_payload_with_clock(
        LiveEntryKind::Tool,
        &classifier,
        40,
        ToolPlatform::WindowsLinux,
        0,
    );
    let nested_classifier_lines = render_transcript_payload_with_clock(
        LiveEntryKind::ToolNested,
        &nested_classifier,
        40,
        ToolPlatform::WindowsLinux,
        0,
    );

    assert_eq!(
        permission_lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec!["  ⎿ \u{00a0}Waiting for permission…".to_string()]
    );
    assert!(classifier_lines.is_empty());
    assert_eq!(nested_classifier_lines.len(), 1);
    assert_eq!(nested_classifier_lines[0].to_string(), "classifying");
}

#[test]
fn tool_progress_is_one_clipped_row_even_with_long_multiline_content() {
    let payload = TranscriptPayload::ToolProgress(ToolProgressPayload {
        tool_call_id: "progress-1".to_string(),
        tool_name: "read".to_string(),
        content: "0123456789abcdef\nsecond line".to_string(),
        kind: ToolProgressKind::Tool,
        agent_id: None,
    });

    let lines = render_transcript_payload_with_clock(
        LiveEntryKind::Tool,
        &payload,
        12,
        ToolPlatform::WindowsLinux,
        0,
    );

    assert_eq!(lines.len(), 1);
    assert!(lines[0].width() <= 12);
    assert!(!lines[0].to_string().contains('\n'));
    assert!(lines[0].to_string().starts_with("  ⎿ \u{00a0}"));
}

#[test]
fn nested_tool_progress_is_one_clipped_row_without_a_gutter() {
    let payload = TranscriptPayload::ToolProgress(ToolProgressPayload {
        tool_call_id: "progress-nested".to_string(),
        tool_name: "read".to_string(),
        content: "0123456789abcdef\nsecond line".to_string(),
        kind: ToolProgressKind::Tool,
        agent_id: Some("agent-1".to_string()),
    });

    let lines = render_transcript_payload_with_clock(
        LiveEntryKind::ToolNested,
        &payload,
        12,
        ToolPlatform::WindowsLinux,
        0,
    );

    assert_eq!(lines.len(), 1);
    assert!(lines[0].width() <= 12);
    assert!(!lines[0].to_string().contains('\n'));
    assert!(!lines[0].to_string().contains('⎿'));
}

#[test]
fn two_line_bash_summaries_use_two_physical_header_rows() {
    let payload = TranscriptPayload::ToolHeader(test_tool_header(
        "bash",
        json!({"command": "echo first\necho second"}),
        ToolCallState::Running,
        None,
    ));

    let lines = render_transcript_payload_with_clock(
        LiveEntryKind::Tool,
        &payload,
        80,
        ToolPlatform::WindowsLinux,
        0,
    );
    let rendered = lines.iter().map(ToString::to_string).collect::<Vec<_>>();

    assert_eq!(rendered.len(), 3);
    assert_eq!(rendered[1], "● Bash(echo first");
    assert_eq!(rendered[2], "  echo second)");
    assert!(rendered.iter().all(|line| !line.contains('\n')));
}

#[test]
fn narrow_two_line_bash_summaries_clip_each_row_without_breaking_parentheses() {
    let payload = TranscriptPayload::ToolHeader(test_tool_header(
        "bash",
        json!({"command": "echo first command\necho second command"}),
        ToolCallState::Running,
        None,
    ));

    for width in [10, 12, 20] {
        let lines = render_transcript_payload_with_clock(
            LiveEntryKind::Tool,
            &payload,
            width,
            ToolPlatform::WindowsLinux,
            0,
        );
        let first = lines[1].to_string();
        let second = lines[2].to_string();

        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| line.width() <= width));
        assert!(lines.iter().all(|line| !line.to_string().contains('\n')));
        assert!(first.starts_with("● Bash("));
        assert!(second.starts_with("  "));
        assert_eq!(
            first.chars().filter(|character| *character == '(').count(),
            1
        );
        assert_eq!(
            second.chars().filter(|character| *character == ')').count(),
            1
        );
        assert!(second.ends_with(')'));
    }
}

#[test]
fn tool_surfaces_fit_the_required_focus_widths() {
    let header = TranscriptPayload::ToolHeader(test_tool_header(
        "read",
        json!({"file_path": "/workspace/a/very/long/path/to/a/file.rs"}),
        ToolCallState::Running,
        None,
    ));
    let result = TranscriptPayload::ToolResult(test_tool_result(
        "read",
        "first line with enough text to wrap\nsecond line\nthird line",
        ToolResultState::Success,
        None,
    ));

    for width in [10, 40, 80] {
        for payload in [&header, &result] {
            let lines = render_transcript_payload_with_clock(
                LiveEntryKind::Tool,
                payload,
                width,
                ToolPlatform::WindowsLinux,
                0,
            );
            assert!(lines.iter().all(|line| line.width() <= width));
        }
    }
}

#[test]
fn tool_header_truncates_without_wrapping_at_narrow_widths() {
    let payload = TranscriptPayload::ToolHeader(test_tool_header(
        "bash",
        json!({"command": "echo a very long command that does not fit"}),
        ToolCallState::Running,
        None,
    ));
    let lines = render_transcript_payload_with_clock(
        LiveEntryKind::Tool,
        &payload,
        10,
        ToolPlatform::WindowsLinux,
        0,
    );

    assert_eq!(lines.len(), 2);
    assert!(lines[1].width() <= 10);
    assert!(!lines[1].to_string().contains('\n'));
}

#[test]
fn tool_result_uses_the_five_cell_gutter_and_nested_results_do_not_stack_it() {
    let result = TranscriptPayload::ToolResult(test_tool_result(
        "custom_tool",
        "first\n\nthird",
        ToolResultState::Success,
        None,
    ));
    let nested = TranscriptPayload::ToolResult(test_tool_result(
        "custom_tool",
        "nested result",
        ToolResultState::Success,
        Some("agent-1"),
    ));
    let lines = render_transcript_payload_with_clock(
        LiveEntryKind::Tool,
        &result,
        40,
        ToolPlatform::WindowsLinux,
        0,
    );
    let nested_lines = render_transcript_payload_with_clock(
        LiveEntryKind::ToolNested,
        &nested,
        40,
        ToolPlatform::WindowsLinux,
        0,
    );

    assert_eq!(lines[1].to_string(), "  ⎿ \u{00a0}first");
    assert_eq!(lines[2].to_string(), "     ");
    assert_eq!(lines[3].to_string(), "     third");
    assert!(!lines[2].to_string().contains('⎿'));
    assert_eq!(nested_lines.len(), 1);
    assert_eq!(nested_lines[0].to_string(), "nested result");
}

#[test]
fn tool_cancellation_is_a_single_dim_message() {
    let payload = TranscriptPayload::ToolResult(test_tool_result(
        "custom_tool",
        "ignored",
        ToolResultState::Cancelled,
        None,
    ));
    let lines = render_transcript_payload_with_clock(
        LiveEntryKind::Tool,
        &payload,
        80,
        ToolPlatform::WindowsLinux,
        0,
    );

    assert_eq!(
        lines.iter().map(ToString::to_string).collect::<Vec<_>>(),
        vec![
            "".to_string(),
            "  ⎿ \u{00a0}Interrupted · What should Claude do instead?".to_string(),
        ]
    );
    assert!(lines[1].spans[0].style.add_modifier == ratatui::style::Modifier::DIM);
}

#[test]
fn tool_errors_normalize_wrappers_and_input_validation() {
    let input_validation = TranscriptPayload::ToolResult(test_tool_result(
        "custom_tool",
        "<tool_use_error>InputValidationError: missing path</tool_use_error>",
        ToolResultState::Error,
        None,
    ));
    let existing_prefix = TranscriptPayload::ToolResult(test_tool_result(
        "custom_tool",
        "<error>Error: already normalized</error>",
        ToolResultState::Error,
        None,
    ));
    let ordinary = TranscriptPayload::ToolResult(test_tool_result(
        "custom_tool",
        "<error>something failed</error>",
        ToolResultState::Error,
        None,
    ));

    let render = |payload: &TranscriptPayload| {
        render_transcript_payload_with_options(
            LiveEntryKind::Tool,
            payload,
            80,
            ToolPlatform::WindowsLinux,
            0,
            false,
        )
    };

    assert!(render(&input_validation)
        .iter()
        .any(|line| line.to_string().contains("Invalid tool parameters")));
    assert!(render(&existing_prefix)
        .iter()
        .any(|line| line.to_string().contains("Error: already normalized")));
    assert!(render(&ordinary)
        .iter()
        .any(|line| line.to_string().contains("Error: something failed")));
}

#[test]
fn tool_errors_and_write_results_use_their_ten_line_markers() {
    let eleven_lines = (1..=11)
        .map(|line| format!("error line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let error = TranscriptPayload::ToolResult(test_tool_result(
        "custom_tool",
        &eleven_lines,
        ToolResultState::Error,
        None,
    ));
    let twelve_lines = (1..=12)
        .map(|line| format!("content line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let write = TranscriptPayload::ToolResult(test_tool_result(
        "write",
        &twelve_lines,
        ToolResultState::Success,
        None,
    ));

    let render = |payload: &TranscriptPayload, verbose| {
        render_transcript_payload_with_options(
            LiveEntryKind::Tool,
            payload,
            80,
            ToolPlatform::WindowsLinux,
            0,
            verbose,
        )
    };
    let error_lines = render(&error, false);
    let verbose_error_lines = render(&error, true);
    let write_lines = render(&write, false);

    assert!(error_lines
        .iter()
        .any(|line| line.to_string().contains("… +1 line (ctrl+o to see all)")));
    assert!(error_lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .any(|span| span.content == "ctrl+o"
            && span
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD)));
    assert!(!verbose_error_lines
        .iter()
        .any(|line| line.to_string().contains("ctrl+o to see all")));
    assert!(verbose_error_lines
        .iter()
        .any(|line| line.to_string().contains("error line 11")));
    assert!(write_lines
        .iter()
        .any(|line| line.to_string().contains("… +2 lines (ctrl+o to expand)")));
}

#[test]
fn removing_front_live_entry_commits_completed_entries_behind_it() {
    let mut terminal = Terminal::with_options(TestBackend::new(80, 24), terminal_options())
        .expect("inline terminal should initialize");
    let mut screen = ScreenModel::default();

    screen
        .apply_change(
            &mut terminal,
            ScreenChange::Upsert(ScreenEntry::new(
                "live-front",
                LiveEntryKind::Assistant,
                vec![Line::from("still running")],
                false,
            )),
        )
        .expect("live entry should apply");
    screen
        .apply_change(
            &mut terminal,
            ScreenChange::Upsert(ScreenEntry::new(
                "completed-behind",
                LiveEntryKind::Other,
                vec![Line::from("completed")],
                true,
            )),
        )
        .expect("completed entry should apply");

    assert_eq!(screen.committed_count(), 0);

    screen
        .apply_change(
            &mut terminal,
            ScreenChange::Remove("live-front".to_string()),
        )
        .expect("front entry should be removed");

    assert_eq!(screen.committed_count(), 1);
    assert!(screen.is_committed("completed-behind"));
    assert!(terminal_text(&terminal).contains("completed"));
}

#[test]
fn committed_long_lines_need_wrapped_height_before_insert() {
    let mut terminal = Terminal::with_options(TestBackend::new(10, 24), terminal_options())
        .expect("inline terminal should initialize");
    let mut screen = ScreenModel::default();
    screen
        .start_live(
            "long-line",
            LiveEntryKind::Assistant,
            vec![Line::from("0123456789ABCDEFGHIJ")],
        )
        .expect("live entry should start");

    screen
        .commit_live(&mut terminal, "long-line")
        .expect("live entry should commit");
    terminal
        .draw(|frame| screen.draw_live(frame, Platform::default()))
        .expect("live viewport should redraw");

    assert_eq!(screen.committed_entries()[0].height() as usize, 4);
    assert!(terminal_text(&terminal).contains("012345"));
    assert!(terminal_text(&terminal).contains("01234567"));
    assert!(terminal_text(&terminal).contains("89ABCDEF"));
    assert!(terminal_text(&terminal).contains("GHIJ"));
}

#[test]
fn transcript_visual_buffers_hold_surface_shapes_at_focus_widths() {
    let body = vec![Line::from("alpha beta gamma")];
    let user_fill = Style::default().bg(Color::Rgb(55, 55, 55));
    let assistant_dot = if cfg!(target_os = "macos") {
        "⏺ "
    } else {
        "● "
    };

    for width in [1, 2, 10, 40, 80] {
        let user_lines = render_entry_lines(LiveEntryKind::User, &body, width);
        let assistant_lines = render_entry_lines(LiveEntryKind::Assistant, &body, width);

        assert!(!user_lines.is_empty());
        assert!(!assistant_lines.is_empty());
        assert_eq!(user_lines[0], Line::default());
        assert_eq!(assistant_lines[0], Line::default());
        if width >= 3 {
            assert_eq!(user_lines[1].width(), width);
            assert!(user_lines[1].to_string().starts_with("❯ "));
            assert!(assistant_lines[1].to_string().starts_with(assistant_dot));
            assert!(user_lines[1]
                .spans
                .iter()
                .any(|span| span.style == user_fill));
        } else {
            assert!(user_lines[1].width() >= width);
            assert!(assistant_lines[1].width() >= width);
        }

        let mut terminal =
            Terminal::new(TestBackend::new(width as u16, 20)).expect("test terminal");
        terminal
            .draw(|frame| {
                frame.render_widget(Paragraph::new(user_lines.clone()), frame.area());
                frame.render_widget(Paragraph::new(assistant_lines.clone()), frame.area());
            })
            .expect("surface should render at the focused width");
    }
}

#[test]
fn assistant_body_uses_the_full_width_after_its_two_cell_gutter() {
    let lines = render_entry_lines(LiveEntryKind::Assistant, &[Line::from("12345678")], 10);
    let expected_prefix = if cfg!(target_os = "macos") {
        "⏺ "
    } else {
        "● "
    };

    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1].to_string(), format!("{expected_prefix}12345678"));
    assert_eq!(lines[1].width(), 10);
}

#[test]
fn transcript_width_arithmetic_is_explicit_at_ten_and_eighty_columns() {
    for width in [10, 80] {
        let body = vec![Line::from("x".repeat(width * 2))];
        let user_lines = render_entry_lines(LiveEntryKind::User, &body, width);
        let assistant_lines = render_entry_lines(LiveEntryKind::Assistant, &body, width);

        assert_eq!(user_lines[0], Line::default());
        assert_eq!(user_lines[1].width(), width);
        assert_eq!(
            user_lines[1]
                .to_string()
                .chars()
                .filter(|character| *character == 'x')
                .count(),
            width - 3
        );
        assert_eq!(
            user_lines[2]
                .to_string()
                .chars()
                .filter(|character| *character == 'x')
                .count(),
            width - 1
        );
        assert!(user_lines[1]
            .spans
            .iter()
            .any(|span| span.style.bg == Some(palette::USER_MESSAGE_BACKGROUND)));
        assert_eq!(user_lines[0].style, Style::default());

        assert_eq!(assistant_lines[1].width(), width);
        assert_eq!(assistant_lines[2].width(), width);
        assert_eq!(
            assistant_lines[1]
                .to_string()
                .chars()
                .filter(|character| *character == 'x')
                .count(),
            width - 2
        );
        assert_eq!(
            assistant_lines[2]
                .to_string()
                .chars()
                .filter(|character| *character == 'x')
                .count(),
            width - 2
        );

        let mut terminal = Terminal::new(TestBackend::new(width as u16, 4))
            .expect("test terminal should initialize");
        terminal
            .draw(|frame| {
                frame.render_widget(Paragraph::new(user_lines.clone()), frame.area());
            })
            .expect("prewrapped user lines should render");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .take(width * user_lines.len())
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains(&user_lines[1].to_string()));
        assert!(rendered.contains(&user_lines[2].to_string()));
    }
}

#[test]
fn prewrapped_messages_keep_their_surface_shape_across_required_widths() {
    for width in [1, 2, 10, 40, 80] {
        let short_user = render_entry_lines(LiveEntryKind::User, &[Line::from("short")], width);
        let wrapped_user = render_entry_lines(
            LiveEntryKind::User,
            &[Line::from("alpha beta gamma")],
            width,
        );
        let hard_newline_user = render_entry_lines(
            LiveEntryKind::User,
            &[Line::from("first"), Line::from("second")],
            width,
        );
        let assistant = render_entry_lines(
            LiveEntryKind::Assistant,
            &[Line::from("alpha beta gamma")],
            width,
        );

        assert!(!short_user.is_empty());
        assert!(!wrapped_user.is_empty());
        assert!(!hard_newline_user.is_empty());
        assert!(!assistant.is_empty());
        assert_eq!(short_user[0], Line::default());
        assert_eq!(hard_newline_user[0], Line::default());
        let hard_newline_text = hard_newline_user
            .iter()
            .skip(1)
            .map(ToString::to_string)
            .collect::<String>();
        assert!(hard_newline_text.contains('s'));
        if width >= 10 {
            assert!(hard_newline_user
                .iter()
                .skip(1)
                .any(|line| line.to_string().contains("second")));
        }
        assert!(assistant
            .iter()
            .skip(2)
            .all(|line| line.to_string().starts_with("  ")));

        let mut terminal = Terminal::new(TestBackend::new(width as u16, 20))
            .expect("test terminal should initialize");
        terminal
            .draw(|frame| {
                frame.render_widget(Paragraph::new(short_user.clone()), frame.area());
                frame.render_widget(Paragraph::new(assistant.clone()), frame.area());
            })
            .expect("prewrapped rows should render at every required width");
    }
}

#[test]
fn consecutive_message_surfaces_have_one_separator_each() {
    let user = render_entry_lines(LiveEntryKind::User, &[Line::from("one")], 10);
    let second_user = render_entry_lines(LiveEntryKind::User, &[Line::from("two")], 10);
    let assistant = render_entry_lines(LiveEntryKind::Assistant, &[Line::from("one")], 10);
    let second_assistant = render_entry_lines(LiveEntryKind::Assistant, &[Line::from("two")], 10);

    let mut users = user.clone();
    users.extend(second_user);
    assert_eq!(
        users
            .iter()
            .filter(|line| line.to_string().is_empty())
            .count(),
        2
    );

    let mut assistants = assistant.clone();
    assistants.extend(second_assistant);
    assert_eq!(
        assistants
            .iter()
            .filter(|line| line.to_string().is_empty())
            .count(),
        2
    );

    let mut mixed = user;
    mixed.extend(render_entry_lines(
        LiveEntryKind::Assistant,
        &[Line::from("response")],
        10,
    ));
    assert_eq!(
        mixed
            .iter()
            .filter(|line| line.to_string().is_empty())
            .count(),
        2
    );
}

#[test]
fn nested_assistant_content_has_no_glyph_or_top_level_separator() {
    let lines = render_entry_lines(
        LiveEntryKind::AssistantNested,
        &[Line::from("nested output")],
        40,
    );

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].to_string(), "nested output");
}

#[test]
fn wide_graphemes_are_retained_when_the_body_capacity_is_smaller() {
    for kind in [LiveEntryKind::User, LiveEntryKind::Assistant] {
        let lines = render_entry_lines(kind, &[Line::from("界")], 2);
        assert!(lines.iter().any(|line| line.to_string().contains('界')));
    }
}

#[test]
fn committed_and_live_rendering_have_identical_rows_for_one_entry() {
    let width = 40;
    let body = vec![Line::from("alpha beta gamma")];
    let expected = render_entry_lines(LiveEntryKind::Assistant, &body, width);
    let mut screen = ScreenModel::default();
    let mut terminal =
        Terminal::with_options(TestBackend::new(width as u16, 24), terminal_options())
            .expect("inline terminal should initialize");

    screen
        .start_live("assistant-1", LiveEntryKind::Assistant, body)
        .expect("live entry should start");
    assert_eq!(
        screen.live_lines_at_width(
            Platform {
                is_windows: false,
                wt_session: false,
            },
            width,
        ),
        expected
    );

    screen
        .commit_live(&mut terminal, "assistant-1")
        .expect("live entry should commit");
    assert_eq!(
        screen.committed_entries()[0].height() as usize,
        expected.len()
    );
    let rendered = terminal_text(&terminal);
    for line in expected.iter().filter(|line| !line.to_string().is_empty()) {
        assert!(rendered.contains(line.to_string().trim_end()));
    }
}

#[test]
fn assistant_streaming_preview_is_disabled_on_windows_or_windows_terminal() {
    assert!(!live_preview_enabled(
        LiveEntryKind::Assistant,
        Platform {
            is_windows: true,
            wt_session: false,
        }
    ));
    assert!(!live_preview_enabled(
        LiveEntryKind::Assistant,
        Platform {
            is_windows: false,
            wt_session: true,
        }
    ));
    assert!(live_preview_enabled(
        LiveEntryKind::Assistant,
        Platform {
            is_windows: false,
            wt_session: false,
        }
    ));
}

#[test]
fn bash_progress_remains_live_on_windows() {
    assert!(live_preview_enabled(
        LiveEntryKind::Bash,
        Platform {
            is_windows: true,
            wt_session: true,
        }
    ));
}

#[test]
fn main_screen_restore_only_disables_bracketed_paste() {
    let mut output = Vec::new();

    picopilot::screen_model::restore_main_screen(&mut output)
        .expect("main-screen restore should succeed");

    assert_eq!(output, b"\x1b[?2004l");
    assert!(!output.windows(8).any(|window| window == b"\x1b[?1049l"));
}
