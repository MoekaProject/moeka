use std::cell::Cell;

use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BufferEdit {
    pub(crate) start: usize,
    pub(crate) deleted_text: String,
    pub(crate) inserted_text: String,
}

const OFFSET_CHUNK_TARGET: usize = 1024;
const OFFSET_CHUNK_MAX: usize = OFFSET_CHUNK_TARGET * 2;

#[derive(Clone, Debug)]
struct OffsetChunk {
    values: Vec<usize>,
    delta: isize,
}

impl OffsetChunk {
    fn value(&self, index: usize) -> usize {
        (self.values[index] as isize + self.delta) as usize
    }

    fn last(&self) -> usize {
        self.value(self.values.len() - 1)
    }

    fn actual_values(&self) -> impl Iterator<Item = usize> + '_ {
        self.values
            .iter()
            .map(|value| (*value as isize + self.delta) as usize)
    }
}

#[derive(Clone, Debug, Default)]
struct ChunkedOffsetIndex {
    chunks: Vec<OffsetChunk>,
    chunk_starts: Vec<usize>,
    len: usize,
    cached_chunk: Cell<usize>,
}

impl ChunkedOffsetIndex {
    fn from_values(values: Vec<usize>) -> Self {
        let mut index = Self::default();
        index.insert_chunks(0, values);
        index
    }

    fn len(&self) -> usize {
        self.len
    }

    fn get(&self, index: usize) -> usize {
        let cached = self
            .cached_chunk
            .get()
            .min(self.chunks.len().saturating_sub(1));
        let cached_start = self.chunk_starts.get(cached).copied().unwrap_or(0);
        let cached_end = self
            .chunk_starts
            .get(cached + 1)
            .copied()
            .unwrap_or(self.len);
        let chunk_index = if cached_start <= index && index < cached_end {
            cached
        } else {
            let chunk = self.chunk_for_index(index);
            self.cached_chunk.set(chunk);
            chunk
        };
        let local = index - self.chunk_starts[chunk_index];
        self.chunks[chunk_index].value(local)
    }

    fn partition_point_le(&self, value: usize) -> usize {
        let chunk_index = self.chunks.partition_point(|chunk| chunk.last() <= value);
        if chunk_index == self.chunks.len() {
            return self.len;
        }
        let chunk = &self.chunks[chunk_index];
        self.chunk_starts[chunk_index]
            + chunk
                .values
                .partition_point(|entry| (*entry as isize + chunk.delta) as usize <= value)
    }

    fn shift_suffix(&mut self, index: usize, delta: isize) {
        if delta == 0 || index >= self.len {
            return;
        }
        let boundary = self.split_at(index);
        for chunk in &mut self.chunks[boundary..] {
            chunk.delta += delta;
        }
    }

    fn splice(&mut self, range: std::ops::Range<usize>, values: Vec<usize>) {
        let start = self.split_at(range.start.min(self.len));
        let end = self.split_at(range.end.min(self.len));
        self.chunks.drain(start..end);
        self.insert_chunks(start, values);
        self.compact_around(start);
        self.rebuild_chunk_starts();
    }

    fn chunk_for_index(&self, index: usize) -> usize {
        debug_assert!(index < self.len);
        self.chunk_starts
            .partition_point(|start| *start <= index)
            .saturating_sub(1)
    }

    fn split_at(&mut self, index: usize) -> usize {
        if index == self.len {
            return self.chunks.len();
        }
        let chunk_index = self.chunk_for_index(index);
        let local = index - self.chunk_starts[chunk_index];
        if local == 0 {
            return chunk_index;
        }
        let right_values = self.chunks[chunk_index].values.split_off(local);
        let delta = self.chunks[chunk_index].delta;
        self.chunks.insert(
            chunk_index + 1,
            OffsetChunk {
                values: right_values,
                delta,
            },
        );
        self.rebuild_chunk_starts();
        chunk_index + 1
    }

