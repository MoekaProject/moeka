use crate::{
    App, Bounds, Half, Hsla, LineLayout, Pixels, Point, Result, SharedString, StrikethroughStyle,
    TextAlign, UnderlineStyle, Window, WrapBoundary, WrappedLineLayout, black, fill, point, px,
    size,
};
use derive_more::{Deref, DerefMut};
use smallvec::SmallVec;
use std::sync::Arc;

/// Set the text decoration for a run of text.
#[derive(Debug, Clone)]
pub struct DecorationRun {
    /// The length of the run in utf-8 bytes.
    pub len: u32,

    /// The color for this run
    pub color: Hsla,

    /// The background color for this run
    pub background_color: Option<Hsla>,

    /// The underline style for this run
    pub underline: Option<UnderlineStyle>,

    /// The strikethrough style for this run
    pub strikethrough: Option<StrikethroughStyle>,
}

/// A line of text that has been shaped and decorated.
#[derive(Clone, Default, Debug, Deref, DerefMut)]
pub struct ShapedLine {
    #[deref]
    #[deref_mut]
    pub(crate) layout: Arc<LineLayout>,
    /// The text that was shaped for this line.
    pub text: SharedString,
    pub(crate) decoration_runs: SmallVec<[DecorationRun; 32]>,
}

