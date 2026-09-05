use std::mem;

use ansi_to_tui::IntoText;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

const MAX_CSI_PARAMETER_BYTES: usize = 256;

#[derive(Debug, Clone, Default)]
pub struct AnsiSanitizer {
    state: State,
    column: usize,
}

#[derive(Debug, Clone, Default)]
enum State {
    #[default]
    Ground,
    Escape,
    Charset,
    Csi {
        parameters: String,
        overflowed: bool,
    },
    Osc {
        saw_escape: bool,
    },
    StringControl {
        saw_escape: bool,
    },
}

impl AnsiSanitizer {
    pub fn push(&mut self, input: &str) -> String {
        let mut output = String::new();
        for character in input.chars() {
            self.push_character(character, &mut output);
        }
        output
    }

    pub fn finish(&mut self) -> String {
        self.reset();
        String::new()
    }

    pub fn reset(&mut self) {
        self.state = State::Ground;
        self.column = 0;
    }

    fn push_character(&mut self, character: char, output: &mut String) {
        let state = mem::replace(&mut self.state, State::Ground);
        match state {
            State::Ground => self.push_ground(character, output),
            State::Escape => self.push_escape(character),
            State::Charset => self.push_charset(character),
            State::Csi {
                mut parameters,
                mut overflowed,
            } => self.push_csi(character, &mut parameters, &mut overflowed, output),
            State::Osc { saw_escape } => self.push_osc(character, saw_escape),
            State::StringControl { saw_escape } => self.push_string_control(character, saw_escape),
        }
    }

    fn push_ground(&mut self, character: char, output: &mut String) {
        match character {
            '\u{1b}' => self.state = State::Escape,
            '\u{009b}' => self.start_csi(),
            '\u{009d}' => self.state = State::Osc { saw_escape: false },
            '\u{0090}' | '\u{009f}' | '\u{009e}' | '\u{0098}' => {
                self.state = State::StringControl { saw_escape: false }
            }
            '\u{009c}' => {}
            '\n' => {
                output.push('\n');
                self.column = 0;
            }
            '\t' => {
                let spaces = 8 - (self.column % 8);
                output.push_str(&" ".repeat(spaces));
                self.column += spaces;
            }
            '\r' => {}
            character if is_c1_control(character) || character.is_control() => {}
            character => {
                output.push(character);
                self.column = self
                    .column
                    .saturating_add(UnicodeWidthChar::width(character).unwrap_or(0));
            }
        }
    }

    fn push_escape(&mut self, character: char) {
        match character {
            '\u{1b}' => self.state = State::Escape,
            '[' => self.start_csi(),
            ']' => self.state = State::Osc { saw_escape: false },
            'P' | '_' | '^' | 'X' => self.state = State::StringControl { saw_escape: false },
            '(' | ')' | '*' | '+' => self.state = State::Charset,
            _ => {}
        }
    }

    fn push_charset(&mut self, character: char) {
        if character == '\u{1b}' {
            self.state = State::Escape;
        }
    }

    fn start_csi(&mut self) {
        self.state = State::Csi {
            parameters: String::new(),
            overflowed: false,
        };
    }

    fn push_csi(
        &mut self,
        character: char,
        parameters: &mut String,
        overflowed: &mut bool,
        output: &mut String,
    ) {
        if character == '\u{1b}' {
            self.state = State::Escape;
            return;
        }

        if ('@'..='~').contains(&character) {
            if character == 'm' && !*overflowed {
                if let Some(sequence) = normalize_sgr(parameters) {
                    output.push_str(&sequence);
                }
            }
            return;
        }

        if (' '..='/').contains(&character) || ('0'..='?').contains(&character) {
            if parameters.len() < MAX_CSI_PARAMETER_BYTES {
                parameters.push(character);
            } else {
                *overflowed = true;
            }
            self.state = State::Csi {
                parameters: mem::take(parameters),
                overflowed: *overflowed,
            };
        }
    }

    fn push_osc(&mut self, character: char, saw_escape: bool) {
        if character == '\u{009c}' || character == '\u{0007}' {
            return;
        }
        if saw_escape && character == '\\' {
            return;
        }
        self.state = State::Osc {
            saw_escape: character == '\u{1b}',
        };
    }

    fn push_string_control(&mut self, character: char, saw_escape: bool) {
        if character == '\u{009c}' {
            return;
        }
        if saw_escape && character == '\\' {
            return;
        }
        self.state = State::StringControl {
            saw_escape: character == '\u{1b}',
        };
    }
}

