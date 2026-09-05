use picopilot::ansi::{parse_sanitized_ansi, sanitize_ansi, AnsiSanitizer};
use ratatui::style::{Color, Modifier, Style};

#[test]
fn sanitizer_enforces_the_input_allowlist() {
    let cases = [
        (
            "normalizes_controls_and_tabs",
            "a\tb\r\nc\rd\u{0000}e\u{000b}f\u{0085}g",
            "a       b\ncdefg",
        ),
        (
            "preserves_printable_unicode_and_width_inputs",
            "e\u{301} 👩\u{200d}💻 界\tX",
            "e\u{301} 👩\u{200d}💻 界 X",
        ),
        (
            "keeps_supported_sgr",
            "\u{1b}[31mred\u{1b}[39m \u{1b}[101;38;5;42;48;2;1;2;3mcolor\u{1b}[49m",
            "\u{1b}[31mred\u{1b}[39m \u{1b}[101;38;5;42;48;2;1;2;3mcolor\u{1b}[49m",
        ),
        (
            "removes_underline_parameters_without_dropping_the_sequence",
            "\u{1b}[4;21;24;58;59;31mred\u{1b}[4mplain",
            "\u{1b}[31mredplain",
        ),
        (
            "drops_unknown_sgr_as_a_whole",
            "before\u{1b}[1;999;31mvisible\u{1b}[3;31mkept",
            "beforevisible\u{1b}[3;31mkept",
        ),
        (
            "drops_non_sgr_csi",
            "a\u{1b}[2Jb\u{1b}[Kc\u{1b}[1;1H d",
            "abc d",
        ),
        (
            "drops_osc_bel_and_st_forms",
            "a\u{1b}]0;title\u{07}b\u{1b}]8;;https://example.test\u{1b}\\c\u{1b}]52;c;secret\u{1b}\\d",
            "abcd",
        ),
        (
            "drops_string_controls",
            "a\u{1b}P dcs\u{1b}\\b\u{1b}_ apc\u{1b}\\c\u{1b}^ pm\u{1b}\\d\u{1b}X sos\u{1b}\\e",
            "abcde",
        ),
        (
            "drops_charset_single_and_truncated_escapes",
            "a\u{1b}(0b\u{1b})Bc\u{1b}*0d\u{1b}+Ee\u{1b}7f\u{1b}cg\u{1b}D h\u{1b}[31",
            "abcdefg h",
        ),
    ];

    for (name, input, expected) in cases {
        assert_eq!(sanitize_ansi(input), expected, "case {name}");
    }
}

#[test]
fn every_character_boundary_has_the_same_sanitized_result() {
    let input = "left\u{1b}[38;2;1;2;3mred\u{1b}]8;;url\u{07}right\u{1b}[0m";
    let expected = sanitize_ansi(input);

    for split in input
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(input.len()))
    {
        let (first, second) = input.split_at(split);
        let mut sanitizer = AnsiSanitizer::default();
        let mut actual = sanitizer.push(first);
        actual.push_str(&sanitizer.push(second));
        actual.push_str(&sanitizer.finish());
        assert_eq!(actual, expected, "split at byte {split}");
    }
}

#[test]
fn parsed_sgr_patches_the_surface_style_and_supports_terminal_colors() {
    let base = Style::default().fg(Color::Gray).bg(Color::Black);
    let sanitized = sanitize_ansi(
        "base \u{1b}[31mred\u{1b}[91mbright\u{1b}[38;5;42mindexed\u{1b}[48;2;1;2;3mtrue",
    );
    let lines = parse_sanitized_ansi(&sanitized, base);

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].to_string(), "base redbrightindexedtrue");
    assert_eq!(lines[0].spans[0].style, base);
    assert_eq!(lines[0].spans[1].style.fg, Some(Color::Red));
    assert_eq!(lines[0].spans[2].style.fg, Some(Color::LightRed));
    assert_eq!(lines[0].spans[3].style.fg, Some(Color::Indexed(42)));
    assert_eq!(lines[0].spans[4].style.fg, Some(Color::Indexed(42)));
    assert_eq!(lines[0].spans[4].style.bg, Some(Color::Rgb(1, 2, 3)));
}

