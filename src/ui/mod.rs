//! Shared application controls. Business actions and document state belong to
//! their callers; visual rules belong to `style`, and palette values to `theme`.
//!
//! Shared controls preserve caller-owned focus, dismissal, keyboard navigation
//! and IME behavior. Semantic slots default to debugger fields; application
//! overrides never recolor document diagrams.

#[cfg(any(target_os = "linux", all(test, feature = "gpui-test-support")))]
pub(crate) mod app_menu;
pub(crate) mod button;
pub(crate) mod decoration;
pub(crate) mod field;
pub(crate) mod input;
pub(crate) mod interaction;
pub(crate) mod list;
pub(crate) mod motion;
pub(crate) mod self_drawn;
pub(crate) mod surface;
pub(crate) mod switch;
pub(crate) mod typography;

#[cfg(test)]
mod tests {
    use crate::{
        style::*,
        theme::{ThemeKind, UiColor},
    };
    use gpui::{Styled, px};

    #[test]
    fn default_explicit_shadows_match_previous_native_presets() {
        use gpui::prelude::*;
        let theme = ThemeKind::OneDark.build();
        let styles = Styles::from_theme(&theme, &theme);
        let mut small = gpui::div().shadow_sm();
        let mut popup = gpui::div().shadow_md();
        assert_eq!(
            Some(styles.ui.small_shadow.layers(1.0)),
            small.style().box_shadow
        );
        assert_eq!(
            Some(styles.ui.popup_shadow.layers(1.0)),
            popup.style().box_shadow
        );
    }

    #[test]
    fn appearance_changes_chrome_without_restyling_document_content() {
        let theme = ThemeKind::OneDark.build();
        let original = Styles::from_theme(&theme, &theme);
        for corners in UiCorners::ALL {
            let mut spec = UiStyleSpec::default();
            spec.appearance.corners = corners;
            let styles = Styles::from_theme_with_spec(&theme, &theme, false, spec);
            let factor = corners.factor();
            assert_eq!(
                styles.settings_panel.settings_input_radius_px,
                if corners == UiCorners::Full {
                    styles.settings_panel.settings_input_height_px * 0.5
                } else {
                    original.settings_panel.settings_input_radius_px * factor
                }
            );
            assert_eq!(
                styles.context_menu.menu_radius_px,
                original.context_menu.menu_radius_px
                    * if corners == UiCorners::Full {
                        1.5
                    } else {
                        factor
                    }
            );
            assert_eq!(
                styles.ui.control.radius_px,
                original.ui.control.radius_px * factor
            );
            let mut button = super::button::button(
                "corners",
                styles.settings_header_button(),
                ControlState::default(),
                1.0,
            );
            assert_eq!(
                button.style().corner_radii.top_left,
                Some(
                    px(if corners == UiCorners::Full {
                        styles.settings_header_button().height_px * 0.5
                    } else {
                        styles.settings_header_button().radius_px
                    })
                    .into()
                )
            );
            assert_eq!(
                styles.ui.spec.application.theme_card_radius_px,
                original.ui.spec.application.theme_card_radius_px * factor
            );
            assert_eq!(
                styles.code_block.code_block_radius_px,
                original.code_block.code_block_radius_px
            );
            assert_eq!(
                styles.table.table_outer_radius_px,
                original.table.table_outer_radius_px
            );
            assert_eq!(
                styles.switch.track_radius_px,
                original.switch.track_radius_px
            );
            let rebuilt = styles.retheme(&theme, &theme, false);
            assert_eq!(
                rebuilt.settings_panel.settings_input_radius_px,
                styles.settings_panel.settings_input_radius_px
            );
        }
    }

    #[test]
    fn full_round_menus_keep_full_width_rows_outside_corner_bands() {
        let theme = ThemeKind::OneDark.build();
        let original = Styles::from_theme(&theme, &theme);
        for scale in [1.0, 1.5, 2.0] {
            let mut spec = UiStyleSpec::default();
            spec.appearance.corners = UiCorners::Full;
            let styles = Styles::from_theme_with_spec(&theme, &theme, false, spec);
            assert_eq!(styles.context_menu.menu_radius_px, 12.0);
            assert_eq!(styles.search_menu_surface().radius_px, 12.0);
            assert_eq!(styles.command_surface().radius_px, 12.0);
            assert_eq!(
                styles.text_input.menu.menu_radius_px,
                styles.context_menu.menu_radius_px
            );
            assert!(styles.context_menu.menu_padding_y_px >= styles.context_menu.menu_radius_px);
            assert!(
                styles.text_input.menu.menu_padding_y_px >= styles.text_input.menu.menu_radius_px
            );
            assert!(
                styles.ui.spec.search_menu_input_height_px()
                    >= styles.search_menu_surface().radius_px
            );
            assert!(
                styles.ui.spec.search_menu_bottom_padding_px()
                    >= styles.search_menu_surface().radius_px
            );
            for item in [
                styles.menu_item(false),
                styles.context_menu.item(),
                styles.text_input.menu.item(),
                styles.search_menu_item(),
                styles.command_item(),
            ] {
                assert_eq!(item.inset_x_px, 0.0);
                let mut row = super::list::list_item(item, ControlState::default(), scale);
                assert_eq!(row.style().margin.left, None);
                assert_eq!(row.style().margin.right, None);
                assert_eq!(row.style().size.width, Some(gpui::relative(1.0).into()));
            }
            let mut scroll = super::surface::menu_scroll_area("test-menu-scroll");
            assert_eq!(scroll.style().overflow.y, Some(gpui::Overflow::Scroll));
            assert_eq!(scroll.style().size.width, Some(gpui::relative(1.0).into()));
            assert_eq!(original.context_menu.item().inset_x_px, 0.0);
            assert_eq!(original.search_menu_item().inset_x_px, 0.0);
            assert_eq!(original.command_item().inset_x_px, 0.0);
        }

        let mut spec = UiStyleSpec::default();
        spec.appearance.corners = UiCorners::Full;
        spec.menu_radius_px = 15.0;
        let styles = Styles::from_theme_with_spec(&theme, &theme, false, spec);
        assert_eq!(styles.search_menu_surface().radius_px, 22.5);
        assert_eq!(styles.search_menu_item().inset_x_px, 0.0);
        assert!(styles.ui.spec.search_menu_input_height_px() >= 22.5);
        assert!(styles.ui.spec.search_menu_bottom_padding_px() >= 22.5);
        let rebuilt = styles.retheme(&theme, &theme, false);
        assert_eq!(rebuilt.search_menu_surface().radius_px, 22.5);
        assert_eq!(rebuilt.search_menu_item().inset_x_px, 0.0);
    }

