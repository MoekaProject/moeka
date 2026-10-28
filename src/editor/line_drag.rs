use crate::ui::self_drawn::DropIndicatorControl;
use std::{
    borrow::Cow,
    ops::Range,
    time::{Duration, Instant},
};

use super::EditorView;
use crate::buffer::Buffer;
use crate::markdown::{BlockType, LineDecoration};
use gpui::{
    AnyElement, AvailableSpace, Bounds, Context, CursorStyle, Entity, MouseButton, MouseMoveEvent,
    Pixels, Point, ScrollWheelEvent, SharedString, Window, canvas, div, prelude::*, px, size,
};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

pub(crate) fn render_line_drag_overlay(editor: Entity<EditorView>) -> AnyElement {
    canvas(
        move |bounds, window, cx| {
            let mut overlay = editor.update(cx, |this, cx| this.render_line_drag(cx));
            overlay.layout_as_root(
                size(
                    AvailableSpace::Definite(bounds.size.width),
                    AvailableSpace::Definite(bounds.size.height),
                ),
                window,
                cx,
            );
            overlay.prepaint_at(bounds.origin, window, cx);
            overlay
        },
        |_, mut overlay, window, cx| overlay.paint(window, cx),
    )
    .absolute()
    .size_full()
    .into_any_element()
}

pub(crate) struct LineDrag {
    tab_id: String,
    trigger_line: usize,
    source: Range<usize>,
    blocks: DragBlocks,
    start: Point<Pixels>,
    position: Point<Pixels>,
    active: bool,
    last_wheel: Option<Instant>,
    preview: SharedString,
    ghost: Option<std::rc::Rc<super::element::LineDragGhost>>,
}

pub(crate) struct LineDropFlash {
    tab_id: String,
    lines: Range<usize>,
    visible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragBlockKind {
    Paragraph,
    Heading,
    ListItem,
    Atomic,
    IndentedCode,
    FrontMatter,
}

#[derive(Clone, Debug)]
struct DragBlock {
    lines: Range<usize>,
    kind: DragBlockKind,
}

#[derive(Default)]
struct DragBlocks {
    blocks: Vec<DragBlock>,
    owners: Vec<Option<usize>>,
}

struct LineDropTarget {
    line: usize,
    y: Pixels,
    indent: String,
}

impl DragBlocks {
    fn new(buffer: &Buffer, decorations: &[LineDecoration]) -> Self {
        let mut result = Self {
            blocks: Vec::new(),
            owners: vec![None; buffer.line_count()],
        };
        let options = Options::ENABLE_TABLES
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_MATH
            | Options::ENABLE_HEADING_ATTRIBUTES;
        let mut atomic_end = 0;
        let mut item_end = 0;
        for (event, bytes) in Parser::new_ext(buffer.text(), options).into_offset_iter() {
            if bytes.start < atomic_end {
                continue;
            }
            let kind = match event {
                Event::Start(Tag::Item) => {
                    item_end = item_end.max(bytes.end);
                    DragBlockKind::ListItem
                }
                Event::Start(Tag::Paragraph) if bytes.start >= item_end => DragBlockKind::Paragraph,
                Event::Start(Tag::Heading { .. }) => DragBlockKind::Heading,
                Event::Start(Tag::CodeBlock(code_kind)) => {
                    let line = buffer.offset_to_line_col(bytes.start).0;
                    if !decorations.get(line).is_some_and(|dec| {
                        matches!(
                            dec.block_type,
                            BlockType::CodeBlock | BlockType::CodeBlockFence
                        )
                    }) {
                        continue;
                    }
                    if matches!(code_kind, CodeBlockKind::Indented) {
                        DragBlockKind::IndentedCode
                    } else {
                        DragBlockKind::Atomic
                    }
                }
                Event::Start(
                    Tag::Table(_)
                    | Tag::BlockQuote(_)
                    | Tag::HtmlBlock
                    | Tag::FootnoteDefinition(_),
                ) => DragBlockKind::Atomic,
                Event::Rule | Event::DisplayMath(_) => DragBlockKind::Atomic,
                _ => continue,
            };
            let first = buffer.offset_to_line_col(bytes.start).0;
            let mut end = buffer.offset_to_line_col(bytes.end.saturating_sub(1)).0 + 1;
            while end > first + 1 && buffer.line_content(end - 1).trim().is_empty() {
                end -= 1;
            }
            if matches!(kind, DragBlockKind::Atomic | DragBlockKind::IndentedCode) {
                atomic_end = bytes.end;
            }
            if result.at(first).is_some_and(|block| {
                block.kind == DragBlockKind::ListItem && block.lines.start == first
            }) && kind != DragBlockKind::ListItem
            {
                continue;
            }
            result.add(first..end, kind);
        }
        for (line, decoration) in decorations.iter().enumerate().take(buffer.line_count()) {
            if let Some(info) = &decoration.table_info
                && info.table_start_line == line
            {
                result.add(
                    info.table_start_line..info.table_end_line,
                    if info.is_front_matter {
                        DragBlockKind::FrontMatter
                    } else {
                        DragBlockKind::Atomic
                    },
                );
                if info.is_front_matter {
                    for row in info.table_start_line + 1..info.table_end_line.saturating_sub(1) {
                        result.add(row..row + 1, DragBlockKind::FrontMatter);
                    }
                }
            }
            if let Some(preview) = &decoration.block_preview
                && preview.is_anchor
            {
                result.add(
                    preview.start_line..preview.end_line + 1,
                    DragBlockKind::Atomic,
                );
            }
        }
        result
    }

    fn add(&mut self, mut lines: Range<usize>, kind: DragBlockKind) {
        lines.end = lines.end.min(self.owners.len());
        if lines.start >= lines.end {
            return;
        }
        let index = self.blocks.len();
        self.owners[lines.clone()].fill(Some(index));
        self.blocks.push(DragBlock { lines, kind });
    }

    fn at(&self, line: usize) -> Option<&DragBlock> {
        self.owners
            .get(line)
            .copied()
            .flatten()
            .map(|index| &self.blocks[index])
    }

    fn source(&self, line: usize) -> Range<usize> {
        self.at(line)
            .map(|block| block.lines.clone())
            .unwrap_or(line..line + 1)
    }

    fn expand_selection(&self, mut lines: Range<usize>) -> Range<usize> {
        loop {
            let previous = lines.clone();
            for line in previous.clone() {
                let block = self.source(line);
                lines.start = lines.start.min(block.start);
                lines.end = lines.end.max(block.end);
            }
            if lines == previous {
                return lines;
            }
        }
    }

    fn boundary_allowed(&self, line: usize) -> bool {
        if line == self.owners.len() {
            return true;
        }
        self.at(line).is_none_or(|block| {
            block.lines.start == line && block.kind != DragBlockKind::FrontMatter
        })
    }

