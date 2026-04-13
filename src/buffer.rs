/// A simple text buffer backed by a String with cached line start offsets.
pub struct Buffer {
    text: String,
    line_starts: Vec<usize>,
}

impl Buffer {
    pub fn new() -> Self {
        let mut buf = Self {
            text: String::new(),
            line_starts: Vec::new(),
        };
        buf.rebuild_line_starts();
        buf
    }

    pub fn from_text(text: &str) -> Self {
        let mut buf = Self {
            text: text.to_string(),
            line_starts: Vec::new(),
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
        self.text.insert_str(offset, text);
        self.rebuild_line_starts();
    }

    pub fn delete(&mut self, range: std::ops::Range<usize>) {
        let start = snap_char_start(&self.text, range.start.min(self.text.len()));
        let end = snap_char_start(&self.text, range.end.min(self.text.len()));
        if start < end {
            self.text.drain(start..end);
            self.rebuild_line_starts();
        }
    }

    pub fn replace(&mut self, range: std::ops::Range<usize>, text: &str) {
        let start = snap_char_start(&self.text, range.start.min(self.text.len()));
        let end = snap_char_start(&self.text, range.end.min(self.text.len()));
        if start <= end {
            self.text.replace_range(start..end, text);
            self.rebuild_line_starts();
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

    pub fn clamp_offset(&self, offset: usize) -> usize {
        let offset = offset.min(self.text.len());
        let mut o = offset;
        while o > 0 && !self.text.is_char_boundary(o) {
            o -= 1;
        }
        o
    }
 
    fn rebuild_line_starts(&mut self) {
        self.line_starts.clear();
        self.line_starts.push(0);
        for (i, byte) in self.text.bytes().enumerate() {
            if byte == b'\n' {
                self.line_starts.push(i + 1);
            }
        }
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
