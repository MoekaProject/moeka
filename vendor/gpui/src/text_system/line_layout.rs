use crate::{FontId, GlyphId, Pixels, PlatformTextSystem, Point, SharedString, Size, point, px};
use collections::FxHashMap;
use parking_lot::{Mutex, RwLock, RwLockUpgradableReadGuard};
use smallvec::SmallVec;
use std::{
    borrow::Borrow,
    hash::{Hash, Hasher},
    ops::Range,
    sync::Arc,
};

use super::LineWrapper;

/// A laid out and styled line of text
#[derive(Default, Debug)]
pub struct LineLayout {
    /// The font size for this line
    pub font_size: Pixels,
    /// The width of the line
    pub width: Pixels,
    /// The ascent of the line
    pub ascent: Pixels,
    /// The descent of the line
    pub descent: Pixels,
    /// The shaped runs that make up this line
    pub runs: Vec<ShapedRun>,
    /// The length of the line in utf-8 bytes
    pub len: usize,
}

/// A run of text that has been shaped .
#[derive(Debug, Clone)]
pub struct ShapedRun {
    /// The font id for this run
    pub font_id: FontId,
    /// The glyphs that make up this run
    pub glyphs: Vec<ShapedGlyph>,
}

/// A single glyph, ready to paint.
#[derive(Clone, Debug)]
pub struct ShapedGlyph {
    /// The ID for this glyph, as determined by the text system.
    pub id: GlyphId,

    /// The position of this glyph in its containing line.
    pub position: Point<Pixels>,

    /// The index of this glyph in the original text.
    pub index: usize,

    /// Whether this glyph is an emoji
    pub is_emoji: bool,
}

impl LineLayout {
    /// The index for the character at the given x coordinate
    pub fn index_for_x(&self, x: Pixels) -> Option<usize> {
        if x >= self.width {
            None
        } else {
            for run in self.runs.iter().rev() {
                for glyph in run.glyphs.iter().rev() {
                    if glyph.position.x <= x {
                        return Some(glyph.index);
                    }
                }
            }
            Some(0)
        }
    }

    /// closest_index_for_x returns the character boundary closest to the given x coordinate
    /// (e.g. to handle aligning up/down arrow keys)
    pub fn closest_index_for_x(&self, x: Pixels) -> usize {
        let mut prev_index = 0;
        let mut prev_x = px(0.);

        for run in self.runs.iter() {
            for glyph in run.glyphs.iter() {
                if glyph.position.x >= x {
                    if glyph.position.x - x < x - prev_x {
                        return glyph.index;
                    } else {
                        return prev_index;
                    }
                }
                prev_index = glyph.index;
                prev_x = glyph.position.x;
            }
        }

        if self.len == 1 {
            if x > self.width / 2. {
                return 1;
            } else {
                return 0;
            }
        }

        self.len
    }

    /// The x position of the character at the given index
    pub fn x_for_index(&self, index: usize) -> Pixels {
        for run in &self.runs {
            for glyph in &run.glyphs {
                if glyph.index >= index {
                    return glyph.position.x;
                }
            }
        }
        self.width
    }

    /// The corresponding Font at the given index
    pub fn font_id_for_index(&self, index: usize) -> Option<FontId> {
        for run in &self.runs {
            for glyph in &run.glyphs {
                if glyph.index >= index {
                    return Some(run.font_id);
                }
            }
        }
        None
    }

