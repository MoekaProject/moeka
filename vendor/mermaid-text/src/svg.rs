use std::collections::HashMap;

use unicode_width::UnicodeWidthStr;

use crate::detect::{self, DiagramKind};
use crate::layout::layered::{LayoutBackend, LayoutConfig, LayoutResult};
use crate::layout::subgraph::{SG_BORDER_PAD, SubgraphBounds, compute_subgraph_bounds};
use crate::types::{
    BarOrientation, Direction, Edge, EdgeEndpoint, EdgeStyle, Graph, Node, NodeShape, Rgb,
};
use crate::{Error, layout, parser};

const CELL_WIDTH: f32 = 15.0;
const CELL_HEIGHT: f32 = 27.0;
const CANVAS_PADDING: f32 = 34.0;
const NODE_LABEL_PADDING_X: f32 = 20.0;
const NODE_FONT_SIZE: f32 = 15.5;
const NODE_LINE_HEIGHT: f32 = 20.0;
const NODE_CORNER_RADIUS: f32 = 9.0;
const NODE_STROKE_WIDTH: f32 = 2.0;
const EDGE_STROKE_WIDTH: f32 = 2.2;
const EDGE_LABEL_FONT_SIZE: f32 = 13.0;
const EDGE_LABEL_HEIGHT: f32 = 23.0;
const NODE_SHADOW: &str = " filter=\"url(#node-shadow)\"";

#[derive(Debug, Clone)]
pub struct SvgRenderOptions<'a> {
    pub font_family: &'a str,
    pub text_color: &'a str,
    pub muted_text_color: &'a str,
    pub edge_label_color: &'a str,
    pub node_fill: &'a str,
    pub node_stroke: &'a str,
    pub edge_stroke: &'a str,
    pub subgraph_fill: &'a str,
    pub subgraph_stroke: &'a str,
}

impl Default for SvgRenderOptions<'_> {
    fn default() -> Self {
        Self {
            font_family: "ui-sans-serif, sans-serif",
            text_color: "#111827",
            muted_text_color: "#6b7280",
            edge_label_color: "#111827",
            node_fill: "#ffffff",
            node_stroke: "#9ca3af",
            edge_stroke: "#6b7280",
            subgraph_fill: "#f9fafb",
            subgraph_stroke: "#d1d5db",
        }
    }
}

#[derive(Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Clone, Copy)]
struct Point {
    x: f32,
    y: f32,
}

impl Rect {
    fn center(self) -> Point {
        Point {
            x: self.x + self.width / 2.0,
            y: self.y + self.height / 2.0,
        }
    }
}

pub fn render_svg(input: &str) -> Result<String, Error> {
    render_svg_with_options(input, &SvgRenderOptions::default())
}

pub fn render_svg_with_options(
    input: &str,
    options: &SvgRenderOptions<'_>,
) -> Result<String, Error> {
    let graph = match detect::detect(input)? {
        DiagramKind::Flowchart => parser::parse(input)?,
        DiagramKind::State => parser::state::parse(input)?,
        other => return Err(Error::UnsupportedDiagram(format!("{other:?} SVG"))),
    };

    Ok(render_graph_svg(&graph, options))
}

fn render_graph_svg(graph: &Graph, options: &SvgRenderOptions<'_>) -> String {
    if graph.nodes.is_empty() {
        return empty_svg(options);
    }

    let mut positions = layout_graph(graph);
    if !graph.subgraphs.is_empty() {
        let (col_offset, row_offset) = subgraph_position_offset(graph, &positions);
        if col_offset != 0 || row_offset != 0 {
            for (col, row) in positions.values_mut() {
                *col += col_offset;
                *row += row_offset;
            }
        }
    }

    let subgraph_bounds = compute_subgraph_bounds(graph, &positions);
    let node_rects = build_node_rects(graph, &positions);
    let (width, height) = canvas_size(graph, &node_rects, &subgraph_bounds);

    let mut body = String::new();
    body.push_str(&marker_defs(options));

    for bounds in &subgraph_bounds {
        body.push_str(&render_subgraph(bounds, graph, options));
    }

    for (index, edge) in graph.edges.iter().enumerate() {
        body.push_str(&render_edge(index, edge, graph, &node_rects, options));
    }

    for node in &graph.nodes {
        if let Some(rect) = node_rects.get(&node.id) {
            body.push_str(&render_node(node, *rect, graph, options));
        }
    }

    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width:.0}\" height=\"{height:.0}\" viewBox=\"0 0 {width:.0} {height:.0}\" style=\"text-rendering:geometricPrecision;shape-rendering:geometricPrecision;font-variant-ligatures:none;\"><g font-family=\"{}\">{body}</g></svg>",
        escape_attr(options.font_family),
    )
}

