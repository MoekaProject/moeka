use gpui::{Animation, AnimationExt, Div, Hsla, SharedString, Stateful, div, prelude::*, px};

use crate::style::{ControlState, SwitchStyles};

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

pub(crate) struct Switch {
    id: SharedString,
    state: ControlState,
    description: SharedString,
    scale: f32,
    styles: SwitchStyles,
}

impl Switch {
    pub(crate) fn new(
        id: impl Into<SharedString>,
        state: ControlState,
        description: impl Into<SharedString>,
        scale: f32,
        styles: SwitchStyles,
    ) -> Self {
        Self {
            id: id.into(),
            state,
            description: description.into(),
            scale,
            styles: styles.for_size(crate::style::SwitchSize::Default),
        }
    }

    pub(crate) fn render(self) -> Stateful<Div> {
        let scale = self.scale;
        let scaled = move |value| px(value * scale);
        let styles = self.styles;
        let colors = styles.colors;
        let animation_duration = styles.animation_duration;
        let track_background = if self.state.checked {
            colors.checked_background
        } else {
            colors.unchecked_background
        };
        let interactive = !self.state.disabled && !self.state.loading && !self.state.read_only;
        let track_hover_background = if !interactive {
            track_background
        } else if self.state.checked {
            colors.checked_hover_background
        } else {
            colors.unchecked_hover_background
        };
        let track_border = if self.state.invalid {
            styles.ring.invalid_color
        } else if self.state.checked {
            colors.checked_background
        } else {
            colors.unchecked_border
        };
        let start_background = if self.state.checked {
            colors.unchecked_background
        } else {
            colors.checked_background
        };
        let start_border = if self.state.checked {
            colors.unchecked_border
        } else {
            colors.checked_background
        };
        let start_thumb_margin = styles.thumb_offset(!self.state.checked);
        let end_thumb_margin = styles.thumb_offset(self.state.checked);
        let animation_key = format!("{}-{}", self.id, self.state.checked);

        let thumb = div()
            .size(scaled(styles.thumb_size_px))
            .rounded(scaled(styles.thumb_radius_px))
            .bg(if self.state.checked {
                colors.thumb
            } else {
                colors.unchecked_thumb
            })
            .with_animation(
                SharedString::from(format!("{animation_key}-thumb")),
                Animation::new(animation_duration).with_easing(styles.easing),
                move |thumb, progress| {
                    thumb.ml(scaled(
                        start_thumb_margin + (end_thumb_margin - start_thumb_margin) * progress,
                    ))
                },
            );

        let track = div()
            .flex_none()
            .w(scaled(styles.track_width_px))
            .h(scaled(styles.track_height_px))
            .p(scaled(styles.track_padding_px))
            .flex()
            .items_center()
            .rounded(scaled(styles.track_radius_px))
            .border(scaled(styles.border_width_px))
            .border_color(track_border)
            .bg(track_background)
            .group_hover("switch", move |style| style.bg(track_hover_background))
            .when(self.state.focus_visible, |track| {
                track.shadow(styles.ring.shadows(scale, self.state.invalid))
            })
            .child(thumb)
            .with_animation(
                SharedString::from(format!("{animation_key}-track")),
                Animation::new(animation_duration).with_easing(styles.easing),
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
            .group("switch")
            .w_full()
            .h(scaled(styles.row_height_px))
            .flex()
            .items_center()
            .gap(scaled(styles.label_gap_px))
            .cursor(styles.cursor)
            .opacity(if self.state.disabled {
                styles.disabled_opacity
            } else if self.state.loading {
                styles.loading_opacity
            } else {
                1.0
            })
            .when(!interactive, super::interaction::inert_activation)
            .child(track)
            .child(
                div()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(scaled(styles.label_font_size_px))
                    .text_color(colors.description)
                    .child(self.description),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::interpolate_hsla;
    use crate::{style::Styles, theme::ThemeKind};

    #[test]
    fn switch_disabled_and_loading_states_reach_native_control() {
        use gpui::Styled;
        let theme = ThemeKind::OneDark.build();
        let styles = Styles::from_theme(&theme, &theme);
        for (state, opacity) in [
            (
                crate::style::ControlState {
                    disabled: true,
                    checked: true,
                    ..Default::default()
                },
                styles.switch.disabled_opacity,
            ),
            (
                crate::style::ControlState {
                    loading: true,
                    ..Default::default()
                },
                styles.switch.loading_opacity,
            ),
            (
                crate::style::ControlState {
                    read_only: true,
                    ..Default::default()
                },
                1.0,
            ),
        ] {
            let mut control =
                super::Switch::new("stateful-switch", state, "Label", 1.0, styles.switch).render();
            assert_eq!(control.style().opacity, Some(opacity));
        }
    }

    #[test]
    fn default_switch_preserves_settings_colors_and_geometry() {
        for kind in ThemeKind::light_themes()
            .iter()
            .chain(ThemeKind::dark_themes())
        {
            let theme = kind.build();
            for sidebar_always_dark in [false, true] {
                let styles = Styles::from_theme_with_options(&theme, &theme, sidebar_always_dark);
                let switch = styles.switch;
                assert_eq!(switch.colors.checked_background, theme.text_primary);
                assert_eq!(switch.colors.checked_hover_background, theme.text_bright);
                assert_eq!(switch.colors.thumb, theme.bg_primary);
                assert_eq!(
                    switch.colors.unchecked_background,
                    styles.settings_panel.settings_option_button_bg
                );
                assert_eq!(
                    switch.colors.unchecked_hover_background,
                    styles.settings_panel.settings_option_button_hover_bg
                );
                assert_eq!(
                    switch.colors.unchecked_border,
                    styles.settings_panel.settings_contrast_border_color
                );
                assert_eq!(
                    switch.colors.description,
                    styles.settings_panel.settings_theme_option_text_color
                );
                assert_eq!(switch.row_height_px, 30.0);
                assert_eq!(switch.track_width_px, 36.0);
                assert_eq!(switch.track_height_px, 21.0);
                assert_eq!(switch.border_width_px, 1.5);
                assert_eq!(switch.thumb_offset(false), 0.0);
                assert_eq!(switch.thumb_offset(true), 15.0);
                assert_eq!(switch.animation_duration.as_millis(), 160);
            }
        }
    }

    #[test]
    fn switch_motion_consumes_configured_travel_without_changing_document_checkbox() {
        let theme = ThemeKind::OneDark.build();
        let mut styles = Styles::from_theme(&theme, &theme);
        let document_border = styles.checkbox.checkbox_border_width_px;
        styles.switch.thumb_travel_px = 22.0;
        styles.switch.border_width_px = 2.0;
        assert_eq!(styles.switch.thumb_offset(true), 22.0);
        assert_eq!(styles.switch.thumb_offset(false), 0.0);
        assert_eq!(styles.checkbox.checkbox_border_width_px, document_border);
    }

    #[test]
    fn color_interpolation_preserves_animation_endpoints() {
        let from = crate::hex_hsla!("#11223344");
        let to = crate::hex_hsla!("#AABBCCDD");
        assert_eq!(interpolate_hsla(from, to, 0.0), from);
        assert_eq!(interpolate_hsla(from, to, 1.0), to);
    }
}
