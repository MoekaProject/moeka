use gpui::{CursorStyle, Div, Hsla, SharedString, Stateful, div, prelude::*, px};

#[derive(Clone, Copy)]
pub(crate) struct CheckboxColors {
    pub checked_background: Hsla,
    pub checked_hover_background: Hsla,
    pub unchecked_background: Hsla,
    pub unchecked_hover_background: Hsla,
    pub unchecked_border: Hsla,
    pub thumb: Hsla,
    pub description: Hsla,
}

pub(crate) struct Checkbox {
    id: SharedString,
    checked: bool,
    description: SharedString,
    scale: f32,
    colors: CheckboxColors,
}

impl Checkbox {
    pub(crate) fn new(
        id: impl Into<SharedString>,
        checked: bool,
        description: impl Into<SharedString>,
        scale: f32,
        colors: CheckboxColors,
    ) -> Self {
        Self {
            id: id.into(),
            checked,
            description: description.into(),
            scale,
            colors,
        }
    }

    pub(crate) fn render(self) -> Stateful<Div> {
        let scaled = |value| px(value * self.scale);
        let track_background = if self.checked {
            self.colors.checked_background
        } else {
            self.colors.unchecked_background
        };
        let track_hover_background = if self.checked {
            self.colors.checked_hover_background
        } else {
            self.colors.unchecked_hover_background
        };
        let track_border = if self.checked {
            self.colors.checked_background
        } else {
            self.colors.unchecked_border
        };

        div()
            .id(self.id)
            .group("checkbox")
            .w_full()
            .h(scaled(30.0))
            .flex()
            .items_center()
            .gap(scaled(8.0))
            .cursor(CursorStyle::Arrow)
            .child(
                div()
                    .flex_none()
                    .w(scaled(36.0))
                    .h(scaled(21.0))
                    .p(scaled(2.0))
                    .flex()
                    .items_center()
                    .rounded(scaled(10.5))
                    .border(scaled(1.0))
                    .border_color(track_border)
                    .bg(track_background)
                    .group_hover("checkbox", move |style| style.bg(track_hover_background))
                    .child(
                        div()
                            .size(scaled(15.0))
                            .ml(scaled(if self.checked { 15.0 } else { 0.0 }))
                            .rounded(scaled(7.5))
                            .bg(self.colors.thumb),
                    ),
            )
            .child(
                div()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(scaled(12.0))
                    .text_color(self.colors.description)
                    .child(self.description),
            )
    }
}