    #[test]
    fn search_close_button_is_round_in_both_rounded_modes() {
        let theme = ThemeKind::OneDark.build();
        for corners in UiCorners::ALL {
            let mut spec = UiStyleSpec::default();
            spec.appearance.corners = corners;
            let styles = Styles::from_theme_with_spec(&theme, &theme, false, spec);
            let close = styles.search_navigation(true);
            for scale in [1.0, 1.5, 2.0] {
                let mut button = super::button::icon_button(
                    "search-close-test",
                    close,
                    ControlState::default(),
                    scale,
                );
                let expected_radius = if corners == UiCorners::Square {
                    0.0
                } else {
                    close.height_px * 0.5
                };
                assert_eq!(
                    button.style().corner_radii.top_left,
                    Some(px(expected_radius * scale).into())
                );
                assert_eq!(
                    button.style().size.width,
                    Some(px(close.height_px * scale).into())
                );
            }
        }
    }

    #[test]
    fn dialog_buttons_use_half_the_scaled_height_in_full_round_mode() {
        let theme = ThemeKind::OneDark.build();
        for corners in UiCorners::ALL {
            let mut spec = UiStyleSpec::default();
            spec.appearance.corners = corners;
            let styles = Styles::from_theme_with_spec(&theme, &theme, false, spec);
            let control = styles.dialog_button(
                styles.dialog.dialog_bg,
                styles.dialog.dialog_message_text_color,
                styles.dialog.dialog_btn_secondary_hover_bg,
            );
            for scale in [0.75, 1.0, 1.5, 2.0] {
                let mut button =
                    super::button::button_frame(control, ControlState::default(), scale);
                let expected_radius = match corners {
                    UiCorners::Square => 0.0,
                    UiCorners::Half => styles.dialog.dialog_btn_radius_px,
                    UiCorners::Full => control.height_px * scale * 0.5,
                };
                assert_eq!(
                    button.style().corner_radii.top_left,
                    Some(px(expected_radius).into())
                );
                assert_eq!(
                    button.style().size.height,
                    Some(px(control.height_px * scale).into())
                );
            }
        }
    }

    #[test]
    fn dialog_buttons_have_borders_with_and_without_elevation() {
        for kind in ThemeKind::light_themes()
            .iter()
            .chain(ThemeKind::dark_themes())
        {
            let theme = kind.build();
            for elevation in [false, true] {
                let mut spec = UiStyleSpec::default();
                spec.appearance.elevation = elevation;
                let styles = Styles::from_theme_with_spec(&theme, &theme, false, spec);
                let button = styles.dialog_button(
                    styles.dialog.dialog_bg,
                    styles.dialog.dialog_message_text_color,
                    styles.dialog.dialog_btn_secondary_hover_bg,
                );
                assert!(button.border_width_px > 0.0);
                assert!(button.normal.border.a > 0.0);
                assert_eq!(button.hover.border, button.normal.border);
            }
        }
    }

    #[test]
    fn workspace_search_close_matches_toggle_button_size_and_rounding() {
        let theme = ThemeKind::OneDark.build();
        for corners in UiCorners::ALL {
            let mut spec = UiStyleSpec::default();
            spec.appearance.corners = corners;
            let styles = Styles::from_theme_with_spec(&theme, &theme, false, spec);
            let close = styles.workspace_close_search_button();
            let toggle = styles.workspace_toggle();
            assert_eq!(close.height_px, toggle.height_px);
            assert_eq!(close.hover.background, toggle.hover.background);
            for scale in [1.0, 1.5, 2.0] {
                let mut button = super::button::icon_button(
                    "workspace-close",
                    close,
                    ControlState::default(),
                    scale,
                );
                let radius = if corners == UiCorners::Square {
                    0.0
                } else {
                    close.height_px * 0.5
                };
                assert_eq!(
                    button.style().corner_radii.top_left,
                    Some(px(radius * scale).into())
                );
                assert_eq!(
                    button.style().size.width,
                    Some(px(close.height_px * scale).into())
                );
            }
        }
    }

    #[test]
    fn raised_appearance_shadows_only_surfaced_controls_and_selected_segments() {
        let theme = ThemeKind::OneDark.build();
        let mut spec = UiStyleSpec::default();
        spec.appearance.elevation = true;
        let styles = Styles::from_theme_with_spec(&theme, &theme, false, spec);
        assert!(styles.ui.control.elevation.is_some());
        let shadow = ElevationStyle::raised(false).shadows(1.0);
        assert_eq!(shadow.len(), 2);
        assert!(shadow.iter().all(|layer| layer.spread_radius == px(0.0)));
        assert!(shadow.iter().all(|layer| layer.blur_radius <= px(2.0)));
        assert!(
            !super::button::raised_shadows(
                styles
                    .ui
                    .spec
                    .button(&theme, ButtonVariant::Default, ButtonSize::Default),
                ControlState::default(),
                1.0,
            )
            .is_empty()
        );
        let ghost = styles
            .ui
            .spec
            .button(&theme, ButtonVariant::Ghost, ButtonSize::Default);
        assert!(super::button::raised_shadows(ghost, ControlState::default(), 1.0).is_empty());
        let segment = styles.raised_segment(styles.settings_toggle());
        assert_eq!(segment.hover.background, styles.ui.segment_hover);
        let header = styles.settings_header_button();
        assert_ne!(header.hover.background, header.normal.background);
        assert!(super::button::raised_shadows(segment, ControlState::default(), 1.0).is_empty());
        assert!(
            !super::button::raised_shadows(
                segment,
                ControlState {
                    selected: true,
                    ..Default::default()
                },
                1.0
            )
            .is_empty()
        );
        assert!(styles.segmented_group(segment.radius_px).tray.is_some());
        let normal = Styles::from_theme(&theme, &theme);
        assert!(
            normal
                .segmented_group(normal.settings_toggle().radius_px)
                .tray
                .is_none()
        );
        assert!(normal.ui.control.elevation.is_none());
    }

