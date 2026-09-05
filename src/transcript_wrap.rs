use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone)]
pub struct WrapSpec {
    pub wrap_width: usize,
    pub fill_width: usize,
    pub first_prefix: Vec<Span<'static>>,
    pub continuation_prefix: Vec<Span<'static>>,
    pub fill_style: Option<Style>,
}

pub fn wrap_lines(lines: &[Line<'_>], spec: &WrapSpec) -> Vec<Line<'static>> {
    let mut spans = Vec::new();
    for (line_index, line) in lines.iter().enumerate() {
        if line_index > 0 {
            spans.push(Span::raw("\n"));
        }
        spans.extend(
            line.spans
                .iter()
                .map(|span| Span::styled(span.content.to_string(), line.style.patch(span.style))),
        );
    }
    wrap_spans(&spans, spec)
}

pub fn wrap_spans(spans: &[Span<'_>], spec: &WrapSpec) -> Vec<Line<'static>> {
    let segments = logical_segments(spans);
    let mut rows = Vec::new();
    let mut row_number = 0;

    for segment in segments {
        let segment_rows = wrap_segment(&segment, spec, row_number);
        row_number += segment_rows.len();
        rows.extend(segment_rows);
    }

    if rows.is_empty() {
        rows.push(Vec::new());
    }

    rows.into_iter()
        .enumerate()
        .map(|(index, body)| finish_row(index, body, spec))
        .collect()
}

#[derive(Clone)]
struct SourceChar {
    character: char,
    style: Style,
}

#[derive(Clone)]
struct Cluster {
    text: String,
    style: Style,
    width: usize,
}

fn logical_segments(spans: &[Span<'_>]) -> Vec<Vec<Cluster>> {
    let source = spans
        .iter()
        .flat_map(|span| {
            span.content.chars().map(move |character| SourceChar {
                character,
                style: span.style,
            })
        })
        .collect::<Vec<_>>();
    let mut normalized_source = Vec::with_capacity(source.len());
    let mut source_index = 0;
    while source_index < source.len() {
        let current = &source[source_index];
        if current.character == '\r'
            && source
                .get(source_index + 1)
                .is_some_and(|next| next.character == '\n')
        {
            source_index += 1;
            continue;
        } else {
            normalized_source.push(current.clone());
        }
        source_index += 1;
    }

    let mut segments = Vec::new();
    let mut segment_source = Vec::new();
    for source_char in normalized_source {
        if source_char.character == '\n' {
            segments.push(clusters(&segment_source));
            segment_source.clear();
        } else {
            segment_source.push(source_char);
        }
    }
    segments.push(clusters(&segment_source));
    segments
}

fn clusters(source: &[SourceChar]) -> Vec<Cluster> {
    if source.is_empty() {
        return Vec::new();
    }

    let text = source
        .iter()
        .map(|source| source.character)
        .collect::<String>();
    let mut byte_styles = Vec::with_capacity(source.len());
    let mut byte_offset = 0;
    for source_char in source {
        byte_styles.push((byte_offset, source_char.style));
        byte_offset += source_char.character.len_utf8();
    }

    let mut display_column = 0;
    text.grapheme_indices(true)
        .flat_map(|(start, grapheme)| {
            let style = byte_styles
                .iter()
                .rev()
                .find(|(offset, _)| *offset <= start)
                .map(|(_, style)| *style)
                .unwrap_or_default();
            let normalized = grapheme.nfc().collect::<String>();
            if normalized == "\t" {
                let spaces = 8 - (display_column % 8);
                display_column += spaces;
                (0..spaces)
                    .map(|_| Cluster {
                        width: 1,
                        text: " ".to_string(),
                        style,
                    })
                    .collect::<Vec<_>>()
            } else {
                let width = UnicodeWidthStr::width(normalized.as_str());
                display_column += width;
                vec![Cluster {
                    width,
                    text: normalized,
                    style,
                }]
            }
        })
        .collect()
}

fn wrap_segment(segment: &[Cluster], spec: &WrapSpec, row_number: usize) -> Vec<Vec<Cluster>> {
    if segment.is_empty() {
        return vec![Vec::new()];
    }

    let mut rows = vec![Vec::new()];
    let mut column = 0;
    for word in word_tokens(segment) {
        let word_width = word.iter().map(|cluster| cluster.width).sum::<usize>();
        let capacity = row_capacity(spec, row_number + rows.len() - 1);
        if column > 0 && column + word_width > capacity {
            rows.push(Vec::new());
            column = 0;
        }

        for cluster in word {
            let capacity = row_capacity(spec, row_number + rows.len() - 1);
            if column > 0 && column + cluster.width > capacity {
                rows.push(Vec::new());
                column = 0;
            }
            rows.last_mut()
                .expect("a row always exists")
                .push(cluster.clone());
            column += cluster.width;
        }
    }

    rows
}

fn word_tokens(segment: &[Cluster]) -> Vec<Vec<Cluster>> {
    let mut tokens = Vec::new();
    let mut token = Vec::new();
    for cluster in segment {
        token.push(cluster.clone());
        if cluster.text == " " {
            tokens.push(std::mem::take(&mut token));
        }
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    tokens
}

fn row_capacity(spec: &WrapSpec, row_number: usize) -> usize {
    let prefix_width = if row_number == 0 {
        prefix_width(&spec.first_prefix)
    } else {
        prefix_width(&spec.continuation_prefix)
    };
    spec.wrap_width.saturating_sub(prefix_width)
}

fn prefix_width(prefix: &[Span<'static>]) -> usize {
    prefix
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

fn finish_row(index: usize, body: Vec<Cluster>, spec: &WrapSpec) -> Line<'static> {
    let mut spans = if index == 0 {
        spec.first_prefix.clone()
    } else {
        spec.continuation_prefix.clone()
    };
    let prefix_len = spans.len();
    for cluster in body {
        let can_merge = spans.len() > prefix_len
            && spans.last().is_some_and(|last| last.style == cluster.style);
        if can_merge {
            let last = spans.last_mut().expect("merge span exists");
            last.content.to_mut().push_str(&cluster.text);
            continue;
        }
        spans.push(Span::styled(cluster.text, cluster.style));
    }

    let width = spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum::<usize>();
    if let Some(fill_style) = spec.fill_style {
        if width < spec.fill_width {
            spans.push(Span::styled(
                " ".repeat(spec.fill_width - width),
                fill_style,
            ));
        }
        Line::from(spans).style(fill_style)
    } else {
        Line::from(spans)
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Style};
    use ratatui::text::Span;
    use unicode_width::UnicodeWidthStr;

    use super::{wrap_spans, WrapSpec};

    fn spec(wrap_width: usize) -> WrapSpec {
        WrapSpec {
            wrap_width,
            fill_width: 0,
            first_prefix: Vec::new(),
            continuation_prefix: Vec::new(),
            fill_style: None,
        }
    }

    #[test]
    fn empty_input_emits_one_prefixed_line() {
        let lines = wrap_spans(
            &[],
            &WrapSpec {
                wrap_width: 9,
                fill_width: 10,
                first_prefix: vec![Span::raw("❯ ")],
                continuation_prefix: Vec::new(),
                fill_style: None,
            },
        );

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].to_string(), "❯ ");
    }

    #[test]
    fn normalizes_nfc_and_crlf_while_preserving_hard_blank_lines() {
        let style = ratatui::style::Style::default().fg(ratatui::style::Color::Yellow);
        let lines = wrap_spans(
            &[Span::styled("e\u{301}\r\n\r\nnext", style)],
            &WrapSpec {
                wrap_width: 20,
                fill_width: 0,
                first_prefix: vec![Span::raw("❯ ")],
                continuation_prefix: vec![Span::raw("  ")],
                fill_style: None,
            },
        );

        assert_eq!(
            lines.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["❯ é", "  ", "  next"]
        );
        assert_eq!(lines[0].spans[1].content, "é");
        assert_eq!(lines[0].spans[1].style, style);
    }

    #[test]
    fn preserves_leading_repeated_and_trailing_ascii_spaces() {
        let lines = wrap_spans(&[Span::raw("  one  two  ")], &spec(8));

        assert_eq!(
            lines.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["  one  ", "two  "]
        );
    }

    #[test]
    fn keeps_spaces_with_the_word_that_precedes_a_soft_break() {
        let lines = wrap_spans(&[Span::raw("one two three")], &spec(7));

        assert_eq!(
            lines.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["one ", "two ", "three"]
        );
    }

    #[test]
    fn expands_tabs_to_the_next_eight_column_stop() {
        let lines = wrap_spans(&[Span::raw("a\tb\n1234567\tz")], &spec(20));

        assert_eq!(
            lines.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["a       b", "1234567 z"]
        );
    }

    #[test]
    fn style_boundaries_do_not_create_word_boundaries() {
        let first = Style::default().fg(Color::Red);
        let second = Style::default().fg(Color::Blue);
        let lines = wrap_spans(
            &[
                Span::styled("hel", first),
                Span::styled("lo wor", second),
                Span::styled("ld", first),
            ],
            &spec(20),
        );

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].to_string(), "hello world");
        assert_eq!(lines[0].spans[0].content, "hel");
        assert_eq!(lines[0].spans[1].content, "lo wor");
        assert_eq!(lines[0].spans[2].content, "ld");
    }

    #[test]
    fn measures_graphemes_with_unicode_width() {
        let lines = wrap_spans(&[Span::raw("a界b e\u{301} 👩\u{200d}💻")], &spec(6));

        assert_eq!(
            lines.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["a界b ", "é 👩\u{200d}💻"]
        );
        assert_eq!("界".width(), 2);
        assert_eq!("¡".width(), 1);
    }

    #[test]
    fn emits_a_cluster_wider_than_the_limit_instead_of_dropping_it() {
        let lines = wrap_spans(&[Span::raw("界a")], &spec(1));

        assert_eq!(
            lines.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["界", "a"]
        );
    }

    #[test]
    fn hard_breaks_oversized_ascii_words_at_grapheme_boundaries() {
        let lines = wrap_spans(&[Span::raw("abcdefghij")], &spec(4));

        assert_eq!(
            lines.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["abcd", "efgh", "ij"]
        );
    }

    #[test]
    fn keeps_combining_marks_with_their_base_cluster() {
        let lines = wrap_spans(&[Span::raw("e\u{301}x")], &spec(1));

        assert_eq!(
            lines.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["é", "x"]
        );
    }

    #[test]
    fn ambiguous_width_characters_are_measured_as_narrow() {
        let lines = wrap_spans(&[Span::raw("¡a")], &spec(2));

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].to_string(), "¡a");
    }

    #[test]
    fn applies_true_hanging_prefixes_after_breaking_words() {
        let lines = wrap_spans(
            &[Span::raw("alpha beta gamma")],
            &WrapSpec {
                wrap_width: 10,
                fill_width: 0,
                first_prefix: vec![Span::raw("● ")],
                continuation_prefix: vec![Span::raw("  ")],
                fill_style: None,
            },
        );

        assert_eq!(
            lines.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["● alpha ", "  beta ", "  gamma"]
        );
    }

    #[test]
    fn pads_background_rows_by_display_width() {
        let fill_style = Style::default().bg(Color::Rgb(55, 55, 55));
        let lines = wrap_spans(
            &[Span::raw("界")],
            &WrapSpec {
                wrap_width: 9,
                fill_width: 10,
                first_prefix: vec![Span::raw("❯ ")],
                continuation_prefix: Vec::new(),
                fill_style: Some(fill_style),
            },
        );

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].width(), 10);
        assert_eq!(lines[0].to_string(), "❯ 界      ");
        assert_eq!(lines[0].style, fill_style);
        assert_eq!(lines[0].spans.last().unwrap().content, "      ");
        assert_eq!(lines[0].spans.last().unwrap().style, fill_style);
    }
}