    pub(super) fn compute_wrap_boundaries(
        &self,
        text: &str,
        wrap_width: Pixels,
        max_lines: Option<usize>,
    ) -> SmallVec<[WrapBoundary; 1]> {
        let mut boundaries = SmallVec::new();
        let mut first_non_whitespace_ix = None;
        let mut last_candidate_ix = None;
        let mut last_candidate_x = px(0.);
        let mut last_boundary = WrapBoundary {
            run_ix: 0,
            glyph_ix: 0,
        };
        let mut last_boundary_x = px(0.);
        let mut prev_ch = '\0';
        let mut glyphs = self
            .runs
            .iter()
            .enumerate()
            .flat_map(move |(run_ix, run)| {
                run.glyphs.iter().enumerate().map(move |(glyph_ix, glyph)| {
                    let character = text[glyph.index..].chars().next().unwrap();
                    (
                        WrapBoundary { run_ix, glyph_ix },
                        character,
                        glyph.position.x,
                    )
                })
            })
            .peekable();

        while let Some((boundary, ch, x)) = glyphs.next() {
            if ch == '\n' {
                continue;
            }

            // Here is very similar to `LineWrapper::wrap_line` to determine text wrapping,
            // but there are some differences, so we have to duplicate the code here.
            if LineWrapper::is_word_char(ch) {
                if prev_ch == ' ' && ch != ' ' && first_non_whitespace_ix.is_some() {
                    last_candidate_ix = Some(boundary);
                    last_candidate_x = x;
                }
            } else {
                if ch != ' ' && first_non_whitespace_ix.is_some() {
                    last_candidate_ix = Some(boundary);
                    last_candidate_x = x;
                }
            }

            if ch != ' ' && first_non_whitespace_ix.is_none() {
                first_non_whitespace_ix = Some(boundary);
            }

            let next_x = glyphs.peek().map_or(self.width, |(_, _, x)| *x);
            let width = next_x - last_boundary_x;

            if width > wrap_width && boundary > last_boundary {
                // When used line_clamp, we should limit the number of lines.
                if let Some(max_lines) = max_lines
                    && boundaries.len() >= max_lines - 1
                {
                    break;
                }

                if let Some(last_candidate_ix) = last_candidate_ix.take() {
                    last_boundary = last_candidate_ix;
                    last_boundary_x = last_candidate_x;
                } else {
                    last_boundary = boundary;
                    last_boundary_x = x;
                }
                boundaries.push(last_boundary);
            }
            prev_ch = ch;
        }

        boundaries
    }
}

/// A line of text that has been wrapped to fit a given width
#[derive(Clone, Default, Debug)]
pub struct WrappedLineLayout {
    /// The line layout, pre-wrapping.
    pub unwrapped_layout: Arc<LineLayout>,

    /// The boundaries at which the line was wrapped
    pub wrap_boundaries: SmallVec<[WrapBoundary; 1]>,

    /// The width of the line, if it was wrapped
    pub wrap_width: Option<Pixels>,

    /// Heights of individual soft-wrapped rows. Empty means a uniform fallback height.
    /// Nonempty metadata must contain one positive, finite height per row and a finite total.
    pub row_heights: Vec<Pixels>,
}

/// A boundary at which a line was wrapped
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WrapBoundary {
    /// The run containing the first glyph of the next wrapped row.
    pub run_ix: usize,
    /// The index of the first glyph of the next wrapped row within that run.
    pub glyph_ix: usize,
}

