//! Content-only column widths; callers account for cell padding and borders.

/// Fill the viewport without taking space from short columns unnecessarily.
///
/// The effective budget is at least one pixel per column. Invalid budgets fall
/// back to that minimum, invalid/nonpositive minimum widths fall back to one
/// pixel, and nonfinite demands fall back to the adaptive minimum width.
/// Calculations use f64 so large demands cannot overflow an f32 sum; returned
/// widths fill the budget to f32 rounding tolerance.
pub(super) fn allocate(desired: &[f32], available: f32, min_content_width: f32) -> Vec<f32> {
    if desired.is_empty() {
        return Vec::new();
    }

    let count = desired.len() as f64;
    let budget = if available.is_finite() {
        f64::from(available).max(count)
    } else {
        count
    };
    let minimum = if min_content_width.is_finite() && min_content_width > 0.0 {
        f64::from(min_content_width)
    } else {
        1.0
    };
    let floor = minimum.min(budget / count);
    let mut widths: Vec<f64> = desired
        .iter()
        .map(|&width| {
            if width.is_finite() {
                f64::from(width).max(floor)
            } else {
                floor
            }
        })
        .collect();
    let total: f64 = widths.iter().sum();

    if total > budget {
        let mut sorted = widths.clone();
        sorted.sort_unstable_by(f64::total_cmp);
        let mut remaining = budget;
        for (index, &width) in sorted.iter().enumerate() {
            let share = remaining / (sorted.len() - index) as f64;
            if width >= share {
                // Freeze smaller demands at natural width, capping only the
                // columns that still compete for the remaining space.
                for width in &mut widths {
                    *width = width.min(share);
                }
                break;
            }
            remaining -= width;
        }
    } else if total < budget {
        let widest = widths.iter().copied().fold(floor, f64::max);
        let ties = widths.iter().filter(|&&width| width == widest).count();
        let extra = (budget - total) / ties as f64;
        for width in &mut widths {
            if *width == widest {
                *width += extra;
            }
        }
    }

    widths.into_iter().map(|width| width as f32).collect()
}

#[cfg(test)]
mod tests {
    use super::allocate;

    fn assert_close(actual: &[f32], expected: &[f32]) {
        assert_eq!(actual.len(), expected.len());
        for (&actual, &expected) in actual.iter().zip(expected) {
            let tolerance = f64::from(expected).abs().max(1.0) * f64::from(f32::EPSILON) * 8.0;
            assert!(
                (f64::from(actual) - f64::from(expected)).abs() <= tolerance,
                "expected {expected}, got {actual}"
            );
        }
    }

    fn assert_fills_budget(widths: &[f32], available: f32) {
        assert!(!widths.is_empty());
        assert!(widths.iter().all(|width| width.is_finite() && *width > 0.0));
        let budget = if available.is_finite() {
            f64::from(available).max(widths.len() as f64)
        } else {
            widths.len() as f64
        };
        let total: f64 = widths.iter().map(|&width| f64::from(width)).sum();
        let tolerance = budget * f64::from(f32::EPSILON) * 4.0;
        assert!(total <= budget + tolerance, "{total} exceeds {budget}");
        assert!((total - budget).abs() <= tolerance, "{total} != {budget}");
    }

    #[test]
    fn constrained_columns_preserve_short_demands() {
        assert_close(&allocate(&[30.0, 400.0], 300.0, 24.0), &[30.0, 270.0]);
        assert_close(
            &allocate(&[30.0, 40.0, 400.0, 600.0], 500.0, 24.0),
            &[30.0, 40.0, 215.0, 215.0],
        );
        assert_close(
            &allocate(&[30.0, 80.0, 100.0, 800.0], 360.0, 24.0),
            &[30.0, 80.0, 100.0, 150.0],
        );
    }

    #[test]
    fn surplus_goes_only_to_the_widest_demands() {
        assert_close(
            &allocate(&[30.0, 50.0, 100.0], 300.0, 24.0),
            &[30.0, 50.0, 220.0],
        );
        assert_close(
            &allocate(&[30.0, 100.0, 100.0], 300.0, 24.0),
            &[30.0, 135.0, 135.0],
        );
        assert_close(&allocate(&[40.0; 3], 300.0, 24.0), &[100.0; 3]);
    }

    #[test]
    fn equal_demands_share_constraints_evenly() {
        assert_close(
            &allocate(&[30.0, 300.0, 300.0], 300.0, 24.0),
            &[30.0, 135.0, 135.0],
        );
        assert_close(&allocate(&[400.0; 3], 300.0, 24.0), &[100.0; 3]);
    }

    #[test]
    fn exact_fit_preserves_natural_widths() {
        let desired = [30.0, 50.0, 100.0];
        assert_eq!(allocate(&desired, 180.0, 24.0), desired.to_vec());
    }

