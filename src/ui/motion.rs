use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, Bounds, ContentMask, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Window, point, px,
};

pub(crate) use gpui::{ScalarTransition as Transition, ease_out_quart as ease_out};

pub(crate) const CARET_FADE_FRAMES: usize = 4;
pub(crate) const CARET_FADE_FRAME_INTERVAL: Duration = Duration::from_millis(35);

pub(crate) fn caret_fade_opacity(from: f32, to: f32, frame: usize, frame_count: usize) -> f32 {
    let progress = frame.min(frame_count) as f32 / frame_count.max(1) as f32;
    let eased = progress * progress * (3.0 - 2.0 * progress);
    from + (to - from) * eased
}

pub(crate) fn enabled(system_reduced: Option<bool>) -> bool {
    !system_reduced.unwrap_or(true)
}

pub(crate) fn hover_progress(
    name: &'static str,
    bounds: Bounds<Pixels>,
    hovered: bool,
    window: &mut Window,
) -> f32 {
    if !window.content_mask().bounds.intersects(&bounds) {
        return if hovered { 1.0 } else { 0.0 };
    }
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    bounds.hash(&mut hash);
    window.hover_transition_progress(ElementId::NamedInteger(name.into(), hash.finish()), hovered)
}

pub(crate) fn value_transition(
    id: impl Into<ElementId>,
    target: f32,
    duration: Duration,
    window: &mut Window,
) -> f32 {
    let duration = if window.is_window_active() {
        duration
    } else {
        Duration::ZERO
    };
    window.with_global_id(id.into(), |id, window| {
        window.with_element_state(id, |state: Option<Transition>, window| {
            let now = Instant::now();
            let mut state = state.unwrap_or_else(|| Transition::settled(target, now));
            state.retarget(target, duration, now);
            if state.running(now) {
                window.request_presentation_frame();
            }
            (state.value(now), state)
        })
    })
}

pub(crate) fn transition<E: IntoElement + 'static>(
    id: impl Into<ElementId>,
    element: E,
    target: f32,
    initial: Option<f32>,
    duration: Duration,
    easing: fn(f32) -> f32,
    paint: impl Fn(E, f32) -> E + 'static,
) -> impl IntoElement {
    MotionElement {
        id: id.into(),
        element: Some(element),
        target,
        initial,
        duration,
        easing,
        paint,
    }
}

struct MotionElement<E, F> {
    id: ElementId,
    element: Option<E>,
    target: f32,
    initial: Option<f32>,
    duration: Duration,
    easing: fn(f32) -> f32,
    paint: F,
}

pub(crate) fn slide(
    element: impl IntoElement,
    offset_y: f32,
    clip_y: Option<std::ops::Range<f32>>,
) -> impl IntoElement {
    Slide {
        element: element.into_any_element(),
        offset_y,
        clip_y,
    }
}

struct Slide {
    element: AnyElement,
    offset_y: f32,
    clip_y: Option<std::ops::Range<f32>>,
}

impl IntoElement for Slide {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Slide {
    type RequestLayoutState = ();
    type PrepaintState = Option<ContentMask<Pixels>>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.element.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let mask = self.clip_y.as_ref().map(|clip| {
            let mut bounds_mask = window.content_mask().bounds;
            let bottom = bounds_mask.bottom().min(bounds.origin.y + px(clip.end));
            bounds_mask.origin.y = bounds_mask.origin.y.max(bounds.origin.y + px(clip.start));
            bounds_mask.size.height = (bottom - bounds_mask.origin.y).max(px(0.0));
            ContentMask {
                bounds: bounds_mask,
            }
        });
        window.with_content_mask(mask.clone(), |window| {
            window.with_element_offset(point(px(0.0), px(self.offset_y)), |window| {
                self.element.prepaint(window, cx);
            });
        });
        mask
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        mask: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_content_mask(mask.clone(), |window| self.element.paint(window, cx));
    }
}

