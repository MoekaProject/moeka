use crate::buffer::Buffer;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

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

    pub fn select_word_left(&mut self, buffer: &Buffer) {
        let offset = prev_word_boundary(buffer.text(), self.offset);
        self.select_to(offset, buffer);
    }

    pub fn select_word_right(&mut self, buffer: &Buffer) {
        let offset = next_word_boundary(buffer.text(), self.offset);
        self.select_to(offset, buffer);
    }

    #[cfg(test)]
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

    #[cfg(test)]
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

pub(crate) fn word_range_at(text: &str, offset: usize) -> Range<usize> {
    let mut probe = offset.min(text.len());
    while probe > 0 && !text.is_char_boundary(probe) {
        probe -= 1;
    }
    if probe == text.len() && probe > 0 {
        probe = text[..probe]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0);
    }

    word_ranges(text)
        .into_iter()
        .find(|range| range.contains(&probe))
        .unwrap_or(probe..probe)
}

fn word_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for (start, word) in UnicodeSegmentation::unicode_word_indices(text) {
        let end = start + word.len();
        if let Some(previous) = ranges.last_mut()
            && previous.end == start
            && is_cjk_word(&text[previous.clone()])
            && is_cjk_word(word)
        {
            previous.end = end;
        } else {
            ranges.push(start..end);
        }
    }
    ranges
}

fn is_cjk_word(word: &str) -> bool {
    word.chars().all(|ch| {
        let code = ch as u32;
        (0x3400..=0x4DBF).contains(&code)
            || (0x4E00..=0x9FFF).contains(&code)
            || (0x3040..=0x309F).contains(&code)
            || (0x30A0..=0x30FF).contains(&code)
            || (0xAC00..=0xD7AF).contains(&code)
            || matches!(ch, '々' | '〆' | '〇' | 'ー')
    })
}

fn prev_word_boundary(text: &str, offset: usize) -> usize {
    word_ranges(text)
        .into_iter()
        .rev()
        .find(|range| range.start < offset)
        .map_or(0, |range| range.start)
}

fn next_word_boundary(text: &str, offset: usize) -> usize {
    word_ranges(text)
        .into_iter()
        .find(|range| range.start > offset)
        .map_or(text.len(), |range| range.start)
}

#[cfg(test)]
mod tests {
    use super::{Cursor, word_range_at};
    use crate::buffer::Buffer;

    #[test]
    fn selects_words_without_splitting_utf8() {
        let buffer = Buffer::from_text("alpha 中文 beta");
        let mut cursor = Cursor {
            offset: "alpha 中文".len(),
            anchor: "alpha 中文".len(),
        };

        cursor.select_word_left(&buffer);
        assert_eq!(&buffer.text()[cursor.selection_range()], "中文");

        cursor.move_to("alpha ".len(), &buffer);
        cursor.select_word_right(&buffer);
        assert_eq!(&buffer.text()[cursor.selection_range()], "中文 ");
    }

    #[test]
    fn unicode_word_ranges_keep_accents_marks_and_apostrophes_together() {
        for text in ["después", "español", "despue\u{301}s", "l'amour"] {
            assert_eq!(word_range_at(text, 2), 0..text.len(), "{text}");
        }
    }

    #[test]
    fn word_navigation_uses_unicode_punctuation_and_accents() {
        let buffer = Buffer::from_text("¿después? español");
        let mut cursor = Cursor {
            offset: buffer.len(),
            anchor: buffer.len(),
        };

        cursor.move_word_left(&buffer);
        assert_eq!(cursor.offset, "¿después? ".len());
        cursor.move_word_left(&buffer);
        assert_eq!(cursor.offset, "¿".len());
    }

    #[test]
    fn adjacent_cjk_words_remain_one_selection() {
        let text = "中文";
        assert_eq!(word_range_at(text, 0), 0..text.len());
    }
}