    fn boundary_allowed_for(&self, source: &Range<usize>, line: usize) -> bool {
        let metadata = self
            .blocks
            .iter()
            .find(|block| block.kind == DragBlockKind::FrontMatter && block.lines.start == 0);
        if let Some(metadata) = metadata {
            let inside = source.start > metadata.lines.start && source.end < metadata.lines.end;
            if inside {
                return line > metadata.lines.start && line < metadata.lines.end;
            }
            if source.start < metadata.lines.end {
                return false;
            }
        }
        self.boundary_allowed(line)
    }
}

impl LineDrag {
    fn update_position(&mut self, position: Point<Pixels>, threshold: f32) -> bool {
        self.position = position;
        let delta = position - self.start;
        let started = !self.active && f32::from(delta.x).hypot(f32::from(delta.y)) >= threshold;
        self.active |= started;
        started
    }
}

fn nearest_line_boundary(
    position: Point<Pixels>,
    viewport: Bounds<Pixels>,
    source: &Range<usize>,
    boundaries: impl IntoIterator<Item = (usize, Pixels)>,
) -> Option<(usize, Pixels)> {
    if !viewport.contains(&position) {
        return None;
    }
    let (line, y) = boundaries
        .into_iter()
        .filter(|(_, y)| *y >= viewport.top() && *y <= viewport.bottom())
        .min_by(|(_, left), (_, right)| {
            f32::from(position.y - *left)
                .abs()
                .total_cmp(&f32::from(position.y - *right).abs())
        })?;
    (!(source.start < line && line < source.end)).then_some((line, y))
}

impl EditorView {
    pub(crate) fn line_drag_ghost_source(&self) -> Option<Range<usize>> {
        self.line_drag
            .as_ref()
            .filter(|drag| drag.active && drag.ghost.is_none())
            .map(|drag| drag.source.clone())
    }

    pub(crate) fn set_line_drag_ghost(
        &mut self,
        ghost: std::rc::Rc<super::element::LineDragGhost>,
    ) {
        if let Some(drag) = &mut self.line_drag {
            drag.ghost = Some(ghost);
        }
    }

    pub(crate) fn line_drag_active(&self) -> bool {
        self.line_drag.as_ref().is_some_and(|drag| drag.active)
    }

    pub(crate) fn cancel_line_drag(&mut self, cx: &mut Context<Self>) -> bool {
        self.line_drop_flash_task = None;
        if self.line_drop_flash.take().is_some() {
            cx.notify();
        }
        self.line_drag_scroll_task = None;
        if self.line_drag.take().is_some() {
            cx.notify();
            true
        } else {
            false
        }
    }

