use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Context, Entity, Hsla, PaintQuad, Pixels, Render, Subscription, Window, canvas, fill,
    prelude::*,
};
use unicode_segmentation::UnicodeSegmentation;

use super::{EditorView, PaneId, TableBoundaryCaret, element};

#[derive(Clone)]
pub(crate) struct CaretPaint {
    pub(crate) bounds: gpui::Bounds<gpui::Pixels>,
    pub(crate) color: Hsla,
    pub(crate) force_visible: bool,
}

pub(crate) fn centered_caret_left(boundary_x: Pixels, width: Pixels) -> Pixels {
    boundary_x - width / 2.0
}

pub(crate) fn text_caret_x(
    text: &str,
    offset: usize,
    boundary: gpui::Point<Pixels>,
    layout: &gpui::LineLayout,
    text_system: &gpui::TextSystem,
    mut position_for_index: impl FnMut(usize) -> Option<gpui::Point<Pixels>>,
) -> Pixels {
    let (Some(before), Some(after)) = (text.get(..offset), text.get(offset..)) else {
        return boundary.x;
    };
    let previous = before.grapheme_indices(true).next_back();
    let next = after.graphemes(true).next();
    if let (Some((start, _)), Some(next)) = (previous, next) {
        let end = offset + next.len();
        if let (Some(left), Some(right)) = (position_for_index(start), position_for_index(end))
            && left.y == boundary.y
            && right.y == boundary.y
        {
            let ink = |range: std::ops::Range<usize>| -> Option<(Pixels, Pixels)> {
                let mut edges: Option<(Pixels, Pixels)> = None;
                for run in &layout.runs {
                    for glyph in &run.glyphs {
                        if !range.contains(&glyph.index) || glyph.is_emoji {
                            continue;
                        }
                        let character = text[glyph.index..].chars().next()?;
                        if character.is_whitespace() {
                            continue;
                        }
                        let bounds = text_system
                            .typographic_bounds(run.font_id, layout.font_size, character)
                            .ok()?;
                        if bounds.size.width <= gpui::px(0.0) {
                            continue;
                        }
                        let left = glyph.position.x + bounds.left();
                        let right = glyph.position.x + bounds.right();
                        edges = Some(edges.map_or((left, right), |(old_left, old_right)| {
                            (old_left.min(left), old_right.max(right))
                        }));
                    }
                }
                edges
            };
            if let (Some((_, left_end)), Some((right_start, _))) =
                (ink(start..offset), ink(offset..end))
            {
                return boundary.x + (left_end + right_start) / 2.0 - layout.x_for_index(offset);
            }
        }
    }
    caret_x_between_grapheme_centers(text, offset, boundary, position_for_index)
}

pub(crate) fn caret_x_between_grapheme_centers(
    text: &str,
    offset: usize,
    boundary: gpui::Point<Pixels>,
    mut position_for_index: impl FnMut(usize) -> Option<gpui::Point<Pixels>>,
) -> Pixels {
    let mut previous_start = None;
    let mut next_end = None;
    for (start, grapheme) in text.grapheme_indices(true) {
        let end = start + grapheme.len();
        if end == offset {
            previous_start = Some(start);
        }
        if start == offset {
            next_end = Some(end);
            break;
        }
    }

    let (Some(previous_start), Some(next_end)) = (previous_start, next_end) else {
        return boundary.x;
    };
    let (Some(previous), Some(next)) = (
        position_for_index(previous_start),
        position_for_index(next_end),
    ) else {
        return boundary.x;
    };
    if previous.y != boundary.y || next.y != boundary.y {
        return boundary.x;
    }

    // Center between the neighboring grapheme advance-cell centers, not just
    // at the next glyph's origin.
    (previous.x + boundary.x * 2.0 + next.x) / 4.0
}

#[derive(Clone, Default)]
pub(crate) struct CaretGeometrySnapshot {
    pub(crate) cursor_offset: usize,
    pub(crate) scroll_offset: f32,
    pub(crate) horizontal_offset: f32,
    pub(crate) table_boundary_caret: Option<TableBoundaryCaret>,
    pub(crate) paints: Vec<CaretPaint>,
}

impl CaretGeometrySnapshot {
    fn matches(
        &self,
        cursor_offset: usize,
        scroll_offset: f32,
        horizontal_offset: f32,
        table_boundary_caret: Option<TableBoundaryCaret>,
    ) -> bool {
        self.cursor_offset == cursor_offset
            && self.table_boundary_caret == table_boundary_caret
            && (self.scroll_offset - scroll_offset).abs() <= 0.5
            && (self.horizontal_offset - horizontal_offset).abs() <= 0.5
    }
}

