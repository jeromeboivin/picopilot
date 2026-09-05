use picopilot::screen_model::{
    enter_main_screen, live_preview_enabled, terminal_options, LiveEntryKind, Platform,
    ScreenModel, FIXED_LIVE_REGION_HEIGHT,
};
use ratatui::backend::TestBackend;
use ratatui::text::Line;
use ratatui::{Terminal, Viewport};

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
    assert!(rows.iter().any(|row| row.starts_with("first line")));
    assert!(rows.iter().any(|row| row.starts_with("second line")));
    assert_eq!(screen.committed_entries()[0].lines().len(), 2);
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
    assert_eq!(
        screen.committed_entries()[0].lines(),
        &[Line::from("original")]
    );
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
