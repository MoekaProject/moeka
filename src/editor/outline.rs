use gpui::{
    AnyElement, Context, CursorStyle, MouseButton, MouseDownEvent, SharedString, div, prelude::*,
    px,
};

use crate::markdown::BlockType;

use super::*;

pub(crate) struct OutlineHeading {
    pub line: usize,
    pub level: u8,
    pub text: SharedString,
}

impl EditorView {
    pub(crate) fn collect_outline_headings(&self) -> Vec<OutlineHeading> {
        let mut headings = Vec::new();
        for (line_idx, dec) in self.decorations.iter().enumerate() {
            if let BlockType::Heading(level) = dec.block_type {
                let raw = self.buffer.line_content(line_idx);
                let text = raw.trim_start_matches('#').trim_start();
                headings.push(OutlineHeading {
                    line: line_idx,
                    level,
                    text: SharedString::from(text.to_string()),
                });
            }
        }
        headings
    }

    pub(crate) fn active_outline_heading_line(&self) -> Option<usize> {
        let cursor_line = self.cursor_line();
        let mut active_line = None;
        for (line_idx, dec) in self.decorations.iter().enumerate() {
            if let BlockType::Heading(_) = dec.block_type {
                if line_idx <= cursor_line {
                    active_line = Some(line_idx);
                } else {
                    break;
                }
            }
        }
        active_line
    }

    pub(crate) fn scroll_to_line(&mut self, target_line: usize, cx: &mut Context<Self>) {
        let target_y = self.line_y_offset(target_line);
        self.scroll_offset = (target_y - self.padding_top()).max(0.0);
        self.cursor.offset = self.buffer.line_start(target_line);
        self.cursor.anchor = self.cursor.offset;
        self.clamp_scroll();
        cx.notify();
    }

    pub(crate) fn render_outline(&self, cx: &mut Context<Self>) -> AnyElement {
        let os = &self.styles.outline;
        let headings = self.collect_outline_headings();
        let active_line = self.active_outline_heading_line();
        let header_height = self.scaled(os.outline_header_height_px);
        let item_height = self.scaled(os.outline_item_height_px);
        let indent_step = self.scaled(os.outline_item_indent_step_px);
        let padding_left = self.scaled(os.outline_item_padding_left_px);
        let padding_right = self.scaled(os.outline_item_padding_right_px);
        let font_size = self.scaled(os.outline_item_font_size_px);
        let header_font_size = self.scaled(os.outline_header_font_size_px);
        let text_color = os.outline_item_text_color;
        let active_text_color = os.outline_item_active_text_color;
        let active_bg = os.outline_item_active_bg;
        let hover_bg = os.outline_item_hover_bg;
        let header_text_color = os.outline_header_text_color;
        let header_padding_x = self.scaled(os.outline_header_padding_x_px);
        let top_spacer = self.scaled(os.outline_top_spacer_height_px);
        let bottom_spacer = self.scaled(os.outline_bottom_spacer_height_px);

        let mut items: Vec<AnyElement> = Vec::with_capacity(headings.len() + 3);

        items.push(
            div()
                .h(px(top_spacer))
                .flex_shrink_0()
                .into_any_element(),
        );

        items.push(
            div()
                .h(px(header_height))
                .flex_shrink_0()
                .flex()
                .items_center()
                .px(px(header_padding_x))
                .text_size(px(header_font_size))
                .text_color(header_text_color)
                .child("OUTLINE")
                .into_any_element(),
        );

        for heading in &headings {
            let is_active = active_line == Some(heading.line);
            let indent = padding_left + (heading.level.saturating_sub(1) as f32) * indent_step;
            let target_line = heading.line;
            let title = heading.text.clone();
            let item_text_color = if is_active {
                active_text_color
            } else {
                text_color
            };
            let item_bg = if is_active { active_bg } else { gpui::transparent_black() };

            items.push(
                div()
                    .id(SharedString::from(format!("outline-{}", target_line)))
                    .h(px(item_height))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .pl(px(indent))
                    .pr(px(padding_right))
                    .text_size(px(font_size))
                    .text_color(item_text_color)
                    .bg(item_bg)
                    .cursor(CursorStyle::Arrow)
                    .hover(move |s| s.bg(hover_bg))
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(title)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.scroll_to_line(target_line, cx);
                        }),
                    )
                    .into_any_element(),
            );
        }

        items.push(
            div()
                .h(px(bottom_spacer))
                .flex_shrink_0()
                .into_any_element(),
        );

        div()
            .id("outline-panel")
            .h_full()
            .w(px(self.outline_width()))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(&self.outline_scroll_handle)
            .bg(os.outline_bg)
            .border_l(px(self.scaled(os.outline_border_width_px)))
            .border_color(os.outline_border_color)
            .children(items)
            .into_any_element()
    }
}
