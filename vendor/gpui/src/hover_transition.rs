// SPDX-License-Identifier: GPL-3.0-or-later
// Moeka-specific additions; the upstream GPUI files retain their existing licenses.

use std::{
    hash::{Hash, Hasher},
    time::{Duration, Instant},
};

use crate::{
    BackgroundTag, ElementId, GlobalElementId, Hitbox, Hsla, Refineable, Rgba, Style,
    StyleRefinement, Window,
};

/// Monotonic easing for short interface transitions.
pub fn ease_out_quart(progress: f32) -> f32 {
    1.0 - (1.0 - progress.clamp(0.0, 1.0)).powi(4)
}

/// A retargetable scalar transition. Settled values never require a frame.
#[derive(Clone, Copy)]
pub struct ScalarTransition {
    from: f32,
    target: f32,
    started: Instant,
    duration: Duration,
    easing: fn(f32) -> f32,
}

impl ScalarTransition {
    /// Start at the actual displayed value, without a first-mount animation.
    pub fn settled(value: f32, now: Instant) -> Self {
        Self {
            from: value,
            target: value,
            started: now,
            duration: Duration::ZERO,
            easing: ease_out_quart,
        }
    }

    /// Sample the current presentation.
    pub fn value(self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return self.target;
        }
        let progress =
            now.saturating_duration_since(self.started).as_secs_f32() / self.duration.as_secs_f32();
        self.from + (self.target - self.from) * (self.easing)(progress.clamp(0.0, 1.0))
    }

    /// Reverse from the current value, or snap immediately for zero duration.
    pub fn retarget(&mut self, target: f32, duration: Duration, now: Instant) {
        if duration.is_zero() {
            self.from = target;
            self.target = target;
            self.started = now;
            self.duration = Duration::ZERO;
        } else if self.target != target {
            self.from = self.value(now);
            self.target = target;
            self.started = now;
            self.duration = duration;
        }
    }

    /// Whether another presentation frame is needed.
    pub fn running(self, now: Instant) -> bool {
        self.from != self.target && now.saturating_duration_since(self.started) < self.duration
    }

    /// Change the curve without restarting the transition or jumping.
    pub fn set_easing(&mut self, easing: fn(f32) -> f32, now: Instant) {
        if !std::ptr::fn_addr_eq(self.easing, easing) {
            let value = self.value(now);
            self.duration = self
                .duration
                .saturating_sub(now.saturating_duration_since(self.started));
            self.from = value;
            self.started = now;
            self.easing = easing;
        }
    }
}

/// Interpolate premultiplied sRGB, including transitions from transparent fills.
pub fn interpolate_color(from: Hsla, to: Hsla, progress: f32) -> Hsla {
    let t = progress.clamp(0.0, 1.0);
    let a: Rgba = from.into();
    let b: Rgba = to.into();
    let alpha = a.a + (b.a - a.a) * t;
    if alpha <= f32::EPSILON {
        return Hsla::default();
    }
    Rgba {
        r: (a.r * a.a * (1.0 - t) + b.r * b.a * t) / alpha,
        g: (a.g * a.a * (1.0 - t) + b.g * b.a * t) / alpha,
        b: (a.b * a.a * (1.0 - t) + b.b * b.a * t) / alpha,
        a: alpha,
    }
    .into()
}

impl Window {
    /// Opt into paint-only hover transitions for this window. Zero disables them.
    pub fn set_hover_transition(&mut self, duration: Duration, easing: fn(f32) -> f32) {
        if self.hover_transition.is_none_or(|(previous, curve)| {
            previous != duration || !std::ptr::fn_addr_eq(curve, easing)
        }) {
            self.refreshing = true;
        }
        self.hover_transition = Some((duration, easing));
    }

    /// Shared hover timing for both styled elements and custom-painted controls.
    /// The ID is scoped to the current element and view; callers must supply stable IDs.
    pub fn hover_transition_progress(&mut self, id: impl Into<ElementId>, hovered: bool) -> f32 {
        let target = if hovered { 1.0 } else { 0.0 };
        let Some((mut duration, easing)) = self.hover_transition else {
            return target;
        };
        if !self.is_window_active() {
            duration = Duration::ZERO;
        }
        if duration.is_zero() {
            return target;
        }
        let mut key = GlobalElementId(self.element_id_stack.clone());
        key.0.push(ElementId::Name("__hover-transition".into()));
        key.0.push(id.into());
        self.with_element_state(&key, |state: Option<ScalarTransition>, window| {
            let now = Instant::now();
            let mut state = state.unwrap_or_else(|| ScalarTransition::settled(target, now));
            state.set_easing(easing, now);
            state.retarget(target, duration, now);
            if state.running(now) {
                window.request_presentation_frame();
            }
            (state.value(now), state)
        })
    }
}

pub(crate) fn apply_hover_style(
    style: &mut Style,
    hover: &StyleRefinement,
    hovered: bool,
    hitbox: &Hitbox,
    identified: bool,
    group: Option<&crate::SharedString>,
    window: &mut Window,
) {
    if window
        .hover_transition
        .is_none_or(|(duration, _)| duration.is_zero())
    {
        if hovered {
            style.refine(hover);
        }
        return;
    }
    let base = style.clone();
    let mut target = base.clone();
    target.refine(hover);
    if base.background == target.background
        && base.border_color == target.border_color
        && base.text.color == target.text.color
        && base.opacity == target.opacity
    {
        if hovered {
            style.refine(hover);
        }
        return;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    group.hash(&mut hasher);
    if !identified {
        hitbox.bounds.hash(&mut hasher);
        hover.background.is_some().hash(&mut hasher);
        hover.border_color.is_some().hash(&mut hasher);
        hover.opacity.is_some().hash(&mut hasher);
        hover
            .text
            .as_ref()
            .is_some_and(|text| text.color.is_some())
            .hash(&mut hasher);
    }
    let progress = window.hover_transition_progress(
        ElementId::NamedInteger("style".into(), hasher.finish()),
        hovered,
    );
    if hovered {
        style.refine(hover);
    }
    if progress <= 0.0 {
        style.background = base.background;
        style.border_color = base.border_color;
        style.text.color = base.text.color;
        style.opacity = base.opacity;
        return;
    }
    if progress >= 1.0 {
        style.background = target.background;
        style.border_color = target.border_color;
        style.text.color = target.text.color;
        style.opacity = target.opacity;
        return;
    }
    if base.background != target.background {
        let from = base
            .background
            .as_ref()
            .and_then(|fill| fill.color())
            .unwrap_or_default();
        let to = target
            .background
            .as_ref()
            .and_then(|fill| fill.color())
            .unwrap_or_default();
        if from.tag == BackgroundTag::Solid && to.tag == BackgroundTag::Solid {
            style.background = Some(interpolate_color(from.solid, to.solid, progress).into());
        }
    }
    if base.border_color != target.border_color {
        style.border_color = Some(interpolate_color(
            base.border_color.unwrap_or_default(),
            target.border_color.unwrap_or_default(),
            progress,
        ));
    }
    if base.text.color != target.text.color {
        let inherited = window.text_style().color;
        style.text.color = Some(interpolate_color(
            base.text.color.unwrap_or(inherited),
            target.text.color.unwrap_or(inherited),
            progress,
        ));
    }
    if base.opacity != target.opacity {
        let from = base.opacity.unwrap_or(1.0);
        style.opacity = Some(from + (target.opacity.unwrap_or(1.0) - from) * progress);
    }
}
