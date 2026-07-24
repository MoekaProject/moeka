use std::time::Duration;

use gpui::{
    Animation, AnimationExt, CursorStyle, Div, Hsla, SharedString, Stateful, div, ease_in_out,
    prelude::*, px,
};

fn interpolate_hsla(from: Hsla, to: Hsla, progress: f32) -> Hsla {
    let progress = progress.clamp(0.0, 1.0);
    let hue_delta = (to.h - from.h + 0.5).rem_euclid(1.0) - 0.5;
    Hsla {
        h: (from.h + hue_delta * progress).rem_euclid(1.0),
        s: from.s + (to.s - from.s) * progress,
        l: from.l + (to.l - from.l) * progress,
        a: from.a + (to.a - from.a) * progress,
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CheckboxColors {
    pub checked_background: Hsla,
    pub checked_hover_background: Hsla,
    pub unchecked_background: Hsla,
    pub unchecked_hover_background: Hsla,
    pub unchecked_border: Hsla,
    pub border_width_px: f32,
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
        let scale = self.scale;
        let scaled = move |value| px(value * scale);
        let animation_duration = Duration::from_millis(160);
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
        let start_background = if self.checked {
            self.colors.unchecked_background
        } else {
            self.colors.checked_background
        };
        let start_border = if self.checked {
            self.colors.unchecked_border
        } else {
            self.colors.checked_background
        };
        let start_thumb_margin = if self.checked { 0.0 } else { 15.0 };
        let end_thumb_margin = if self.checked { 15.0 } else { 0.0 };
        let animation_key = format!("{}-{}", self.id, self.checked);

        let thumb = div()
            .size(scaled(15.0))
            .rounded(scaled(7.5))
            .bg(self.colors.thumb)
            .with_animation(
                SharedString::from(format!("{animation_key}-thumb")),
                Animation::new(animation_duration).with_easing(ease_in_out),
                move |thumb, progress| {
                    thumb.ml(scaled(
                        start_thumb_margin + (end_thumb_margin - start_thumb_margin) * progress,
                    ))
                },
            );

        let track = div()
            .flex_none()
            .w(scaled(36.0))
            .h(scaled(21.0))
            .p(scaled(2.0))
            .flex()
            .items_center()
            .rounded(scaled(10.5))
            .border(scaled(self.colors.border_width_px))
            .border_color(track_border)
            .bg(track_background)
            .group_hover("checkbox", move |style| style.bg(track_hover_background))
            .child(thumb)
            .with_animation(
                SharedString::from(format!("{animation_key}-track")),
                Animation::new(animation_duration).with_easing(ease_in_out),
                move |track, progress| {
                    track
                        .bg(interpolate_hsla(
                            start_background,
                            track_background,
                            progress,
                        ))
                        .border_color(interpolate_hsla(start_border, track_border, progress))
                },
            );

        div()
            .id(self.id)
            .group("checkbox")
            .w_full()
            .h(scaled(30.0))
            .flex()
            .items_center()
            .gap(scaled(8.0))
            .cursor(CursorStyle::Arrow)
            .child(track)
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

#[cfg(test)]
mod tests {
    use super::interpolate_hsla;

    #[test]
    fn color_interpolation_preserves_animation_endpoints() {
        let from = crate::hex_hsla!("#11223344");
        let to = crate::hex_hsla!("#AABBCCDD");
        assert_eq!(interpolate_hsla(from, to, 0.0), from);
        assert_eq!(interpolate_hsla(from, to, 1.0), to);
    }
}