    #[test]
    fn minimum_is_adaptive_and_does_not_replace_natural_demands() {
        assert_close(&allocate(&[1.0, 2.0, 300.0], 60.0, 50.0), &[20.0; 3]);
        assert_close(
            &allocate(&[8.0, 30.0, 600.0], 180.0, 24.0),
            &[24.0, 30.0, 126.0],
        );
        assert_close(&allocate(&[5.0, 200.0], 300.0, 30.0), &[30.0, 270.0]);
        assert_close(
            &allocate(&[0.0, 0.125, 100.0], 10.0, 0.25),
            &[0.25, 0.25, 9.5],
        );
    }

    #[test]
    fn empty_single_and_very_narrow_tables() {
        assert!(allocate(&[], 300.0, 24.0).is_empty());
        assert_close(&allocate(&[400.0], 300.0, 24.0), &[300.0]);
        assert_close(&allocate(&[30.0], 300.0, 24.0), &[300.0]);
        for budget in [-100.0, 0.0, 0.25, 2.0, 3.0] {
            assert_close(&allocate(&[30.0, 40.0, 400.0], budget, 24.0), &[1.0; 3]);
        }
    }

    #[test]
    fn invalid_inputs_have_finite_positive_fallbacks() {
        for budget in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let widths = allocate(&[30.0, 400.0], budget, 24.0);
            assert_close(&widths, &[1.0; 2]);
            assert_fills_budget(&widths, budget);
        }
        assert_close(
            &allocate(
                &[f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 10.0],
                40.0,
                2.0,
            ),
            &[2.0, 2.0, 2.0, 34.0],
        );
        assert_close(&allocate(&[-4.0, 0.0, 10.0], 20.0, 2.0), &[2.0, 2.0, 16.0]);
        for minimum in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -10.0, 0.0] {
            assert_close(&allocate(&[0.0, 10.0], 20.0, minimum), &[1.0, 19.0]);
        }
        let smallest = f32::from_bits(1);
        let widths = allocate(&[0.0, 100.0], 10.0, smallest);
        assert_eq!(widths[0], smallest);
        assert_fills_budget(&widths, 10.0);
    }

    #[test]
    fn large_demands_do_not_overflow() {
        assert_close(&allocate(&[30.0, f32::MAX], 300.0, 24.0), &[30.0, 270.0]);
        for budget in [300.0, f32::MAX] {
            let widths = allocate(&[f32::MAX; 17], budget, 24.0);
            assert_fills_budget(&widths, budget);
            assert!(widths.iter().all(|width| *width == widths[0]));
        }
    }

    #[test]
    fn allocation_scales_with_pixel_measurements() {
        for (desired, budget) in [
            ([30.0, 40.0, 400.0, 600.0], 500.0),
            ([30.0, 40.0, 100.0, 100.0], 500.0),
        ] {
            let baseline = allocate(&desired, budget, 24.0);
            for scale in [0.03125, 0.125, 1.0, 2.0, 16.0, 1024.0, 1.0e20, 1.0e30] {
                let scaled = desired.map(|width| width * scale);
                let widths = allocate(&scaled, budget * scale, 24.0 * scale);
                let expected: Vec<f32> = baseline.iter().map(|width| width * scale).collect();
                assert_close(&widths, &expected);
                assert_fills_budget(&widths, budget * scale);
            }
        }
    }

    #[test]
    fn column_order_does_not_change_allocations() {
        for (desired, budget, minimum) in [
            ([30.0, 40.0, 400.0, 600.0], 500.0, 24.0),
            ([30.0, 40.0, 100.0, 100.0], 500.0, 24.0),
            ([1.0, 2.0, 300.0, 400.0], 60.0, 50.0),
        ] {
            let expected = allocate(&desired, budget, minimum);
            for shift in 0..desired.len() {
                for reverse in [false, true] {
                    let mut order: Vec<usize> = (0..desired.len()).collect();
                    order.rotate_left(shift);
                    if reverse {
                        order.reverse();
                    }
                    let reordered: Vec<f32> = order.iter().map(|&index| desired[index]).collect();
                    let reordered_expected: Vec<f32> =
                        order.iter().map(|&index| expected[index]).collect();
                    assert_close(&allocate(&reordered, budget, minimum), &reordered_expected);
                }
            }
        }
    }

    #[test]
    fn rounding_keeps_widths_positive_and_within_budget() {
        for count in [1, 2, 3, 7, 32, 257] {
            let desired: Vec<f32> = (0..count)
                .map(|index| ((index * 97 + 13) % 503) as f32 * 0.3)
                .collect();
            for budget in [
                count as f32 * 0.5,
                count as f32,
                count as f32 * 17.3,
                count as f32 * 400.1,
            ] {
                for minimum in [0.25, 24.0, 4000.0] {
                    let widths = allocate(&desired, budget, minimum);
                    assert_eq!(widths.len(), count);
                    assert_fills_budget(&widths, budget);
                    let floor =
                        f64::from(minimum).min(f64::from(budget).max(count as f64) / count as f64);
                    for width in widths {
                        assert!(f64::from(width) >= floor * (1.0 - f64::from(f32::EPSILON)));
                    }
                }
            }
        }
    }
}
