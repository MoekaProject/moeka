use std::sync::{Arc, Weak};

use gpui::{
    App, Bounds, ContentMask, Corners, DecorationRun, Hsla, LineLayout, PaintQuad, Pixels, Point,
    RenderImage, ShapedLine, Window, WrapBoundary, WrappedLine, fill, point, px, size,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_crop_keeps_gutter_outside_the_projected_content_column() {
        let body = Bounds::new(point(px(60.0), px(0.0)), size(px(720.0), px(600.0)));
        let target = Bounds::new(point(px(900.0), px(0.0)), size(px(100.0), px(600.0)));
        let scale = 92.0 / 720.0;
        let project = |x| project_point(point(px(x), px(0.0)), body, 0.0, target, 4.0, scale, 0.0);
        assert_eq!(project(60.0).x, px(904.0));
        assert_eq!(project(780.0).x, px(996.0));
        assert!(project(40.0).x < project(60.0).x);
    }

    #[test]
    fn chinese_and_english_punctuation_becomes_gaps_without_changing_positions() {
        let text = "a，b,c。d!";
        let count = text.chars().count();
        let layout = LineLayout {
            width: px(count as f32 * 8.0),
            ascent: px(10.0),
            descent: px(2.0),
            ..Default::default()
        };
        let runs = [DecorationRun {
            len: text.len() as u32,
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
        for punctuation_gaps in [true, false] {
            let mut snapshot = snapshot(100);
            let glyphs = text.char_indices().enumerate().map(|(column, (index, _))| {
                (
                    WrapBoundary {
                        run_ix: 0,
                        glyph_ix: column,
                    },
                    index,
                    point(px(column as f32 * 8.0), px(0.0)),
                )
            });
            snapshot.text_glyphs(
                &layout,
                text,
                &runs,
                &[],
                &[],
                point(px(0.0), px(0.0)),
                px(20.0),
                None,
                glyphs,
                punctuation_gaps,
            );
            let boxes = snapshot
                .paints
                .iter()
                .map(|paint| {
                    let Primitive::Quad(quad) = &paint.primitive else {
                        panic!("text block")
                    };
                    (
                        f32::from(quad.bounds.left()),
                        f32::from(quad.bounds.size.width),
                    )
                })
                .collect::<Vec<_>>();
            if punctuation_gaps {
                assert_eq!(
                    boxes,
                    vec![(0.0, 8.0), (16.0, 8.0), (32.0, 8.0), (48.0, 8.0)]
                );
            } else {
                assert_eq!(
                    boxes,
                    vec![(0.0, 64.0)],
                    "code punctuation must remain visible"
                );
            }
        }
        for ch in "，。！？；：、（）《》【】“”‘’—…,.!?;:'\"()[]".chars() {
            assert!(punctuation_gap(ch), "{ch}");
        }
        for ch in "中文AZ09🙂\u{301}".chars() {
            assert!(!punctuation_gap(ch), "{ch}");
        }
    }

    #[test]
    fn generated_diagrams_and_formulas_reuse_original_preview_images() {
        for kind in [
            crate::markdown::BlockPreviewKind::Mermaid,
            crate::markdown::BlockPreviewKind::Math,
        ] {
            let mut snapshot = snapshot(100);
            let image = Arc::new(RenderImage::new(vec![image::Frame::new(
                image::RgbaImage::from_raw(
                    2,
                    2,
                    vec![0, 0, 255, 255, 0, 0, 0, 0, 255, 0, 0, 255, 0, 255, 0, 255],
                )
                .unwrap(),
            )]));
            let bounds = Bounds::new(point(px(20.0), px(40.0)), size(px(300.0), px(160.0)));
            let clip = Bounds::new(point(px(30.0), px(50.0)), size(px(200.0), px(100.0)));
            snapshot.media(bounds, &image, px(0.0), Some(clip));
            assert_eq!(Arc::strong_count(&image), 1);
            assert_eq!(Arc::weak_count(&image), 1);
            assert_eq!(snapshot.paints.len(), 1, "{kind:?}");
            let Primitive::Image(captured, weak, _) = &snapshot.paints[0].primitive else {
                panic!("preview must use its original image");
            };
            assert_eq!(*captured, bounds);
            assert!(Arc::ptr_eq(&weak.upgrade().unwrap(), &image));
            assert_eq!(snapshot.paints[0].clip, clip);
        }
    }

    #[test]
    fn code_and_line_numbers_use_colored_blocks_in_their_original_columns() {
        let mut snapshot = snapshot(100);
        let number_color = gpui::hsla(0.0, 0.0, 0.5, 1.0);
        let code_color = gpui::hsla(0.6, 0.8, 0.6, 1.0);
        for (text, origin_x, color) in [("12", 4.0, number_color), ("let", 40.0, code_color)] {
            let layout = LineLayout {
                width: px(text.len() as f32 * 8.0),
                ascent: px(10.0),
                descent: px(2.0),
                ..Default::default()
            };
            let runs = [DecorationRun {
                len: text.len() as u32,
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            }];
            let glyphs = (0..text.len()).map(|index| {
                (
                    WrapBoundary {
                        run_ix: 0,
                        glyph_ix: index,
                    },
                    index,
                    point(px(index as f32 * 8.0), px(0.0)),
                )
            });
            snapshot.text_glyphs(
                &layout,
                text,
                &runs,
                &[],
                &[],
                point(px(origin_x), px(20.0)),
                px(20.0),
                None,
                glyphs,
                false,
            );
        }
        assert_eq!(snapshot.paints.len(), 2);
        for (paint, x, width, color) in [
            (&snapshot.paints[0], 4.0, 16.0, number_color),
            (&snapshot.paints[1], 40.0, 24.0, code_color),
        ] {
            let Primitive::Quad(block) = &paint.primitive else {
                panic!("text must remain a block")
            };
            assert_eq!(block.bounds.left(), px(x));
            assert_eq!(block.bounds.size.width, px(width));
            assert_eq!(block.background, color.into());
        }
    }

    #[test]
    fn document_icons_are_solid_blocks_with_original_color_and_bounds() {
        let mut snapshot = snapshot(100);
        let bounds = Bounds::new(point(px(2.0), px(40.0)), size(px(18.0), px(18.0)));
        let color = gpui::hsla(0.3, 0.7, 0.6, 0.8);
        snapshot.icon(bounds, color);
        let Primitive::Quad(icon) = &snapshot.paints[0].primitive else {
            panic!("icon block")
        };
        assert_eq!(icon.bounds, bounds);
        assert_eq!(icon.background, color.into());
        assert_eq!(icon.border_widths, gpui::Edges::all(px(0.0)));
    }

    fn snapshot(limit: usize) -> Snapshot {
        Snapshot::new(
            Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(600.0))),
            48_000_000.0,
            gpui::white(),
            limit,
        )
    }

    #[test]
    fn text_blocks_use_resolved_run_colors_glyph_advances_and_actual_wrap_heights() {
        let mut snapshot = snapshot(100);
        let layout = LineLayout {
            ascent: px(12.0),
            descent: px(4.0),
            width: px(50.0),
            len: 7,
            ..Default::default()
        };
        let red = gpui::hsla(0.0, 0.9, 0.4, 1.0);
        let blue = gpui::hsla(0.6, 0.9, 0.4, 0.7);
        let runs = [
            DecorationRun {
                len: 5,
                color: red,
                background_color: None,
                underline: None,
                strikethrough: None,
            },
            DecorationRun {
                len: 2,
                color: blue,
                background_color: None,
                underline: None,
                strikethrough: None,
            },
        ];
        let glyphs = [(0, 0.0), (1, 4.0), (2, 10.0), (5, 26.0), (6, 34.0)]
            .into_iter()
            .enumerate()
            .map(|(glyph_ix, (index, x))| {
                (
                    WrapBoundary {
                        run_ix: 0,
                        glyph_ix,
                    },
                    index,
                    point(px(x), px(0.0)),
                )
            });
        snapshot.text_glyphs(
            &layout,
            "a 中bc",
            &runs,
            &[WrapBoundary {
                run_ix: 0,
                glyph_ix: 3,
            }],
            &[px(24.0), px(36.0)],
            point(px(40.0), px(100.0)),
            px(24.0),
            None,
            glyphs,
            true,
        );
        let quads = snapshot
            .paints
            .iter()
            .map(|paint| {
                let Primitive::Quad(quad) = &paint.primitive else {
                    panic!("text must use blocks")
                };
                (quad.bounds, quad.background)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            quads,
            vec![
                (
                    Bounds::new(point(px(40.0), px(104.0)), size(px(4.0), px(16.0))),
                    red.into()
                ),
                (
                    Bounds::new(point(px(50.0), px(104.0)), size(px(16.0), px(16.0))),
                    red.into()
                ),
                (
                    Bounds::new(point(px(40.0), px(134.0)), size(px(24.0), px(16.0))),
                    blue.into()
                ),
            ]
        );
    }

    #[test]
    fn backgrounds_and_clips_are_preserved_without_recoloring() {
        let mut snapshot = snapshot(10);
        let color = gpui::hsla(0.3, 0.7, 0.5, 0.4);
        let bounds = Bounds::new(point(px(20.0), px(30.0)), size(px(200.0), px(100.0)));
        let clip = Bounds::new(point(px(0.0), px(0.0)), size(px(100.0), px(60.0)));
        let quad = gpui::quad(
            bounds,
            px(8.0),
            color,
            px(2.0),
            gpui::black(),
            gpui::BorderStyle::default(),
        );
        snapshot.quad(&quad, Some(clip));
        let Primitive::Quad(captured) = &snapshot.paints[0].primitive else {
            panic!("quad")
        };
        assert_eq!(captured.bounds, bounds);
        assert_eq!(captured.background, color.into());
        assert_eq!(captured.corner_radii, quad.corner_radii);
        assert_eq!(captured.border_widths, quad.border_widths);
        assert_eq!(snapshot.paints[0].clip, clip);
    }

    #[test]
    fn loaded_media_is_borrowed_and_not_retained_by_snapshot() {
        let mut snapshot = snapshot(10);
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(
            image::RgbaImage::new(1, 1),
        )]));
        snapshot.media(snapshot.viewport, &image, px(4.0), None);
        assert_eq!(Arc::strong_count(&image), 1);
        let Primitive::Image(bounds, weak, radius) = &snapshot.paints[0].primitive else {
            panic!("image")
        };
        assert_eq!(*bounds, snapshot.viewport);
        assert_eq!(*radius, px(4.0));
        assert!(weak.upgrade().is_some());
        drop(image);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn source_coordinates_use_one_scale_and_local_precision_near_document_end() {
        let viewport = Bounds::new(point(px(100.0), px(200.0)), size(px(800.0), px(600.0)));
        let target = Bounds::new(point(px(950.0), px(200.0)), size(px(100.0), px(600.0)));
        let projected = project_point(
            point(px(140.0), px(224.0)),
            viewport,
            48_000_000.25,
            target,
            4.0,
            0.1,
            4_800_000.0,
        );
        assert!((f32::from(projected.x) - 958.0).abs() < 0.001);
        assert!((f32::from(projected.y) - 202.425).abs() < 0.001);
    }

    #[test]
    fn capture_stops_at_its_budget() {
        let mut snapshot = snapshot(3);
        for i in 0..100 {
            snapshot.quad(
                &fill(
                    Bounds::new(point(px(0.0), px(i as f32 * 2.0)), size(px(10.0), px(1.0))),
                    gpui::black(),
                ),
                None,
            );
        }
        assert_eq!(snapshot.len(), 3);
        assert!(snapshot.full());
    }

    #[test]
    fn existing_path_triangles_are_reused_without_changing_geometry() {
        let a = point(px(10.0), px(20.0));
        let b = point(px(30.0), px(40.0));
        let c = point(px(50.0), px(20.0));
        let uv = (point(0.0, 0.0), point(0.5, 0.0), point(1.0, 1.0));
        let mut path = gpui::Path::new(a);
        path.push_triangle((a, b, c), uv);
        let color = gpui::hsla(0.2, 0.5, 0.3, 0.6);
        let mut snapshot = snapshot(100);
        snapshot.path(&path, color);
        let Primitive::Triangle(points, captured_color) = &snapshot.paints[0].primitive else {
            panic!("triangle")
        };
        assert_eq!(*points, [(a, uv.0), (b, uv.1), (c, uv.2)]);
        assert_eq!(*captured_color, color);
    }

    #[test]
    fn empty_or_whitespace_text_cannot_bypass_the_capture_budget() {
        let mut snapshot = snapshot(4);
        let layout = LineLayout::default();
        for _ in 0..100 {
            snapshot.text_glyphs(
                &layout,
                "",
                &[],
                &[],
                &[],
                point(px(0.0), px(0.0)),
                px(20.0),
                None,
                std::iter::empty(),
                true,
            );
        }
        assert_eq!(snapshot.remaining_texts, 0);
        assert!(!snapshot.full());
        assert_eq!(snapshot.len(), 0);
    }

    #[test]
    fn coalesced_text_is_not_truncated_by_the_rectangle_budget() {
        let mut snapshot = snapshot(10);
        snapshot.viewport.size.width = px(16_000.0);
        let text = "a".repeat(16_000);
        let layout = LineLayout {
            width: px(16_000.0),
            ascent: px(10.0),
            descent: px(2.0),
            ..Default::default()
        };
        let color = gpui::black();
        let runs = [DecorationRun {
            len: 16_000,
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
        let glyphs = (0..16_000).map(|index| {
            (
                WrapBoundary {
                    run_ix: 0,
                    glyph_ix: index,
                },
                index,
                point(px(index as f32), px(0.0)),
            )
        });
        snapshot.text_glyphs(
            &layout,
            &text,
            &runs,
            &[],
            &[],
            point(px(0.0), px(0.0)),
            px(20.0),
            None,
            glyphs,
            true,
        );
        assert_eq!(snapshot.len(), 1);
        let Primitive::Quad(quad) = &snapshot.paints[0].primitive else {
            panic!("text block")
        };
        assert_eq!(quad.bounds.size.width, px(16_000.0));
        assert!(!snapshot.full());
    }

    #[test]
    fn dense_code_capture_reaches_the_bottom_of_the_requested_range() {
        let mut snapshot = Snapshot::new(
            Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(12_000.0))),
            0.0,
            gpui::white(),
            600,
        );
        let text = "x".repeat(100);
        let layout = LineLayout {
            width: px(800.0),
            ascent: px(12.0),
            descent: px(4.0),
            ..Default::default()
        };
        let colors = [DecorationRun {
            len: text.len() as u32,
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
        for row in 0..600 {
            snapshot.text_glyphs(
                &layout,
                &text,
                &colors,
                &[],
                &[],
                point(px(0.0), px(row as f32 * 20.0)),
                px(20.0),
                None,
                (0..100).map(|column| {
                    (
                        WrapBoundary {
                            run_ix: 0,
                            glyph_ix: column,
                        },
                        column,
                        point(px(column as f32 * 8.0), px(0.0)),
                    )
                }),
                false,
            );
        }
        assert_eq!(snapshot.paints.len(), 600);
        assert!(
            snapshot.text_bottom >= 11_998.0,
            "capture stopped at {}",
            snapshot.text_bottom
        );
        let Primitive::Quad(last) = &snapshot.paints.last().unwrap().primitive else {
            panic!("text block");
        };
        assert_eq!(last.bounds.size.width, px(800.0));
    }

    #[test]
    fn offscreen_text_and_primitives_do_not_consume_capture_capacity() {
        let mut snapshot = snapshot(3);
        let layout = LineLayout {
            width: px(80.0),
            ascent: px(12.0),
            descent: px(4.0),
            ..Default::default()
        };
        for row in 0..1000 {
            snapshot.quad(
                &fill(
                    Bounds::new(
                        point(px(0.0), px(-20.0 - row as f32 * 20.0)),
                        size(px(80.0), px(16.0)),
                    ),
                    gpui::black(),
                ),
                None,
            );
            snapshot.text_glyphs(
                &layout,
                "hidden",
                &[],
                &[],
                &[],
                point(px(0.0), px(-20.0 - row as f32 * 20.0)),
                px(20.0),
                None,
                std::iter::from_fn(|| panic!("offscreen glyphs must not be visited")),
                true,
            );
        }
        assert_eq!(snapshot.remaining_items, 3);
        assert_eq!(snapshot.remaining_texts, 3);
        assert_eq!(snapshot.remaining_glyphs, MAX_GLYPHS);
        assert_eq!(snapshot.len(), 0);
    }

    #[test]
    fn dense_syntax_colors_fit_the_shared_snapshot_memory_budget() {
        let mut snapshot = Snapshot::new(
            Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(16_000.0))),
            0.0,
            gpui::white(),
            super::super::snapshot_item_limit(1),
        );
        let text = "x".repeat(100);
        let layout = LineLayout {
            width: px(800.0),
            ascent: px(12.0),
            descent: px(4.0),
            ..Default::default()
        };
        let colors = (0..10)
            .map(|run| DecorationRun {
                len: 10,
                color: if run % 2 == 0 {
                    gpui::black()
                } else {
                    gpui::white()
                },
                background_color: None,
                underline: None,
                strikethrough: None,
            })
            .collect::<Vec<_>>();
        for row in 0..800 {
            snapshot.text_glyphs(
                &layout,
                &text,
                &colors,
                &[],
                &[],
                point(px(0.0), px(row as f32 * 20.0)),
                px(20.0),
                None,
                (0..100).map(|column| {
                    (
                        WrapBoundary {
                            run_ix: 0,
                            glyph_ix: column,
                        },
                        column,
                        point(px(column as f32 * 8.0), px(0.0)),
                    )
                }),
                false,
            );
        }
        assert_eq!(snapshot.paints.len(), 8000);
        assert!(snapshot.text_bottom >= 15_998.0);
    }
}

