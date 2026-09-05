use std::str::FromStr;
use std::sync::OnceLock;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color as SyntectColor, FontStyle as SyntectFontStyle, ScopeSelectors, Style as SyntectStyle,
    StyleModifier, Theme, ThemeItem, ThemeSettings,
};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::palette;

const SENTINEL_PLAIN: SyntectColor = SyntectColor {
    r: 1,
    g: 1,
    b: 1,
    a: 255,
};
const SENTINEL_BLUE: SyntectColor = SyntectColor {
    r: 2,
    g: 2,
    b: 2,
    a: 255,
};
const SENTINEL_CYAN: SyntectColor = SyntectColor {
    r: 3,
    g: 3,
    b: 3,
    a: 255,
};
const SENTINEL_TYPE: SyntectColor = SyntectColor {
    r: 4,
    g: 4,
    b: 4,
    a: 255,
};
const SENTINEL_GREEN: SyntectColor = SyntectColor {
    r: 5,
    g: 5,
    b: 5,
    a: 255,
};
const SENTINEL_RED: SyntectColor = SyntectColor {
    r: 6,
    g: 6,
    b: 6,
    a: 255,
};
const SENTINEL_YELLOW: SyntectColor = SyntectColor {
    r: 7,
    g: 7,
    b: 7,
    a: 255,
};
const SENTINEL_GRAY: SyntectColor = SyntectColor {
    r: 8,
    g: 8,
    b: 8,
    a: 255,
};
const SENTINEL_BACKGROUND: SyntectColor = SyntectColor {
    r: 9,
    g: 9,
    b: 9,
    a: 255,
};

pub(crate) fn assistant_markdown_lines(content: &str, base_style: Style) -> Vec<Line<'static>> {
    static HIGHLIGHTER: OnceLock<CodeHighlighter> = OnceLock::new();
    let highlighter = HIGHLIGHTER.get_or_init(CodeHighlighter::new);
    assistant_markdown_lines_with_highlighter(content, base_style, Some(highlighter))
}

pub(crate) fn assistant_markdown_lines_for_widths(
    content: &str,
    base_style: Style,
    table_width: usize,
    terminal_width: usize,
) -> Vec<Line<'static>> {
    static HIGHLIGHTER: OnceLock<CodeHighlighter> = OnceLock::new();
    let highlighter = HIGHLIGHTER.get_or_init(CodeHighlighter::new);
    assistant_markdown_lines_with_highlighter_and_widths(
        content,
        base_style,
        Some(highlighter),
        table_width,
        terminal_width,
    )
}

fn assistant_markdown_lines_with_highlighter(
    content: &str,
    base_style: Style,
    highlighter: Option<&CodeHighlighter>,
) -> Vec<Line<'static>> {
    assistant_markdown_lines_with_highlighter_and_widths(content, base_style, highlighter, 80, 80)
}

fn assistant_markdown_lines_with_highlighter_and_widths(
    content: &str,
    base_style: Style,
    highlighter: Option<&CodeHighlighter>,
    table_width: usize,
    terminal_width: usize,
) -> Vec<Line<'static>> {
    AssistantMarkdownRenderer::new(base_style, highlighter, table_width, terminal_width)
        .render(content)
}

struct AssistantMarkdownRenderer<'a> {
    lines: Vec<Line<'static>>,
    spans: Vec<Span<'static>>,
    styles: Vec<Style>,
    base_style: Style,
    list_stack: Vec<ListState>,
    blockquote_depth: usize,
    blockquote_needs_gap: bool,
    quote_prefix_emitted: bool,
    code_block: Option<CodeBlockState>,
    table: Option<MarkdownTable>,
    link_stack: Vec<LinkState>,
    suppressed_depth: usize,
    highlighter: Option<&'a CodeHighlighter>,
    table_width: usize,
    terminal_width: usize,
}

struct ListState {
    ordered: bool,
    next: u64,
}

struct CodeBlockState {
    language: Option<String>,
    content: String,
}

struct LinkState {
    href: String,
    image: bool,
}

struct MarkdownTable {
    alignments: Vec<Alignment>,
    rows: Vec<Vec<TableCell>>,
    current_row: Vec<TableCell>,
    current_cell: TableCell,
    header_rows: usize,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct TableCell {
    spans: Vec<Span<'static>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TableCluster {
    text: String,
    style: Style,
    width: usize,
}

type WrappedTableCell = Vec<Vec<TableCluster>>;

impl<'a> AssistantMarkdownRenderer<'a> {
    fn new(
        base_style: Style,
        highlighter: Option<&'a CodeHighlighter>,
        table_width: usize,
        terminal_width: usize,
    ) -> Self {
        Self {
            lines: Vec::new(),
            spans: Vec::new(),
            styles: vec![base_style],
            base_style,
            list_stack: Vec::new(),
            blockquote_depth: 0,
            blockquote_needs_gap: false,
            quote_prefix_emitted: false,
            code_block: None,
            table: None,
            link_stack: Vec::new(),
            suppressed_depth: 0,
            highlighter,
            table_width,
            terminal_width,
        }
    }

    fn render(mut self, content: &str) -> Vec<Line<'static>> {
        let parser = Parser::new_ext(content, Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES);
        for event in parser {
            self.push(event);
        }
        self.finish()
    }

