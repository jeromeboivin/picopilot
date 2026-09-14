use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use github_copilot_sdk::types::{Model, SessionId, SessionMetadata};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::Terminal;
use serde_json::json;
use tokio::sync::oneshot;

use crate::events::{
    ContextAttributionSnapshot, ContextCategorySnapshot, UsageMetricsSnapshot, UsageSnapshot,
};
use crate::permissions::{ApprovalCategory, ApprovalDecision, ApprovalRequest};
use crate::screen_model::{
    render_transcript_payload_with_options, LiveEntryKind, NoticeKind, SubagentPayload,
    ToolCallState, ToolHeaderPayload, ToolPlatform, ToolProgressKind, ToolProgressPayload,
    ToolResultPayload, ToolResultState, TranscriptPayload,
};
use crate::skills::{Skill, SkillCatalog, SkillRoot, SkillRootSource, SkillSelection};

use super::{
    draw, render_spinner_line_for_platform, spinner_frames, App, ChatEntry, SpinnerPlatform,
};

const WIDTHS: &[usize] = &[20, 40, 80, 120];
const GALLERY_PATH: &str = "tests/fixtures/rendering/gallery.txt";
const REGENERATE_ENV: &str = "PICOPILOT_REGENERATE_RENDERING_FIXTURES";

#[derive(Debug)]
struct TranscriptFixture {
    name: String,
    kind: LiveEntryKind,
    payload: TranscriptPayload,
    platform: ToolPlatform,
    animation_elapsed_ms: u64,
    verbose: bool,
}

#[test]
fn committed_rendering_gallery_matches_production_renderer() {
    let expected = include_str!("../tests/fixtures/rendering/gallery.txt");
    let actual = rendering_gallery();

    if actual != expected {
        let diff = similar::TextDiff::from_lines(expected, &actual)
            .unified_diff()
            .context_radius(3)
            .header("committed gallery", "production rendering")
            .to_string();
        panic!("rendering gallery is stale; regenerate only after review:\n{diff}");
    }
}

#[test]
fn rendering_gallery_generation_is_deterministic() {
    assert_eq!(rendering_gallery(), rendering_gallery());
}

