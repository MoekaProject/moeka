//! Data model, serialization, and per-tab state for the `.moeka` whiteboard
//! (an Excalidraw-style vector canvas rendered with GPUI primitives).
//!
//! A `.moeka` file is a small JSON document describing vector elements
//! (rectangles, ellipses, arrows, lines, text). Colors are stored as semantic
//! palette tokens that resolve against the active editor theme, so drawings
//! recolor when the user switches themes.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::Hsla;
use serde::{Deserialize, Serialize};

use crate::theme::Theme;

/// File extension (without the dot) for whiteboard documents.
pub(crate) const WHITEBOARD_EXTENSION: &str = "moeka";

/// Current on-disk format version.
const WHITEBOARD_FORMAT_VERSION: u32 = 1;

fn default_version() -> u32 {
    WHITEBOARD_FORMAT_VERSION
}

fn default_zoom() -> f32 {
    1.0
}

fn default_opacity() -> f32 {
    1.0
}

fn default_route_mid() -> f32 {
    0.5
}

// ── Enums ───────────────────────────────────────────────────────────────────

/// The kind of a drawable element.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ElementKind {
    Rectangle,
    Ellipse,
    Arrow,
    Line,
    Text,
}

impl ElementKind {
    /// Elements that are described by two endpoints (start → end) rather than a
    /// filled box.
    pub(crate) fn is_linear(self) -> bool {
        matches!(self, ElementKind::Arrow | ElementKind::Line)
    }