fn layout_graph(graph: &Graph) -> HashMap<String, (usize, usize)> {
    let config = LayoutConfig::default();
    #[allow(deprecated)]
    let LayoutResult { positions } = match config.backend {
        LayoutBackend::Sugiyama => layout::sugiyama::sugiyama_layout(graph, &config),
        LayoutBackend::Native | LayoutBackend::LayeredLegacy => {
            layout::layered::layout(graph, &config)
        }
    };
    positions
}

fn subgraph_position_offset(
    graph: &Graph,
    positions: &HashMap<String, (usize, usize)>,
) -> (usize, usize) {
    let node_sg_map = graph.node_to_subgraph();
    let max_depth = compute_max_nesting_depth(graph);
    let required_pad = SG_BORDER_PAD * (max_depth + 1);

    let mut min_col = usize::MAX;
    let mut min_row = usize::MAX;
    for (node_id, &(col, row)) in positions.iter() {
        if node_sg_map.contains_key(node_id) {
            min_col = min_col.min(col);
            min_row = min_row.min(row);
        }
    }
    if min_col == usize::MAX {
        return (0, 0);
    }
    (
        required_pad.saturating_sub(min_col),
        required_pad.saturating_sub(min_row),
    )
}

fn compute_max_nesting_depth(graph: &Graph) -> usize {
    fn depth_of(graph: &Graph, subgraph: &crate::types::Subgraph, current: usize) -> usize {
        let mut max = current;
        for child_id in &subgraph.subgraph_ids {
            if let Some(child) = graph.find_subgraph(child_id) {
                max = max.max(depth_of(graph, child, current + 1));
            }
        }
        max
    }

    graph
        .subgraphs
        .iter()
        .map(|subgraph| depth_of(graph, subgraph, 0))
        .max()
        .unwrap_or(0)
}

fn build_node_rects(
    graph: &Graph,
    positions: &HashMap<String, (usize, usize)>,
) -> HashMap<String, Rect> {
    graph
        .nodes
        .iter()
        .filter_map(|node| {
            let &(col, row) = positions.get(&node.id)?;
            Some((
                node.id.clone(),
                rect_for_cells(col, row, node_width_cells(node), node_height_cells(node)),
            ))
        })
        .collect()
}

fn node_width_cells(node: &Node) -> usize {
    let inner = node.label_width() + 4;
    match node.shape {
        NodeShape::Circle
        | NodeShape::Stadium
        | NodeShape::Hexagon
        | NodeShape::Asymmetric
        | NodeShape::Subroutine
        | NodeShape::Parallelogram
        | NodeShape::ParallelogramBackslash
        | NodeShape::Trapezoid
        | NodeShape::TrapezoidInverted => inner + 2,
        NodeShape::DoubleCircle => inner + 4,
        NodeShape::Bar(BarOrientation::Vertical) => 2,
        NodeShape::Bar(BarOrientation::Horizontal) => inner.max(8),
        _ => inner,
    }
    .max(6)
}

fn node_height_cells(node: &Node) -> usize {
    let extra = node.label_line_count().saturating_sub(1);
    match node.shape {
        NodeShape::Cylinder => 4 + extra,
        NodeShape::DoubleCircle => 5 + extra,
        NodeShape::Bar(BarOrientation::Vertical) => 5,
        NodeShape::Bar(BarOrientation::Horizontal) => 1,
        _ => 3 + extra,
    }
}

fn rect_for_cells(col: usize, row: usize, width: usize, height: usize) -> Rect {
    Rect {
        x: CANVAS_PADDING + col as f32 * CELL_WIDTH,
        y: CANVAS_PADDING + row as f32 * CELL_HEIGHT,
        width: width as f32 * CELL_WIDTH,
        height: height as f32 * CELL_HEIGHT,
    }
}

