use std::ops::Range;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use crate::i18n::runtime::RuntimeText;
use anyhow::{Result, bail};
use hayro::hayro_interpret::{
    self as interpret, BlendMode, ClipPath, Device, DrawMode, DrawProps, Image, ImageDrawProps,
    InterpreterCache, InterpreterSettings, RectExt, SoftMask, TransformExt,
    font::{Glyph, GlyphRun},
    hayro_cmap::{BfString, WritingMode},
};
use hayro::hayro_syntax::{Pdf, object::Dict};
use hayro::kurbo::{Affine, BezPath, Point, Rect, Shape};

const MAX_PAGE_PIXELS: usize = 16 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(super) enum LinkTarget {
    Page {
        page: usize,
        point: Option<Point>,
        zoom: Option<f32>,
    },
    Fit {
        page: usize,
        rectangle: Option<Rect>,
        width: bool,
        height: bool,
        point: Option<Point>,
    },
    Url(String),
    Unsupported,
}

fn named_target<'a>(pdf: &'a Pdf, key: &[u8]) -> Option<hayro::hayro_syntax::object::Object<'a>> {
    use hayro::hayro_syntax::object::{Array, Dict, Object};
    let catalog = pdf.xref().get::<Dict<'_>>(pdf.xref().root_id())?;
    if let Some(value) = catalog
        .get::<Dict<'_>>(b"Dests")
        .and_then(|dict| dict.get::<Object<'_>>(key))
    {
        return Some(value);
    }
    let root = catalog
        .get::<Dict<'_>>(b"Names")?
        .get::<Dict<'_>>(b"Dests")?;
    let mut stack = vec![root];
    let mut count = 0;
    let mut seen = std::collections::HashSet::new();
    while let Some(node) = stack.pop() {
        count += 1;
        if count > 10000 || !seen.insert((node.data().as_ptr(), node.data().len())) {
            return None;
        }
        if let Some(names) = node.get::<Array<'_>>(b"Names") {
            let mut entries = names.flex_iter();
            while let Some(name) = entries.next::<hayro::hayro_syntax::object::String<'_>>() {
                let value = entries.next::<Object<'_>>()?;
                if name.as_bytes() == key {
                    return Some(value);
                }
            }
        }
        if let Some(kids) = node.get::<Array<'_>>(b"Kids") {
            stack.extend(kids.iter::<Dict<'_>>());
        }
    }
    None
}

#[derive(Clone, Debug)]
pub(super) struct PdfLink {
    pub bounds: Rect,
    pub quads: Vec<BezPath>,
    pub target: LinkTarget,
}