    fn push(&mut self, event: Event<'_>) {
        if self.code_block.is_some() {
            match event {
                Event::Text(text) => {
                    if let Some(code_block) = &mut self.code_block {
                        code_block.content.push_str(&text);
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    if let Some(code_block) = &mut self.code_block {
                        code_block.content.push('\n');
                    }
                }
                Event::End(TagEnd::CodeBlock) => self.end_code_block(),
                _ => {}
            }
            return;
        }

        if self.suppressed_depth > 0 {
            match event {
                Event::Start(_) => self.suppressed_depth += 1,
                Event::End(_) => self.suppressed_depth = self.suppressed_depth.saturating_sub(1),
                _ => {}
            }
            return;
        }

        if self.table.is_some() {
            self.push_table_event(event);
            return;
        }

        if !self.link_stack.is_empty() {
            match event {
                Event::Start(Tag::Link { dest_url, .. }) => self.link_stack.push(LinkState {
                    href: sanitize_text(&dest_url),
                    image: false,
                }),
                Event::Start(Tag::Image { dest_url, .. }) => self.link_stack.push(LinkState {
                    href: sanitize_text(&dest_url),
                    image: true,
                }),
                Event::End(TagEnd::Link) | Event::End(TagEnd::Image) => self.end_link(),
                _ => {}
            }
            return;
        }

        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.push_text(&text),
            Event::Code(code) => self.push_inline_code(&code),
            Event::SoftBreak | Event::HardBreak => self.flush_line(true),
            Event::Rule => self.push_rule(),
            Event::TaskListMarker(_)
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::FootnoteReference(_) => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Table(alignments) => {
                self.flush_line(false);
                self.table = Some(MarkdownTable {
                    alignments,
                    rows: Vec::new(),
                    current_row: Vec::new(),
                    current_cell: TableCell::default(),
                    header_rows: 0,
                });
            }
            Tag::TableHead | Tag::TableRow | Tag::TableCell => {}
            Tag::Paragraph => {
                if self.blockquote_depth > 0 && self.list_stack.is_empty() {
                    if self.blockquote_needs_gap {
                        self.lines.push(Line::default());
                    }
                    self.blockquote_needs_gap = false;
                }
            }
            Tag::Heading { level, .. } => {
                let modifiers = match level {
                    HeadingLevel::H1 => Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED,
                    HeadingLevel::H2
                    | HeadingLevel::H3
                    | HeadingLevel::H4
                    | HeadingLevel::H5
                    | HeadingLevel::H6 => Modifier::BOLD,
                };
                self.push_style(self.style().add_modifier(modifiers));
            }
            Tag::Strong => self.push_style(self.style().add_modifier(Modifier::BOLD)),
            Tag::Emphasis => self.push_style(self.style().add_modifier(Modifier::ITALIC)),
            Tag::BlockQuote(_) => {
                self.flush_line(false);
                self.blockquote_depth += 1;
                self.blockquote_needs_gap = false;
                self.push_style(self.style().add_modifier(Modifier::ITALIC));
            }
            Tag::List(start) => {
                self.flush_line(false);
                self.blockquote_needs_gap = false;
                self.list_stack.push(ListState {
                    ordered: start.is_some(),
                    next: start.unwrap_or(1),
                });
            }
            Tag::Item => {
                let marker = self.list_marker();
                let style = self.style();
                self.push_text_with_style(&marker, style);
            }
            Tag::CodeBlock(kind) => {
                self.flush_line(false);
                let language = match kind {
                    CodeBlockKind::Fenced(info) => Some(info.into_string()),
                    CodeBlockKind::Indented => None,
                };
                self.code_block = Some(CodeBlockState {
                    language,
                    content: String::new(),
                });
            }
            Tag::Link { dest_url, .. } => self.link_stack.push(LinkState {
                href: sanitize_text(&dest_url),
                image: false,
            }),
            Tag::Image { dest_url, .. } => self.link_stack.push(LinkState {
                href: sanitize_text(&dest_url),
                image: true,
            }),
            Tag::HtmlBlock
            | Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::MetadataBlock(_) => self.suppressed_depth = 1,
            Tag::Strikethrough | Tag::Superscript | Tag::Subscript => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::TableCell => {
                if let Some(table) = &mut self.table {
                    table
                        .current_row
                        .push(std::mem::take(&mut table.current_cell));
                }
            }
            TagEnd::TableRow => {
                if let Some(table) = &mut self.table {
                    table.rows.push(std::mem::take(&mut table.current_row));
                }
            }
            TagEnd::TableHead => {
                if let Some(table) = &mut self.table {
                    if !table.current_row.is_empty() {
                        table.rows.push(std::mem::take(&mut table.current_row));
                    }
                    table.header_rows = table.rows.len();
                }
            }
            TagEnd::Table => self.render_table(),
            TagEnd::Paragraph => {
                self.flush_line(false);
                self.blockquote_needs_gap = self.blockquote_depth > 0 && self.list_stack.is_empty();
            }
            TagEnd::Item => self.flush_line(false),
            TagEnd::Heading(_) => {
                self.flush_line(false);
                self.pop_style();
                self.lines.push(Line::default());
            }
            TagEnd::Strong | TagEnd::Emphasis => self.pop_style(),
            TagEnd::BlockQuote(_) => {
                self.flush_line(false);
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                self.blockquote_needs_gap = false;
                self.pop_style();
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
            }
            TagEnd::HtmlBlock
            | TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::MetadataBlock(_) => {}
            TagEnd::CodeBlock => {}
            TagEnd::Link | TagEnd::Image => {}
            TagEnd::Strikethrough | TagEnd::Superscript | TagEnd::Subscript => {}
        }
    }