impl WrappedLineLayout {
    /// The length of the underlying text, in utf8 bytes.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.unwrapped_layout.len
    }

    /// The width of this line, in pixels, whether or not it was wrapped.
    pub fn width(&self) -> Pixels {
        self.wrap_width
            .unwrap_or(Pixels::MAX)
            .min(self.unwrapped_layout.width)
    }

    pub(super) fn valid_row_heights(&self) -> &[Pixels] {
        if self.row_heights.len() == self.wrap_boundaries.len() + 1
            && self
                .row_heights
                .iter()
                .try_fold(Pixels::ZERO, |top, height| {
                    let bottom = top + *height;
                    (height.0.is_finite() && *height > Pixels::ZERO && bottom.0.is_finite())
                        .then_some(bottom)
                })
                .is_some()
        {
            &self.row_heights
        } else {
            &[]
        }
    }

    /// The height of a soft-wrapped row, or `fallback` when no valid metadata exists.
    /// Out-of-range rows also return `fallback`.
    pub fn row_height(&self, row: usize, fallback: Pixels) -> Pixels {
        self.valid_row_heights()
            .get(row)
            .copied()
            .unwrap_or(fallback)
    }

    /// The top of a soft-wrapped row, relative to this layout's origin.
    /// Rows at or beyond the row count return the total height.
    pub fn row_top(&self, row: usize, fallback: Pixels) -> Pixels {
        let row = row.min(self.wrap_boundaries.len() + 1);
        let row_heights = self.valid_row_heights();
        if row_heights.is_empty() {
            fallback * row
        } else {
            row_heights[..row]
                .iter()
                .copied()
                .fold(Pixels::ZERO, |top, height| top + height)
        }
    }

    /// The row containing `y`, relative to this layout's origin.
    /// Negative coordinates select row zero; exact row bottoms select the next row.
    /// Coordinates at or below the total height return the row count (out of bounds).
    pub fn row_for_y(&self, y: Pixels, fallback: Pixels) -> usize {
        if y <= Pixels::ZERO {
            return 0;
        }
        let row_count = self.wrap_boundaries.len() + 1;
        let row_heights = self.valid_row_heights();
        if row_heights.is_empty() {
            if y >= fallback * row_count {
                return row_count;
            }
            let mut row = ((y / fallback) as usize).min(row_count - 1);
            // Multiplication and division can round differently. Resolve
            // exact row tops against the same geometry used by `row_top`.
            if y < fallback * row {
                row = row.saturating_sub(1);
            } else if y >= fallback * (row + 1) {
                row += 1;
            }
            return row;
        }
        let mut bottom = Pixels::ZERO;
        for (row, height) in row_heights.iter().enumerate() {
            bottom += *height;
            if y < bottom {
                return row;
            }
        }
        row_count
    }

    /// The size of the whole wrapped text, using `line_height` as the uniform fallback.
    pub fn size(&self, line_height: Pixels) -> Size<Pixels> {
        Size {
            width: self.width(),
            height: self.row_top(self.wrap_boundaries.len() + 1, line_height),
        }
    }

    /// The ascent of a line in this layout
    pub fn ascent(&self) -> Pixels {
        self.unwrapped_layout.ascent
    }

    /// The descent of a line in this layout
    pub fn descent(&self) -> Pixels {
        self.unwrapped_layout.descent
    }

    /// The wrap boundaries in this layout
    pub fn wrap_boundaries(&self) -> &[WrapBoundary] {
        &self.wrap_boundaries
    }

    /// The font size of this layout
    pub fn font_size(&self) -> Pixels {
        self.unwrapped_layout.font_size
    }

    /// The runs in this layout, sans wrapping
    pub fn runs(&self) -> &[ShapedRun] {
        &self.unwrapped_layout.runs
    }

    /// The index corresponding to a given position in this layout for the given line height.
    ///
    /// See also [`Self::closest_index_for_position`].
    pub fn index_for_position(
        &self,
        position: Point<Pixels>,
        line_height: Pixels,
    ) -> Result<usize, usize> {
        self._index_for_position(position, line_height, false)
    }

    /// The closest index to a given position in this layout for the given line height.
    ///
    /// Closest means the character boundary closest to the given position.
    ///
    /// See also [`LineLayout::closest_index_for_x`].
    pub fn closest_index_for_position(
        &self,
        position: Point<Pixels>,
        line_height: Pixels,
    ) -> Result<usize, usize> {
        self._index_for_position(position, line_height, true)
    }

    fn _index_for_position(
        &self,
        position: Point<Pixels>,
        line_height: Pixels,
        closest: bool,
    ) -> Result<usize, usize> {
        let wrapped_line_ix = self.row_for_y(position.y, line_height);

        let wrapped_line_start_index;
        let wrapped_line_start_x;
        if wrapped_line_ix > 0 {
            let Some(line_start_boundary) = self.wrap_boundaries.get(wrapped_line_ix - 1) else {
                return Err(0);
            };
            let run = &self.unwrapped_layout.runs[line_start_boundary.run_ix];
            let glyph = &run.glyphs[line_start_boundary.glyph_ix];
            wrapped_line_start_index = glyph.index;
            wrapped_line_start_x = glyph.position.x;
        } else {
            wrapped_line_start_index = 0;
            wrapped_line_start_x = Pixels::ZERO;
        };

        let wrapped_line_end_index;
        let wrapped_line_end_x;
        if wrapped_line_ix < self.wrap_boundaries.len() {
            let next_wrap_boundary_ix = wrapped_line_ix;
            let next_wrap_boundary = self.wrap_boundaries[next_wrap_boundary_ix];
            let run = &self.unwrapped_layout.runs[next_wrap_boundary.run_ix];
            let glyph = &run.glyphs[next_wrap_boundary.glyph_ix];
            wrapped_line_end_index = glyph.index;
            wrapped_line_end_x = glyph.position.x;
        } else {
            wrapped_line_end_index = self.unwrapped_layout.len;
            wrapped_line_end_x = self.unwrapped_layout.width;
        };

        let mut position_in_unwrapped_line = position;
        position_in_unwrapped_line.x += wrapped_line_start_x;
        if position_in_unwrapped_line.x < wrapped_line_start_x {
            Err(wrapped_line_start_index)
        } else if position_in_unwrapped_line.x >= wrapped_line_end_x {
            Err(wrapped_line_end_index)
        } else {
            if closest {
                Ok(self
                    .unwrapped_layout
                    .closest_index_for_x(position_in_unwrapped_line.x))
            } else {
                Ok(self
                    .unwrapped_layout
                    .index_for_x(position_in_unwrapped_line.x)
                    .unwrap())
            }
        }
    }

    /// Returns the pixel position for the given byte index.
    /// An index exactly at a wrap boundary belongs to the preceding row's end.
    pub fn position_for_index(&self, index: usize, line_height: Pixels) -> Option<Point<Pixels>> {
        let mut line_start_ix = 0;
        let line_end_indices = self
            .wrap_boundaries
            .iter()
            .map(|wrap_boundary| {
                let run = &self.unwrapped_layout.runs[wrap_boundary.run_ix];
                let glyph = &run.glyphs[wrap_boundary.glyph_ix];
                glyph.index
            })
            .chain([self.len()])
            .enumerate();
        for (ix, line_end_ix) in line_end_indices {
            if index < line_start_ix {
                break;
            } else if index > line_end_ix {
                line_start_ix = line_end_ix;
                continue;
            } else {
                let line_start_x = self.unwrapped_layout.x_for_index(line_start_ix);
                let x = self.unwrapped_layout.x_for_index(index) - line_start_x;
                return Some(point(x, self.row_top(ix, line_height)));
            }
        }

        None
    }
}

