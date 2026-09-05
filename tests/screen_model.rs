use picopilot::events::EventUpdate;
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

    assert_eq!(screen.committed_entries()[0].height() as usize, 5);
    assert!(terminal_text(&terminal).contains("012345"));
    assert!(terminal_text(&terminal).contains("6789AB"));
    assert!(terminal_text(&terminal).contains("CDEFGH"));
    assert!(terminal_text(&terminal).contains("IJ"));
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