    #[test]
    fn tab_chrome_ignores_corner_and_elevation_options() {
        let theme = ThemeKind::OneDark.build();
        let baseline = Styles::from_theme(&theme, &theme);
        for corners in UiCorners::ALL {
            for elevation in [false, true] {
                let mut spec = UiStyleSpec::default();
                spec.appearance = UiAppearance { corners, elevation };
                let styles = Styles::from_theme_with_spec(&theme, &theme, false, spec);
                let tab = styles.tab_surface();
                let height = styles.tab_bar.tab_bar_height_px;
                let mut active = super::chrome::tab_surface(tab, 1.0, height, true);
                let mut inactive = super::chrome::tab_surface(tab, 1.0, height, false);
                assert_eq!(active.style().size.height, Some(px(height).into()));
                assert_eq!(inactive.style().size.height, active.style().size.height);
                assert_eq!(active.style().margin.top, None);
                assert_eq!(active.style().margin.bottom, None);
                assert_eq!(inactive.style().margin.top, None);
                assert_eq!(inactive.style().margin.bottom, None);
                assert_eq!(active.style().corner_radii.top_left, None);
                assert!(active.style().box_shadow.is_none());
                assert!(inactive.style().box_shadow.is_none());
                assert_eq!(
                    active.style().border_widths.right,
                    Some(px(baseline.tab_bar.tab_item_border_width_px).into())
                );
                assert_eq!(
                    styles.tab_bar.tab_item_badge_radius_px,
                    baseline.tab_bar.tab_item_badge_radius_px
                );
                assert_eq!(
                    styles.tab_bar.tab_update_button_radius_px,
                    baseline.tab_bar.tab_update_button_radius_px
                );
                assert_eq!(
                    styles.tab_bar.tab_history_btn_radius_px,
                    baseline.tab_bar.tab_history_btn_radius_px
                );
                assert_eq!(
                    styles.tab_name(true, false, false).text.weight,
                    styles.tab_name(false, false, false).text.weight
                );
                assert_eq!(
                    styles.tab_name(true, false, false).text.size_px,
                    styles.tab_name(false, false, false).text.size_px
                );
                assert_eq!(
                    styles.tab_bar.tab_item_icon_size_px,
                    baseline.tab_bar.tab_item_icon_size_px
                );
            }
        }
    }

    #[test]
    fn clickable_text_and_icon_metrics_stay_constant_when_pressed() {
        let theme = ThemeKind::OneDark.build();
        let styles = Styles::from_theme(&theme, &theme);
        for control in [
            styles.settings_header_button(),
            styles.settings_toggle(),
            styles.tab_close(true, false),
        ] {
            let mut idle = super::button::button_frame(control, ControlState::default(), 1.5);
            let mut pressed = super::button::button_frame(
                control,
                ControlState {
                    pressed: true,
                    ..Default::default()
                },
                1.5,
            );
            assert_eq!(idle.style().size, pressed.style().size);
            assert_eq!(
                idle.style().text_style().as_ref().unwrap().font_size,
                pressed.style().text_style().as_ref().unwrap().font_size
            );
            assert_eq!(
                idle.style().text_style().as_ref().unwrap().font_weight,
                pressed.style().text_style().as_ref().unwrap().font_weight
            );
            let mut icon = super::button::icon(
                control,
                "assets/icons/navigation/arrow-down.svg",
                control.normal.foreground,
                1.5,
            );
            assert_eq!(
                icon.style().size.width,
                Some(px(control.icon_size_px * 1.5).into())
            );
        }
    }

    #[test]
    fn filename_suffix_follows_the_natural_width_of_a_shrinkable_basename() {
        let (name, extension) = crate::editor::split_file_name_for_display("help-zh-cn.md");
        assert_eq!((name, extension), ("help-zh-cn", Some(".md")));
        let mut base = super::decoration::file_name_base("help-zh-cn");
        let mut suffix = super::decoration::suffix(0.5, ".md");
        let theme = ThemeKind::OneDark.build();
        let styles = Styles::from_theme(&theme, &theme);
        let mut row = super::decoration::file_name(styles.sidebar_name(theme.text_primary), 1.0);
        assert_eq!(base.style().flex_grow, Some(0.0));
        assert_eq!(base.style().flex_shrink, Some(1.0));
        assert_eq!(base.style().size.width, None);
        assert_eq!(suffix.style().flex_grow, Some(0.0));
        assert_eq!(suffix.style().flex_shrink, Some(0.0));
        assert_eq!(row.style().gap.width, Some(px(0.0).into()));
    }

    #[test]
    fn raised_sidebar_selection_uses_an_inset_surface_without_changing_row_height() {
        let theme = ThemeKind::OneDark.build();
        let normal = Styles::from_theme(&theme, &theme);
        assert_eq!(normal.sidebar_tree_row().selected_inset_px, 0.0);
        let mut spec = UiStyleSpec::default();
        spec.appearance.corners = UiCorners::Full;
        spec.appearance.elevation = true;
        let styles = Styles::from_theme_with_spec(&theme, &theme, false, spec);
        let row_style = styles.sidebar_tree_row();
        assert_eq!(row_style.selected_inset_px, 4.0);
        assert!(row_style.selected_elevation.is_some());
        assert_eq!(
            row_style.selected_radius_px,
            (row_style.height_px - 2.0) * 0.5
        );
        let mut row = super::list::tree_row(
            row_style,
            TreeRowState {
                selected: true,
                ..Default::default()
            },
            1.5,
        );
        assert_eq!(
            row.style().size.height,
            Some(px(row_style.height_px * 1.5).into())
        );
        assert_eq!(
            row_style.selected_background,
            styles.sidebar.sidebar_item_raised_bg
        );
    }