    pub(crate) fn begin_line_drag(
        &mut self,
        line: usize,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab_is_read_only()
            || !self.active_tab_is_markdown()
            || self.context_menu.is_some()
            || self.is_selecting
            || self.embedded_editor
            || line >= self.buffer.line_count()
        {
            return;
        }
        let Some(tab_id) = self.active_tab_id.clone() else {
            return;
        };
        self.line_drop_flash = None;
        self.line_drop_flash_task = None;
        let blocks = DragBlocks::new(&self.buffer, &self.decorations);
        let mut source = blocks.source(line);
        let selected = self.cursor.selection_range();
        if !selected.is_empty() {
            let first = self.buffer.offset_to_line_col(selected.start).0;
            let last = self
                .buffer
                .offset_to_line_col(selected.end.saturating_sub(1))
                .0;
            if first != last && (first..=last).contains(&line) {
                source = blocks.expand_selection(first..last + 1);
            }
        }
        if self.folded_sections.contains(&line)
            && let Some(range) = self.fold_range_for_line(line)
        {
            source.end = range.end;
        }
        source.end = source.end.min(self.buffer.line_count());
        let preview = (source.start..source.end.min(source.start + 3))
            .map(|line| {
                self.buffer
                    .line_content(line)
                    .chars()
                    .take(100)
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        self.line_drag = Some(LineDrag {
            tab_id,
            trigger_line: line,
            source,
            blocks,
            start: position,
            position,
            active: false,
            last_wheel: None,
            preview: preview.into(),
            ghost: None,
        });
        cx.notify();
    }

    fn line_drag_valid(&self) -> bool {
        self.line_drag.as_ref().is_some_and(|drag| {
            self.active_tab_id.as_deref() == Some(drag.tab_id.as_str())
                && self.active_tab_is_markdown()
                && !self.active_tab_is_read_only()
        })
    }

    fn line_drag_target(&self, position: Point<Pixels>) -> Option<LineDropTarget> {
        let drag = self.line_drag.as_ref()?;
        let viewport = self.line_format_toolbar_geometries.first()?.viewport;
        let boundaries =
            self.format_line_areas
                .iter()
                .enumerate()
                .filter(|(_, (bounds, _))| {
                    bounds.size.height > px(0.0)
                        && bounds.bottom() >= viewport.top()
                        && bounds.top() <= viewport.bottom()
                })
                .flat_map(|(index, (bounds, line))| {
                    let mut after = line + 1;
                    if self.folded_sections.contains(line) {
                        after = self
                            .fold_range_for_line(*line)
                            .map(|range| range.end)
                            .unwrap_or(after);
                    }
                    if let Some(info) = self
                        .decorations
                        .get(*line)
                        .and_then(|dec| dec.table_info.as_ref())
                    {
                        after = if info.is_front_matter {
                            line + 1
                        } else {
                            info.table_end_line
                        };
                    }
                    if let Some(preview) = self
                        .decorations
                        .get(*line)
                        .and_then(|dec| dec.block_preview.as_ref())
                        && preview.is_anchor
                    {
                        after = preview.end_line + 1;
                    }
                    if let Some((_, next_line)) = self.format_line_areas[index + 1..].iter().find(
                        |(next_bounds, next_line)| {
                            *next_line > *line && next_bounds.size.height > px(0.0)
                        },
                    ) {
                        after = *next_line;
                    }
                    [
                        (*line, bounds.top()),
                        (after.min(self.buffer.line_count()), bounds.bottom()),
                    ]
                });
        let (line, y) = nearest_line_boundary(
            position,
            viewport,
            &drag.source,
            boundaries.filter(|(line, _)| drag.blocks.boundary_allowed_for(&drag.source, *line)),
        )?;
        let steps = (f32::from(position.x - drag.start.x)
            / self
                .scaled(self.styles.format_toolbar.line_drag_indent_step_px)
                .max(1.0))
        .trunc() as i32;
        let indent = if drag
            .blocks
            .at(drag.source.start)
            .is_some_and(|block| block.kind == DragBlockKind::FrontMatter)
        {
            leading_indent(self.buffer.line_content(drag.source.start)).to_string()
        } else {
            drop_indent(&self.buffer, &drag.blocks, &drag.source, line, steps)
        };
        if (line == drag.source.start || line == drag.source.end)
            && indent_columns(&indent)
                == source_indent_columns(&self.buffer, &drag.blocks, drag.source.start)
        {
            return None;
        }
        Some(LineDropTarget { line, y, indent })
    }

    pub(crate) fn update_line_drag(
        &mut self,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.line_drag.is_none() {
            return false;
        }
        if event.pressed_button != Some(MouseButton::Left) || !self.line_drag_valid() {
            self.cancel_line_drag(cx);
            return true;
        }
        let threshold = self.scaled(self.styles.format_toolbar.line_drag_threshold_px);
        let drag = self.line_drag.as_mut().unwrap();
        let started = drag.update_position(event.position, threshold);
        if started {
            self.dismiss_line_format_toolbar(cx);
        }
        self.sync_line_drag_scroll(cx);
        cx.notify();
        true
    }

    fn sync_line_drag_scroll(&mut self, cx: &mut Context<Self>) {
        if !self.line_drag_active() || self.line_drag_scroll_task.is_some() {
            return;
        }
        let pane_id = self.current_pane_id();
        self.line_drag_scroll_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
                let keep = this
                    .update(cx, |this, cx| {
                        if this.focused_pane_id != pane_id {
                            return false;
                        }
                        if !this.line_drag_valid() || !this.line_drag_active() {
                            return false;
                        }
                        if this
                            .line_drag
                            .as_ref()
                            .and_then(|drag| drag.last_wheel)
                            .is_some_and(|last| last.elapsed() < Duration::from_millis(180))
                        {
                            return true;
                        }
                        let position = this.line_drag.as_ref().unwrap().position;
                        let Some(viewport) = this
                            .line_format_toolbar_geometries
                            .first()
                            .map(|geometry| geometry.viewport)
                        else {
                            return false;
                        };
                        if position.x < viewport.left() || position.x > viewport.right() {
                            return true;
                        }
                        if let Some(delta) = this.selection_drag_scroll_delta_for_position(position)
                        {
                            let previous = this.scroll_offset;
                            this.scroll_offset += delta.clamp(-48.0, 48.0);
                            this.clamp_scroll();
                            if this.scroll_offset != previous {
                                cx.notify();
                            }
                        }
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        }));
    }

    pub(crate) fn finish_line_drag(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.line_drag.is_none() {
            return false;
        }
        self.line_drag_scroll_task = None;
        if !self.line_drag_valid() {
            return self.cancel_line_drag(cx);
        }
        let destination = self.line_drag_target(position);
        let drag = self.line_drag.take().unwrap();
        if !drag.active {
            if self
                .format_trigger_areas
                .iter()
                .any(|(bounds, line)| *line == drag.trigger_line && bounds.contains(&position))
            {
                self.toggle_line_format_toolbar(drag.trigger_line, cx);
            }
        } else if let Some(destination) = destination {
            let base_indent = source_indent_columns(&self.buffer, &drag.blocks, drag.source.start);
            let separate_blocks = drag.blocks.at(drag.source.start).is_some_and(|block| {
                !matches!(
                    block.kind,
                    DragBlockKind::ListItem | DragBlockKind::FrontMatter
                )
            });
            self.move_dragged_lines(drag.source, destination, base_indent, separate_blocks, cx);
        }
        cx.notify();
        true
    }

    fn move_dragged_lines(
        &mut self,
        source: Range<usize>,
        target: LineDropTarget,
        base_indent: usize,
        separate_blocks: bool,
        cx: &mut Context<Self>,
    ) {
        let destination = target.line;
        let Some(movement) = line_move_with_indent(
            &self.buffer,
            source.clone(),
            destination,
            &target.indent,
            base_indent,
            separate_blocks,
        ) else {
            return;
        };
        if !self.begin_content_edit(cx) {
            return;
        }
        let ordered_before = if self
            .decorations
            .get(source.start)
            .and_then(|row| row.table_info.as_ref())
            .is_some_and(|table| table.is_front_matter)
        {
            Vec::new()
        } else {
            ordered_drag_lists(&self.buffer)
        };
        let folded_ends: Vec<_> = self
            .folded_section_ends
            .iter()
            .map(|(&anchor, &end)| {
                let (line, column) = self.buffer.offset_to_line_col(end);
                (
                    movement.remap_line(anchor),
                    movement.remap_line(line),
                    column,
                )
            })
            .collect();
        self.checkpoint();
        self.buffer.replace(movement.range.clone(), &movement.text);
        renumber_dragged_lists(&mut self.buffer, &ordered_before, &source, &movement);
        self.cursor.move_to(
            self.buffer.line_start(movement.inserted_lines.start),
            &self.buffer,
        );
        self.clear_extra_cursors();
        self.clear_wrap_boundary_affinity();
        self.folded_sections = self
            .folded_sections
            .iter()
            .map(|line| movement.remap_line(*line))
            .collect();
        self.folded_image_previews = self
            .folded_image_previews
            .iter()
            .map(|line| movement.remap_line(*line))
            .collect();
        self.folded_section_ends = folded_ends
            .into_iter()
            .map(|(anchor, line, column)| (anchor, self.buffer.line_col_to_offset(line, column)))
            .collect();
        self.selected_handle = None;
        self.table_boundary_caret = None;
        self.mark_active_tab_dirty();
        self.reparse();
        self.scroll_to_cursor();
        self.flash_dropped_lines(movement.inserted_lines, cx);
        self.pause_blinking(cx);
    }

    fn flash_dropped_lines(&mut self, lines: Range<usize>, cx: &mut Context<Self>) {
        let Some(tab_id) = self.active_tab_id.clone() else {
            return;
        };
        let pane_id = self.current_pane_id();
        let show_ms = self.styles.outline.outline_flash_show_duration_ms;
        let gap_ms = self.styles.outline.outline_flash_gap_duration_ms;
        self.line_drop_flash = Some(LineDropFlash {
            tab_id: tab_id.clone(),
            lines,
            visible: true,
        });
        self.line_drop_flash_task = Some(cx.spawn(async move |this, cx| {
            for (delay, visible) in [
                (show_ms, Some(false)),
                (gap_ms, Some(true)),
                (show_ms, None),
            ] {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(delay))
                    .await;
                let keep = this
                    .update(cx, |this, cx| {
                        this.with_pane_if_alive(pane_id, |this| {
                            if this.active_tab_id.as_ref() != Some(&tab_id) {
                                return false;
                            }
                            let Some(flash) = this.line_drop_flash.as_mut() else {
                                return false;
                            };
                            if let Some(visible) = visible {
                                flash.visible = visible;
                            } else {
                                this.line_drop_flash = None;
                                this.line_drop_flash_task = None;
                            }
                            cx.notify();
                            true
                        })
                        .unwrap_or(false)
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        }));
        cx.notify();
    }

    pub(crate) fn paint_line_drag_feedback(&self, viewport: Bounds<Pixels>, window: &mut Window) {
        let styles = &self.styles.format_toolbar;
        let dimmed = self
            .line_drag
            .as_ref()
            .filter(|drag| drag.active && self.active_tab_id.as_ref() == Some(&drag.tab_id))
            .map(|drag| {
                (
                    &drag.source,
                    1.0 - styles.line_drag_source_opacity,
                    self.styles.editor.editor_bg,
                )
            });
        let flashed = self
            .line_drop_flash
            .as_ref()
            .filter(|flash| flash.visible && self.active_tab_id.as_ref() == Some(&flash.tab_id))
            .map(|flash| {
                (
                    &flash.lines,
                    styles.line_drop_flash_opacity,
                    styles.line_drag_indicator_color,
                )
            });
        for (lines, opacity, color) in dimmed.into_iter().chain(flashed) {
            let bounds = self
                .format_line_areas
                .iter()
                .filter(|(_, line)| lines.contains(line))
                .map(|(bounds, _)| bounds.intersect(&viewport))
                .filter(|bounds| bounds.size.height > px(0.0) && bounds.size.width > px(0.0))
                .reduce(|left, right| left.union(&right));
            if let Some(bounds) = bounds {
                DropIndicatorControl::feedback(bounds, color, opacity, window);
            }
        }
    }

    fn render_line_drag(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(drag) = self.line_drag.as_ref().filter(|drag| drag.active) else {
            return div().into_any_element();
        };
        let styles = &self.styles.format_toolbar;
        let border = self.styles.window.window_border_width_px;
        let ghost_width = drag
            .ghost
            .as_ref()
            .map(|ghost| f32::from(ghost.bounds.size.width))
            .unwrap_or(self.last_text_width)
            .max(1.0);
        let ghost_height = drag
            .ghost
            .as_ref()
            .map(|ghost| f32::from(ghost.bounds.size.height))
            .unwrap_or(self.scaled(self.styles.self_drawn.line_drag_fallback_height_px));
        let ghost_left = (f32::from(drag.position.x)
            + self.styles.self_drawn.line_drag_pointer_offset_px
            - border)
            .clamp(
                0.0,
                (self.last_window_width
                    - ghost_width
                    - self.styles.self_drawn.line_drag_viewport_margin_px)
                    .max(0.0),
            );
        let ghost_top = (f32::from(drag.position.y)
            + self.styles.self_drawn.line_drag_pointer_offset_px
            - border)
            .clamp(
                0.0,
                (self.last_window_height
                    - ghost_height
                    - self.styles.self_drawn.line_drag_viewport_margin_px)
                    .max(0.0),
            );
        let indicator = self.line_drag_target(drag.position).map(|target| {
            let y = target.y;
            let viewport = self
                .line_format_toolbar_geometries
                .first()
                .unwrap()
                .viewport;
            let indent_px = indent_columns(&target.indent) as f32 * self.base_font_size() * 0.5;
            let right =
                (self.last_content_left + self.last_text_width).min(f32::from(viewport.right()));
            let left = (self.last_content_left + indent_px)
                .max(f32::from(viewport.left()))
                .min(right - 1.0);
            DropIndicatorControl::line(
                left - border,
                (y - px(border + styles.line_drag_indicator_width_px / 2.0)).clamp(
                    viewport.top() - px(border),
                    viewport.bottom() - px(border + styles.line_drag_indicator_width_px),
                ),
                (right - left).max(1.0),
                styles.line_drag_indicator_width_px,
                styles.line_drag_indicator_color,
            )
        });
        let ghost_content = if let Some(ghost) = drag.ghost.clone() {
            canvas(
                |_, _, _| (),
                move |bounds, _, window, cx| ghost.paint(bounds.origin, window, cx),
            )
            .size_full()
            .into_any_element()
        } else {
            div()
                .size_full()
                .text_size(px(self.base_font_size()))
                .child(drag.preview.clone())
                .into_any_element()
        };
        div()
            .absolute()
            .size_full()
            .occlude()
            .cursor(CursorStyle::ClosedHand)
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                if this.line_drag_active()
                    && this.line_drag_valid()
                    && this
                        .line_format_toolbar_geometries
                        .first()
                        .is_some_and(|geometry| geometry.viewport.contains(&event.position))
                {
                    if let Some(drag) = &mut this.line_drag {
                        drag.position = event.position;
                        drag.last_wheel = Some(Instant::now());
                    }
                    this.on_scroll(event, window, cx);
                }
                cx.stop_propagation();
            }))
            .children(indicator)
            .child(
                div()
                    .absolute()
                    .left(px(ghost_left))
                    .top(px(ghost_top))
                    .w(px(ghost_width))
                    .h(px(ghost_height))
                    .overflow_hidden()
                    .bg(self.styles.editor.editor_bg)
                    .opacity(styles.line_drag_ghost_opacity)
                    .child(ghost_content),
            )
            .into_any_element()
    }
}

struct LineMove {
    range: Range<usize>,
    text: String,
    inserted_lines: Range<usize>,
    first_line: usize,
    mapped_lines: Vec<usize>,
    added_lines: usize,
}

impl LineMove {
    fn remap_line(&self, line: usize) -> usize {
        if line < self.first_line {
            line
        } else if let Some(mapped) = self.mapped_lines.get(line - self.first_line) {
            *mapped
        } else {
            line + self.added_lines
        }
    }
}

struct OrderedDragList {
    lines: Range<usize>,
    start: u64,
    items: Vec<(usize, Range<usize>)>,
    item_ends: Vec<usize>,
}

fn ordered_drag_lists(buffer: &Buffer) -> Vec<OrderedDragList> {
    let mut lists: Vec<OrderedDragList> = Vec::new();
    let mut stack = Vec::new();
    for (event, range) in Parser::new_ext(
        buffer.text(),
        Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES,
    )
    .into_offset_iter()
    {
        match event {
            Event::Start(Tag::List(start)) => {
                stack.push(start.map(|start| {
                    let index = lists.len();
                    lists.push(OrderedDragList {
                        lines: buffer.offset_to_line_col(range.start).0
                            ..buffer.offset_to_line_col(range.end.saturating_sub(1)).0 + 1,
                        start,
                        items: Vec::new(),
                        item_ends: Vec::new(),
                    });
                    index
                }));
            }
            Event::End(TagEnd::List(_)) => {
                stack.pop();
            }
            Event::Start(Tag::Item) => {
                if let Some(Some(index)) = stack.last() {
                    let line = buffer.offset_to_line_col(range.start).0;
                    let text = buffer.line_content(line);
                    let quote = crate::markdown::blockquote_prefix_len(text);
                    let indent = leading_indent(&text[quote..]).len();
                    let start = quote + indent;
                    let digits = text[start..].bytes().take_while(u8::is_ascii_digit).count();
                    if digits > 0
                        && matches!(text.as_bytes().get(start + digits), Some(b'.' | b')'))
                    {
                        let offset = buffer.line_start(line) + start;
                        lists[*index].items.push((line, offset..offset + digits));
                        lists[*index]
                            .item_ends
                            .push(buffer.offset_to_line_col(range.end.saturating_sub(1)).0 + 1);
                    }
                }
            }
            _ => {}
        }
    }
    lists
}

fn renumber_dragged_lists(
    buffer: &mut Buffer,
    before: &[OrderedDragList],
    source: &Range<usize>,
    movement: &LineMove,
) {
    let source_gap = if source.end < movement.first_line + movement.mapped_lines.len() {
        movement.remap_line(source.end)
    } else if source.start > 0 {
        movement.remap_line(source.start - 1) + 1
    } else {
        0
    };
    let mut edits = Vec::new();
    let mut indent_adjustments = std::collections::BTreeMap::<usize, isize>::new();
    for list in ordered_drag_lists(buffer) {
        let touches_drop = list.lines.start <= movement.inserted_lines.end
            && list.lines.end >= movement.inserted_lines.start;
        if !touches_drop && !list.lines.contains(&source_gap) && list.lines.end != source_gap {
            continue;
        }
        let preserved = before
            .iter()
            .filter_map(|old| {
                let retained = old
                    .items
                    .iter()
                    .filter(|(line, _)| {
                        !source.contains(line)
                            && list
                                .items
                                .iter()
                                .any(|(new_line, _)| *new_line == movement.remap_line(*line))
                    })
                    .count();
                (retained > 0).then_some((retained, old.start))
            })
            .max_by_key(|(retained, _)| *retained);
        let start = preserved.map(|(_, start)| start).unwrap_or(list.start);
        for (index, (line, digits)) in list.items.iter().enumerate() {
            let expected = start.saturating_add(index as u64).to_string();
            if buffer.text_for_range(digits.clone()) != expected {
                let difference = expected.len() as isize - digits.len() as isize;
                if difference != 0 {
                    let content = buffer.line_content(*line);
                    let quote = crate::markdown::blockquote_prefix_len(content);
                    let body = &content[quote..];
                    let prefix_indent = leading_indent(body);
                    let marker_width =
                        crate::markdown::ordered_list_prefix_len(&body[prefix_indent.len()..]);
                    let content_column =
                        indent_columns(&body[..prefix_indent.len() + marker_width]);
                    for child in line + 1..list.item_ends[index] {
                        let text = buffer.line_content(child);
                        let quote = crate::markdown::blockquote_prefix_len(text);
                        if indent_columns(leading_indent(&text[quote..])) >= content_column
                            && !text.trim().is_empty()
                        {
                            *indent_adjustments.entry(child).or_default() += difference;
                        }
                    }
                }
                edits.push((digits.clone(), expected));
            }
        }
    }
    for (line, difference) in indent_adjustments {
        if difference == 0 {
            continue;
        }
        let text = buffer.line_content(line);
        let quote = crate::markdown::blockquote_prefix_len(text);
        let indent = leading_indent(&text[quote..]);
        let replacement = if difference > 0 {
            format!("{indent}{}", " ".repeat(difference as usize))
        } else {
            strip_indent_columns(indent, difference.unsigned_abs()).into_owned()
        };
        let start = buffer.line_start(line) + quote;
        edits.push((start..start + indent.len(), replacement));
    }
    edits.sort_by_key(|(range, _)| range.start);
    for (range, replacement) in edits.into_iter().rev() {
        buffer.replace(range, &replacement);
    }
}

fn leading_indent(text: &str) -> &str {
    &text[..text.len() - text.trim_start_matches([' ', '\t']).len()]
}

fn indent_columns(indent: &str) -> usize {
    indent.bytes().fold(0, |column, byte| {
        if byte == b'\t' {
            column + 4 - column % 4
        } else {
            column + 1
        }
    })
}

fn strip_indent_columns(text: &str, width: usize) -> Cow<'_, str> {
    let mut columns = 0;
    let mut end = 0;
    for byte in leading_indent(text).bytes() {
        if columns >= width {
            break;
        }
        columns += if byte == b'\t' { 4 - columns % 4 } else { 1 };
        end += 1;
    }
    if columns > width {
        Cow::Owned(format!("{}{}", " ".repeat(columns - width), &text[end..]))
    } else {
        Cow::Borrowed(&text[end..])
    }
}