#[test]
#[ignore = "writes the committed gallery after explicit review"]
fn regenerate_rendering_gallery() {
    assert_eq!(
        std::env::var(REGENERATE_ENV).ok().as_deref(),
        Some("1"),
        "set {REGENERATE_ENV}=1 to regenerate {GALLERY_PATH}"
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(GALLERY_PATH);
    fs::write(path, rendering_gallery()).expect("rendering gallery should be writable");
}

fn rendering_gallery() -> String {
    let mut output = String::new();
    writeln!(output, "picopilot-rendering-gallery-v1").unwrap();
    writeln!(output, "widths={}", join_widths()).unwrap();
    writeln!(output, "style=foreground/background/modifiers").unwrap();
    writeln!(output).unwrap();

    for fixture in transcript_fixtures() {
        append_transcript_fixture(&mut output, &fixture);
    }
    append_spinner_fixtures(&mut output);
    append_app_fixtures(&mut output);
    if output.ends_with('\n') {
        output.pop();
    }
    output
}

fn join_widths() -> String {
    WIDTHS
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn append_transcript_fixture(output: &mut String, fixture: &TranscriptFixture) {
    writeln!(
        output,
        "[transcript name={} kind={:?} platform={:?} clock={} verbose={}]",
        fixture.name, fixture.kind, fixture.platform, fixture.animation_elapsed_ms, fixture.verbose
    )
    .unwrap();
    for &width in WIDTHS {
        let lines = render_transcript_payload_with_options(
            fixture.kind,
            &fixture.payload,
            width,
            fixture.platform,
            fixture.animation_elapsed_ms,
            fixture.verbose,
        );
        append_lines(output, width, &lines);
    }
    writeln!(output).unwrap();
}

fn append_lines(output: &mut String, width: usize, lines: &[Line<'static>]) {
    writeln!(output, "width={width} lines={}", lines.len()).unwrap();
    for (line_index, line) in lines.iter().enumerate() {
        writeln!(
            output,
            "line={line_index} width={} text={} fg={} bg={} modifiers={:?}/{:?}",
            line.width(),
            quote(&line.to_string()),
            color_name(line.style.fg),
            color_name(line.style.bg),
            line.style.add_modifier,
            line.style.sub_modifier
        )
        .unwrap();
        for (span_index, span) in line.spans.iter().enumerate() {
            writeln!(
                output,
                "span={span_index} text={} fg={} bg={} modifiers={:?}/{:?}",
                quote(span.content.as_ref()),
                color_name(span.style.fg),
                color_name(span.style.bg),
                span.style.add_modifier,
                span.style.sub_modifier
            )
            .unwrap();
        }
    }
}

fn append_spinner_fixtures(output: &mut String) {
    let platforms = [
        ("macos", SpinnerPlatform::Macos),
        ("windows-linux", SpinnerPlatform::WindowsLinux),
        ("ghostty", SpinnerPlatform::Ghostty),
    ];
    let clocks = [0, 120, 600, 3_000, 30_000];
    for (platform_name, platform) in platforms {
        let mut app = gallery_app();
        app.status.busy = true;
        app.spinner.active = true;
        app.spinner.started_at_ms = 0;
        app.spinner.last_output_at_ms = 0;
        app.spinner.last_advance_at_ms = 0;
        app.spinner.verb = "Working…".to_string();
        app.spinner.assistant_characters = 96;
        app.spinner.displayed_characters = 48;
        app.assistant_live_ids.insert("assistant".to_string());
        writeln!(
            output,
            "[spinner platform={platform_name} frames={:?}]",
            spinner_frames(platform)
        )
        .unwrap();
        for &width in WIDTHS {
            for &clock in &clocks {
                let line = render_spinner_line_for_platform(&app, width, clock, platform);
                writeln!(output, "clock={clock}").unwrap();
                append_lines(output, width, &[line]);
            }
        }
        app.set_reduced_motion(true);
        writeln!(output, "reduced_motion=true").unwrap();
        for &width in WIDTHS {
            let line = render_spinner_line_for_platform(&app, width, 600, platform);
            append_lines(output, width, &[line]);
        }
        writeln!(output).unwrap();
    }
}

fn append_app_fixtures(output: &mut String) {
    append_startup_app_fixture(output, "startup-authoritative-artwork", 24, |_| {});
    append_startup_app_fixture(output, "startup-two-column-and-vertical", 14, |_| {});
    append_startup_wordmark_fallback_fixture(output);
    append_startup_app_fixture(output, "startup-wrapped-values", 14, |app| {
        app.set_model(Some("gallery-model".to_string()));
        app.preload_models(vec![Model {
            id: "gallery-model".to_string(),
            name: "Gallery Model With A Deterministic Long Display Name".to_string(),
            ..Model::default()
        }]);
    });
    append_startup_app_fixture(output, "startup-bounded-metadata", 10, |app| {
        app.set_model(Some("gallery-model".to_string()));
        app.preload_models(vec![Model {
            id: "gallery-model".to_string(),
            name: "Gallery Model With A Deterministic Long Display Name".to_string(),
            ..Model::default()
        }]);
    });
    append_startup_app_fixture(output, "startup-metadata-omission", 14, |app| {
        app.set_model(None);
        app.set_toolset(crate::toolset::Toolset::empty());
        app.set_skill_selection(SkillSelection::none());
    });
    append_app_fixture(output, "input-typed", 14, |app| {
        for character in "Unicode input: cafe[31m e[0m 👩‍💻".chars() {
            app.push_input(character);
        }
    });
    append_app_fixture(output, "completion", 14, |app| {
        for character in "/s".chars() {
            app.push_input(character);
        }
    });
    append_app_fixture(output, "picker-sessions", 14, setup_session_picker);
    append_app_fixture(output, "picker-models", 14, setup_model_picker);
    append_app_fixture(output, "picker-tools", 14, |app| app.open_tool_picker());
    append_app_fixture(output, "picker-skills", 14, setup_skill_picker);
    append_app_fixture(output, "picker-approval", 14, setup_approval_picker);
    append_app_fixture(output, "approval-resolved", 14, setup_resolved_approval);
    append_app_fixture(output, "status", 16, setup_status);
    append_app_fixture(output, "usage", 16, setup_usage);
    append_app_fixture(
        output,
        "nested-concurrent-tasks",
        16,
        setup_concurrent_tasks,
    );
    append_app_fixture(
        output,
        "consecutive-user-messages",
        14,
        setup_consecutive_users,
    );
}

fn append_startup_app_fixture<F>(output: &mut String, name: &str, height: u16, setup: F)
where
    F: Fn(&mut App),
{
    writeln!(output, "[startup name={name} height={height}]").unwrap();
    for &width in WIDTHS {
        let mut app = startup_gallery_app();
        setup(&mut app);
        let mut terminal = Terminal::new(TestBackend::new(width as u16, height))
            .expect("startup gallery terminal should initialize");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("startup gallery surface should render");
        append_buffer(output, width, height as usize, terminal.backend().buffer());
    }
    writeln!(output).unwrap();
}

fn append_startup_wordmark_fallback_fixture(output: &mut String) {
    const WIDTH: usize = 3;
    const HEIGHT: u16 = 14;

    writeln!(
        output,
        "[startup name=startup-wordmark-fallback height={HEIGHT}]"
    )
    .unwrap();
    let app = startup_gallery_app();
    let mut terminal = Terminal::new(TestBackend::new(WIDTH as u16, HEIGHT))
        .expect("startup fallback gallery terminal should initialize");
    terminal
        .draw(|frame| draw(frame, &app))
        .expect("startup fallback gallery surface should render");
    append_buffer(output, WIDTH, HEIGHT as usize, terminal.backend().buffer());
    writeln!(output).unwrap();
}

fn append_app_fixture<F>(output: &mut String, name: &str, height: u16, setup: F)
where
    F: Fn(&mut App),
{
    writeln!(output, "[app name={name} height={height}]").unwrap();
    for &width in WIDTHS {
        let mut app = gallery_app();
        setup(&mut app);
        let mut terminal = Terminal::new(TestBackend::new(width as u16, height))
            .expect("gallery terminal should initialize");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("gallery surface should render");
        append_buffer(output, width, height as usize, terminal.backend().buffer());
    }
    writeln!(output).unwrap();
}

fn append_buffer(output: &mut String, width: usize, height: usize, buffer: &Buffer) {
    writeln!(output, "width={width} rows={height}").unwrap();
    for y in 0..height {
        let text = (0..width)
            .map(|x| buffer[(x as u16, y as u16)].symbol())
            .collect::<String>();
        writeln!(output, "row={y} text={}", quote(&text)).unwrap();
        let mut start = 0;
        while start < width {
            let style = cell_style(&buffer[(start as u16, y as u16)]);
            let mut end = start + 1;
            while end < width && cell_style(&buffer[(end as u16, y as u16)]) == style {
                end += 1;
            }
            let span_text = (start..end)
                .map(|x| buffer[(x as u16, y as u16)].symbol())
                .collect::<String>();
            writeln!(
                output,
                "cell_span={start}..{end} text={} fg={} bg={} modifiers={:?}/{:?}",
                quote(&span_text),
                color_name(style.fg),
                color_name(style.bg),
                style.add_modifier,
                style.sub_modifier
            )
            .unwrap();
            start = end;
        }
    }
}

fn cell_style(cell: &ratatui::buffer::Cell) -> Style {
    Style::default()
        .fg(cell.fg)
        .bg(cell.bg)
        .add_modifier(cell.modifier)
}

fn quote(value: &str) -> String {
    serde_json::to_string(value).expect("fixture text should be serializable")
}

fn color_name(color: Option<Color>) -> String {
    match color {
        None => "default".to_string(),
        Some(Color::Reset) => "reset".to_string(),
        Some(Color::Black) => "black".to_string(),
        Some(Color::Red) => "red".to_string(),
        Some(Color::Green) => "green".to_string(),
        Some(Color::Yellow) => "yellow".to_string(),
        Some(Color::Blue) => "blue".to_string(),
        Some(Color::Magenta) => "magenta".to_string(),
        Some(Color::Cyan) => "cyan".to_string(),
        Some(Color::Gray) => "gray".to_string(),
        Some(Color::DarkGray) => "dark-gray".to_string(),
        Some(Color::LightRed) => "light-red".to_string(),
        Some(Color::LightGreen) => "light-green".to_string(),
        Some(Color::LightYellow) => "light-yellow".to_string(),
        Some(Color::LightBlue) => "light-blue".to_string(),
        Some(Color::LightMagenta) => "light-magenta".to_string(),
        Some(Color::LightCyan) => "light-cyan".to_string(),
        Some(Color::White) => "white".to_string(),
        Some(Color::Indexed(index)) => format!("indexed({index})"),
        Some(Color::Rgb(red, green, blue)) => format!("#{red:02x}{green:02x}{blue:02x}"),
    }
}

fn transcript_fixtures() -> Vec<TranscriptFixture> {
    let mut fixtures = vec![
        transcript(
            "user-short-wrapped-hard-newline-unicode",
            LiveEntryKind::User,
            TranscriptPayload::PreRendered(vec![Line::from(
                "short user message with a hard\nnewline, CJK 界, combining e\u{301}, emoji 👩‍💻, tab\tend",
            )]),
        ),
        transcript(
            "user-truncation-10001",
            LiveEntryKind::User,
            TranscriptPayload::PreRendered(vec![Line::from("x".repeat(10_001))]),
        ),
        transcript(
            "assistant-streaming-partial",
            LiveEntryKind::Assistant,
            TranscriptPayload::AssistantMarkdown("partial assistant response".to_string()),
        ),
        transcript(
            "assistant-nested-no-dot",
            LiveEntryKind::AssistantNested,
            TranscriptPayload::PreRendered(vec![Line::from("nested assistant response")]),
        ),
        transcript(
            "markdown-headings-emphasis-links-lists",
            LiveEntryKind::Assistant,
            TranscriptPayload::AssistantMarkdown(
                "# H1\n\n## H2\n\n###### H6\n\n**bold** *italic* ~~strike~~ `literal` [link](https://example.test) [mail](mailto:test@example.test) #123 ![image](image.png)\n\n> quote\n>\n> next\n\n1. one\n   - nested\n- [ ] task\n\n---\n\n<div>drop</div>"
                    .to_string(),
            ),
        ),
        transcript(
            "code-known-language",
            LiveEntryKind::Assistant,
            TranscriptPayload::AssistantMarkdown(
                "```rust\nfn main() { println!(\"ready\"); }\n```".to_string(),
            ),
        ),
        transcript(
            "code-unknown-language-plaintext",
            LiveEntryKind::Assistant,
            TranscriptPayload::AssistantMarkdown(
                "```not-a-language\nplain fallback text\n```".to_string(),
            ),
        ),
        transcript(
            "table-aligned-wrapped-fallback",
            LiveEntryKind::Assistant,
            TranscriptPayload::AssistantMarkdown(
                "| Name | Description |\n| :--- | ---: |\n| alpha | wrapped cell content |\n| beta | second row |".to_string(),
            ),
        ),
        transcript(
            "reasoning-collapsed",
            LiveEntryKind::Other,
            TranscriptPayload::Reasoning {
                content: "private reasoning content".to_string(),
                expanded: false,
            },
        ),
        transcript(
            "reasoning-expanded",
            LiveEntryKind::Other,
            TranscriptPayload::Reasoning {
                content: "expanded reasoning with **markdown** and a second line".to_string(),
                expanded: true,
            },
        ),
        transcript(
            "notice-warning-error-diagnostic",
            LiveEntryKind::Other,
            TranscriptPayload::Notice {
                kind: NoticeKind::Warning,
                content: "warning diagnostic row".to_string(),
            },
        ),
        transcript(
            "notice-error",
            LiveEntryKind::Other,
            TranscriptPayload::Notice {
                kind: NoticeKind::Error,
                content: "error diagnostic row".to_string(),
            },
        ),
        transcript(
            "notice-diagnostic",
            LiveEntryKind::Other,
            TranscriptPayload::Notice {
                kind: NoticeKind::Diagnostic,
                content: "diagnostic row".to_string(),
            },
        ),
        transcript(
            "notice-approval-resolved",
            LiveEntryKind::Other,
            TranscriptPayload::Notice {
                kind: NoticeKind::Approval,
                content: "Bash allowed once".to_string(),
            },
        ),
        transcript(
            "subagent-task-top-level",
            LiveEntryKind::Other,
            TranscriptPayload::Subagent(SubagentPayload {
                tool_call_id: "task-1".to_string(),
                description: "review renderer".to_string(),
                state: ToolCallState::Success,
                error: None,
                duration_ms: Some(1_234),
                total_tool_calls: Some(4),
                total_tokens: Some(5_678),
                agent_id: None,
            }),
        ),
        transcript(
            "subagent-task-nested",
            LiveEntryKind::ToolNested,
            TranscriptPayload::Subagent(SubagentPayload {
                tool_call_id: "task-2".to_string(),
                description: "nested task".to_string(),
                state: ToolCallState::Running,
                error: None,
                duration_ms: None,
                total_tool_calls: None,
                total_tokens: None,
                agent_id: Some("agent-2".to_string()),
            }),
        ),
        transcript(
            "tool-progress-live-output",
            LiveEntryKind::Tool,
            TranscriptPayload::ToolProgress(ToolProgressPayload {
                tool_call_id: "progress-1".to_string(),
                tool_name: "bash".to_string(),
                output: "line one\nline two\nline three\nline four\nline five\nline six".to_string(),
                status: "running".to_string(),
                kind: ToolProgressKind::Tool,
                agent_id: None,
                started_at: Some(0),
                timeout: Some("30s".to_string()),
            }),
        ),
        transcript(
            "tool-result-ansi-success",
            LiveEntryKind::Tool,
            tool_result(
                "bash",
                Some(json!({"command": "printf red"})),
                crate::ansi::sanitize_ansi("\u{1b}[31mred\u{1b}[0m output"),
                ToolResultState::Success,
                None,
            ),
        ),
        transcript(
            "tool-result-error",
            LiveEntryKind::Tool,
            tool_result(
                "bash",
                Some(json!({"command": "false"})),
                "command failed with exit code 1".to_string(),
                ToolResultState::Error,
                None,
            ),
        ),
        transcript(
            "bash-truncation-verbose-off",
            LiveEntryKind::Tool,
            TranscriptPayload::ToolHeader(ToolHeaderPayload {
                tool_call_id: "bash-header".to_string(),
                tool_name: "bash".to_string(),
                arguments: Some(json!({
                    "command": "printf 'a very long command that is deliberately longer than the summary limit'\nprintf second line\nprintf third line"
                })),
                agent_id: None,
                started_at: 0,
                state: ToolCallState::Running,
                cwd: PathBuf::from("WORKSPACE"),
            }),
        ),
        transcript(
            "bash-truncation-verbose-on",
            LiveEntryKind::Tool,
            TranscriptPayload::ToolHeader(ToolHeaderPayload {
                tool_call_id: "bash-header".to_string(),
                tool_name: "bash".to_string(),
                arguments: Some(json!({
                    "command": "printf 'a very long command that is deliberately longer than the summary limit'\nprintf second line\nprintf third line"
                })),
                agent_id: None,
                started_at: 0,
                state: ToolCallState::Running,
                cwd: PathBuf::from("WORKSPACE"),
            }),
        ),
        transcript(
            "edit-diff-context-and-words",
            LiveEntryKind::Tool,
            tool_result(
                "edit",
                Some(json!({
                    "file_path": "src/render.rs",
                    "old_string": "same\nold value",
                    "new_string": "same\nnew value"
                })),
                "ignored".to_string(),
                ToolResultState::Success,
                None,
            ),
        ),
    ];

    if let Some(fixture) = fixtures
        .iter_mut()
        .find(|fixture| fixture.name == "bash-truncation-verbose-on")
    {
        fixture.verbose = true;
    }

    for state in [
        ToolCallState::Queued,
        ToolCallState::Running,
        ToolCallState::Success,
        ToolCallState::Error,
    ] {
        fixtures.push(transcript(
            match state {
                ToolCallState::Queued => "tool-header-queued",
                ToolCallState::Running => "tool-header-running",
                ToolCallState::Success => "tool-header-success",
                ToolCallState::Error => "tool-header-error",
                _ => unreachable!(),
            },
            LiveEntryKind::Tool,
            TranscriptPayload::ToolHeader(ToolHeaderPayload {
                tool_call_id: "tool-state".to_string(),
                tool_name: "read".to_string(),
                arguments: Some(json!({"file_path": "src/lib.rs"})),
                agent_id: None,
                started_at: 0,
                state,
                cwd: PathBuf::from("WORKSPACE"),
            }),
        ));
    }

    for fixture in &mut fixtures {
        fixture.platform = ToolPlatform::WindowsLinux;
    }
    let mut macos_variants = Vec::new();
    for fixture in fixtures.iter().filter(|fixture| {
        fixture.name.starts_with("tool-header") || fixture.name == "tool-result-ansi-success"
    }) {
        let variant = TranscriptFixture {
            name: format!("{}-macos", fixture.name),
            kind: fixture.kind,
            payload: fixture.payload.clone(),
            platform: ToolPlatform::MacOs,
            animation_elapsed_ms: fixture.animation_elapsed_ms,
            verbose: fixture.verbose,
        };
        macos_variants.push(variant);
    }
    fixtures.extend(macos_variants);
    fixtures
}

fn transcript(
    name: impl Into<String>,
    kind: LiveEntryKind,
    payload: TranscriptPayload,
) -> TranscriptFixture {
    TranscriptFixture {
        name: name.into(),
        kind,
        payload,
        platform: ToolPlatform::WindowsLinux,
        animation_elapsed_ms: 0,
        verbose: false,
    }
}

fn tool_result(
    tool_name: &str,
    arguments: Option<serde_json::Value>,
    content: String,
    state: ToolResultState,
    agent_id: Option<&str>,
) -> TranscriptPayload {
    TranscriptPayload::ToolResult(ToolResultPayload {
        tool_call_id: "tool-result".to_string(),
        tool_name: tool_name.to_string(),
        arguments,
        content,
        partial_output: None,
        shell_completion: None,
        state,
        agent_id: agent_id.map(str::to_string),
        cwd: PathBuf::from("WORKSPACE"),
    })
}

fn gallery_app() -> App {
    let mut app =
        App::new_with_working_directory(Some("gpt-5".to_string()), Path::new("WORKSPACE"));
    app.dismiss_startup_surface();
    app
}

fn startup_gallery_app() -> App {
    let mut app = App::new_with_working_directory(
        Some("gpt-5".to_string()),
        Path::new("WORKSPACE/picopilot-project"),
    );
    app.preload_models(vec![Model {
        id: "gpt-5".to_string(),
        name: "GPT-5 Gallery".to_string(),
        ..Model::default()
    }]);
    app.set_toolset(crate::toolset::Toolset::shell_only());
    let root = SkillRoot {
        path: PathBuf::from("WORKSPACE/.agents/skills"),
        source: SkillRootSource::Project,
    };
    let catalog = SkillCatalog::from_parts(
        vec![root.clone()],
        vec![Skill {
            name: "startup-gallery".to_string(),
            description: "Deterministic startup fixture metadata".to_string(),
            user_invocable: true,
            directory: root.path.join("startup-gallery"),
            root,
        }],
        Vec::new(),
    );
    app.set_skill_catalog(catalog.clone());
    app.set_skill_selection(SkillSelection::from_names(&catalog, ["startup-gallery"]));
    app
}

fn setup_session_picker(app: &mut App) {
    app.set_sessions(vec![
        SessionMetadata {
            session_id: SessionId::from("session-1"),
            start_time: "2026-01-01T12:00:00Z".to_string(),
            modified_time: "2026-01-01T12:01:00Z".to_string(),
            summary: Some("first gallery session".to_string()),
            is_remote: false,
        },
        SessionMetadata {
            session_id: SessionId::from("session-2"),
            start_time: "2026-01-01T12:00:00Z".to_string(),
            modified_time: "2026-01-01T12:02:00Z".to_string(),
            summary: Some("second gallery session".to_string()),
            is_remote: true,
        },
    ]);
}

fn setup_model_picker(app: &mut App) {
    app.set_local_model_ids(["local-model".to_string()]);
    app.set_models(vec![
        Model {
            billing: Some(serde_json::from_value(json!({
                "tokenPrices": {
                    "batchSize": 1000000,
                    "inputPrice": 250.0,
                    "outputPrice": 1500.0
                }
            }))
            .expect("model pricing should deserialize")),
            id: "gpt-5".to_string(),
            name: "GPT-5".to_string(),
            supported_context_tiers: Some(vec!["default".to_string(), "long_context".to_string()]),
            supported_reasoning_efforts: Some(vec!["low".to_string(), "high".to_string()]),
            ..Model::default()
        },
        Model {
            id: "local-model".to_string(),
            name: "Local Model".to_string(),
            ..Model::default()
        },
    ]);
}

fn setup_skill_picker(app: &mut App) {
    let root = SkillRoot {
        path: PathBuf::from("WORKSPACE/.agents/skills"),
        source: SkillRootSource::Project,
    };
    let catalog = SkillCatalog::from_parts(
        vec![root.clone()],
        vec![
            Skill {
                name: "rust-review".to_string(),
                description: "Review Rust code".to_string(),
                user_invocable: true,
                directory: root.path.join("rust-review"),
                root: root.clone(),
            },
            Skill {
                name: "rendering-check".to_string(),
                description: "Check visual fixtures".to_string(),
                user_invocable: true,
                directory: root.path.join("rendering-check"),
                root,
            },
        ],
        Vec::new(),
    );
    app.set_skill_catalog(catalog.clone());
    app.set_skill_selection(SkillSelection::from_names(&catalog, ["rust-review"]));
    app.open_skill_picker();
}

fn setup_approval_picker(app: &mut App) {
    let (respond_to, _response) = oneshot::channel();
    app.enqueue_approval(ApprovalRequest {
        category: ApprovalCategory::Shell,
        tool_name: "bash".to_string(),
        details: "cargo test --all-targets".to_string(),
        respond_to,
    });
}

fn setup_resolved_approval(app: &mut App) {
    let (respond_to, _response) = oneshot::channel();
    app.enqueue_approval(ApprovalRequest {
        category: ApprovalCategory::Shell,
        tool_name: "bash".to_string(),
        details: "cargo test --all-targets".to_string(),
        respond_to,
    });
    let request = app
        .resolve_approval(ApprovalDecision::ApproveOnce)
        .expect("gallery approval should resolve");
    let _ = request.respond_to.send(ApprovalDecision::ApproveOnce);
}

fn setup_status(app: &mut App) {
    app.set_session_id("session-gallery");
    app.set_reasoning_effort(Some("high".to_string()));
    app.add_local_command("/status");
    app.add_local_output_lines(super::status_detail_lines(app));
}

fn setup_usage(app: &mut App) {
    app.status.usage = Some(UsageSnapshot {
        current_tokens: 12_345,
        token_limit: 100_000,
        messages: 7,
        conversation_tokens: Some(8_000),
        system_tokens: Some(2_000),
        tool_definitions_tokens: Some(2_345),
    });
    app.set_usage_metrics(UsageMetricsSnapshot {
        total_nano_aiu: Some(1_234_000_000.0),
        total_premium_request_cost: 2.5,
        total_user_requests: 7,
        total_api_duration_ms: 456,
        current_model: Some("gpt-5".to_string()),
    });
    app.set_context_attribution(Some(ContextAttributionSnapshot {
        model_id: "gpt-5".to_string(),
        total_tokens: 12_345,
        prompt_token_limit: 100_000,
        categories: vec![
            ContextCategorySnapshot {
                label: "conversation".to_string(),
                tokens: 8_000,
            },
            ContextCategorySnapshot {
                label: "tools".to_string(),
                tokens: 4_345,
            },
        ],
        compactions: 1,
    }));
    app.add_local_command("/usage");
    app.add_local_output_lines(super::usage_detail_lines(app));
}

fn setup_concurrent_tasks(app: &mut App) {
    app.entries.extend([
        ChatEntry::Subagent {
            name: "task-one".to_string(),
            tool_call_id: "task-one".to_string(),
            description: "inspect renderer".to_string(),
            display_name: "Task".to_string(),
            status: super::SubagentStatus::Running,
            error: None,
            metrics: None,
            agent_id: None,
        },
        ChatEntry::Subagent {
            name: "task-two".to_string(),
            tool_call_id: "task-two".to_string(),
            description: "inspect tests".to_string(),
            display_name: "Task".to_string(),
            status: super::SubagentStatus::Completed,
            error: None,
            metrics: Some(super::SubagentMetrics {
                duration_ms: Some(2_000),
                total_tool_calls: Some(3),
                total_tokens: Some(456),
            }),
            agent_id: Some("agent-two".to_string()),
        },
    ]);
}

fn setup_consecutive_users(app: &mut App) {
    app.add_user_message("first user message".to_string());
    app.add_user_message("second user message".to_string());
}