#[test]
fn parsed_sgr_handles_modifier_off_codes_and_removes_underline() {
    let base = Style::default().add_modifier(Modifier::BOLD);
    let sanitized = sanitize_ansi("\u{1b}[1;2;3;7;9mstyled\u{1b}[22;23;27;29mplain\u{1b}[4mclean");
    let lines = parse_sanitized_ansi(&sanitized, base);

    assert_eq!(lines[0].to_string(), "styledplainclean");
    assert!(lines[0].spans[0]
        .style
        .add_modifier
        .contains(Modifier::BOLD | Modifier::DIM | Modifier::ITALIC));
    assert!(lines[0].spans[0]
        .style
        .add_modifier
        .contains(Modifier::REVERSED));
    assert!(lines[0].spans[0]
        .style
        .add_modifier
        .contains(Modifier::CROSSED_OUT));
    assert!(!lines[0].spans[1].style.add_modifier.contains(Modifier::DIM));
    assert!(!lines[0].spans[1]
        .style
        .add_modifier
        .contains(Modifier::ITALIC));
    assert!(!lines[0].spans[1]
        .style
        .add_modifier
        .contains(Modifier::REVERSED));
    assert!(!lines[0]
        .spans
        .iter()
        .any(|span| span.style.add_modifier.contains(Modifier::UNDERLINED)));
    assert!(lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .all(|span| !span.content.contains('\u{1b}')));
}

#[test]
fn stream_state_is_independent_and_finish_drops_incomplete_sequences() {
    let mut red = AnsiSanitizer::default();
    let mut green = AnsiSanitizer::default();

    assert_eq!(red.push("red \u{1b}[31"), "red ");
    assert_eq!(green.push("green \u{1b}[32"), "green ");
    assert_eq!(red.push("mR"), "\u{1b}[31mR");
    assert_eq!(green.push("mG"), "\u{1b}[32mG");

    let mut unfinished = AnsiSanitizer::default();
    assert_eq!(unfinished.push("visible\u{1b}[38;2"), "visible");
    assert_eq!(unfinished.finish(), "");
    assert_eq!(unfinished.push(";1;2;3mcontinued"), ";1;2;3mcontinued");
}

#[test]
fn string_controls_only_end_on_an_immediate_escape_backslash_pair() {
    let input =
        "\u{1b}]title\u{1b}x\\leaked\u{0007}visible\u{1b}Pdata\u{1b}y\\also-leaked\u{009c}done";
    let expected = "visibledone";

    assert_eq!(sanitize_ansi(input), expected);

    let mut sanitizer = AnsiSanitizer::default();
    let mut actual = String::new();
    for chunk in [
        "\u{1b}]title\u{1b}",
        "x\\leaked\u{0007}visible\u{1b}Pdata\u{1b}",
        "y\\also-leaked\u{009c}done",
    ] {
        actual.push_str(&sanitizer.push(chunk));
    }
    actual.push_str(&sanitizer.finish());

    assert_eq!(actual, expected);
}

#[test]
fn underline_color_payloads_are_removed_without_dropping_other_sgr() {
    assert_eq!(
        sanitize_ansi("\u{1b}[58;5;42;31mred\u{1b}[59m\u{1b}[58;2;1;2;3;32mgreen"),
        "\u{1b}[31mred\u{1b}[32mgreen"
    );
}

#[test]
fn grapheme_width_stays_correct_when_combining_and_zwj_sequences_split() {
    let input = "e\u{301} 👩\u{200d}💻 界\tX";
    let expected = "e\u{301} 👩\u{200d}💻 界 X";

    assert_eq!(sanitize_ansi(input), expected);

    for split in input
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(input.len()))
    {
        let (first, second) = input.split_at(split);
        let mut sanitizer = AnsiSanitizer::default();
        let mut actual = sanitizer.push(first);
        actual.push_str(&sanitizer.push(second));
        actual.push_str(&sanitizer.finish());
        assert_eq!(actual, expected, "split at byte {split}");
    }
}

#[test]
fn reset_discards_pending_grapheme_width() {
    let mut sanitizer = AnsiSanitizer::default();
    assert_eq!(sanitizer.push("e\u{301}"), "e\u{301}");

    sanitizer.reset();

    assert_eq!(sanitizer.push("\tX"), "        X");
}