const MAX_GLYPHS: usize = super::MAX_PREPARE_BYTES;

fn punctuation_gap(ch: char) -> bool {
    if ch.is_ascii() {
        return ch.is_ascii_punctuation();
    }
    static PUNCTUATION: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let mut encoded = [0; 4];
    PUNCTUATION
        .get_or_init(|| regex::Regex::new(r"^\p{P}$").expect("Unicode punctuation pattern"))
        .is_match(ch.encode_utf8(&mut encoded))
}

fn project_point(
    p: Point<Pixels>,
    viewport: Bounds<Pixels>,
    scroll: f64,
    target: Bounds<Pixels>,
    padding: f32,
    scale: f64,
    document_origin: f64,
) -> Point<Pixels> {
    point(
        target.left() + px(padding + f32::from(p.x - viewport.left()) * scale as f32),
        target.top()
            + px(
                ((scroll + f32::from(p.y - viewport.top()) as f64) * scale - document_origin)
                    as f32,
            ),
    )
}

#[derive(Clone)]
enum Primitive {
    Quad(PaintQuad),
    Image(Bounds<Pixels>, Weak<RenderImage>, Pixels),
    Triangle([(Point<Pixels>, Point<f32>); 3], Hsla),
}

impl Primitive {
    fn bounds(&self) -> Bounds<Pixels> {
        match self {
            Self::Quad(quad) => quad.bounds,
            Self::Image(bounds, ..) => *bounds,
            Self::Triangle(points, _) => {
                let [a, b, c] = points.map(|(point, _)| point);
                let left = a.x.min(b.x).min(c.x);
                let top = a.y.min(b.y).min(c.y);
                Bounds::new(
                    point(left, top),
                    size(a.x.max(b.x).max(c.x) - left, a.y.max(b.y).max(c.y) - top),
                )
            }
        }
    }
}