fn canvas_size(
    graph: &Graph,
    node_rects: &HashMap<String, Rect>,
    subgraph_bounds: &[SubgraphBounds],
) -> (f32, f32) {
    let mut max_x = CANVAS_PADDING;
    let mut max_y = CANVAS_PADDING;

    for rect in node_rects.values() {
        max_x = max_x.max(rect.x + rect.width);
        max_y = max_y.max(rect.y + rect.height);
    }
    for bounds in subgraph_bounds {
        let rect = rect_for_cells(bounds.col, bounds.row, bounds.width, bounds.height);
        max_x = max_x.max(rect.x + rect.width);
        max_y = max_y.max(rect.y + rect.height);
    }
    if graph.edges.iter().any(|edge| edge.label.is_some()) {
        max_y += CELL_HEIGHT;
    }

    (max_x + CANVAS_PADDING, max_y + CANVAS_PADDING)
}

fn marker_defs(options: &SvgRenderOptions<'_>) -> String {
    let edge = escape_attr(options.edge_stroke);
    format!(
        "<defs><filter id=\"node-shadow\" x=\"-16%\" y=\"-20%\" width=\"132%\" height=\"145%\"><feDropShadow dx=\"0\" dy=\"2\" stdDeviation=\"2.4\" flood-color=\"#000000\" flood-opacity=\"0.14\"/></filter><marker id=\"arrow\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"9\" markerHeight=\"9\" orient=\"auto-start-reverse\"><path d=\"M 0 0 L 10 5 L 0 10 z\" fill=\"{edge}\"/></marker><marker id=\"circle\" viewBox=\"0 0 10 10\" refX=\"5\" refY=\"5\" markerWidth=\"9\" markerHeight=\"9\" orient=\"auto\"><circle cx=\"5\" cy=\"5\" r=\"3.5\" fill=\"none\" stroke=\"{edge}\" stroke-width=\"2\"/></marker><marker id=\"cross\" viewBox=\"0 0 10 10\" refX=\"5\" refY=\"5\" markerWidth=\"9\" markerHeight=\"9\" orient=\"auto\"><path d=\"M 2 2 L 8 8 M 8 2 L 2 8\" fill=\"none\" stroke=\"{edge}\" stroke-width=\"2\" stroke-linecap=\"round\"/></marker></defs>"
    )
}

fn render_subgraph(
    bounds: &SubgraphBounds,
    graph: &Graph,
    options: &SvgRenderOptions<'_>,
) -> String {
    let rect = rect_for_cells(bounds.col, bounds.row, bounds.width, bounds.height);
    let style = graph.subgraph_styles.get(&bounds.id);
    let stroke = style
        .and_then(|style| style.stroke)
        .map_or_else(|| options.subgraph_stroke.to_string(), rgb_to_hex);
    let fill = style
        .and_then(|style| style.fill)
        .map_or_else(|| options.subgraph_fill.to_string(), rgb_to_hex);
    let label_x = rect.x + 14.0;
    let label_y = rect.y + 20.0;

    format!(
        "<g><rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"12\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.5\" stroke-dasharray=\"6 5\" opacity=\"0.78\"/><text x=\"{label_x:.1}\" y=\"{label_y:.1}\" font-size=\"13\" font-weight=\"700\" fill=\"{}\">{}</text></g>",
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        escape_attr(&fill),
        escape_attr(&stroke),
        escape_attr(options.muted_text_color),
        escape_text(&bounds.label),
    )
}