pub(crate) struct LineLayoutCache {
    previous_frame: Mutex<FrameCache>,
    current_frame: RwLock<FrameCache>,
    platform_text_system: Arc<dyn PlatformTextSystem>,
}

#[derive(Default)]
struct FrameCache {
    lines: FxHashMap<Arc<CacheKey>, Arc<LineLayout>>,
    wrapped_lines: FxHashMap<Arc<CacheKey>, Arc<WrappedLineLayout>>,
    used_lines: Vec<Arc<CacheKey>>,
    used_wrapped_lines: Vec<Arc<CacheKey>>,
}

#[derive(Clone, Default)]
pub(crate) struct LineLayoutIndex {
    lines_index: usize,
    wrapped_lines_index: usize,
}

impl LineLayoutCache {
    pub fn new(platform_text_system: Arc<dyn PlatformTextSystem>) -> Self {
        Self {
            previous_frame: Mutex::default(),
            current_frame: RwLock::default(),
            platform_text_system,
        }
    }

    pub fn layout_index(&self) -> LineLayoutIndex {
        let frame = self.current_frame.read();
        LineLayoutIndex {
            lines_index: frame.used_lines.len(),
            wrapped_lines_index: frame.used_wrapped_lines.len(),
        }
    }

    pub fn reuse_layouts(&self, range: Range<LineLayoutIndex>) {
        let mut previous_frame = &mut *self.previous_frame.lock();
        let mut current_frame = &mut *self.current_frame.write();

        for key in &previous_frame.used_lines[range.start.lines_index..range.end.lines_index] {
            if let Some((key, line)) = previous_frame.lines.remove_entry(key) {
                current_frame.lines.insert(key, line);
            }
            current_frame.used_lines.push(key.clone());
        }

        for key in &previous_frame.used_wrapped_lines
            [range.start.wrapped_lines_index..range.end.wrapped_lines_index]
        {
            if let Some((key, line)) = previous_frame.wrapped_lines.remove_entry(key) {
                current_frame.wrapped_lines.insert(key, line);
            }
            current_frame.used_wrapped_lines.push(key.clone());
        }
    }