fn source_indent_columns(buffer: &Buffer, blocks: &DragBlocks, line: usize) -> usize {
    let columns = indent_columns(leading_indent(buffer.line_content(line)));
    if blocks
        .at(line)
        .is_some_and(|block| block.kind == DragBlockKind::IndentedCode)
    {
        columns.saturating_sub(4)
    } else {
        columns
    }
}

fn child_indent(text: &str) -> Option<String> {
    let prefix = leading_indent(text);
    let rest = &text[prefix.len()..];
    let marker_len = crate::markdown::ordered_list_prefix_len(rest);
    let marker_len = if marker_len > 0 {
        marker_len
    } else {
        let bytes = rest.as_bytes();
        if !matches!(bytes.first(), Some(b'-' | b'+' | b'*'))
            || !matches!(bytes.get(1), Some(b' ' | b'\t'))
        {
            return None;
        }
        1 + bytes[1..]
            .iter()
            .take_while(|byte| matches!(byte, b' ' | b'\t'))
            .count()
    };
    let columns = indent_columns(&text[..prefix.len() + marker_len]);
    let extra = columns.saturating_sub(indent_columns(prefix));
    Some(format!("{prefix}{}", " ".repeat(extra.max(2))))
}

fn drop_indent(
    buffer: &Buffer,
    blocks: &DragBlocks,
    source: &Range<usize>,
    destination: usize,
    horizontal_steps: i32,
) -> String {
    let neighbor = [
        (destination < buffer.line_count()).then_some(destination),
        destination.checked_sub(1),
    ]
    .into_iter()
    .flatten()
    .filter(|line| !source.contains(line))
    .find(|line| !buffer.line_content(*line).trim().is_empty());
    let neighbor = if destination == source.start || destination == source.end {
        Some(source.start)
    } else {
        neighbor
    };
    let base = neighbor
        .map(|line| {
            let line = blocks
                .at(line)
                .map(|block| block.lines.start)
                .unwrap_or(line);
            let indent = leading_indent(buffer.line_content(line));
            if blocks
                .at(line)
                .is_some_and(|block| block.kind == DragBlockKind::IndentedCode)
            {
                " ".repeat(source_indent_columns(buffer, blocks, line))
            } else {
                indent.to_string()
            }
        })
        .unwrap_or_default();
    let mut candidates = vec![String::new(), base.clone()];
    for block in &blocks.blocks {
        if block.kind != DragBlockKind::ListItem
            || source.contains(&block.lines.start)
            || block.lines.start >= destination
            || block.lines.end < destination
        {
            continue;
        }
        let text = buffer.line_content(block.lines.start);
        candidates.push(leading_indent(text).to_string());
        if let Some(indent) = child_indent(text) {
            candidates.push(indent);
        }
    }
    candidates.sort_by_key(|indent| indent_columns(indent));
    candidates.dedup_by_key(|indent| indent_columns(indent));
    let base_index = candidates
        .iter()
        .position(|indent| indent_columns(indent) == indent_columns(&base))
        .unwrap_or(0);
    let index = (base_index as i64 + i64::from(horizontal_steps))
        .clamp(0, candidates.len() as i64 - 1) as usize;
    if index == base_index {
        base
    } else {
        candidates.swap_remove(index)
    }
}