    fn push_table_event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(Tag::Strong) => self.push_style(self.style().add_modifier(Modifier::BOLD)),
            Event::Start(Tag::Emphasis) => {
                self.push_style(self.style().add_modifier(Modifier::ITALIC))
            }
            Event::Start(_) => {}
            Event::End(TagEnd::TableCell) => {
                if let Some(table) = &mut self.table {
                    table
                        .current_row
                        .push(std::mem::take(&mut table.current_cell));
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(table) = &mut self.table {
                    table.rows.push(std::mem::take(&mut table.current_row));
                }
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(table) = &mut self.table {
                    if !table.current_row.is_empty() {
                        table.rows.push(std::mem::take(&mut table.current_row));
                    }
                    table.header_rows = table.rows.len();
                }
            }
            Event::End(TagEnd::Table) => self.render_table(),
            Event::Text(text) => self.push_table_text(&text),
            Event::Code(code) => {
                let mut style = self.style();
                style.fg = Some(palette::PERMISSION);
                style.bg = None;
                self.push_table_text_with_style(&sanitize_text(&code), style);
            }
            Event::SoftBreak => self.push_table_text(" "),
            Event::HardBreak => self.push_table_text("\n"),
            Event::End(TagEnd::Strong) | Event::End(TagEnd::Emphasis) => self.pop_style(),
            _ => {}
        }
    }

    fn push_text(&mut self, text: &str) {
        if self.link_stack.is_empty() {
            for (index, line) in text.split('\n').enumerate() {
                if index > 0 {
                    self.flush_line(true);
                }
                if !line.is_empty() {
                    self.push_linkified_text(line);
                }
            }
        }
    }

    fn push_linkified_text(&mut self, text: &str) {
        let text = sanitize_text(text);
        let mut cursor = 0;
        let mut scan = 0;
        while scan < text.len() {
            let Some(end) = issue_reference_end(&text, scan) else {
                scan += text[scan..]
                    .chars()
                    .next()
                    .expect("scan is below text length")
                    .len_utf8();
                continue;
            };
            if cursor < scan {
                let plain = text[cursor..scan].to_string();
                self.push_text_with_style(&plain, self.style());
            }
            let reference = &text[scan..end];
            let Some(hash) = reference.rfind('#') else {
                unreachable!();
            };
            let path = &reference[..hash];
            let issue = &reference[hash + 1..];
            let url = format!("https://github.com/{path}/issues/{issue}");
            self.push_text_with_style(&url, self.style());
            cursor = end;
            scan = end;
        }
        if cursor < text.len() {
            let plain = text[cursor..].to_string();
            self.push_text_with_style(&plain, self.style());
        }
    }

    fn push_inline_code(&mut self, code: &str) {
        let mut style = self.style();
        style.fg = Some(palette::PERMISSION);
        style.bg = None;
        self.push_text_with_style(&sanitize_text(code), style);
    }

    fn push_rule(&mut self) {
        self.push_text_with_style("---", Style::default());
        self.flush_line(false);
    }

    fn list_marker(&mut self) -> String {
        let depth = self.list_stack.len().saturating_sub(1);
        let Some(list) = self.list_stack.last_mut() else {
            return "- ".to_string();
        };
        if !list.ordered {
            return format!("{}- ", "  ".repeat(depth));
        }
        let number = list.next;
        list.next = list.next.saturating_add(1);
        let label = match depth {
            0 | 1 | 4.. => number.to_string(),
            2 => number_to_letters(number),
            3 => number_to_roman(number),
        };
        format!("{}{label}. ", "  ".repeat(depth))
    }

    fn end_link(&mut self) {
        let Some(link) = self.link_stack.pop() else {
            return;
        };
        let href = link.href.strip_prefix("mailto:").unwrap_or(&link.href);
        let style = if link.image {
            Style::default()
        } else {
            self.style()
        };
        self.push_text_with_style(href, style);
    }

    fn end_code_block(&mut self) {
        let Some(code_block) = self.code_block.take() else {
            return;
        };
        let lines = self
            .highlighter
            .map(|highlighter| {
                highlighter.highlight_code(
                    &code_block.content,
                    code_block.language.as_deref(),
                    self.base_style,
                )
            })
            .unwrap_or_else(|| plain_code_lines(&code_block.content, self.base_style));
        self.append_code_lines(lines);
    }

    fn append_code_lines(&mut self, lines: Vec<Line<'static>>) {
        for line in lines {
            let is_blank = line.to_string().trim().is_empty();
            if is_blank || self.blockquote_depth == 0 {
                self.lines.push(line);
                continue;
            }
            let mut spans = vec![Span::styled(
                "▎ ",
                self.base_style.add_modifier(Modifier::DIM),
            )];
            spans.extend(line.spans);
            self.lines.push(Line::from(spans));
        }
    }

    fn push_table_text(&mut self, text: &str) {
        let style = self.style();
        self.push_table_text_with_style(&sanitize_text(text), style);
    }

    fn push_table_text_with_style(&mut self, text: &str, style: Style) {
        if let Some(table) = &mut self.table {
            push_table_span(&mut table.current_cell.spans, text, style);
        }
    }

    fn render_table(&mut self) {
        let Some(table) = self.table.take() else {
            return;
        };
        self.lines.extend(render_assistant_table(
            &table,
            self.table_width,
            self.terminal_width,
        ));
    }

    fn push_text_with_style(&mut self, text: &str, style: Style) {
        if text.is_empty() {
            return;
        }
        self.ensure_quote_prefix();
        self.spans.push(Span::styled(text.to_string(), style));
    }

    fn ensure_quote_prefix(&mut self) {
        if self.blockquote_depth > 0 && !self.quote_prefix_emitted {
            self.spans.push(Span::styled(
                "▎ ".repeat(self.blockquote_depth),
                self.base_style.add_modifier(Modifier::DIM),
            ));
            self.quote_prefix_emitted = true;
        }
    }

    fn style(&self) -> Style {
        *self
            .styles
            .last()
            .expect("assistant Markdown style is present")
    }

    fn push_style(&mut self, style: Style) {
        self.styles.push(style);
    }

    fn pop_style(&mut self) {
        if self.styles.len() > 1 {
            self.styles.pop();
        }
    }

    fn flush_line(&mut self, force: bool) {
        if self.spans.is_empty() {
            if force {
                self.lines.push(Line::default());
            }
        } else {
            self.lines.push(Line::from(std::mem::take(&mut self.spans)));
        }
        self.quote_prefix_emitted = false;
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_line(false);
        self.lines
    }
}

const MIN_TABLE_COLUMN_WIDTH: usize = 3;
const TABLE_SAFETY_MARGIN: usize = 4;
const TABLE_BORDER_OVERHEAD: usize = 1;
const MAX_TABLE_ROW_LINES: usize = 4;

fn push_table_span(spans: &mut Vec<Span<'static>>, text: &str, style: Style) {
    if text.is_empty() {
        return;
    }
    if spans.last().is_some_and(|span| span.style == style) {
        spans
            .last_mut()
            .expect("table span exists")
            .content
            .to_mut()
            .push_str(text);
    } else {
        spans.push(Span::styled(text.to_string(), style));
    }
}