#[derive(Clone)]
struct Paint {
    primitive: Primitive,
    clip: Bounds<Pixels>,
}

/// A bounded silhouette of the final accepted editor frame, in its paint coordinates.
/// Text uses glyph advance cells; all media borrows already-decoded editor resources.
pub(crate) struct Snapshot {
    pub(super) viewport: Bounds<Pixels>,
    pub(super) scroll: f64,
    pub(super) background: Hsla,
    paints: Vec<Paint>,
    remaining_glyphs: usize,
    remaining_items: usize,
    remaining_texts: usize,
    pub(super) text_bottom: f32,
}

impl Snapshot {
    pub(crate) fn new(
        viewport: Bounds<Pixels>,
        scroll: f64,
        background: Hsla,
        limit: usize,
    ) -> Self {
        Self {
            viewport,
            scroll,
            background,
            paints: Vec::new(),
            remaining_glyphs: MAX_GLYPHS,
            remaining_items: limit,
            remaining_texts: limit,
            text_bottom: 0.0,
        }
    }

    pub(super) fn item_size() -> usize {
        std::mem::size_of::<Paint>()
    }

    pub(super) fn len(&self) -> usize {
        self.paints.len()
    }

    pub(crate) fn full(&self) -> bool {
        self.remaining_items == 0
    }

