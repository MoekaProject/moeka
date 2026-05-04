use std::time::Duration;

use gpui::{AnyElement, Bounds, Context, Pixels, SharedString, div, point, prelude::*, px, size};

use super::{
    EditorView,
    shortcut::{
        ShortcutRenderStyle, estimate_shortcut_width, estimate_text_width, render_shortcut,
    },
};

#[derive(Clone, PartialEq)]
pub(crate) struct TooltipState {
    pub id: &'static str,
    pub title: SharedString,
    pub subtitle: Option<SharedString>,
    pub anchor_bounds: Bounds<Pixels>,
}

impl EditorView {
    pub(crate) fn sync_tooltip_anchor(
        &mut self,
        tooltip_id: &'static str,
        anchor_bounds: Bounds<Pixels>,
    ) {
        if let Some(pending) = self.tooltip_pending.as_mut() {
            if pending.id == tooltip_id {
                pending.anchor_bounds = anchor_bounds;
            }
        }

        if let Some(visible) = self.tooltip_visible.as_mut() {
            if visible.id == tooltip_id {
                visible.anchor_bounds = anchor_bounds;
            }
        }
    }

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

    pub(crate) fn render_tooltip_overlay(&self, pane_width: f32) -> Option<AnyElement> {
        let tooltip = self.tooltip_visible.as_ref()?;
        let ts = &self.styles.tooltip;

        let viewport_padding = self.scaled(ts.tooltip_viewport_padding_px);
        let max_width = self.scaled(ts.tooltip_max_width_px);
        let offset = self.scaled(ts.tooltip_offset_px);
        let padding_x = self.scaled(ts.tooltip_padding_x_px);
        let padding_y = self.scaled(ts.tooltip_padding_y_px);
        let title_font_size = self.scaled(ts.tooltip_title_font_size_px);
        let subtitle_font_size = self.scaled(ts.tooltip_subtitle_font_size_px);
        let subtitle_shortcut_style = ShortcutRenderStyle {
            font_size_px: subtitle_font_size,
            text_color: ts.tooltip_subtitle_text_color,
            gap_px: self.scaled(ts.tooltip_shortcut_gap_px),
        };
        let title_width = estimate_text_width(tooltip.title.as_ref(), title_font_size);
        let subtitle_width = tooltip
            .subtitle
            .as_ref()
            .map(|subtitle| estimate_shortcut_width(subtitle, subtitle_shortcut_style))
            .unwrap_or(0.0);
        let content_width = title_width
            + if subtitle_width > 0.0 {
                self.scaled(ts.tooltip_gap_px) + subtitle_width
            } else {
                0.0
            };
        let tooltip_width = (content_width + padding_x * 2.0)
            .min(max_width)
            .max(padding_x * 2.0 + title_font_size);
        let tooltip_height = padding_y * 2.0 + title_font_size.max(subtitle_font_size);
        let pane_width = pane_width.max(tooltip_width + viewport_padding * 2.0);
        let pane_height = self
            .last_viewport_height
            .max(tooltip_height + viewport_padding * 2.0);
        let anchor_center_x = f32::from(tooltip.anchor_bounds.origin.x)
            + f32::from(tooltip.anchor_bounds.size.width) / 2.0;
        let anchor_top = f32::from(tooltip.anchor_bounds.origin.y);
        let anchor_height = f32::from(tooltip.anchor_bounds.size.height);
        let left = (anchor_center_x - tooltip_width / 2.0).clamp(
            viewport_padding,
            (pane_width - tooltip_width - viewport_padding).max(viewport_padding),
        );
        let above_top = anchor_top - offset - tooltip_height;
        let below_top = anchor_top + anchor_height + offset;
        let fits_above = above_top >= viewport_padding;
        let fits_below = below_top + tooltip_height <= pane_height - viewport_padding;
        let top = if fits_above || !fits_below {
            above_top.max(viewport_padding)
        } else {
            below_top
        };

        Some(
            div()
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(tooltip_width))
                .px(px(padding_x))
                .py(px(padding_y))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(self.scaled(ts.tooltip_gap_px)))
                .bg(ts.tooltip_bg)
                .rounded(px(self.scaled(ts.tooltip_radius_px)))
                .child(
                    div()
                        .text_size(px(title_font_size))
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
