use std::ops::Range;

use gpui::ScrollHandle;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ListWindow {
    pub visible: Range<usize>,
    pub render: Range<usize>,
    pub preload: Range<usize>,
    pub top_spacer_px: f32,
    pub bottom_spacer_px: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgressiveListPolicy {
    pub initial_count: usize,
    pub batch_size: usize,
    pub preload_items: usize,
}

impl ProgressiveListPolicy {
    pub(crate) const fn new(initial_count: usize, batch_size: usize, preload_items: usize) -> Self {
        Self {
            initial_count,
            batch_size,
            preload_items,
        }
    }

    pub(crate) fn ensure_index(self, current: usize, total: usize, index: usize) -> usize {
        current
            .max(index.saturating_add(self.batch_size))
            .min(total)
    }

    pub(crate) fn extend_near_end(self, current: usize, total: usize, bottom_item: usize) -> usize {
        if current >= total
            || bottom_item.saturating_add(self.preload_items) < current.saturating_sub(1)
        {
            current
        } else {
            current.saturating_add(self.batch_size).min(total)
        }
    }
}

pub(crate) const SEARCH_LIST_POLICY: ProgressiveListPolicy =
    ProgressiveListPolicy::new(100, 100, 12);
pub(crate) const PICKER_LIST_POLICY: ProgressiveListPolicy =
    ProgressiveListPolicy::new(200, 200, 12);

impl ListWindow {
    fn empty() -> Self {
        Self {
            visible: 0..0,
            render: 0..0,
            preload: 0..0,
            top_spacer_px: 0.0,
            bottom_spacer_px: 0.0,
        }
    }
}

pub(crate) fn fixed_list_window(
    scroll_handle: &ScrollHandle,
    fallback_viewport_height: f32,
    prefix_height: f32,
    item_height: f32,
    item_count: usize,
    render_overscan: usize,
    preload_overscan: usize,
) -> ListWindow {
    if item_count == 0 || item_height <= 0.0 {
        return ListWindow::empty();
    }

    let measured_viewport_height: f32 = scroll_handle.bounds().size.height.into();
    let viewport_height = if measured_viewport_height > 0.0 {
        measured_viewport_height
    } else {
        fallback_viewport_height.max(item_height)
    };
    let scroll_y = (-f32::from(scroll_handle.offset().y)).max(0.0);
    fixed_list_window_for_metrics(
        scroll_y,
        viewport_height,
        prefix_height,
        item_height,
        item_count,
        render_overscan,
        preload_overscan,
    )
}

fn fixed_list_window_for_metrics(
    scroll_y: f32,
    viewport_height: f32,
    prefix_height: f32,
    item_height: f32,
    item_count: usize,
    render_overscan: usize,
    preload_overscan: usize,
) -> ListWindow {
    if item_count == 0 || item_height <= 0.0 {
        return ListWindow::empty();
    }

    let content_start = (scroll_y - prefix_height).max(0.0);
    let content_end = (scroll_y + viewport_height - prefix_height).max(0.0);
    let visible_start = ((content_start / item_height).floor() as usize).min(item_count);
    let visible_end = ((content_end / item_height).ceil() as usize)
        .min(item_count)
        .max(visible_start);
    let render_start = visible_start.saturating_sub(render_overscan);
    let render_end = visible_end.saturating_add(render_overscan).min(item_count);
    let preload_start = visible_start.saturating_sub(preload_overscan);
    let preload_end = visible_end.saturating_add(preload_overscan).min(item_count);

    ListWindow {
        visible: visible_start..visible_end,
        render: render_start..render_end,
        preload: preload_start..preload_end,
        top_spacer_px: render_start as f32 * item_height,
        bottom_spacer_px: item_count.saturating_sub(render_end) as f32 * item_height,
    }
}

pub(crate) fn variable_list_window(
    scroll_handle: &ScrollHandle,
    prefix_height: f32,
    item_heights: &[f32],
    render_overscan: usize,
    preload_overscan: usize,
) -> ListWindow {
    if item_heights.is_empty() {
        return ListWindow::empty();
    }

    let viewport_height: f32 = scroll_handle.bounds().size.height.into();
    let scroll_y = (-f32::from(scroll_handle.offset().y)).max(0.0);
    variable_list_window_for_metrics(
        scroll_y,
        viewport_height,
        prefix_height,
        item_heights,
        render_overscan,
        preload_overscan,
    )
}

fn variable_list_window_for_metrics(
    scroll_y: f32,
    viewport_height: f32,
    prefix_height: f32,
    item_heights: &[f32],
    render_overscan: usize,
    preload_overscan: usize,
) -> ListWindow {
    if item_heights.is_empty() {
        return ListWindow::empty();
    }

    let content_start = (scroll_y - prefix_height).max(0.0);
    let content_end = (scroll_y + viewport_height - prefix_height).max(0.0);

    let mut offsets = Vec::with_capacity(item_heights.len() + 1);
    offsets.push(0.0);
    for height in item_heights {
        offsets.push(offsets.last().copied().unwrap_or(0.0) + height.max(0.0));
    }

    let item_count = item_heights.len();
    let visible_start = offsets
        .partition_point(|offset| *offset <= content_start)
        .saturating_sub(1)
        .min(item_count);
    let visible_end = offsets
        .partition_point(|offset| *offset < content_end)
        .min(item_count)
        .max(visible_start);
    let render_start = visible_start.saturating_sub(render_overscan);
    let render_end = visible_end.saturating_add(render_overscan).min(item_count);
    let preload_start = visible_start.saturating_sub(preload_overscan);
    let preload_end = visible_end.saturating_add(preload_overscan).min(item_count);
    let total_height = offsets.last().copied().unwrap_or(0.0);

    ListWindow {
        visible: visible_start..visible_end,
        render: render_start..render_end,
        preload: preload_start..preload_end,
        top_spacer_px: offsets[render_start],
        bottom_spacer_px: total_height - offsets[render_end],
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ProgressiveListPolicy, fixed_list_window_for_metrics, variable_list_window_for_metrics,
    };

    #[test]
    fn fixed_window_separates_visible_render_and_preload_ranges() {
        let window = fixed_list_window_for_metrics(200.0, 100.0, 0.0, 20.0, 100, 2, 5);

        assert_eq!(window.visible, 10..15);
        assert_eq!(window.render, 8..17);
        assert_eq!(window.preload, 5..20);
        assert_eq!(window.top_spacer_px, 160.0);
        assert_eq!(window.bottom_spacer_px, 1660.0);
    }

    #[test]
    fn variable_window_uses_prefix_heights_for_spacers() {
        let heights = [10.0, 20.0, 30.0, 40.0];
        let window = variable_list_window_for_metrics(30.0, 40.0, 0.0, &heights, 1, 2);

        assert_eq!(window.visible, 2..4);
        assert_eq!(window.render, 1..4);
        assert_eq!(window.preload, 0..4);
        assert_eq!(window.top_spacer_px, 10.0);
        assert_eq!(window.bottom_spacer_px, 0.0);
    }

    #[test]
    fn progressive_policy_extends_only_near_the_rendered_end() {
        let policy = ProgressiveListPolicy::new(100, 100, 12);
        assert_eq!(policy.extend_near_end(100, 500, 50), 100);
        assert_eq!(policy.extend_near_end(100, 500, 88), 200);
        assert_eq!(policy.ensure_index(100, 500, 250), 350);
    }
}