    fn push(&mut self, primitive: Primitive, clip: Option<Bounds<Pixels>>) {
        let clip = clip.map_or(self.viewport, |clip| clip.intersect(&self.viewport));
        let visible = primitive.bounds().intersect(&clip);
        if visible.size.width > px(0.0) && visible.size.height > px(0.0) {
            if let Primitive::Quad(next) = &primitive
                && let Some(Paint {
                    primitive: Primitive::Quad(previous),
                    clip: previous_clip,
                }) = self.paints.last_mut()
                && *previous_clip == clip
                && previous.background == next.background
                && previous.border_widths == gpui::Edges::all(px(0.0))
                && next.border_widths == previous.border_widths
                && previous.corner_radii == Corners::all(px(0.0))
                && next.corner_radii == previous.corner_radii
                && previous.bounds.top() == next.bounds.top()
                && previous.bounds.size.height == next.bounds.size.height
                && previous.bounds.right() == next.bounds.left()
            {
                previous.bounds.size.width += next.bounds.size.width;
                return;
            }
            if self.full() {
                return;
            }
            self.remaining_items -= 1;
            self.paints.push(Paint { primitive, clip });
        }
    }

    pub(crate) fn quad(&mut self, quad: &PaintQuad, clip: Option<Bounds<Pixels>>) {
        self.push(Primitive::Quad(quad.clone()), clip);
    }

