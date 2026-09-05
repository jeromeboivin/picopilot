use picopilot::events::EventUpdate;
use picopilot::palette;
use picopilot::screen_model::{
    enter_main_screen, live_preview_enabled, render_entry_lines, terminal_options, LiveEntryKind,
    Platform, ScreenChange, ScreenEntry, ScreenModel, FIXED_LIVE_REGION_HEIGHT,
};
use picopilot::tui::App;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::{Terminal, Viewport};
use serde_json::json;

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
        let actual = render_entry_lines(entry.kind(), entry.lines(), width)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            expected.into_iter().map(str::to_string).collect::<Vec<_>>()
        );
    }
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
    assert_eq!(screen.committed_count(), 2);
    assert!(rendered.find("assistant final") < rendered.find("tool completed"));
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
