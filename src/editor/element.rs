use std::collections::HashSet;
use std::sync::Arc;

use gpui::{
    App, BorderStyle, Bounds, Corners, Element, ElementId, ElementInputHandler, Entity,
    FontFallbacks, GlobalElementId, Hsla, IntoElement, LayoutId, PaintQuad, Pixels, Point,
    ShapedLine, Style, TextAlign, TextRun, TransformationMatrix, Window, WrappedLine, fill, point,
    px, quad, relative, size,
};

use crate::markdown::BlockType;

use super::helpers;
use super::*;

pub(crate) struct EditorElement {
    pub editor: Entity<EditorView>,
}

pub(crate) struct EditorPrepaintState {
    lines: Vec<PrepaintLine>,
    cursor_quads: Vec<PaintQuad>,
    selection_quads: Vec<PaintQuad>,
    search_match_quads: Vec<PaintQuad>,
    line_highlight: Option<PaintQuad>,
    code_block_bgs: Vec<PaintQuad>,
    marked_underlines: Vec<PaintQuad>,
    gutter_quads: Vec<PaintQuad>,
    gutter_labels: Vec<(ShapedLine, Point<Pixels>, Pixels)>,
    /// (bounds, svg_path, color, rotation_degrees)
    gutter_svgs: Vec<(Bounds<Pixels>, &'static str, Hsla, f32)>,
    /// HR dashed lines: (y_position, start_x, width)
    hr_lines: Vec<(f32, f32, f32)>,
    /// Image previews to paint: (bounds, image_data)
    image_previews: Vec<(Bounds<Pixels>, Arc<RenderImage>)>,
    image_borders: Vec<PaintQuad>,
    /// Table cell borders and backgrounds
    table_borders: Vec<PaintQuad>,
    table_bgs: Vec<PaintQuad>,
    /// Image URL-edit icons: (bounds, line_idx, alpha)
    image_edit_icons: Vec<(Bounds<Pixels>, usize, f32)>,
}

struct PrepaintLine {
    wrapped: WrappedLine,
    origin: Point<Pixels>,
    line_height: Pixels,
    /// Per-visual-line shaped segments for indented painting.
    segments: Option<Vec<(WrappedLine, Point<Pixels>)>>,
}

impl IntoElement for EditorElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = EditorPrepaintState;

