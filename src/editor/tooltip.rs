use std::time::Duration;

use gpui::{AnyElement, Bounds, Context, Pixels, SharedString, div, point, prelude::*, px, size};

use super::{
    EditorView,
    shortcut::{ShortcutRenderStyle, render_shortcut},
};

#[derive(Clone, PartialEq)]
pub(crate) struct TooltipState {
    pub id: &'static str,
    pub title: SharedString,
    pub subtitle: Option<SharedString>,
    pub anchor_bounds: Bounds<Pixels>,
}

impl EditorView {
    pub(crate) fn begin_tooltip_hover(&mut self, tooltip: TooltipState, cx: &mut Context<Self>) {
        let pending_same = self
            .tooltip_pending
            .as_ref()
            .is_some_and(|pending| pending.id == tooltip.id);
        let visible_same = self
            .tooltip_visible
            .as_ref()
            .is_some_and(|visible| visible.id == tooltip.id);

        self.tooltip_pending = Some(tooltip.clone());

        if visible_same {
            self.tooltip_visible = Some(tooltip);
            cx.notify();
            return;
        }

        if pending_same {
            return;
        }

        self.tooltip_task = None;
        self.tooltip_visible = None;

        let tooltip_id = tooltip.id;
        let delay = Duration::from_millis(self.styles.tooltip.tooltip_delay_ms);
        self.tooltip_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .tooltip_pending
                    .as_ref()
                    .is_some_and(|pending| pending.id == tooltip_id)
                {
                    this.tooltip_visible = this.tooltip_pending.clone();
                    this.tooltip_task = None;
                    cx.notify();
                }
            });
        }));
    }

    pub(crate) fn end_tooltip_hover(&mut self, tooltip_id: &'static str, cx: &mut Context<Self>) {
        let mut changed = false;

        if self
            .tooltip_pending
            .as_ref()
            .is_some_and(|pending| pending.id == tooltip_id)
        {
            self.tooltip_pending = None;
            self.tooltip_task = None;
            changed = true;
        }

        if self
            .tooltip_visible
            .as_ref()
            .is_some_and(|visible| visible.id == tooltip_id)
        {
            self.tooltip_visible = None;
            changed = true;
        }

        if changed {
            cx.notify();
        }
    }

    pub(crate) fn clear_tooltip(&mut self, cx: &mut Context<Self>) {
        if self.tooltip_pending.is_none()
            && self.tooltip_visible.is_none()
            && self.tooltip_task.is_none()
        {
            return;
        }

        self.tooltip_pending = None;
        self.tooltip_visible = None;
        self.tooltip_task = None;
        cx.notify();
    }

    pub(crate) fn render_tooltip_overlay(&self) -> Option<AnyElement> {
        let tooltip = self.tooltip_visible.as_ref()?;
        let ts = &self.styles.tooltip;

        let max_width = self.scaled(ts.tooltip_max_width_px);
        let offset = self.scaled(ts.tooltip_offset_px);
        let padding_x = self.scaled(ts.tooltip_padding_x_px);
        let padding_y = self.scaled(ts.tooltip_padding_y_px);
        let estimated_height = padding_y * 2.0 + self.scaled(ts.tooltip_title_font_size_px) + 2.0;
        let anchor_left = f32::from(tooltip.anchor_bounds.origin.x);
        let anchor_top = f32::from(tooltip.anchor_bounds.origin.y);
        let anchor_width = f32::from(tooltip.anchor_bounds.size.width);
        let anchor_height = f32::from(tooltip.anchor_bounds.size.height);
        let min_x = 8.0;
        let max_x = (self.last_editor_width - max_width - min_x).max(min_x);
        let left = (anchor_left + anchor_width / 2.0 - max_width / 2.0).clamp(min_x, max_x);
        let above_top = anchor_top - offset - estimated_height;
        let below_top = anchor_top + anchor_height + offset;
        let fits_above = above_top >= 8.0;
        let fits_below = below_top + estimated_height <= self.last_window_height - 8.0;
        let top = if fits_above || !fits_below {
            above_top.max(8.0)
        } else {
            below_top
        };
        let subtitle_shortcut_style = ShortcutRenderStyle {
            font_size_px: self.scaled(ts.tooltip_subtitle_font_size_px),
            text_color: ts.tooltip_subtitle_text_color,
            gap_px: self.scaled(ts.tooltip_shortcut_gap_px),
            key_min_width_px: self.scaled(ts.tooltip_shortcut_key_min_width_px),
            key_height_px: self.scaled(ts.tooltip_shortcut_key_height_px),
            key_padding_x_px: self.scaled(ts.tooltip_shortcut_key_padding_x_px),
            key_radius_px: self.scaled(ts.tooltip_shortcut_key_radius_px),
            key_bg: ts.tooltip_shortcut_key_bg,
        };

        Some(
            div()
                .absolute()
                .left(px(left))
                .top(px(top))
                .max_w(px(max_width))
                .px(px(padding_x))
                .py(px(padding_y))
                .flex()
                .items_center()
                .gap(px(self.scaled(ts.tooltip_gap_px)))
                .bg(ts.tooltip_bg)
                .border(px(self.scaled(ts.tooltip_border_width_px)))
                .border_color(ts.tooltip_border_color)
                .rounded(px(self.scaled(ts.tooltip_radius_px)))
                .child(
                    div()
                        .text_size(px(self.scaled(ts.tooltip_title_font_size_px)))
                        .text_color(ts.tooltip_title_text_color)
                        .child(tooltip.title.clone()),
                )
                .when_some(tooltip.subtitle.as_ref(), |this, subtitle| {
                    this.child(
                        div()
                            .opacity(ts.tooltip_subtitle_opacity)
                            .child(render_shortcut(subtitle, subtitle_shortcut_style)),
                    )
                })
                .into_any_element(),
        )
    }
}

pub(crate) fn toolbar_anchor_bounds(x: f32, y: f32, button_size: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(button_size), px(button_size)))
}