#[cfg(test)]
fn moved_line(line: usize, source: &Range<usize>, destination: usize) -> usize {
    let count = source.end - source.start;
    if source.contains(&line) {
        let start = if destination <= source.start {
            destination
        } else {
            destination - count
        };
        start + line - source.start
    } else if destination < source.start && (destination..source.start).contains(&line) {
        line + count
    } else if destination > source.end && (source.end..destination).contains(&line) {
        line - count
    } else {
        line
    }
}

#[cfg(test)]
fn line_move(buffer: &Buffer, source: Range<usize>, destination: usize) -> Option<LineMove> {
    let blocks = DragBlocks::new(buffer, &crate::markdown::parse_decorations(buffer.text()));
    let indent = drop_indent(buffer, &blocks, &source, destination, 0);
    let base = source_indent_columns(buffer, &blocks, source.start);
    line_move_with_indent(buffer, source, destination, &indent, base, false)
}

fn line_move_with_indent(
    buffer: &Buffer,
    source: Range<usize>,
    destination: usize,
    target_indent: &str,
    base_indent: usize,
    separate_blocks: bool,
) -> Option<LineMove> {
    if source.is_empty()
        || source.end > buffer.line_count()
        || destination > buffer.line_count()
        || (source.start < destination && destination < source.end)
    {
        return None;
    }
    let moved_start = source.start.min(destination);
    let moved_end = source.end.max(destination);
    let start = moved_start.saturating_sub(1);
    let end = (moved_end + 1).min(buffer.line_count());
    let range = buffer.line_start(start)..buffer.line_end(end - 1);
    let mut lines = (start..end)
        .map(|line| {
            let text = buffer.line_content(line);
            if source.contains(&line) && !text.trim().is_empty() {
                let content = strip_indent_columns(text, base_indent);
                (line, Cow::Owned(format!("{target_indent}{content}")))
            } else {
                (line, Cow::Borrowed(text))
            }
        })
        .collect::<Vec<_>>();
    if destination < source.start {
        lines[moved_start - start..moved_end - start].rotate_right(source.len());
    } else {
        lines[moved_start - start..moved_end - start].rotate_left(source.len());
    }
    let mut mapped_lines = vec![0; end - start];
    let mut output = Vec::with_capacity(lines.len() + 3);
    for (index, (line, text)) in lines.iter().enumerate() {
        if separate_blocks
            && destination != source.start
            && destination != source.end
            && let Some((previous_line, previous_text)) =
                index.checked_sub(1).map(|previous| &lines[previous])
            && !text.trim().is_empty()
            && !previous_text.trim().is_empty()
            && (source.contains(line) != source.contains(previous_line)
                || (!source.contains(line) && *line != previous_line + 1))
        {
            output.push("");
        }
        mapped_lines[line - start] = start + output.len();
        output.push(text.as_ref());
    }
    let added_lines = output.len() - lines.len();
    let text = output.join("\n");
    if text == buffer.text_for_range(range.clone()) {
        return None;
    }
    Some(LineMove {
        range,
        text,
        inserted_lines: {
            let first = mapped_lines[source.start - start];
            first..first + source.len()
        },
        first_line: start,
        mapped_lines,
        added_lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_rows_drag_only_inside_the_delimiters() {
        let buffer = Buffer::from_text("---\na: one\nb: two\n---\nbody");
        let blocks = DragBlocks::new(&buffer, &crate::markdown::parse_decorations(buffer.text()));
        assert_eq!(blocks.source(1), 1..2);
        assert!(blocks.boundary_allowed_for(&(1..2), 3));
        assert!(!blocks.boundary_allowed_for(&(1..2), 0));
        assert!(!blocks.boundary_allowed_for(&(1..2), 4));
        assert!(!blocks.boundary_allowed_for(&(4..5), 2));
        let movement = line_move_with_indent(&buffer, 1..2, 3, "", 0, false).unwrap();
        let mut edited = Buffer::from_text(buffer.text());
        edited.replace(movement.range, &movement.text);
        assert_eq!(edited.text(), "---\nb: two\na: one\n---\nbody");
    }

    #[test]
    fn ordered_list_drag_renumbers_destination_and_source_in_one_transaction() {
        for (text, source, destination, expected) in [
            (
                "1. one\n2. two\n3. three",
                2..3,
                1,
                "1. one\n2. three\n3. two",
            ),
            (
                "1. one\n2. two\n3. three",
                2..3,
                0,
                "1. three\n2. one\n3. two",
            ),
            (
                "5. one\n6. two\n7. three",
                0..1,
                3,
                "5. two\n6. three\n7. one",
            ),
            (
                "8. one\n9. two\n   - child\n10. three",
                3..4,
                1,
                "8. one\n9. three\n10. two\n    - child",
            ),
            (
                "1) one\n2) two\n\n5) other\n6) last",
                1..2,
                5,
                "1) one\n\n2) other\n3) last\n4) two",
            ),
            (
                "1) one\n2) two\n\ntext\n\n5) other\n6) last",
                1..2,
                7,
                "1) one\n\ntext\n\n5) other\n6) last\n7) two",
            ),
            (
                "1. one\n   7. child\n   8. other\n2. two\n3. three",
                4..5,
                0,
                "1. three\n2. one\n   7. child\n   8. other\n3. two",
            ),
        ] {
            let mut buffer = Buffer::from_text(text);
            let before = ordered_drag_lists(&buffer);
            let movement =
                line_move_with_indent(&buffer, source.clone(), destination, "", 0, false).unwrap();
            buffer.begin_edit_recording();
            buffer.replace(movement.range.clone(), &movement.text);
            renumber_dragged_lists(&mut buffer, &before, &source, &movement);
            assert_eq!(buffer.text(), expected);
            let edits = buffer.finish_edit_recording();
            assert!(buffer.apply_edits_inverse(&edits));
            assert_eq!(buffer.text(), text);
            assert!(buffer.apply_edits_forward(&edits));
            assert_eq!(buffer.text(), expected);
        }
    }

    #[test]
    fn press_is_pending_until_threshold_and_drag_never_becomes_a_click() {
        let start = gpui::point(px(100.0), px(100.0));
        let mut drag = LineDrag {
            tab_id: "test".into(),
            trigger_line: 0,
            source: 0..1,
            blocks: DragBlocks::default(),
            start,
            position: start,
            active: false,
            last_wheel: None,
            preview: "test".into(),
            ghost: None,
        };
        assert!(!drag.active);
        assert!(!drag.update_position(gpui::point(px(102.0), px(101.0)), 4.0));
        assert!(!drag.active);
        assert!(drag.update_position(gpui::point(px(105.0), px(100.0)), 4.0));
        assert!(drag.active);
        assert!(!drag.update_position(start, 4.0));
        assert!(drag.active);
    }

    #[test]
    fn drop_targets_allow_source_edges_for_reindent_and_reject_other_panes() {
        let viewport = Bounds::new(
            gpui::point(px(100.0), px(50.0)),
            gpui::size(px(400.0), px(400.0)),
        );
        let boundaries = [(0, px(60.0)), (1, px(90.0)), (5, px(350.0)), (6, px(380.0))];
        assert_eq!(
            nearest_line_boundary(
                gpui::point(px(140.0), px(342.0)),
                viewport,
                &(0..1),
                boundaries
            ),
            Some((5, px(350.0)))
        );
        assert_eq!(
            nearest_line_boundary(
                gpui::point(px(140.0), px(70.0)),
                viewport,
                &(0..1),
                boundaries
            ),
            Some((0, px(60.0)))
        );
        assert_eq!(
            nearest_line_boundary(
                gpui::point(px(540.0), px(342.0)),
                viewport,
                &(0..1),
                boundaries
            ),
            None
        );
        assert_eq!(
            nearest_line_boundary(
                gpui::point(px(140.0), px(400.0)),
                viewport,
                &(0..1),
                boundaries
            ),
            Some((6, px(380.0)))
        );
    }

    #[test]
    fn line_moves_preserve_text_and_can_be_undone_as_one_edit() {
        for (text, source, destination, expected) in [
            ("one\ntwo\nthree", 0..1, 3, "two\nthree\none"),
            ("one\ntwo\nthree", 2..3, 0, "three\none\ntwo"),
            ("one\ntwo\nthree\n", 0..1, 3, "two\nthree\none\n"),
            ("\ntext\n", 0..1, 2, "text\n\n"),
            (
                "\u{4e2d}\u{6587}\n\u{1f600}\nend",
                1..2,
                0,
                "\u{1f600}\n\u{4e2d}\u{6587}\nend",
            ),
            (
                "before\n```\ncode\n```\nafter",
                1..4,
                5,
                "before\nafter\n```\ncode\n```",
            ),
        ] {
            let mut buffer = Buffer::from_text(text);
            let movement = line_move(&buffer, source, destination).unwrap();
            buffer.begin_edit_recording();
            buffer.replace(movement.range, &movement.text);
            assert_eq!(buffer.text(), expected);
            let edits = buffer.finish_edit_recording();
            assert_eq!(edits.len(), 1);
            assert!(buffer.apply_edits_inverse(&edits));
            assert_eq!(buffer.text(), text);
            assert!(buffer.apply_edits_forward(&edits));
            assert_eq!(buffer.text(), expected);
        }
    }

    #[test]
    fn source_and_adjacent_boundaries_are_no_ops() {
        let buffer = Buffer::from_text("a\nb\nc\nd");
        for destination in 1..=3 {
            assert!(line_move(&buffer, 1..3, destination).is_none());
        }
        assert!(line_move(&buffer, 1..3, 5).is_none());
        assert!(line_move(&buffer, 1..5, 0).is_none());
    }

    #[test]
    fn semantic_drag_ranges_preserve_markdown_blocks() {
        for (text, line, expected) in [
            ("# heading\nbody\n\n# next", 0, 0..1),
            ("first **bold**\nsecond [[wiki]]\n\nend", 1, 0..2),
            ("Title\n=====\n\nbody", 1, 0..2),
            ("- parent\n  - child\n    continuation\n- peer", 0, 0..3),
            ("- parent\n  - child\n    continuation\n- peer", 1, 1..3),
            (
                "- parent\n  - child\n    continuation\n  tail\n- peer",
                3,
                1..4,
            ),
            ("1. ordered\n   continuation\n2. peer", 0, 0..2),
            ("- [ ] task\n  - [x] child\n- peer", 0, 0..2),
            ("before\n\n```rust\ncode\n```\n\nafter", 3, 2..5),
            ("~~~\ncode\n~~~\n\nafter", 2, 0..3),
            ("    code\n    more\n\nafter", 1, 0..2),
            ("| a | b |\n| - | - |\n| c | d |\n\nafter", 1, 0..3),
            ("> [!TIP]\n> advice\n> - nested\n\nafter", 1, 0..3),
            ("[^note]: text\n    more\n\nafter", 1, 0..2),
            ("<div>\ncontent\n</div>\n\nafter", 1, 0..3),
            ("$$\nx + y\n$$\n\nafter", 1, 0..3),
            ("---\ntitle: Example\n---\n\nbody", 1, 1..2),
            ("![image](path.png)\n\n---\n", 0, 0..1),
        ] {
            let buffer = Buffer::from_text(text);
            let blocks = DragBlocks::new(&buffer, &crate::markdown::parse_decorations(text));
            assert_eq!(blocks.source(line), expected, "{text}");
        }
    }

    #[test]
    fn atomic_blocks_reject_drop_boundaries_inside_their_syntax() {
        let text = "before\n\n```\ncode\n```\n\nafter";
        let buffer = Buffer::from_text(text);
        let blocks = DragBlocks::new(&buffer, &crate::markdown::parse_decorations(text));
        assert!(blocks.boundary_allowed(2));
        assert!(!blocks.boundary_allowed(3));
        assert!(!blocks.boundary_allowed(4));
        assert!(blocks.boundary_allowed(5));
    }

    #[test]
    fn selections_expand_to_complete_blocks_and_front_matter_stays_at_the_top() {
        let text = "---\ntitle: Example\n---\n\nparagraph\ncontinuation\n\n```\ncode\n```";
        let buffer = Buffer::from_text(text);
        let blocks = DragBlocks::new(&buffer, &crate::markdown::parse_decorations(text));
        assert_eq!(blocks.expand_selection(5..9), 4..10);
        assert_eq!(blocks.at(0).unwrap().kind, DragBlockKind::FrontMatter);
        assert!(!blocks.boundary_allowed(0));
        assert!(!blocks.boundary_allowed(1));
        assert!(blocks.boundary_allowed(3));
    }

    #[test]
    fn semantic_moves_preserve_block_separation_and_remap_following_lines() {
        for (text, source, destination, expected, inserted) in [
            (
                "before\n```\ncode\n```\nafter",
                1..4,
                5,
                "before\n\nafter\n\n```\ncode\n```",
                4..7,
            ),
            (
                "    code\n    more\n\nparagraph",
                0..2,
                4,
                "\nparagraph\n\n    code\n    more",
                3..5,
            ),
        ] {
            let mut buffer = Buffer::from_text(text);
            let movement =
                line_move_with_indent(&buffer, source.clone(), destination, "", 0, true).unwrap();
            assert_eq!(movement.inserted_lines, inserted);
            assert_eq!(movement.remap_line(source.start), inserted.start);
            assert_eq!(
                movement.remap_line(buffer.line_count()),
                buffer.line_count() + movement.added_lines
            );
            buffer.begin_edit_recording();
            buffer.replace(movement.range.clone(), &movement.text);
            assert_eq!(buffer.text(), expected);
            let decorations = crate::markdown::parse_decorations(buffer.text());
            assert!(matches!(
                decorations[inserted.start].block_type,
                BlockType::CodeBlock | BlockType::CodeBlockFence
            ));
            let edits = buffer.finish_edit_recording();
            assert_eq!(edits.len(), 1);
            assert!(buffer.apply_edits_inverse(&edits));
            assert_eq!(buffer.text(), text);
            assert!(buffer.apply_edits_forward(&edits));
            assert_eq!(buffer.text(), expected);
        }
    }

    #[test]
    fn moved_lines_apply_target_indentation_without_losing_relative_indent() {
        for (text, source, destination, expected, inserted) in [
            (
                "item\n  before\n  after",
                0..1,
                2,
                "  before\n  item\n  after",
                1..2,
            ),
            (
                "item\nplain\n\tnested",
                0..1,
                2,
                "plain\n\titem\n\tnested",
                1..2,
            ),
            (
                "  parent\n    child\n\tpeer",
                0..2,
                3,
                "\tpeer\n\tparent\n\t  child",
                1..3,
            ),
            ("\tpeer\n  item", 1..2, 0, "\titem\n\tpeer", 0..1),
            ("  item\nplain", 0..1, 2, "plain\nitem", 1..2),
            (
                "item\n\t before\n  after",
                0..1,
                2,
                "\t before\n\t item\n  after",
                1..2,
            ),
        ] {
            let mut buffer = Buffer::from_text(text);
            let target_line = expected.split('\n').nth(inserted.start).unwrap();
            let target_indent = leading_indent(target_line);
            let base = indent_columns(leading_indent(buffer.line_content(source.start)));
            let movement =
                line_move_with_indent(&buffer, source, destination, target_indent, base, false)
                    .unwrap();
            assert_eq!(movement.inserted_lines, inserted);
            buffer.begin_edit_recording();
            buffer.replace(movement.range, &movement.text);
            assert_eq!(buffer.text(), expected);
            let edits = buffer.finish_edit_recording();
            assert_eq!(edits.len(), 1);
            assert!(buffer.apply_edits_inverse(&edits));
            assert_eq!(buffer.text(), text);
        }
    }

    #[test]
    fn drop_indentation_follows_target_level_and_allows_parent_or_root() {
        let text = "- parent\n  - child\n- peer\n\n- dragged";
        let buffer = Buffer::from_text(text);
        let blocks = DragBlocks::new(&buffer, &crate::markdown::parse_decorations(text));
        assert_eq!(drop_indent(&buffer, &blocks, &(4..5), 1, 0), "  ");
        assert_eq!(drop_indent(&buffer, &blocks, &(4..5), 1, -1), "");
        assert_eq!(drop_indent(&buffer, &blocks, &(4..5), 2, 0), "");
        assert_eq!(drop_indent(&buffer, &blocks, &(4..5), 2, 1), "  ");
        assert_eq!(drop_indent(&buffer, &blocks, &(4..5), 2, 2), "    ");
        assert_eq!(drop_indent(&buffer, &blocks, &(4..5), 0, 9), "");
        assert_eq!(drop_indent(&buffer, &blocks, &(1..2), 1, 0), "  ");
        assert_eq!(drop_indent(&buffer, &blocks, &(1..2), 2, 0), "  ");
        assert_eq!(drop_indent(&buffer, &blocks, &(1..2), 1, -1), "");
    }

    #[test]
    fn reindent_in_place_can_lift_a_child_without_changing_its_text() {
        let mut buffer = Buffer::from_text("- parent\n  - child\n    - nested\n- peer");
        let movement = line_move_with_indent(&buffer, 1..3, 1, "", 2, false).unwrap();
        buffer.replace(movement.range, &movement.text);
        assert_eq!(buffer.text(), "- parent\n- child\n  - nested\n- peer");
        assert_eq!(movement.inserted_lines, 1..3);
    }

    #[test]
    fn tab_columns_and_code_payload_are_preserved_on_reindent() {
        assert_eq!(strip_indent_columns("\tcode", 2), "  code");
        assert_eq!(strip_indent_columns("  \tcode", 4), "code");
        assert_eq!(strip_indent_columns("\t  code", 4), "  code");
        let text = "    code\n        nested\n\ntext";
        let mut buffer = Buffer::from_text(text);
        let blocks = DragBlocks::new(&buffer, &crate::markdown::parse_decorations(text));
        let source = blocks.source(0);
        assert_eq!(source_indent_columns(&buffer, &blocks, 0), 0);
        let movement = line_move_with_indent(&buffer, source, 4, "", 0, false).unwrap();
        buffer.replace(movement.range, &movement.text);
        assert_eq!(buffer.text(), "\ntext\n    code\n        nested");
    }

    #[test]
    fn cursor_and_fold_indices_follow_their_lines() {
        assert_eq!(
            (0..5)
                .map(|line| moved_line(line, &(1..3), 5))
                .collect::<Vec<_>>(),
            vec![0, 3, 4, 1, 2]
        );
        assert_eq!(
            (0..5)
                .map(|line| moved_line(line, &(2..4), 0))
                .collect::<Vec<_>>(),
            vec![2, 3, 0, 1, 4]
        );
    }
}