    fn id(&self) -> Option<ElementId> {
        Some("editor-element".into())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let editor = self.editor.read(cx);
        let theme = &editor.theme;
        let buffer = &editor.buffer;
        let cursor = &editor.cursor;
        let decorations = &editor.decorations;
        let scroll_offset = editor.scroll_offset;
        let cursor_line = editor.cursor_line();

        let viewport_height: f32 = bounds.size.height.into();
        let viewport_width: f32 = bounds.size.width.into();
        let origin_x: f32 = bounds.origin.x.into();
        let origin_y: f32 = bounds.origin.y.into();
        let editor_width = if editor.editor_left_aligned {
            viewport_width
        } else {
            viewport_width.min(editor.max_editor_width())
        };
        let centered_left = origin_x + ((viewport_width - editor_width) / 2.0).max(0.0);
        let editor_left = if editor.editor_left_aligned {
            origin_x
        } else {
            centered_left
        };
        let text_width = (editor_width - editor.padding_left() - editor.padding_right())
            .max(editor.scaled(120.0));
        let wrap_width = text_width;

        let mut lines = Vec::new();
        let mut selection_quads = Vec::new();
        let mut search_match_quads: Vec<PaintQuad> = Vec::new();
        let mut line_highlight = None;
        let mut code_block_bgs = Vec::new();
        let mut cursor_quads: Vec<PaintQuad> = Vec::new();
        let mut marked_underlines = Vec::new();

        let mut y = origin_y + editor.padding_top();
        let line_count = buffer.line_count();

        // Collect all cursor selection ranges (primary + extra)
        let mut all_sel_ranges: Vec<std::ops::Range<usize>> =
            Vec::with_capacity(1 + editor.extra_cursors.len());
        let sel_range = cursor.selection_range();
        let _has_selection = cursor.has_selection();
        all_sel_ranges.push(sel_range.clone());
        for ec in &editor.extra_cursors {
            all_sel_ranges.push(ec.selection_range());
        }
        // Collect all cursor offsets for painting cursor bars
        let mut all_cursor_offsets: Vec<usize> = Vec::with_capacity(1 + editor.extra_cursors.len());
        all_cursor_offsets.push(cursor.offset);
        for ec in &editor.extra_cursors {
            all_cursor_offsets.push(ec.offset);
        }
        let any_selection = all_sel_ranges.iter().any(|r| r.start != r.end);

        let mut gutter_quads = Vec::new();
        let gutter_labels: Vec<(ShapedLine, Point<Pixels>, Pixels)> = Vec::new();
        let mut gutter_svgs: Vec<(Bounds<Pixels>, &'static str, Hsla, f32)> = Vec::new();
        let mut checkbox_hit_areas: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut image_hit_areas: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut display_info_cache: Vec<Option<LineDisplayInfo>> = Vec::with_capacity(line_count);
        let has_valid_cache = editor.line_display_info.len() == line_count
            && (editor.last_viewport_width - viewport_width).abs() < 0.5;
        let mut hr_lines: Vec<(f32, f32, f32)> = Vec::new();
        let mut image_previews: Vec<(Bounds<Pixels>, Arc<RenderImage>)> = Vec::new();
        let mut image_borders: Vec<PaintQuad> = Vec::new();
        let mut table_borders: Vec<PaintQuad> = Vec::new();
        let mut table_bgs: Vec<PaintQuad> = Vec::new();
        let mut image_edit_icons: Vec<(Bounds<Pixels>, usize, f32)> = Vec::new();
        let mut image_edit_hit_areas_local: Vec<(Bounds<Pixels>, usize)> = Vec::new();
        let mut gutter_fold_hit_areas_local: Vec<(Bounds<Pixels>, usize, bool)> = Vec::new();

        let hidden_lines: HashSet<usize> = {
            let mut set = HashSet::new();
            for &fold_line in &editor.folded_sections {
                if let Some(range) = editor.fold_range_for_line(fold_line) {
                    for l in range {
                        set.insert(l);
                    }
                }
            }
            set
        };

        let text_style = window.text_style();
        let editor_styles = &editor.styles.editor;
        let configured_font_family = |configured: &'static str| {
            if configured == ".SystemUIFont" {
                text_style.font().family.clone()
            } else {
                configured.into()
            }
        };
        let mut monospace_font = text_style.font();
        monospace_font.family = configured_font_family(editor_styles.editor_monospace_font_family);
        monospace_font.fallbacks = Some(FontFallbacks::from_fonts(vec![
            editor_styles.editor_body_cjk_font_family.to_string(),
        ]));

        let mut body_font = text_style.font();
        body_font.family = configured_font_family(editor_styles.editor_body_latin_font_family);
        body_font.fallbacks = Some(FontFallbacks::from_fonts(vec![
            editor_styles.editor_body_cjk_font_family.to_string(),
        ]));

        let mut body_cjk_font = text_style.font();
        body_cjk_font.family = configured_font_family(editor_styles.editor_body_cjk_font_family);
        body_cjk_font.fallbacks = None;

        let mut heading_font = text_style.font();
        heading_font.family =
            configured_font_family(editor_styles.editor_heading_latin_font_family);
        heading_font.fallbacks = Some(FontFallbacks::from_fonts(vec![
            editor_styles.editor_heading_cjk_font_family.to_string(),
        ]));

        let mut heading_cjk_font = text_style.font();
        heading_cjk_font.family =
            configured_font_family(editor_styles.editor_heading_cjk_font_family);
        heading_cjk_font.fallbacks = None;

        let base_char_width = {
            let test_run = TextRun {
                len: 1,
                font: monospace_font.clone(),
                color: gpui::transparent_black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            if let Ok(test_lines) = window.text_system().shape_text(
                "x".into(),
                px(editor.base_font_size()),
                &[test_run],
                None,
                None,
            ) {
                f32::from(test_lines[0].width())
            } else {
                editor.base_font_size() * 0.6
            }
        };

        let image_cache = &editor.image_cache;
        let table_cell_padding_px = editor.scaled(editor.styles.table.table_cell_padding_px);
        let table_cell_padding_chars = if table_cell_padding_px <= 0.0 {
            0
        } else {
            (table_cell_padding_px / base_char_width).ceil().max(1.0) as usize
        };
        let collapsed_task_prefix_chars = ((editor.base_font_size()
            * editor.styles.checkbox.checkbox_size_factor
            + editor.scaled(editor.styles.checkbox.checkbox_text_gap_px))
            / base_char_width)
            .ceil()
            .max(3.0) as usize;

        let is_code_block_type = |i: usize| {
            matches!(
                decorations.get(i).map(|d| &d.block_type),
                Some(BlockType::CodeBlock | BlockType::CodeBlockFence)
            )
        };

        for line_idx in 0..line_count {
            let is_cursor_line_early = line_idx == cursor_line;
            if has_valid_cache && !is_cursor_line_early {
                if let Some(Some(cached)) = editor.line_display_info.get(line_idx) {
                    let cached_h =
                        cached.line_height * cached.visual_line_count as f32 + cached.extra_height;
                    let est_top = y - scroll_offset;
                    let est_bottom = est_top + cached_h;
                    if est_bottom < origin_y || est_top > origin_y + viewport_height {
                        display_info_cache.push(Some(cached.clone()));
                        y += cached_h;
                        continue;
                    }
                }
            }

            let decoration = decorations.get(line_idx).cloned().unwrap_or_default();
            let block_type = &decoration.block_type;
            let font_size = editor.font_size_for_block(block_type);
            let line_height = editor.line_height_for_block(block_type);
            let (block_padding_top, block_padding_bottom) = editor.block_padding_for_line(line_idx);

            if hidden_lines.contains(&line_idx) {
                let line_content = buffer.line_content(line_idx);
                let blank_run = TextRun {
                    len: 1,
                    font: body_font.clone(),
                    color: gpui::transparent_black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let wrapped_line = window
                    .text_system()
                    .shape_text(" ".into(), px(font_size), &[blank_run], None, None)
                    .ok()
                    .and_then(|ls| ls.into_iter().next());
                display_info_cache.push(wrapped_line.map(|wl| LineDisplayInfo {
                    display_text: String::new(),
                    display_to_buffer: Vec::new(),
                    buffer_to_display: vec![0; line_content.len()],
                    font_size,
                    line_height: 0.0,
                    visual_line_count: 0,
                    wrapped_line: wl,
                    extra_height: 0.0,
                    indent_px: 0.0,
                    content_x: editor_left + editor.padding_left(),
                }));
                continue;
            }
            let is_cursor_line = line_idx == cursor_line;

            let line_content = buffer.line_content(line_idx);
            let line_start_offset = buffer.line_start(line_idx);
            let line_end_offset = buffer.line_end(line_idx);

            let cursor_in_table = if let Some(ref ti) = decoration.table_info {
                cursor_line >= ti.table_start_line && cursor_line < ti.table_end_line
            } else {
                false
            };
            let ime_active_on_line = editor.marked_range.as_ref().is_some_and(|marked| {
                marked.start < line_end_offset && marked.end >= line_start_offset
            });
            let cursor_col = if editor.is_selecting && editor.cursor.has_selection() {
                let sel = editor.cursor.selection_range();
                if sel.start >= line_start_offset && sel.end <= line_end_offset {
                    let sel_start_col = sel.start - line_start_offset;
                    let sel_end_col = sel.end - line_start_offset;
                    let within_syntax = decoration.inline_spans.iter().any(|span| {
                        let prefix_len = match span.kind {
                            crate::markdown::InlineKind::Bold => 2,
                            crate::markdown::InlineKind::Italic => 1,
                            crate::markdown::InlineKind::Strikethrough => 2,
                            crate::markdown::InlineKind::InlineCode => 1,
                            crate::markdown::InlineKind::Link
                            | crate::markdown::InlineKind::Image => {
                                if span.kind == crate::markdown::InlineKind::Image {
                                    2
                                } else {
                                    1
                                }
                            }
                        };
                        let full_start = span.range.start.saturating_sub(prefix_len);
                        let full_end = decoration
                            .syntax_ranges
                            .iter()
                            .filter(|sr| sr.range.end > span.range.end)
                            .map(|sr| sr.range.end)
                            .max()
                            .unwrap_or(span.range.end);
                        sel_start_col >= full_start && sel_end_col <= full_end
                    });
                    if within_syntax {
                        Some(cursor.offset.saturating_sub(line_start_offset))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else if ime_active_on_line {
                Some(line_content.len())
            } else if is_cursor_line {
                Some(cursor.offset.saturating_sub(line_start_offset))
            } else if cursor_in_table {
                Some(line_content.len())
            } else {
                None
            };

            let hide_table_separator = matches!(block_type, BlockType::Table)
                && decoration
                    .table_info
                    .as_ref()
                    .is_some_and(|table_info| table_info.is_separator);
            if hide_table_separator {
                let blank_run = TextRun {
                    len: 1,
                    font: body_font.clone(),
                    color: gpui::transparent_black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let wrapped_line = window
                    .text_system()
                    .shape_text(" ".into(), px(font_size), &[blank_run], None, None)
                    .ok()
                    .and_then(|lines| lines.into_iter().next());
                display_info_cache.push(wrapped_line.map(|wrapped_line| LineDisplayInfo {
                    display_text: String::new(),
                    display_to_buffer: Vec::new(),
                    buffer_to_display: vec![0; line_content.len()],
                    font_size,
                    line_height: 0.0,
                    visual_line_count: 0,
                    wrapped_line,
                    extra_height: 0.0,
                    indent_px: 0.0,
                    content_x: editor_left + editor.padding_left(),
                }));
                continue;
            }

            let table_width_chars = if *block_type == BlockType::Table {
                Some((text_width / base_char_width) as usize)
            } else {
                None
            };
            let (display_text, display_to_buffer, buffer_to_display, prefix_collapsed) =
                helpers::build_display_text(
                    line_content,
                    &decoration,
                    cursor_col,
                    table_width_chars,
                    table_cell_padding_chars,
                    collapsed_task_prefix_chars,
                    editor.styles.checkbox.checkbox_parent_prefix_chars as usize,
                );

            let runs = helpers::build_text_runs_for_display(
                &display_text,
                line_content,
                &display_to_buffer,
                &decoration,
                cursor_col,
                prefix_collapsed,
                &body_font,
                &body_cjk_font,
                &monospace_font,
                &heading_font,
                &heading_cjk_font,
                collapsed_task_prefix_chars,
                font_size,
                theme,
            );

            let display_shared: SharedString = if display_text.is_empty() {
                " ".into()
            } else {
                display_text.clone().into()
            };

            let is_list_type = matches!(
                block_type,
                BlockType::UnorderedList | BlockType::OrderedList | BlockType::TaskList(_)
            );
            let indent_px = if is_list_type {
                let prefix_bytes = helpers::compute_display_prefix_bytes(
                    &display_text,
                    block_type,
                    prefix_collapsed,
                    collapsed_task_prefix_chars,
                );
                if prefix_bytes > 0 && prefix_bytes <= display_text.len() {
                    let prefix_chars = display_text
                        .char_indices()
                        .take_while(|(i, _)| *i < prefix_bytes)
                        .count();
                    prefix_chars as f32 * base_char_width
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let effective_wrap_width = if indent_px > 0.0 {
                (wrap_width - indent_px).max(wrap_width * 0.3)
            } else {
                wrap_width
            };

            let wrap_width_param = if *block_type == BlockType::Table {
                None
            } else {
                Some(px(effective_wrap_width))
            };

            let Ok(wrapped_lines) = window.text_system().shape_text(
                display_shared,
                px(font_size),
                &runs,
                wrap_width_param,
                None,
            ) else {
                display_info_cache.push(None);
                y += line_height;
                continue;
            };

            let wrapped = &wrapped_lines[0];
            let visual_line_count = wrapped.wrap_boundaries().len() + 1;
            let text_line_height = line_height * visual_line_count as f32;
            let image_gap_top = editor.scaled(editor.styles.image_preview.image_gap_top_px);
            let image_gap_bottom = editor.scaled(editor.styles.image_preview.image_gap_bottom_px);

            let image_active = decoration.image_path.is_some()
                && helpers::is_image_syntax_active(
                    &decoration,
                    line_content,
                    line_start_offset,
                    cursor.offset,
                    &sel_range,
                );

            let image_extra_height = if let Some(ref path) = decoration.image_path {
                if editor.folded_image_previews.contains(&line_idx) {
                    0.0
                } else if let Some(Some(img_data)) = image_cache.get(path) {
                    let img_size = img_data.size(0);
                    let img_w = img_size.width.0 as f32;
                    let img_h = img_size.height.0 as f32;
                    if img_w > 0.0 && img_h > 0.0 {
                        let (_, display_h) =
                            helpers::compute_image_preview_size(img_w, img_h, text_width);
                        display_h + image_gap_top + image_gap_bottom
                    } else {
                        0.0
                    }
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let extra_height = block_padding_top + block_padding_bottom + image_extra_height;
            let total_line_height = text_line_height + extra_height;

            let line_top = y - scroll_offset;
            let line_bottom = line_top + total_line_height;
            let text_line_top = line_top + block_padding_top;
            let content_x =
                if matches!(block_type, BlockType::CodeBlock | BlockType::CodeBlockFence) {
                    editor_left
                        + editor.padding_left()
                        + editor.scaled(editor.styles.code_block.code_block_text_padding_left_px)
                } else {
                    editor_left + editor.padding_left()
                };

            display_info_cache.push(Some(LineDisplayInfo {
                display_text: display_text.clone(),
                display_to_buffer: display_to_buffer.clone(),
                buffer_to_display: buffer_to_display.clone(),
                font_size,
                line_height,
                visual_line_count,
                wrapped_line: wrapped.clone(),
                extra_height,
                indent_px,
                content_x,
            }));

            if line_bottom < origin_y || line_top > origin_y + viewport_height {
                y += total_line_height;
                continue;
            }

            let segments = if indent_px > 0.0 && wrapped.wrap_boundaries().len() > 0 {
                let byte_indices = helpers::wrap_boundary_byte_indices(wrapped);
                let mut shaped_segments: Vec<(WrappedLine, Point<Pixels>)> = Vec::new();
                let mut seg_start = 0;
                let seg_count = byte_indices.len() + 1;
                for seg_idx in 0..seg_count {
                    let seg_end = if seg_idx < byte_indices.len() {
                        byte_indices[seg_idx]
                    } else {
                        display_text.len()
                    };
                    let seg_text = &display_text[seg_start..seg_end];
                    let seg_runs = helpers::extract_runs_for_range(&runs, seg_start, seg_end);
                    let seg_shared: SharedString = if seg_text.is_empty() {
                        " ".into()
                    } else {
                        seg_text.to_string().into()
                    };
                    if let Ok(seg_wrapped_lines) = window.text_system().shape_text(
                        seg_shared,
                        px(font_size),
                        &seg_runs,
                        None,
                        None,
                    ) {
                        if let Some(seg_wrapped) = seg_wrapped_lines.into_iter().next() {
                            let seg_x = if seg_idx == 0 {
                                content_x
                            } else {
                                content_x + indent_px
                            };
                            let seg_y = text_line_top + seg_idx as f32 * line_height;
                            shaped_segments.push((seg_wrapped, point(px(seg_x), px(seg_y))));
                        }
                    }
                    seg_start = seg_end;
                }
                Some(shaped_segments)
            } else {
                None
            };

            let origin = point(px(content_x), px(text_line_top));

            // Code block background
            if matches!(block_type, BlockType::CodeBlock | BlockType::CodeBlockFence) {
                let is_start_fence = matches!(block_type, BlockType::CodeBlockFence)
                    && (line_idx == 0 || !is_code_block_type(line_idx - 1));
                let is_end_fence = matches!(block_type, BlockType::CodeBlockFence)
                    && (line_idx + 1 >= line_count || !is_code_block_type(line_idx + 1));
                let cursor_on_this_line = cursor_line == line_idx;
                let clip_top = if !cursor_on_this_line && is_start_fence {
                    15.0
                } else {
                    0.0
                };
                let clip_bottom = if !cursor_on_this_line && is_end_fence {
                    15.0
                } else {
                    0.0
                };
                let bg_height = total_line_height - clip_top - clip_bottom;
                if bg_height > 0.0 {
                    code_block_bgs.push(fill(
                        Bounds::new(
                            point(
                                px(editor_left + editor.padding_left()),
                                px(line_top + clip_top),
                            ),
                            size(px(text_width), px(bg_height)),
                        ),
                        theme.bg_code,
                    ));
                }
            }

            // Table rendering
            if *block_type == BlockType::Table {
                if let Some(ref ti) = decoration.table_info {
                    let border_color = theme.border_decorative;
                    let border_w = 1.0_f32;
                    let table_left = editor_left + editor.padding_left();
                    let table_right = editor_left + editor.padding_left() + text_width;
                    let table_width = table_right - table_left;

                    if ti.is_separator {
                        table_borders.push(fill(
                            Bounds::new(
                                point(px(table_left), px(line_top)),
                                size(px(table_width), px(border_w)),
                            ),
                            border_color,
                        ));
                    } else {
                        if ti.is_header {
                            table_bgs.push(fill(
                                Bounds::new(
                                    point(px(table_left), px(line_top)),
                                    size(px(table_width), px(text_line_height)),
                                ),
                                theme.bg_code,
                            ));
                        }

                        table_borders.push(fill(
                            Bounds::new(
                                point(px(table_left), px(line_top)),
                                size(px(table_width), px(border_w)),
                            ),
                            border_color,
                        ));
                        table_borders.push(fill(
                            Bounds::new(
                                point(px(table_left), px(line_top + text_line_height - border_w)),
                                size(px(table_width), px(border_w)),
                            ),
                            border_color,
                        ));
                        table_borders.push(fill(
                            Bounds::new(
                                point(px(table_left), px(line_top)),
                                size(px(border_w), px(text_line_height)),
                            ),
                            border_color,
                        ));
                        table_borders.push(fill(
                            Bounds::new(
                                point(px(table_right - border_w), px(line_top)),
                                size(px(border_w), px(text_line_height)),
                            ),
                            border_color,
                        ));

                        let render_col_widths = helpers::compute_table_render_widths(
                            ti,
                            table_width_chars,
                            table_cell_padding_chars,
                        );
                        let mut col_x_offset = 0;
                        for (ci, cell) in ti.cells.iter().enumerate() {
                            let cell_chars = helpers::display_width(cell.trim());
                            let col_w = render_col_widths
                                .get(ci)
                                .copied()
                                .unwrap_or(cell_chars)
                                .max(cell_chars);
                            col_x_offset += col_w + table_cell_padding_chars * 2;
                            if ci + 1 < ti.cells.len() {
                                let sep_x = table_left + col_x_offset as f32 * base_char_width;
                                table_borders.push(fill(
                                    Bounds::new(
                                        point(px(sep_x), px(line_top)),
                                        size(px(border_w), px(text_line_height)),
                                    ),
                                    border_color,
                                ));
                            }
                        }
                    }
                }
            }

            // HR dashed line
            if *block_type == BlockType::HorizontalRule && !is_cursor_line {
                let hr_y = text_line_top + text_line_height / 2.0;
                hr_lines.push((hr_y, editor_left + editor.padding_left(), text_width));
            }

            // Image preview
            if !editor.folded_image_previews.contains(&line_idx) {
                if let Some(ref path) = decoration.image_path {
                    if let Some(Some(img_data)) = image_cache.get(path) {
                        let img_size = img_data.size(0);
                        let img_w = img_size.width.0 as f32;
                        let img_h = img_size.height.0 as f32;
                        if img_w > 0.0 && img_h > 0.0 {
                            let (display_w, display_h) =
                                helpers::compute_image_preview_size(img_w, img_h, text_width);
                            let img_top = text_line_top + text_line_height + image_gap_top;
                            let img_left = editor_left
                                + editor.padding_left()
                                + ((text_width - display_w) / 2.0).max(0.0);
                            let img_bounds = Bounds::new(
                                point(px(img_left), px(img_top)),
                                size(px(display_w), px(display_h)),
                            );
                            image_previews.push((img_bounds, img_data.clone()));
                            image_hit_areas.push((img_bounds, line_idx));
                            if image_active {
                                image_borders.push(quad(
                                    img_bounds,
                                    Corners::all(px(10.0)),
                                    gpui::transparent_black(),
                                    px(2.0),
                                    theme.cursor,
                                    BorderStyle::default(),
                                ));
                            }
                        }
                    }
                }
            }

            // Gutter decorations
            let gutter_center_y = text_line_top + line_height / 2.0;
            match block_type {
                BlockType::Heading(level) => {
                    let prefix_len = (*level as usize) + 1;
                    let heading_prefix_folded = cursor_col
                        .map(|col| helpers::prefix_is_folded(0, prefix_len + 1, Some(col)))
                        .unwrap_or(false);
                    let fold_hovered_this_line = editor.hovering_fold_line == Some(line_idx);
                    let gutter_styles = &editor.styles.gutter;
                    let max_heading_width = editor.scaled(gutter_styles.gutter_heading_max_total_width_px);
                    let heading_offset = editor.scaled(gutter_styles.gutter_heading_icon_offset_px);
                    let available_gutter = editor.padding_left() - heading_offset;
                    if is_cursor_line
                        && heading_prefix_folded
                        && !fold_hovered_this_line
                        && available_gutter >= max_heading_width
                    {
                        let icon_size = match *level {
                            1 => editor.scaled(gutter_styles.gutter_heading_h1_icon_size_px),
                            2 => editor.scaled(gutter_styles.gutter_heading_h2_icon_size_px),
                            3 => editor.scaled(gutter_styles.gutter_heading_h3_icon_size_px),
                            _ => editor.scaled(gutter_styles.gutter_heading_h4_icon_size_px),
                        };
                        let gap = editor.scaled(gutter_styles.gutter_heading_icon_gap_px);
                        let total_width = *level as f32 * icon_size + (*level as f32 - 1.0) * gap;
                        let start_x =
                            editor_left + editor.padding_left() - total_width - heading_offset;
                        for icon_index in 0..*level {
                            let icon_x = start_x + icon_index as f32 * (icon_size + gap);
                            let icon_y = gutter_center_y - icon_size / 2.0;
                            gutter_svgs.push((
                                Bounds::new(
                                    point(px(icon_x), px(icon_y)),
                                    size(px(icon_size), px(icon_size)),
                                ),
                                HASH_SVG_PATH,
                                theme.gutter_heading,
                                0.0_f32,
                            ));
                        }
                    }
                }
                BlockType::Blockquote => {
                    let icon_size = 16.0_f32;
                    let icon_x =
                        editor_left + editor.padding_left() - icon_size - editor.scaled(10.0);
                    let icon_y = gutter_center_y - 2.0 - icon_size / 2.0;
                    gutter_svgs.push((
                        Bounds::new(
                            point(px(icon_x), px(icon_y)),
                            size(px(icon_size), px(icon_size)),
                        ),
                        COMMENT_SVG_PATH,
                        theme.border_decorative,
                        0.0_f32,
                    ));
                }
                _ => {}
            }

            // Checkbox painting
            if prefix_collapsed {
                if let BlockType::TaskList(checked) = block_type {
                    let cb_size = font_size * editor.styles.checkbox.checkbox_size_factor;
                    let line_content_str = buffer.line_content(line_idx);
                    let leading_ws_bytes = line_content_str.len()
                        - line_content_str.trim_start_matches([' ', '\t']).len();
                    // For nested tasks, shift checkbox left to align with
                    // parent's content start (parent prefix is typically 2 chars).
                    let cb_indent = leading_ws_bytes.saturating_sub(
                        editor.styles.checkbox.checkbox_parent_prefix_chars as usize,
                    );
                    let cb_x = content_x + cb_indent as f32 * base_char_width;
                    let cb_y = text_line_top + (line_height - cb_size) / 2.0;
                    let cb_bounds =
                        Bounds::new(point(px(cb_x), px(cb_y)), size(px(cb_size), px(cb_size)));
                    let corner_r = px(editor.scaled(editor.styles.checkbox.checkbox_radius_px));
                    if *checked {
                        gutter_quads.push(quad(
                            cb_bounds,
                            Corners::all(corner_r),
                            theme.checkbox_checked,
                            px(0.0),
                            gpui::transparent_black(),
                            BorderStyle::default(),
                        ));
                        let icon_size = cb_size * editor.styles.checkbox.checkbox_check_icon_scale;
                        let icon_offset = (cb_size - icon_size) / 2.0;
                        gutter_svgs.push((
                            Bounds::new(
                                point(px(cb_x + icon_offset), px(cb_y + icon_offset)),
                                size(px(icon_size), px(icon_size)),
                            ),
                            CHECK_SVG_PATH,
                            gpui::white(),
                            0.0_f32,
                        ));
                    } else {
                        gutter_quads.push(quad(
                            cb_bounds,
                            Corners::all(corner_r),
                            gpui::transparent_black(),
                            px(1.5),
                            theme.border_tertiary,
                            BorderStyle::default(),
                        ));
                    }
                    let hit_padding = editor.scaled(2.0);
                    let hit_bounds = Bounds::new(
                        point(px(cb_x - hit_padding), px(line_top)),
                        size(px(cb_size + hit_padding * 2.0), px(total_line_height)),
                    );
                    checkbox_hit_areas.push((hit_bounds, line_idx));
                }
            }

            // Gutter fold icon
            if !hidden_lines.contains(&line_idx) {
                let fi_size = editor.scaled(editor.styles.gutter.gutter_fold_icon_size_px);
                let fi_x = content_x - fi_size - editor.scaled(5.0);

                if let Some(_) = editor.fold_range_for_line(line_idx) {
                    let is_folded = editor.folded_sections.contains(&line_idx);
                    let is_hovered = editor.hovering_fold_line == Some(line_idx);
                    let fi_y = if *block_type == BlockType::CodeBlockFence && !is_folded {
                        text_line_top + line_height + (line_height - fi_size) / 2.0
                    } else {
                        text_line_top + (line_height - fi_size) / 2.0
                    };
                    let fi_alpha = if is_folded {
                        0.75_f32
                    } else if is_hovered {
                        0.7_f32
                    } else {
                        0.0_f32
                    };
                    if fi_alpha > 0.0 {
                        let rotation_deg = if is_folded { 0.0_f32 } else { 90.0_f32 };
                        let mut fi_color = if is_folded {
                            theme.text_accent
                        } else {
                            theme.text_secondary
                        };
                        fi_color.a = fi_alpha;
                        gutter_svgs.push((
                            Bounds::new(point(px(fi_x), px(fi_y)), size(px(fi_size), px(fi_size))),
                            FOLD_SVG_PATH,
                            fi_color,
                            rotation_deg,
                        ));
                    }
                    gutter_fold_hit_areas_local.push((
                        Bounds::new(
                            point(px(fi_x - 3.0), px(line_top)),
                            size(px(fi_size + 6.0), px(line_height)),
                        ),
                        line_idx,
                        false,
                    ));
                }

                // Image preview fold icon
                if decoration.image_path.is_some()
                    && image_cache
                        .get(decoration.image_path.as_ref().unwrap())
                        .is_some_and(|v| v.is_some())
                {
                    let is_img_folded = editor.folded_image_previews.contains(&line_idx);
                    let is_hovered = editor.hovering_fold_line == Some(line_idx);
                    let preview_center_y = if is_img_folded {
                        line_top + line_height / 2.0
                    } else {
                        line_top + text_line_height + extra_height / 2.0
                    };
                    let fi_y = preview_center_y - fi_size / 2.0;
                    let fi_alpha = if is_img_folded {
                        0.75_f32
                    } else if is_hovered {
                        0.7_f32
                    } else {
                        0.0_f32
                    };
                    if fi_alpha > 0.0 {
                        let rotation_deg = if is_img_folded { 0.0_f32 } else { 90.0_f32 };
                        let mut fi_color = if is_img_folded {
                            theme.text_accent
                        } else {
                            theme.text_secondary
                        };
                        fi_color.a = fi_alpha;
                        gutter_svgs.push((
                            Bounds::new(point(px(fi_x), px(fi_y)), size(px(fi_size), px(fi_size))),
                            FOLD_SVG_PATH,
                            fi_color,
                            rotation_deg,
                        ));
                    }
                    gutter_fold_hit_areas_local.push((
                        Bounds::new(
                            point(px(fi_x - 3.0), px(fi_y - 2.0)),
                            size(px(fi_size + 6.0), px(fi_size + 4.0)),
                        ),
                        line_idx,
                        true,
                    ));
                }
            }

            // Image URL-edit icon
            if image_active {
                let ie_size = editor.scaled(13.0);
                let syntax_end_x = wrapped
                    .position_for_index(display_text.len(), px(line_height))
                    .map(|pos| f32::from(pos.x))
                    .unwrap_or(0.0);
                let ie_x = content_x + syntax_end_x + editor.scaled(10.0);
                let ie_y = text_line_top + (line_height - ie_size) / 2.0;
                let ie_bounds =
                    Bounds::new(point(px(ie_x), px(ie_y)), size(px(ie_size), px(ie_size)));
                let is_hovered = editor.hovering_image_edit_line == Some(line_idx);
                let alpha = if is_hovered { 0.55 } else { 0.7 };
                image_edit_icons.push((ie_bounds, line_idx, alpha));
                image_edit_hit_areas_local.push((ie_bounds, line_idx));
            }

            // Line highlight
            if is_cursor_line && !any_selection && editor.extra_cursors.is_empty() {
                line_highlight = Some(fill(
                    Bounds::new(
                        point(px(origin_x), px(line_top)),
                        size(px(viewport_width), px(total_line_height)),
                    ),
                    theme.line_highlight,
                ));
            }

            // Selection quads — iterate over all cursors (primary + extra)
            for sr in &all_sel_ranges {
                if sr.start == sr.end {
                    continue;
                }
                let sel_start_on_line = sr.start.max(line_start_offset);
                let sel_end_on_line = sr.end.min(line_end_offset);

                if sel_start_on_line < sel_end_on_line {
                    let buf_start_col = sel_start_on_line - line_start_offset;
                    let buf_end_col = sel_end_on_line - line_start_offset;
                    let disp_start =
                        helpers::buffer_to_display_offset(&buffer_to_display, buf_start_col);
                    let disp_end =
                        helpers::buffer_to_display_offset(&buffer_to_display, buf_end_col);
                    selection_quads.extend(helpers::build_selection_quads(
                        &display_text,
                        &runs,
                        wrapped,
                        disp_start,
                        disp_end,
                        content_x,
                        text_line_top,
                        line_height,
                        font_size,
                        indent_px,
                        theme.selection,
                        window,
                    ));
                }
                if sr.end > line_end_offset && sr.start <= line_end_offset {
                    let disp_len = display_text.len();
                    let pos_end = wrapped
                        .position_for_index(disp_len, px(line_height))
                        .unwrap_or_default();
                    let indent_end = helpers::indent_for_y(indent_px, pos_end.y, line_height);
                    selection_quads.push(fill(
                        Bounds::from_corners(
                            point(
                                px(content_x + indent_end) + pos_end.x,
                                px(text_line_top) + pos_end.y,
                            ),
                            point(
                                px(content_x + indent_end)
                                    + pos_end.x
                                    + px(editor.base_font_size() * 0.4),
                                px(text_line_top) + pos_end.y + px(line_height),
                            ),
                        ),
                        theme.selection,
                    ));
                }
            }

            // Search match quads
            if !editor.search_matches.is_empty() {
                let match_bg = editor.styles.search.search_match_bg;
                let active_bg = editor.styles.search.search_match_active_bg;
                for (match_idx, sr) in editor.search_matches.iter().enumerate() {
                    let is_active = editor.search_current_index == Some(match_idx);
                    let sel_start_on_line = sr.start.max(line_start_offset);
                    let sel_end_on_line = sr.end.min(line_end_offset);
                    if sel_start_on_line < sel_end_on_line {
                        let buf_start_col = sel_start_on_line - line_start_offset;
                        let buf_end_col = sel_end_on_line - line_start_offset;
                        let disp_start =
                            helpers::buffer_to_display_offset(&buffer_to_display, buf_start_col);
                        let disp_end =
                            helpers::buffer_to_display_offset(&buffer_to_display, buf_end_col);
                        search_match_quads.extend(helpers::build_selection_quads(
                            &display_text,
                            &runs,
                            wrapped,
                            disp_start,
                            disp_end,
                            content_x,
                            line_top,
                            line_height,
                            font_size,
                            indent_px,
                            if is_active { active_bg } else { match_bg },
                            window,
                        ));
                    }
                }
            }

            // Cursor bars — paint for all cursors (primary + extra)
            if editor.focus_handle.is_focused(window)
                && (editor.cursor_opacity > 0.0 || editor.is_selecting)
            {
                for &cur_offset in &all_cursor_offsets {
                    if cur_offset < line_start_offset || cur_offset > line_end_offset {
                        continue;
                    }
                    // For line_end_offset, only match if this is the actual last offset
                    // (handles cursor at end of line)
                    if cur_offset == line_end_offset && cur_offset != line_start_offset {
                        // cursor is at line end — show it here
                    }
                    let buf_cursor_col = cur_offset.saturating_sub(line_start_offset);
                    let buf_cursor_col = buf_cursor_col.min(line_content.len());
                    let disp_cursor_col = if buf_cursor_col >= line_content.len() {
                        display_text.len()
                    } else {
                        helpers::buffer_to_display_offset(&buffer_to_display, buf_cursor_col)
                    };

                    let cursor_pos = wrapped
                        .position_for_index(disp_cursor_col, px(line_height))
                        .unwrap_or_default();

                    let cursor_indent = helpers::indent_for_y(indent_px, cursor_pos.y, line_height);
                    let cursor_height =
                        line_height * editor.styles.cursor.cursor_height_factor.clamp(0.1, 1.0);
                    let cursor_top = text_line_top
                        + f32::from(cursor_pos.y)
                        + (line_height - cursor_height) / 2.0;
                    let mut cursor_color = theme.cursor;
                    cursor_color.a *= if editor.is_selecting {
                        editor.styles.cursor.cursor_opacity_max
                    } else {
                        editor.cursor_opacity
                    };
                    cursor_quads.push(fill(
                        Bounds::new(
                            point(px(content_x + cursor_indent) + cursor_pos.x, px(cursor_top)),
                            size(px(editor.styles.cursor.cursor_width_px), px(cursor_height)),
                        ),
                        cursor_color,
                    ));
                }
            }

            // IME marked text underline
            if let Some(ref marked) = editor.marked_range {
                let mark_start_on_line = marked.start.max(line_start_offset);
                let mark_end_on_line = marked.end.min(line_end_offset);
                if mark_start_on_line < mark_end_on_line {
                    let buf_start = mark_start_on_line - line_start_offset;
                    let buf_end = mark_end_on_line - line_start_offset;
                    let disp_start =
                        helpers::buffer_to_display_offset(&buffer_to_display, buf_start);
                    let disp_end = helpers::buffer_to_display_offset(&buffer_to_display, buf_end);
                    let pos_start = wrapped
                        .position_for_index(disp_start, px(line_height))
                        .unwrap_or_default();
                    let pos_end = wrapped
                        .position_for_index(disp_end, px(line_height))
                        .unwrap_or_default();
                    let indent_start = helpers::indent_for_y(indent_px, pos_start.y, line_height);
                    let underline_y = text_line_top + f32::from(pos_start.y) + line_height - 2.0;
                    marked_underlines.push(fill(
                        Bounds::from_corners(
                            point(px(content_x + indent_start) + pos_start.x, px(underline_y)),
                            point(
                                px(content_x + indent_start) + pos_end.x,
                                px(underline_y + 1.0),
                            ),
                        ),
                        theme.text_primary,
                    ));
                }
            }

            lines.push(PrepaintLine {
                wrapped: wrapped.clone(),
                origin,
                line_height: px(line_height),
                segments,
            });

            y += total_line_height;
        }

        self.editor.update(cx, |editor, _| {
            editor.line_display_info = display_info_cache;
            editor.last_viewport_height = viewport_height;
            editor.last_viewport_width = viewport_width;
            editor.checkbox_areas = checkbox_hit_areas.clone();
            editor.image_click_areas = image_hit_areas.clone();
            editor.gutter_fold_areas = gutter_fold_hit_areas_local.clone();
            editor.image_edit_areas = image_edit_hit_areas_local.clone();
            editor.last_editor_left = editor_left;
            editor.last_editor_width = editor_width;
            editor.last_content_left = editor_left + editor.padding_left();
            editor.last_text_width = text_width;
            editor.content_origin = bounds.origin;
        });

        // Trigger loading for any image paths not yet in cache
        let decorations_ref = self.editor.read(cx).decorations.clone();
        let mut new_image_paths: Vec<String> = Vec::new();
        for dec in &decorations_ref {
            if let Some(ref path) = dec.image_path {
                let has = self.editor.read(cx).image_cache.contains_key(path);
                if !has {
                    new_image_paths.push(path.clone());
                }
            }
        }
        if !new_image_paths.is_empty() {
            for path in new_image_paths {
                self.editor.update(cx, |editor, cx| {
                    editor.queue_image_load(path, cx);
                });
            }
        }

        EditorPrepaintState {
            lines,
            cursor_quads,
            selection_quads,
            search_match_quads,
            line_highlight,
            code_block_bgs,
            marked_underlines,
            gutter_quads,
            gutter_labels,
            gutter_svgs,
            hr_lines,
            image_previews,
            image_borders,
            table_borders,
            table_bgs,
            image_edit_icons,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.editor.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );

        for quad in prepaint.code_block_bgs.drain(..) {
            window.paint_quad(quad);
        }

        for quad in prepaint.table_bgs.drain(..) {
            window.paint_quad(quad);
        }

        if let Some(highlight) = prepaint.line_highlight.take() {
            window.paint_quad(highlight);
        }

        // Search match highlights (behind selection)
        for quad in prepaint.search_match_quads.drain(..) {
            window.paint_quad(quad);
        }

        for quad in prepaint.selection_quads.drain(..) {
            window.paint_quad(quad);
        }

        for quad in prepaint.gutter_quads.drain(..) {
            window.paint_quad(quad);
        }
        for (icon_bounds, icon_path, color, rotation_deg) in &prepaint.gutter_svgs {
            let transform = if *rotation_deg != 0.0 {
                let cx_f: f32 = icon_bounds.origin.x.into();
                let cy_f: f32 = icon_bounds.origin.y.into();
                let w_f: f32 = icon_bounds.size.width.into();
                let h_f: f32 = icon_bounds.size.height.into();
                let angle = gpui::Radians(rotation_deg.to_radians());
                TransformationMatrix::unit()
                    .translate(gpui::point(
                        gpui::ScaledPixels::from(cx_f + w_f * 0.5),
                        gpui::ScaledPixels::from(cy_f + h_f * 0.5),
                    ))
                    .rotate(angle)
                    .translate(gpui::point(
                        gpui::ScaledPixels::from(-w_f * 0.5),
                        gpui::ScaledPixels::from(-h_f * 0.5),
                    ))
            } else {
                TransformationMatrix::default()
            };
            let _ = window.paint_svg(*icon_bounds, (*icon_path).into(), transform, *color, cx);
        }
        for (shaped, origin, line_height) in &prepaint.gutter_labels {
            shaped.paint(*origin, *line_height, window, cx).ok();
        }

        for quad in prepaint.table_borders.drain(..) {
            window.paint_quad(quad);
        }

        // HR dashed lines
        for (hr_y, hr_start_x, hr_width) in &prepaint.hr_lines {
            let dash_len = 6.0_f32;
            let gap_len = 4.0_f32;
            let mut x = *hr_start_x;
            let end_x = *hr_start_x + hr_width;
            let theme = &self.editor.read(cx).theme;
            while x < end_x {
                let seg_end = (x + dash_len).min(end_x);
                window.paint_quad(fill(
                    Bounds::new(point(px(x), px(*hr_y)), size(px(seg_end - x), px(1.0))),
                    theme.border_decorative,
                ));
                x = seg_end + gap_len;
            }
        }

        for line in &prepaint.lines {
            if let Some(ref segments) = line.segments {
                for (seg_wrapped, seg_origin) in segments {
                    seg_wrapped
                        .paint(
                            *seg_origin,
                            line.line_height,
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        )
                        .ok();
                }
            } else {
                line.wrapped
                    .paint(
                        line.origin,
                        line.line_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    )
                    .ok();
            }
        }

        // Image previews
        for (img_bounds, img_data) in &prepaint.image_previews {
            window
                .paint_image(
                    *img_bounds,
                    Corners::all(px(10.0)),
                    img_data.clone(),
                    0,
                    false,
                )
                .ok();
        }

        for quad in prepaint.image_borders.drain(..) {
            window.paint_quad(quad);
        }

        // Image URL-edit icons
        for (icon_bounds, _line, alpha) in &prepaint.image_edit_icons {
            let theme = &self.editor.read(cx).theme;
            let mut color = theme.text_secondary;
            color.a = *alpha;
            let _ = window.paint_svg(
                *icon_bounds,
                OPEN_SVG_PATH.into(),
                TransformationMatrix::default(),
                color,
                cx,
            );
        }

        for cursor in prepaint.cursor_quads.drain(..) {
            window.paint_quad(cursor);
        }

        for quad in prepaint.marked_underlines.drain(..) {
            window.paint_quad(quad);
        }
    }
}