impl<E: IntoElement + 'static, F: Fn(E, f32) -> E + 'static> IntoElement for MotionElement<E, F> {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl<E: IntoElement + 'static, F: Fn(E, f32) -> E + 'static> Element for MotionElement<E, F> {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        window.with_element_state(
            id.expect("motion element ID"),
            |state: Option<Transition>, window| {
                let now = Instant::now();
                let mut state = state.unwrap_or_else(|| {
                    Transition::settled(self.initial.unwrap_or(self.target), now)
                });
                state.set_easing(self.easing, now);
                state.retarget(
                    self.target,
                    if window.is_window_active() {
                        self.duration
                    } else {
                        Duration::ZERO
                    },
                    now,
                );
                let value = state.value(now);
                if state.running(now) {
                    window.request_presentation_frame();
                }
                let mut element =
                    (self.paint)(self.element.take().expect("motion layout runs once"), value)
                        .into_any_element();
                ((element.request_layout(window, cx), element), state)
            },
        )
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut AnyElement,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Styled;

    #[test]
    fn hover_color_interpolation_preserves_hue_from_transparent() {
        let blue: gpui::Hsla = gpui::rgb(0x2060c0).into();
        let middle: gpui::Rgba =
            gpui::interpolate_color(gpui::transparent_black(), blue, 0.5).into();
        let target: gpui::Rgba = blue.into();
        assert!((middle.a - 0.5).abs() < 0.001);
        assert!((middle.r - target.r).abs() < 0.001);
        assert!((middle.b - target.b).abs() < 0.001);
    }

    #[test]
    fn buttons_ignore_pressed_colors_and_chrome_stays_transparent() {
        let theme = crate::theme::ThemeKind::OneDark.build();
        let styles = crate::style::Styles::from_theme_with_spec(
            &theme,
            &theme,
            false,
            crate::style::UiStyleSpec {
                appearance: crate::style::UiAppearance {
                    corners: crate::style::UiCorners::Full,
                    elevation: true,
                },
                ..Default::default()
            },
        );
        for style in [
            styles.sidebar_visibility_button(true),
            styles.sidebar_visibility_button(false),
            styles.toolbar_button(55.0),
            styles.tab_action("tab-bar-info-toggle", false, 33.0, 1.0),
        ] {
            assert_eq!(style.normal.background.a, 0.0);
            assert_eq!(style.hover.background.a, 0.0);
            assert_eq!(style.border_width_px, 0.0);
            assert!(style.elevation.is_none());
        }
        let mut style = styles.ui.control;
        assert_eq!(style.radius_px, styles.ui.control.radius_px);
        assert!(style.full_round);
        style.pressed.background = gpui::rgb(0xff0000).into();
        let mut idle = crate::ui::button::button_frame(style, Default::default(), 1.0);
        let mut pressed = crate::ui::button::button_frame(
            style,
            crate::style::ControlState {
                pressed: true,
                ..Default::default()
            },
            1.0,
        );
        assert_eq!(idle.style().background, pressed.style().background);
        assert_eq!(idle.style().opacity, pressed.style().opacity);
        let dialog = styles.dialog_button(
            gpui::rgb(0x3060a0).into(),
            gpui::rgb(0xffffff).into(),
            gpui::rgb(0x204080).into(),
        );
        assert_ne!(dialog.normal.background, dialog.hover.background);
        assert_eq!(dialog.normal.foreground, dialog.hover.foreground);
        assert_eq!(dialog.hover.opacity, 1.0);
    }

    #[test]
    fn dialog_hover_uses_visible_palette_colors_without_fading_text() {
        for kind in crate::theme::ThemeKind::light_themes()
            .iter()
            .chain(crate::theme::ThemeKind::dark_themes())
        {
            let theme = kind.build();
            let styles = crate::style::Styles::from_theme(&theme, &theme);
            let ds = &styles.dialog;
            for (background, foreground, hover) in [
                (
                    ds.dialog_btn_primary_bg,
                    ds.dialog_btn_primary_text_color,
                    ds.dialog_btn_primary_hover_bg,
                ),
                (
                    ds.dialog_btn_secondary_bg,
                    ds.dialog_btn_secondary_text_color,
                    ds.dialog_btn_secondary_hover_bg,
                ),
                (
                    ds.dialog_btn_danger_bg,
                    ds.dialog_btn_danger_text_color,
                    ds.dialog_btn_danger_hover_bg,
                ),
            ] {
                let control = styles.dialog_button(background, foreground, hover);
                assert_eq!(control.hover.background, hover);
                assert_eq!(control.hover.background.a, control.normal.background.a);
                assert_eq!(control.hover.foreground, control.normal.foreground);
                assert_eq!(control.hover.opacity, 1.0);
                let normal: gpui::Rgba = background.into();
                let hovered: gpui::Rgba = hover.into();
                let difference = (normal.r - hovered.r)
                    .abs()
                    .max((normal.g - hovered.g).abs())
                    .max((normal.b - hovered.b).abs());
                assert!(
                    difference >= 8.0 / 255.0,
                    "{kind:?}: hover difference {difference}"
                );
            }
        }
    }

