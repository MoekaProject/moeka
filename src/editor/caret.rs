use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Context, Entity, Hsla, PaintQuad, Render, Subscription, Window, canvas, fill, prelude::*,
};

use super::{EditorView, PaneId, element};

#[derive(Clone)]
pub(crate) struct CaretPaint {
    pub(crate) bounds: gpui::Bounds<gpui::Pixels>,
    pub(crate) color: Hsla,
    pub(crate) force_visible: bool,
}

pub(crate) type SharedCaretGeometry = Rc<RefCell<Vec<CaretPaint>>>;

// Four smoothstep samples over 140ms keep the fade visibly continuous while
// avoiding a display-rate present loop on older integrated GPUs.
const CARET_FADE_FRAMES: usize = 4;
const CARET_FADE_FRAME_INTERVAL: Duration = Duration::from_millis(35);

fn caret_fade_opacity(from: f32, to: f32, frame: usize, frame_count: usize) -> f32 {
    let progress = frame.min(frame_count) as f32 / frame_count.max(1) as f32;
    let eased = progress * progress * (3.0 - 2.0 * progress);
    from + (to - from) * eased
}

#[derive(Clone, Copy)]
pub(crate) struct CaretAnimationConfig {
    pub(crate) pause: Duration,
    pub(crate) visible: Duration,
    pub(crate) transparent: Duration,
    pub(crate) min_opacity: f32,
    pub(crate) max_opacity: f32,
}

pub(crate) struct EditorContentView {
    editor: Entity<EditorView>,
    pane_id: PaneId,
    caret_geometry: SharedCaretGeometry,
    _editor_observer: Subscription,
}

impl EditorContentView {
    pub(crate) fn new(
        editor: Entity<EditorView>,
        pane_id: PaneId,
        caret_geometry: SharedCaretGeometry,
        cx: &mut Context<Self>,
    ) -> Self {
        let observer = cx.observe(&editor, |_, _, cx| cx.notify());
        Self {
            editor,
            pane_id,
            caret_geometry,
            _editor_observer: observer,
        }
    }
}

impl Render for EditorContentView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        element::EditorElement {
            editor: self.editor.clone(),
            pane_id: self.pane_id,
            caret_geometry: self.caret_geometry.clone(),
        }
    }
}

pub(crate) struct CaretOverlayView {
    geometry: SharedCaretGeometry,
    config: CaretAnimationConfig,
    opacity: f32,
    active: bool,
    editor_blink_epoch: usize,
    animation_epoch: usize,
}

impl CaretOverlayView {
    pub(crate) fn new(geometry: SharedCaretGeometry, config: CaretAnimationConfig) -> Self {
        Self {
            geometry,
            opacity: config.max_opacity,
            active: false,
            editor_blink_epoch: 0,
            animation_epoch: 0,
            config,
        }
    }

    pub(crate) fn sync(
        &mut self,
        active: bool,
        editor_blink_epoch: usize,
        config: CaretAnimationConfig,
        cx: &mut Context<Self>,
    ) {
        self.config = config;
        let was_active = self.active;
        let became_active = active && !was_active;
        let reset = active && self.editor_blink_epoch != editor_blink_epoch;
        self.active = active;
        self.editor_blink_epoch = editor_blink_epoch;

        if !active {
            if was_active || self.opacity != config.max_opacity {
                self.animation_epoch = self.animation_epoch.wrapping_add(1);
                self.opacity = config.max_opacity;
                cx.notify();
            }
            return;
        }

        if became_active || reset {
            self.animation_epoch = self.animation_epoch.wrapping_add(1);
            self.opacity = config.max_opacity;
            let delay = if reset { config.pause } else { config.visible };
            self.schedule_transition(
                self.animation_epoch,
                delay,
                config.max_opacity,
                config.min_opacity,
                cx,
            );
            cx.notify();
        }
    }

    fn schedule_transition(
        &self,
        epoch: usize,
        hold: Duration,
        from: f32,
        to: f32,
        cx: &mut Context<Self>,
    ) {
        let config = self.config;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(hold).await;
            for frame in 1..=CARET_FADE_FRAMES {
                cx.background_executor()
                    .timer(CARET_FADE_FRAME_INTERVAL)
                    .await;
                let keep_animating = this
                    .update(cx, |this, cx| {
                        if !this.active || this.animation_epoch != epoch {
                            return false;
                        }

                        this.opacity = caret_fade_opacity(from, to, frame, CARET_FADE_FRAMES);
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep_animating {
                    return;
                }
            }

            this.update(cx, |this, cx| {
                if !this.active || this.animation_epoch != epoch {
                    return;
                }

                let (next_hold, next_target) = if to <= config.min_opacity {
                    (config.transparent, config.max_opacity)
                } else {
                    (config.visible, config.min_opacity)
                };
                this.schedule_transition(epoch, next_hold, to, next_target, cx);
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::{CARET_FADE_FRAMES, caret_fade_opacity};

    #[test]
    fn overlay_caret_fade_is_smooth_monotonic_and_reaches_the_endpoint() {
        let samples = (0..=CARET_FADE_FRAMES)
            .map(|frame| caret_fade_opacity(1.0, 0.0, frame, CARET_FADE_FRAMES))
            .collect::<Vec<_>>();
        assert_eq!(samples.first().copied(), Some(1.0));
        assert_eq!(samples.last().copied(), Some(0.0));
        assert!(samples.windows(2).all(|pair| pair[0] > pair[1]));

        assert_eq!(
            caret_fade_opacity(0.0, 1.0, CARET_FADE_FRAMES, CARET_FADE_FRAMES),
            1.0
        );
    }
}

impl Render for CaretOverlayView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let paints = self.geometry.borrow().clone();
        let opacity = self.opacity;
        canvas(
            move |_, _, _| paints,
            move |_, paints, window, _| {
                for paint in paints {
                    let opacity = if paint.force_visible { 1.0 } else { opacity };
                    if opacity <= 0.0 {
                        continue;
                    }
                    let mut color = paint.color;
                    color.a *= opacity;
                    let quad: PaintQuad = fill(paint.bounds, color);
                    window.paint_quad(quad);
                }
            },
        )
        .absolute()
        .size_full()
    }
}