    fn insert_chunks(&mut self, at: usize, values: Vec<usize>) {
        if values.is_empty() {
            return;
        }
        let chunks = values
            .chunks(OFFSET_CHUNK_TARGET)
            .map(|values| OffsetChunk {
                values: values.to_vec(),
                delta: 0,
            })
            .collect::<Vec<_>>();
        self.chunks.splice(at..at, chunks);
        self.rebuild_chunk_starts();
    }

    fn compact_around(&mut self, at: usize) {
        if self.chunks.len() < 2 {
            return;
        }
        let mut index = at.saturating_sub(1).min(self.chunks.len() - 1);
        while index + 1 < self.chunks.len() && index <= at.saturating_add(1) {
            let combined_len =
                self.chunks[index].values.len() + self.chunks[index + 1].values.len();
            if combined_len <= OFFSET_CHUNK_MAX {
                let mut values = self.chunks[index].actual_values().collect::<Vec<_>>();
                values.extend(self.chunks[index + 1].actual_values());
                self.chunks[index] = OffsetChunk { values, delta: 0 };
                self.chunks.remove(index + 1);
            } else {
                index += 1;
            }
        }
    }

    fn rebuild_chunk_starts(&mut self) {
        self.chunk_starts.clear();
        self.len = 0;
        for chunk in &self.chunks {
            if chunk.values.is_empty() {
                continue;
            }
            self.chunk_starts.push(self.len);
            self.len += chunk.values.len();
        }
        self.cached_chunk.set(0);
    }

    #[cfg(test)]
    fn to_vec(&self) -> Vec<usize> {
        self.chunks
            .iter()
            .flat_map(OffsetChunk::actual_values)
            .collect()
    }
}

/// A contiguous text buffer with chunked, lazily shifted line-start indexes.
pub struct Buffer {
    text: String,
    line_starts: ChunkedOffsetIndex,
    line_utf16_starts: ChunkedOffsetIndex,
    line_char_starts: ChunkedOffsetIndex,
    record_edits: bool,
    recorded_edits: Vec<BufferEdit>,
}

impl Buffer {
    pub fn new() -> Self {
        let mut buf = Self {
            text: String::new(),
            line_starts: ChunkedOffsetIndex::default(),
            line_utf16_starts: ChunkedOffsetIndex::default(),
            line_char_starts: ChunkedOffsetIndex::default(),
            record_edits: false,
            recorded_edits: Vec::new(),
        };
        buf.rebuild_line_starts();
        buf
    }

    pub fn from_text(text: &str) -> Self {
        Self::from_string(text.to_string())
    }

