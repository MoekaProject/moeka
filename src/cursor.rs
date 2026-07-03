use crate::buffer::Buffer;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cursor {
    pub offset: usize,
    pub anchor: usize,
}

impl Cursor {
    pub fn new() -> Self {
        Self {
            offset: 0,
            anchor: 0,
        }
    }

    pub fn has_selection(&self) -> bool {
        self.offset != self.anchor
    }

    pub fn selection_range(&self) -> std::ops::Range<usize> {
        let start = self.offset.min(self.anchor);
        let end = self.offset.max(self.anchor);
        start..end
    }

    pub fn move_to(&mut self, offset: usize, buffer: &Buffer) {
        let offset = buffer.clamp_grapheme_offset(offset);
        self.offset = offset;
        self.anchor = offset;
    }

    pub fn select_to(&mut self, offset: usize, buffer: &Buffer) {
        self.offset = buffer.clamp_grapheme_offset(offset);
    }

    pub fn move_left(&mut self, buffer: &Buffer) {
        if self.has_selection() {
            let start = self.selection_range().start;
            self.move_to(start, buffer);
        } else {
            let prev = buffer.prev_grapheme_boundary(self.offset);
            self.move_to(prev, buffer);
        }
    }

    pub fn move_right(&mut self, buffer: &Buffer) {
        if self.has_selection() {
            let end = self.selection_range().end;
            self.move_to(end, buffer);
        } else {
            let next = buffer.next_grapheme_boundary(self.offset);
            self.move_to(next, buffer);
        }
    }

    pub fn move_up(&mut self, buffer: &Buffer) {
        let (line, col) = buffer.offset_to_line_col(self.offset);
        if line > 0 {
            let new_offset = buffer.line_col_to_offset(line - 1, col);
            self.move_to(new_offset, buffer);
        } else {
            self.move_to(0, buffer);
        }
    }

    pub fn move_down(&mut self, buffer: &Buffer) {
        let (line, col) = buffer.offset_to_line_col(self.offset);
        if line + 1 < buffer.line_count() {
            let new_offset = buffer.line_col_to_offset(line + 1, col);
            self.move_to(new_offset, buffer);
        } else {
            self.move_to(buffer.len(), buffer);
        }
    }

    pub fn move_to_line_start(&mut self, buffer: &Buffer) {
        let (line, _) = buffer.offset_to_line_col(self.offset);
        let offset = buffer.line_start(line);
        self.move_to(offset, buffer);
    }

    pub fn move_to_line_end(&mut self, buffer: &Buffer) {
        let (line, _) = buffer.offset_to_line_col(self.offset);
        let offset = buffer.line_end(line);
        self.move_to(offset, buffer);
    }

    pub fn select_left(&mut self, buffer: &Buffer) {
        let prev = buffer.prev_grapheme_boundary(self.offset);
        self.select_to(prev, buffer);
    }

    pub fn select_right(&mut self, buffer: &Buffer) {
        let next = buffer.next_grapheme_boundary(self.offset);
        self.select_to(next, buffer);
    }

    pub fn select_up(&mut self, buffer: &Buffer) {
        let (line, col) = buffer.offset_to_line_col(self.offset);
        if line > 0 {
            let new_offset = buffer.line_col_to_offset(line - 1, col);
            self.select_to(new_offset, buffer);
        } else {
            self.select_to(0, buffer);
        }
    }

    pub fn select_down(&mut self, buffer: &Buffer) {
        let (line, col) = buffer.offset_to_line_col(self.offset);
        if line + 1 < buffer.line_count() {
            let new_offset = buffer.line_col_to_offset(line + 1, col);
            self.select_to(new_offset, buffer);
        } else {
            self.select_to(buffer.len(), buffer);
        }
    }

    pub fn select_all(&mut self, buffer: &Buffer) {
        self.anchor = 0;
        self.offset = buffer.len();
    }

    pub fn select_to_line_start(&mut self, buffer: &Buffer) {
        let (line, _) = buffer.offset_to_line_col(self.offset);
        let offset = buffer.line_start(line);
        self.select_to(offset, buffer);
    }

    pub fn select_to_line_end(&mut self, buffer: &Buffer) {
        let (line, _) = buffer.offset_to_line_col(self.offset);
        let offset = buffer.line_end(line);
        self.select_to(offset, buffer);
    }

    pub fn insert(&mut self, buffer: &mut Buffer, text: &str) {
        let range = self.selection_range();
        buffer.replace(range.clone(), text);
        let new_offset = range.start + text.len();
        self.move_to(new_offset, buffer);
    }

    pub fn backspace(&mut self, buffer: &mut Buffer) {
        if self.has_selection() {
            let range = self.selection_range();
            buffer.delete(range.clone());
            self.move_to(range.start, buffer);
        } else if self.offset > 0 {
            let prev = buffer.prev_grapheme_boundary(self.offset);
            buffer.delete(prev..self.offset);
            self.move_to(prev, buffer);
        }
    }

    pub fn delete(&mut self, buffer: &mut Buffer) {
        if self.has_selection() {
            let range = self.selection_range();
            buffer.delete(range.clone());
            self.move_to(range.start, buffer);
        } else if self.offset < buffer.len() {
            let next = buffer.next_grapheme_boundary(self.offset);
            buffer.delete(self.offset..next);
        }
    }

    pub fn move_word_left(&mut self, buffer: &Buffer) {
        let offset = prev_word_boundary(buffer.text(), self.offset);
        self.move_to(offset, buffer);
    }

    pub fn move_word_right(&mut self, buffer: &Buffer) {
        let offset = next_word_boundary(buffer.text(), self.offset);
        self.move_to(offset, buffer);
    }

    pub fn delete_word_left(&mut self, buffer: &mut Buffer) {
        if self.has_selection() {
            let range = self.selection_range();
            buffer.delete(range.clone());
            self.move_to(range.start, buffer);
        } else {
            let prev = prev_word_boundary(buffer.text(), self.offset);
            buffer.delete(prev..self.offset);
            self.move_to(prev, buffer);
        }
    }

    pub fn delete_word_right(&mut self, buffer: &mut Buffer) {
        if self.has_selection() {
            let range = self.selection_range();
            buffer.delete(range.clone());
            self.move_to(range.start, buffer);
        } else {
            let next = next_word_boundary(buffer.text(), self.offset);
            buffer.delete(self.offset..next);
        }
    }
}

fn prev_word_boundary(text: &str, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }
    let before = &text[..offset];
    let trimmed = before.trim_end();
    if trimmed.is_empty() {
        return 0;
    }
    for (idx, ch) in trimmed.char_indices().rev() {
        if ch.is_whitespace() || ch.is_ascii_punctuation() {
            return idx + ch.len_utf8();
        }
    }
    0
}

fn next_word_boundary(text: &str, offset: usize) -> usize {
    if offset >= text.len() {
        return text.len();
    }
    let after = &text[offset..];
    let mut found_word_end = false;
    for (idx, ch) in after.char_indices() {
        if !found_word_end {
            if ch.is_whitespace() || ch.is_ascii_punctuation() {
                found_word_end = true;
            }
        } else if !ch.is_whitespace() {
            return offset + idx;
        }
    }
    text.len()
}