pub(crate) type SharedCaretGeometry = Rc<RefCell<CaretGeometrySnapshot>>;

// Four smoothstep samples over 140ms keep the fade visibly continuous while
// avoiding a display-rate present loop on older integrated GPUs.
const CARET_FADE_FRAMES: usize = 4;
const CARET_FADE_FRAME_INTERVAL: Duration = Duration::from_millis(35);

fn caret_fade_opacity(from: f32, to: f32, frame: usize, frame_count: usize) -> f32 {
    let progress = frame.min(frame_count) as f32 / frame_count.max(1) as f32;
    let eased = progress * progress * (3.0 - 2.0 * progress);
    from + (to - from) * eased
}

#[derive(Clone, Copy)]
pub(crate) struct CaretAnimationConfig {
    pub(crate) pause: Duration,
    pub(crate) visible: Duration,
    pub(crate) transparent: Duration,
    pub(crate) min_opacity: f32,
    pub(crate) max_opacity: f32,
}

pub(crate) struct EditorContentView {
    editor: Entity<EditorView>,
    pane_id: PaneId,
    caret_geometry: SharedCaretGeometry,
    _editor_observer: Subscription,
}

impl EditorContentView {
    pub(crate) fn new(
        editor: Entity<EditorView>,
        pane_id: PaneId,
        caret_geometry: SharedCaretGeometry,
        cx: &mut Context<Self>,
    ) -> Self {
        let observer = cx.observe(&editor, |_, _, cx| cx.notify());
        Self {
            editor,
            pane_id,
            caret_geometry,
            _editor_observer: observer,
        }
    }
}

impl Render for EditorContentView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        element::EditorElement {
            editor: self.editor.clone(),
            pane_id: self.pane_id,
            caret_geometry: self.caret_geometry.clone(),
        }
    }
}

pub(crate) struct CaretOverlayView {
    editor: Entity<EditorView>,
    geometry: SharedCaretGeometry,
    config: CaretAnimationConfig,
    opacity: f32,
    active: bool,
    editor_blink_epoch: usize,
    animation_epoch: usize,
}

impl CaretOverlayView {
    pub(crate) fn new(
        editor: Entity<EditorView>,
        geometry: SharedCaretGeometry,
        config: CaretAnimationConfig,
    ) -> Self {
        Self {
            editor,
            geometry,
            opacity: config.max_opacity,
            active: false,
            editor_blink_epoch: 0,
            animation_epoch: 0,
            config,
        }
    }

    pub(crate) fn sync(
        &mut self,
        active: bool,
        editor_blink_epoch: usize,
        config: CaretAnimationConfig,
        cx: &mut Context<Self>,
    ) {
        self.config = config;
        let was_active = self.active;
        let became_active = active && !was_active;
        let reset = active && self.editor_blink_epoch != editor_blink_epoch;
        self.active = active;
        self.editor_blink_epoch = editor_blink_epoch;

        if !active {
            if was_active || self.opacity != config.max_opacity {
                self.animation_epoch = self.animation_epoch.wrapping_add(1);
                self.opacity = config.max_opacity;
                cx.notify();
            }
            return;
        }

        if became_active || reset {
            self.animation_epoch = self.animation_epoch.wrapping_add(1);
            self.opacity = config.max_opacity;
            let delay = if reset { config.pause } else { config.visible };
            self.schedule_transition(
                self.animation_epoch,
                delay,
                config.max_opacity,
                config.min_opacity,
                cx,
            );
            cx.notify();
        }
    }