fn render_assistant_table(
    table: &MarkdownTable,
    table_width: usize,
    terminal_width: usize,
) -> Vec<Line<'static>> {
    let column_count = table.rows.iter().map(Vec::len).max().unwrap_or(0);
    if column_count == 0 || table.rows.is_empty() {
        return Vec::new();
    }

    let (ideal_widths, minimum_widths) = table_column_widths(table, column_count);
    let Some(widths) = allocate_table_widths(&ideal_widths, &minimum_widths, table_width) else {
        return render_vertical_table(table, table_width, terminal_width, column_count);
    };
    let wrapped_rows = table
        .rows
        .iter()
        .map(|row| {
            (0..column_count)
                .map(|column| {
                    row.get(column)
                        .map(|cell| wrap_table_cell(cell, widths[column]))
                        .unwrap_or_else(|| wrap_table_cell(&TableCell::default(), widths[column]))
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let row_too_tall = wrapped_rows
        .iter()
        .any(|row| row.iter().map(Vec::len).max().unwrap_or_default() > MAX_TABLE_ROW_LINES);
    let normal = render_normal_table(table, &widths, &wrapped_rows, column_count);
    let available_width = table_width.saturating_sub(TABLE_SAFETY_MARGIN);
    let normal_too_wide = normal.iter().any(|line| line.width() > available_width);

    if row_too_tall || normal_too_wide {
        render_vertical_table(table, table_width, terminal_width, column_count)
    } else {
        normal
    }
}

fn table_column_widths(table: &MarkdownTable, column_count: usize) -> (Vec<usize>, Vec<usize>) {
    let mut ideal = Vec::with_capacity(column_count);
    let mut minimum = Vec::with_capacity(column_count);
    for column in 0..column_count {
        let mut ideal_width = 0;
        let mut longest_word = 0;
        for cell in table.rows.iter().filter_map(|row| row.get(column)) {
            let lines = table_cell_lines(cell);
            ideal_width = ideal_width.max(
                lines
                    .iter()
                    .map(|line| line.iter().map(|cluster| cluster.width).sum())
                    .max()
                    .unwrap_or_default(),
            );
            longest_word = longest_word.max(
                lines
                    .iter()
                    .flat_map(|line| table_word_tokens(line))
                    .map(|word| word.iter().map(|cluster| cluster.width).sum())
                    .max()
                    .unwrap_or_default(),
            );
        }
        ideal.push(ideal_width.max(MIN_TABLE_COLUMN_WIDTH));
        minimum.push(longest_word.max(MIN_TABLE_COLUMN_WIDTH));
    }
    (ideal, minimum)
}

fn table_content_budget(terminal_width: usize, column_count: usize) -> usize {
    terminal_width.saturating_sub(
        TABLE_SAFETY_MARGIN + TABLE_BORDER_OVERHEAD + column_count * MIN_TABLE_COLUMN_WIDTH,
    )
}

fn allocate_table_widths(
    ideal_widths: &[usize],
    minimum_widths: &[usize],
    terminal_width: usize,
) -> Option<Vec<usize>> {
    let column_count = ideal_widths.len();
    let budget = table_content_budget(terminal_width, column_count);
    let minimum_total = column_count * MIN_TABLE_COLUMN_WIDTH;
    if budget < minimum_total {
        return None;
    }

    let ideal_total = ideal_widths.iter().sum::<usize>();
    if ideal_total <= budget {
        return Some(ideal_widths.to_vec());
    }

    let minimum_total = minimum_widths.iter().sum::<usize>();
    if minimum_total <= budget {
        let mut widths = minimum_widths.to_vec();
        let weights = ideal_widths
            .iter()
            .zip(minimum_widths)
            .map(|(ideal, minimum)| ideal.saturating_sub(*minimum))
            .collect::<Vec<_>>();
        distribute_table_slack(&mut widths, &weights, budget - minimum_total);
        return Some(widths);
    }

    let mut widths = vec![MIN_TABLE_COLUMN_WIDTH; column_count];
    let weights = minimum_widths
        .iter()
        .map(|minimum| minimum.saturating_sub(MIN_TABLE_COLUMN_WIDTH))
        .collect::<Vec<_>>();
    distribute_table_slack(
        &mut widths,
        &weights,
        budget - column_count * MIN_TABLE_COLUMN_WIDTH,
    );
    Some(widths)
}

fn distribute_table_slack(widths: &mut [usize], weights: &[usize], slack: usize) {
    if slack == 0 || widths.is_empty() {
        return;
    }
    let total_weight = weights.iter().sum::<usize>();
    if total_weight == 0 {
        let share = slack / widths.len();
        let remainder = slack % widths.len();
        for (index, width) in widths.iter_mut().enumerate() {
            *width += share + usize::from(index < remainder);
        }
        return;
    }

    let mut remainders = Vec::with_capacity(widths.len());
    let mut distributed = 0;
    for (index, weight) in weights.iter().copied().enumerate() {
        let numerator = (slack as u128) * (weight as u128);
        let share = (numerator / total_weight as u128) as usize;
        let remainder = numerator % total_weight as u128;
        widths[index] += share;
        distributed += share;
        remainders.push((remainder, index));
    }

    remainders.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    for (_, index) in remainders.into_iter().take(slack - distributed) {
        widths[index] += 1;
    }
}

fn render_normal_table(
    table: &MarkdownTable,
    widths: &[usize],
    wrapped_rows: &[Vec<WrappedTableCell>],
    column_count: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![table_border(widths, '┌', '┬', '┐')];
    for (row_index, row) in wrapped_rows.iter().enumerate() {
        let row_height = row.iter().map(Vec::len).max().unwrap_or(1);
        for line_index in 0..row_height {
            let mut spans = vec![Span::raw("│")];
            for column in 0..column_count {
                let cell = &row[column];
                let top_padding = (row_height - cell.len()) / 2;
                let cell_line = line_index
                    .checked_sub(top_padding)
                    .and_then(|index| cell.get(index))
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let alignment = if row_index < table.header_rows {
                    Alignment::Center
                } else {
                    table
                        .alignments
                        .get(column)
                        .copied()
                        .unwrap_or(Alignment::None)
                };
                append_aligned_cell(&mut spans, cell_line, widths[column], alignment);
                spans.push(Span::raw("│"));
            }
            lines.push(Line::from(spans));
        }
        if row_index + 1 < wrapped_rows.len() {
            lines.push(table_border(widths, '├', '┼', '┤'));
        }
    }
    lines.push(table_border(widths, '└', '┴', '┘'));
    lines
}

fn table_border(widths: &[usize], left: char, junction: char, right: char) -> Line<'static> {
    let mut spans = vec![Span::raw(left.to_string())];
    for (index, width) in widths.iter().copied().enumerate() {
        spans.push(Span::raw("─".repeat(width + 2)));
        spans.push(Span::raw(if index + 1 == widths.len() {
            right.to_string()
        } else {
            junction.to_string()
        }));
    }
    Line::from(spans)
}

fn append_aligned_cell(
    spans: &mut Vec<Span<'static>>,
    cell_line: &[TableCluster],
    width: usize,
    alignment: Alignment,
) {
    spans.push(Span::raw(" "));
    let content_width = cell_line.iter().map(|cluster| cluster.width).sum::<usize>();
    let padding = width.saturating_sub(content_width);
    let (left, right) = match alignment {
        Alignment::Right => (padding, 0),
        Alignment::Center => (padding / 2, padding - padding / 2),
        Alignment::None | Alignment::Left => (0, padding),
    };
    if left > 0 {
        spans.push(Span::raw(" ".repeat(left)));
    }
    append_table_clusters(spans, cell_line);
    if right > 0 {
        spans.push(Span::raw(" ".repeat(right)));
    }
    spans.push(Span::raw(" "));
}

fn append_table_clusters(spans: &mut Vec<Span<'static>>, clusters: &[TableCluster]) {
    for cluster in clusters {
        push_table_span(spans, &cluster.text, cluster.style);
    }
}

fn render_vertical_table(
    table: &MarkdownTable,
    table_width: usize,
    terminal_width: usize,
    column_count: usize,
) -> Vec<Line<'static>> {
    let Some(headers) = table.rows.first() else {
        return Vec::new();
    };
    let data_rows = table
        .rows
        .iter()
        .skip(table.header_rows)
        .collect::<Vec<_>>();
    let separator_width = if table_width < terminal_width {
        table_width
    } else {
        terminal_width.saturating_sub(1)
    }
    .min(40);
    let mut lines = Vec::new();

    for (row_index, row) in data_rows.iter().enumerate() {
        for column in 0..column_count {
            let header = headers.get(column).cloned().unwrap_or_default();
            let label_width = table_width.saturating_sub(3).max(1);
            let label_lines = wrap_table_label(&header, label_width);
            let value = row.get(column).cloned().unwrap_or_default();
            let value_width = table_width
                .saturating_sub(
                    label_lines
                        .iter()
                        .map(|line| line.iter().map(|cluster| cluster.width).sum::<usize>())
                        .max()
                        .unwrap_or_default()
                        .saturating_add(2),
                )
                .max(1);
            let value_lines = wrap_table_cell(&value, value_width);
            for (line_index, label_line) in label_lines.iter().enumerate() {
                let mut spans = Vec::new();
                append_bold_table_clusters(&mut spans, label_line);
                if line_index + 1 == label_lines.len() {
                    spans.push(Span::raw(": "));
                    append_table_clusters(&mut spans, &value_lines[0]);
                }
                lines.push(Line::from(spans));
            }
            for value_line in value_lines.iter().skip(1) {
                let mut spans = vec![Span::raw("  ")];
                append_table_clusters(&mut spans, value_line);
                lines.push(Line::from(spans));
            }
        }
        if row_index + 1 < data_rows.len() {
            lines.push(Line::from("─".repeat(separator_width)));
        }
    }
    lines
}

fn append_bold_table_clusters(spans: &mut Vec<Span<'static>>, clusters: &[TableCluster]) {
    for cluster in clusters {
        push_table_span(
            spans,
            &cluster.text,
            cluster.style.add_modifier(Modifier::BOLD),
        );
    }
}

fn wrap_table_label(cell: &TableCell, width: usize) -> WrappedTableCell {
    let clusters = table_clusters(&cell.spans)
        .into_iter()
        .map(|cluster| {
            if cluster.text == "\n" {
                TableCluster {
                    text: " ".to_string(),
                    style: cluster.style,
                    width: 1,
                }
            } else {
                cluster
            }
        })
        .collect::<Vec<_>>();
    wrap_table_line(&clusters, width)
}

fn table_cell_lines(cell: &TableCell) -> Vec<Vec<TableCluster>> {
    let mut lines = vec![Vec::new()];
    for cluster in table_clusters(&cell.spans) {
        if cluster.text == "\n" {
            lines.push(Vec::new());
        } else {
            lines
                .last_mut()
                .expect("table cell always has a line")
                .push(cluster);
        }
    }
    lines
}

fn table_clusters(spans: &[Span<'static>]) -> Vec<TableCluster> {
    let mut text = String::new();
    let mut styles = Vec::new();
    let mut byte_offset = 0;
    for span in spans {
        for character in span.content.chars() {
            styles.push((byte_offset, span.style));
            byte_offset += character.len_utf8();
            text.push(character);
        }
    }

    let mut display_column = 0;
    text.grapheme_indices(true)
        .flat_map(|(start, grapheme)| {
            let style = styles
                .iter()
                .rev()
                .find(|(offset, _)| *offset <= start)
                .map(|(_, style)| *style)
                .unwrap_or_default();
            let normalized = grapheme.nfc().collect::<String>();
            if normalized == "\n" {
                display_column = 0;
                return vec![TableCluster {
                    text: normalized,
                    style,
                    width: 0,
                }];
            }
            if normalized == "\t" {
                let spaces = 8 - (display_column % 8);
                display_column += spaces;
                return (0..spaces)
                    .map(|_| TableCluster {
                        text: " ".to_string(),
                        style,
                        width: 1,
                    })
                    .collect::<Vec<_>>();
            }
            let width = UnicodeWidthStr::width(normalized.as_str());
            display_column += width;
            vec![TableCluster {
                text: normalized,
                style,
                width,
            }]
        })
        .collect()
}

fn wrap_table_cell(cell: &TableCell, width: usize) -> WrappedTableCell {
    let width = width.max(1);
    table_cell_lines(cell)
        .into_iter()
        .flat_map(|line| wrap_table_line(&line, width))
        .collect()
}

fn wrap_table_line(line: &[TableCluster], width: usize) -> WrappedTableCell {
    if line.is_empty() {
        return vec![Vec::new()];
    }
    let mut rows = vec![Vec::new()];
    let mut column = 0;
    for word in table_word_tokens(line) {
        let word_width = word.iter().map(|cluster| cluster.width).sum::<usize>();
        if column > 0 && column + word_width > width {
            rows.push(Vec::new());
            column = 0;
        }
        for cluster in word {
            if column > 0 && column + cluster.width > width {
                rows.push(Vec::new());
                column = 0;
            }
            rows.last_mut()
                .expect("table row always exists")
                .push(cluster.clone());
            column += cluster.width;
        }
    }
    rows
}

fn table_word_tokens(line: &[TableCluster]) -> Vec<Vec<TableCluster>> {
    let mut tokens = Vec::new();
    let mut token = Vec::new();
    for cluster in line {
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

struct CodeHighlighter {
    syntax_set: SyntaxSet,
    theme: Theme,
}

impl CodeHighlighter {
    fn new() -> Self {
        Self {
            syntax_set: two_face::syntax::extra_newlines(),
            theme: ansi_role_theme(),
        }
    }

    fn syntax_for(&self, language: Option<&str>) -> Option<&SyntaxReference> {
        let language = language?.split_whitespace().next()?;
        (!language.is_empty()).then(|| self.syntax_set.find_syntax_by_token(language))?
    }

    fn highlight_code(
        &self,
        code: &str,
        language: Option<&str>,
        base_style: Style,
    ) -> Vec<Line<'static>> {
        let body = code_body_without_parser_newline(code);
        let Some(syntax) = self.syntax_for(language) else {
            return plain_code_lines(body, base_style);
        };
        let mut highlighter = HighlightLines::new(syntax, &self.theme);
        let mut lines = Vec::new();
        for source_line in body.split('\n') {
            let source_line = source_line.strip_suffix('\r').unwrap_or(source_line);
            let line_with_newline = format!("{source_line}\n");
            let Ok(ranges) = highlighter.highlight_line(&line_with_newline, &self.syntax_set)
            else {
                return plain_code_lines(body, base_style);
            };
            let mut spans = Vec::new();
            for (style, text) in ranges {
                let text = text.trim_end_matches(['\r', '\n']);
                let text = sanitize_text(text);
                if !text.is_empty() {
                    spans.push(Span::styled(text, ratatui_style(style, base_style)));
                }
            }
            lines.push(Line::from(spans));
        }
        lines
    }
}

fn plain_code_lines(code: &str, base_style: Style) -> Vec<Line<'static>> {
    let body = code_body_without_parser_newline(code);
    body.split('\n')
        .map(|line| {
            let line = sanitize_text(line.strip_suffix('\r').unwrap_or(line));
            if line.is_empty() {
                Line::default()
            } else {
                Line::from(Span::styled(line, code_base_style(base_style)))
            }
        })
        .collect()
}

fn code_body_without_parser_newline(code: &str) -> &str {
    let code = code.strip_suffix('\n').unwrap_or(code);
    code.strip_suffix('\r').unwrap_or(code)
}

fn code_base_style(mut base_style: Style) -> Style {
    base_style.bg = None;
    base_style
}

fn ratatui_style(style: SyntectStyle, base_style: Style) -> Style {
    let mut result = code_base_style(base_style);
    result.fg = match style.foreground {
        SENTINEL_BLUE => Some(Color::Blue),
        SENTINEL_CYAN | SENTINEL_TYPE => Some(Color::Cyan),
        SENTINEL_GREEN => Some(Color::Green),
        SENTINEL_RED => Some(Color::Red),
        SENTINEL_YELLOW => Some(Color::Yellow),
        SENTINEL_GRAY => Some(Color::Gray),
        SENTINEL_PLAIN => base_style.fg,
        _ => base_style.fg,
    };
    if style.foreground == SENTINEL_TYPE {
        result.add_modifier.insert(Modifier::DIM);
    }
    if style.font_style.contains(SyntectFontStyle::BOLD) {
        result.add_modifier.insert(Modifier::BOLD);
    }
    if style.font_style.contains(SyntectFontStyle::ITALIC) {
        result.add_modifier.insert(Modifier::ITALIC);
    }
    if style.font_style.contains(SyntectFontStyle::UNDERLINE) {
        result.add_modifier.insert(Modifier::UNDERLINED);
    }
    result
}

fn ansi_role_theme() -> Theme {
    Theme {
        name: Some("picopilot ANSI roles".to_string()),
        author: None,
        settings: ThemeSettings {
            foreground: Some(SENTINEL_PLAIN),
            background: Some(SENTINEL_BACKGROUND),
            ..ThemeSettings::default()
        },
        scopes: vec![
            color_scope(
                "keyword, literal, class, name, storage, constant.language, entity.name.class, entity.name.namespace, entity.name.label, entity.name.constant, variable.language",
                SENTINEL_BLUE,
            ),
            color_scope(
                "built_in, built-in, attribute, support, entity.other.attribute-name",
                SENTINEL_CYAN,
            ),
            color_scope(
                "type, support.type, storage.type, entity.name.type",
                SENTINEL_TYPE,
            ),
            color_scope(
                "number, comment, doctag, addition, constant.numeric, markup.inserted, comment.documentation",
                SENTINEL_GREEN,
            ),
            color_scope(
                "string, regexp, deletion, constant.character, string.regexp, markup.deleted",
                SENTINEL_RED,
            ),
            color_scope("function, entity.name.function, variable.function", SENTINEL_YELLOW),
            color_scope(
                "meta, tag, punctuation.definition.tag, entity.name.tag, entity.tag",
                SENTINEL_GRAY,
            ),
            modifier_scope("emphasis, markup.italic", SyntectFontStyle::ITALIC),
            modifier_scope("strong, markup.bold", SyntectFontStyle::BOLD),
            modifier_scope("link, markup.underline", SyntectFontStyle::UNDERLINE),
        ],
    }
}

fn color_scope(scope: &str, color: SyntectColor) -> ThemeItem {
    ThemeItem {
        scope: ScopeSelectors::from_str(scope).expect("ANSI role scope is valid"),
        style: StyleModifier {
            foreground: Some(color),
            ..StyleModifier::default()
        },
    }
}

fn modifier_scope(scope: &str, font_style: SyntectFontStyle) -> ThemeItem {
    ThemeItem {
        scope: ScopeSelectors::from_str(scope).expect("ANSI modifier scope is valid"),
        style: StyleModifier {
            font_style: Some(font_style),
            ..StyleModifier::default()
        },
    }
}

fn number_to_letters(mut number: u64) -> String {
    if number == 0 {
        return "0".to_string();
    }
    let mut result = String::new();
    while number > 0 {
        number -= 1;
        result.push((b'a' + (number % 26) as u8) as char);
        number /= 26;
    }
    result.chars().rev().collect()
}

fn number_to_roman(mut number: u64) -> String {
    if number == 0 {
        return "0".to_string();
    }
    let symbols = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut result = String::new();
    for (value, symbol) in symbols {
        while number >= value {
            number -= value;
            result.push_str(symbol);
        }
    }
    result
}

fn sanitize_text(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_control())
        .collect()
}

fn issue_reference_end(text: &str, start: usize) -> Option<usize> {
    if start > 0 {
        let previous = text[..start].chars().next_back()?;
        if previous.is_ascii_alphanumeric() || matches!(previous, '-' | '_' | '.' | '/') {
            return None;
        }
    }
    let owner_end = take_issue_name(text, start)?;
    if text.as_bytes().get(owner_end) != Some(&b'/') {
        return None;
    }
    let repository_start = owner_end + 1;
    let repository_end = take_issue_name(text, repository_start)?;
    if text.as_bytes().get(repository_end) != Some(&b'#') {
        return None;
    }
    let issue_start = repository_end + 1;
    let issue_end = issue_start
        + text[issue_start..]
            .chars()
            .take_while(char::is_ascii_digit)
            .map(char::len_utf8)
            .sum::<usize>();
    if issue_end == issue_start {
        return None;
    }
    if text[issue_end..]
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return None;
    }
    Some(issue_end)
}