    pub fn truncate_layouts(&self, index: LineLayoutIndex) {
        let mut current_frame = &mut *self.current_frame.write();
        current_frame.used_lines.truncate(index.lines_index);
        current_frame
            .used_wrapped_lines
            .truncate(index.wrapped_lines_index);
    }

    pub fn finish_frame(&self) {
        let mut prev_frame = self.previous_frame.lock();
        let mut curr_frame = self.current_frame.write();
        std::mem::swap(&mut *prev_frame, &mut *curr_frame);
        curr_frame.lines.clear();
        curr_frame.wrapped_lines.clear();
        curr_frame.used_lines.clear();
        curr_frame.used_wrapped_lines.clear();
    }

    pub fn layout_wrapped_line<Text>(
        &self,
        text: Text,
        font_size: Pixels,
        runs: &[FontRun],
        wrap_width: Option<Pixels>,
        max_lines: Option<usize>,
    ) -> Arc<WrappedLineLayout>
    where
        Text: AsRef<str>,
        SharedString: From<Text>,
    {
        let key = &CacheKeyRef {
            text: text.as_ref(),
            font_size,
            runs,
            wrap_width,
            force_width: None,
        } as &dyn AsCacheKeyRef;

        let current_frame = self.current_frame.upgradable_read();
        if let Some(layout) = current_frame.wrapped_lines.get(key) {
            return layout.clone();
        }

        let previous_frame_entry = self.previous_frame.lock().wrapped_lines.remove_entry(key);
        if let Some((key, layout)) = previous_frame_entry {
            let mut current_frame = RwLockUpgradableReadGuard::upgrade(current_frame);
            current_frame
                .wrapped_lines
                .insert(key.clone(), layout.clone());
            current_frame.used_wrapped_lines.push(key);
            layout
        } else {
            drop(current_frame);
            let text = SharedString::from(text);
            let unwrapped_layout = self.layout_line::<&SharedString>(&text, font_size, runs, None);
            let wrap_boundaries = if let Some(wrap_width) = wrap_width {
                unwrapped_layout.compute_wrap_boundaries(text.as_ref(), wrap_width, max_lines)
            } else {
                SmallVec::new()
            };
            let layout = Arc::new(WrappedLineLayout {
                unwrapped_layout,
                wrap_boundaries,
                wrap_width,
                row_heights: Vec::new(),
            });
            let key = Arc::new(CacheKey {
                text,
                font_size,
                runs: SmallVec::from(runs),
                wrap_width,
                force_width: None,
            });

            let mut current_frame = self.current_frame.write();
            current_frame
                .wrapped_lines
                .insert(key.clone(), layout.clone());
            current_frame.used_wrapped_lines.push(key);

            layout
        }
    }

    pub fn layout_line<Text>(
        &self,
        text: Text,
        font_size: Pixels,
        runs: &[FontRun],
        force_width: Option<Pixels>,
    ) -> Arc<LineLayout>
    where
        Text: AsRef<str>,
        SharedString: From<Text>,
    {
        let key = &CacheKeyRef {
            text: text.as_ref(),
            font_size,
            runs,
            wrap_width: None,
            force_width,
        } as &dyn AsCacheKeyRef;

        let current_frame = self.current_frame.upgradable_read();
        if let Some(layout) = current_frame.lines.get(key) {
            return layout.clone();
        }

        let mut current_frame = RwLockUpgradableReadGuard::upgrade(current_frame);
        if let Some((key, layout)) = self.previous_frame.lock().lines.remove_entry(key) {
            current_frame.lines.insert(key.clone(), layout.clone());
            current_frame.used_lines.push(key);
            layout
        } else {
            let text = SharedString::from(text);
            let mut layout = self
                .platform_text_system
                .layout_line(&text, font_size, runs);

            if let Some(force_width) = force_width {
                let mut glyph_pos = 0;
                for run in layout.runs.iter_mut() {
                    for glyph in run.glyphs.iter_mut() {
                        if (glyph.position.x - glyph_pos * force_width).abs() > px(1.) {
                            glyph.position.x = glyph_pos * force_width;
                        }
                        glyph_pos += 1;
                    }
                }
            }

            let key = Arc::new(CacheKey {
                text,
                font_size,
                runs: SmallVec::from(runs),
                wrap_width: None,
                force_width,
            });
            let layout = Arc::new(layout);
            current_frame.lines.insert(key.clone(), layout.clone());
            current_frame.used_lines.push(key);
            layout
        }
    }
}