fn render_edge(
    index: usize,
    edge: &Edge,
    graph: &Graph,
    node_rects: &HashMap<String, Rect>,
    options: &SvgRenderOptions<'_>,
) -> String {
    let (Some(from), Some(to)) = (node_rects.get(&edge.from), node_rects.get(&edge.to)) else {
        return String::new();
    };
    let start = boundary_point(*from, to.center());
    let end = boundary_point(*to, from.center());
    let (path, label_point) = edge_path(start, end, graph.direction);
    let colors = graph.edge_styles.get(&index).copied().unwrap_or_default();
    let stroke = colors
        .stroke
        .map_or_else(|| options.edge_stroke.to_string(), rgb_to_hex);
    let label_color = colors
        .color
        .map_or_else(|| options.edge_label_color.to_string(), rgb_to_hex);
    let dash = match edge.style {
        EdgeStyle::Dotted => " stroke-dasharray=\"4 7\"",
        _ => "",
    };
    let stroke_width = match edge.style {
        EdgeStyle::Thick => EDGE_STROKE_WIDTH + 1.0,
        _ => EDGE_STROKE_WIDTH,
    };
    let marker_start = endpoint_marker(edge.start, true);
    let marker_end = endpoint_marker(edge.end, false);
    let mut rendered = format!(
        "<path d=\"{path}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{stroke_width}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"{dash}{marker_start}{marker_end}/>",
        escape_attr(&stroke),
    );

    if let Some(label) = edge
        .label
        .as_deref()
        .filter(|label| !label.trim().is_empty())
    {
        let label_width = UnicodeWidthStr::width(label) as f32 * 8.2 + 20.0;
        rendered.push_str(&format!(
            "<g><rect x=\"{:.1}\" y=\"{:.1}\" width=\"{label_width:.1}\" height=\"{EDGE_LABEL_HEIGHT}\" rx=\"8\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1\"/><text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" dominant-baseline=\"middle\" font-size=\"{EDGE_LABEL_FONT_SIZE}\" font-weight=\"700\" fill=\"{}\">{}</text></g>",
            label_point.x - label_width / 2.0,
            label_point.y - EDGE_LABEL_HEIGHT / 2.0,
            escape_attr(options.node_fill),
            escape_attr(&stroke),
            label_point.x,
            label_point.y + 0.5,
            escape_attr(&label_color),
            escape_text(label),
        ));
    }

    rendered
}

fn boundary_point(rect: Rect, toward: Point) -> Point {
    let center = rect.center();
    let dx = toward.x - center.x;
    let dy = toward.y - center.y;
    if dx.abs() * rect.height > dy.abs() * rect.width {
        Point {
            x: if dx >= 0.0 {
                rect.x + rect.width
            } else {
                rect.x
            },
            y: center.y,
        }
    } else {
        Point {
            x: center.x,
            y: if dy >= 0.0 {
                rect.y + rect.height
            } else {
                rect.y
            },
        }
    }
}

fn edge_path(start: Point, end: Point, direction: Direction) -> (String, Point) {
    if direction.is_horizontal() {
        let mid_x = (start.x + end.x) / 2.0;
        (
            format!(
                "M {:.1} {:.1} H {:.1} V {:.1} H {:.1}",
                start.x, start.y, mid_x, end.y, end.x
            ),
            Point {
                x: mid_x,
                y: (start.y + end.y) / 2.0,
            },
        )
    } else {
        let mid_y = (start.y + end.y) / 2.0;
        (
            format!(
                "M {:.1} {:.1} V {:.1} H {:.1} V {:.1}",
                start.x, start.y, mid_y, end.x, end.y
            ),
            Point {
                x: (start.x + end.x) / 2.0,
                y: mid_y,
            },
        )
    }
}

fn endpoint_marker(endpoint: EdgeEndpoint, start: bool) -> &'static str {
    match (endpoint, start) {
        (EdgeEndpoint::Arrow, true) => " marker-start=\"url(#arrow)\"",
        (EdgeEndpoint::Arrow, false) => " marker-end=\"url(#arrow)\"",
        (EdgeEndpoint::Circle, true) => " marker-start=\"url(#circle)\"",
        (EdgeEndpoint::Circle, false) => " marker-end=\"url(#circle)\"",
        (EdgeEndpoint::Cross, true) => " marker-start=\"url(#cross)\"",
        (EdgeEndpoint::Cross, false) => " marker-end=\"url(#cross)\"",
        (EdgeEndpoint::None, _) => "",
    }
}

