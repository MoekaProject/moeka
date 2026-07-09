use unicode_segmentation::UnicodeSegmentation;

/// A simple text buffer backed by a String with cached line start offsets.
pub struct Buffer {
    text: String,
    line_starts: Vec<usize>,
    line_utf16_starts: Vec<usize>,
}

impl Buffer {
    pub fn new() -> Self {
        let mut buf = Self {
            text: String::new(),
            line_starts: Vec::new(),
            line_utf16_starts: Vec::new(),
        };
        buf.rebuild_line_starts();
        buf
    }

    pub fn from_text(text: &str) -> Self {
        let mut buf = Self {
            text: text.to_string(),
            line_starts: Vec::new(),
            line_utf16_starts: Vec::new(),
        };
        buf.rebuild_line_starts();
        buf
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn insert(&mut self, offset: usize, text: &str) {
        let offset = snap_char_start(&self.text, offset.min(self.text.len()));
        let start_utf16 = self.utf16_offset_for_offset(offset);
        self.text.insert_str(offset, text);
        self.splice_line_starts(offset, offset, start_utf16, start_utf16, text);
    }

    pub fn delete(&mut self, range: std::ops::Range<usize>) {
        let start = snap_char_start(&self.text, range.start.min(self.text.len()));
        let end = snap_char_start(&self.text, range.end.min(self.text.len()));
        if start < end {
            let start_utf16 = self.utf16_offset_for_offset(start);
            let end_utf16 = self.utf16_offset_for_offset(end);
            self.text.drain(start..end);
            self.splice_line_starts(start, end, start_utf16, end_utf16, "");
        }
    }

    pub fn replace(&mut self, range: std::ops::Range<usize>, text: &str) {
        let start = snap_char_start(&self.text, range.start.min(self.text.len()));
        let end = snap_char_start(&self.text, range.end.min(self.text.len()));
        if start <= end {
            let start_utf16 = self.utf16_offset_for_offset(start);
            let end_utf16 = self.utf16_offset_for_offset(end);
            self.text.replace_range(start..end, text);
            self.splice_line_starts(start, end, start_utf16, end_utf16, text);
        }
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    pub fn line_start(&self, line: usize) -> usize {
        if line < self.line_starts.len() {
            self.line_starts[line]
        } else {
            self.text.len()
        }
    }

    pub fn line_end(&self, line: usize) -> usize {
        if line + 1 < self.line_starts.len() {
            let next_start = self.line_starts[line + 1];
            if next_start > 0 && self.text.as_bytes().get(next_start - 1) == Some(&b'\n') {
                next_start - 1
            } else {
                next_start
            }
        } else {
            self.text.len()
        }
    }

    pub fn line_content(&self, line: usize) -> &str {
        let start = self.line_start(line);
        let end = self.line_end(line);
        &self.text[start..end]
    }

    pub fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.text.len());
        let line = match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(line) => line.saturating_sub(1),
        };
        let col = offset - self.line_starts[line];
        (line, col)
    }

    pub fn line_col_to_offset(&self, line: usize, col: usize) -> usize {
        let line = line.min(self.line_count().saturating_sub(1));
        let line_start = self.line_start(line);
        let line_end = self.line_end(line);
        let max_col = line_end - line_start;
        line_start + col.min(max_col)
    }

    pub fn utf16_offset_for_offset(&self, offset: usize) -> usize {
        let offset = snap_char_start(&self.text, offset.min(self.text.len()));
        let (line, col) = self.offset_to_line_col(offset);
        self.line_utf16_starts[line] + self.line_content(line)[..col].encode_utf16().count()
    }

    pub fn offset_for_utf16_offset(&self, utf16_offset: usize) -> usize {
        let total_utf16 = self.line_utf16_starts.last().copied().unwrap_or(0)
            + self
                .line_content(self.line_count().saturating_sub(1))
                .encode_utf16()
                .count();
        let utf16_offset = utf16_offset.min(total_utf16);
        let line = match self.line_utf16_starts.binary_search(&utf16_offset) {
            Ok(line) => line,
            Err(line) => line.saturating_sub(1),
        };
        let line_start_utf16 = self.line_utf16_starts[line];
        let target_col = utf16_offset.saturating_sub(line_start_utf16);
        let line_start = self.line_start(line);
        let mut utf16_count = 0usize;
        for (byte_idx, ch) in self.line_content(line).char_indices() {
            if utf16_count >= target_col {
                return line_start + byte_idx;
            }
            utf16_count += ch.len_utf16();
        }
        self.line_end(line)
    }

    pub fn clamp_offset(&self, offset: usize) -> usize {
        let offset = offset.min(self.text.len());
        let mut o = offset;
        while o > 0 && !self.text.is_char_boundary(o) {
            o -= 1;
        }
        o
    }

    pub fn clamp_grapheme_offset(&self, offset: usize) -> usize {
        self.clamp_grapheme_offset_in_line(offset)
    }

    pub fn prev_grapheme_boundary(&self, offset: usize) -> usize {
        if self.text.is_empty() {
            return 0;
        }

        let offset = self.clamp_grapheme_offset_in_line(offset);
        let (line, _) = self.offset_to_line_col(offset);
        let line_start = self.line_start(line);
        if offset <= line_start {
            return line
                .checked_sub(1)
                .map(|prev_line| self.line_end(prev_line))
                .unwrap_or(0);
        }

        let line_text = self.line_content(line);
        line_start + prev_grapheme_boundary(line_text, offset - line_start)
    }

    pub fn next_grapheme_boundary(&self, offset: usize) -> usize {
        if self.text.is_empty() {
            return 0;
        }

        let offset = self.clamp_grapheme_offset_in_line(offset);
        let (line, _) = self.offset_to_line_col(offset);
        let line_start = self.line_start(line);
        let line_end = self.line_end(line);
        if offset >= line_end {
            return if line + 1 < self.line_count() {
                self.line_start(line + 1)
            } else {
                self.text.len()
            };
        }

        let line_text = self.line_content(line);
        line_start + next_grapheme_boundary(line_text, offset - line_start)
    }

    fn clamp_grapheme_offset_in_line(&self, offset: usize) -> usize {
        if self.text.is_empty() {
            return 0;
        }

        let offset = snap_char_start(&self.text, offset.min(self.text.len()));
        if offset >= self.text.len() {
            return self.text.len();
        }

        let (line, _) = self.offset_to_line_col(offset);
        let line_start = self.line_start(line);
        let line_end = self.line_end(line);
        if offset <= line_start || offset >= line_end {
            return offset;
        }

        let line_text = self.line_content(line);
        line_start + clamp_grapheme_boundary(line_text, offset - line_start)
    }

    fn rebuild_line_starts(&mut self) {
        self.line_starts.clear();
        self.line_utf16_starts.clear();
        self.line_starts.push(0);
        self.line_utf16_starts.push(0);
        let mut utf16_offset = 0usize;
        for (i, ch) in self.text.char_indices() {
            utf16_offset += ch.len_utf16();
            if ch == '\n' {
                self.line_starts.push(i + ch.len_utf8());
                self.line_utf16_starts.push(utf16_offset);
            }
        }
    }

    /// Incrementally update the cached line-start offsets after the byte range
    /// `[start, end)` of the *old* text was replaced by `inserted`.
    ///
    /// This avoids the O(n) full rescan of `rebuild_line_starts` on every edit,
    /// which is the dominant per-keystroke cost on very large files. The caller
    /// must already have applied the edit to `self.text` using the same snapped
    /// `start`/`end` offsets.
    fn splice_line_starts(
        &mut self,
        start: usize,
        end: usize,
        start_utf16: usize,
        end_utf16: usize,
        inserted: &str,
    ) {
        let delta = inserted.len() as isize - (end - start) as isize;
        let inserted_utf16_len = inserted.encode_utf16().count();
        let utf16_delta =
            inserted_utf16_len as isize - end_utf16.saturating_sub(start_utf16) as isize;

        // Line starts in `(start, end]` correspond to newlines inside the removed
        // region and must be dropped; line starts strictly after `end` only shift
        // by the byte delta. `line_starts[0]` is always 0 (<= start) so it stays.
        let first = self.line_starts.partition_point(|&v| v <= start);
        let last = self.line_starts.partition_point(|&v| v <= end);

        let mut new_starts: Vec<usize> = Vec::new();
        let mut new_utf16_starts: Vec<usize> = Vec::new();
        let mut inserted_utf16_offset = 0usize;
        for (i, ch) in inserted.char_indices() {
            inserted_utf16_offset += ch.len_utf16();
            if ch == '\n' {
                new_starts.push(start + i + ch.len_utf8());
                new_utf16_starts.push(start_utf16 + inserted_utf16_offset);
            }
        }

        if delta != 0 {
            for v in &mut self.line_starts[last..] {
                *v = (*v as isize + delta) as usize;
            }
        }
        if utf16_delta != 0 {
            for v in &mut self.line_utf16_starts[last..] {
                *v = (*v as isize + utf16_delta) as usize;
            }
        }
        self.line_starts.splice(first..last, new_starts);
        self.line_utf16_starts.splice(first..last, new_utf16_starts);
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

/// Snap `offset` backward to the nearest UTF-8 char boundary.
fn snap_char_start(text: &str, offset: usize) -> usize {
    let mut o = offset.min(text.len());
    while o > 0 && !text.is_char_boundary(o) {
        o -= 1;
    }
    o
}

pub fn clamp_grapheme_boundary(text: &str, offset: usize) -> usize {
    if text.is_empty() {
        return 0;
    }

    let offset = snap_char_start(text, offset.min(text.len()));
    if offset >= text.len() {
        return text.len();
    }

    text.grapheme_indices(true)
        .rev()
        .find_map(|(idx, _)| if idx <= offset { Some(idx) } else { None })
        .unwrap_or(0)
}

pub fn prev_grapheme_boundary(text: &str, offset: usize) -> usize {
    let offset = clamp_grapheme_boundary(text, offset);
    if offset == 0 {
        return 0;
    }

    text.grapheme_indices(true)
        .rev()
        .find_map(|(idx, _)| if idx < offset { Some(idx) } else { None })
        .unwrap_or(0)
}

pub fn next_grapheme_boundary(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    if offset >= text.len() {
        return text.len();
    }

    let offset = clamp_grapheme_boundary(text, offset);
    text.grapheme_indices(true)
        .find_map(|(idx, _)| if idx > offset { Some(idx) } else { None })
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::{Buffer, clamp_grapheme_boundary, next_grapheme_boundary, prev_grapheme_boundary};

    #[test]
    fn grapheme_helpers_treat_combining_sequence_as_single_cursor_stop() {
        let text = "a\u{0301}b";
        let combining_boundary = text
            .char_indices()
            .nth(1)
            .map(|(idx, _)| idx)
            .expect("combining mark boundary");
        let b_offset = text.find('b').expect("b offset");

        assert_eq!(clamp_grapheme_boundary(text, combining_boundary), 0);
        assert_eq!(prev_grapheme_boundary(text, b_offset), 0);
        assert_eq!(next_grapheme_boundary(text, 0), b_offset);
    }

    #[test]
    fn buffer_grapheme_methods_keep_emoji_modifier_together() {
        let buffer = Buffer::from_text("👍🏻!");
        let modifier_boundary = buffer
            .text()
            .char_indices()
            .nth(1)
            .map(|(idx, _)| idx)
            .expect("emoji modifier boundary");
        let bang_offset = buffer.text().find('!').expect("bang offset");

        assert_eq!(buffer.clamp_grapheme_offset(modifier_boundary), 0);
        assert_eq!(buffer.prev_grapheme_boundary(bang_offset), 0);
        assert_eq!(buffer.next_grapheme_boundary(0), bang_offset);
    }

    fn expected_line_starts(text: &str) -> Vec<usize> {
        let mut starts = vec![0usize];
        for (i, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                starts.push(i + 1);
            }
        }
        starts
    }

    fn expected_line_utf16_starts(text: &str) -> Vec<usize> {
        let mut starts = vec![0usize];
        let mut utf16_offset = 0usize;
        for ch in text.chars() {
            utf16_offset += ch.len_utf16();
            if ch == '\n' {
                starts.push(utf16_offset);
            }
        }
        starts
    }

    fn assert_line_starts_consistent(buffer: &Buffer) {
        assert_eq!(
            buffer.line_starts,
            expected_line_starts(buffer.text()),
            "incremental line_starts diverged from full rebuild for {:?}",
            buffer.text()
        );
        assert_eq!(
            buffer.line_utf16_starts,
            expected_line_utf16_starts(buffer.text()),
            "incremental line_utf16_starts diverged from full rebuild for {:?}",
            buffer.text()
        );
    }

    #[test]
    fn incremental_line_starts_match_full_rebuild() {
        let mut buffer = Buffer::from_text("line0\nline1\nline2\n");
        assert_line_starts_consistent(&buffer);

        // Insert without a newline in the middle of a line.
        buffer.insert(2, "XY");
        assert_line_starts_consistent(&buffer);

        // Insert a newline (splits a line).
        buffer.insert(6, "\n");
        assert_line_starts_consistent(&buffer);

        // Insert multi-line text.
        buffer.insert(0, "a\nb\nc");
        assert_line_starts_consistent(&buffer);

        // Delete across a newline (merges lines).
        let text_len = buffer.len();
        buffer.delete(3..text_len.min(9));
        assert_line_starts_consistent(&buffer);

        // Replace a range that removes and adds newlines.
        buffer.replace(0..buffer.len().min(4), "p\nq\nr\n");
        assert_line_starts_consistent(&buffer);

        // Delete everything.
        buffer.delete(0..buffer.len());
        assert_line_starts_consistent(&buffer);
        assert_eq!(buffer.line_count(), 1);

        // Insert into empty buffer.
        buffer.insert(0, "first\nsecond");
        assert_line_starts_consistent(&buffer);
    }

    #[test]
    fn utf16_offsets_use_line_cache_and_handle_multibyte_text() {
        let mut buffer = Buffer::from_text("a😀\n中👍🏻z");
        let emoji = buffer.text().find('😀').expect("emoji offset");
        let second_line = buffer.line_start(1);
        let z = buffer.text().find('z').expect("z offset");

        assert_eq!(buffer.utf16_offset_for_offset(0), 0);
        assert_eq!(buffer.utf16_offset_for_offset(emoji), 1);
        assert_eq!(buffer.utf16_offset_for_offset(second_line), 4);
        assert_eq!(buffer.offset_for_utf16_offset(0), 0);
        assert_eq!(buffer.offset_for_utf16_offset(1), emoji);
        assert_eq!(buffer.offset_for_utf16_offset(4), second_line);
        assert_eq!(
            buffer.offset_for_utf16_offset(buffer.utf16_offset_for_offset(z)),
            z
        );

        buffer.replace(1..z, "é\n🙂");
        assert_line_starts_consistent(&buffer);
        for offset in 0..=buffer.len() {
            if buffer.text().is_char_boundary(offset) {
                let utf16 = buffer.utf16_offset_for_offset(offset);
                assert_eq!(buffer.offset_for_utf16_offset(utf16), offset);
            }
        }
    }
}