impl PdfLink {
    pub fn contains(&self, point: Point) -> bool {
        if self.quads.is_empty() {
            self.bounds.contains(point)
        } else {
            self.quads.iter().any(|quad| quad.winding(point) != 0)
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct PdfHeading {
    pub title: String,
    pub depth: usize,
    pub target: LinkTarget,
}

#[derive(Clone, Debug)]
pub(super) struct PageInfo {
    pub width: f32,
    pub height: f32,
    pub links: Vec<PdfLink>,
}

#[derive(Clone, Debug)]
pub(super) struct DocumentInfo {
    pub pages: Vec<PageInfo>,
    pub headings: Vec<PdfHeading>,
    pub outline_error: Option<String>,
}

fn target(pdf: &Pdf, object: hayro::hayro_syntax::object::Object<'_>, depth: usize) -> LinkTarget {
    use hayro::hayro_syntax::object::{Name, Object};
    if depth > 16 {
        return LinkTarget::Unsupported;
    }
    match object {
        Object::Dict(dict) => {
            if dict
                .get::<Name<'_>>(b"S")
                .is_some_and(|name| name.as_ref() == b"URI")
                && let Some(uri) = dict.get::<hayro::hayro_syntax::object::String<'_>>(b"URI")
                && let Ok(uri) = decode_string(uri.as_bytes())
                && ["https://", "http://", "mailto:"].iter().any(|scheme| {
                    uri.get(..scheme.len())
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
                })
            {
                return LinkTarget::Url(uri);
            }
            if dict
                .get::<Name<'_>>(b"S")
                .is_none_or(|name| name.as_ref() == b"GoTo")
                && let Some(dest) = dict.get::<Object<'_>>(b"D")
            {
                return target(pdf, dest, depth + 1);
            }
            if let Some(dest) = dict.get::<Object<'_>>(b"Dest") {
                return target(pdf, dest, depth + 1);
            }
            LinkTarget::Unsupported
        }
        Object::Array(array) => {
            let Some(first) = array
                .raw_iter()
                .next()
                .and_then(|object| object.as_obj_ref())
            else {
                return LinkTarget::Unsupported;
            };
            let Some(page) = pdf
                .pages()
                .iter()
                .position(|page| page.raw().obj_id() == Some(first.into()))
            else {
                return LinkTarget::Unsupported;
            };
            let mut items = array.flex_iter();
            let _ = items.next::<Object<'_>>();
            let mode = items.next::<Name<'_>>();
            let Some(mode) = mode else {
                return LinkTarget::Unsupported;
            };
            if matches!(mode.as_ref(), b"Fit" | b"FitB") {
                return LinkTarget::Fit {
                    page,
                    rectangle: None,
                    width: true,
                    height: true,
                    point: None,
                };
            }
            if matches!(mode.as_ref(), b"FitH" | b"FitBH" | b"FitV" | b"FitBV") {
                let width = matches!(mode.as_ref(), b"FitH" | b"FitBH");
                let coordinate = items
                    .next::<Object<'_>>()
                    .and_then(|value| f64::try_from(value).ok());
                let point = coordinate.map(|value| {
                    pdf.pages()[page].initial_transform(true).to_kurbo()
                        * if width {
                            Point::new(0.0, value)
                        } else {
                            Point::new(value, 0.0)
                        }
                });
                return LinkTarget::Fit {
                    page,
                    rectangle: None,
                    width,
                    height: !width,
                    point,
                };
            }
            if mode.as_ref() == b"FitR" {
                let values = (
                    items.next::<f64>(),
                    items.next::<f64>(),
                    items.next::<f64>(),
                    items.next::<f64>(),
                );
                if let (Some(x0), Some(y0), Some(x1), Some(y1)) = values {
                    let rectangle = pdf.pages()[page]
                        .initial_transform(true)
                        .to_kurbo()
                        .transform_rect_bbox(Rect::new(x0, y0, x1, y1));
                    if rectangle.width() > 0.0 && rectangle.height() > 0.0 {
                        return LinkTarget::Fit {
                            page,
                            rectangle: Some(rectangle),
                            width: true,
                            height: true,
                            point: None,
                        };
                    }
                }
                return LinkTarget::Unsupported;
            }
            let (point, zoom) = if mode.as_ref() == b"XYZ" {
                let x = items
                    .next::<Object<'_>>()
                    .and_then(|value| f64::try_from(value).ok());
                let y = items
                    .next::<Object<'_>>()
                    .and_then(|value| f64::try_from(value).ok());
                let zoom = items
                    .next::<Object<'_>>()
                    .and_then(|value| f32::try_from(value).ok())
                    .filter(|zoom| zoom.is_finite() && *zoom > 0.0);
                (
                    y.or(x).map(|_| {
                        pdf.pages()[page].initial_transform(true).to_kurbo()
                            * Point::new(x.unwrap_or(0.0), y.unwrap_or(0.0))
                    }),
                    zoom,
                )
            } else {
                return LinkTarget::Unsupported;
            };
            LinkTarget::Page { page, point, zoom }
        }
        Object::Name(name) => named_target(pdf, name.as_ref())
            .map_or(LinkTarget::Unsupported, |object| {
                target(pdf, object, depth + 1)
            }),
        Object::String(name) => named_target(pdf, name.as_bytes())
            .map_or(LinkTarget::Unsupported, |object| {
                target(pdf, object, depth + 1)
            }),
        _ => LinkTarget::Unsupported,
    }
}

pub(super) fn inspect(pdf: &Pdf) -> Result<DocumentInfo> {
    use hayro::hayro_syntax::object::{Array, Name, Object};
    let mut pages = Vec::new();
    for page in pdf.pages().iter() {
        let (width, height) = page.render_dimensions();
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            bail!(RuntimeText::InvalidDimensions);
        }
        let mut links = Vec::new();
        if let Some(annotations) = page.raw().get::<Array<'_>>(b"Annots") {
            for annotation in annotations.iter::<Dict<'_>>() {
                if annotation.get::<u32>(b"F").unwrap_or(0) & (2 | 32) != 0 {
                    continue;
                }
                if annotation
                    .get::<Name<'_>>(b"Subtype")
                    .is_none_or(|name| name.as_ref() != b"Link")
                {
                    continue;
                }
                let Some(rect) = annotation.get::<hayro::hayro_syntax::object::Rect>(b"Rect")
                else {
                    continue;
                };
                let destination = annotation
                    .get::<Object<'_>>(b"Dest")
                    .or_else(|| annotation.get(b"A"));
                let mut quads = Vec::new();
                if let Some(points) = annotation.get::<Array<'_>>(b"QuadPoints") {
                    let values = points.iter::<f64>().take(8192).collect::<Vec<_>>();
                    for coordinates in values.chunks_exact(8) {
                        let transform = page.initial_transform(true).to_kurbo();
                        let mut points = [0, 1, 2, 3].map(|index| {
                            transform
                                * Point::new(coordinates[index * 2], coordinates[index * 2 + 1])
                        });
                        let center = points
                            .iter()
                            .fold(Point::ZERO, |sum, point| sum + point.to_vec2())
                            .to_vec2()
                            / 4.0;
                        points.sort_by(|a, b| {
                            (a.y - center.y)
                                .atan2(a.x - center.x)
                                .total_cmp(&(b.y - center.y).atan2(b.x - center.x))
                        });
                        let mut quad = BezPath::new();
                        quad.move_to(points[0]);
                        for point in &points[1..] {
                            quad.line_to(*point);
                        }
                        quad.close_path();
                        quads.push(quad);
                    }
                }
                links.push(PdfLink {
                    bounds: page
                        .initial_transform(true)
                        .to_kurbo()
                        .transform_rect_bbox(rect.to_kurbo()),
                    quads,
                    target: destination
                        .map_or(LinkTarget::Unsupported, |value| target(pdf, value, 0)),
                });
            }
        }
        pages.push(PageInfo {
            width,
            height,
            links,
        });
    }
    if pages.is_empty() {
        bail!(RuntimeText::DocumentUnavailable);
    }
    let mut headings = Vec::new();
    let catalog = pdf.xref().get::<Dict<'_>>(pdf.xref().root_id());
    let mut stack = Vec::new();
    if let Some(first) = catalog
        .and_then(|catalog| catalog.get::<Dict<'_>>(b"Outlines"))
        .and_then(|dict| dict.get::<Dict<'_>>(b"First"))
    {
        stack.push((first, 0));
    }
    let mut seen = std::collections::HashSet::new();
    let mut outline_error = None;
    while let Some((node, depth)) = stack.pop() {
        if headings.len() >= 10000
            || depth > 64
            || !seen.insert((node.data().as_ptr(), node.data().len()))
        {
            outline_error = Some(RuntimeText::PdfOutlineInvalid.to_string());
            break;
        }
        let title = match node
            .get::<hayro::hayro_syntax::object::String<'_>>(b"Title")
            .map(|title| decode_string(title.as_bytes()))
            .transpose()
        {
            Ok(title) => title.unwrap_or_default(),
            Err(error) => {
                outline_error = Some(format!("{}: {error}", RuntimeText::PdfOutlineInvalid));
                break;
            }
        };
        let destination = node.get::<Object<'_>>(b"Dest").or_else(|| node.get(b"A"));
        headings.push(PdfHeading {
            title,
            depth,
            target: destination.map_or(LinkTarget::Unsupported, |value| target(pdf, value, 0)),
        });
        let next = node
            .get_ref(b"Next")
            .and_then(|reference| pdf.xref().get::<Dict<'_>>(reference.into()))
            .or_else(|| node.get::<Dict<'_>>(b"Next"));
        let first = node
            .get_ref(b"First")
            .and_then(|reference| pdf.xref().get::<Dict<'_>>(reference.into()))
            .or_else(|| node.get::<Dict<'_>>(b"First"));
        if node.contains_key(b"Next") && next.is_none()
            || node.contains_key(b"First") && first.is_none()
        {
            outline_error = Some(RuntimeText::PdfOutlineInvalid.to_string());
        }
        if let Some(next) = next {
            stack.push((next, depth));
        }
        if let Some(first) = first {
            stack.push((first, depth + 1));
        }
    }
    Ok(DocumentInfo {
        pages,
        headings,
        outline_error,
    })
}

#[derive(Clone, Debug)]
pub(super) struct TextCluster {
    pub range: Range<usize>,
    pub bounds: Rect,
}

#[derive(Clone, Debug, Default)]
pub(super) struct PageText {
    pub text: String,
    pub clusters: Vec<TextCluster>,
    pub unmapped: usize,
    pub images: Vec<PageImage>,
}

#[derive(Clone, Debug)]
pub(super) struct PageImage {
    pub index: usize,
    pub bounds: Rect,
    transform: Affine,
    width: u32,
    height: u32,
}

impl PageImage {
    pub fn contains(&self, point: Point) -> bool {
        self.bounds.contains(point)
            && Rect::new(0.0, 0.0, f64::from(self.width), f64::from(self.height))
                .contains(self.transform.inverse() * point)
    }
}

impl PageText {
    pub fn hit(&self, point: Point) -> Option<usize> {
        self.clusters
            .iter()
            .filter(|cluster| cluster.bounds.contains(point))
            .min_by(|a, b| a.bounds.area().total_cmp(&b.bounds.area()))
            .map(|cluster| cluster.range.start)
    }

    pub fn nearest_hit(&self, point: Point) -> Option<(&TextCluster, f64)> {
        self.clusters
            .iter()
            .map(|cluster| {
                let bounds = cluster.bounds;
                let dx = (bounds.x0 - point.x).max(0.0).max(point.x - bounds.x1);
                let dy = (bounds.y0 - point.y).max(0.0).max(point.y - bounds.y1);
                (cluster, dx * dx + dy * dy)
            })
            .min_by(|(a, da), (b, db)| {
                da.total_cmp(db)
                    .then_with(|| a.bounds.area().total_cmp(&b.bounds.area()))
            })
    }

    pub fn highlight_rects(&self, range: &Range<usize>) -> Vec<Rect> {
        if range.is_empty() {
            return Vec::new();
        }
        let mut rectangles: Vec<Rect> = Vec::new();
        let mut previous: Option<&TextCluster> = None;
        let start = self
            .clusters
            .partition_point(|cluster| cluster.range.end <= range.start);
        for cluster in self.clusters[start..]
            .iter()
            .take_while(|cluster| cluster.range.start < range.end)
        {
            let join = previous.is_some_and(|previous| {
                self.text
                    .get(previous.range.start..cluster.range.start)
                    .is_some_and(|text| !text.contains(['\n', '\r']))
            });
            if join && let Some(last) = rectangles.last_mut() {
                *last = last.union(cluster.bounds);
            } else {
                rectangles.push(cluster.bounds);
            }
            previous = Some(cluster);
        }
        rectangles
    }
}

struct TextDevice {
    text: PageText,
    previous: Option<(Point, Point, f64)>,
    actual: Vec<Option<(String, usize, Option<Rect>)>>,
    seen: std::collections::HashSet<(String, [u64; 6])>,
    clips: Vec<Rect>,
    limit: usize,
    exhausted: bool,
    image_index: usize,
}

#[derive(Default)]
struct GlyphBounds {
    bounds: Option<Rect>,
}

impl<'a> Device<'a> for GlyphBounds {
    fn draw_path(&mut self, path: &BezPath, props: DrawProps<'a>, _: &DrawMode) {
        let bounds = props.transform.transform_rect_bbox(path.bounding_box());
        self.bounds = Some(self.bounds.map_or(bounds, |prior| prior.union(bounds)));
    }
    fn draw_image(&mut self, _: Image<'a, '_>, props: ImageDrawProps<'a>) {
        let bounds = props
            .transform
            .transform_rect_bbox(Rect::new(0.0, 0.0, 1.0, 1.0));
        self.bounds = Some(self.bounds.map_or(bounds, |prior| prior.union(bounds)));
    }
    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, _: &DrawMode) {
        for glyph in run.glyphs() {
            if let Glyph::Outline(outline) = &**glyph {
                let bounds = (props.transform * glyph.transform())
                    .transform_rect_bbox(outline.outline().bounding_box());
                self.bounds = Some(self.bounds.map_or(bounds, |prior| prior.union(bounds)));
            }
        }
    }
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

impl TextDevice {
    fn append(&mut self, text: &str, bounds: Rect) {
        if self
            .text
            .text
            .len()
            .saturating_add(text.len())
            .saturating_add((self.text.clusters.len() + 1) * std::mem::size_of::<TextCluster>())
            .saturating_add(self.text.images.len() * std::mem::size_of::<PageImage>())
            > self.limit
        {
            self.exhausted = true;
            return;
        }
        if let Some(Some((_, _, combined))) = self.actual.iter_mut().rev().find(|v| v.is_some()) {
            *combined = Some(combined.map_or(bounds, |rect| rect.union(bounds)));
            return;
        }
        let start = self.text.text.len();
        self.text.text.push_str(text);
        self.text.clusters.push(TextCluster {
            range: start..self.text.text.len(),
            bounds,
        });
    }
}

impl<'a> Device<'a> for TextDevice {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, clip: &ClipPath) {
        let bounds = clip.path.bounding_box();
        self.clips.push(
            self.clips
                .last()
                .map_or(bounds, |prior| prior.intersect(bounds)),
        );
    }
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn pop_clip(&mut self) {
        self.clips.pop();
    }
    fn pop_transparency_group(&mut self) {}
    fn draw_image(&mut self, image: Image<'a, '_>, props: ImageDrawProps<'a>) {
        let index = self.image_index;
        self.image_index += 1;
        if self.exhausted {
            return;
        }
        let width = image.width();
        let height = image.height();
        let bounds = props.transform.transform_rect_bbox(Rect::new(
            0.0,
            0.0,
            f64::from(width),
            f64::from(height),
        ));
        let bounds = self
            .clips
            .last()
            .map_or(bounds, |clip| bounds.intersect(*clip));
        if !bounds.is_finite()
            || bounds.width() <= 0.0
            || bounds.height() <= 0.0
            || !props.transform.determinant().is_finite()
            || props.transform.determinant() == 0.0
        {
            return;
        }
        if self.text.text.len()
            + self.text.clusters.len() * std::mem::size_of::<TextCluster>()
            + (self.text.images.len() + 1) * std::mem::size_of::<PageImage>()
            > self.limit
        {
            self.exhausted = true;
            return;
        }
        self.text.images.push(PageImage {
            index,
            bounds,
            transform: props.transform,
            width,
            height,
        });
    }

    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, _: &DrawMode) {
        for positioned in run.glyphs() {
            if self.exhausted {
                break;
            }
            let Some(unicode) = positioned.as_unicode() else {
                self.text.unmapped += 1;
                continue;
            };
            let text = match unicode {
                BfString::Char(character) => character.to_string(),
                BfString::String(string) => string,
            };
            if self.seen.len() >= self.limit / 128 || text.len() > self.limit {
                self.exhausted = true;
                break;
            }
            let transform = props.transform * positioned.transform();
            if !self
                .seen
                .insert((text.clone(), transform.as_coeffs().map(f64::to_bits)))
            {
                continue;
            }
            let (bounds, advance_width) = match &**positioned {
                Glyph::Outline(glyph) => (
                    transform.transform_rect_bbox(glyph.outline().bounding_box()),
                    glyph.advance_width(),
                ),
                Glyph::Type3(glyph) => {
                    let mut bounds = GlyphBounds::default();
                    glyph.interpret(
                        &mut bounds,
                        props.transform,
                        positioned.transform(),
                        &props.paint,
                    );
                    let Some(bounds) = bounds.bounds else {
                        self.text.unmapped += 1;
                        continue;
                    };
                    (bounds, glyph.advance_width())
                }
            };
            let bounds = self
                .clips
                .last()
                .map_or(bounds, |clip| bounds.intersect(*clip));
            let origin = transform * Point::ZERO;
            let vertical = run.writing_mode() == WritingMode::Vertical;
            let advance = transform
                * if vertical {
                    Point::new(0.0, -1000.0)
                } else {
                    Point::new(f64::from(advance_width.unwrap_or(0.0)), 0.0)
                };
            let unit = transform
                * if vertical {
                    Point::new(0.0, -1000.0)
                } else {
                    Point::new(1000.0, 0.0)
                }
                - origin;
            let height = unit.hypot();
            if !self.actual.iter().any(Option::is_some)
                && let Some((previous_origin, previous_end, _)) = self.previous
                && height > 0.0
            {
                let axis = unit / height;
                let delta = origin - previous_origin;
                let line_distance = (delta.x * axis.y - delta.y * axis.x).abs();
                let gap = (origin - previous_end).dot(axis);
                if line_distance > height * 0.7 {
                    self.text.text.push('\n');
                } else if gap > height * 0.2
                    && !self.text.text.ends_with(char::is_whitespace)
                    && !text.starts_with(char::is_whitespace)
                {
                    self.text.text.push(' ');
                }
            }
            self.previous = Some((origin, advance, height));
            if bounds.width() > 0.0 && bounds.height() > 0.0 {
                self.append(&text, bounds);
            } else if text.chars().all(char::is_whitespace) {
                self.text.text.push_str(&text);
            }
        }
    }

    fn begin_marked_content(&mut self, _: &[u8], properties: Option<&Dict<'_>>) {
        let actual = properties
            .and_then(|dict| dict.get::<hayro::hayro_syntax::object::String<'_>>(b"ActualText"))
            .and_then(|string| match decode_string(string.as_bytes()) {
                Ok(text) => Some(text),
                Err(_) => {
                    self.text.unmapped += 1;
                    None
                }
            });
        self.actual
            .push(actual.map(|text| (text, self.text.text.len(), None)));
    }

    fn end_marked_content(&mut self) {
        if let Some(Some((text, _, Some(bounds)))) = self.actual.pop() {
            self.append(&text, bounds);
        }
    }
}

pub(super) fn decode_string(bytes: &[u8]) -> Result<String> {
    if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        if bytes.len() % 2 != 0 {
            bail!(RuntimeText::InvalidOutput);
        }
        return Ok(String::from_utf16(
            &bytes
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>(),
        )?);
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return Ok(std::str::from_utf8(bytes)?.to_owned());
    }
    // PDFDocEncoding differs from Latin-1 in these two ranges.
    const LOW: [char; 8] = ['˘', 'ˇ', 'ˆ', '˙', '˝', '˛', '˚', '˜'];
    const HIGH: [char; 32] = [
        '•', '†', '‡', '…', '—', '–', 'ƒ', '⁄', '‹', '›', '−', '‰', '„', '“', '”', '‘', '’', '‚',
        '™', 'ﬁ', 'ﬂ', 'Ł', 'Œ', 'Š', 'Ÿ', 'Ž', 'ı', 'ł', 'œ', 'š', 'ž', '\0',
    ];
    Ok(bytes
        .iter()
        .map(|&byte| match byte {
            0x18..=0x1f => LOW[(byte - 0x18) as usize],
            0x80..=0x9f => HIGH[(byte - 0x80) as usize],
            0xa0 => '€',
            _ => char::from(byte),
        })
        .collect())
}

pub(super) fn parse(bytes: Arc<[u8]>) -> Result<Pdf> {
    // PdfData requires a sized owner; wrap the shared slice instead of copying it.
    Pdf::new(Arc::new(bytes)).map_err(|error| match error {
        hayro::hayro_syntax::LoadPdfError::Decryption(error) => {
            anyhow::anyhow!("{}: {error:?}", RuntimeText::PdfProtected)
        }
        error => anyhow::anyhow!("{}: {error:?}", RuntimeText::RenderFailed),
    })
}

pub(super) fn extract(pdf: &Pdf, cancelled: &AtomicBool) -> Result<Vec<PageText>> {
    let cache = InterpreterCache::new();
    let mut total = 0usize;
    let mut pages = Vec::new();
    for page in pdf.pages().iter() {
        if cancelled.load(Ordering::Relaxed) {
            bail!(RuntimeText::DocumentUnavailable);
        }
        let (width, height) = page.render_dimensions();
        let mut device = TextDevice {
            text: PageText::default(),
            previous: None,
            actual: Vec::new(),
            seen: std::collections::HashSet::new(),
            clips: Vec::new(),
            limit: MAX_TEXT_BYTES - total,
            exhausted: false,
            image_index: 0,
        };
        let mut context = interpret::Context::new(
            page.initial_transform(true).to_kurbo(),
            Rect::new(0.0, 0.0, width as f64, height as f64),
            &cache,
            page.xref(),
            InterpreterSettings::default(),
        );
        interpret::interpret_page(page, &mut context, &mut device);
        if device.exhausted {
            bail!(RuntimeText::PdfMemoryLimit);
        }
        total = total
            .checked_add(device.text.text.len())
            .and_then(|total| {
                total.checked_add(device.text.clusters.len() * std::mem::size_of::<TextCluster>())
            })
            .and_then(|total| {
                total.checked_add(device.text.images.len() * std::mem::size_of::<PageImage>())
            })
            .ok_or_else(|| anyhow::anyhow!(RuntimeText::PdfMemoryLimit))?;
        if total > MAX_TEXT_BYTES {
            bail!(RuntimeText::PdfMemoryLimit);
        }
        pages.push(device.text);
    }
    Ok(pages)
}

fn checked_image_dimensions(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 {
        bail!(RuntimeText::InvalidDimensions);
    }
    if u64::from(width) * u64::from(height) > MAX_PAGE_PIXELS as u64 {
        bail!(RuntimeText::PdfMemoryLimit);
    }
    Ok(())
}

fn decode_image(image: Image<'_, '_>) -> Result<image::RgbaImage> {
    use interpret::{ImageData, Paint};
    checked_image_dimensions(image.width(), image.height())?;
    let mut result = Err(anyhow::anyhow!(RuntimeText::InvalidOutput));
    match image {
        Image::Raster(image) => image.with_rgba(
            |data, alpha| {
                result = (|| {
                    let (width, height) = (data.width(), data.height());
                    checked_image_dimensions(width, height)?;
                    let mut rgba = match data {
                        ImageData::Rgb(data) => image::RgbImage::from_raw(width, height, data.data)
                            .map(image::DynamicImage::ImageRgb8),
                        ImageData::Luma(data) => {
                            image::GrayImage::from_raw(width, height, data.data)
                                .map(image::DynamicImage::ImageLuma8)
                        }
                    }
                    .ok_or_else(|| anyhow::anyhow!(RuntimeText::InvalidOutput))?
                    .into_rgba8();
                    if let Some(alpha) = alpha {
                        checked_image_dimensions(alpha.width, alpha.height)?;
                        let alpha =
                            image::GrayImage::from_raw(alpha.width, alpha.height, alpha.data)
                                .ok_or_else(|| anyhow::anyhow!(RuntimeText::InvalidOutput))?;
                        let alpha = if alpha.dimensions() == (width, height) {
                            alpha
                        } else {
                            image::imageops::resize(
                                &alpha,
                                width,
                                height,
                                image::imageops::FilterType::Triangle,
                            )
                        };
                        for (pixel, alpha) in rgba.pixels_mut().zip(alpha.pixels()) {
                            pixel[3] = alpha[0];
                        }
                    }
                    Ok(rgba)
                })();
            },
            None,
        ),
        Image::Stencil(image) => image.with_stencil(
            |data, paint| {
                result = (|| {
                    checked_image_dimensions(data.width, data.height)?;
                    let Paint::Color(color) = paint else {
                        bail!(RuntimeText::FeatureUnavailable);
                    };
                    let color = color.to_rgba().to_rgba8();
                    let alpha = image::GrayImage::from_raw(data.width, data.height, data.data)
                        .ok_or_else(|| anyhow::anyhow!(RuntimeText::InvalidOutput))?;
                    Ok(image::RgbaImage::from_fn(
                        data.width,
                        data.height,
                        |x, y| {
                            image::Rgba([
                                color[0],
                                color[1],
                                color[2],
                                (u16::from(alpha.get_pixel(x, y)[0]) * u16::from(color[3]) / 255)
                                    as u8,
                            ])
                        },
                    ))
                })();
            },
            None,
        ),
    }
    result
}

pub(super) fn image_png(
    pdf: &Pdf,
    page: usize,
    image: usize,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>> {
    struct ImageDevice<'a> {
        target: usize,
        index: usize,
        result: Option<Result<image::RgbaImage>>,
        cancelled: &'a AtomicBool,
    }
    impl<'a> Device<'a> for ImageDevice<'_> {
        fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}
        fn push_clip_path(&mut self, _: &ClipPath) {}
        fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
        fn pop_clip(&mut self) {}
        fn pop_transparency_group(&mut self) {}
        fn draw_glyph_run(&mut self, _: &GlyphRun<'_, 'a>, _: DrawProps<'a>, _: &DrawMode) {}
        fn draw_image(&mut self, image: Image<'a, '_>, _: ImageDrawProps<'a>) {
            if self.index == self.target && !self.cancelled.load(Ordering::Relaxed) {
                self.result = Some(decode_image(image));
            }
            self.index += 1;
        }
    }
    if cancelled.load(Ordering::Relaxed) {
        bail!(RuntimeText::DocumentUnavailable);
    }
    let page = pdf
        .pages()
        .get(page)
        .ok_or_else(|| anyhow::anyhow!(RuntimeText::DocumentUnavailable))?;
    let cache = InterpreterCache::new();
    let (width, height) = page.render_dimensions();
    let mut context = interpret::Context::new(
        page.initial_transform(true).to_kurbo(),
        Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
        &cache,
        page.xref(),
        InterpreterSettings::default(),
    );
    let mut device = ImageDevice {
        target: image,
        index: 0,
        result: None,
        cancelled,
    };
    interpret::interpret_page(page, &mut context, &mut device);
    if cancelled.load(Ordering::Relaxed) {
        bail!(RuntimeText::DocumentUnavailable);
    }
    let image = device
        .result
        .ok_or_else(|| anyhow::anyhow!(RuntimeText::DocumentUnavailable))??;
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png)?;
    Ok(png.into_inner())
}

pub(super) fn raster_dimensions(width: f32, height: f32, scale: f32) -> Result<(u16, u16)> {
    let width = (width * scale).ceil();
    let height = (height * scale).ceil();
    if !scale.is_finite()
        || scale <= 0.0
        || !width.is_finite()
        || !height.is_finite()
        || width < 1.0
        || height < 1.0
        || width > u16::MAX as f32
        || height > u16::MAX as f32
    {
        bail!(RuntimeText::InvalidDimensions);
    }
    let (width, height) = (width as u16, height as u16);
    if usize::from(width) * usize::from(height) > MAX_PAGE_PIXELS {
        bail!(RuntimeText::PdfMemoryLimit);
    }
    Ok((width, height))
}

#[cfg(test)]
pub(super) fn raster(
    pdf: &Pdf,
    index: usize,
    scale: f32,
) -> Result<(image::RgbaImage, Vec<String>)> {
    let page = pdf
        .pages()
        .get(index)
        .ok_or_else(|| anyhow::anyhow!(RuntimeText::DocumentUnavailable))?;
    let (width, height) = page.render_dimensions();
    raster_dimensions(width, height, scale)?;
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let sink = warnings.clone();
    let settings = InterpreterSettings {
        warning_sink: Arc::new(move |warning| {
            sink.lock()
                .expect("PDF warning mutex poisoned")
                .push(format!("{warning:?}"));
        }),
        ..Default::default()
    };
    let pixmap = hayro::render(
        page,
        &hayro::RenderCache::new(),
        &settings,
        &hayro::RenderSettings::default(),
        &hayro::PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: hayro::vello_cpu::color::palette::css::WHITE,
        },
    );
    let image = image::RgbaImage::from_raw(
        u32::from(pixmap.width()),
        u32::from(pixmap.height()),
        pixmap.data_as_u8_slice().to_vec(),
    )
    .ok_or_else(|| anyhow::anyhow!(RuntimeText::InvalidOutput))?;
    let warnings = warnings.lock().expect("PDF warning mutex poisoned").clone();
    Ok((image, warnings))
}