    /// Whether this element kind can carry a centered text label.
    pub(crate) fn supports_label(self) -> bool {
        matches!(
            self,
            ElementKind::Rectangle | ElementKind::Ellipse | ElementKind::Arrow | ElementKind::Line
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum StrokeWidth {
    Small,
    Medium,
    Large,
}

impl StrokeWidth {
    /// Stroke thickness in canvas units (multiplied by zoom when painted).
    pub(crate) fn px(self) -> f32 {
        match self {
            StrokeWidth::Small => 1.5,
            StrokeWidth::Medium => 2.5,
            StrokeWidth::Large => 4.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum StrokeStyle {
    Solid,
    Dashed,
    Dotted,
}

/// How "hand-drawn" an element's outline looks. `Clean` draws crisp straight
/// edges; `Sketch` and `Draft` perturb the outline (and overdraw it) to imitate
/// a hand-drawn look, with `Draft` being the roughest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum StrokeRoughness {
    #[default]
    Clean,
    Sketch,
    Draft,
}

impl StrokeRoughness {
    /// Perturbation amplitude in canvas units (0 for a clean straight outline).
    pub(crate) fn amplitude(self) -> f32 {
        match self {
            StrokeRoughness::Clean => 0.0,
            StrokeRoughness::Sketch => 1.0,
            StrokeRoughness::Draft => 2.2,
        }
    }

    /// Number of overlapping strokes drawn for the outline (2 for the layered
    /// hand-drawn look).
    pub(crate) fn passes(self) -> usize {
        match self {
            StrokeRoughness::Clean => 1,
            StrokeRoughness::Sketch | StrokeRoughness::Draft => 2,
        }
    }

    /// Relative stroke-width wobble (0..1) for the hand-drawn styles.
    pub(crate) fn width_variation(self) -> f32 {
        match self {
            StrokeRoughness::Clean => 0.0,
            StrokeRoughness::Sketch => 0.35,
            StrokeRoughness::Draft => 0.55,
        }
    }

    /// Opacity of the second overdraw pass relative to the base stroke.
    pub(crate) fn overdraw_alpha(self) -> f32 {
        match self {
            StrokeRoughness::Clean => 0.0,
            StrokeRoughness::Sketch => 0.45,
            StrokeRoughness::Draft => 0.6,
        }
    }
}

/// How a linear element (arrow/line) is routed between its endpoints.
/// `Straight` draws a direct segment; `Orthogonal` routes with horizontal and
/// vertical segments (an elbow connector) whose bend position is adjustable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RouteStyle {
    #[default]
    Straight,
    Orthogonal,
    Curved,
}

/// Rectangle corner rounding. Stored per element; `None` (sharp) is the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CornerRadius {
    #[default]
    None,
    Small,
    Medium,
    Large,
}

impl CornerRadius {
    /// Corner radius in canvas units.
    pub(crate) fn px(self) -> f32 {
        match self {
            CornerRadius::None => 0.0,
            CornerRadius::Small => 8.0,
            CornerRadius::Medium => 16.0,
            CornerRadius::Large => 28.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum FontSize {
    Small,
    Medium,
    Large,
}

impl FontSize {
    /// Font size in canvas units (multiplied by zoom when painted).
    pub(crate) fn px(self) -> f32 {
        match self {
            FontSize::Small => 16.0,
            FontSize::Medium => 22.0,
            FontSize::Large => 32.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CanvasTextAlign {
    Left,
    Center,
    Right,
}

/// A palette token resolving to a fixed color (consistent across themes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PaletteColor {
    /// No fill (valid for backgrounds only).
    Transparent,
    /// Near-black ink (legacy files stored this as "ink").
    #[serde(alias = "ink")]
    Black,
    White,
    Red,
    Orange,
    Yellow,
    Green,
    Cyan,
    Blue,
    Purple,
}

impl PaletteColor {
    /// Palette tokens offered as stroke colors, in display order.
    pub(crate) const STROKE_SWATCHES: [PaletteColor; 10] = [
        PaletteColor::Transparent,
        PaletteColor::Black,
        PaletteColor::White,
        PaletteColor::Red,
        PaletteColor::Orange,
        PaletteColor::Yellow,
        PaletteColor::Green,
        PaletteColor::Cyan,
        PaletteColor::Blue,
        PaletteColor::Purple,
    ];

    /// Palette tokens offered as font colors (no transparent).
    pub(crate) const FONT_SWATCHES: [PaletteColor; 9] = [
        PaletteColor::Black,
        PaletteColor::White,
        PaletteColor::Red,
        PaletteColor::Orange,
        PaletteColor::Yellow,
        PaletteColor::Green,
        PaletteColor::Cyan,
        PaletteColor::Blue,
        PaletteColor::Purple,
    ];

    /// Palette tokens offered as background colors (includes transparent).
    pub(crate) const BACKGROUND_SWATCHES: [PaletteColor; 10] = [
        PaletteColor::Transparent,
        PaletteColor::Black,
        PaletteColor::White,
        PaletteColor::Red,
        PaletteColor::Orange,
        PaletteColor::Yellow,
        PaletteColor::Green,
        PaletteColor::Cyan,
        PaletteColor::Blue,
        PaletteColor::Purple,
    ];

    /// Resolve this token to a concrete color. Returns `None` for
    /// `Transparent`. The theme is no longer consulted (fixed palette), but the
    /// parameter is kept for call-site stability.
    pub(crate) fn resolve(self, _theme: &Theme) -> Option<Hsla> {
        match self {
            PaletteColor::Transparent => None,
            PaletteColor::Black => Some(crate::hex_hsla!("#1e1e1e")),
            PaletteColor::White => Some(crate::hex_hsla!("#f5f5f5")),
            PaletteColor::Red => Some(crate::hex_hsla!("#e03131")),
            PaletteColor::Orange => Some(crate::hex_hsla!("#f08c00")),
            PaletteColor::Yellow => Some(crate::hex_hsla!("#f5c211")),
            PaletteColor::Green => Some(crate::hex_hsla!("#2f9e44")),
            PaletteColor::Cyan => Some(crate::hex_hsla!("#0fa3b1")),
            PaletteColor::Blue => Some(crate::hex_hsla!("#1971c2")),
            PaletteColor::Purple => Some(crate::hex_hsla!("#9c36b5")),
        }
    }

    /// Resolve as a stroke color, falling back to the theme ink when
    /// transparent.
    pub(crate) fn resolve_stroke(self, theme: &Theme) -> Hsla {
        self.resolve(theme).unwrap_or(theme.text_primary)
    }
}

/// Active drawing tool. Persisted per tab but not written to disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WhiteboardTool {
    Select,
    Rectangle,
    Ellipse,
    Arrow,
    Line,
    Text,
}

impl WhiteboardTool {
    /// The element kind a shape tool creates, if any.
    pub(crate) fn element_kind(self) -> Option<ElementKind> {
        match self {
            WhiteboardTool::Select => None,
            WhiteboardTool::Rectangle => Some(ElementKind::Rectangle),
            WhiteboardTool::Ellipse => Some(ElementKind::Ellipse),
            WhiteboardTool::Arrow => Some(ElementKind::Arrow),
            WhiteboardTool::Line => Some(ElementKind::Line),
            WhiteboardTool::Text => Some(ElementKind::Text),
        }
    }
}

// ── Style sub-structs ───────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct StrokeSpec {
    pub color: PaletteColor,
    pub width: StrokeWidth,
    pub style: StrokeStyle,
    #[serde(default)]
    pub roughness: StrokeRoughness,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct BackgroundSpec {
    pub color: PaletteColor,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub fill: FillStyle,
    /// Fill color saturation factor (0..=1); 1 is the palette color itself.
    #[serde(default = "default_opacity")]
    pub saturation: f32,
}

/// Scale a color's saturation channel (used for the fill-saturation levels).
pub(crate) fn with_saturation(color: gpui::Hsla, factor: f32) -> gpui::Hsla {
    gpui::Hsla {
        s: color.s * factor.clamp(0.0, 1.0),
        ..color
    }
}

/// How a shape's background is painted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum FillStyle {
    /// Solid fill.
    #[default]
    Solid,
    /// Single-direction diagonal hatching.
    Hatch,
    /// Cross-hatched diagonals.
    #[serde(rename = "crosshatch")]
    CrossHatch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct FontSpec {
    pub size: FontSize,
    pub align: CanvasTextAlign,
    /// Text color; `None` falls back to the stroke color (legacy behavior).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<PaletteColor>,
    /// Font family name; `None` uses the editor's body font.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
}

impl FontSpec {
    /// The effective text color token, falling back to the stroke color.
    pub(crate) fn effective_color(&self, stroke: PaletteColor) -> PaletteColor {
        self.color.unwrap_or(stroke)
    }
}

/// The full visual style applied to an element (and used as the default style
/// for newly drawn shapes).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct WhiteboardStyle {
    pub stroke: StrokeSpec,
    pub background: BackgroundSpec,
    pub font: FontSpec,
    #[serde(default)]
    pub corner_radius: CornerRadius,
    #[serde(default)]
    pub route: RouteStyle,
}

impl Default for WhiteboardStyle {
    fn default() -> Self {
        Self {
            stroke: StrokeSpec {
                color: PaletteColor::Black,
                width: StrokeWidth::Medium,
                style: StrokeStyle::Solid,
                roughness: StrokeRoughness::Clean,
            },
            background: BackgroundSpec {
                color: PaletteColor::Transparent,
                opacity: 1.0,
                fill: FillStyle::Solid,
                saturation: 1.0,
            },
            font: FontSpec {
                size: FontSize::Medium,
                align: CanvasTextAlign::Left,
                color: None,
                family: None,
            },
            corner_radius: CornerRadius::None,
            route: RouteStyle::Straight,
        }
    }
}

// ── Element ─────────────────────────────────────────────────────────────────

/// A single drawable element.
///
/// All elements use a `(x, y, width, height)` box model in canvas coordinates.
/// For linear elements (arrow/line) the visible segment runs from `(x, y)` to
/// `(x + width, y + height)`, so `width`/`height` may be negative.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct WhiteboardElement {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: ElementKind,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Rotation in radians, clockwise, around the element center.
    #[serde(default)]
    pub rotation: f32,
    pub stroke: StrokeSpec,
    pub background: BackgroundSpec,
    pub font: FontSpec,
    /// Corner rounding for rectangles (ignored by other kinds).
    #[serde(default)]
    pub corner_radius: CornerRadius,
    /// Routing mode for linear elements (arrow/line).
    #[serde(default)]
    pub route: RouteStyle,
    /// Bend position (0..1) along the dominant axis for orthogonal routing.
    #[serde(default = "default_route_mid")]
    pub route_mid: f32,
    /// Second bend position for orthogonal routing; `None` mirrors
    /// `route_mid` (single-bend legacy shape).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_mid2: Option<f32>,
    /// Position (0..1, may overshoot) of the orthogonal cross segment along
    /// the non-dominant axis.
    #[serde(default = "default_route_mid")]
    pub route_cross: f32,
    /// Signed perpendicular offset (canvas units) of the curve apex for curved
    /// routing.
    #[serde(default)]
    pub route_curve: f32,
    /// Signed offset of the curve apex along the chord direction (canvas
    /// units), letting the waypoint be dragged freely in 2D.
    #[serde(default)]
    pub route_curve_t: f32,
    /// Group membership stack: innermost group first, outermost last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub group_ids: Vec<String>,
    /// Element id this linear element's start endpoint is magnetically bound to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_binding: Option<String>,
    /// Element id this linear element's end endpoint is magnetically bound to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_binding: Option<String>,
    /// Text content for `Text` elements, or a centered label for shapes.
    #[serde(default)]
    pub text: String,
    /// Reserved for future multi-point paths; unused in v1 (always `None`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<[f32; 2]>>,
}

impl WhiteboardElement {
    /// Create a new element of `kind` with the given box and style.
    pub(crate) fn new(
        kind: ElementKind,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        style: WhiteboardStyle,
    ) -> Self {
        Self {
            id: new_element_id(),
            kind,
            x,
            y,
            width,
            height,
            rotation: 0.0,
            stroke: style.stroke,
            background: style.background,
            font: style.font,
            corner_radius: style.corner_radius,
            route: style.route,
            route_mid: 0.5,
            route_mid2: None,
            route_cross: 0.5,
            route_curve: 0.0,
            route_curve_t: 0.0,
            group_ids: Vec::new(),
            start_binding: None,
            end_binding: None,
            text: String::new(),
            points: None,
        }
    }

    /// Center of the element's box in canvas coordinates.
    pub(crate) fn center(&self) -> (f32, f32) {
        (self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    /// The outermost group this element belongs to, if any.
    pub(crate) fn outermost_group(&self) -> Option<&str> {
        self.group_ids.last().map(String::as_str)
    }

    /// The style bundle for this element.
    pub(crate) fn style(&self) -> WhiteboardStyle {
        WhiteboardStyle {
            stroke: self.stroke,
            background: self.background,
            font: self.font.clone(),
            corner_radius: self.corner_radius,
            route: self.route,
        }
    }
}

// ── Document ────────────────────────────────────────────────────────────────

/// Canvas viewport (pan + zoom) and canvas-wide appearance, persisted with the
/// document.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub(crate) struct WhiteboardAppState {
    #[serde(rename = "scrollX", default)]
    pub scroll_x: f32,
    #[serde(rename = "scrollY", default)]
    pub scroll_y: f32,
    #[serde(default = "default_zoom")]
    pub zoom: f32,
    /// Canvas background tint painted over the theme background.
    #[serde(default = "default_canvas_background")]
    pub background: PaletteColor,
    #[serde(default = "default_opacity")]
    pub background_opacity: f32,
}

fn default_canvas_background() -> PaletteColor {
    PaletteColor::Transparent
}

impl Default for WhiteboardAppState {
    fn default() -> Self {
        Self {
            scroll_x: 0.0,
            scroll_y: 0.0,
            zoom: 1.0,
            background: PaletteColor::Transparent,
            background_opacity: 1.0,
        }
    }
}

/// A complete whiteboard document, matching the `.moeka` JSON layout.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct WhiteboardDocument {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub elements: Vec<WhiteboardElement>,
    #[serde(rename = "appState", default)]
    pub app_state: WhiteboardAppState,
}

impl Default for WhiteboardDocument {
    fn default() -> Self {
        Self {
            version: WHITEBOARD_FORMAT_VERSION,
            elements: Vec::new(),
            app_state: WhiteboardAppState::default(),
        }
    }
}

impl WhiteboardDocument {
    /// Parse a document from raw file bytes. Empty/whitespace input yields an
    /// empty document; malformed JSON also yields an empty document rather than
    /// failing the tab open.
    pub(crate) fn from_json_bytes(bytes: &[u8]) -> Self {
        if bytes.iter().all(|byte| byte.is_ascii_whitespace()) {
            return Self::default();
        }
        serde_json::from_slice(bytes).unwrap_or_default()
    }

    /// Serialize to pretty JSON for saving to disk.
    pub(crate) fn to_json_string(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    pub(crate) fn element_index(&self, id: &str) -> Option<usize> {
        self.elements.iter().position(|element| element.id == id)
    }

    pub(crate) fn element(&self, id: &str) -> Option<&WhiteboardElement> {
        self.elements.iter().find(|element| element.id == id)
    }
}

// ── Per-tab state ───────────────────────────────────────────────────────────

/// Live, in-memory state for a whiteboard tab. The document round-trips to
/// disk; selection, tool, and default style are transient session state.
#[derive(Clone, Debug)]
pub(crate) struct WhiteboardTabState {
    pub document: WhiteboardDocument,
    pub selected_ids: Vec<String>,
    pub tool: WhiteboardTool,
    pub default_style: WhiteboardStyle,
    pub undo_stack: Vec<WhiteboardDocument>,
    pub redo_stack: Vec<WhiteboardDocument>,
}

impl WhiteboardTabState {
    pub(crate) fn new(document: WhiteboardDocument) -> Self {
        Self {
            document,
            selected_ids: Vec::new(),
            tool: WhiteboardTool::Select,
            default_style: WhiteboardStyle::default(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Load a whiteboard tab from a file path, tolerating missing/empty/invalid
    /// files by falling back to an empty document.
    pub(crate) fn load_from_path(path: &Path) -> Self {
        let document = std::fs::read(path)
            .map(|bytes| WhiteboardDocument::from_json_bytes(&bytes))
            .unwrap_or_default();
        Self::new(document)
    }

    pub(crate) fn selected_elements(&self) -> impl Iterator<Item = &WhiteboardElement> {
        self.document
            .elements
            .iter()
            .filter(|element| self.selected_ids.iter().any(|id| id == &element.id))
    }

    pub(crate) fn is_selected(&self, id: &str) -> bool {
        self.selected_ids.iter().any(|selected| selected == id)
    }
}

// ── Interaction state ───────────────────────────────────────────────────────

/// Which bounding-box handle is being manipulated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResizeHandle {
    TopLeft,
    TopRight,
    BottomRight,
    BottomLeft,
    Top,
    Right,
    Bottom,
    Left,
}

impl ResizeHandle {
    /// All eight handles in corner-then-edge order (matches
    /// `whiteboard_geometry::resize_handle_points`).
    pub(crate) const ALL: [ResizeHandle; 8] = [
        ResizeHandle::TopLeft,
        ResizeHandle::TopRight,
        ResizeHandle::BottomRight,
        ResizeHandle::BottomLeft,
        ResizeHandle::Top,
        ResizeHandle::Right,
        ResizeHandle::Bottom,
        ResizeHandle::Left,
    ];

    /// Whether this is one of the four corner handles.
    pub(crate) fn is_corner(self) -> bool {
        matches!(
            self,
            ResizeHandle::TopLeft
                | ResizeHandle::TopRight
                | ResizeHandle::BottomRight
                | ResizeHandle::BottomLeft
        )
    }
}

/// Which adjustable handle of a routed linear element is being dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteHandle {
    /// Bend on the start side (orthogonal).
    SideStart,
    /// The cross segment (orthogonal) or the curve apex (curved).
    Cross,
    /// Bend on the end side (orthogonal).
    SideEnd,
}

/// An in-progress pointer interaction on the canvas. Stored transiently on the
/// editor while a drag is active.
#[derive(Clone, Debug)]
pub(crate) enum WhiteboardDrag {
    /// Drawing a new shape; the element is already inserted and the drag sets
    /// its size. `start` is the anchor point in canvas coordinates.
    Create { id: String, start: (f32, f32) },
    /// Moving the current selection. `start` is the pointer position at drag
    /// start (canvas space); `origins` the dragged elements' original x/y so
    /// positions snap to the grid as absolute values; `moved` tracks whether a
    /// real drag happened (vs. a click).
    Move {
        start: (f32, f32),
        origins: Vec<(String, f32, f32)>,
        moved: bool,
    },
    /// Resizing a single element via a bounding-box handle. `anchor` is the
    /// fixed opposite point (canvas space), `rotation` the element's angle, and
    /// `aspect` the original width/height ratio (for Shift-constrained resize).
    Resize {
        id: String,
        handle: ResizeHandle,
        anchor: (f32, f32),
        rotation: f32,
        aspect: f32,
    },
    /// Dragging one endpoint of a linear element (`end` = the second endpoint).
    MoveEndpoint { id: String, end: bool },
    Rotate {
        id: String,
        center: (f32, f32),
        start_pointer: f32,
        start_rotation: f32,
    },
    /// Rubber-band selection rectangle (canvas space).
    Marquee {
        start: (f32, f32),
        current: (f32, f32),
    },
    /// Dragging the mid waypoint of an orthogonal linear element to adjust the
    /// bend position (`route_mid`).
    RouteWaypoint { id: String, handle: RouteHandle },
    /// Resizing the whole multi-selection via its combined bounding box.
    /// `anchor` is the fixed opposite point, `handle_origin` the handle's
    /// original position (both canvas space); `originals` capture each
    /// element's starting box.
    GroupResize {
        handle: ResizeHandle,
        anchor: (f32, f32),
        handle_origin: (f32, f32),
        originals: Vec<(String, f32, f32, f32, f32)>,
    },
    /// Rotating the whole multi-selection around its bbox center. `originals`
    /// capture each element's starting box and rotation.
    GroupRotate {
        center: (f32, f32),
        start_pointer: f32,
        originals: Vec<(String, f32, f32, f32, f32, f32)>,
    },
    /// Panning the canvas (Space + drag). `start_pointer` is the screen
    /// position at drag start; `start_scroll` the document scroll then.
    Pan {
        start_pointer: (f32, f32),
        start_scroll: (f32, f32),
    },
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Generate a process-unique element id without pulling in a UUID dependency.
pub(crate) fn new_element_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("el-{nanos:x}-{seq:x}")
}

/// Whether `path` is a whiteboard document by extension.
pub(crate) fn is_whiteboard_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(WHITEBOARD_EXTENSION))
}

const CLIPBOARD_KIND: &str = "moeka/elements";

#[derive(Serialize, Deserialize)]
struct ClipboardPayload {
    kind: String,
    elements: Vec<WhiteboardElement>,
}

/// Serialize selected elements for the system clipboard.
pub(crate) fn serialize_elements_for_clipboard(elements: &[WhiteboardElement]) -> String {
    let payload = ClipboardPayload {
        kind: CLIPBOARD_KIND.to_string(),
        elements: elements.to_vec(),
    };
    serde_json::to_string(&payload).unwrap_or_default()
}

/// Parse whiteboard elements previously copied to the clipboard, if the text is
/// a whiteboard clipboard payload.
pub(crate) fn parse_clipboard_elements(text: &str) -> Option<Vec<WhiteboardElement>> {
    let payload: ClipboardPayload = serde_json::from_str(text).ok()?;
    (payload.kind == CLIPBOARD_KIND).then_some(payload.elements)
}