/// A run of text with a single font.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct FontRun {
    pub(crate) len: usize,
    pub(crate) font_id: FontId,
}

trait AsCacheKeyRef {
    fn as_cache_key_ref(&self) -> CacheKeyRef<'_>;
}

#[derive(Clone, Debug, Eq)]
struct CacheKey {
    text: SharedString,
    font_size: Pixels,
    runs: SmallVec<[FontRun; 1]>,
    wrap_width: Option<Pixels>,
    force_width: Option<Pixels>,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct CacheKeyRef<'a> {
    text: &'a str,
    font_size: Pixels,
    runs: &'a [FontRun],
    wrap_width: Option<Pixels>,
    force_width: Option<Pixels>,
}

impl PartialEq for dyn AsCacheKeyRef + '_ {
    fn eq(&self, other: &dyn AsCacheKeyRef) -> bool {
        self.as_cache_key_ref() == other.as_cache_key_ref()
    }
}

impl Eq for dyn AsCacheKeyRef + '_ {}

impl Hash for dyn AsCacheKeyRef + '_ {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_cache_key_ref().hash(state)
    }
}

impl AsCacheKeyRef for CacheKey {
    fn as_cache_key_ref(&self) -> CacheKeyRef<'_> {
        CacheKeyRef {
            text: &self.text,
            font_size: self.font_size,
            runs: self.runs.as_slice(),
            wrap_width: self.wrap_width,
            force_width: self.force_width,
        }
    }
}

impl PartialEq for CacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.as_cache_key_ref().eq(&other.as_cache_key_ref())
    }
}

impl Hash for CacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_cache_key_ref().hash(state);
    }
}

impl<'a> Borrow<dyn AsCacheKeyRef + 'a> for Arc<CacheKey> {
    fn borrow(&self) -> &(dyn AsCacheKeyRef + 'a) {
        self.as_ref() as &dyn AsCacheKeyRef
    }
}

impl AsCacheKeyRef for CacheKeyRef<'_> {
    fn as_cache_key_ref(&self) -> CacheKeyRef<'_> {
        *self
    }
}

