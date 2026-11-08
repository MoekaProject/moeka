// SPDX-License-Identifier: GPL-3.0-or-later
// Moeka-specific presentation animation; upstream GPUI retains its existing license.

use std::time::{Duration, Instant};

use crate::{Background, Bounds, Hsla, ScaledPixels, Scene, interpolate_color};

const MAX_COLORS: usize = 65_536;

struct ColorSample {
    bounds: Bounds<ScaledPixels>,
    order: u32,
    color: Hsla,
}

struct FillSample {
    bounds: Bounds<ScaledPixels>,
    order: u32,
    fill: Background,
    border: Hsla,
}

pub(crate) struct SceneColorTransition {
    started: Option<Instant>,
    duration: Duration,
    easing: fn(f32) -> f32,
    quads: Vec<FillSample>,
    paths: Vec<FillSample>,
    shadows: Vec<ColorSample>,
    underlines: Vec<ColorSample>,
    glyphs: Vec<ColorSample>,
}

fn mix_fill(from: Background, mut to: Background, progress: f32) -> Background {
    if from.tag == to.tag {
        to.solid = interpolate_color(from.solid, to.solid, progress);
        for (old, new) in from.colors.iter().zip(&mut to.colors) {
            new.color = interpolate_color(old.color, new.color, progress);
        }
    }
    to
}

impl SceneColorTransition {
    pub(crate) fn new(scene: &Scene, duration: Duration, easing: fn(f32) -> f32) -> Self {
        let mut remaining = MAX_COLORS;
        let mut budget = |count: usize| {
            let count = count.min(remaining);
            remaining -= count;
            count
        };
        Self {
            started: None,
            duration,
            easing,
            quads: scene
                .quads
                .iter()
                .take(budget(scene.quads.len()))
                .map(|item| FillSample {
                    bounds: item.bounds,
                    order: item.order,
                    fill: item.background,
                    border: item.border_color,
                })
                .collect(),
            glyphs: scene
                .monochrome_sprites
                .iter()
                .take(budget(scene.monochrome_sprites.len()))
                .map(|item| ColorSample {
                    bounds: item.bounds,
                    order: item.order,
                    color: item.color,
                })
                .collect(),
            paths: scene
                .paths
                .iter()
                .take(budget(scene.paths.len()))
                .map(|item| FillSample {
                    bounds: item.bounds,
                    order: item.order,
                    fill: item.color,
                    border: Hsla::default(),
                })
                .collect(),
            shadows: scene
                .shadows
                .iter()
                .take(budget(scene.shadows.len()))
                .map(|item| ColorSample {
                    bounds: item.bounds,
                    order: item.order,
                    color: item.color,
                })
                .collect(),
            underlines: scene
                .underlines
                .iter()
                .take(budget(scene.underlines.len()))
                .map(|item| ColorSample {
                    bounds: item.bounds,
                    order: item.order,
                    color: item.color,
                })
                .collect(),
        }
    }

    pub(crate) fn apply(&mut self, scene: &mut Scene) -> bool {
        if self.duration.is_zero() {
            return false;
        }
        let elapsed = self
            .started
            .get_or_insert_with(Instant::now)
            .elapsed()
            .as_secs_f32()
            / self.duration.as_secs_f32();
        if elapsed >= 1.0 {
            return false;
        }
        let progress = (self.easing)(elapsed.clamp(0.0, 1.0));
        let mut changed = false;
        // Paint operations stay canonical: cached views replay target colors,
        // never the previous frame's interpolated presentation.
        for (old, new) in self.quads.iter().zip(&mut scene.quads) {
            if old.bounds == new.bounds && old.order == new.order {
                changed |= old.fill != new.background || old.border != new.border_color;
                new.background = mix_fill(old.fill, new.background, progress);
                new.border_color = interpolate_color(old.border, new.border_color, progress);
            }
        }
        for (old, new) in self.paths.iter().zip(&mut scene.paths) {
            if old.bounds == new.bounds && old.order == new.order {
                changed |= old.fill != new.color;
                new.color = mix_fill(old.fill, new.color, progress);
            }
        }
        for (old, new) in self.glyphs.iter().zip(&mut scene.monochrome_sprites) {
            if old.bounds == new.bounds && old.order == new.order {
                changed |= old.color != new.color;
                new.color = interpolate_color(old.color, new.color, progress);
            }
        }
        for (old, new) in self.shadows.iter().zip(&mut scene.shadows) {
            if old.bounds == new.bounds && old.order == new.order {
                changed |= old.color != new.color;
                new.color = interpolate_color(old.color, new.color, progress);
            }
        }
        for (old, new) in self.underlines.iter().zip(&mut scene.underlines) {
            if old.bounds == new.bounds && old.order == new.order {
                changed |= old.color != new.color;
                new.color = interpolate_color(old.color, new.color, progress);
            }
        }
        changed
    }
}