    #[test]
    fn raised_settings_header_keeps_the_native_palette_with_quieter_chrome() {
        for kind in ThemeKind::light_themes()
            .iter()
            .chain(ThemeKind::dark_themes())
        {
            let theme = kind.build();
            let baseline = Styles::from_theme(&theme, &theme);
            let mut spec = UiStyleSpec::default();
            spec.appearance.elevation = true;
            let raised = Styles::from_theme_with_spec(&theme, &theme, false, spec);
            assert_eq!(baseline.ui.spec.settings_padding_y_px, 36.0);
            assert_eq!(raised.ui.spec.settings_padding_y_px, 36.0);
            assert_eq!(
                raised.settings_header_button().normal.background,
                raised.ui.raised_surface
            );
            assert_ne!(
                raised.settings_header_button().hover.background,
                raised.settings_header_button().normal.background
            );
            assert_ne!(raised.ui.segment_hover, raised.ui.segment_tray);
            assert_eq!(
                raised.ui.segment_tray,
                theme.with_light_surface_contrast().bg_secondary
            );
            assert_eq!(
                raised.settings_header_button().normal.border,
                raised.settings_panel.settings_site_button_border_color
            );
            for style in [
                raised.settings_header_button(),
                raised.settings_picker(),
                raised.settings_folder_picker(),
                raised.settings_stepper(),
                raised.dialog_button(
                    raised.dialog.dialog_bg,
                    raised.dialog.dialog_message_text_color,
                    raised.dialog.dialog_btn_secondary_hover_bg,
                ),
            ] {
                assert!(style.border_width_px > 0.0);
                assert!(style.normal.border.a > 0.0);
                assert_eq!(style.hover.border, style.normal.border);
                assert!(
                    !super::button::raised_shadows(style, ControlState::default(), 1.0).is_empty()
                );
            }
            assert_eq!(
                raised.settings_header_button().height_px,
                baseline.settings_header_button().height_px
            );
        }
    }

    #[test]
    fn settings_vertical_padding_is_independent_of_elevation() {
        let theme = ThemeKind::OneDark.build();
        for padding in [0.0, 36.0, 60.0, 84.0] {
            for elevation in [false, true] {
                let mut spec = UiStyleSpec::default();
                spec.settings_padding_y_px = padding;
                spec.appearance.elevation = elevation;
                let styles = Styles::from_theme_with_spec(&theme, &theme, false, spec);
                assert_eq!(styles.ui.spec.settings_padding_y_px, padding);
            }
        }
    }

    #[test]
    fn button_variant_size_state_matrix_reaches_native_elements() {
        let theme = ThemeKind::OneDark.build();
        let spec = UiStyleSpec::default();
        for variant in [
            ButtonVariant::Default,
            ButtonVariant::Outline,
            ButtonVariant::Secondary,
            ButtonVariant::Ghost,
            ButtonVariant::Destructive,
            ButtonVariant::Link,
        ] {
            for size in [
                ButtonSize::Default,
                ButtonSize::Xs,
                ButtonSize::Sm,
                ButtonSize::Lg,
                ButtonSize::Icon,
                ButtonSize::IconXs,
                ButtonSize::IconSm,
                ButtonSize::IconLg,
            ] {
                let style = spec.button(&theme, variant, size);
                for state in [
                    ControlState::default(),
                    ControlState {
                        hovered: true,
                        ..Default::default()
                    },
                    ControlState {
                        pressed: true,
                        ..Default::default()
                    },
                    ControlState {
                        selected: true,
                        hovered: true,
                        ..Default::default()
                    },
                    ControlState {
                        checked: true,
                        ..Default::default()
                    },
                    ControlState {
                        open: true,
                        ..Default::default()
                    },
                    ControlState {
                        invalid: true,
                        focus_visible: true,
                        ..Default::default()
                    },
                    ControlState {
                        disabled: true,
                        hovered: true,
                        ..Default::default()
                    },
                    ControlState {
                        loading: true,
                        ..Default::default()
                    },
                ] {
                    let mut icon = super::button::icon(
                        style,
                        "assets/icons/navigation/arrow-down.svg",
                        style.resolve(state).foreground,
                        1.5,
                    );
                    assert_eq!(
                        icon.style().size.width,
                        Some(px(style.icon_size_px * 1.5).into())
                    );
                    let mut element = super::button::button("matrix", style, state, 1.5);
                    assert_eq!(
                        element.style().size.height,
                        Some(px(style.height_px * 1.5).into())
                    );
                    assert_eq!(element.style().opacity, Some(style.resolve(state).opacity));
                    assert!(style.icon_size_px <= style.height_px);
                }
            }
        }
    }

    #[test]
    fn interaction_precedence_is_independent_of_builder_order() {
        let theme = ThemeKind::OneDark.build();
        let style =
            UiStyleSpec::default().button(&theme, ButtonVariant::Outline, ButtonSize::Default);
        let disabled = style.resolve(ControlState {
            disabled: true,
            hovered: true,
            pressed: true,
            ..Default::default()
        });
        assert_eq!(disabled.background, style.normal.background);
        assert_eq!(disabled.opacity, style.disabled_opacity);
        let selected_hover = style.resolve(ControlState {
            checked: true,
            hovered: true,
            ..Default::default()
        });
        assert_eq!(selected_hover.background, style.selected_hover.background);
        let invalid_focus = style.resolve(ControlState {
            invalid: true,
            focus_visible: true,
            ..Default::default()
        });
        assert_eq!(invalid_focus.border, style.ring.invalid_color);
        let read_only = style.resolve(ControlState {
            read_only: true,
            pressed: true,
            ..Default::default()
        });
        assert_eq!(read_only.background, style.normal.background);
    }