fn render_node(node: &Node, rect: Rect, graph: &Graph, options: &SvgRenderOptions<'_>) -> String {
    let style = graph.node_styles.get(&node.id).copied().unwrap_or_default();
    let fill = style
        .fill
        .map_or_else(|| options.node_fill.to_string(), rgb_to_hex);
    let stroke = style
        .stroke
        .map_or_else(|| options.node_stroke.to_string(), rgb_to_hex);
    let text_color = style
        .color
        .map_or_else(|| options.text_color.to_string(), rgb_to_hex);
    let mut rendered = String::new();
    rendered.push_str(&render_node_shape(node.shape, rect, &fill, &stroke));
    rendered.push_str(&render_node_label(node, rect, &text_color));
    rendered
}

fn render_node_shape(shape: NodeShape, rect: Rect, fill: &str, stroke: &str) -> String {
    let fill = escape_attr(fill);
    let stroke = escape_attr(stroke);
    match shape {
        NodeShape::Rounded | NodeShape::Note => format!(
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"12\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{NODE_STROKE_WIDTH}\"{NODE_SHADOW}/>",
            rect.x, rect.y, rect.width, rect.height
        ),
        NodeShape::Stadium => format!(
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{NODE_STROKE_WIDTH}\"{NODE_SHADOW}/>",
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            rect.height / 2.0
        ),
        NodeShape::Circle => format!(
            "<ellipse cx=\"{:.1}\" cy=\"{:.1}\" rx=\"{:.1}\" ry=\"{:.1}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{NODE_STROKE_WIDTH}\"{NODE_SHADOW}/>",
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0,
            rect.width / 2.0,
            rect.height / 2.0
        ),
        NodeShape::DoubleCircle => format!(
            "<ellipse cx=\"{:.1}\" cy=\"{:.1}\" rx=\"{:.1}\" ry=\"{:.1}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{NODE_STROKE_WIDTH}\"{NODE_SHADOW}/><ellipse cx=\"{:.1}\" cy=\"{:.1}\" rx=\"{:.1}\" ry=\"{:.1}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1.4\"/>",
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0,
            rect.width / 2.0,
            rect.height / 2.0,
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0,
            rect.width / 2.0 - 6.0,
            rect.height / 2.0 - 6.0
        ),
        NodeShape::Diamond => polygon(
            &[
                (rect.x + rect.width / 2.0, rect.y),
                (rect.x + rect.width, rect.y + rect.height / 2.0),
                (rect.x + rect.width / 2.0, rect.y + rect.height),
                (rect.x, rect.y + rect.height / 2.0),
            ],
            &fill,
            &stroke,
        ),
        NodeShape::Hexagon => polygon(
            &[
                (rect.x + 16.0, rect.y),
                (rect.x + rect.width - 16.0, rect.y),
                (rect.x + rect.width, rect.y + rect.height / 2.0),
                (rect.x + rect.width - 16.0, rect.y + rect.height),
                (rect.x + 16.0, rect.y + rect.height),
                (rect.x, rect.y + rect.height / 2.0),
            ],
            &fill,
            &stroke,
        ),
        NodeShape::Parallelogram => polygon(
            &[
                (rect.x + 14.0, rect.y),
                (rect.x + rect.width, rect.y),
                (rect.x + rect.width - 14.0, rect.y + rect.height),
                (rect.x, rect.y + rect.height),
            ],
            &fill,
            &stroke,
        ),
        NodeShape::ParallelogramBackslash => polygon(
            &[
                (rect.x, rect.y),
                (rect.x + rect.width - 14.0, rect.y),
                (rect.x + rect.width, rect.y + rect.height),
                (rect.x + 14.0, rect.y + rect.height),
            ],
            &fill,
            &stroke,
        ),
        NodeShape::Trapezoid => polygon(
            &[
                (rect.x + 14.0, rect.y),
                (rect.x + rect.width - 14.0, rect.y),
                (rect.x + rect.width, rect.y + rect.height),
                (rect.x, rect.y + rect.height),
            ],
            &fill,
            &stroke,
        ),
        NodeShape::TrapezoidInverted => polygon(
            &[
                (rect.x, rect.y),
                (rect.x + rect.width, rect.y),
                (rect.x + rect.width - 14.0, rect.y + rect.height),
                (rect.x + 14.0, rect.y + rect.height),
            ],
            &fill,
            &stroke,
        ),
        NodeShape::Asymmetric => polygon(
            &[
                (rect.x, rect.y),
                (rect.x + rect.width - 14.0, rect.y),
                (rect.x + rect.width, rect.y + rect.height / 2.0),
                (rect.x + rect.width - 14.0, rect.y + rect.height),
                (rect.x, rect.y + rect.height),
            ],
            &fill,
            &stroke,
        ),
        NodeShape::Cylinder => format!(
            "<path d=\"M {:.1} {:.1} C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1} V {:.1} C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1} V {:.1}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{NODE_STROKE_WIDTH}\"{NODE_SHADOW}/><path d=\"M {:.1} {:.1} C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1.4\"/>",
            rect.x,
            rect.y + 10.0,
            rect.x,
            rect.y - 3.0,
            rect.x + rect.width,
            rect.y - 3.0,
            rect.x + rect.width,
            rect.y + 10.0,
            rect.y + rect.height - 10.0,
            rect.x + rect.width,
            rect.y + rect.height + 3.0,
            rect.x,
            rect.y + rect.height + 3.0,
            rect.x,
            rect.y + rect.height - 10.0,
            rect.y + 10.0,
            rect.x,
            rect.y + 10.0,
            rect.x,
            rect.y + 23.0,
            rect.x + rect.width,
            rect.y + 23.0,
            rect.x + rect.width,
            rect.y + 10.0,
        ),
        NodeShape::Subroutine => format!(
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{NODE_CORNER_RADIUS}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{NODE_STROKE_WIDTH}\"{NODE_SHADOW}/><path d=\"M {:.1} {:.1} V {:.1} M {:.1} {:.1} V {:.1}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1.4\"/>",
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            rect.x + 10.0,
            rect.y,
            rect.y + rect.height,
            rect.x + rect.width - 10.0,
            rect.y,
            rect.y + rect.height,
        ),
        NodeShape::Bar(BarOrientation::Horizontal) => format!(
            "<path d=\"M {:.1} {:.1} H {:.1}\" stroke=\"{stroke}\" stroke-width=\"7\" stroke-linecap=\"round\"/>",
            rect.x,
            rect.y + rect.height / 2.0,
            rect.x + rect.width,
        ),
        NodeShape::Bar(BarOrientation::Vertical) => format!(
            "<path d=\"M {:.1} {:.1} V {:.1}\" stroke=\"{stroke}\" stroke-width=\"7\" stroke-linecap=\"round\"/>",
            rect.x + rect.width / 2.0,
            rect.y,
            rect.y + rect.height,
        ),
        NodeShape::Rectangle => format!(
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{NODE_CORNER_RADIUS}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{NODE_STROKE_WIDTH}\"{NODE_SHADOW}/>",
            rect.x, rect.y, rect.width, rect.height
        ),
    }
}