    pub(crate) fn media(
        &mut self,
        bounds: Bounds<Pixels>,
        image: &Arc<RenderImage>,
        radius: Pixels,
        clip: Option<Bounds<Pixels>>,
    ) {
        self.push(
            Primitive::Image(bounds, Arc::downgrade(image), radius),
            clip,
        );
    }

    pub(crate) fn icon(&mut self, bounds: Bounds<Pixels>, color: Hsla) {
        self.quad(&fill(bounds, color), None);
    }

    pub(crate) fn path(&mut self, path: &gpui::Path<Pixels>, color: Hsla) {
        for triangle in path.triangles().take(self.remaining_items) {
            self.push(Primitive::Triangle(triangle, color), None);
        }
    }

    pub(crate) fn wrapped(
        &mut self,
        line: &WrappedLine,
        origin: Point<Pixels>,
        line_height: Pixels,
        clip: Option<Bounds<Pixels>>,
        punctuation_gaps: bool,
    ) {
        self.text(
            &line.unwrapped_layout,
            &line.text,
            line.decoration_runs(),
            &line.wrap_boundaries,
            &line.row_heights,
            origin,
            line_height,
            clip,
            punctuation_gaps,
        );
    }

    pub(crate) fn shaped(&mut self, line: &ShapedLine, origin: Point<Pixels>, height: Pixels) {
        self.text(
            line,
            &line.text,
            line.decoration_runs(),
            &[],
            &[],
            origin,
            height,
            None,
            false,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        layout: &LineLayout,
        text: &str,
        colors: &[DecorationRun],
        wraps: &[WrapBoundary],
        row_heights: &[Pixels],
        origin: Point<Pixels>,
        fallback: Pixels,
        clip: Option<Bounds<Pixels>>,
        punctuation_gaps: bool,
    ) {
        let glyphs = layout.runs.iter().enumerate().flat_map(|(run_ix, run)| {
            run.glyphs.iter().enumerate().map(move |(glyph_ix, glyph)| {
                (
                    WrapBoundary { run_ix, glyph_ix },
                    glyph.index,
                    glyph.position,
                )
            })
        });
        self.text_glyphs(
            layout,
            text,
            colors,
            wraps,
            row_heights,
            origin,
            fallback,
            clip,
            glyphs,
            punctuation_gaps,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn text_glyphs(
        &mut self,
        layout: &LineLayout,
        text: &str,
        colors: &[DecorationRun],
        wraps: &[WrapBoundary],
        row_heights: &[Pixels],
        origin: Point<Pixels>,
        fallback: Pixels,
        clip: Option<Bounds<Pixels>>,
        glyphs: impl Iterator<Item = (WrapBoundary, usize, Point<Pixels>)>,
        punctuation_gaps: bool,
    ) {
        if self.full() || self.remaining_texts == 0 || self.remaining_glyphs == 0 {
            return;
        }
        let rows = wraps.len() + 1;
        let height = row_heights
            .iter()
            .take(rows)
            .fold(px(0.0), |height, row| height + *row)
            + fallback * rows.saturating_sub(row_heights.len()) as f32;
        let visible = clip.map_or(self.viewport, |clip| clip.intersect(&self.viewport));
        if origin.y >= visible.bottom()
            || origin.y + height <= visible.top()
            || visible.size.width <= px(0.0)
            || visible.size.height <= px(0.0)
        {
            return;
        }
        self.remaining_texts -= 1;
        let mut glyphs = glyphs.peekable();
        let mut wraps = wraps.iter().peekable();
        let mut row = 0;
        let mut top = origin.y;
        let mut row_height = row_heights.first().copied().unwrap_or(fallback);
        let mut row_x = px(0.0);
        let mut color_index = 0;
        let mut color_end = colors.first().map_or(0, |run| run.len as usize);
        while let Some((index, source_index, position)) = glyphs.next() {
            if self.remaining_glyphs == 0 {
                break;
            }
            self.remaining_glyphs -= 1;
            if wraps.peek() == Some(&&index) {
                wraps.next();
                top += row_height;
                row += 1;
                row_height = row_heights.get(row).copied().unwrap_or(fallback);
                row_x = position.x;
            }
            while source_index >= color_end && color_index < colors.len() {
                color_index += 1;
                color_end += colors.get(color_index).map_or(0, |run| run.len as usize);
            }
            let Some(style) = colors.get(color_index) else {
                continue;
            };
            let end_x = glyphs.peek().map_or(layout.width, |(_, _, next)| next.x);
            let width = end_x - position.x;
            if width <= px(0.0) {
                continue;
            }
            let x = origin.x + position.x - row_x;
            // Advance cells preserve actual spacing, wrapping, run colors and font metrics.
            if text
                .get(source_index..)
                .and_then(|tail| tail.chars().next())
                .is_none_or(|ch| ch.is_whitespace() || (punctuation_gaps && punctuation_gap(ch)))
            {
                continue;
            }
            {
                let height = layout.ascent + layout.descent;
                let y = top + (row_height - height) / 2.0;
                if y < self.viewport.bottom() && y + height > self.viewport.top() {
                    self.text_bottom = self.text_bottom.max(f32::from(
                        (y + height).min(self.viewport.bottom()) - self.viewport.top(),
                    ));
                }
                self.quad(
                    &fill(Bounds::new(point(x, y), size(width, height)), style.color),
                    clip,
                );
            }
            if let Some(underline) = style.underline {
                let y = top
                    + (row_height - layout.ascent - layout.descent) / 2.0
                    + layout.ascent
                    + layout.descent * 0.618;
                self.quad(
                    &fill(
                        Bounds::new(point(x, y), size(width, underline.thickness)),
                        underline.color.unwrap_or(style.color),
                    ),
                    clip,
                );
            }
            if let Some(strike) = style.strikethrough {
                let fallback_baseline =
                    (fallback - layout.ascent - layout.descent) / 2.0 + layout.ascent;
                let y = top
                    + (layout.ascent * 0.5 + fallback_baseline) * 0.5
                    + (row_height - fallback) / 2.0;
                self.quad(
                    &fill(
                        Bounds::new(point(x, y), size(width, strike.thickness)),
                        strike.color.unwrap_or(style.color),
                    ),
                    clip,
                );
            }
        }
    }

    pub(super) fn paint(
        &self,
        target: Bounds<Pixels>,
        padding: f32,
        scale: f64,
        document_origin: f64,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let transform = |p: Point<Pixels>| {
            project_point(
                p,
                self.viewport,
                self.scroll,
                target,
                padding,
                scale,
                document_origin,
            )
        };
        let bounds =
            |b: Bounds<Pixels>| Bounds::new(transform(b.origin), b.size.map(|v| v * scale as f32));
        let viewport = bounds(self.viewport).intersect(&target);
        if viewport.size.width <= px(0.0) || viewport.size.height <= px(0.0) {
            return;
        }
        // The lane owns its translucent background; snapshots must not stack opaque fills.
        for paint in &self.paints {
            let clip = bounds(paint.clip).intersect(&target);
            if clip.size.width <= px(0.0) || clip.size.height <= px(0.0) {
                continue;
            }
            window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                match &paint.primitive {
                    Primitive::Quad(source) => {
                        let mut quad = source.clone();
                        quad.bounds = bounds(quad.bounds);
                        quad.corner_radii = quad.corner_radii.map(|v| *v * scale as f32);
                        quad.border_widths = quad.border_widths.map(|v| *v * scale as f32);
                        window.paint_quad(quad);
                    }
                    Primitive::Image(source, image, radius) => {
                        if let Some(image) = image.upgrade()
                            && let Err(error) = window.paint_image(
                                bounds(*source),
                                Corners::all(*radius * scale as f32),
                                image,
                                0,
                                false,
                            )
                        {
                            eprintln!("Paint minimap image: {error}");
                        }
                    }
                    Primitive::Triangle(triangle, color) => {
                        let [a, b, c] = triangle;
                        let mut path = gpui::Path::new(transform(a.0));
                        path.push_triangle(
                            (transform(a.0), transform(b.0), transform(c.0)),
                            (a.1, b.1, c.1),
                        );
                        window.paint_path(path, *color);
                    }
                }
            });
        }
    }
}