    #[test]
    fn motion_follows_system_accessibility_without_an_application_toggle() {
        assert!(!enabled(Some(true)));
        assert!(!enabled(None));
        assert!(enabled(Some(false)));
    }

    #[test]
    fn style_policy_disables_switch_motion_without_changing_control_dimensions() {
        let theme = crate::theme::ThemeKind::OneDark.build();
        let enabled = crate::style::Styles::from_theme(&theme, &theme);
        let disabled = crate::style::Styles::from_theme_with_spec(
            &theme,
            &theme,
            false,
            crate::style::UiStyleSpec {
                animations_enabled: false,
                ..Default::default()
            },
        );
        assert!(disabled.switch.animation_duration.is_zero());
        assert!(
            disabled
                .ui
                .spec
                .motion_duration(disabled.ui.spec.hover_motion_duration)
                .is_zero()
        );
        assert!(
            disabled
                .ui
                .spec
                .motion_duration(disabled.ui.spec.overlay_motion_duration)
                .is_zero()
        );
        assert_eq!(
            disabled.switch.track_width_px,
            enabled.switch.track_width_px
        );
        assert_eq!(
            disabled.switch.track_height_px,
            enabled.switch.track_height_px
        );
        assert_eq!(
            disabled.editor.editor_font_size_px,
            enabled.editor.editor_font_size_px
        );
    }

    #[test]
    fn first_mount_is_settled_and_reversal_is_continuous() {
        let now = Instant::now();
        let mut state = Transition::settled(0.0, now);
        assert!(!state.running(now));
        state.retarget(1.0, Duration::from_millis(160), now);
        let halfway = now + Duration::from_millis(80);
        let before = state.value(halfway);
        state.retarget(0.0, Duration::from_millis(100), halfway);
        assert_eq!(state.value(halfway), before);
        assert_eq!(state.value(halfway + Duration::from_millis(100)), 0.0);
        assert!(!state.running(halfway + Duration::from_millis(100)));
    }

    #[test]
    fn repeated_render_targets_do_not_restart_entrance() {
        let now = Instant::now();
        let duration = Duration::from_millis(140);
        let mut state = Transition::settled(0.0, now);
        state.retarget(1.0, duration, now);
        state.retarget(1.0, duration, now + Duration::from_millis(100));
        assert_eq!(state.value(now + duration), 1.0);
        assert!(!state.running(now + duration));
    }

    #[test]
    fn custom_easing_changes_preserve_value_and_completion_time() {
        fn linear(progress: f32) -> f32 {
            progress
        }
        let now = Instant::now();
        let duration = Duration::from_millis(160);
        let mut state = Transition::settled(0.0, now);
        state.set_easing(linear, now);
        state.retarget(1.0, duration, now);
        let halfway = now + duration / 2;
        assert_eq!(state.value(halfway), 0.5);
        state.set_easing(ease_out, halfway);
        assert_eq!(state.value(halfway), 0.5);
        assert!(state.value(now + Duration::from_millis(120)) > 0.75);
        assert_eq!(state.value(now + duration), 1.0);
        assert!(!state.running(now + duration));
    }

    #[test]
    fn disabled_motion_settles_without_nan_or_followup_frames() {
        let now = Instant::now();
        let mut state = Transition::settled(0.0, now);
        state.retarget(1.0, Duration::from_millis(160), now);
        state.retarget(1.0, Duration::ZERO, now + Duration::from_millis(20));
        assert_eq!(state.value(now), 1.0);
        assert!(!state.running(now));
        state.retarget(0.0, Duration::ZERO, now);
        assert_eq!(state.value(now), 0.0);
        for progress in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert!((0.0..=1.0).contains(&ease_out(progress)));
        }
    }
}