#[cfg(test)]
pub(super) fn test_wrapped_layout(row_heights: Vec<Pixels>) -> WrappedLineLayout {
    WrappedLineLayout {
        unwrapped_layout: Arc::new(LineLayout {
            font_size: px(18.),
            width: px(60.),
            ascent: px(18.),
            descent: px(6.),
            runs: vec![ShapedRun {
                font_id: FontId(0),
                glyphs: (0..6)
                    .map(|index| ShapedGlyph {
                        id: GlyphId(0),
                        position: point(px(index as f32 * 10.), Pixels::ZERO),
                        index,
                        is_emoji: false,
                    })
                    .collect(),
            }],
            len: 6,
        }),
        wrap_boundaries: smallvec::smallvec![
            WrapBoundary {
                run_ix: 0,
                glyph_ix: 2,
            },
            WrapBoundary {
                run_ix: 0,
                glyph_ix: 4,
            },
        ],
        wrap_width: Some(px(20.)),
        row_heights,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_row_heights_and_exact_boundaries() {
        let fallback = px(24.);
        for heights in [[24., 60., 24.], [60., 24., 24.]] {
            let layout = test_wrapped_layout(heights.map(px).to_vec());
            assert_eq!(
                layout.size(fallback),
                Size {
                    width: px(20.),
                    height: px(108.)
                }
            );
            assert_eq!(layout.row_for_y(px(-1.), fallback), 0);
            assert_eq!(layout.row_for_y(px(0.), fallback), 0);
            let mut top = Pixels::ZERO;
            for (row, height) in heights.into_iter().enumerate() {
                assert_eq!(layout.row_height(row, fallback), px(height));
                assert_eq!(layout.row_top(row, fallback), top);
                assert_eq!(layout.row_for_y(top, fallback), row);
                top += px(height);
                assert_eq!(layout.row_for_y(top - px(0.25), fallback), row);
                assert_eq!(layout.row_for_y(top, fallback), row + 1);
            }
            assert_eq!(layout.row_top(3, fallback), px(108.));
            assert_eq!(layout.row_top(usize::MAX, fallback), px(108.));
            assert_eq!(layout.row_height(usize::MAX, fallback), fallback);
            assert_eq!(layout.row_for_y(px(109.), fallback), 3);
            assert_eq!(layout.row_for_y(px(f32::INFINITY), fallback), 3);
        }
    }

    #[test]
    fn position_for_index_preserves_upstream_wrap_affinity() {
        let fallback = px(24.);
        for heights in [[24., 60., 24.], [60., 24., 24.]] {
            let layout = test_wrapped_layout(heights.map(px).to_vec());
            for (index, x, y) in [
                (0, 0., 0.),
                (1, 10., 0.),
                (2, 20., 0.),
                (3, 10., heights[0]),
                (4, 20., heights[0]),
                (5, 10., heights[0] + heights[1]),
                (6, 20., heights[0] + heights[1]),
            ] {
                assert_eq!(
                    layout.position_for_index(index, fallback),
                    Some(point(px(x), px(y)))
                );
            }
            assert_eq!(layout.position_for_index(7, fallback), None);
        }
    }

    #[test]
    fn utf8_byte_indices_and_wrap_affinity_survive_multiple_font_runs() {
        let text = "aé中😀bé";
        let mut runs = test_wrapped_layout(Vec::new())
            .unwrapped_layout
            .runs
            .clone();
        for (glyph, index) in runs[0].glyphs.iter_mut().zip([0, 1, 3, 6, 10, 11]) {
            glyph.index = index;
        }
        let glyphs = runs[0].glyphs.split_off(2);
        runs.push(ShapedRun {
            font_id: FontId(1),
            glyphs,
        });
        let layout = WrappedLineLayout {
            unwrapped_layout: Arc::new(LineLayout {
                runs,
                width: px(60.),
                len: text.len(),
                ..Default::default()
            }),
            wrap_boundaries: smallvec::smallvec![
                WrapBoundary {
                    run_ix: 1,
                    glyph_ix: 0
                },
                WrapBoundary {
                    run_ix: 1,
                    glyph_ix: 2
                },
            ],
            wrap_width: Some(px(20.)),
            row_heights: vec![px(24.), px(60.), px(24.)],
        };
        for (index, x, y) in [
            (0, 0., 0.),
            (1, 10., 0.),
            (2, 20., 0.),
            (3, 20., 0.),
            (4, 10., 24.),
            (6, 10., 24.),
            (7, 20., 24.),
            (10, 20., 24.),
            (11, 10., 84.),
            (12, 20., 84.),
            (13, 20., 84.),
        ] {
            assert_eq!(
                layout.position_for_index(index, px(24.)),
                Some(point(px(x), px(y)))
            );
        }
        assert_eq!(layout.position_for_index(text.len() + 1, px(24.)), None);
        for (row, start, next, end) in [(0, 0, 1, 3), (1, 3, 6, 10), (2, 10, 11, 13)] {
            let y = layout.row_top(row, px(24.));
            assert_eq!(
                layout.index_for_position(point(px(6.), y), px(24.)),
                Ok(start)
            );
            assert_eq!(
                layout.closest_index_for_position(point(px(6.), y), px(24.)),
                Ok(next)
            );
            assert!(text.is_char_boundary(next));
            for closest in [false, true] {
                assert_eq!(
                    layout._index_for_position(point(px(-1.), y), px(24.), closest),
                    Err(start)
                );
                assert_eq!(
                    layout._index_for_position(point(px(20.), y), px(24.), closest),
                    Err(end)
                );
            }
        }
    }

    #[test]
    fn hit_testing_uses_actual_rows_and_retains_out_of_bounds_errors() {
        let fallback = px(24.);
        for heights in [[24., 60., 24.], [60., 24., 24.]] {
            let layout = test_wrapped_layout(heights.map(px).to_vec());
            let mut top = Pixels::ZERO;
            for (row, height) in heights.into_iter().enumerate() {
                let start = row * 2;
                for y in [top, top + px(height / 2.), top + px(height) - px(0.25)] {
                    assert_eq!(
                        layout.index_for_position(point(px(6.), y), fallback),
                        Ok(start)
                    );
                    assert_eq!(
                        layout.closest_index_for_position(point(px(6.), y), fallback),
                        Ok(start + 1)
                    );
                    for closest in [false, true] {
                        assert_eq!(
                            layout._index_for_position(point(px(-1.), y), fallback, closest),
                            Err(start)
                        );
                        assert_eq!(
                            layout._index_for_position(point(px(20.), y), fallback, closest),
                            Err(start + 2)
                        );
                    }
                }
                top += px(height);
            }
            for closest in [false, true] {
                assert_eq!(
                    layout._index_for_position(point(px(1.), px(-1.)), fallback, closest),
                    Ok(0)
                );
                assert_eq!(
                    layout._index_for_position(point(px(1.), top), fallback, closest),
                    Err(0)
                );
                assert_eq!(
                    layout._index_for_position(point(px(1.), top + px(100.)), fallback, closest),
                    Err(0)
                );
            }
        }
    }

    #[test]
    fn empty_or_invalid_metadata_uses_uniform_fallback() {
        let fallback = px(24.);
        for heights in [
            vec![],
            vec![px(60.)],
            vec![px(24.), px(60.)],
            vec![px(24.), px(60.), px(24.), px(24.)],
            vec![px(24.), px(0.), px(24.)],
            vec![px(24.), px(-1.), px(24.)],
            vec![px(24.), px(f32::NAN), px(24.)],
            vec![px(24.), px(f32::INFINITY), px(24.)],
            vec![px(24.), px(f32::NEG_INFINITY), px(24.)],
            vec![px(f32::MAX), px(f32::MAX), px(24.)],
        ] {
            let layout = test_wrapped_layout(heights);
            assert!(layout.valid_row_heights().is_empty());
            assert_eq!(layout.size(fallback).height, px(72.));
            for row in 0..3 {
                let top = fallback * row;
                assert_eq!(layout.row_height(row, fallback), fallback);
                assert_eq!(layout.row_top(row, fallback), top);
                assert_eq!(layout.row_for_y(top, fallback), row);
                assert_eq!(layout.row_for_y(top + px(23.), fallback), row);
                assert_eq!(
                    layout.index_for_position(point(px(1.), top), fallback),
                    Ok(row * 2)
                );
                assert_eq!(
                    layout.closest_index_for_position(point(px(6.), top), fallback),
                    Ok(row * 2 + 1)
                );
            }
            assert_eq!(
                layout.position_for_index(5, fallback),
                Some(point(px(10.), px(48.)))
            );
            assert_eq!(layout.row_for_y(px(72.), fallback), 3);
            assert_eq!(
                layout.index_for_position(point(px(1.), px(72.)), fallback),
                Err(0)
            );
            assert_eq!(
                layout.closest_index_for_position(point(px(1.), px(72.)), fallback),
                Err(0)
            );
        }
    }

    #[test]
    fn empty_text_still_has_one_row() {
        let layout = WrappedLineLayout {
            row_heights: vec![px(60.)],
            ..Default::default()
        };
        assert_eq!(layout.row_height(0, px(24.)), px(60.));
        assert_eq!(layout.size(px(24.)).height, px(60.));
        assert_eq!(layout.row_top(1, px(24.)), px(60.));
        assert_eq!(layout.row_for_y(px(59.), px(24.)), 0);
        assert_eq!(layout.row_for_y(px(60.), px(24.)), 1);
        assert_eq!(
            layout.position_for_index(0, px(24.)),
            Some(point(Pixels::ZERO, Pixels::ZERO))
        );
        assert_eq!(
            layout.index_for_position(point(Pixels::ZERO, px(60.)), px(24.)),
            Err(0)
        );
    }
}