pub(super) const TILE_SIZE: u32 = 1024;

pub(super) fn view_transform(width: f32, height: f32, turns: u8) -> Affine {
    let (width, height) = (f64::from(width), f64::from(height));
    match turns % 4 {
        1 => Affine::new([0.0, -1.0, 1.0, 0.0, 0.0, width]),
        2 => Affine::new([-1.0, 0.0, 0.0, -1.0, width, height]),
        3 => Affine::new([0.0, 1.0, -1.0, 0.0, height, 0.0]),
        _ => Affine::IDENTITY,
    }
}

#[cfg(test)]
pub(super) fn raster_tile<'a>(
    pdf: &'a Pdf,
    cache: &hayro::RenderCache<'a>,
    index: usize,
    scale: f32,
    column: u32,
    row: u32,
) -> Result<(image::RgbaImage, Vec<String>)> {
    raster_tile_rotated(pdf, cache, index, scale, column, row, 0)
}

pub(super) fn raster_tile_rotated<'a>(
    pdf: &'a Pdf,
    cache: &hayro::RenderCache<'a>,
    index: usize,
    scale: f32,
    column: u32,
    row: u32,
    turns: u8,
) -> Result<(image::RgbaImage, Vec<String>)> {
    let page = pdf
        .pages()
        .get(index)
        .ok_or_else(|| anyhow::anyhow!(RuntimeText::DocumentUnavailable))?;
    let (page_width, page_height) = page.render_dimensions();
    let rotation = view_transform(page_width, page_height, turns);
    let rotated = rotation.transform_rect_bbox(Rect::new(
        0.0,
        0.0,
        f64::from(page_width),
        f64::from(page_height),
    ));
    let width = (rotated.width() as f32 * scale).ceil();
    let height = (rotated.height() as f32 * scale).ceil();
    let x = f64::from(column) * f64::from(TILE_SIZE);
    let y = f64::from(row) * f64::from(TILE_SIZE);
    if !scale.is_finite()
        || scale <= 0.0
        || !width.is_finite()
        || !height.is_finite()
        || x >= f64::from(width)
        || y >= f64::from(height)
    {
        bail!(RuntimeText::InvalidDimensions);
    }
    let width = (f64::from(width) - x).min(f64::from(TILE_SIZE)) as u16;
    let height = (f64::from(height) - y).min(f64::from(TILE_SIZE)) as u16;
    raster_dimensions(f32::from(width), f32::from(height), 1.0)?;
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let sink = warnings.clone();
    let settings = InterpreterSettings {
        warning_sink: Arc::new(move |warning| {
            let mut warnings = sink.lock().expect("PDF warning mutex poisoned");
            if warnings.len() < 32 {
                warnings.push(format!("{warning:?}"));
            }
        }),
        ..Default::default()
    };
    let mut context = hayro::vello_cpu::RenderContext::new(width, height);
    hayro::render_into(
        page,
        cache,
        &settings,
        &hayro::RenderSettings::default(),
        &mut context,
        hayro::kurbo::Affine::translate((-x, -y))
            * hayro::kurbo::Affine::scale(f64::from(scale))
            * rotation
            * page.initial_transform(true).to_kurbo(),
    );
    context.flush();
    let mut pixmap = hayro::vello_cpu::Pixmap::new(width, height);
    context.render_with(
        &mut pixmap,
        &mut hayro::vello_cpu::Resources::default(),
        hayro::vello_cpu::RasterizerSettings {
            target_init: hayro::vello_cpu::TargetInit::Clear(
                hayro::vello_cpu::color::palette::css::WHITE,
            ),
            ..Default::default()
        },
    );
    let image = image::RgbaImage::from_raw(
        u32::from(width),
        u32::from(height),
        pixmap.data_as_u8_slice().to_vec(),
    )
    .ok_or_else(|| anyhow::anyhow!(RuntimeText::InvalidOutput))?;
    let warnings = warnings.lock().expect("PDF warning mutex poisoned").clone();
    Ok((image, warnings))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    #[test]
    fn pdf_classification_and_session_state_remain_read_only_and_compatible() {
        use super::super::{
            FileKind, PdfViewState, classify_file_kind_for_bytes, config_store::TabViewState,
        };
        for (path, bytes) in [
            ("document.PDF", &b"invalid PDF"[..]),
            ("unknown.bin", &b"%PDF-1.7\n"[..]),
        ] {
            assert_eq!(
                classify_file_kind_for_bytes(std::path::Path::new(path), bytes),
                FileKind::Pdf
            );
        }
        let legacy: TabViewState = serde_yaml::from_str("cursor_offset: 4").unwrap();
        assert_eq!(legacy.pdf, PdfViewState::default());
        let state = TabViewState {
            pdf: PdfViewState {
                page: 5,
                fraction: 0.4,
                zoom: 1.5,
                thumbnail_scroll: 90.0,
            },
            ..Default::default()
        };
        let encoded = serde_yaml::to_string(&state).unwrap();
        assert_eq!(
            serde_yaml::from_str::<TabViewState>(&encoded).unwrap(),
            state
        );
    }

    fn document(stream: &str, extra: &str) -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R {extra} >>"
            ),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            format!(
                "<< /Length {} >>\nstream\n{stream}\nendstream",
                stream.len()
            ),
        ];
        objects_pdf(&objects)
    }

    fn objects_pdf(objects: &[String]) -> Vec<u8> {
        let mut pdf = "%PDF-1.7\n".to_string();
        let mut offsets = vec![0];
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
        }
        let xref = pdf.len();
        pdf.push_str(&format!(
            "xref\n0 {}\n0000000000 65535 f \n",
            objects.len() + 1
        ));
        for offset in offsets.iter().skip(1) {
            pdf.push_str(&format!("{offset:010} 00000 n \n"));
        }
        pdf.push_str(&format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        ));
        pdf.into_bytes()
    }

    pub(crate) fn image_document(extra: &str) -> Vec<u8> {
        let content = "q 20 40 20 20 re W n 40 0 0 20 20 40 cm /Im1 Do Q\nq 30 30 -15 15 100 20 cm /Im1 Do Q\nq 20 0 0 20 150 40 cm BI /W 2 /H 1 /BPC 8 /CS /RGB /F /AHx ID ff000000ff00> EI Q\nq 1 0 0 rg 20 0 0 20 150 10 cm /Mask Do Q";
        objects_pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] {extra} /Resources << /XObject << /Im1 4 0 R /Mask 7 0 R >> >> /Contents 5 0 R >>"),
            "<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /ASCIIHexDecode /SMask 6 0 R /Length 13 >>\nstream\nff00000000ff>\nendstream".into(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
            "<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /ASCIIHexDecode /Length 5 >>\nstream\nff80>\nendstream".into(),
            "<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /ImageMask true /BitsPerComponent 1 /Filter /ASCIIHexDecode /Length 3 >>\nstream\n80>\nendstream".into(),
        ])
    }

    #[test]
    fn pdf_embedded_images_keep_original_pixels_masks_and_placement() {
        for extra in ["", "/Rotate 90", "/CropBox [10 10 180 90]"] {
            let pdf = parse(image_document(extra).into()).unwrap();
            let cancelled = AtomicBool::new(false);
            let pages = extract(&pdf, &cancelled).unwrap();
            assert_eq!(pages[0].images.len(), 4);
            for entry in &pages[0].images {
                assert!(entry.contains(entry.bounds.center()));
                assert!(!entry.contains(Point::new(-100.0, -100.0)));
            }
            let rotated = &pages[0].images[1];
            assert!(
                !rotated.contains(Point::new(rotated.bounds.x0 + 0.1, rotated.bounds.y0 + 0.1))
            );
            let decode = |index| {
                image::load_from_memory(&image_png(&pdf, 0, index, &cancelled).unwrap())
                    .unwrap()
                    .into_rgba8()
            };
            let first = decode(0);
            assert_eq!(first.dimensions(), (2, 1));
            assert_eq!(first.get_pixel(0, 0).0, [255, 0, 0, 255]);
            assert_eq!(first.get_pixel(1, 0).0, [0, 0, 255, 128]);
            assert_eq!(first, decode(1));
            let inline = decode(2);
            assert_eq!(inline.get_pixel(1, 0).0, [0, 255, 0, 255]);
            let stencil = decode(3);
            assert_eq!(stencil.get_pixel(0, 0).0, [255, 0, 0, 0]);
            assert_eq!(stencil.get_pixel(1, 0).0, [255, 0, 0, 255]);
            assert!(image_png(&pdf, 0, 99, &cancelled).is_err());
            cancelled.store(true, Ordering::Relaxed);
            assert!(image_png(&pdf, 0, 0, &cancelled).is_err());
        }
        assert!(checked_image_dimensions(u32::MAX, u32::MAX).is_err());
    }

    #[test]
    fn pdf_extract_and_render_share_rotated_crop_coordinates() {
        for extra in ["", "/Rotate 90", "/CropBox [10 10 180 90]"] {
            let pdf = parse(document("BT /F1 16 Tf 20 40 Td (Hello) Tj ET", extra).into()).unwrap();
            let pages = extract(&pdf, &AtomicBool::new(false)).unwrap();
            assert_eq!(pages[0].text, "Hello");
            let (image, warnings) = raster(&pdf, 0, 1.0).unwrap();
            assert!(warnings.is_empty(), "{warnings:?}");
            let bounds = pages[0].clusters[0].bounds;
            assert!(bounds.x0 >= 0.0 && bounds.y0 >= 0.0, "{bounds:?}");
            assert!(bounds.x1 <= f64::from(image.width()));
            assert!(bounds.y1 <= f64::from(image.height()));
            assert_eq!(pages[0].hit(bounds.center()), Some(0));
            let mut ink = false;
            for y in bounds.y0.floor() as u32..(bounds.y1.ceil() as u32).min(image.height()) {
                for x in bounds.x0.floor() as u32..(bounds.x1.ceil() as u32).min(image.width()) {
                    ink |= image.get_pixel(x, y)[0] < 128;
                }
            }
            assert!(ink, "Text geometry does not overlap rendered ink");
        }
    }

    #[test]
    fn pdf_actual_text_preserves_multicharacter_unicode_cluster() {
        let pdf = parse(
            document(
                "/Span << /ActualText <FEFF00660069> >> BDC BT /F1 16 Tf 20 40 Td (X) Tj ET EMC",
                "",
            )
            .into(),
        )
        .unwrap();
        let pages = extract(&pdf, &AtomicBool::new(false)).unwrap();
        assert_eq!(pages[0].text, "fi");
        assert_eq!(pages[0].clusters.len(), 1);
        assert_eq!(pages[0].clusters[0].range, 0..2);
        assert_eq!(decode_string(&[0xfe, 0xff, 0x4e, 0x2d]).unwrap(), "中");
    }

    #[test]
    fn pdf_rejects_unsafe_allocations_and_cancelled_extraction() {
        for dimensions in [
            (f32::NAN, 100.0, 1.0),
            (100.0, 100.0, -1.0),
            (65536.0, 1.0, 1.0),
            (8000.0, 8000.0, 1.0),
        ] {
            assert!(raster_dimensions(dimensions.0, dimensions.1, dimensions.2).is_err());
        }
        let bytes: Arc<[u8]> = document("", "").into();
        let original = bytes.as_ptr();
        let pdf = parse(bytes).unwrap();
        assert_eq!(pdf.data().as_ref().as_ptr(), original);
        assert!(extract(&pdf, &AtomicBool::new(true)).is_err());
        assert!(parse(Arc::from(&b"not a PDF"[..])).is_err());
    }

    #[test]
    fn pdf_links_resolve_names_fit_coordinates_and_safe_actions() {
        let pdf = parse(document("", "/Annots [<< /Subtype /Link /Rect [0 0 100 80] /QuadPoints [10 10 30 10 10 30 30 30] /Dest [3 0 R /FitH 75] >> << /Subtype /Link /Rect [0 0 5 5] /A << /S /URI /URI (HTTPS://example.org) >> >> << /Subtype /Link /Rect [0 0 5 5] /A << /S /Launch /URI (https://example.org) >> >> << /Subtype /Link /Rect [0 0 5 5] /A << /S /URI /URI (javascript:alert) >> >>]").into()).unwrap();
        let info = inspect(&pdf).unwrap();
        let links = &info.pages[0].links;
        assert_eq!(links.len(), 4);
        assert!(links[0].contains(Point::new(20.0, 80.0)));
        assert!(!links[0].contains(Point::new(90.0, 50.0)));
        assert!(
            matches!(
                links[0].target,
                LinkTarget::Fit {
                    page: 0,
                    point: Some(Point { y: 25.0, .. }),
                    width: true,
                    height: false,
                    ..
                }
            ),
            "{:?}",
            links[0].target
        );
        assert!(matches!(&links[1].target, LinkTarget::Url(url) if url == "HTTPS://example.org"));
        assert!(matches!(links[2].target, LinkTarget::Unsupported));
        assert!(matches!(links[3].target, LinkTarget::Unsupported));

        let pdf = parse(objects_pdf(&[
            "<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R /Names << /Dests << /Names [(chapter) << /D [3 0 R /XYZ 20 60 2] >>] >> >> >>".into(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>".into(),
            "<< /First 5 0 R >>".into(),
            "<< /Title <FEFF4E2D> /Dest (chapter) /First 6 0 R >>".into(),
            "<< /Title (Child) /Dest [3 0 R /Fit] >>".into(),
        ]).into()).unwrap();
        let info = inspect(&pdf).unwrap();
        assert_eq!(info.headings[0].title, "中");
        assert_eq!(info.headings[1].depth, 1);
        assert!(matches!(
            info.headings[0].target,
            LinkTarget::Page {
                page: 0,
                point: Some(Point { x: 20.0, y: 40.0 }),
                zoom: Some(2.0)
            }
        ));
    }

    #[test]
    fn pdf_bad_outline_preserves_readable_pages() {
        let pdf = parse(
            objects_pdf(&[
                "<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R >>".into(),
                "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>".into(),
                "<< /First 5 0 R >>".into(),
                "<< /Title (Cycle) /Next 5 0 R >>".into(),
            ])
            .into(),
        )
        .unwrap();
        let info = inspect(&pdf).unwrap();
        assert_eq!(info.pages.len(), 1);
        assert!(info.outline_error.is_some(), "{info:?}");
        assert_eq!(info.headings.len(), 1);
    }

    #[test]
    fn pdf_invisible_text_and_fill_stroke_are_not_lost_or_duplicated() {
        let pdf = parse(
            document(
                "BT /F1 16 Tf 20 60 Td 2 Tr (Visible) Tj 0 -25 Td 3 Tr (Hidden) Tj ET",
                "",
            )
            .into(),
        )
        .unwrap();
        let pages = extract(&pdf, &AtomicBool::new(false)).unwrap();
        assert_eq!(pages[0].text, "Visible\nHidden");
        assert_eq!(pages[0].clusters.len(), 13);
    }

    #[test]
    fn pdf_tiles_match_full_page_at_boundaries() {
        let pdf = parse(
            document(
                "0.2 0.4 0.7 rg 0 0 200 100 re f BT /F1 16 Tf 130 40 Td (Boundary) Tj ET",
                "",
            )
            .into(),
        )
        .unwrap();
        let (whole, _) = raster(&pdf, 0, 7.0).unwrap();
        let cache = hayro::RenderCache::new();
        for column in 0..2 {
            let (tile, _) = raster_tile(&pdf, &cache, 0, 7.0, column, 0).unwrap();
            for (x, y, pixel) in tile.enumerate_pixels() {
                let expected = whole.get_pixel(column * TILE_SIZE + x, y);
                assert!(
                    pixel
                        .0
                        .iter()
                        .zip(expected.0)
                        .all(|(a, b)| a.abs_diff(b) <= 2),
                    "tile {column}: {x}, {y}"
                );
            }
        }
        assert!(raster_tile(&pdf, &cache, 0, 1.0, 1, 0).is_err());
    }

    #[test]
    fn pdf_view_rotation_matches_pixels_across_tile_boundaries() {
        for extra in ["", "/Rotate 90 /CropBox [10 10 180 90]"] {
            let pdf = parse(image_document(extra).into()).unwrap();
            let (base, _) = raster(&pdf, 0, 7.0).unwrap();
            let cache = hayro::RenderCache::new();
            for turns in 0..4 {
                let expected = match turns {
                    1 => image::imageops::rotate270(&base),
                    2 => image::imageops::rotate180(&base),
                    3 => image::imageops::rotate90(&base),
                    _ => base.clone(),
                };
                for row in 0..expected.height().div_ceil(TILE_SIZE) {
                    for column in 0..expected.width().div_ceil(TILE_SIZE) {
                        let (tile, _) =
                            raster_tile_rotated(&pdf, &cache, 0, 7.0, column, row, turns).unwrap();
                        let mut differences = 0;
                        for (x, y, pixel) in tile.enumerate_pixels() {
                            let target =
                                expected.get_pixel(column * TILE_SIZE + x, row * TILE_SIZE + y);
                            let difference = pixel
                                .0
                                .iter()
                                .zip(target.0)
                                .map(|(a, b)| a.abs_diff(b))
                                .max()
                                .unwrap();
                            differences += usize::from(difference > 2);
                            if difference > 2 {
                                // Nearest-neighbor stencil sampling may choose either
                                // side of a pixel boundary after a quarter-turn.
                                let px = column * TILE_SIZE + x;
                                let py = row * TILE_SIZE + y;
                                assert!(
                                    (py.saturating_sub(1)..=(py + 1).min(expected.height() - 1))
                                        .any(|ny| {
                                            (px.saturating_sub(1)
                                                ..=(px + 1).min(expected.width() - 1))
                                                .any(|nx| {
                                                    pixel
                                                        .0
                                                        .iter()
                                                        .zip(expected.get_pixel(nx, ny).0)
                                                        .all(|(a, b)| a.abs_diff(b) <= 2)
                                                })
                                        }),
                                    "rotation {turns}: interior pixel {px},{py}"
                                );
                            }
                        }
                        assert!(
                            differences * 10_000 <= (tile.width() * tile.height()) as usize,
                            "rotation {turns}: {differences} edge differences"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn pdf_type3_text_geometry_matches_interpreted_glyph() {
        let stream = "BT /F1 20 Tf 20 40 Td (A) Tj ET";
        let glyph = "600 0 d0 0 0 500 700 re f";
        let pdf = parse(objects_pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".into(),
            "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 500 700] /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << /A 6 0 R >> /Encoding << /Type /Encoding /Differences [65 /A] >> /FirstChar 65 /LastChar 65 /Widths [600] /Resources << >> >>".into(),
            format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
            format!("<< /Length {} >>\nstream\n{glyph}\nendstream", glyph.len()),
        ]).into()).unwrap();
        let pages = extract(&pdf, &AtomicBool::new(false)).unwrap();
        assert_eq!(pages[0].text, "A");
        let bounds = pages[0].clusters[0].bounds;
        assert!(
            (bounds.x0 - 20.0).abs() < 0.01 && (bounds.y0 - 46.0).abs() < 0.01,
            "{bounds:?}"
        );
        assert!(
            (bounds.width() - 10.0).abs() < 0.01 && (bounds.height() - 14.0).abs() < 0.01,
            "{bounds:?}"
        );
    }

    #[test]
    fn pdf_vertical_text_keeps_one_column_in_reading_order() {
        let stream = "BT /F1 16 Tf 40 80 Td <00010002> Tj ET";
        let cmap = "/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> def /CMapName /Test def /CMapType 2 def 1 begincodespacerange <0000> <FFFF> endcodespacerange 2 beginbfchar <0001> <0041> <0002> <0042> endbfchar endcmap CMapName currentdict /CMap defineresource pop end end";
        let pdf = parse(objects_pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".into(),
            "<< /Type /Font /Subtype /Type0 /BaseFont /Helvetica /Encoding /Identity-V /DescendantFonts [6 0 R] /ToUnicode 7 0 R >>".into(),
            format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Helvetica /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor 8 0 R /DW 1000 >>".into(),
            format!("<< /Length {} >>\nstream\n{cmap}\nendstream", cmap.len()),
            "<< /Type /FontDescriptor /FontName /Helvetica /Flags 4 /FontBBox [0 -200 1000 800] /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 >>".into(),
        ]).into()).unwrap();
        let pages = extract(&pdf, &AtomicBool::new(false)).unwrap();
        assert_eq!(pages[0].text, "AB");
        assert_eq!(pages[0].clusters.len(), 2);
        assert!(pages[0].clusters[1].bounds.y0 > pages[0].clusters[0].bounds.y0);
    }

    #[test]
    fn pdf_highlights_join_glyphs_and_spaces_without_joining_text_rows() {
        let text = PageText {
            text: "a 中b\ncd".into(),
            clusters: vec![
                TextCluster {
                    range: 0..1,
                    bounds: Rect::new(0.0, 2.0, 5.0, 10.0),
                },
                TextCluster {
                    range: 2..5,
                    bounds: Rect::new(12.0, 0.0, 22.0, 12.0),
                },
                TextCluster {
                    range: 5..6,
                    bounds: Rect::new(25.0, 3.0, 30.0, 10.0),
                },
                TextCluster {
                    range: 7..8,
                    bounds: Rect::new(0.0, 20.0, 5.0, 30.0),
                },
                TextCluster {
                    range: 8..9,
                    bounds: Rect::new(7.0, 21.0, 12.0, 30.0),
                },
            ],
            unmapped: 0,
            images: Vec::new(),
        };
        assert_eq!(
            text.highlight_rects(&(0..9)),
            vec![
                Rect::new(0.0, 0.0, 30.0, 12.0),
                Rect::new(0.0, 20.0, 12.0, 30.0)
            ]
        );
        assert_eq!(
            text.highlight_rects(&(2..6)),
            vec![Rect::new(12.0, 0.0, 30.0, 12.0)]
        );
        assert!(text.highlight_rects(&(3..3)).is_empty());
        assert_eq!(
            text.nearest_hit(Point::new(-30.0, 5.0)).unwrap().0.range,
            0..1
        );
        assert_eq!(
            text.nearest_hit(Point::new(9.0, 5.0)).unwrap().0.range,
            2..5
        );
        assert_eq!(
            text.nearest_hit(Point::new(11.0, 18.0)).unwrap().0.range,
            8..9
        );
        assert!(PageText::default().nearest_hit(Point::ZERO).is_none());

        let punctuation = PageText {
            text: ".'".into(),
            clusters: vec![
                TextCluster {
                    range: 0..1,
                    bounds: Rect::new(0.0, 8.0, 3.0, 10.0),
                },
                TextCluster {
                    range: 1..2,
                    bounds: Rect::new(5.0, 0.0, 7.0, 2.0),
                },
            ],
            unmapped: 0,
            images: Vec::new(),
        };
        assert_eq!(
            punctuation.highlight_rects(&(0..2)),
            vec![Rect::new(0.0, 0.0, 7.0, 10.0)]
        );

        let vertical = PageText {
            text: "ab\nc".into(),
            clusters: vec![
                TextCluster {
                    range: 0..1,
                    bounds: Rect::new(20.0, 0.0, 30.0, 10.0),
                },
                TextCluster {
                    range: 1..2,
                    bounds: Rect::new(22.0, 15.0, 28.0, 25.0),
                },
                TextCluster {
                    range: 3..4,
                    bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
                },
            ],
            unmapped: 0,
            images: Vec::new(),
        };
        assert_eq!(
            vertical.highlight_rects(&(0..4)),
            vec![
                Rect::new(20.0, 0.0, 30.0, 25.0),
                Rect::new(0.0, 0.0, 10.0, 10.0)
            ]
        );
    }
}