    pub fn from_string(mut text: String) -> Self {
        if text.len() >= 100_000 {
            text.reserve(text.len() / 16);
        }
        let mut buf = Self {
            text,
            line_starts: ChunkedOffsetIndex::default(),
            line_utf16_starts: ChunkedOffsetIndex::default(),
            line_char_starts: ChunkedOffsetIndex::default(),
            record_edits: false,
            recorded_edits: Vec::new(),
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
        self.replace(offset..offset, text);
    }

    pub fn delete(&mut self, range: std::ops::Range<usize>) {
        self.replace(range, "");
    }

    pub fn replace(&mut self, range: std::ops::Range<usize>, text: &str) {
        let start = snap_char_start(&self.text, range.start.min(self.text.len()));
        let end = snap_char_start(&self.text, range.end.min(self.text.len()));
        if start <= end {
            let deleted_text = self.record_edits.then(|| self.text[start..end].to_string());
            let start_utf16 = self.utf16_offset_for_offset(start);
            let end_utf16 = self.utf16_offset_for_offset(end);
            let start_char = self.character_offset_for_offset(start);
            let end_char = self.character_offset_for_offset(end);
            self.text.replace_range(start..end, text);
            self.splice_line_starts(
                start,
                end,
                start_utf16,
                end_utf16,
                start_char,
                end_char,
                text,
            );
            if let Some(deleted_text) = deleted_text {
                self.recorded_edits.push(BufferEdit {
                    start,
                    deleted_text,
                    inserted_text: text.to_string(),
                });
            }
        }
    }

    pub(crate) fn begin_edit_recording(&mut self) {
        self.record_edits = true;
        self.recorded_edits.clear();
    }

    pub(crate) fn finish_edit_recording(&mut self) -> Vec<BufferEdit> {
        self.record_edits = false;
        std::mem::take(&mut self.recorded_edits)
    }

    pub(crate) fn apply_edits_forward(&mut self, edits: &[BufferEdit]) -> bool {
        for edit in edits {
            let end = edit.start + edit.deleted_text.len();
            if self.text.get(edit.start..end) != Some(edit.deleted_text.as_str()) {
                return false;
            }
            self.replace(edit.start..end, &edit.inserted_text);
        }
        true
    }

    pub(crate) fn apply_edits_inverse(&mut self, edits: &[BufferEdit]) -> bool {
        for edit in edits.iter().rev() {
            let end = edit.start + edit.inserted_text.len();
            if self.text.get(edit.start..end) != Some(edit.inserted_text.as_str()) {
                return false;
            }
            self.replace(edit.start..end, &edit.deleted_text);
        }
        true
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    pub fn line_start(&self, line: usize) -> usize {
        if line < self.line_starts.len() {
            self.line_starts.get(line)
        } else {
            self.text.len()
        }
    }

    pub fn line_end(&self, line: usize) -> usize {
        if line + 1 < self.line_starts.len() {
            let next_start = self.line_starts.get(line + 1);
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

    pub fn text_for_range(&self, range: std::ops::Range<usize>) -> &str {
        let start = self.clamp_offset(range.start);
        let end = self.clamp_offset(range.end);
        &self.text[start.min(end)..end]
    }

    pub fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.text.len());
        let line = self
            .line_starts
            .partition_point_le(offset)
            .saturating_sub(1);
        let col = offset - self.line_starts.get(line);
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
        self.line_utf16_starts.get(line) + self.line_content(line)[..col].encode_utf16().count()
    }

    pub fn offset_for_utf16_offset(&self, utf16_offset: usize) -> usize {
        let total_utf16 = self
            .line_utf16_starts
            .get(self.line_count().saturating_sub(1))
            + self
                .line_content(self.line_count().saturating_sub(1))
                .encode_utf16()
                .count();
        let utf16_offset = utf16_offset.min(total_utf16);
        let line = self
            .line_utf16_starts
            .partition_point_le(utf16_offset)
            .saturating_sub(1);
        let line_start_utf16 = self.line_utf16_starts.get(line);
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

    pub fn character_offset_for_offset(&self, offset: usize) -> usize {
        let offset = self.clamp_offset(offset);
        let (line, col) = self.offset_to_line_col(offset);
        self.line_char_starts.get(line) + self.line_content(line)[..col].chars().count()
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
        let mut line_starts = vec![0];
        let mut line_utf16_starts = vec![0];
        let mut line_char_starts = vec![0];
        let mut utf16_offset = 0usize;
        let mut char_offset = 0usize;
        for (i, ch) in self.text.char_indices() {
            utf16_offset += ch.len_utf16();
            char_offset += 1;
            if ch == '\n' {
                line_starts.push(i + ch.len_utf8());
                line_utf16_starts.push(utf16_offset);
                line_char_starts.push(char_offset);
            }
        }
        self.line_starts = ChunkedOffsetIndex::from_values(line_starts);
        self.line_utf16_starts = ChunkedOffsetIndex::from_values(line_utf16_starts);
        self.line_char_starts = ChunkedOffsetIndex::from_values(line_char_starts);
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
        start_char: usize,
        end_char: usize,
        inserted: &str,
    ) {
        let delta = inserted.len() as isize - (end - start) as isize;
        let inserted_utf16_len = inserted.encode_utf16().count();
        let inserted_char_len = inserted.chars().count();
        let utf16_delta =
            inserted_utf16_len as isize - end_utf16.saturating_sub(start_utf16) as isize;
        let removed_char_len = end_char.saturating_sub(start_char);
        let char_delta = inserted_char_len as isize - removed_char_len as isize;

        // Line starts in `(start, end]` correspond to newlines inside the removed
        // region and must be dropped; line starts strictly after `end` only shift
        // by the byte delta. `line_starts[0]` is always 0 (<= start) so it stays.
        let first = self.line_starts.partition_point_le(start);
        let last = self.line_starts.partition_point_le(end);

        let mut new_starts: Vec<usize> = Vec::new();
        let mut new_utf16_starts: Vec<usize> = Vec::new();
        let mut new_char_starts: Vec<usize> = Vec::new();
        let mut inserted_utf16_offset = 0usize;
        let mut inserted_char_offset = 0usize;
        for (i, ch) in inserted.char_indices() {
            inserted_utf16_offset += ch.len_utf16();
            inserted_char_offset += 1;
            if ch == '\n' {
                new_starts.push(start + i + ch.len_utf8());
                new_utf16_starts.push(start_utf16 + inserted_utf16_offset);
                new_char_starts.push(start_char + inserted_char_offset);
            }
        }

        self.line_starts.shift_suffix(last, delta);
        self.line_utf16_starts.shift_suffix(last, utf16_delta);
        self.line_char_starts.shift_suffix(last, char_delta);
        self.line_starts.splice(first..last, new_starts);
        self.line_utf16_starts.splice(first..last, new_utf16_starts);
        self.line_char_starts.splice(first..last, new_char_starts);
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
    use super::{
        Buffer, BufferEdit, clamp_grapheme_boundary, next_grapheme_boundary, prev_grapheme_boundary,
    };

    #[test]
    fn text_for_range_snaps_non_boundary_offsets() {
        let buffer = Buffer::from_text("ab。中文");
        assert_eq!(buffer.text_for_range(3..8), "。中");
        assert_eq!(buffer.text_for_range(999..1000), "");
    }

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

    fn expected_line_char_starts(text: &str) -> Vec<usize> {
        let mut starts = vec![0usize];
        let mut char_offset = 0usize;
        for ch in text.chars() {
            char_offset += 1;
            if ch == '\n' {
                starts.push(char_offset);
            }
        }
        starts
    }

    fn assert_line_starts_consistent(buffer: &Buffer) {
        assert_eq!(
            buffer.line_starts.to_vec(),
            expected_line_starts(buffer.text()),
            "incremental line_starts diverged from full rebuild for {:?}",
            buffer.text()
        );
        assert_eq!(
            buffer.line_utf16_starts.to_vec(),
            expected_line_utf16_starts(buffer.text()),
            "incremental line_utf16_starts diverged from full rebuild for {:?}",
            buffer.text()
        );
        assert_eq!(
            buffer.line_char_starts.to_vec(),
            expected_line_char_starts(buffer.text()),
            "incremental line_char_starts diverged from full rebuild for {:?}",
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
    fn records_multi_edit_transactions_without_full_snapshots() {
        let mut buffer = Buffer::from_text("alpha\nbeta\n");
        buffer.begin_edit_recording();
        buffer.insert(0, "X");
        buffer.replace(7..11, "B");
        buffer.delete(1..2);

        assert_eq!(buffer.text(), "Xlpha\nB\n");
        assert_eq!(
            buffer.finish_edit_recording(),
            vec![
                BufferEdit {
                    start: 0,
                    deleted_text: String::new(),
                    inserted_text: "X".to_string(),
                },
                BufferEdit {
                    start: 7,
                    deleted_text: "beta".to_string(),
                    inserted_text: "B".to_string(),
                },
                BufferEdit {
                    start: 1,
                    deleted_text: "a".to_string(),
                    inserted_text: String::new(),
                },
            ]
        );

        let edits = vec![
            BufferEdit {
                start: 0,
                deleted_text: String::new(),
                inserted_text: "X".to_string(),
            },
            BufferEdit {
                start: 7,
                deleted_text: "beta".to_string(),
                inserted_text: "B".to_string(),
            },
            BufferEdit {
                start: 1,
                deleted_text: "a".to_string(),
                inserted_text: String::new(),
            },
        ];
        assert!(buffer.apply_edits_inverse(&edits));
        assert_eq!(buffer.text(), "alpha\nbeta\n");
        assert!(buffer.apply_edits_forward(&edits));
        assert_eq!(buffer.text(), "Xlpha\nB\n");
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
