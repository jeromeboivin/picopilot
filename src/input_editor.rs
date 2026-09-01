#[derive(Debug, Default)]
pub(crate) struct InputEditor {
    text: String,
    cursor: usize,
    preferred_column: Option<usize>,
}

impl InputEditor {
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn insert_char(&mut self, character: char) {
        let mut text = String::new();
        text.push(character);
        self.insert_at_cursor(&text);
    }

    pub(crate) fn insert_newline(&mut self) {
        self.insert_at_cursor("\n");
    }

    pub(crate) fn insert_paste(&mut self, pasted: &str) {
        let pasted = pasted.replace("\r\n", "\n").replace('\r', "\n");
        self.insert_at_cursor(&pasted);
    }

    pub(crate) fn backspace(&mut self) {
        let Some((cursor, _)) = self.text[..self.cursor].char_indices().next_back() else {
            return;
        };
        self.text.drain(cursor..self.cursor);
        self.cursor = cursor;
        self.preferred_column = None;
    }

    pub(crate) fn delete(&mut self) {
        let Some(character) = self.text[self.cursor..].chars().next() else {
            return;
        };
        let end = self.cursor + character.len_utf8();
        self.text.drain(self.cursor..end);
        self.preferred_column = None;
    }

    pub(crate) fn move_left(&mut self) {
        if let Some((cursor, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.cursor = cursor;
            self.preferred_column = None;
        }
    }

    pub(crate) fn move_right(&mut self) {
        if let Some(character) = self.text[self.cursor..].chars().next() {
            self.cursor += character.len_utf8();
            self.preferred_column = None;
        }
    }

    pub(crate) fn move_up(&mut self) {
        self.move_vertical(-1);
    }

    pub(crate) fn move_down(&mut self) {
        self.move_vertical(1);
    }

    pub(crate) fn move_home(&mut self, all_lines: bool) {
        self.cursor = if all_lines {
            0
        } else {
            self.current_line_bounds().0
        };
        self.preferred_column = None;
    }

    pub(crate) fn move_end(&mut self, all_lines: bool) {
        self.cursor = if all_lines {
            self.text.len()
        } else {
            self.current_line_bounds().1
        };
        self.preferred_column = None;
    }

    pub(crate) fn cursor(&self) -> (usize, usize) {
        let mut line = 0;
        let mut column = 0;
        for character in self.text[..self.cursor].chars() {
            if character == '\n' {
                line += 1;
                column = 0;
            } else {
                column += 1;
            }
        }
        (line, column)
    }

    pub(crate) fn cursor_byte_offset(&self) -> usize {
        self.cursor
    }

    pub(crate) fn take(&mut self) -> String {
        let text = std::mem::take(&mut self.text);
        self.cursor = 0;
        self.preferred_column = None;
        text
    }

    pub(crate) fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.preferred_column = None;
    }

    fn insert_at_cursor(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.preferred_column = None;
    }

    fn move_vertical(&mut self, direction: isize) {
        let (line, column) = self.cursor();
        let preferred_column = *self.preferred_column.get_or_insert(column);
        let target_line = line as isize + direction;
        if target_line < 0 {
            return;
        }

        let lines = self.line_ranges();
        let Some(&(line_start, line_end)) = lines.get(target_line as usize) else {
            return;
        };
        self.cursor = byte_offset_at_column(
            &self.text[line_start..line_end],
            preferred_column.min(self.text[line_start..line_end].chars().count()),
        ) + line_start;
    }

    fn current_line_bounds(&self) -> (usize, usize) {
        let line_start = self.text[..self.cursor]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        let line_end = self.text[self.cursor..]
            .find('\n')
            .map(|index| self.cursor + index)
            .unwrap_or(self.text.len());
        (line_start, line_end)
    }

    fn line_ranges(&self) -> Vec<(usize, usize)> {
        let mut lines = Vec::new();
        let mut line_start = 0;
        for (index, character) in self.text.char_indices() {
            if character == '\n' {
                lines.push((line_start, index));
                line_start = index + character.len_utf8();
            }
        }
        lines.push((line_start, self.text.len()));
        lines
    }
}

fn byte_offset_at_column(line: &str, column: usize) -> usize {
    line.char_indices()
        .nth(column)
        .map(|(index, _)| index)
        .unwrap_or(line.len())
}

#[cfg(test)]
mod tests {
    use super::InputEditor;

    #[test]
    fn inserts_at_the_cursor_without_splitting_utf8() {
        let mut editor = InputEditor::default();

        editor.insert_paste("a🙂c");
        editor.move_left();
        editor.insert_char('b');

        assert_eq!(editor.text(), "a🙂bc");
        assert_eq!(editor.cursor(), (0, 3));
    }

    #[test]
    fn backspace_and_delete_handle_characters_and_newlines() {
        let mut editor = InputEditor::default();
        editor.insert_paste("ab\n🙂c");
        editor.move_home(false);
        editor.move_down();
        editor.move_home(false);
        editor.delete();

        assert_eq!(editor.text(), "ab\nc");
        editor.backspace();
        assert_eq!(editor.text(), "abc");
    }

    #[test]
    fn vertical_movement_preserves_the_preferred_column() {
        let mut editor = InputEditor::default();
        editor.insert_paste("12345\nxy\n123456");
        editor.move_end(false);
        editor.move_up();
        assert_eq!(editor.cursor(), (1, 2));
        editor.move_up();
        assert_eq!(editor.cursor(), (0, 5));
        editor.move_down();
        assert_eq!(editor.cursor(), (1, 2));
        editor.move_down();
        assert_eq!(editor.cursor(), (2, 6));
    }

    #[test]
    fn normalizes_pasted_line_endings() {
        let mut editor = InputEditor::default();

        editor.insert_paste("first\r\nsecond\rthird");

        assert_eq!(editor.text(), "first\nsecond\nthird");
    }

    #[test]
    fn take_and_clear_reset_the_cursor() {
        let mut editor = InputEditor::default();
        editor.insert_paste("abc");
        editor.move_left();

        assert_eq!(editor.take(), "abc");
        editor.insert_char('x');
        assert_eq!(editor.text(), "x");

        editor.insert_paste("yz");
        editor.clear();
        editor.insert_char('a');
        assert_eq!(editor.text(), "a");
    }
}