    fn schedule_transition(
        &self,
        epoch: usize,
        hold: Duration,
        from: f32,
        to: f32,
        cx: &mut Context<Self>,
    ) {
        let config = self.config;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(hold).await;
            for frame in 1..=CARET_FADE_FRAMES {
                cx.background_executor()
                    .timer(CARET_FADE_FRAME_INTERVAL)
                    .await;
                let keep_animating = this
                    .update(cx, |this, cx| {
                        if !this.active || this.animation_epoch != epoch {
                            return false;
                        }

                        this.opacity = caret_fade_opacity(from, to, frame, CARET_FADE_FRAMES);
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep_animating {
                    return;
                }
            }

            this.update(cx, |this, cx| {
                if !this.active || this.animation_epoch != epoch {
                    return;
                }

                let (next_hold, next_target) = if to <= config.min_opacity {
                    (config.transparent, config.max_opacity)
                } else {
                    (config.visible, config.min_opacity)
                };
                this.schedule_transition(epoch, next_hold, to, next_target, cx);
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use gpui::px;

    use super::{
        CARET_FADE_FRAMES, CaretGeometrySnapshot, caret_fade_opacity,
        caret_x_between_grapheme_centers, centered_caret_left,
    };
    use crate::editor::{TableBoundaryCaret, TableBoundarySide};

    #[test]
    fn overlay_caret_fade_is_smooth_monotonic_and_reaches_the_endpoint() {
        let samples = (0..=CARET_FADE_FRAMES)
            .map(|frame| caret_fade_opacity(1.0, 0.0, frame, CARET_FADE_FRAMES))
            .collect::<Vec<_>>();
        assert_eq!(samples.first().copied(), Some(1.0));
        assert_eq!(samples.last().copied(), Some(0.0));
        assert!(samples.windows(2).all(|pair| pair[0] > pair[1]));

        assert_eq!(
            caret_fade_opacity(0.0, 1.0, CARET_FADE_FRAMES, CARET_FADE_FRAMES),
            1.0
        );
    }

    #[test]
    fn text_caret_stroke_is_centered_on_the_character_boundary() {
        let boundary = px(12.5);
        let width = px(2.0);
        assert_eq!(centered_caret_left(boundary, width) + width / 2.0, boundary);
    }

    #[test]
    fn text_caret_is_centered_between_adjacent_grapheme_centers() {
        let text = "iW";
        let x =
            caret_x_between_grapheme_centers(text, 1, gpui::point(px(16.0), px(0.0)), |index| {
                match index {
                    0 => Some(gpui::point(px(0.0), px(0.0))),
                    2 => Some(gpui::point(px(20.0), px(0.0))),
                    _ => None,
                }
            });
        assert_eq!(x, px(13.0));
    }

    #[test]
    fn text_caret_keeps_grapheme_and_wrap_boundaries_intact() {
        let text = "e\u{301}界";
        let boundary = gpui::point(px(10.0), px(24.0));
        assert_eq!(
            caret_x_between_grapheme_centers(text, 3, boundary, |index| match index {
                0 => Some(gpui::point(px(0.0), px(24.0))),
                6 => Some(gpui::point(px(18.0), px(24.0))),
                _ => None,
            }),
            px(9.5)
        );
        assert_eq!(
            caret_x_between_grapheme_centers(text, 1, boundary, |_| None),
            boundary.x
        );
        assert_eq!(
            caret_x_between_grapheme_centers(text, 3, boundary, |index| match index {
                0 => Some(gpui::point(px(0.0), px(0.0))),
                6 => Some(gpui::point(px(18.0), px(48.0))),
                _ => None,
            }),
            boundary.x
        );
    }

    #[test]
    fn overlay_rejects_geometry_from_stale_cursor_or_scroll_state() {
        let snapshot = CaretGeometrySnapshot {
            cursor_offset: 42,
            scroll_offset: 120.0,
            horizontal_offset: 24.0,
            table_boundary_caret: None,
            paints: Vec::new(),
        };
        assert!(snapshot.matches(42, 120.4, 24.4, None));
        assert!(!snapshot.matches(43, 120.0, 24.0, None));
        assert!(!snapshot.matches(42, 120.6, 24.0, None));
        assert!(!snapshot.matches(42, 120.0, 24.6, None));
        assert!(!snapshot.matches(
            42,
            120.0,
            24.0,
            Some(TableBoundaryCaret {
                table_start_line: 3,
                side: TableBoundarySide::Above,
            }),
        ));
    }
}

impl Render for CaretOverlayView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let geometry = self.geometry.clone();
        let editor = self.editor.clone();
        let opacity = self.opacity;
        canvas(
            move |_, _, cx| {
                let snapshot = geometry.borrow().clone();
                let editor = editor.read(cx);
                let cursor_line = editor.buffer.offset_to_line_col(editor.cursor.offset).0;
                let horizontal_offset = editor.horizontal_scroll_offset_for_line(cursor_line);
                if snapshot.matches(
                    editor.cursor.offset,
                    editor.scroll_offset,
                    horizontal_offset,
                    editor.table_boundary_caret,
                ) {
                    snapshot.paints
                } else {
                    Vec::new()
                }
            },
            move |_, paints, window, _| {
                for paint in paints {
                    let opacity = if paint.force_visible { 1.0 } else { opacity };
                    if opacity <= 0.0 {
                        continue;
                    }
                    let mut color = paint.color;
                    color.a *= opacity;
                    let quad: PaintQuad = fill(paint.bounds, color);
                    window.paint_quad(quad);
                }
            },
        )
        .absolute()
        .size_full()
    }
}