    #[test]
    fn nondefault_spec_changes_real_control_input_list_and_surface_metrics_only() {
        let theme = ThemeKind::OneDark.build();
        let original = Styles::from_theme(&theme, &theme);
        let mut spec = UiStyleSpec::default();
        spec.menu_row_height_px = 37.0;
        spec.menu_radius_px = 15.0;
        spec.spacing_px = 7.0;
        spec.settings_max_width_px = 760.0;
        spec.control_heights_px[0] = 41.0;
        spec.focus_ring_width_px = 3.0;
        let changed = Styles::from_theme_with_spec(&theme, &theme, true, spec);
        let mut control =
            super::button::button("custom", changed.ui.control, Default::default(), 1.0);
        assert_eq!(control.style().size.height, Some(px(41.0).into()));
        let mut row = super::list::list_item(changed.search_menu_item(), Default::default(), 2.0);
        assert_eq!(row.style().size.height, Some(px(74.0).into()));
        let mut input =
            super::input::input_shell(&changed.input_styles(InputRole::Picker, 2.0), false, 74.0);
        assert_eq!(input.style().size.height, Some(px(74.0).into()));
        assert_eq!(
            changed
                .input_styles(InputRole::Picker, 2.0)
                .text_input_height_px,
            74.0
        );
        let mut surface = super::surface::surface(&changed.search_menu_surface(), 2.0);
        assert_eq!(surface.style().corner_radii.top_left, Some(px(30.0).into()));
        assert_eq!(changed.settings_field().gap_px, 21.0);
        assert_eq!(changed.ui.spec.search_menu_input_height_px(), 46.0);
        assert_eq!(changed.context_menu.menu_item_height_px, 37.0);
        assert_eq!(changed.text_input.menu.menu_item_height_px, 37.0);
        assert_eq!(
            changed.editor.editor_font_size_px,
            original.editor.editor_font_size_px
        );
        assert_eq!(
            changed.editor.editor_paragraph_line_height_factor,
            original.editor.editor_paragraph_line_height_factor
        );
        assert_eq!(
            changed.checkbox.checkbox_border_width_px,
            original.checkbox.checkbox_border_width_px
        );
        assert_eq!(
            changed.selection.selection_color,
            original.selection.selection_color
        );
        assert_eq!(
            changed.ime.ime_underline_color,
            original.ime.ime_underline_color
        );
    }

    #[test]
    fn default_application_roles_preserve_metrics_and_theme_surface_ownership() {
        for kind in ThemeKind::light_themes()
            .iter()
            .chain(ThemeKind::dark_themes())
        {
            let theme = kind.build();
            for dark_sidebar in [false, true] {
                let styles = Styles::from_theme_with_options(&theme, &theme, dark_sidebar);
                assert_eq!(
                    styles.settings_header_button().height_px,
                    styles.settings_panel.settings_site_button_height_px
                );
                assert_eq!(styles.menu_item(false).control.height_px, 24.0);
                assert_eq!(styles.search_menu_item().control.height_px, 24.0);
                assert_eq!(styles.search_menu_surface().radius_px, 8.0);
                assert_eq!(
                    styles
                        .input_styles(InputRole::Replace, 1.5)
                        .text_input_height_px,
                    styles
                        .input_styles(InputRole::Search, 1.5)
                        .text_input_height_px
                );
                assert_eq!(
                    styles
                        .input_styles(InputRole::Rename, 1.0)
                        .text_input_text_color,
                    styles.sidebar.sidebar_edit_input_text_color
                );
                assert_eq!(styles.text_input.menu.menu_bg, styles.context_menu.menu_bg);
                let filter = styles.input_styles(InputRole::SidebarFilter, 1.5);
                assert_eq!(filter.text_input_bg, gpui::transparent_black());
                assert_eq!(filter.text_input_focus_bg, gpui::transparent_black());
                assert!(filter.text_input_hover_bg.is_none());
                assert_eq!(filter.text_input_border_width_px, 0.0);
                assert_eq!(filter.text_input_focus_ring.width_px, 0.0);
                assert_eq!(
                    filter.text_input_focus_placeholder_color,
                    Some(styles.sidebar.sidebar_filter_muted_color)
                );
                assert_eq!(
                    filter.text_input_height_px,
                    styles.sidebar.sidebar_header_height_px * 1.5
                );
                assert_eq!(
                    styles.text_input.menu.menu_item_height_px,
                    styles.context_menu.menu_item_height_px
                );
                assert_eq!(styles.dialog_surface().background, styles.dialog.dialog_bg);
                assert_eq!(styles.theme_debugger.foreground, theme.text_primary);
            }
        }
    }

    #[test]
    fn semantic_slots_follow_debugger_source_without_a_second_palette() {
        for kind in ThemeKind::light_themes()
            .iter()
            .chain(ThemeKind::dark_themes())
        {
            for slot in UiColor::ALL {
                let mut theme = kind.build();
                assert_eq!(theme.ui_color(slot), theme.color(slot.source()));
                let custom = crate::hex_hsla!("#536A89CC");
                theme.set_color(slot.source(), custom);
                assert_eq!(theme.ui_color(slot), custom);
            }
        }
    }
}

pub(crate) mod card;
pub(crate) mod chrome;
pub(crate) mod panel;

#[cfg(test)]
mod application_tests {
    use crate::{style::*, theme::ThemeKind};
    use gpui::{Styled, px};

    #[test]
    fn settings_separator_preserves_fixed_pixels_at_all_ui_scales() {
        let theme = ThemeKind::OneDark.build();
        let styles = Styles::from_theme(&theme, &theme);
        for scale in [0.75, 1.0, 1.25, 1.5, 2.0] {
            let mut settings =
                super::surface::separator(styles.settings_section_heading().separator, scale);
            let mut menu = super::surface::separator(styles.context_menu.separator(), scale);
            assert_eq!(settings.style().size.height, Some(px(1.0).into()));
            assert_eq!(
                menu.style().size.height,
                Some(px(styles.context_menu.menu_separator_height_px * scale).into())
            );
        }
    }