pub fn sanitize_ansi(input: &str) -> String {
    let mut sanitizer = AnsiSanitizer::default();
    let mut output = sanitizer.push(input);
    output.push_str(&sanitizer.finish());
    output
}

pub fn sanitize_plain(input: &str) -> String {
    let sanitized = sanitize_ansi(input);
    plain_from_sanitized(&sanitized)
}

pub fn plain_from_sanitized(input: &str) -> String {
    parse_sanitized_ansi(input, Style::default())
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_sanitized_ansi(input: &str, base_style: Style) -> Vec<Line<'static>> {
    let mut lines = vec![Vec::new()];
    let mut overlay = Style::default();
    let mut text_start = 0;
    let mut index = 0;

    while index < input.len() {
        let character = input[index..]
            .chars()
            .next()
            .expect("index always points to a character boundary");
        if character == '\n' {
            push_text_span(&mut lines, &input[text_start..index], base_style, overlay);
            lines.push(Vec::new());
            index += character.len_utf8();
            text_start = index;
            continue;
        }

        if character == '\u{1b}' && input[index..].starts_with("\u{1b}[") {
            let Some(relative_end) = input[index + 2..].find('m') else {
                index += character.len_utf8();
                continue;
            };
            let end = index + 2 + relative_end;
            push_text_span(&mut lines, &input[text_start..index], base_style, overlay);
            apply_sgr(&input[index..=end], &mut overlay);
            index = end + 1;
            text_start = index;
            continue;
        }

        if character == '\u{1b}' {
            push_text_span(&mut lines, &input[text_start..index], base_style, overlay);
            index += character.len_utf8();
            text_start = index;
            continue;
        }

        index += character.len_utf8();
    }

    push_text_span(&mut lines, &input[text_start..], base_style, overlay);
    lines.into_iter().map(Line::from).collect()
}

