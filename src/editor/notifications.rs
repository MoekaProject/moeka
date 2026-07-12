use std::time::Duration;

use gpui::Context;

use super::EditorView;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotificationSeverity {
    Info,
    Error,
}

pub(crate) struct Notification {
    pub id: usize,
    pub severity: NotificationSeverity,
    pub message: String,
}

impl EditorView {
    pub(crate) fn notify_error(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.push_notification(NotificationSeverity::Error, message.into(), cx);
    }

    pub(crate) fn notify_info(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.push_notification(NotificationSeverity::Info, message.into(), cx);
    }

    fn push_notification(
        &mut self,
        severity: NotificationSeverity,
        message: String,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_notification_id;
        self.next_notification_id = self.next_notification_id.wrapping_add(1);
        self.notifications.push(Notification {
            id,
            severity,
            message,
        });
        let max_visible = self.styles.notification.notification_max_visible;
        while self.notifications.len() > max_visible {
            self.notifications.remove(0);
        }
        let dismiss_after =
            Duration::from_millis(self.styles.notification.notification_dismiss_after_ms);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(dismiss_after).await;
            this.update(cx, |editor, cx| {
                editor.dismiss_notification(id, cx);
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn dismiss_notification(&mut self, id: usize, cx: &mut Context<Self>) {
        let count_before = self.notifications.len();
        self.notifications
            .retain(|notification| notification.id != id);
        if self.notifications.len() != count_before {
            cx.notify();
        }
    }

    pub(crate) fn render_notifications(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        use gpui::{CursorStyle, MouseButton, SharedString, div, prelude::*, px};

        let ns = &self.styles.notification;
        let hover_opacity = ns.notification_hover_opacity;

        div()
            .absolute()
            .right(px(self.scaled(ns.notification_margin_right_px)))
            .bottom(px(self.scaled(ns.notification_margin_bottom_px)))
            .flex()
            .flex_col()
            .gap(px(self.scaled(ns.notification_gap_px)))
            .children(self.notifications.iter().map(|notification| {
                let id = notification.id;
                let accent = match notification.severity {
                    NotificationSeverity::Error => ns.notification_error_accent,
                    NotificationSeverity::Info => ns.notification_info_accent,
                };
                div()
                    .id(("notification", id))
                    .occlude()
                    .w(px(self.scaled(ns.notification_width_px)))
                    .bg(ns.notification_bg)
                    .border(px(ns.notification_border_width_px))
                    .border_color(ns.notification_border_color)
                    .rounded(px(ns.notification_radius_px))
                    .overflow_hidden()
                    .flex()
                    .flex_row()
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.opacity(hover_opacity))
                    .child(
                        div()
                            .w(px(ns.notification_accent_width_px))
                            .flex_none()
                            .bg(accent),
                    )
                    .child(
                        div()
                            .flex_1()
                            .px(px(self.scaled(ns.notification_padding_x_px)))
                            .py(px(self.scaled(ns.notification_padding_y_px)))
                            .text_size(px(self.scaled(ns.notification_font_size_px)))
                            .text_color(ns.notification_text_color)
                            .child(SharedString::from(notification.message.clone())),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.dismiss_notification(id, cx);
                        }),
                    )
            }))
            .into_any_element()
    }
}