fn polygon(points: &[(f32, f32)], fill: &str, stroke: &str) -> String {
    let points = points
        .iter()
        .map(|(x, y)| format!("{x:.1},{y:.1}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "<polygon points=\"{points}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{NODE_STROKE_WIDTH}\" stroke-linejoin=\"round\"{NODE_SHADOW}/>"
    )
}

fn render_node_label(node: &Node, rect: Rect, text_color: &str) -> String {
    if matches!(node.shape, NodeShape::Bar(_)) {
        return String::new();
    }

    let lines: Vec<&str> = if node.label.is_empty() {
        vec![""]
    } else {
        node.label.lines().collect()
    };
    let total_height = (lines.len().saturating_sub(1) as f32) * NODE_LINE_HEIGHT;
    let start_y = rect.y + rect.height / 2.0 - total_height / 2.0;
    let text_x = rect.x + rect.width / 2.0;
    let max_text_width = (rect.width - NODE_LABEL_PADDING_X * 2.0).max(10.0);
    let mut rendered = String::new();

    for (index, line) in lines.iter().enumerate() {
        rendered.push_str(&format!(
            "<text x=\"{text_x:.1}\" y=\"{:.1}\" text-anchor=\"middle\" dominant-baseline=\"middle\" font-size=\"{NODE_FONT_SIZE}\" font-weight=\"650\" fill=\"{}\" textLength=\"{:.1}\" lengthAdjust=\"spacingAndGlyphs\">{}</text>",
            start_y + index as f32 * NODE_LINE_HEIGHT,
            escape_attr(text_color),
            text_length(line, max_text_width),
            escape_text(line),
        ));
    }

    rendered
}

fn text_length(text: &str, max_width: f32) -> f32 {
    let natural = UnicodeWidthStr::width(text) as f32 * 8.2;
    natural.min(max_width).max(1.0)
}

fn empty_svg(options: &SvgRenderOptions<'_>) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"120\" height=\"72\" viewBox=\"0 0 120 72\"><text x=\"60\" y=\"36\" text-anchor=\"middle\" dominant-baseline=\"middle\" font-family=\"{}\" font-size=\"14\" fill=\"{}\">Empty diagram</text></svg>",
        escape_attr(options.font_family),
        escape_attr(options.muted_text_color),
    )
}

