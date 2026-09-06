use std::time::{Duration, Instant};

use similar::{Algorithm, ChangeTag, TextDiff};

const CONTEXT_LINES: usize = 3;

/// A combined one-megabyte input ceiling keeps semantic diff work bounded while
/// leaving the summary available for larger file edits.
pub(crate) const MAX_DIFF_INPUT_BYTES: usize = 1024 * 1024;
pub(crate) const DIFF_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileDiff {
    pub(crate) additions: usize,
    pub(crate) removals: usize,
    pub(crate) hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiffHunk {
    pub(crate) rows: Vec<DiffRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffRowKind {
    Context,
    Removed,
    Added,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiffRow {
    pub(crate) kind: DiffRowKind,
    pub(crate) number: usize,
    pub(crate) text: String,
    pub(crate) segments: Vec<DiffSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiffSegment {
    pub(crate) text: String,
    pub(crate) changed: bool,
}

type InlineSegmentPair = (Vec<DiffSegment>, Vec<DiffSegment>);

pub(crate) fn build_file_diff(old_text: &str, new_text: &str) -> FileDiff {
    build_file_diff_with_budget(old_text, new_text, DIFF_TIMEOUT)
}

pub(crate) fn build_file_diff_with_budget(
    old_text: &str,
    new_text: &str,
    budget: Duration,
) -> FileDiff {
    if old_text == new_text {
        return FileDiff {
            additions: 0,
            removals: 0,
            hunks: Vec::new(),
        };
    }

    if old_text.len().saturating_add(new_text.len()) > MAX_DIFF_INPUT_BYTES || budget.is_zero() {
        return degraded_diff(old_text, new_text);
    }

    let started_at = Instant::now();
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Myers)
        .timeout(budget)
        .diff_lines(old_text, new_text);
    let (additions, removals) = change_counts(&diff);
    if started_at.elapsed() >= budget {
        return FileDiff {
            additions,
            removals,
            hunks: Vec::new(),
        };
    }

    let mut hunks = Vec::new();
    for operations in diff.grouped_ops(CONTEXT_LINES) {
        if started_at.elapsed() >= budget {
            return FileDiff {
                additions,
                removals,
                hunks: Vec::new(),
            };
        }

        let mut rows = Vec::new();
        for operation in operations {
            for change in diff.iter_changes(&operation) {
                let (kind, number) = match change.tag() {
                    ChangeTag::Equal => (
                        DiffRowKind::Context,
                        change.new_index().unwrap_or_default() + 1,
                    ),
                    ChangeTag::Delete => (
                        DiffRowKind::Removed,
                        change.old_index().unwrap_or_default() + 1,
                    ),
                    ChangeTag::Insert => (
                        DiffRowKind::Added,
                        change.new_index().unwrap_or_default() + 1,
                    ),
                };
                let text = strip_line_ending(change.value_ref()).to_string();
                rows.push(DiffRow {
                    kind,
                    number,
                    segments: vec![DiffSegment {
                        text: text.clone(),
                        changed: false,
                    }],
                    text,
                });
            }
        }

        if apply_word_highlighting(&mut rows, started_at, budget).is_err() {
            return FileDiff {
                additions,
                removals,
                hunks: Vec::new(),
            };
        }
        hunks.push(DiffHunk { rows });
    }

    FileDiff {
        additions,
        removals,
        hunks,
    }
}

fn degraded_diff(old_text: &str, new_text: &str) -> FileDiff {
    FileDiff {
        additions: line_count(new_text),
        removals: line_count(old_text),
        hunks: Vec::new(),
    }
}

fn change_counts<T: similar::DiffableStr + ?Sized>(diff: &TextDiff<'_, '_, T>) -> (usize, usize) {
    diff.iter_all_changes()
        .fold((0, 0), |(additions, removals), change| match change.tag() {
            ChangeTag::Insert => (additions + 1, removals),
            ChangeTag::Delete => (additions, removals + 1),
            ChangeTag::Equal => (additions, removals),
        })
}

fn line_count(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let mut lines = text.split('\n').count();
    if text.ends_with('\n') {
        lines = lines.saturating_sub(1);
    }
    lines
}

fn strip_line_ending(text: &str) -> &str {
    let text = text.strip_suffix('\n').unwrap_or(text);
    text.strip_suffix('\r').unwrap_or(text)
}

fn apply_word_highlighting(
    rows: &mut [DiffRow],
    started_at: Instant,
    budget: Duration,
) -> Result<(), ()> {
    let mut index = 0;
    while index < rows.len() {
        if rows[index].kind != DiffRowKind::Removed {
            index += 1;
            continue;
        }
        let removal_start = index;
        while index < rows.len() && rows[index].kind == DiffRowKind::Removed {
            index += 1;
        }
        let addition_start = index;
        while index < rows.len() && rows[index].kind == DiffRowKind::Added {
            index += 1;
        }
        let pair_count = (addition_start - removal_start).min(index - addition_start);
        for offset in 0..pair_count {
            if started_at.elapsed() >= budget {
                return Err(());
            }
            let old_text = rows[removal_start + offset].text.clone();
            let new_text = rows[addition_start + offset].text.clone();
            let remaining = budget.saturating_sub(started_at.elapsed());
            let Some((old_segments, new_segments)) =
                inline_segments(&old_text, &new_text, remaining)?
            else {
                continue;
            };
            rows[removal_start + offset].segments = old_segments;
            rows[addition_start + offset].segments = new_segments;
        }
    }
    Ok(())
}

fn inline_segments(
    old_text: &str,
    new_text: &str,
    budget: Duration,
) -> Result<Option<InlineSegmentPair>, ()> {
    if budget.is_zero() {
        return Err(());
    }
    let old_tokens = tokenize(old_text);
    let new_tokens = tokenize(new_text);
    let old_refs = old_tokens.iter().map(String::as_str).collect::<Vec<_>>();
    let new_refs = new_tokens.iter().map(String::as_str).collect::<Vec<_>>();
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Myers)
        .timeout(budget)
        .diff_slices(&old_refs, &new_refs);
    let mut old_segments = Vec::new();
    let mut new_segments = Vec::new();
    let mut changed_tokens = 0;
    for change in diff.iter_all_changes() {
        let changed = change.tag() != ChangeTag::Equal;
        if changed {
            changed_tokens += 1;
        }
        match change.tag() {
            ChangeTag::Equal => {
                push_segment(&mut old_segments, change.value_ref(), false);
                push_segment(&mut new_segments, change.value_ref(), false);
            }
            ChangeTag::Delete => push_segment(&mut old_segments, change.value_ref(), true),
            ChangeTag::Insert => push_segment(&mut new_segments, change.value_ref(), true),
        }
    }
    let total_tokens = old_tokens.len().saturating_add(new_tokens.len());
    if total_tokens == 0 || changed_tokens * 5 > total_tokens * 2 || changed_tokens == 0 {
        return Ok(None);
    }
    Ok(Some((old_segments, new_segments)))
}

fn push_segment(segments: &mut Vec<DiffSegment>, text: &str, changed: bool) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = segments.last_mut().filter(|last| last.changed == changed) {
        last.text.push_str(text);
    } else {
        segments.push(DiffSegment {
            text: text.to_string(),
            changed,
        });
    }
}

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_kind = None;
    for character in text.chars() {
        let kind = if character.is_alphabetic() || character.is_numeric() || character == '_' {
            Some(TokenKind::Word)
        } else if character.is_whitespace() {
            Some(TokenKind::Whitespace)
        } else {
            None
        };
        if kind.is_some() && kind == current_kind {
            current.push(character);
        } else {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            current_kind = kind;
            current.push(character);
            if current_kind.is_none() {
                tokens.push(std::mem::take(&mut current));
            }
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Word,
    Whitespace,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{build_file_diff_with_budget, tokenize, DiffRowKind, MAX_DIFF_INPUT_BYTES};

    #[test]
    fn tokenizes_unicode_word_whitespace_and_remaining_codepoints() {
        assert_eq!(tokenize("été_2 你好!"), vec!["été_2", " ", "你好", "!"]);
    }

    #[test]
    fn zero_budget_retains_summary_and_omits_hunks() {
        let diff = build_file_diff_with_budget("old\n", "new\n", Duration::ZERO);

        assert_eq!((diff.additions, diff.removals), (1, 1));
        assert!(diff.hunks.is_empty());
    }

    #[test]
    fn oversized_input_retains_summary_and_omits_hunks() {
        let old = "o".repeat(MAX_DIFF_INPUT_BYTES / 2 + 1);
        let new = "n".repeat(MAX_DIFF_INPUT_BYTES / 2 + 1);
        let diff = build_file_diff_with_budget(&old, &new, Duration::from_secs(5));

        assert_eq!((diff.additions, diff.removals), (1, 1));
        assert!(diff.hunks.is_empty());
    }

    #[test]
    fn line_rows_keep_independent_old_and_new_numbers() {
        let diff = build_file_diff_with_budget("old\n", "new\n", Duration::from_secs(5));
        let rows = &diff.hunks[0].rows;

        assert_eq!(rows[0].kind, DiffRowKind::Removed);
        assert_eq!(rows[0].number, 1);
        assert_eq!(rows[1].kind, DiffRowKind::Added);
        assert_eq!(rows[1].number, 1);
    }
}
