use std::time::Duration;

use gpui::Context;

use super::EditorView;

pub(crate) struct Notification {
    pub id: usize,
    pub message: String,
    pub closing: bool,
}

#[cfg(all(test, feature = "gpui-test-support"))]
#[gpui::test]
fn localized_diagnostics_preserve_original_error_details(cx: &mut gpui::TestAppContext) {
    use gpui::AppContext;
    let editor =
        cx.update(|cx| cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some(""), false)));
    editor.update(cx, |editor, cx| {
        let input = cx.new(|cx| {
            let mut input =
                crate::text_input::TextInput::new(editor.styles.text_input.clone(), "Commands", cx);
            input.set_text("keep query", cx);
            input
        });
        editor.command_palette_editor = Some(input.clone());
        editor.ui_language = crate::i18n::UiLanguage::ChineseSimplified;
        super::shortcut::apply_application_menus(
            cx,
            super::shortcut::app_menus(editor.ui_language),
            editor.ui_language,
        );
        assert_eq!(crate::i18n::interface_language(cx), editor.ui_language);
        editor.refresh_localized_inputs(cx);
        assert_eq!(
            input.read(cx).placeholder.as_ref(),
            editor.t(crate::i18n::TextKey::CommandPalettePlaceholder)
        );
        assert_eq!(input.read(cx).text(), "keep query");
        editor.notify_diagnostic("OS error 13: Permission denied", cx);
        let message = &editor.notifications.last().unwrap().message;
        assert!(message.starts_with(editor.t(crate::i18n::TextKey::OperationFailed)));
        assert!(message.ends_with("OS error 13: Permission denied"));
        editor.notify_diagnostic(crate::i18n::runtime::RuntimeText::ExportBusy, cx);
        assert_eq!(
            editor.notifications.last().unwrap().message,
            "操作失败: 另一个导出任务仍在进行中，请等待完成"
        );
    });
}

impl EditorView {
    pub(crate) fn notify_diagnostic(
        &mut self,
        detail: impl std::fmt::Display,
        cx: &mut Context<Self>,
    ) {
        self.notify_error(
            format!(
                "{}: {detail}",
                self.t(crate::i18n::TextKey::OperationFailed)
            ),
            cx,
        );
    }

    pub(crate) fn notify_error(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.push_notification(message.into(), cx);
    }

    pub(crate) fn notify_info(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.push_notification(message.into(), cx);
    }

    fn push_notification(&mut self, message: String, cx: &mut Context<Self>) {
        if self
            .notifications
            .iter()
            .any(|notification| notification.message == message)
        {
            return;
        }
        let id = self.next_notification_id;
        self.next_notification_id = self.next_notification_id.wrapping_add(1);
        self.notifications.push(Notification {
            id,
            message,
            closing: false,
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
        if self.animations_enabled() {
            let Some(notification) = self
                .notifications
                .iter_mut()
                .find(|notification| notification.id == id)
            else {
                return;
            };
            if notification.closing {
                return;
            }
            notification.closing = true;
            let duration = self.styles.ui.spec.notification_exit_duration;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(duration).await;
                this.update(cx, |editor, cx| {
                    editor
                        .notifications
                        .retain(|notification| notification.id != id);
                    cx.notify();
                })
                .ok();
            })
            .detach();
            cx.notify();
            return;
        }
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
                let card = crate::ui::surface::surface(&self.styles.notification_surface(), 1.0)
                    .id(("notification", id))
                    .occlude()
                    .w(px(self.scaled(ns.notification_width_px)))
                    .overflow_hidden()
                    .flex()
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.opacity(hover_opacity))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .px(px(self.scaled(ns.notification_padding_x_px)))
                            .py(px(self.scaled(ns.notification_padding_y_px)))
                            .whitespace_normal()
                            .text_size(px(self.scaled(ns.notification_font_size_px)))
                            .text_color(self.styles.notification_surface().foreground)
                            .child(SharedString::from(notification.message.clone())),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.dismiss_notification(id, cx);
                        }),
                    )
                    .when(notification.closing, |card| {
                        crate::ui::interaction::inert_activation(card).cursor(CursorStyle::Arrow)
                    });
                crate::ui::motion::transition(
                    ("notification-motion", id),
                    div().flex_none().child(card),
                    if notification.closing { 0.0 } else { 1.0 },
                    Some(0.0),
                    self.styles
                        .ui
                        .spec
                        .motion_duration(if notification.closing {
                            self.styles.ui.spec.notification_exit_duration
                        } else {
                            self.styles.ui.spec.overlay_motion_duration
                        }),
                    self.styles.ui.spec.easing,
                    |card, opacity| card.opacity(opacity),
                )
                .into_any_element()
            }))
            .into_any_element()
    }
}