fn push_text_span(
    lines: &mut [Vec<Span<'static>>],
    content: &str,
    base_style: Style,
    overlay: Style,
) {
    if content.is_empty() {
        return;
    }
    let mut style = base_style.patch(overlay);
    style.add_modifier.remove(Modifier::UNDERLINED);
    style.sub_modifier.remove(Modifier::UNDERLINED);
    let spans = lines.last_mut().expect("at least one line exists");
    if spans.last().is_some_and(|span| span.style == style) {
        spans
            .last_mut()
            .expect("span exists after checking")
            .content
            .to_mut()
            .push_str(content);
    } else {
        spans.push(Span::styled(content.to_string(), style));
    }
}

fn apply_sgr(sequence: &str, overlay: &mut Style) {
    let parameters = &sequence[2..sequence.len() - 1];
    let values = parameters
        .split(';')
        .filter_map(|value| value.parse::<u16>().ok())
        .collect::<Vec<_>>();
    let Some(parser_style) = parser_style_for_sgr(sequence) else {
        return;
    };

    let mut index = 0;
    while index < values.len() {
        match values[index] {
            0 => *overlay = Style::default(),
            1 => overlay.add_modifier.insert(Modifier::BOLD),
            2 => overlay.add_modifier.insert(Modifier::DIM),
            3 => overlay.add_modifier.insert(Modifier::ITALIC),
            7 => overlay.add_modifier.insert(Modifier::REVERSED),
            9 => overlay.add_modifier.insert(Modifier::CROSSED_OUT),
            22 => {
                overlay.add_modifier.remove(Modifier::BOLD | Modifier::DIM);
                overlay.sub_modifier.insert(Modifier::BOLD | Modifier::DIM);
            }
            23 => {
                overlay.add_modifier.remove(Modifier::ITALIC);
                overlay.sub_modifier.insert(Modifier::ITALIC);
            }
            27 => {
                overlay.add_modifier.remove(Modifier::REVERSED);
                overlay.sub_modifier.insert(Modifier::REVERSED);
            }
            29 => {
                overlay.add_modifier.remove(Modifier::CROSSED_OUT);
                overlay.sub_modifier.insert(Modifier::CROSSED_OUT);
            }
            30..=37 | 90..=97 => {
                if let Some(color) = named_foreground_color(values[index]) {
                    overlay.fg = Some(color);
                }
            }
            39 => overlay.fg = None,
            40..=47 | 100..=107 => {
                if let Some(color) = named_background_color(values[index]) {
                    overlay.bg = Some(color);
                }
            }
            49 => overlay.bg = None,
            38 => {
                if let Some(color) = parser_style.fg {
                    overlay.fg = (color != Color::Reset).then_some(color);
                }
                index += extended_color_parameter_count(&values, index);
            }
            48 => {
                if let Some(color) = parser_style.bg {
                    overlay.bg = (color != Color::Reset).then_some(color);
                }
                index += extended_color_parameter_count(&values, index);
            }
            _ => {}
        }
        index += 1;
    }
}

fn parser_style_for_sgr(sequence: &str) -> Option<Style> {
    let probe = format!("{sequence}x");
    let text = probe.as_bytes().into_text().ok()?;
    text.lines
        .first()
        .and_then(|line| line.spans.first())
        .map(|span| span.style)
}

fn extended_color_parameter_count(values: &[u16], index: usize) -> usize {
    match values.get(index + 1) {
        Some(5) => 2,
        Some(2) => 4,
        _ => 0,
    }
}

fn named_foreground_color(value: u16) -> Option<Color> {
    match value {
        30 => Some(Color::Black),
        31 => Some(Color::Red),
        32 => Some(Color::Green),
        33 => Some(Color::Yellow),
        34 => Some(Color::Blue),
        35 => Some(Color::Magenta),
        36 => Some(Color::Cyan),
        37 => Some(Color::Gray),
        90 => Some(Color::DarkGray),
        91 => Some(Color::LightRed),
        92 => Some(Color::LightGreen),
        93 => Some(Color::LightYellow),
        94 => Some(Color::LightBlue),
        95 => Some(Color::LightMagenta),
        96 => Some(Color::LightCyan),
        97 => Some(Color::White),
        _ => None,
    }
}

fn named_background_color(value: u16) -> Option<Color> {
    match value {
        40 => Some(Color::Black),
        41 => Some(Color::Red),
        42 => Some(Color::Green),
        43 => Some(Color::Yellow),
        44 => Some(Color::Blue),
        45 => Some(Color::Magenta),
        46 => Some(Color::Cyan),
        47 => Some(Color::Gray),
        100 => Some(Color::DarkGray),
        101 => Some(Color::LightRed),
        102 => Some(Color::LightGreen),
        103 => Some(Color::LightYellow),
        104 => Some(Color::LightBlue),
        105 => Some(Color::LightMagenta),
        106 => Some(Color::LightCyan),
        107 => Some(Color::White),
        _ => None,
    }
}

fn is_c1_control(character: char) -> bool {
    ('\u{0080}'..='\u{009f}').contains(&character)
}

fn normalize_sgr(parameters: &str) -> Option<String> {
    if parameters
        .chars()
        .any(|character| !character.is_ascii_digit() && character != ';')
    {
        return None;
    }

    let values = if parameters.is_empty() {
        vec![0]
    } else {
        parameters
            .split(';')
            .map(|value| {
                if value.is_empty() {
                    Some(0)
                } else {
                    value.parse::<u16>().ok()
                }
            })
            .collect::<Option<Vec<_>>>()?
    };

    let mut normalized = Vec::new();
    let mut index = 0;
    while index < values.len() {
        let value = values[index];
        match value {
            4 | 21 | 24 | 59 => index += 1,
            58 => match values.get(index + 1) {
                Some(5 | 2) => index = extended_underline_color_end(&values, index)?,
                _ => index += 1,
            },
            38 | 48 => {
                let end = extended_color_end(&values, index)?;
                normalized.extend_from_slice(&values[index..end]);
                index = end;
            }
            0 | 1 | 2 | 3 | 7 | 9 | 22 | 23 | 27 | 29 | 30..=37 | 39 | 40..=49 | 90..=107 => {
                normalized.push(value);
                index += 1;
            }
            _ => return None,
        }
    }

    if normalized.is_empty() {
        return Some(String::new());
    }

    Some(format!(
        "\u{1b}[{}m",
        normalized
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(";")
    ))
}

fn extended_color_end(values: &[u16], index: usize) -> Option<usize> {
    match values.get(index + 1) {
        Some(5) if values.get(index + 2).is_some_and(|value| *value <= 255) => Some(index + 3),
        Some(2)
            if values
                .get(index + 2..index + 5)
                .is_some_and(|values| values.iter().all(|value| *value <= 255)) =>
        {
            Some(index + 5)
        }
        _ => None,
    }
}

fn extended_underline_color_end(values: &[u16], index: usize) -> Option<usize> {
    match values.get(index + 1) {
        Some(5) if values.get(index + 2).is_some() => Some(index + 3),
        Some(2) if values.get(index + 2..index + 5).is_some() => Some(index + 5),
        _ => None,
    }
}