    #[test]
    fn nondefault_application_metrics_reach_real_compositions() {
        let theme = ThemeKind::OneDark.build();
        let mut spec = UiStyleSpec::default();
        spec.application.status_height_px = 31.0;
        spec.application.theme_card_padding_px = 17.0;
        spec.application.drag_cursor_gap_px = 23.0;
        spec.application.switch_size = SwitchSize::Sm;
        let styles = Styles::from_theme_with_spec(&theme, &theme, true, spec);
        let mut bar = super::chrome::status_bar(styles.status_bar(), 2.0, 0.0);
        assert_eq!(bar.style().size.height, Some(px(62.0).into()));
        let mut card = super::card::theme_card(styles.theme_card(&theme, true), 2.0);
        assert_eq!(card.style().padding.left, Some(px(34.0).into()));
        let drag = styles.drag_preview(true, 2.0);
        let mut preview =
            super::surface::drag_preview(&drag, gpui::point(px(4.0), px(6.0)), "item");
        assert_eq!(preview.style().padding.left, Some(px(27.0).into()));
        let compact = styles
            .switch
            .for_size(styles.ui.spec.application.switch_size);
        assert_eq!(compact.track_height_px, 16.0);
        assert_eq!(compact.thumb_size_px, 10.0);
        assert!(compact.thumb_offset(true) + compact.thumb_size_px < compact.track_width_px);
    }

    #[test]
    fn badge_toggle_group_and_native_caption_variants_render() {
        let theme = ThemeKind::OneDark.build();
        let spec = UiStyleSpec::default();
        for variant in [
            BadgeVariant::Default,
            BadgeVariant::Secondary,
            BadgeVariant::Outline,
            BadgeVariant::Destructive,
            BadgeVariant::Ghost,
            BadgeVariant::Link,
        ] {
            let style = spec.badge(&theme, variant);
            let mut badge = super::typography::badge(style, 1.5);
            assert_eq!(
                badge.style().padding.left,
                Some(px(style.padding_x_px * 1.5).into())
            );
        }
        for variant in [ToggleVariant::Default, ToggleVariant::Outline] {
            for size in [ToggleSize::Default, ToggleSize::Sm, ToggleSize::Lg] {
                let style = spec.toggle(&theme, variant, size);
                let state = ControlState {
                    indeterminate: true,
                    ..Default::default()
                };
                assert_eq!(style.resolve(state).background, style.selected.background);
                let mut toggle = super::button::button("toggle", style, state, 1.0);
                assert_eq!(toggle.style().size.height, Some(px(style.height_px).into()));
            }
        }
        for orientation in [Orientation::Horizontal, Orientation::Vertical] {
            let mut group = super::button::toggle_group(
                ToggleGroupStyle {
                    tray: None,
                    alignment: GroupAlignment::Stretch,
                    orientation,
                    gap_px: 7.0,
                },
                2.0,
            );
            assert_eq!(group.style().gap.width, Some(px(14.0).into()));
        }
        let styles = Styles::from_theme(&theme, &theme);
        let windows = styles.caption_button(true, true, 32.0, 1.0);
        assert_eq!(windows.min_width_px, WINDOW_CAPTION_BUTTON_WIDTH_PX);
        assert_eq!(windows.height_px, 32.0);
        assert_eq!(
            windows.hover.background,
            styles.ui.colors.caption_close_hover_background
        );
        let linux = styles.caption_button(false, false, 32.0, 1.0);
        assert_eq!(linux.radius_px, 10.0);
        assert_eq!(linux.icon_size_px, 12.0);
    }

    #[test]
    fn ring_offset_has_an_independent_surface_colored_gap() {
        let theme = ThemeKind::OneDark.build();
        let mut spec = UiStyleSpec::default();
        spec.focus_ring_width_px = 2.0;
        spec.focus_ring_offset_px = 3.0;
        let style = spec.button(&theme, ButtonVariant::Outline, ButtonSize::Default);
        let layers = style.ring.shadows(2.0, false);
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].spread_radius, px(10.0));
        assert_eq!(layers[1].spread_radius, px(6.0));
        assert_eq!(layers[1].color, style.ring.offset_color);
        let mut control = super::button::button(
            "ring",
            style,
            ControlState {
                focus_visible: true,
                ..Default::default()
            },
            2.0,
        );
        assert_eq!(control.style().box_shadow, Some(layers));
    }

    #[test]
    fn inverted_bold_menu_resolves_the_live_shared_palette() {
        let theme = ThemeKind::OneDark.build();
        let mut spec = UiStyleSpec::default();
        spec.menu_surface = MenuSurface::Inverted;
        spec.menu_emphasis = MenuEmphasis::Bold;
        let styles = Styles::from_theme_with_spec(&theme, &theme, true, spec);
        assert_eq!(styles.context_menu.menu_bg, theme.text_primary);
        assert_eq!(
            styles.context_menu.menu_item_primary_text_color,
            theme.bg_primary
        );
        assert_eq!(
            styles.context_menu.item().control.selected.background,
            theme.bg_selected
        );
        let mut panel = super::surface::surface(&styles.context_menu.surface(), 1.0);
        assert!(panel.style().background.is_some());
        assert_eq!(
            styles.editor.editor_font_size_px,
            Styles::from_theme(&theme, &theme)
                .editor
                .editor_font_size_px
        );
    }
}

#[cfg(test)]
mod semantic_override_tests {
    use crate::{
        style::*,
        theme::{ThemeKind, UiColor, UiColorOverrides},
    };
    use gpui::{Styled, div};

    #[test]
    fn each_semantic_override_is_independent_and_can_return_to_its_debugger_default() {
        let theme = ThemeKind::OneDark.build();
        let custom = crate::hex_hsla!("#4582A1");
        for selected in UiColor::ALL {
            let mut colors = UiColorOverrides::default();
            colors.set(selected, custom);
            for slot in UiColor::ALL {
                assert_eq!(
                    colors.resolve(&theme, slot),
                    if slot == selected {
                        custom
                    } else {
                        theme.ui_color(slot)
                    }
                );
            }
            colors.clear(selected);
            assert_eq!(colors.resolve(&theme, selected), theme.ui_color(selected));
        }
    }