impl ShapedLine {
    /// The length of the line in utf-8 bytes.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.layout.len
    }

    /// Override the len, useful if you're rendering text a
    /// as text b (e.g. rendering invisibles).
    pub fn with_len(mut self, len: usize) -> Self {
        let layout = self.layout.as_ref();
        self.layout = Arc::new(LineLayout {
            font_size: layout.font_size,
            width: layout.width,
            ascent: layout.ascent,
            descent: layout.descent,
            runs: layout.runs.clone(),
            len,
        });
        self
    }

    /// Paint the line of text to the window.
    pub fn paint(
        &self,
        origin: Point<Pixels>,
        line_height: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<()> {
        paint_line(
            origin,
            &self.layout,
            line_height,
            TextAlign::default(),
            None,
            &self.decoration_runs,
            &[],
            &[],
            window,
            cx,
        )?;

        Ok(())
    }

    /// Paint the background of the line to the window.
    pub fn paint_background(
        &self,
        origin: Point<Pixels>,
        line_height: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<()> {
        paint_line_background(
            origin,
            &self.layout,
            line_height,
            TextAlign::default(),
            None,
            &self.decoration_runs,
            &[],
            &[],
            window,
            cx,
        )?;

        Ok(())
    }
}

/// A line of text that has been shaped, decorated, and wrapped by the text layout system.
#[derive(Clone, Default, Debug, Deref, DerefMut)]
pub struct WrappedLine {
    #[deref]
    #[deref_mut]
    pub(crate) layout: Arc<WrappedLineLayout>,
    /// The text that was shaped for this line.
    pub text: SharedString,
    pub(crate) decoration_runs: SmallVec<[DecorationRun; 32]>,
}

impl WrappedLine {
    /// The length of the underlying, unwrapped layout, in utf-8 bytes.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.layout.len()
    }

    /// Recompute soft-wrap boundaries after glyph positions have changed.
    pub fn rewrap(&mut self, wrap_width: Pixels) {
        let unwrapped_layout = self.layout.unwrapped_layout.clone();
        let wrap_boundaries =
            unwrapped_layout.compute_wrap_boundaries(&self.text, wrap_width, None);
        self.layout = Arc::new(WrappedLineLayout {
            unwrapped_layout,
            wrap_boundaries,
            wrap_width: Some(wrap_width),
            row_heights: Vec::new(),
        });
    }

    /// Set the heights of individual soft-wrapped rows without changing cached shaping.
    /// Empty metadata uses the paint/layout fallback height. Invalid metadata is cleared.
    /// Rewrapping clears these heights, so callers must set them after computing wraps.
    pub fn set_row_heights(&mut self, row_heights: Vec<Pixels>) {
        let layout = Arc::make_mut(&mut self.layout);
        layout.row_heights = row_heights;
        if layout.valid_row_heights().len() != layout.row_heights.len() {
            layout.row_heights.clear();
        }
    }

    /// Paint this line of text to the window.
    pub fn paint(
        &self,
        origin: Point<Pixels>,
        line_height: Pixels,
        align: TextAlign,
        bounds: Option<Bounds<Pixels>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<()> {
        let align_width = match bounds {
            Some(bounds) => Some(bounds.size.width),
            None => self.layout.wrap_width,
        };

        paint_line(
            origin,
            &self.layout.unwrapped_layout,
            line_height,
            align,
            align_width,
            &self.decoration_runs,
            &self.wrap_boundaries,
            self.layout.valid_row_heights(),
            window,
            cx,
        )?;

        Ok(())
    }

    /// Paint the background of line of text to the window.
    pub fn paint_background(
        &self,
        origin: Point<Pixels>,
        line_height: Pixels,
        align: TextAlign,
        bounds: Option<Bounds<Pixels>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<()> {
        let align_width = match bounds {
            Some(bounds) => Some(bounds.size.width),
            None => self.layout.wrap_width,
        };

        paint_line_background(
            origin,
            &self.layout.unwrapped_layout,
            line_height,
            align,
            align_width,
            &self.decoration_runs,
            &self.wrap_boundaries,
            self.layout.valid_row_heights(),
            window,
            cx,
        )?;

        Ok(())
    }
}

/// Vertical geometry shared by glyphs and decorations in a soft-wrapped row.
struct PaintRow {
    top: Pixels,
    height: Pixels,
    baseline_offset: Point<Pixels>,
    strikethrough_offset: Pixels,
}

impl PaintRow {
    fn new(top: Pixels, height: Pixels, fallback: Pixels, layout: &LineLayout) -> Self {
        let padding_top = (height - layout.ascent - layout.descent) / 2.;
        let fallback_baseline = (fallback - layout.ascent - layout.descent) / 2. + layout.ascent;
        Self {
            top,
            height,
            baseline_offset: point(Pixels::ZERO, padding_top + layout.ascent),
            // Preserve uniform placement, then move with the centered glyph baseline.
            strikethrough_offset: ((layout.ascent * 0.5 + fallback_baseline) * 0.5)
                + (height - fallback) / 2.,
        }
    }

    fn underline_y(&self, layout: &LineLayout) -> Pixels {
        self.top + self.baseline_offset.y + layout.descent * 0.618
    }

    fn strikethrough_y(&self) -> Pixels {
        self.top + self.strikethrough_offset
    }

    fn glyph_bounds(
        &self,
        origin: Point<Pixels>,
        max_glyph_size: crate::Size<Pixels>,
        fallback: Pixels,
    ) -> Bounds<Pixels> {
        let baseline_shift = (self.height - fallback) / 2.;
        // Keep the uniform culling box and include its baseline-shifted counterpart.
        Bounds::new(
            origin + point(Pixels::ZERO, baseline_shift.min(Pixels::ZERO)),
            size(
                max_glyph_size.width,
                max_glyph_size.height + baseline_shift.abs(),
            ),
        )
    }
}

fn paint_line(
    origin: Point<Pixels>,
    layout: &LineLayout,
    line_height: Pixels,
    align: TextAlign,
    align_width: Option<Pixels>,
    decoration_runs: &[DecorationRun],
    wrap_boundaries: &[WrapBoundary],
    row_heights: &[Pixels],
    window: &mut Window,
    cx: &mut App,
) -> Result<()> {
    let line_bounds = Bounds::new(
        origin,
        size(
            layout.width,
            if row_heights.is_empty() {
                line_height * (wrap_boundaries.len() as f32 + 1.)
            } else {
                row_heights.iter().copied().fold(Pixels::ZERO, |a, b| a + b)
            },
        ),
    );
    window.paint_layer(line_bounds, |window| {
        let mut row_ix = 0;
        let mut row = PaintRow::new(
            origin.y,
            row_heights.first().copied().unwrap_or(line_height),
            line_height,
            layout,
        );
        let mut decoration_runs = decoration_runs.iter();
        let mut wraps = wrap_boundaries.iter().peekable();
        let mut run_end = 0;
        let mut color = black();
        let mut current_underline: Option<(Point<Pixels>, UnderlineStyle)> = None;
        let mut current_strikethrough: Option<(Point<Pixels>, StrikethroughStyle)> = None;
        let text_system = cx.text_system().clone();
        let mut glyph_origin = point(
            aligned_origin_x(
                origin,
                align_width.unwrap_or(layout.width),
                px(0.0),
                &align,
                layout,
                wraps.peek(),
            ),
            origin.y,
        );
        let mut prev_glyph_position = Point::default();
        let mut max_glyph_size = size(px(0.), px(0.));

        for (run_ix, run) in layout.runs.iter().enumerate() {
            max_glyph_size = text_system.bounding_box(run.font_id, layout.font_size).size;

            for (glyph_ix, glyph) in run.glyphs.iter().enumerate() {
                glyph_origin.x += glyph.position.x - prev_glyph_position.x;

                if wraps.peek() == Some(&&WrapBoundary { run_ix, glyph_ix }) {
                    wraps.next();
                    if let Some((underline_origin, underline_style)) = current_underline.as_mut() {
                        if glyph_origin.x == underline_origin.x {
                            underline_origin.x -= max_glyph_size.width.half();
                        };
                        window.paint_underline(
                            *underline_origin,
                            glyph_origin.x - underline_origin.x,
                            underline_style,
                        );
                    }
                    if let Some((strikethrough_origin, strikethrough_style)) =
                        current_strikethrough.as_mut()
                    {
                        if glyph_origin.x == strikethrough_origin.x {
                            strikethrough_origin.x -= max_glyph_size.width.half();
                        };
                        window.paint_strikethrough(
                            *strikethrough_origin,
                            glyph_origin.x - strikethrough_origin.x,
                            strikethrough_style,
                        );
                    }

                    glyph_origin.x = aligned_origin_x(
                        origin,
                        align_width.unwrap_or(layout.width),
                        glyph.position.x,
                        &align,
                        layout,
                        wraps.peek(),
                    );
                    row_ix += 1;
                    row = PaintRow::new(
                        row.top + row.height,
                        row_heights.get(row_ix).copied().unwrap_or(line_height),
                        line_height,
                        layout,
                    );
                    glyph_origin.y = row.top;
                    // A style ending at this wrap has already been painted on the old row.
                    if glyph.index >= run_end {
                        current_underline = None;
                        current_strikethrough = None;
                    }
                    if let Some((underline_origin, _)) = current_underline.as_mut() {
                        underline_origin.x = glyph_origin.x;
                        underline_origin.y = row.underline_y(layout);
                    }
                    if let Some((strikethrough_origin, _)) = current_strikethrough.as_mut() {
                        strikethrough_origin.x = glyph_origin.x;
                        strikethrough_origin.y = row.strikethrough_y();
                    }
                }
                prev_glyph_position = glyph.position;

                let mut finished_underline: Option<(Point<Pixels>, UnderlineStyle)> = None;
                let mut finished_strikethrough: Option<(Point<Pixels>, StrikethroughStyle)> = None;
                if glyph.index >= run_end {
                    let mut style_run = decoration_runs.next();

                    // ignore style runs that apply to a partial glyph
                    while let Some(run) = style_run {
                        if glyph.index < run_end + (run.len as usize) {
                            break;
                        }
                        run_end += run.len as usize;
                        style_run = decoration_runs.next();
                    }

                    if let Some(style_run) = style_run {
                        if let Some((_, underline_style)) = &mut current_underline
                            && style_run.underline.as_ref() != Some(underline_style)
                        {
                            finished_underline = current_underline.take();
                        }
                        if let Some(run_underline) = style_run.underline.as_ref() {
                            current_underline.get_or_insert((
                                point(glyph_origin.x, row.underline_y(layout)),
                                UnderlineStyle {
                                    color: Some(run_underline.color.unwrap_or(style_run.color)),
                                    thickness: run_underline.thickness,
                                    wavy: run_underline.wavy,
                                },
                            ));
                        }
                        if let Some((_, strikethrough_style)) = &mut current_strikethrough
                            && style_run.strikethrough.as_ref() != Some(strikethrough_style)
                        {
                            finished_strikethrough = current_strikethrough.take();
                        }
                        if let Some(run_strikethrough) = style_run.strikethrough.as_ref() {
                            current_strikethrough.get_or_insert((
                                point(glyph_origin.x, row.strikethrough_y()),
                                StrikethroughStyle {
                                    color: Some(run_strikethrough.color.unwrap_or(style_run.color)),
                                    thickness: run_strikethrough.thickness,
                                },
                            ));
                        }

                        run_end += style_run.len as usize;
                        color = style_run.color;
                    } else {
                        run_end = layout.len;
                        finished_underline = current_underline.take();
                        finished_strikethrough = current_strikethrough.take();
                    }
                }

                if let Some((mut underline_origin, underline_style)) = finished_underline {
                    if underline_origin.x == glyph_origin.x {
                        underline_origin.x -= max_glyph_size.width.half();
                    };
                    window.paint_underline(
                        underline_origin,
                        glyph_origin.x - underline_origin.x,
                        &underline_style,
                    );
                }

                if let Some((mut strikethrough_origin, strikethrough_style)) =
                    finished_strikethrough
                {
                    if strikethrough_origin.x == glyph_origin.x {
                        strikethrough_origin.x -= max_glyph_size.width.half();
                    };
                    window.paint_strikethrough(
                        strikethrough_origin,
                        glyph_origin.x - strikethrough_origin.x,
                        &strikethrough_style,
                    );
                }

                let max_glyph_bounds = row.glyph_bounds(glyph_origin, max_glyph_size, line_height);

                let content_mask = window.content_mask();
                if max_glyph_bounds.intersects(&content_mask.bounds) {
                    if glyph.is_emoji {
                        window.paint_emoji(
                            glyph_origin + row.baseline_offset,
                            run.font_id,
                            glyph.id,
                            layout.font_size,
                        )?;
                    } else {
                        window.paint_glyph(
                            glyph_origin + row.baseline_offset,
                            run.font_id,
                            glyph.id,
                            layout.font_size,
                            color,
                        )?;
                    }
                }
            }
        }

        let last_line_end_x = glyph_origin.x + layout.width - prev_glyph_position.x;

        if let Some((mut underline_start, underline_style)) = current_underline.take() {
            if last_line_end_x == underline_start.x {
                underline_start.x -= max_glyph_size.width.half()
            };
            window.paint_underline(
                underline_start,
                last_line_end_x - underline_start.x,
                &underline_style,
            );
        }

        if let Some((mut strikethrough_start, strikethrough_style)) = current_strikethrough.take() {
            if last_line_end_x == strikethrough_start.x {
                strikethrough_start.x -= max_glyph_size.width.half()
            };
            window.paint_strikethrough(
                strikethrough_start,
                last_line_end_x - strikethrough_start.x,
                &strikethrough_style,
            );
        }

        Ok(())
    })
}

fn paint_line_background(
    origin: Point<Pixels>,
    layout: &LineLayout,
    line_height: Pixels,
    align: TextAlign,
    align_width: Option<Pixels>,
    decoration_runs: &[DecorationRun],
    wrap_boundaries: &[WrapBoundary],
    row_heights: &[Pixels],
    window: &mut Window,
    cx: &mut App,
) -> Result<()> {
    let line_bounds = Bounds::new(
        origin,
        size(
            layout.width,
            if row_heights.is_empty() {
                line_height * (wrap_boundaries.len() as f32 + 1.)
            } else {
                row_heights.iter().copied().fold(Pixels::ZERO, |a, b| a + b)
            },
        ),
    );
    window.paint_layer(line_bounds, |window| {
        let mut decoration_runs = decoration_runs.iter();
        let mut wraps = wrap_boundaries.iter().peekable();
        let mut run_end = 0;
        let mut row_ix = 0;
        let mut row = PaintRow::new(
            origin.y,
            row_heights.first().copied().unwrap_or(line_height),
            line_height,
            layout,
        );
        let mut current_background: Option<(Point<Pixels>, Hsla)> = None;
        let text_system = cx.text_system().clone();
        let mut glyph_origin = point(
            aligned_origin_x(
                origin,
                align_width.unwrap_or(layout.width),
                px(0.0),
                &align,
                layout,
                wraps.peek(),
            ),
            origin.y,
        );
        let mut prev_glyph_position = Point::default();
        let mut max_glyph_size = size(px(0.), px(0.));
        for (run_ix, run) in layout.runs.iter().enumerate() {
            max_glyph_size = text_system.bounding_box(run.font_id, layout.font_size).size;

            for (glyph_ix, glyph) in run.glyphs.iter().enumerate() {
                glyph_origin.x += glyph.position.x - prev_glyph_position.x;

                if wraps.peek() == Some(&&WrapBoundary { run_ix, glyph_ix }) {
                    wraps.next();
                    if let Some((background_origin, background_color)) = current_background.as_mut()
                    {
                        if glyph_origin.x == background_origin.x {
                            background_origin.x -= max_glyph_size.width.half()
                        }
                        window.paint_quad(fill(
                            Bounds {
                                origin: *background_origin,
                                size: size(glyph_origin.x - background_origin.x, row.height),
                            },
                            *background_color,
                        ));
                    }

                    glyph_origin.x = aligned_origin_x(
                        origin,
                        align_width.unwrap_or(layout.width),
                        glyph.position.x,
                        &align,
                        layout,
                        wraps.peek(),
                    );
                    row_ix += 1;
                    row = PaintRow::new(
                        row.top + row.height,
                        row_heights.get(row_ix).copied().unwrap_or(line_height),
                        line_height,
                        layout,
                    );
                    glyph_origin.y = row.top;
                    if glyph.index >= run_end {
                        current_background = None;
                    }
                    if let Some((background_origin, _)) = current_background.as_mut() {
                        background_origin.x = glyph_origin.x;
                        background_origin.y = row.top;
                    }
                }
                prev_glyph_position = glyph.position;

                let mut finished_background: Option<(Point<Pixels>, Hsla)> = None;
                if glyph.index >= run_end {
                    let mut style_run = decoration_runs.next();

                    // ignore style runs that apply to a partial glyph
                    while let Some(run) = style_run {
                        if glyph.index < run_end + (run.len as usize) {
                            break;
                        }
                        run_end += run.len as usize;
                        style_run = decoration_runs.next();
                    }

                    if let Some(style_run) = style_run {
                        if let Some((_, background_color)) = &mut current_background
                            && style_run.background_color.as_ref() != Some(background_color)
                        {
                            finished_background = current_background.take();
                        }
                        if let Some(run_background) = style_run.background_color {
                            current_background.get_or_insert((
                                point(glyph_origin.x, glyph_origin.y),
                                run_background,
                            ));
                        }
                        run_end += style_run.len as usize;
                    } else {
                        run_end = layout.len;
                        finished_background = current_background.take();
                    }
                }

                if let Some((mut background_origin, background_color)) = finished_background {
                    if background_origin.x == glyph_origin.x {
                        background_origin.x -= max_glyph_size.width.half();
                    };
                    let width = glyph_origin.x - background_origin.x;
                    window.paint_quad(fill(
                        Bounds {
                            origin: background_origin,
                            size: size(width, row.height),
                        },
                        background_color,
                    ));
                }
            }
        }

        let last_line_end_x = glyph_origin.x + layout.width - prev_glyph_position.x;

        if let Some((mut background_origin, background_color)) = current_background.take() {
            if last_line_end_x == background_origin.x {
                background_origin.x -= max_glyph_size.width.half()
            };
            window.paint_quad(fill(
                Bounds {
                    origin: background_origin,
                    size: size(last_line_end_x - background_origin.x, row.height),
                },
                background_color,
            ));
        }

        Ok(())
    })
}

fn aligned_origin_x(
    origin: Point<Pixels>,
    align_width: Pixels,
    last_glyph_x: Pixels,
    align: &TextAlign,
    layout: &LineLayout,
    wrap_boundary: Option<&&WrapBoundary>,
) -> Pixels {
    let end_of_line = if let Some(WrapBoundary { run_ix, glyph_ix }) = wrap_boundary {
        layout.runs[*run_ix].glyphs[*glyph_ix].position.x
    } else {
        layout.width
    };

    let line_width = end_of_line - last_glyph_x;

    match align {
        TextAlign::Left => origin.x,
        TextAlign::Center => (origin.x * 2.0 + align_width - line_width) / 2.0,
        TextAlign::Right => origin.x + align_width - line_width,
    }
}

#[cfg(test)]
mod tests {
    use super::super::line_layout::{LineLayoutCache, test_wrapped_layout};
    use super::*;

    fn wrapped_line() -> WrappedLine {
        WrappedLine {
            layout: Arc::new(test_wrapped_layout(Vec::new())),
            text: "abcdef".into(),
            decoration_runs: SmallVec::new(),
        }
    }

    fn decorated_line(len: u32) -> WrappedLine {
        let mut line = wrapped_line();
        line.decoration_runs.push(DecorationRun {
            len,
            color: black(),
            background_color: Some(black()),
            underline: Some(UnderlineStyle {
                color: Some(black()),
                thickness: px(1.),
                wavy: false,
            }),
            strikethrough: Some(StrikethroughStyle {
                color: Some(black()),
                thickness: px(1.),
            }),
        });
        line
    }

    fn painted_decorations(
        line: &WrappedLine,
        align: TextAlign,
    ) -> (Vec<Bounds<Pixels>>, Vec<Bounds<Pixels>>) {
        let mut cx = crate::TestAppContext::single();
        cx.add_empty_window().update(|window, cx| {
            window.invalidator.set_phase(crate::DrawPhase::Paint);
            let quad_start = window.next_frame.scene.quads.len();
            let underline_start = window.next_frame.scene.underlines.len();
            let origin = point(px(100.), px(100.));
            let bounds = Some(Bounds::new(origin, size(px(80.), px(200.))));
            line.paint_background(origin, px(24.), align, bounds, window, cx)
                .unwrap();
            line.paint(origin, px(24.), align, bounds, window, cx)
                .unwrap();
            let scale = window.scale_factor();
            let backgrounds = window.next_frame.scene.quads[quad_start..]
                .iter()
                .map(|quad| quad.bounds.map(|value| px(value.0 / scale)))
                .collect();
            let underlines = window.next_frame.scene.underlines[underline_start..]
                .iter()
                .map(|underline| underline.bounds.map(|value| px(value.0 / scale)))
                .collect();
            window.invalidator.set_phase(crate::DrawPhase::None);
            (backgrounds, underlines)
        })
    }

    #[test]
    fn wrapped_decorations_follow_each_rows_height_and_alignment() {
        for heights in [
            vec![],
            vec![px(24.), px(60.), px(24.)],
            vec![px(60.), px(24.), px(24.)],
        ] {
            for align in [TextAlign::Left, TextAlign::Center, TextAlign::Right] {
                let mut line = decorated_line(6);
                Arc::make_mut(&mut line.layout).wrap_boundaries[1].glyph_ix = 5;
                line.set_row_heights(heights.clone());
                let (backgrounds, underlines) = painted_decorations(&line, align);
                assert_eq!(backgrounds.len(), 3);
                assert_eq!(underlines.len(), 6);
                for (row_ix, width) in [20., 30., 10.].into_iter().enumerate() {
                    let x = match align {
                        TextAlign::Left => 100.,
                        TextAlign::Center => 100. + (80. - width) / 2.,
                        TextAlign::Right => 100. + 80. - width,
                    };
                    let top = px(100.) + line.row_top(row_ix, px(24.));
                    let height = line.row_height(row_ix, px(24.));
                    let row = PaintRow::new(top, height, px(24.), &line.unwrapped_layout);
                    assert_eq!(
                        backgrounds[row_ix],
                        Bounds::new(point(px(x), top), size(px(width), height))
                    );
                    for (decoration_ix, y) in [
                        row.underline_y(&line.unwrapped_layout),
                        row.strikethrough_y(),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        assert_eq!(
                            underlines[row_ix * 2 + decoration_ix],
                            Bounds::new(point(px(x), y), size(px(width), px(1.)))
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn decorations_ending_at_a_wrap_do_not_paint_on_the_next_row() {
        let mut line = decorated_line(2);
        line.set_row_heights(vec![px(24.), px(60.), px(24.)]);
        let (backgrounds, underlines) = painted_decorations(&line, TextAlign::Left);
        assert_eq!(backgrounds.len(), 1);
        assert_eq!(
            backgrounds[0],
            Bounds::new(point(px(100.), px(100.)), size(px(20.), px(24.)))
        );
        assert_eq!(underlines.len(), 2);
        for underline in underlines {
            assert_eq!(underline.size.width, px(20.));
            assert!(underline.origin.y < px(124.));
        }
    }

    #[test]
    fn zero_advance_background_runs_retain_the_minimum_painted_width() {
        let mut line = decorated_line(1);
        let mut layout = LineLayout {
            runs: line.unwrapped_layout.runs.clone(),
            font_size: line.font_size(),
            width: line.unwrapped_layout.width,
            ascent: line.ascent(),
            descent: line.descent(),
            len: line.len(),
        };
        layout.runs[0].glyphs[1].position.x = Pixels::ZERO;
        Arc::make_mut(&mut line.layout).unwrapped_layout = Arc::new(layout);
        line.set_row_heights(vec![px(24.), px(60.), px(24.)]);
        let (backgrounds, _) = painted_decorations(&line, TextAlign::Left);
        assert_eq!(backgrounds.len(), 1);
        assert_eq!(backgrounds[0].origin.y, px(100.));
        assert_eq!(backgrounds[0].size.height, px(24.));
        assert!(backgrounds[0].size.width > Pixels::ZERO);
        assert_eq!(backgrounds[0].right(), px(100.));
    }

    #[test]
    fn set_row_heights_does_not_mutate_shared_shaping() {
        let mut line = wrapped_line();
        let cached_layout = line.layout.clone();
        let original = line.clone();
        line.set_row_heights(vec![px(24.), px(60.), px(24.)]);
        assert!(!Arc::ptr_eq(&line.layout, &cached_layout));
        assert!(Arc::ptr_eq(
            &line.unwrapped_layout,
            &cached_layout.unwrapped_layout
        ));
        assert!(cached_layout.row_heights.is_empty());
        assert!(original.row_heights.is_empty());
        assert_eq!(original.size(px(24.)).height, px(72.));
        assert_eq!(line.size(px(24.)).height, px(108.));

        let clone = line.clone();
        line.set_row_heights(vec![px(60.), px(24.), px(24.)]);
        assert_eq!(clone.row_heights, vec![px(24.), px(60.), px(24.)]);
        assert_eq!(line.row_heights, vec![px(60.), px(24.), px(24.)]);
    }

    #[test]
    fn row_heights_do_not_leak_into_current_or_previous_frame_caches() {
        let cache = LineLayoutCache::new(Arc::new(crate::NoopTextSystem::new()));
        let cached = cache.layout_wrapped_line("abcdef", px(18.), &[], Some(px(24.)), None);
        assert_eq!(cached.wrap_boundaries.len(), 2);
        let mut line = WrappedLine {
            layout: cached.clone(),
            text: "abcdef".into(),
            decoration_runs: SmallVec::new(),
        };
        line.set_row_heights(vec![px(24.), px(60.), px(24.)]);
        for _ in 0..2 {
            let reused = cache.layout_wrapped_line("abcdef", px(18.), &[], Some(px(24.)), None);
            assert!(Arc::ptr_eq(&cached, &reused));
            assert!(reused.row_heights.is_empty());
            assert!(Arc::ptr_eq(
                &line.unwrapped_layout,
                &reused.unwrapped_layout
            ));
            assert_eq!(reused.size(px(24.)).height, px(72.));
            assert_eq!(line.size(px(24.)).height, px(108.));
            cache.finish_frame();
        }
    }

    #[test]
    fn set_row_heights_clears_invalid_metadata_without_panicking() {
        let mut line = wrapped_line();
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
            line.set_row_heights(vec![px(24.), px(60.), px(24.)]);
            line.set_row_heights(heights);
            assert!(line.row_heights.is_empty());
            assert_eq!(line.size(px(24.)).height, px(72.));
        }

        let mut unwrapped = WrappedLine::default();
        unwrapped.set_row_heights(vec![px(60.)]);
        assert_eq!(unwrapped.size(px(24.)).height, px(60.));
    }

    #[test]
    fn rewrap_clears_old_row_metrics_even_when_row_count_is_unchanged() {
        let mut line = wrapped_line();
        line.set_row_heights(vec![px(24.), px(60.), px(24.)]);
        let original = line.clone();
        line.rewrap(px(20.));
        assert_eq!(line.wrap_boundaries.len(), 2);
        assert!(line.row_heights.is_empty());
        assert_eq!(line.size(px(24.)).height, px(72.));
        assert_eq!(original.row_heights, vec![px(24.), px(60.), px(24.)]);

        line.set_row_heights(vec![px(24.), px(60.), px(24.)]);
        line.rewrap(px(60.));
        assert!(line.wrap_boundaries.is_empty());
        assert!(line.row_heights.is_empty());
        assert_eq!(line.size(px(24.)).height, px(24.));
    }

    #[test]
    fn painting_rows_center_baselines_and_follow_them_with_decorations() {
        let layout = test_wrapped_layout(Vec::new()).unwrapped_layout;
        for (heights, baselines, strikes) in [
            ([24., 60., 24.], [18., 60., 102.], [13.5, 55.5, 97.5]),
            ([60., 24., 24.], [36., 78., 102.], [31.5, 73.5, 97.5]),
        ] {
            let mut top = px(100.);
            for (row_ix, height) in heights.into_iter().enumerate() {
                let row = PaintRow::new(top, px(height), px(24.), &layout);
                assert_eq!(row.height, px(height));
                assert_eq!(row.top, top);
                assert_eq!(
                    row.top + row.baseline_offset.y,
                    px(100. + baselines[row_ix])
                );
                assert_eq!(row.strikethrough_y(), px(100. + strikes[row_ix]));
                let expected_underline = px(100. + baselines[row_ix]) + layout.descent * 0.618;
                assert!((row.underline_y(&layout).0 - expected_underline.0).abs() < 0.0001);
                top += row.height;
            }
            assert_eq!(top, px(208.));
        }
    }

    #[test]
    fn glyph_culling_includes_the_shifted_font_box_for_tall_and_short_rows() {
        let layout = test_wrapped_layout(Vec::new()).unwrapped_layout;
        let origin = point(px(100.), px(100.));
        let font_size = size(px(30.), px(100.));
        for height in [12., 24., 60.] {
            let row = PaintRow::new(origin.y, px(height), px(24.), &layout);
            let bounds = row.glyph_bounds(origin, font_size, px(24.));
            let shifted_top = origin.y + (px(height) - px(24.)) / 2.;
            assert!(bounds.top() <= shifted_top);
            assert!(bounds.bottom() >= shifted_top + font_size.height);
            assert_eq!(bounds.origin.x, origin.x);
            assert_eq!(bounds.size.width, font_size.width);
            if height == 24. {
                assert_eq!(bounds, Bounds::new(origin, font_size));
            }
        }
    }

    #[test]
    fn uniform_paint_geometry_is_unchanged() {
        let layout = test_wrapped_layout(Vec::new()).unwrapped_layout;
        for height in [24., 30., 60.] {
            let height = px(height);
            let old_baseline = (height - layout.ascent - layout.descent) / 2. + layout.ascent;
            for row_ix in 0..3 {
                let top = px(100.) + height * row_ix;
                let row = PaintRow::new(top, height, height, &layout);
                assert_eq!(row.baseline_offset, point(Pixels::ZERO, old_baseline));
                assert_eq!(
                    row.underline_y(&layout),
                    top + old_baseline + layout.descent * 0.618
                );
                assert_eq!(
                    row.strikethrough_y(),
                    top + ((layout.ascent * 0.5 + old_baseline) * 0.5)
                );
            }
        }
    }
}