fn take_issue_name(text: &str, start: usize) -> Option<usize> {
    let end = start
        + text[start..]
            .chars()
            .take_while(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
            })
            .map(char::len_utf8)
            .sum::<usize>();
    (end > start).then_some(end)
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier, Style};
    use unicode_width::UnicodeWidthStr;

    use super::{
        allocate_table_widths, assistant_markdown_lines_with_highlighter,
        assistant_markdown_lines_with_highlighter_and_widths, table_content_budget,
        wrap_table_cell, CodeHighlighter, TableCell,
    };

    fn lines(content: &str) -> Vec<ratatui::text::Line<'static>> {
        assistant_markdown_lines_with_highlighter(content, Style::default().fg(Color::White), None)
    }

    fn lines_at_width(content: &str, width: usize) -> Vec<ratatui::text::Line<'static>> {
        assistant_markdown_lines_with_highlighter_and_widths(
            content,
            Style::default().fg(Color::White),
            None,
            width,
            width,
        )
    }

    #[test]
    fn assistant_headings_have_level_specific_modifiers_and_spacing() {
        let rendered = lines("# One\n\n## Two\n\n###### Six");
        assert_eq!(
            rendered.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["One", "", "Two", "", "Six", ""]
        );
        let h1 = rendered[0].spans[0].style;
        assert_eq!(h1.fg, Some(Color::White));
        assert!(h1.add_modifier.contains(Modifier::BOLD));
        assert!(h1.add_modifier.contains(Modifier::ITALIC));
        assert!(h1.add_modifier.contains(Modifier::UNDERLINED));
        assert!(rendered[2].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
        assert!(!rendered[2].spans[0]
            .style
            .add_modifier
            .contains(Modifier::ITALIC));
    }

    #[test]
    fn nested_strong_and_emphasis_combine_modifiers() {
        let rendered = lines("**bold _and italic_**");
        let style = rendered[0].spans[1].style;
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert!(style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn strikethrough_is_literal_and_inline_code_uses_permission_without_background() {
        let rendered = lines("~~literal~~ and `code`");
        assert_eq!(rendered[0].to_string(), "~~literal~~ and code");
        let code = rendered[0]
            .spans
            .iter()
            .find(|span| span.content.as_ref() == "code")
            .expect("inline code span");
        assert_eq!(code.style.fg, Some(crate::palette::PERMISSION));
        assert_eq!(code.style.bg, None);
    }

    #[test]
    fn disabled_highlighting_uses_plaintext() {
        let rendered = lines("```rust\nfn main() {}\n```");
        assert_eq!(rendered[0].to_string(), "fn main() {}");
        assert_eq!(rendered[0].spans.len(), 1);
        assert_eq!(rendered[0].spans[0].style.fg, Some(Color::White));
    }

    #[test]
    fn known_grammars_resolve_with_oniguruma_assets() {
        let highlighter = CodeHighlighter::new();
        for language in ["rust", "typescript", "toml", "dockerfile", "powershell"] {
            assert!(
                highlighter.syntax_for(Some(language)).is_some(),
                "grammar should resolve for {language}"
            );
        }
    }

    #[test]
    fn blockquotes_prefix_nonblank_lines_and_keep_blank_lines_unmarked() {
        let rendered = lines("> first\n>\n> third");
        assert_eq!(
            rendered.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["▎ first", "", "▎ third"]
        );
        let bar = rendered[0].spans[0].style;
        assert!(bar.add_modifier.contains(Modifier::DIM));
        assert!(!bar.add_modifier.contains(Modifier::ITALIC));
        let text = rendered[0].spans[1].style;
        assert!(text.add_modifier.contains(Modifier::ITALIC));
        assert!(!text.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn lists_use_plain_hyphens_and_depth_specific_ordered_markers() {
        let unordered = lines("- root\n  - child\n    - grandchild");
        assert_eq!(
            unordered
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec!["- root", "  - child", "    - grandchild"]
        );

        let ordered =
            lines("1. zero\n   1. one\n      1. two\n         1. three\n            1. four");
        assert_eq!(
            ordered.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec![
                "1. zero",
                "  1. one",
                "    a. two",
                "      i. three",
                "        1. four"
            ]
        );
    }

    #[test]
    fn ordered_lists_honor_non_one_starts_and_task_items_lose_checkboxes() {
        let rendered = lines("4. four\n5. five\n\n- [ ] todo\n- [x] done");
        assert_eq!(
            rendered.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["4. four", "5. five", "- todo", "- done"]
        );
    }

    #[test]
    fn rules_are_literal_and_html_or_definitions_are_dropped() {
        let rule = lines("---");
        assert_eq!(
            rule.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["---"]
        );
        assert_eq!(rule[0].spans[0].style, Style::default());
        assert!(lines("<div>hidden</div>").is_empty());
        assert_eq!(
            lines("<b>visible</b>")
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec!["visible"]
        );
        assert!(lines("[reference]: https://example.com").is_empty());
    }

    #[test]
    fn links_images_and_issue_references_use_plain_fallbacks() {
        let rendered = lines(
            "[docs](https://example.com/docs) [mail](mailto:user@example.com) owner/repo#123 ![image](https://example.com/image.png)",
        );
        assert_eq!(
            rendered.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec![
                "https://example.com/docs user@example.com https://github.com/owner/repo/issues/123 https://example.com/image.png"
            ]
        );
        assert_eq!(rendered[0].spans[0].style.fg, Some(Color::White));
        assert_eq!(
            rendered[0].spans.last().expect("image span").style,
            Style::default()
        );
    }

    #[test]
    fn assistant_output_contains_no_terminal_control_bytes() {
        let rendered = lines("visible\x1b[31m [link](https://example.com/\x1b[0m)");
        assert!(rendered.iter().flat_map(|line| &line.spans).all(|span| {
            span.content
                .chars()
                .all(|character| !character.is_control())
        }));
    }

    #[test]
    fn known_code_uses_named_ansi_roles_without_indent_or_label() {
        let highlighter = CodeHighlighter::new();
        let rendered = assistant_markdown_lines_with_highlighter(
            "```rust\nfn main() { let count = 42; let text = \"ok\"; }\n```",
            Style::default().fg(Color::White),
            Some(&highlighter),
        );
        assert_eq!(
            rendered[0].to_string(),
            "fn main() { let count = 42; let text = \"ok\"; }"
        );
        assert_eq!(rendered.len(), 1);
        assert!(!rendered[0].to_string().contains("rust"));
        assert!(rendered
            .iter()
            .flat_map(|line| &line.spans)
            .any(|span| { span.style.fg == Some(Color::Blue) }));
        assert!(rendered
            .iter()
            .flat_map(|line| &line.spans)
            .any(|span| { span.style.fg == Some(Color::Yellow) }));
        assert!(rendered.iter().flat_map(|line| &line.spans).any(|span| {
            span.style.fg == Some(Color::Green) || span.style.fg == Some(Color::Red)
        }));
        assert!(rendered
            .iter()
            .flat_map(|line| &line.spans)
            .all(|span| { span.style.bg.is_none() }));
    }

    #[test]
    fn code_scope_roles_cover_type_comment_and_function_styles() {
        let highlighter = CodeHighlighter::new();
        let rendered = assistant_markdown_lines_with_highlighter(
            "```rust\nstruct User { value: i32 }\n// note\nfn run() -> bool { true }\n```",
            Style::default().fg(Color::White),
            Some(&highlighter),
        );
        assert!(rendered.iter().flat_map(|line| &line.spans).any(|span| {
            span.style.fg == Some(Color::Cyan) && span.style.add_modifier.contains(Modifier::DIM)
        }));
        assert!(rendered
            .iter()
            .flat_map(|line| &line.spans)
            .any(|span| { span.style.fg == Some(Color::Green) }));
        assert!(rendered
            .iter()
            .flat_map(|line| &line.spans)
            .any(|span| { span.style.fg == Some(Color::Yellow) }));
    }

    #[test]
    fn missing_and_unknown_code_languages_use_plaintext() {
        let highlighter = CodeHighlighter::new();
        for content in ["```\nplain\n```", "```unknown-language\nplain\n```"] {
            let rendered = assistant_markdown_lines_with_highlighter(
                content,
                Style::default().fg(Color::White),
                Some(&highlighter),
            );
            assert_eq!(rendered[0].spans.len(), 1);
            assert_eq!(rendered[0].spans[0].style.fg, Some(Color::White));
        }
    }

    #[test]
    fn code_blocks_keep_internal_blank_lines_and_one_final_logical_newline() {
        let highlighter = CodeHighlighter::new();
        let rendered = assistant_markdown_lines_with_highlighter(
            "```rust\nfirst\n\nsecond\n```",
            Style::default().fg(Color::White),
            Some(&highlighter),
        );
        assert_eq!(
            rendered.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["first", "", "second"]
        );
    }

    #[test]
    fn long_highlighted_code_wraps_through_transcript_rows_with_styles() {
        let highlighter = CodeHighlighter::new();
        let code = assistant_markdown_lines_with_highlighter(
            "```rust\nfn very_long_function_name() { let value = 12345; }\n```",
            Style::default().fg(Color::White),
            Some(&highlighter),
        );
        let wrapped = crate::screen_model::render_entry_lines(
            crate::screen_model::LiveEntryKind::Assistant,
            &code,
            10,
        );
        assert!(wrapped.len() > 2);
        assert!(wrapped
            .iter()
            .flat_map(|line| &line.spans)
            .any(|span| { span.style.fg == Some(Color::Yellow) }));
        assert!(wrapped
            .iter()
            .flat_map(|line| &line.spans)
            .any(|span| { span.style.fg == Some(Color::Blue) }));
        assert!(wrapped.iter().all(|line| {
            line.spans
                .iter()
                .flat_map(|span| span.content.chars())
                .all(|character| !character.is_control())
        }));
    }

    #[test]
    fn claude_style_table_layout_is_rendered() {
        let rendered = lines("| Header | Value |\n| --- | --- |\n| key | value |");
        assert_eq!(
            rendered.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec![
                "┌────────┬───────┐",
                "│ Header │ Value │",
                "├────────┼───────┤",
                "│ key    │ value │",
                "└────────┴───────┘",
            ]
        );
        assert!(rendered[0]
            .spans
            .iter()
            .all(|span| span.style == Style::default()));
        assert!(rendered[1]
            .spans
            .iter()
            .all(|span| !span.style.add_modifier.contains(Modifier::BOLD)));
    }

    #[test]
    fn one_column_tables_have_complete_box_borders() {
        let rendered = lines_at_width("| Header |\n| --- |\n| value |", 80);

        assert_eq!(
            rendered.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec![
                "┌────────┐",
                "│ Header │",
                "├────────┤",
                "│ value  │",
                "└────────┘",
            ]
        );
    }

    #[test]
    fn table_alignment_applies_only_to_data_cells() {
        let rendered = lines_at_width(
            "| Left | Center | Right |\n| :--- | :---: | ---: |\n| a | b | c |",
            80,
        );
        let rows = rendered.iter().map(ToString::to_string).collect::<Vec<_>>();

        assert_eq!(rows[1], "│ Left │ Center │ Right │");
        assert_eq!(rows[3], "│ a    │   b    │     c │");
        assert!(!rendered[1]
            .spans
            .iter()
            .any(|span| span.style.add_modifier.contains(Modifier::BOLD)));
    }

    #[test]
    fn table_has_a_middle_rule_after_the_header_and_between_data_rows() {
        let rendered = lines_at_width(
            "| H | V |\n| --- | --- |\n| one | 1 |\n| two | 2 |\n| three | 3 |",
            80,
        );
        let rules = rendered
            .iter()
            .filter(|line| line.to_string().starts_with('├'))
            .count();

        assert_eq!(rules, 3);
        assert_eq!(
            rendered.first().unwrap().to_string().chars().next(),
            Some('┌')
        );
        assert_eq!(
            rendered.last().unwrap().to_string().chars().next(),
            Some('└')
        );
    }

    #[test]
    fn table_width_arithmetic_includes_borders_padding_and_safety_margin() {
        assert_eq!(table_content_budget(40, 2), 29);
        assert_eq!(table_content_budget(10, 2), 0);
        assert_eq!(
            allocate_table_widths(&[10, 20], &[3, 5], 40),
            Some(vec![10, 19])
        );
    }

    #[test]
    fn long_words_are_hard_broken_at_grapheme_boundaries_when_minimums_need_slack() {
        let rendered = lines_at_width("| H |\n| --- |\n| abcdefghij |", 12);
        let rows = rendered.iter().map(ToString::to_string).collect::<Vec<_>>();

        assert_eq!(
            rows,
            vec![
                "┌──────┐",
                "│  H   │",
                "├──────┤",
                "│ abcd │",
                "│ efgh │",
                "│ ij   │",
                "└──────┘",
            ]
        );
    }

    #[test]
    fn styled_spans_cross_wrap_boundaries_without_creating_word_breaks() {
        let rendered = lines_at_width("| H |\n| --- |\n| **hel**lo world |", 16);
        let data = rendered
            .iter()
            .find(|line| line.to_string().contains("hello"))
            .expect("first styled data row");
        let content = data
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(content, "│ hello    │");
        assert!(data.spans.iter().any(|span| {
            span.content.contains("hel") && span.style.add_modifier.contains(Modifier::BOLD)
        }));
    }

    #[test]
    fn unicode_table_rows_use_display_width_and_keep_graphemes() {
        let rendered = lines_at_width(
            "| H | Value |\n| --- | --- |\n| 界 | e\u{301} 👩\u{200d}💻 |",
            80,
        );
        let data = rendered
            .iter()
            .find(|line| line.to_string().contains("界"))
            .expect("unicode data row");

        assert!(data.to_string().contains("界"));
        assert!(data.to_string().contains("é"));
        assert!(!data.to_string().contains('\u{301}'));
        assert_eq!(data.width(), rendered[0].width());
    }

    #[test]
    fn a_grapheme_wider_than_its_cell_is_retained() {
        let cell = TableCell {
            spans: vec![ratatui::text::Span::raw("界")],
        };
        let wrapped = wrap_table_cell(&cell, 1);

        assert_eq!(wrapped.len(), 1);
        assert_eq!(wrapped[0][0].text, "界");
    }

    #[test]
    fn five_wrapped_lines_trigger_vertical_key_value_fallback() {
        let rendered = lines_at_width(
            "| Header |\n| --- |\n| abcdefghijklmnopqrstabcdefghijklmnopqrst |\n| second |",
            16,
        );
        let rows = rendered.iter().map(ToString::to_string).collect::<Vec<_>>();

        assert!(rows.iter().any(|row| row.starts_with("Header: ")));
        assert!(rows.iter().any(|row| row == "───────────────"));
        assert!(rows.iter().all(|row| !row.starts_with('┌')));
    }

    #[test]
    fn four_wrapped_lines_remain_a_normal_table() {
        let rendered = lines_at_width(
            "| Header |\n| --- |\n| abcdefghijklmnopqrstabcdefghijkl |",
            16,
        );

        assert!(rendered.first().unwrap().to_string().starts_with('┌'));
        assert!(rendered.last().unwrap().to_string().starts_with('└'));
    }

    #[test]
    fn width_overflow_uses_vertical_layout_and_capped_separators() {
        let rendered = lines_at_width(
            "| A | B | C |\n| --- | --- | --- |\n| one | two | three |\n| four | five | six |",
            20,
        );
        let rows = rendered.iter().map(ToString::to_string).collect::<Vec<_>>();

        assert!(rows.iter().all(|row| !row.starts_with('┌')));
        assert!(rows.iter().any(|row| row == &"─".repeat(19)));
        assert!(rows.iter().any(|row| row.starts_with("A: ")));
    }

    #[test]
    fn vertical_layout_wraps_long_header_labels_to_the_table_width() {
        let rendered = lines_at_width(
            "| Long header label |\n| --- |\n| one two three four five |",
            12,
        );

        assert!(rendered
            .iter()
            .all(|line| UnicodeWidthStr::width(line.to_string().as_str()) <= 12));
    }

    #[test]
    fn narrow_widths_do_not_panic_during_table_rendering() {
        for width in [0, 1, 2, 10, 40, 80] {
            let rendered = lines_at_width(
                "| A | B | C |\n| --- | --- | --- |\n| one | two | three |",
                width,
            );
            assert!(!rendered.is_empty(), "width {width} should render rows");
        }
    }
}