    #[test]
    fn primary_card_popover_input_and_sidebar_overrides_reach_native_consumers_without_core_drift()
    {
        let theme = ThemeKind::OneDark.build();
        let original = Styles::from_theme(&theme, &theme);
        let primary = crate::hex_hsla!("#CB385C");
        let card = crate::hex_hsla!("#3B6289");
        let popover = crate::hex_hsla!("#21685A");
        let input = crate::hex_hsla!("#DC9338");
        let sidebar = crate::hex_hsla!("#5D3E8A");
        let mut spec = UiStyleSpec::default();
        for (slot, color) in [
            (UiColor::Primary, primary),
            (UiColor::Card, card),
            (UiColor::Popover, popover),
            (UiColor::Input, input),
            (UiColor::Sidebar, sidebar),
        ] {
            spec.colors.set(slot, color);
        }
        let changed = Styles::from_theme_with_spec(&theme, &theme, true, spec);
        assert_eq!(
            changed
                .ui
                .spec
                .button(&theme, ButtonVariant::Default, ButtonSize::Default)
                .normal
                .background,
            primary
        );
        assert_eq!(changed.switch.colors.checked_background, primary);
        assert_eq!(
            changed.ui.control.normal.foreground,
            original.ui.control.normal.foreground
        );
        let mut card_element = super::card::theme_card(changed.theme_card(&theme, false), 1.0);
        let mut expected_card = div().bg(card);
        assert_eq!(
            card_element.style().background,
            expected_card.style().background
        );
        let mut menu = super::surface::surface(&changed.context_menu.surface(), 1.0);
        let mut expected_menu = div().bg(popover);
        assert_eq!(menu.style().background, expected_menu.style().background);
        assert_eq!(changed.quick_open.quick_open_panel_bg, popover);
        assert_eq!(changed.text_input.menu.menu_bg, popover);
        assert_eq!(changed.settings_input_frame().border_color, input);
        assert_eq!(
            changed.context_menu.menu_border_color,
            original.context_menu.menu_border_color
        );
        assert_eq!(changed.sidebar.sidebar_bg, sidebar);
        assert_eq!(
            changed.theme_card(&theme, false).swatches,
            original.theme_card(&theme, false).swatches
        );
        assert_eq!(changed.editor.editor_bg, original.editor.editor_bg);
        assert_eq!(
            changed.selection.selection_color,
            original.selection.selection_color
        );
        assert_eq!(
            changed.checkbox.checkbox_checked_bg,
            original.checkbox.checkbox_checked_bg
        );
        assert_eq!(
            changed.code_block.code_block_bg,
            original.code_block.code_block_bg
        );
        assert_eq!(
            changed.table.table_header_bg,
            original.table.table_header_bg
        );
        let light = ThemeKind::light_themes()[0].build();
        let rethemed = changed.retheme(&light, &light, true);
        assert_eq!(rethemed.switch.colors.checked_background, primary);
        assert_eq!(rethemed.context_menu.menu_bg, popover);
        assert_eq!(rethemed.theme_card(&light, false).background, card);
    }
}

#[cfg(test)]
mod native_detail_tests {
    use crate::{style::*, theme::ThemeKind};
    use gpui::Styled;

    #[test]
    fn native_typography_fallbacks_and_spacing_reach_application_elements() {
        let theme = ThemeKind::OneDark.build();
        let mut spec = UiStyleSpec::default();
        spec.spacing_px = 6.0;
        spec.font_fallbacks = Some(&["PingFang SC", "Noto Sans CJK SC"]);
        let styles = Styles::from_theme_with_spec(&theme, &theme, true, spec);
        assert_eq!(styles.ui.space(SpaceStep::Three), 18.0);
        let mut root = super::chrome::window_frame(styles.window_frame(), 0.0, 1.0);
        assert_eq!(
            root.text_style()
                .as_ref()
                .unwrap()
                .font_fallbacks
                .as_ref()
                .unwrap()
                .fallback_list(),
            &["PingFang SC", "Noto Sans CJK SC"]
        );
        let mut label = super::typography::label(
            TextStyle {
                font_fallbacks: Some(&["Noto Sans CJK SC"]),
                italic: true,
                decoration: TextDecoration::Underline,
                ..TextStyle::new(13.0, theme.text_primary)
            },
            1.0,
        );
        assert_eq!(
            label
                .text_style()
                .as_ref()
                .unwrap()
                .font_fallbacks
                .as_ref()
                .unwrap()
                .fallback_list(),
            &["Noto Sans CJK SC"]
        );
    }

    #[test]
    fn picker_metrics_preserve_their_native_geometry_contract() {
        let theme = ThemeKind::OneDark.build();
        let mut spec = UiStyleSpec::default();
        spec.application.color_slider_height_px = 28.0;
        spec.application.color_sv_handle_size_px = 20.0;
        let styles = Styles::from_theme_with_spec(&theme, &theme, true, spec);
        assert_eq!(styles.theme_debugger.slider_height_px, 28.0);
        assert_eq!(styles.debugger_picker_surface(true, false).radius_px, 14.0);
        assert_eq!(
            styles
                .debugger_handle_surface(false, theme.text_primary)
                .radius_px,
            10.0
        );
    }
}

#[cfg(test)]
mod group_tests {
    use crate::{style::*, theme::ThemeKind};
    use gpui::{Styled, px};
    #[test]
    fn toggle_group_alignment_and_vertical_corners_reach_native_style() {
        for (alignment, expected) in [
            (GroupAlignment::Start, gpui::AlignItems::FlexStart),
            (GroupAlignment::Center, gpui::AlignItems::Center),
            (GroupAlignment::End, gpui::AlignItems::FlexEnd),
            (GroupAlignment::Stretch, gpui::AlignItems::Stretch),
        ] {
            let mut group = super::button::toggle_group(
                ToggleGroupStyle {
                    tray: None,
                    alignment,
                    orientation: Orientation::Vertical,
                    gap_px: 3.0,
                },
                2.0,
            );
            assert_eq!(group.style().align_items, Some(expected));
        }
        let theme = ThemeKind::OneDark.build();
        let style =
            UiStyleSpec::default().toggle(&theme, ToggleVariant::Outline, ToggleSize::Default);
        let mut segment = super::button::toggle_segment(
            "vertical",
            style,
            Default::default(),
            GroupPosition {
                first: true,
                last: false,
                orientation: Orientation::Vertical,
            },
            1.0,
        );
        assert_eq!(
            segment.style().corner_radii.top_left,
            Some(px(style.radius_px).into())
        );
        assert_eq!(
            segment.style().corner_radii.bottom_left,
            Some(px(0.0).into())
        );
    }
}

