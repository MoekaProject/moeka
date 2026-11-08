pub(super) struct PinchScale {
    previous: f64,
}

impl Default for PinchScale {
    fn default() -> Self {
        Self { previous: 1.0 }
    }
}

impl PinchScale {
    pub(super) fn update(&mut self, scale: f64) -> Option<f32> {
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        let delta = (scale / self.previous - 1.0) as f32;
        if !delta.is_finite() {
            return None;
        }
        self.previous = scale;
        Some(delta)
    }
}

#[cfg(test)]
mod tests {
    use super::PinchScale;

    #[test]
    fn cumulative_scales_produce_incremental_zoom() {
        let mut pinch = PinchScale::default();
        assert!((pinch.update(1.2).unwrap() - 0.2).abs() < 0.00001);
        assert!((pinch.update(1.8).unwrap() - 0.5).abs() < 0.00001);
        assert!((pinch.update(0.9).unwrap() + 0.5).abs() < 0.00001);
        assert_eq!(PinchScale::default().update(1.0), Some(0.0));
    }

    #[test]
    fn invalid_scales_do_not_change_the_active_gesture() {
        let mut pinch = PinchScale::default();
        for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::MAX] {
            assert_eq!(pinch.update(invalid), None);
        }
        assert!((pinch.update(1.25).unwrap() - 0.25).abs() < 0.00001);
    }
}