fn rgb_to_hex(rgb: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}

fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attr(text: &str) -> String {
    escape_text(text)
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_flowchart_as_shape_svg() {
        let svg = render_svg("graph LR\nA[Start] --> B{Ready?}").unwrap();

        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("<rect"));
        assert!(svg.contains("<polygon"));
        assert!(svg.contains("<path"));
        assert!(svg.contains("Start"));
        assert!(svg.contains("Ready?"));
    }

    #[test]
    fn unsupported_diagrams_return_unsupported_error() {
        let error = render_svg("sequenceDiagram\nAlice->>Bob: hi").unwrap_err();

        assert!(matches!(error, Error::UnsupportedDiagram(_)));
    }

    #[test]
    fn renders_labeled_nodes_in_complex_flowchart_without_bare_id_labels() {
        let source = r#"flowchart TD
  A["商品页加载<br/>content script document_start 注入"] --> B["登录态检查<br/>chrome.storage.local.pluginAuth"]
  B -->|未登录| B0["不进入商品分析<br/>只等登录态变化"]
  B -->|已登录| C["白名单门控<br/>focus + host 白名单 + Shopify 自动发现"]
  C -->|未命中| C0["静默退出<br/>页面完全不显示插件"]
  C -->|命中| D["挂载 Shadow DOM React Root<br/>但 pluginActive 仍可为 false"]
  D --> E["商品主管线准备<br/>XPath / Shopify JSON / URL PDP / 截图 OCR"]
  E -->|非 PDP| E0["不显示插件"]
  E -->|确认 PDP + 拿到商品| F["暂存 preparedAnalysis<br/>小女孩进入正常态，等待点击"]
  F --> G["用户点击小女孩"]
  G --> H["POST /api/plugin-ai/analyze<br/>带 pageData + extractedProduct + engine"]
  H --> I{"engine?"}
  I -->|server| J["服务端研究 + Tavily + 平台模型最终成稿"]
  J --> K["SSE: analysis_report_start/component/done"]
  I -->|user_ai| L["服务端只做品类 + 模板 + web prompt 备料"]
  L --> M["SSE: bridge_prompt + bridgeHandoff done"]
  M --> N["插件 Bridge Client 接管"]
  N --> O["Bridge Tab 驱动 ChatGPT/Gemini iframe"]
  O --> P["用户 AI 输出 JSONL 原文"]
  P --> Q["POST /analyze/bridge-complete"]
  Q --> R{"服务端验收通过?"}
  R -->|通过| S["返回同构 events 数组"]
  R -->|未 done 且有额度| O2["BRIDGE_CONTINUE 续写"]
  O2 --> P
  R -->|失败/额度尽| T["POST /analyze/bridge-fallback<br/>服务端兜底成稿"]
  T --> K
  S --> U["逐条喂 mapAnalyzeSseEvent"]
  K --> U
  U --> V["analysis-store streamingReport"]
  V --> W["SidePanel 渲染最终组件报告"]"#;
        let svg = render_svg(source).unwrap();

        for bare_label in ["C", "D", "F", "S", "T"] {
            assert!(!svg.contains(&format!(">{bare_label}</text>")));
        }
        for expected in [
            "白名单门控",
            "挂载 Shadow DOM React Root",
            "暂存 preparedAnalysis",
            "返回同构 events 数组",
            "POST /analyze/bridge-fallback",
        ] {
            assert!(svg.contains(expected));
        }
    }
}