#[cfg(test)]
mod composition_contract_tests {
    use crate::{style::*, theme::ThemeKind};
    use gpui::{IntoElement, Styled, px};

    #[test]
    fn vertical_fields_and_container_variants_reach_native_layout() {
        let theme = ThemeKind::OneDark.build();
        for variant in [
            ContainerVariant::Default,
            ContainerVariant::Outline,
            ContainerVariant::Muted,
        ] {
            let mut spec = UiStyleSpec::default();
            spec.composition.field_orientation = Orientation::Vertical;
            spec.composition.field_variant = variant;
            spec.border_width_px = 3.0;
            let styles = Styles::from_theme_with_spec(&theme, &theme, true, spec);
            let style = styles.settings_field();
            let mut field =
                super::field::field(style.clone(), 1.5, "label", gpui::div().into_any_element());
            assert_eq!(
                field.style().flex_direction,
                Some(gpui::FlexDirection::Column)
            );
            assert_eq!(
                field.style().border_widths.top,
                Some(
                    px(if variant == ContainerVariant::Outline {
                        3.0
                    } else {
                        0.0
                    })
                    .into()
                )
            );
            assert_eq!(
                field.style().background,
                Some(style.surface.background.into())
            );
        }
    }

    #[test]
    fn item_sizes_variants_orientation_and_empty_chrome_are_consumed() {
        let theme = ThemeKind::OneDark.build();
        for size in [ItemSize::Default, ItemSize::Sm, ItemSize::Xs] {
            for variant in [
                ContainerVariant::Default,
                ContainerVariant::Outline,
                ContainerVariant::Muted,
            ] {
                let mut spec = UiStyleSpec::default();
                spec.composition.item.size = size;
                spec.composition.item.variant = variant;
                spec.composition.item.orientation = Orientation::Vertical;
                spec.composition.empty_variant = variant;
                let styles = Styles::from_theme_with_spec(&theme, &theme, true, spec);
                let style = styles.context_menu.item();
                assert_eq!(
                    style.control.height_px,
                    styles.ui.spec.menu_row_height_px * style.presentation.height_factor()
                );
                let mut row = super::list::list_item(style, Default::default(), 2.0);
                assert_eq!(
                    row.style().flex_direction,
                    Some(gpui::FlexDirection::Column)
                );
                assert_eq!(
                    row.style().min_size.height,
                    Some(px(style.control.height_px * 2.0).into())
                );
                if variant == ContainerVariant::Outline {
                    assert_eq!(
                        row.style().border_widths.top,
                        Some(px(style.control.border_width_px * 2.0).into())
                    );
                }
                let empty_style = styles.search_menu_empty();
                let mut empty = super::list::empty(empty_style, 2.0);
                if variant == ContainerVariant::Outline {
                    assert_eq!(
                        empty.style().border_widths.top,
                        Some(px(empty_style.border_width_px * 2.0).into())
                    );
                }
                if variant == ContainerVariant::Muted {
                    assert_eq!(
                        empty.style().background,
                        Some(empty_style.background.into())
                    );
                }
            }
        }
    }

    #[test]
    fn alert_severity_and_dialog_size_outline_drive_native_surfaces() {
        let theme = ThemeKind::OneDark.build();
        for variant in [
            AlertVariant::Default,
            AlertVariant::Destructive,
            AlertVariant::Success,
            AlertVariant::Warning,
            AlertVariant::Info,
        ] {
            let mut spec = UiStyleSpec::default();
            spec.composition.alert_variant = variant;
            spec.composition.dialog_variant = DialogVariant::Outline;
            spec.composition.dialog_size = DialogSize::Lg;
            spec.border_width_px = 4.0;
            let styles = Styles::from_theme_with_spec(&theme, &theme, true, spec);
            let alert = styles.notification_surface();
            let mut notification = super::surface::surface(&alert, 1.0);
            assert_eq!(
                notification.style().text.as_ref().unwrap().color,
                Some(alert.foreground)
            );
            assert_eq!(styles.dialog_width(), styles.dialog.dialog_width_px * 1.5);
            let mut dialog = super::surface::surface(&styles.dialog_surface(), 1.0);
            assert_eq!(dialog.style().border_widths.top, Some(px(4.0).into()));
        }
    }

    #[test]
    fn decoration_metrics_have_shared_native_leaves() {
        let theme = ThemeKind::OneDark.build();
        let mut spec = UiStyleSpec::default();
        spec.application.debugger_swatch_size_px = 22.0;
        spec.application.debugger_swatch_radius_px = 7.0;
        spec.application.format_more_dot_size_px = 4.0;
        spec.application.format_more_dot_count = 5;
        spec.application.info_share_hit_extra_px = 16.0;
        let styles = Styles::from_theme_with_spec(&theme, &theme, true, spec);
        let mut swatch = super::decoration::swatch(styles.debugger_swatch(theme.text_primary), 2.0);
        assert_eq!(swatch.style().size.width, Some(px(44.0).into()));
        assert_eq!(swatch.style().corner_radii.top_left, Some(px(14.0).into()));
        let dots = styles.format_ellipsis(theme.text_primary);
        assert_eq!(dots.count, 5);
        assert_eq!(dots.dot_size_px, 4.0);
        let _rendered_dots = super::decoration::ellipsis(dots, 1.5);
        let action = styles.info_share_action();
        assert_eq!(action.height_px, action.icon_size_px + 16.0);
    }
}
