#![allow(dead_code)]

use std::time::Duration;

use gpui::{FontWeight, Hsla};

use crate::theme::{Theme, ThemeKind, with_alpha};

pub(crate) const TOP_BAR_HEIGHT_PX: f32 = 33.0;

#[derive(Clone, Copy)]
pub(crate) struct WhitespaceMarkerStyle {
    pub dot_size_px: f32,
    pub tab_height_px: f32,
    pub tab_stroke_px: f32,
    pub color: Hsla,
}

impl Styles {
    pub(crate) fn whitespace_markers(&self, scale: f32) -> WhitespaceMarkerStyle {
        WhitespaceMarkerStyle {
            dot_size_px: self.editor.editor_whitespace_dot_size_px * scale,
            tab_height_px: self.editor.editor_whitespace_tab_height_px * scale,
            tab_stroke_px: self.editor.editor_whitespace_tab_stroke_px * scale,
            color: self.editor.editor_whitespace_dot_color,
        }
    }
}

/// Fluent caption buttons are 46dp wide regardless of the surrounding UI.
pub(crate) const WINDOW_CAPTION_BUTTON_WIDTH_PX: f32 = 46.0;
pub(crate) const LINUX_CAPTION_BUTTONS_WIDTH_PX: f32 = 108.0;

/// Leading margin for the tab bar's left controls once fullscreen removes the
/// macOS traffic lights.
pub(crate) const FULLSCREEN_LEADING_MARGIN_PX: f32 = 8.0;

/// Per-component style configuration, derived from the active theme.
pub struct Styles {
    pub minimap: MinimapStyles,
    pub line_diff: LineDiffStyles,
    pub self_drawn: SelfDrawnStyles,
    pub ui: UiStyles,
    pub theme_debugger: ThemeDebuggerStyles,
    pub ui_font_family: &'static str,
    pub ui_font_size_px: f32,
    pub anchored_popup_gap_px: f32,
    pub anchored_popup_margin_px: f32,
    pub status_bar_padding_left_px: f32,
    pub window: WindowStyles,
    pub editor: EditorStyles,
    pub sidebar: SidebarStyles,
    pub outline: OutlineStyles,
    pub utility_panel: UtilityPanelStyles,
    pub tab_bar: TabBarStyles,
    pub toolbar: ToolbarStyles,
    pub format_toolbar: FormatToolbarStyles,
    pub tooltip: TooltipStyles,
    pub cursor: CursorStyles,
    pub code_block: CodeBlockStyles,
    pub quote_block: QuoteBlockStyles,
    pub wiki_link: WikiLinkStyles,
    pub checkbox: CheckboxStyles,
    pub table: TableStyles,
    pub gutter: GutterStyles,
    pub context_menu: ContextMenuStyles,
    pub selection: SelectionStyles,
    pub hr: HrStyles,
    pub ime: ImeStyles,
    pub image_preview: ImagePreviewStyles,
    pub dialog: DialogStyles,
    pub notification: NotificationStyles,
    pub text_input: TextInputStyles,
    pub search: SearchStyles,
    pub workspace_search: WorkspaceSearchStyles,
    pub quick_open: QuickOpenStyles,
    pub settings_panel: SettingsPanelStyles,
    pub switch: SwitchStyles,
}

#[derive(Clone, Copy)]
pub struct LineDiffStyles {
    pub old_row_opacity: f32,
    pub reveal_duration: Duration,
    pub feedback_duration: Duration,
    pub gutter_width_px: f32,
    pub stripe_thickness_px: f32,
    pub stripe_period_px: f32,
    pub stripe_opacity: f32,
    pub stripe_inactive_opacity: f32,
    pub stripe_hover_opacity: f32,
    pub added_stripe_color: Hsla,
    pub modified_stripe_color: Hsla,
    pub deleted_stripe_color: Hsla,
    pub added_stripe_hover_color: Hsla,
    pub modified_stripe_hover_color: Hsla,
    pub deleted_stripe_hover_color: Hsla,
    pub added_background_color: Hsla,
    pub modified_background_color: Hsla,
    pub deleted_background_color: Hsla,
    pub added_text_background_color: Hsla,
    pub modified_text_background_color: Hsla,
    pub deleted_text_background_color: Hsla,
    pub restore_inset_px: f32,
    pub restore_text_color: Hsla,
    pub restore_hover_text_color: Hsla,
}

impl LineDiffStyles {
    fn from_theme(theme: &Theme, is_light: bool) -> Self {
        let brighten = |color: Hsla| Hsla {
            l: (color.l + if is_light { 0.12 } else { 0.18 }).min(1.0),
            ..color
        };
        Self {
            old_row_opacity: 0.5,
            reveal_duration: Duration::from_millis(180),
            feedback_duration: Duration::from_millis(150),
            gutter_width_px: 6.4,
            stripe_thickness_px: 3.0,
            stripe_period_px: 6.0,
            stripe_opacity: 0.5,
            stripe_inactive_opacity: 0.16,
            stripe_hover_opacity: 0.9,
            added_stripe_color: theme.text_accent,
            modified_stripe_color: theme.status_success,
            deleted_stripe_color: theme.text_danger,
            added_stripe_hover_color: brighten(theme.text_accent),
            modified_stripe_hover_color: brighten(theme.status_success),
            deleted_stripe_hover_color: brighten(theme.text_danger),
            added_background_color: with_alpha(theme.text_accent, 0.02),
            modified_background_color: with_alpha(theme.status_success, 0.02),
            deleted_background_color: with_alpha(theme.text_danger, 0.02),
            added_text_background_color: with_alpha(theme.text_accent, 0.48),
            modified_text_background_color: with_alpha(theme.status_success, 0.48),
            deleted_text_background_color: with_alpha(theme.text_danger, 0.48),
            restore_inset_px: 12.0,
            restore_text_color: with_alpha(theme.text_primary, 0.65),
            restore_hover_text_color: theme.text_primary,
        }
    }
}

#[derive(Clone, Copy)]
pub struct MinimapStyles {
    pub width_px: f32,
    pub opacity: f32,
    pub min_pane_width_px: f32,
    pub padding_px: f32,
    pub min_handle_hit_height_px: f32,
    pub border_width_px: f32,
    pub colors: crate::theme::MinimapPalette,
}

impl MinimapStyles {
    pub fn lane_width(&self, scale: f32) -> f32 {
        self.width_px * scale
    }

    pub fn handle_color(&self, hovered: bool, pressed: bool) -> Hsla {
        if pressed {
            self.colors.viewport_pressed
        } else if hovered {
            self.colors.viewport_hover
        } else {
            self.colors.viewport
        }
    }
}

#[derive(Clone, Copy)]
pub struct SwitchStyles {
    pub colors: crate::theme::SwitchPalette,
    pub ring: RingStyle,
    pub disabled_opacity: f32,
    pub loading_opacity: f32,
    pub row_height_px: f32,
    pub track_width_px: f32,
    pub track_height_px: f32,
    pub track_padding_px: f32,
    pub track_radius_px: f32,
    pub border_width_px: f32,
    pub thumb_size_px: f32,
    pub thumb_radius_px: f32,
    pub thumb_travel_px: f32,
    pub label_gap_px: f32,
    pub label_font_size_px: f32,
    pub animation_duration: Duration,
    pub easing: fn(f32) -> f32,
    pub cursor: gpui::CursorStyle,
}

impl SwitchStyles {
    pub fn thumb_offset(&self, checked: bool) -> f32 {
        if checked { self.thumb_travel_px } else { 0.0 }
    }
}

// ── Shared application contract (unscaled logical pixels) ─────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ControlState {
    pub hovered: bool,
    pub pressed: bool,
    pub focus_visible: bool,
    pub disabled: bool,
    pub selected: bool,
    pub checked: bool,
    pub open: bool,
    pub invalid: bool,
    pub read_only: bool,
    pub indeterminate: bool,
    pub loading: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonVariant {
    Default,
    Outline,
    Secondary,
    Ghost,
    Destructive,
    Link,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonSize {
    Default,
    Xs,
    Sm,
    Lg,
    Icon,
    IconXs,
    IconSm,
    IconLg,
}

#[derive(Clone, Copy)]
pub struct ControlColors {
    pub background: Hsla,
    pub foreground: Hsla,
    pub border: Hsla,
    pub opacity: f32,
}
#[derive(Clone, Copy)]
pub struct RingStyle {
    pub color: Hsla,
    pub invalid_color: Hsla,
    pub width_px: f32,
    pub offset_px: f32,
    pub offset_color: Hsla,
}
impl RingStyle {
    pub fn shadows(self, scale: f32, invalid: bool) -> Vec<gpui::BoxShadow> {
        if self.width_px <= 0.0 {
            return Vec::new();
        }
        let mut layers = vec![gpui::BoxShadow {
            color: if invalid {
                self.invalid_color
            } else {
                self.color
            },
            offset: gpui::point(gpui::px(0.0), gpui::px(0.0)),
            blur_radius: gpui::px(0.0),
            spread_radius: gpui::px((self.width_px + self.offset_px) * scale),
        }];
        if self.offset_px > 0.0 {
            layers.push(gpui::BoxShadow {
                color: self.offset_color,
                offset: gpui::point(gpui::px(0.0), gpui::px(0.0)),
                blur_radius: gpui::px(0.0),
                spread_radius: gpui::px(self.offset_px * scale),
            });
        }
        layers
    }
}

#[derive(Clone, Copy)]
pub struct ControlStyle {
    pub height_px: f32,
    pub min_width_px: f32,
    pub padding_x_px: f32,
    pub gap_px: f32,
    pub icon_size_px: f32,
    pub radius_px: f32,
    /// Round surfaced controls to half their height in the full-corner mode.
    pub full_round: bool,
    pub scale_radius: bool,
    pub border_width_px: f32,
    pub scale_border: bool,
    pub font_size_px: f32,
    pub font_weight: FontWeight,
    pub underline_on_hover: bool,
    pub cursor: gpui::CursorStyle,
    pub normal: ControlColors,
    pub hover: ControlColors,
    pub pressed: ControlColors,
    pub selected: ControlColors,
    pub selected_hover: ControlColors,
    pub disabled_opacity: f32,
    pub loading_opacity: f32,
    /// Raised shadow for controls with a visible resting surface.
    pub elevation: Option<ElevationStyle>,
    pub ring: RingStyle,
}
impl ControlStyle {
    /// Disabled/read-only suppress transient interaction; invalid is orthogonal
    /// to selection and focus. Selected-hover is an explicit palette, not order
    /// dependent builder overrides.
    pub fn resolve(self, state: ControlState) -> ControlColors {
        let selected = state.selected || state.checked || state.open || state.indeterminate;
        let mut colors = if selected { self.selected } else { self.normal };
        if !state.disabled && !state.loading && !state.read_only {
            if state.pressed {
                colors = if selected {
                    self.selected_hover
                } else {
                    self.pressed
                };
            } else if state.hovered {
                colors = if selected {
                    self.selected_hover
                } else {
                    self.hover
                };
            }
        }
        if state.invalid {
            colors.border = self.ring.invalid_color;
        }
        if state.disabled {
            colors.opacity *= self.disabled_opacity;
        } else if state.loading {
            colors.opacity *= self.loading_opacity;
        }
        colors
    }
}
#[derive(Clone, Copy)]
pub struct GroupPosition {
    pub orientation: Orientation,
    pub first: bool,
    pub last: bool,
}

/// Compact outer shadow for raised controls.
#[derive(Clone, Copy)]
pub struct ElevationStyle {
    pub layers: [ShadowLayer; 2],
}
impl ElevationStyle {
    pub fn raised(dark: bool) -> Self {
        let alpha = if dark { 0.16 } else { 0.055 };
        Self {
            layers: [
                ShadowLayer {
                    color: crate::theme::ui_shadow_color(alpha),
                    x_px: 0.0,
                    y_px: 1.0,
                    blur_px: 2.0,
                    spread_px: 0.0,
                },
                ShadowLayer {
                    color: crate::theme::ui_shadow_color(alpha * 0.35),
                    x_px: 0.0,
                    y_px: 0.0,
                    blur_px: 1.0,
                    spread_px: 0.0,
                },
            ],
        }
    }
    pub fn shadows(&self, scale: f32) -> Vec<gpui::BoxShadow> {
        ShadowStyle {
            layers: self.layers.to_vec(),
        }
        .layers(scale)
    }
}

#[derive(Clone, Copy)]
pub struct ShadowLayer {
    pub color: Hsla,
    pub x_px: f32,
    pub y_px: f32,
    pub blur_px: f32,
    pub spread_px: f32,
}
#[derive(Clone, Default)]
pub struct ShadowStyle {
    pub layers: Vec<ShadowLayer>,
}
impl ShadowStyle {
    pub fn layers(&self, scale: f32) -> Vec<gpui::BoxShadow> {
        self.layers
            .iter()
            .map(|s| gpui::BoxShadow {
                color: s.color,
                offset: gpui::point(gpui::px(s.x_px * scale), gpui::px(s.y_px * scale)),
                blur_radius: gpui::px(s.blur_px * scale),
                spread_radius: gpui::px(s.spread_px * scale),
            })
            .collect()
    }
}
#[derive(Clone)]
pub struct SurfaceStyle {
    pub background: Hsla,
    pub foreground: Hsla,
    pub border_color: Hsla,
    pub border_width_px: f32,
    pub radius_px: f32,
    pub opacity: f32,
    pub shadow: ShadowStyle,
    pub scale_border: bool,
    pub scale_shadow: bool,
}
#[derive(Clone, Copy)]
pub struct SeparatorStyle {
    pub scale_thickness: bool,
    pub color: Hsla,
    pub thickness_px: f32,
    pub margin_px: f32,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TextOverflow {
    Inherit,
    Ellipsis,
    Wrap,
}
#[derive(Clone, Copy)]
pub struct TextStyle {
    pub font_fallbacks: Option<&'static [&'static str]>,
    pub italic: bool,
    pub decoration: TextDecoration,
    pub size_px: f32,
    pub weight: FontWeight,
    pub line_height_px: Option<f32>,
    pub font_family: Option<&'static str>,
    pub color: Hsla,
    pub overflow: TextOverflow,
}
impl TextStyle {
    pub fn new(size_px: f32, color: Hsla) -> Self {
        Self {
            size_px,
            color,
            font_fallbacks: None,
            italic: false,
            decoration: TextDecoration::None,
            weight: FontWeight::NORMAL,
            line_height_px: None,
            font_family: None,
            overflow: TextOverflow::Inherit,
        }
    }
}
#[derive(Clone, Copy)]
pub struct BadgeStyle {
    pub text: TextStyle,
    pub padding_x_px: f32,
    pub padding_y_px: f32,
    pub radius_px: f32,
    pub border_width_px: f32,
    pub border_color: Hsla,
    pub background: Hsla,
}
#[derive(Clone, Copy)]
pub struct ListItemStyle {
    pub inset_x_px: f32,
    pub presentation: ItemPresentation,
    pub control: ControlStyle,
}
#[derive(Clone, Copy)]
pub struct EmptyStyle {
    pub variant: ContainerVariant,
    pub background: Hsla,
    pub border: Hsla,
    pub border_width_px: f32,
    pub radius_px: f32,
    pub height_px: f32,
    pub text: TextStyle,
}

/// Corner rounding applied to application chrome. Document content keeps its
/// own radii so Markdown rendering does not change with the interface option.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiCorners {
    Square,
    #[default]
    #[serde(alias = "standard", alias = "small")]
    Half,
    #[serde(alias = "large")]
    Full,
}

impl UiCorners {
    pub const ALL: [Self; 3] = [Self::Square, Self::Half, Self::Full];

    pub fn factor(self) -> f32 {
        match self {
            Self::Square => 0.0,
            Self::Half => 1.0,
            Self::Full => 2.0,
        }
    }

    fn menu_factor(self) -> f32 {
        match self {
            Self::Full => 1.5,
            _ => self.factor(),
        }
    }
}

/// User adjustable layers on top of the native Moeka style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UiAppearance {
    pub corners: UiCorners,
    /// Raised controls: surfaced buttons, selected segments and cards cast a
    /// compact outer shadow that stays in place when pressed.
    pub elevation: bool,
}

/// Native Moeka style tokens. `appearance` is resolved into the radii and
/// control elevation when styles are built; the stored spec stays unscaled.
#[derive(Clone)]
pub struct UiStyleSpec {
    pub appearance: UiAppearance,
    pub composition: CompositionSpec,
    pub font_fallbacks: Option<&'static [&'static str]>,
    pub colors: crate::theme::UiColorOverrides,
    pub application: ApplicationMetrics,
    pub self_drawn: SelfDrawnSpec,
    pub radius_px: f32,
    pub spacing_px: f32,
    pub control_heights_px: [f32; 4],
    pub control_padding_x_px: [f32; 4],
    pub control_icon_sizes_px: [f32; 4],
    pub control_font_size_px: f32,
    pub border_width_px: f32,
    pub disabled_opacity: f32,
    pub loading_opacity: f32,
    pub focus_ring_width_px: f32,
    pub focus_ring_offset_px: f32,
    pub menu_row_height_px: f32,
    pub menu_emphasis: MenuEmphasis,
    pub menu_surface: MenuSurface,
    pub menu_input_padding_y_px: f32,
    pub menu_list_padding_bottom_px: f32,
    pub menu_radius_px: f32,
    pub menu_icon_size_px: f32,
    pub menu_detail_max_width_px: f32,
    pub tooltip_line_height_factor: f32,
    pub settings_max_width_px: f32,
    pub settings_padding_x_px: f32,
    pub settings_padding_y_px: f32,
    pub settings_section_gap_px: f32,
    pub menu_motion_duration: Duration,
    pub overlay_motion_duration: Duration,
    pub hover_motion_duration: Duration,
    pub tree_motion_duration: Duration,
    pub theme_motion_duration: Duration,
    pub notification_exit_duration: Duration,
    pub animations_enabled: bool,
    pub switch_motion_duration: Duration,
    pub easing: fn(f32) -> f32,
}
impl Default for UiStyleSpec {
    fn default() -> Self {
        Self {
            appearance: UiAppearance::default(),
            self_drawn: SelfDrawnSpec::default(),
            composition: CompositionSpec::default(),
            font_fallbacks: None,
            colors: crate::theme::UiColorOverrides::default(),
            application: ApplicationMetrics::default(),
            radius_px: 6.0,
            spacing_px: 4.0,
            control_heights_px: [28.0, 20.0, 24.0, 32.0],
            control_padding_x_px: [12.0, 6.0, 8.0, 16.0],
            control_icon_sizes_px: [14.0, 12.0, 13.0, 16.0],
            control_font_size_px: 12.0,
            border_width_px: 1.0,
            disabled_opacity: 0.4,
            loading_opacity: 0.6,
            focus_ring_width_px: 0.0,
            focus_ring_offset_px: 0.0,
            menu_row_height_px: 24.0,
            menu_emphasis: MenuEmphasis::Subtle,
            menu_surface: MenuSurface::Default,
            menu_input_padding_y_px: 4.0,
            menu_list_padding_bottom_px: 4.0,
            menu_radius_px: 8.0,
            menu_icon_size_px: 14.0,
            menu_detail_max_width_px: 140.0,
            tooltip_line_height_factor: 1.3,
            settings_max_width_px: 640.0,
            settings_padding_x_px: 40.0,
            settings_padding_y_px: 36.0,
            settings_section_gap_px: 32.0,
            menu_motion_duration: Duration::from_millis(120),
            overlay_motion_duration: Duration::from_millis(140),
            hover_motion_duration: Duration::from_millis(140),
            tree_motion_duration: Duration::from_millis(220),
            theme_motion_duration: Duration::from_millis(180),
            notification_exit_duration: Duration::from_millis(90),
            animations_enabled: true,
            switch_motion_duration: Duration::from_millis(160),
            easing: crate::ui::motion::ease_out,
        }
    }
}
impl UiStyleSpec {
    pub fn tree_transition_duration(&self, expanding: bool) -> Duration {
        self.motion_duration(if expanding {
            self.tree_motion_duration
        } else {
            self.tree_motion_duration.mul_f32(0.75)
        })
    }

    pub fn motion_duration(&self, duration: Duration) -> Duration {
        if self.animations_enabled {
            duration
        } else {
            Duration::ZERO
        }
    }

    pub fn search_menu_input_height_px(&self) -> f32 {
        let height =
            self.menu_row_height_px + self.menu_input_padding_y_px * 2.0 + self.border_width_px;
        if self.appearance.corners == UiCorners::Full {
            height.max(self.menu_radius_px)
        } else {
            height
        }
    }

    pub fn search_menu_bottom_padding_px(&self) -> f32 {
        if self.appearance.corners == UiCorners::Full {
            self.menu_list_padding_bottom_px.max(self.menu_radius_px)
        } else {
            self.menu_list_padding_bottom_px
        }
    }

    pub fn corner(&self, radius_px: f32) -> f32 {
        radius_px * self.appearance.corners.factor()
    }

    /// Applies the corner option to the spec level radii. Called once per build
    /// on a copy, so rebuilding from the stored spec never scales twice.
    fn resolved(&self) -> Self {
        let factor = self.appearance.corners.factor();
        let mut spec = self.clone();
        spec.radius_px *= factor;
        spec.menu_radius_px *= self.appearance.corners.menu_factor();
        let a = &mut spec.application;
        for radius in [
            &mut a.binary_action_radius_px,
            &mut a.info_share_radius_px,
            &mut a.debugger_swatch_radius_px,
            &mut a.debugger_segment_radius_px,
            &mut a.color_picker_radius_px,
            &mut a.search_fold_radius_px,
            &mut a.drag_radius_px,
            &mut a.theme_card_radius_px,
            &mut a.workspace_toggle_radius_px,
        ] {
            *radius *= factor;
        }
        spec
    }

    pub fn radii(&self) -> [f32; 7] {
        [0.6, 0.8, 1.0, 1.4, 1.8, 2.2, 2.6].map(|ratio| self.radius_px * ratio)
    }
    pub fn button(&self, theme: &Theme, variant: ButtonVariant, size: ButtonSize) -> ControlStyle {
        use crate::theme::UiColor as C;
        let index = match size {
            ButtonSize::Default | ButtonSize::Icon => 0,
            ButtonSize::Xs | ButtonSize::IconXs => 1,
            ButtonSize::Sm | ButtonSize::IconSm => 2,
            ButtonSize::Lg | ButtonSize::IconLg => 3,
        };
        let icon_only = matches!(
            size,
            ButtonSize::Icon | ButtonSize::IconXs | ButtonSize::IconSm | ButtonSize::IconLg
        );
        let transparent = gpui::transparent_black();
        let colors = |bg, fg, border| ControlColors {
            background: bg,
            foreground: fg,
            border,
            opacity: 1.0,
        };
        let normal = match variant {
            ButtonVariant::Default => colors(
                self.colors.resolve(theme, C::Primary),
                self.colors.resolve(theme, C::PrimaryForeground),
                transparent,
            ),
            ButtonVariant::Destructive => colors(
                self.colors.resolve(theme, C::Destructive),
                self.colors.resolve(theme, C::DestructiveForeground),
                transparent,
            ),
            ButtonVariant::Secondary => colors(
                self.colors.resolve(theme, C::Secondary),
                self.colors.resolve(theme, C::SecondaryForeground),
                transparent,
            ),
            ButtonVariant::Outline => colors(
                self.colors.resolve(theme, C::Background),
                self.colors.resolve(theme, C::Foreground),
                self.colors.resolve(theme, C::Border),
            ),
            ButtonVariant::Ghost => colors(
                transparent,
                self.colors.resolve(theme, C::Foreground),
                transparent,
            ),
            ButtonVariant::Link => colors(
                transparent,
                self.colors.resolve(theme, C::Info),
                transparent,
            ),
        };
        let hover = if matches!(variant, ButtonVariant::Default | ButtonVariant::Destructive) {
            ControlColors {
                background: Hsla {
                    a: normal.background.a * 0.9,
                    ..normal.background
                },
                ..normal
            }
        } else if variant == ButtonVariant::Link {
            ControlColors {
                foreground: theme.text_bright,
                ..normal
            }
        } else {
            ControlColors {
                background: theme.bg_hover,
                ..normal
            }
        };
        let selected = colors(
            self.colors.resolve(theme, C::Accent),
            self.colors.resolve(theme, C::AccentForeground),
            normal.border,
        );
        ControlStyle {
            height_px: self.control_heights_px[index],
            min_width_px: if icon_only {
                self.control_heights_px[index]
            } else {
                0.0
            },
            padding_x_px: if icon_only {
                0.0
            } else {
                self.control_padding_x_px[index]
            },
            gap_px: self.spacing_px,
            icon_size_px: self.control_icon_sizes_px[index],
            radius_px: self.radii()[2],
            full_round: self.appearance.corners == UiCorners::Full,
            scale_radius: true,
            scale_border: true,
            border_width_px: if variant == ButtonVariant::Outline {
                self.border_width_px
            } else {
                0.0
            },
            font_size_px: self.control_font_size_px,
            font_weight: FontWeight::NORMAL,
            underline_on_hover: variant == ButtonVariant::Link,
            cursor: gpui::CursorStyle::Arrow,
            normal,
            hover,
            pressed: hover,
            selected,
            selected_hover: selected,
            disabled_opacity: self.disabled_opacity,
            loading_opacity: self.loading_opacity,
            elevation: self
                .appearance
                .elevation
                .then(|| ElevationStyle::raised(theme.bg_primary.l < 0.5)),
            ring: RingStyle {
                color: self.colors.resolve(theme, C::Ring),
                invalid_color: self.colors.resolve(theme, C::Destructive),
                width_px: self.focus_ring_width_px,
                offset_px: self.focus_ring_offset_px,
                offset_color: self.colors.resolve(theme, C::Background),
            },
        }
    }
}

pub struct UiStyles {
    pub colors: crate::theme::ApplicationUiPalette,
    pub spec: UiStyleSpec,
    pub control: ControlStyle,
    pub toggle: ControlStyle,
    pub badge: BadgeStyle,
    pub small_shadow: ShadowStyle,
    pub popup_shadow: ShadowStyle,
    /// Spec as configured, before the appearance options were resolved.
    pub base_spec: UiStyleSpec,
    pub raised_surface: Hsla,
    pub raised_foreground: Hsla,
    /// Hover fill for a raised surface; distinct but no heavier than the surface.
    pub raised_hover: Hsla,
    /// Light hover fill for unselected segments inside the tray.
    pub segment_hover: Hsla,
    pub segment_tray: Hsla,
}
impl UiStyles {
    fn new(theme: &Theme, base_spec: UiStyleSpec) -> Self {
        let spec = base_spec.resolved();
        let dark = theme.bg_primary.l < 0.5;
        let small_shadow = ShadowStyle {
            layers: vec![
                ShadowLayer {
                    color: crate::theme::ui_shadow_color(0.1),
                    x_px: 0.0,
                    y_px: 1.0,
                    blur_px: 3.0,
                    spread_px: 0.0,
                },
                ShadowLayer {
                    color: crate::theme::ui_shadow_color(0.1),
                    x_px: 0.0,
                    y_px: 1.0,
                    blur_px: 2.0,
                    spread_px: -1.0,
                },
            ],
        };
        let popup_shadow = ShadowStyle {
            layers: vec![
                ShadowLayer {
                    color: crate::theme::ui_shadow_color(0.1),
                    x_px: 0.0,
                    y_px: 4.0,
                    blur_px: 6.0,
                    spread_px: -1.0,
                },
                ShadowLayer {
                    color: crate::theme::ui_shadow_color(0.1),
                    x_px: 0.0,
                    y_px: 2.0,
                    blur_px: 4.0,
                    spread_px: -2.0,
                },
            ],
        };
        Self {
            colors: theme.application_ui_palette(),
            control: spec.button(theme, ButtonVariant::Ghost, ButtonSize::Default),
            toggle: spec.toggle(theme, ToggleVariant::Default, ToggleSize::Default),
            badge: spec.badge(theme, BadgeVariant::Ghost),
            spec,
            base_spec,
            small_shadow,
            popup_shadow,
            raised_surface: if dark {
                theme.bg_hover
            } else {
                theme.bg_primary
            },
            raised_foreground: theme.text_bright,
            raised_hover: if dark {
                theme.bg_hover.blend(with_alpha(theme.text_bright, 0.06))
            } else {
                theme.bg_primary.blend(with_alpha(theme.bg_hover, 0.5))
            },
            segment_hover: with_alpha(
                if dark {
                    theme.bg_hover
                } else {
                    theme.bg_primary
                },
                0.6,
            ),
            segment_tray: theme.bg_secondary,
        }
    }
}

impl Styles {
    pub fn document_selection_color(&self, multiple_cursors: bool) -> Hsla {
        if multiple_cursors {
            self.selection.multi_selection_color
        } else {
            self.selection.selection_color
        }
    }

    pub fn settings_header_button(&self) -> ControlStyle {
        let s = &self.settings_panel;
        let mut c = self.ui.control;
        c.height_px = s.settings_site_button_height_px;
        c.padding_x_px = s.settings_site_button_padding_x_px;
        c.gap_px = s.settings_site_button_gap_px;
        c.icon_size_px = s.settings_site_button_icon_size_px;
        c.radius_px = s.settings_site_button_radius_px;
        c.border_width_px = s.settings_site_button_border_width_px;
        c.scale_border = false;
        c.font_size_px = s.settings_site_button_font_size_px;
        c.normal = ControlColors {
            background: if self.ui.spec.appearance.elevation {
                self.ui.raised_surface
            } else {
                s.settings_site_button_bg
            },
            foreground: s.settings_site_button_text_color,
            border: s.settings_site_button_border_color,
            opacity: 1.0,
        };
        c.hover = ControlColors {
            background: if self.ui.spec.appearance.elevation {
                self.ui.raised_hover
            } else {
                s.settings_site_button_hover_bg
            },
            ..c.normal
        };
        c.pressed = c.hover;
        c
    }

    pub fn settings_toggle(&self) -> ControlStyle {
        let s = &self.settings_panel;
        let mut c = self.ui.toggle;
        c.height_px = s.settings_theme_toggle_height_px;
        c.padding_x_px = s.settings_theme_toggle_padding_x_px;
        c.gap_px = s.settings_theme_toggle_gap_px;
        c.icon_size_px = s.settings_theme_toggle_icon_size_px;
        c.radius_px = s.settings_theme_toggle_radius_px;
        c.border_width_px = s.settings_theme_toggle_border_width_px;
        c.font_size_px = s.settings_theme_option_font_size_px;
        c.normal = ControlColors {
            background: s.settings_option_button_bg,
            foreground: s.settings_option_button_text_color,
            border: s.settings_theme_option_border_color,
            opacity: 1.0,
        };
        c.hover = ControlColors {
            background: s.settings_option_button_hover_bg,
            ..c.normal
        };
        c.selected = ControlColors {
            background: s.settings_option_button_active_bg,
            foreground: s.settings_option_button_active_text_color,
            ..c.normal
        };
        c.selected_hover = c.selected;
        c.pressed = c.hover;
        c
    }

    pub fn menu_item(&self, destructive: bool) -> ListItemStyle {
        let mut c = self.context_menu.item().control;
        c.gap_px = self.context_menu.menu_item_action_gap_px;
        if destructive {
            c.normal.foreground = self.context_menu.menu_item_danger_text_color;
            c.hover.foreground = c.normal.foreground;
            c.selected.foreground = c.normal.foreground;
            c.selected_hover.foreground = c.normal.foreground;
            c.pressed.foreground = c.normal.foreground;
        }
        self.ui.item(c)
    }

    pub fn search_menu_item(&self) -> ListItemStyle {
        let s = &self.quick_open;
        let mut c = self.ui.control;
        c.height_px = self.ui.spec.menu_row_height_px;
        c.padding_x_px = s.quick_open_result_padding_x_px;
        c.gap_px = self.ui.spec.spacing_px * 1.5;
        c.font_size_px = s.quick_open_result_name_font_size_px;
        c.normal.foreground = s.quick_open_result_text_color;
        c.hover = ControlColors {
            background: s.quick_open_result_hover_bg,
            ..c.normal
        };
        c.selected = ControlColors {
            background: s.quick_open_result_selected_bg,
            foreground: s.quick_open_result_accent_text_color,
            ..c.normal
        };
        c.selected_hover = ControlColors {
            background: s.quick_open_result_hover_bg,
            ..c.selected
        };
        c.pressed = c.hover;
        self.ui.item(c)
    }

    pub fn search_menu_surface(&self) -> SurfaceStyle {
        let s = &self.quick_open;
        SurfaceStyle {
            scale_border: false,
            scale_shadow: false,
            background: s.quick_open_panel_bg,
            foreground: s.quick_open_result_text_color,
            border_color: s.quick_open_panel_border_color,
            border_width_px: self.ui.spec.border_width_px,
            radius_px: self.ui.spec.menu_radius_px,
            opacity: 1.0,
            shadow: self.ui.popup_shadow.clone(),
        }
    }
    pub fn search_menu_empty(&self) -> EmptyStyle {
        EmptyStyle {
            variant: self.ui.spec.composition.empty_variant,
            background: self.ui.control.selected.background,
            border: self.ui.control.ring.color,
            border_width_px: self.ui.spec.border_width_px,
            radius_px: self.ui.spec.radius_px,
            height_px: self.quick_open.quick_open_empty_height_px,
            text: TextStyle::new(
                self.quick_open.quick_open_empty_font_size_px,
                self.quick_open.quick_open_empty_text_color,
            ),
        }
    }

    pub fn dialog_button(
        &self,
        background: Hsla,
        foreground: Hsla,
        hover_background: Hsla,
    ) -> ControlStyle {
        let s = &self.dialog;
        let mut c = self.ui.control;
        c.height_px = s.dialog_btn_height_px;
        c.padding_x_px = s.dialog_btn_padding_x_px;
        c.font_size_px = s.dialog_btn_font_size_px;
        c.radius_px = s.dialog_btn_radius_px;
        c.scale_radius = c.full_round;
        c.border_width_px = s.dialog_border_width_px;
        c.scale_border = false;
        c.normal = ControlColors {
            background,
            foreground,
            border: s.dialog_border_color,
            opacity: 1.0,
        };
        c.hover = ControlColors {
            background: hover_background,
            ..c.normal
        };
        c.pressed = c.hover;
        c
    }
    pub fn dialog_surface(&self) -> SurfaceStyle {
        let s = &self.dialog;
        SurfaceStyle {
            scale_border: false,
            scale_shadow: false,
            background: s.dialog_bg,
            foreground: s.dialog_message_text_color,
            border_color: s.dialog_border_color,
            border_width_px: if self.ui.spec.composition.dialog_variant == DialogVariant::Outline {
                self.ui.spec.border_width_px
            } else {
                s.dialog_border_width_px
            },
            radius_px: s.dialog_radius_px,
            opacity: 1.0,
            shadow: ShadowStyle::default(),
        }
    }
    pub fn notification_surface(&self) -> SurfaceStyle {
        let s = &self.notification;
        let foreground = match self.ui.spec.composition.alert_variant {
            AlertVariant::Default => s.notification_text_color,
            AlertVariant::Destructive => self.ui.colors.danger,
            AlertVariant::Success => self.ui.colors.success,
            AlertVariant::Warning => self.ui.colors.warning,
            AlertVariant::Info => self.ui.colors.accent,
        };
        SurfaceStyle {
            scale_border: false,
            scale_shadow: false,
            background: s.notification_bg,
            foreground,
            border_color: s.notification_border_color,
            border_width_px: s.notification_border_width_px,
            radius_px: s.notification_radius_px,
            opacity: 1.0,
            shadow: ShadowStyle::default(),
        }
    }
}

impl Styles {
    pub fn search_toggle(&self) -> ControlStyle {
        let s = &self.search;
        let mut c = self.ui.toggle;
        c.height_px = s.search_option_btn_size_px;
        c.padding_x_px = 0.0;
        c.icon_size_px = s.search_option_btn_icon_size_px;
        c.radius_px = s.search_option_btn_radius_px;
        c.normal.foreground = s.search_option_btn_icon_color;
        c.hover = ControlColors {
            background: s.search_option_btn_hover_bg,
            ..c.normal
        };
        c.selected = ControlColors {
            background: s.search_option_btn_active_bg,
            foreground: s.search_option_btn_active_icon_color,
            ..c.normal
        };
        c.selected_hover = ControlColors {
            background: s.search_option_btn_hover_bg,
            ..c.selected
        };
        c.pressed = c.hover;
        c
    }
    pub fn editor_theme_toggle(&self) -> ControlStyle {
        let s = &self.settings_panel;
        let mut c = self.settings_toggle();
        c.height_px = s.settings_theme_option_height_px;
        c.padding_x_px = s.settings_theme_option_padding_x_px;
        c.radius_px = s.settings_theme_option_radius_px;
        c.border_width_px = s.settings_theme_option_border_width_px;
        c.normal.foreground = s.settings_theme_option_text_color;
        c.hover = ControlColors {
            background: s.settings_theme_option_hover_bg,
            ..c.normal
        };
        c.selected = ControlColors {
            background: s.settings_theme_option_active_bg,
            foreground: s.settings_theme_option_active_text_color,
            ..c.normal
        };
        c.selected_hover = c.selected;
        c.pressed = c.hover;
        c
    }
    pub fn format_menu_item(&self, active: bool) -> ListItemStyle {
        let mut item = self.menu_item(false);
        if active {
            let color = self.context_menu.menu_format_active_text_color;
            item.control.normal.foreground = color;
            item.control.hover.foreground = color;
            item.control.selected.foreground = color;
            item.control.selected_hover.foreground = color;
        }
        item
    }
    pub fn toolbar_button(&self, height_px: f32) -> ControlStyle {
        let mut c = self.ui.control;
        c.height_px = height_px;
        c.padding_x_px = 0.0;
        c.radius_px = 0.0;
        c.border_width_px = 0.0;
        c.elevation = None;
        c.normal.background = gpui::transparent_black();
        c.normal.border = gpui::transparent_black();
        c.hover = c.normal;
        c.pressed = c.normal;
        c
    }
    pub fn settings_picker(&self) -> ControlStyle {
        let s = &self.settings_panel;
        let mut c = self.ui.control;
        c.height_px = s.settings_input_height_px;
        c.padding_x_px = 10.0;
        c.radius_px = s.settings_input_radius_px;
        c.border_width_px = s.settings_option_button_border_width_px;
        c.scale_border = false;
        c.font_size_px = self.ui_font_size_px;
        c.normal = ControlColors {
            background: s.settings_option_button_bg,
            foreground: s.settings_option_button_text_color,
            border: s.settings_option_button_border_color,
            opacity: 1.0,
        };
        c.hover = ControlColors {
            background: s.settings_option_button_hover_bg,
            ..c.normal
        };
        c.pressed = c.hover;
        c
    }
    pub fn tooltip_surface(&self) -> SurfaceStyle {
        let s = &self.tooltip;
        SurfaceStyle {
            scale_border: true,
            scale_shadow: false,
            background: s.tooltip_bg,
            foreground: s.tooltip_title_text_color,
            border_color: s.tooltip_border_color,
            border_width_px: s.tooltip_border_width_px,
            radius_px: s.tooltip_radius_px,
            opacity: 1.0,
            shadow: ShadowStyle::default(),
        }
    }
    pub fn tooltip_keycap(&self) -> BadgeStyle {
        let s = &self.tooltip;
        BadgeStyle {
            text: TextStyle::new(
                s.tooltip_subtitle_font_size_px,
                s.tooltip_subtitle_text_color,
            ),
            padding_x_px: s.tooltip_shortcut_key_padding_x_px,
            padding_y_px: 0.0,
            radius_px: s.tooltip_shortcut_key_radius_px,
            border_width_px: 0.0,
            border_color: gpui::transparent_black(),
            background: s.tooltip_shortcut_key_bg,
        }
    }
}

#[derive(Clone)]
pub struct FieldStyle {
    pub orientation: Orientation,
    pub surface: SurfaceStyle,
    pub label: TextStyle,
    pub label_width_px: f32,
    pub gap_px: f32,
}
#[derive(Clone, Copy)]
pub struct SectionStyle {
    pub title: TextStyle,
    pub separator: SeparatorStyle,
    pub gap_px: f32,
}
impl Styles {
    pub fn settings_field(&self) -> FieldStyle {
        let mut surface = self.dialog_surface();
        surface.shadow = ShadowStyle::default();
        surface.border_width_px = 0.0;
        surface.background = gpui::transparent_black();
        match self.ui.spec.composition.field_variant {
            ContainerVariant::Outline => {
                surface.border_width_px = self.ui.spec.border_width_px;
            }
            ContainerVariant::Muted => {
                surface.background = self.settings_panel.settings_option_button_bg;
            }
            ContainerVariant::Default => {}
        }
        FieldStyle {
            orientation: self.ui.spec.composition.field_orientation,
            surface,
            label: TextStyle::new(
                self.ui_font_size_px,
                self.settings_panel.settings_secondary_text_color,
            ),
            label_width_px: 120.0,
            gap_px: self.ui.spec.spacing_px * 3.0,
        }
    }
    pub fn settings_section_heading(&self) -> SectionStyle {
        SectionStyle {
            title: TextStyle {
                weight: FontWeight::BOLD,
                ..TextStyle::new(
                    self.ui_font_size_px - 1.0,
                    self.settings_panel.settings_primary_text_color,
                )
            },
            separator: SeparatorStyle {
                scale_thickness: false,
                color: self.settings_panel.settings_contrast_border_color,
                thickness_px: self.ui.spec.border_width_px,
                margin_px: 0.0,
            },
            gap_px: self.ui.spec.spacing_px * 2.0,
        }
    }
    pub fn settings_template_help(&self, invalid: bool) -> TextStyle {
        TextStyle {
            overflow: TextOverflow::Wrap,
            ..TextStyle::new(
                self.settings_panel.settings_option_button_font_size_px,
                if invalid {
                    self.ui.colors.danger
                } else {
                    self.settings_panel.settings_secondary_text_color
                },
            )
        }
    }
    pub fn settings_stepper(&self) -> ControlStyle {
        let mut c = self.ui.control;
        c.height_px = self.settings_panel.settings_input_height_px;
        c.padding_x_px = 0.0;
        c.radius_px = self.settings_panel.settings_input_radius_px;
        c.font_size_px = self.ui_font_size_px + 1.0;
        c.border_width_px = self.settings_panel.settings_option_button_border_width_px;
        c.normal.foreground = self.settings_panel.settings_primary_text_color;
        c.normal.border = self.settings_panel.settings_contrast_border_color;
        c.hover = ControlColors {
            background: self.settings_panel.settings_stepper_hover_bg,
            ..c.normal
        };
        c.pressed = c.hover;
        c
    }
    pub fn command_item(&self) -> ListItemStyle {
        let mut c = self.search_menu_item().control;
        c.height_px = self.quick_open.quick_open_result_row_height_px;
        c.gap_px = self.ui.spec.spacing_px * 2.0;
        c.selected.foreground = c.normal.foreground;
        c.selected_hover.foreground = c.normal.foreground;
        self.ui.item(c)
    }
    pub fn command_surface(&self) -> SurfaceStyle {
        SurfaceStyle {
            radius_px: self.quick_open.quick_open_panel_radius_px,
            ..self.search_menu_surface()
        }
    }
}

pub struct ThemeDebuggerStyles {
    pub header_height_px: f32,
    pub row_height_px: f32,
    pub header_padding_x_px: f32,
    pub section_gap_px: f32,
    pub card_padding_px: f32,
    pub input_gap_px: f32,
    pub label_font_size_px: f32,
    pub row_gap_px: f32,
    pub slider_height_px: f32,
    pub slider_handle_width_px: f32,
    pub sv_handle_size_px: f32,
    pub border: Hsla,
    pub divider: Hsla,
    pub panel: Hsla,
    pub card: Hsla,
    pub hover: Hsla,
    pub selected: Hsla,
    pub foreground: Hsla,
    pub muted: Hsla,
}
impl ThemeDebuggerStyles {
    fn from_theme(theme: &Theme) -> Self {
        use crate::theme::UiColor as C;
        Self {
            header_height_px: 30.0,
            row_height_px: 30.0,
            header_padding_x_px: 14.0,
            section_gap_px: 12.0,
            card_padding_px: 12.0,
            input_gap_px: 8.0,
            label_font_size_px: 12.0,
            row_gap_px: 10.0,
            slider_height_px: 16.0,
            slider_handle_width_px: 12.0,
            sv_handle_size_px: 14.0,
            border: theme.border_secondary,
            divider: theme.ui_color(C::Border),
            panel: theme.ui_color(C::Secondary),
            card: theme.ui_color(C::Card),
            hover: theme.bg_hover,
            selected: theme.ui_color(C::Accent),
            foreground: theme.ui_color(C::Foreground),
            muted: theme.ui_color(C::MutedForeground),
        }
    }
}

impl Styles {
    pub fn debugger_item(&self) -> ListItemStyle {
        let s = &self.theme_debugger;
        let mut c = self.ui.control;
        c.height_px = s.row_height_px;
        c.padding_x_px = s.header_padding_x_px;
        c.gap_px = s.row_gap_px;
        c.font_size_px = s.label_font_size_px;
        c.normal.foreground = s.foreground;
        c.hover = ControlColors {
            background: s.hover,
            ..c.normal
        };
        c.selected = ControlColors {
            background: s.selected,
            ..c.normal
        };
        c.selected_hover = c.hover;
        self.ui.item(c)
    }
}

impl ContextMenuStyles {
    pub fn item(&self) -> ListItemStyle {
        let normal = ControlColors {
            background: gpui::transparent_black(),
            foreground: self.menu_item_primary_text_color,
            border: gpui::transparent_black(),
            opacity: 1.0,
        };
        let hover = ControlColors {
            background: self.menu_item_hover_bg,
            ..normal
        };
        let selected = ControlColors {
            background: self.menu_item_selected_bg,
            ..normal
        };
        ListItemStyle {
            inset_x_px: self.menu_item_inset_x_px,
            presentation: self.item_presentation,
            control: ControlStyle {
                height_px: self.menu_item_height_px,
                min_width_px: 0.0,
                padding_x_px: self.menu_item_padding_x_px,
                gap_px: self.menu_format_icon_gap_px,
                icon_size_px: self.menu_format_icon_size_px,
                radius_px: self.menu_item_radius_px,
                full_round: false,
                scale_radius: true,
                border_width_px: self.menu_border_width_px,
                scale_border: false,
                font_size_px: self.menu_item_font_size_px,
                font_weight: FontWeight::NORMAL,
                underline_on_hover: false,
                cursor: gpui::CursorStyle::Arrow,
                normal,
                hover,
                selected,
                selected_hover: hover,
                pressed: hover,
                disabled_opacity: self.menu_item_disabled_opacity,
                loading_opacity: 0.6,
                elevation: None,
                ring: RingStyle {
                    color: self.menu_border_color,
                    invalid_color: self.menu_item_danger_text_color,
                    width_px: 0.0,
                    offset_px: 0.0,
                    offset_color: self.menu_bg,
                },
            },
        }
    }
    pub fn surface(&self) -> SurfaceStyle {
        SurfaceStyle {
            scale_border: false,
            scale_shadow: false,
            background: self.menu_bg,
            foreground: self.menu_item_primary_text_color,
            border_color: self.menu_border_color,
            border_width_px: self.menu_border_width_px,
            radius_px: self.menu_radius_px,
            opacity: 1.0,
            shadow: self.menu_shadow.clone(),
        }
    }
    pub fn separator(&self) -> SeparatorStyle {
        SeparatorStyle {
            scale_thickness: true,
            color: self.menu_separator_color,
            thickness_px: self.menu_separator_height_px,
            margin_px: self.menu_separator_margin_y_px,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputRole {
    Search,
    Replace,
    Settings,
    Stepper,
    Picker,
    QuickOpen,
    WorkspaceFilter,
    WorkspaceSearch,
    SidebarFilter,
    Rename,
    ThemeDebugger,
}

impl Styles {
    /// Input styles are materialized for the existing TextInput engine. Each
    /// role retains its original scale policy; no text/IME geometry is changed.
    pub fn input_styles(&self, role: InputRole, scale: f32) -> TextInputStyles {
        let scaled = |value: f32| value * scale;
        let mut input = match role {
            InputRole::Search => {
                let mut styles = self.text_input.clone();
                styles.text_input_height_px = scaled(self.search.search_input_height_px);
                styles.text_input_padding_left_px = scaled(self.search.search_input_left_inset_px);
                styles.text_input_padding_right_px =
                    scaled(self.search.search_input_right_inset_px);
                styles.text_input_font_size_px = scaled(self.search.search_input_font_size_px);
                styles.text_input_radius_px = scaled(self.text_input.text_input_radius_px);
                styles.text_input_bg = gpui::transparent_black();
                styles.text_input_focus_bg = self.search.search_input_focus_bg;
                styles.text_input_text_color = self.search.search_match_text_color;
                styles.text_input_placeholder_color = self.search.search_counter_text_color;
                styles
            }
            InputRole::Replace => self.input_styles(InputRole::Search, scale),
            InputRole::Settings => {
                let mut styles = self.text_input.clone();
                let settings = &self.settings_panel;
                styles.text_input_bg = settings.settings_option_button_bg;
                styles.text_input_hover_bg = Some(settings.settings_option_button_hover_bg);
                styles.text_input_focus_bg = settings.settings_option_button_hover_bg;
                styles.text_input_border_width_px = 0.0;
                styles.text_input_height_px = scaled(settings.settings_input_height_px);
                styles.text_input_padding_left_px = scaled(8.0);
                styles.text_input_padding_right_px = scaled(8.0);
                styles.text_input_font_size_px = scaled(self.ui_font_size_px + 1.0);
                styles.text_input_radius_px = scaled(settings.settings_input_radius_px);
                styles
            }
            InputRole::Stepper => {
                let mut styles = self.input_styles(InputRole::Settings, scale);
                styles.text_input_padding_left_px = scaled(2.0);
                styles.text_input_padding_right_px = scaled(2.0);
                styles.text_input_radius_px =
                    scaled(self.settings_panel.settings_stepper_input_radius_px);
                styles.text_input_center = true;
                styles
            }
            InputRole::Picker => {
                let mut styles = self.text_input.clone();
                let menu = &self.quick_open;
                styles.text_input_bg = menu.quick_open_input_bg;
                styles.text_input_hover_bg = Some(menu.quick_open_input_focus_bg);
                styles.text_input_focus_bg = menu.quick_open_input_focus_bg;
                styles.text_input_text_color = menu.quick_open_input_text_color;
                styles.text_input_placeholder_color = menu.quick_open_result_secondary_text_color;
                styles.text_input_border_width_px = 0.0;
                styles.text_input_height_px = scaled(self.ui.spec.menu_row_height_px);
                styles.text_input_padding_left_px = menu.quick_open_input_text_padding_left_px;
                styles.text_input_padding_right_px = menu.quick_open_input_text_padding_left_px;
                styles.text_input_font_size_px = scaled(menu.quick_open_result_name_font_size_px);
                styles
            }
            InputRole::QuickOpen => {
                let mut styles = self.text_input.clone();
                styles.text_input_padding_left_px =
                    self.quick_open.quick_open_input_text_padding_left_px;
                styles.text_input_height_px = scaled(self.ui.spec.menu_row_height_px);
                styles.text_input_font_size_px =
                    scaled(self.quick_open.quick_open_result_name_font_size_px);
                styles.text_input_bg = self.quick_open.quick_open_input_bg;
                styles.text_input_focus_bg = self.quick_open.quick_open_input_focus_bg;
                styles.text_input_text_color = self.quick_open.quick_open_input_text_color;
                styles
            }
            InputRole::WorkspaceFilter => {
                let mut styles = self.input_styles(InputRole::WorkspaceSearch, scale);
                styles.text_input_focus_bg = self.workspace_search.ws_search_input_focus_bg;
                styles.text_input_padding_left_px =
                    scaled(self.text_input.text_input_padding_right_px);
                styles
            }
            InputRole::SidebarFilter => {
                let mut styles = self.input_styles(InputRole::WorkspaceSearch, scale);
                styles.text_input_height_px = scaled(self.sidebar.sidebar_header_height_px);
                styles.text_input_font_size_px = scaled(self.sidebar.sidebar_item_font_size_px);
                styles.text_input_padding_left_px = 0.0;
                styles.text_input_padding_right_px = 0.0;
                styles.text_input_radius_px = 0.0;
                styles.text_input_hover_bg = None;
                styles.text_input_focus_ring.width_px = 0.0;
                styles.text_input_placeholder_color = self.sidebar.sidebar_filter_muted_color;
                styles.text_input_focus_placeholder_color =
                    Some(self.sidebar.sidebar_filter_muted_color);
                styles
            }
            InputRole::WorkspaceSearch => {
                let mut styles = self.text_input.clone();
                let workspace_styles = &self.workspace_search;
                styles.text_input_height_px = scaled(
                    workspace_styles.ws_search_input_height_px
                        - 2.0 * workspace_styles.ws_search_input_border_width_px,
                );
                styles.text_input_padding_left_px =
                    scaled(workspace_styles.ws_search_input_left_inset_px);
                styles.text_input_padding_right_px =
                    scaled(self.text_input.text_input_padding_right_px);
                styles.text_input_font_size_px = scaled(self.text_input.text_input_font_size_px);
                styles.text_input_bg = gpui::transparent_black();
                styles.text_input_focus_bg = gpui::transparent_black();
                styles.text_input_text_color = self.sidebar.sidebar_item_text_color;
                styles.text_input_placeholder_color = self.sidebar.sidebar_empty_text_color;
                styles.text_input_radius_px = scaled(workspace_styles.ws_search_input_radius_px);
                styles.text_input_border_width_px = 0.0;
                styles.text_input_selection_color = self.sidebar.sidebar_item_selected_bg;
                styles
            }
            InputRole::Rename => {
                let mut styles = self.text_input.clone();
                let sidebar_styles = &self.sidebar;
                styles.text_input_height_px = scaled(sidebar_styles.sidebar_edit_input_height_px);
                styles.text_input_padding_left_px =
                    scaled(sidebar_styles.sidebar_edit_input_padding_x_px);
                styles.text_input_padding_right_px =
                    scaled(sidebar_styles.sidebar_edit_input_padding_x_px);
                // Match the file-tree item text size so renaming/creating an item does
                // not visually shrink or grow the label (both scale with the UI zoom).
                styles.text_input_font_size_px = scaled(sidebar_styles.sidebar_item_font_size_px);
                styles.text_input_bg = sidebar_styles.sidebar_edit_input_bg;
                styles.text_input_focus_bg = sidebar_styles.sidebar_edit_input_bg;
                styles.text_input_border_width_px =
                    sidebar_styles.sidebar_edit_input_border_width_px;
                styles.text_input_border_color = sidebar_styles.sidebar_edit_input_border_color;
                styles.text_input_focus_border_color =
                    sidebar_styles.sidebar_edit_input_border_color;
                styles.text_input_radius_px = scaled(sidebar_styles.sidebar_edit_input_radius_px);
                styles.text_input_text_color = sidebar_styles.sidebar_edit_input_text_color;
                styles.text_input_placeholder_color =
                    sidebar_styles.sidebar_edit_input_placeholder_color;
                styles
            }
            InputRole::ThemeDebugger => {
                let mut styles = self.text_input.clone();
                styles.text_input_height_px = 28.0;
                styles.text_input_font_size_px = self.ui_font_size_px;
                styles
            }
        };
        input.menu = self.context_menu.clone();
        input.menu_scale = scale;
        input.cursor = self.cursor.clone();
        input.cursor.cursor_width_px = scaled(self.cursor.cursor_width_px);
        input.cursor_fade = self.ui.spec.animations_enabled;
        input
    }
}

const RIGHT_PANEL_MIN_WIDTH_PX: f32 = 220.0;
const RIGHT_PANEL_MAX_WIDTH_PX: f32 = 560.0;

// ── Window ──────────────────────────────────────────────────────────

pub struct WindowStyles {
    pub window_bg: Hsla,
    pub window_border_width_px: f32,
    pub window_border_color: Hsla,
    pub window_radius_px: f32,
}

// ── Editor ──────────────────────────────────────────────────────────

pub struct EditorStyles {
    pub editor_font_size_px: f32,
    pub editor_h1_font_size_px: f32,
    pub editor_h2_font_size_px: f32,
    pub editor_h3_font_size_px: f32,
    pub editor_h4_font_size_px: f32,
    pub editor_h5_font_size_px: f32,
    pub editor_h6_font_size_px: f32,
    pub editor_paragraph_line_height_factor: f32,
    pub editor_heading_line_height_factor: f32,
    pub editor_letter_spacing_px: f32,
    pub editor_body_latin_font_family: &'static str,
    pub editor_body_cjk_font_family: &'static str,
    pub editor_heading_latin_font_family: &'static str,
    pub editor_heading_cjk_font_family: &'static str,
    pub editor_italic_latin_font_family: &'static str,
    pub editor_italic_cjk_font_family: &'static str,
    pub editor_monospace_font_family: &'static str,
    pub editor_paragraph_padding_top_px: f32,
    pub editor_paragraph_padding_bottom_px: f32,
    pub editor_heading_padding_top_px: f32,
    pub editor_heading_padding_bottom_px: f32,
    pub editor_padding_left_px: f32,
    pub editor_padding_top_px: f32,
    pub editor_non_markdown_padding_top_px: f32,
    pub editor_padding_right_px: f32,
    pub editor_padding_bottom_px: f32,
    pub editor_wrap_right_padding_px: f32,
    pub editor_max_width_px: f32,
    pub editor_non_markdown_max_width_px: f32,
    pub editor_list_content_indent_px: f32,
    pub editor_unordered_list_marker_cycle: [&'static str; 3],
    pub editor_unordered_list_marker_font_size_offset_px: f32,
    pub editor_unordered_list_marker_line_height_offset_px: f32,
    pub editor_superscript_scale_factor: f32,
    pub editor_subscript_scale_factor: f32,
    pub editor_superscript_offset_px: f32,
    pub editor_subscript_offset_px: f32,
    pub editor_bg: Hsla,
    pub editor_text_color: Hsla,
    pub editor_empty_placeholder_opacity: f32,
    pub editor_indent_guide_color: Hsla,
    pub editor_indent_guide_width_px: f32,
    pub editor_indent_guide_dash_length_px: f32,
    pub editor_indent_guide_gap_length_px: f32,
    pub editor_whitespace_dot_size_px: f32,
    pub editor_whitespace_dot_color: Hsla,
    pub editor_whitespace_tab_height_px: f32,
    pub editor_whitespace_tab_stroke_px: f32,
    pub editor_inline_code_bg: Hsla,
    pub editor_inline_code_bg_x_padding_px: f32,
    pub editor_inline_code_bg_radius_px: f32,
    pub editor_inline_code_bg_y_inset_px: f32,
    pub editor_inline_highlight_bg: Hsla,
    pub editor_list_marker_text_color: Hsla,
    pub editor_list_marker_font_weight: FontWeight,
    pub editor_heading_marker_text_color: Hsla,
    pub editor_selection_match_bg: Hsla,
    pub editor_selection_match_border: Hsla,
    pub editor_bracket_match_border: Hsla,
    pub editor_match_border_width_px: f32,
    pub editor_scale_step: f32,
    pub editor_scale_min: f32,
    pub editor_scale_max: f32,
}

// ── Sidebar ─────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SidebarStyles {
    // Container
    pub sidebar_width_px: f32,
    pub sidebar_bg: Hsla,
    pub sidebar_bottom_spacer_height_px: f32,
    pub sidebar_top_buttons_visible: bool,
    pub sidebar_top_button_padding_right_px: f32,
    pub sidebar_top_button_gap_px: f32,
    pub sidebar_top_button_width_px: f32,
    pub sidebar_top_button_height_px: f32,
    pub sidebar_top_button_size_px: f32,
    pub sidebar_top_button_icon_size_px: f32,
    pub sidebar_top_button_inactive_color: Hsla,
    pub sidebar_top_button_active_color: Hsla,
    // Logo
    pub sidebar_logo_size_px: f32,
    pub sidebar_logo_padding_y_px: f32,
    // Header (root directory row)
    pub sidebar_header_height_px: f32,
    pub sidebar_header_bg: Hsla,
    pub sidebar_header_padding_left_px: f32,
    pub sidebar_header_padding_right_px: f32,
    pub sidebar_header_gap_px: f32,
    pub sidebar_header_icon_size_px: f32,
    pub sidebar_header_root_icon_size_px: f32,
    pub sidebar_header_star_icon_size_px: f32,
    pub sidebar_header_icon_color: Hsla,
    pub sidebar_header_font_size_px: f32,
    pub sidebar_header_text_color: Hsla,
    pub sidebar_header_hover_bg: Hsla,
    pub sidebar_header_action_gap_px: f32,
    pub sidebar_header_action_btn_size_px: f32,
    pub sidebar_header_action_icon_size_px: f32,
    pub sidebar_starred_item_close_icon_size_px: f32,
    pub sidebar_header_action_opacity: f32,
    pub sidebar_header_action_hover_opacity: f32,
    // File tree item
    pub sidebar_item_height_px: f32,
    pub sidebar_item_indent_base_px: f32,
    pub sidebar_root_padding_x_px: f32,
    pub sidebar_root_region_max_fraction: f32,
    pub sidebar_min_tree_rows: f32,
    pub sidebar_item_indent_step_px: f32,
    pub sidebar_item_left_shift_px: f32,
    pub sidebar_item_padding_right_px: f32,
    pub sidebar_item_gap_px: f32,
    pub sidebar_item_hover_bg: Hsla,
    pub sidebar_item_selected_bg: Hsla,
    pub sidebar_item_raised_bg: Hsla,
    // File tree item — fold arrow
    pub sidebar_item_fold_container_width_px: f32,
    pub sidebar_item_fold_container_height_px: f32,
    pub sidebar_item_fold_icon_size_px: f32,
    pub sidebar_item_fold_icon_color: Hsla,
    // File tree item — file/folder icon
    pub sidebar_dir_icon_size_px: f32,
    pub sidebar_item_icon_size_px: f32,
    pub sidebar_item_icon_color: Hsla,
    pub sidebar_item_selected_icon_color: Hsla,
    pub sidebar_item_badge_min_width_px: f32,
    pub sidebar_item_badge_height_px: f32,
    pub sidebar_item_badge_padding_x_px: f32,
    pub sidebar_item_badge_radius_px: f32,
    pub sidebar_item_badge_font_size_px: f32,
    pub sidebar_item_badge_bg: Hsla,
    pub sidebar_item_badge_text_color: Hsla,
    // Directory file-count badge (file tree rows, workspace/starred/file list headers)
    pub sidebar_count_badge_min_width_px: f32,
    pub sidebar_count_badge_height_px: f32,
    pub sidebar_count_badge_padding_x_px: f32,
    pub sidebar_count_badge_radius_px: f32,
    pub sidebar_count_badge_font_size_px: f32,
    pub sidebar_count_badge_margin_left_px: f32,
    pub sidebar_count_badge_bg: Hsla,
    pub sidebar_count_badge_text_color: Hsla,
    // File tree item — label
    pub sidebar_item_font_size_px: f32,
    pub sidebar_item_text_color: Hsla,
    pub sidebar_item_selected_text_color: Hsla,
    pub sidebar_filter_muted_color: Hsla,
    pub sidebar_filter_match_color: Hsla,
    pub sidebar_item_suffix_opacity: f32,
    // File tree item — active file dot
    pub sidebar_item_active_dot_size_px: f32,
    pub sidebar_item_active_dot_color: Hsla,
    // Inline edit row
    pub sidebar_edit_input_height_px: f32,
    pub sidebar_edit_input_padding_x_px: f32,
    pub sidebar_edit_input_bg: Hsla,
    pub sidebar_edit_input_border_width_px: f32,
    pub sidebar_edit_input_border_color: Hsla,
    pub sidebar_edit_input_radius_px: f32,
    pub sidebar_edit_input_font_size_px: f32,
    pub sidebar_edit_input_text_color: Hsla,
    pub sidebar_edit_input_placeholder_color: Hsla,
    // Error message
    pub sidebar_error_padding_x_px: f32,
    pub sidebar_error_padding_y_px: f32,
    pub sidebar_error_font_size_px: f32,
    pub sidebar_error_text_color: Hsla,
    // Empty state
    pub sidebar_empty_gap_px: f32,
    pub sidebar_empty_padding_x_px: f32,
    pub sidebar_empty_font_size_px: f32,
    pub sidebar_empty_text_color: Hsla,
    pub sidebar_empty_btn_padding_x_px: f32,
    pub sidebar_empty_btn_padding_y_px: f32,
    pub sidebar_empty_btn_border_width_px: f32,
    pub sidebar_empty_btn_border_color: Hsla,
    pub sidebar_empty_btn_font_size_px: f32,
    pub sidebar_empty_btn_text_color: Hsla,
    pub sidebar_empty_btn_hover_bg: Hsla,
    // Resize handle
    pub sidebar_resize_handle_width_px: f32,
    pub sidebar_resize_indicator_width_px: f32,
    pub sidebar_resize_active_indicator_width_px: f32,
    pub sidebar_resize_indicator_color: Hsla,
    pub sidebar_resize_active_indicator_color: Hsla,
    pub sidebar_resize_min_width_px: f32,
    pub sidebar_resize_max_width_px: f32,
    pub sidebar_visibility_animation_ms: u64,
    // Indent alignment guides
    pub sidebar_indent_guide_color: Hsla,
    pub sidebar_indent_guide_width_px: f32,
    pub sidebar_indent_guide_dash_length_px: f32,
    pub sidebar_indent_guide_gap_length_px: f32,
    // Focus ring
    pub sidebar_item_focus_border_color: Hsla,
    // Active border (selected item)
    pub sidebar_item_active_border_color: Hsla,
    // File list pane (to the right of the file tree)
    pub file_list_width_px: f32,
    pub file_list_bg: Hsla,
    pub file_list_header_height_px: f32,
    pub file_list_header_padding_left_px: f32,
    pub file_list_header_padding_right_px: f32,
    pub file_list_header_gap_px: f32,
    pub file_list_header_font_size_px: f32,
    pub file_list_header_text_color: Hsla,
    pub file_list_header_border_width_px: f32,
    pub file_list_header_border_color: Hsla,
    pub file_list_item_padding_x_px: f32,
    pub file_list_item_padding_y_px: f32,
    pub file_list_item_gap_px: f32,
    pub file_list_item_title_gap_px: f32,
    pub file_list_item_title_font_size_px: f32,
    pub file_list_item_title_line_height_px: f32,
    pub file_list_item_title_color: Hsla,
    pub file_list_item_selected_title_color: Hsla,
    pub file_list_item_preview_font_size_px: f32,
    pub file_list_item_preview_line_height_px: f32,
    pub file_list_item_preview_max_lines: usize,
    pub file_list_item_preview_color: Hsla,
    pub file_list_item_time_font_size_px: f32,
    pub file_list_item_time_line_height_px: f32,
    pub file_list_item_time_color: Hsla,
    pub file_list_item_thumbnail_gap_px: f32,
    pub file_list_item_thumbnail_radius_px: f32,
    pub file_list_item_thumbnail_border_width_px: f32,
    pub file_list_item_thumbnail_border_color: Hsla,
    pub file_list_item_bg: Hsla,
    pub file_list_item_hover_bg: Hsla,
    pub file_list_item_selected_bg: Hsla,
    pub file_list_item_separator_width_px: f32,
    pub file_list_item_separator_color: Hsla,
    pub file_list_empty_font_size_px: f32,
    pub file_list_empty_text_color: Hsla,
    pub file_list_resize_min_width_px: f32,
    pub file_list_resize_max_width_px: f32,
    // Sidebar / file list overlay scrollbar (chrome palette so it stays
    // visible on the dark sidebar even in light mode)
}

// ── Outline ─────────────────────────────────────────────────────────

pub struct OutlineStyles {
    // Rail container (overlay on the editor's right edge)
    pub outline_rail_right_margin_px: f32,
    pub outline_rail_vertical_padding_px: f32,
    // Heading label
    pub outline_rail_label_font_size_px: f32,
    pub outline_rail_label_max_width_px: f32,
    pub outline_rail_label_indent_step_px: f32,
    pub outline_rail_label_indent_guide_width_px: f32,
    pub outline_rail_label_indent_guide_dash_length_px: f32,
    pub outline_rail_label_indent_guide_gap_length_px: f32,
    pub outline_rail_label_indent_guide_color: Hsla,
    pub outline_rail_label_fold_icon_size_px: f32,
    pub outline_rail_label_fold_icon_gap_px: f32,
    pub outline_rail_label_gap_px: f32,
    pub outline_rail_label_item_height_px: f32,
    pub outline_rail_label_item_gap_px: f32,
    pub outline_rail_label_item_padding_y_px: f32,
    pub outline_rail_label_line_height_px: f32,
    pub outline_rail_label_wrap_chars_per_line: usize,
    pub outline_rail_label_max_lines: usize,
    pub outline_rail_label_color: Hsla,
    pub outline_rail_label_hover_color: Hsla,
    pub outline_rail_label_active_color: Hsla,
    pub outline_rail_tick_level_opacities: [f32; 6],
    pub outline_rail_label_level_opacities: [f32; 6],
    pub outline_rail_label_level_brightness_mix: [f32; 6],
    // Row metrics
    pub outline_rail_row_height_px: f32,
    // Tick indicator
    pub outline_rail_tick_width_px: f32,
    pub outline_rail_tick_thickness_px: f32,
    pub outline_rail_tick_color: Hsla,
    pub outline_rail_tick_active_color: Hsla,
    pub outline_rail_tick_active_extra_width_px: f32,
    pub outline_rail_tick_active_neighbor_count: usize,
    pub outline_rail_tick_active_falloff_power: f32,
    pub outline_rail_tick_active_animation_seconds: f32,
    /// Distance (px) over which ticks fade out near the top and bottom edges of
    /// the visible rail viewport, and the hover highlight fade-in duration.
    pub outline_rail_edge_fade_px: f32,
    pub outline_rail_hover_animation_seconds: f32,
    // Flash highlight after clicking a heading
    pub outline_flash_initial_delay_ms: u64,
    pub outline_flash_show_duration_ms: u64,
    pub outline_flash_gap_duration_ms: u64,
}

// ── Utility Panel ───────────────────────────────────────────────────

pub struct UtilityPanelStyles {
    pub utility_panel_width_px: f32,
    pub utility_panel_bg: Hsla,
    pub utility_panel_terminal_padding_px: f32,
    pub utility_panel_border_width_px: f32,
    pub utility_panel_border_color: Hsla,
    pub utility_panel_top_spacer_height_px: f32,
    pub utility_panel_bottom_spacer_height_px: f32,
    pub utility_panel_header_height_px: f32,
    pub utility_panel_header_padding_x_px: f32,
    pub utility_panel_header_font_size_px: f32,
    pub utility_panel_header_text_color: Hsla,
    pub utility_panel_section_gap_px: f32,
    pub utility_panel_section_padding_x_px: f32,
    pub utility_panel_section_title_font_size_px: f32,
    pub utility_panel_section_title_text_color: Hsla,
    pub utility_panel_item_height_px: f32,
    pub utility_panel_item_gap_px: f32,
    pub utility_panel_item_font_size_px: f32,
    pub utility_panel_item_label_text_color: Hsla,
    pub utility_panel_item_value_text_color: Hsla,
    pub utility_panel_terminal_inactive_opacity: f32,
    pub utility_panel_button_height_px: f32,
    pub utility_panel_button_gap_px: f32,
    pub utility_panel_button_padding_x_px: f32,
    pub utility_panel_button_radius_px: f32,
    pub utility_panel_button_border_width_px: f32,
    pub utility_panel_button_border_color: Hsla,
    pub utility_panel_button_text_color: Hsla,
    pub utility_panel_button_hover_bg: Hsla,
    pub utility_panel_empty_font_size_px: f32,
    pub utility_panel_empty_text_color: Hsla,
    pub utility_panel_resize_handle_width_px: f32,
    pub utility_panel_resize_indicator_width_px: f32,
    pub utility_panel_resize_active_indicator_width_px: f32,
    pub utility_panel_resize_indicator_color: Hsla,
    pub utility_panel_resize_active_indicator_color: Hsla,
    pub utility_panel_resize_min_width_px: f32,
    pub utility_panel_resize_max_width_px: f32,
    pub utility_panel_visibility_animation_ms: u64,
}

// ── Tab Bar ─────────────────────────────────────────────────────────

pub struct TabBarStyles {
    // Container
    pub tab_bar_height_px: f32,
    pub tab_bar_bg: Hsla,
    pub tab_bar_border_width_px: f32,
    pub tab_bar_border_color: Hsla,
    // Tab item
    pub tab_item_min_width_px: f32,
    pub tab_item_max_width_px: f32,
    pub tab_item_padding_left_px: f32,
    pub tab_item_padding_right_px: f32,
    pub tab_item_gap_px: f32,
    pub tab_item_bg: Hsla,
    pub tab_item_hover_bg: Hsla,
    pub tab_item_active_bg: Hsla,
    pub tab_item_border_width_px: f32,
    pub tab_item_border_color: Hsla,
    // Tab item — icon
    pub tab_item_icon_size_px: f32,
    pub tab_action_emphasis_icon_size_px: f32,
    pub tab_item_icon_color: Hsla,
    pub tab_item_icon_active_opacity: f32,
    pub tab_item_icon_inactive_opacity: f32,
    pub tab_item_badge_min_width_px: f32,
    pub tab_item_badge_height_px: f32,
    pub tab_item_badge_padding_x_px: f32,
    pub tab_item_badge_radius_px: f32,
    pub tab_item_badge_font_size_px: f32,
    pub tab_item_badge_bg: Hsla,
    pub tab_item_badge_text_color: Hsla,
    // Tab item — label
    pub tab_item_font_size_px: f32,
    pub tab_item_text_color: Hsla,
    pub tab_item_active_text_color: Hsla,
    pub tab_item_suffix_opacity: f32,
    // Tab item — close button
    pub tab_item_close_size_px: f32,
    pub tab_item_close_font_size_px: f32,
    pub tab_item_close_text_color: Hsla,
    pub tab_item_close_hover_bg: Hsla,
    pub tab_item_close_active_opacity: f32,
    pub tab_item_close_inactive_opacity: f32,
    // Tab dirty indicator
    pub tab_item_dirty_dot_size_px: f32,
    pub tab_item_dirty_dot_color: Hsla,
    // Tab deleted indicator
    pub tab_item_deleted_text_color: Hsla,
    // New-tab button
    pub tab_new_width_px: f32,
    pub tab_new_font_size_px: f32,
    pub tab_new_text_color: Hsla,
    pub tab_new_hover_text_color: Hsla,
    // Fixed trailing drop target after the new-tab button
    pub tab_end_drop_target_width_px: f32,
    // Update button
    pub tab_update_button_height_inset_px: f32,
    pub tab_update_button_min_height_px: f32,
    pub tab_update_button_padding_x_px: f32,
    pub tab_update_button_gap_px: f32,
    pub tab_update_button_radius_px: f32,
    pub tab_update_button_opacity: f32,
    pub tab_update_button_hover_opacity: f32,
    pub tab_update_button_icon_size_px: f32,
    pub tab_update_button_font_size_px: f32,
    pub tab_update_button_text_color: Hsla,
    pub tab_update_button_hover_text_color: Hsla,
    pub tab_update_button_icon_color: Hsla,
    // Tab drag ghost
    pub tab_drag_ghost_opacity: f32,
    pub tab_drag_source_opacity: f32,
    // Tab drop indicator
    pub tab_drop_indicator_width_px: f32,
    pub tab_drop_indicator_color: Hsla,
    // Tab pin
    pub tab_pin_icon_size_px: f32,
    pub tab_pin_icon_color: Hsla,
    pub tab_pinned_width_px: f32,
    // Tab overflow indicator
    pub tab_overflow_font_size_px: f32,
    pub tab_overflow_text_color: Hsla,
    pub tab_overflow_bg: Hsla,
    pub tab_overflow_padding_x_px: f32,
    pub tab_history_hidden_sidebar_margin_px: f32,
    pub tab_history_padding_x_px: f32,
    pub tab_history_gap_px: f32,
    pub tab_history_group_border_width_px: f32,
    pub tab_history_group_border_color: Hsla,
    pub tab_history_btn_size_px: f32,
    pub tab_history_btn_radius_px: f32,
    pub tab_history_btn_bg: Hsla,
    pub tab_history_btn_icon_size_px: f32,
    pub tab_history_btn_icon_color: Hsla,
    pub tab_history_btn_hover_icon_color: Hsla,
    pub tab_history_btn_icon_disabled_color: Hsla,
    pub tab_history_btn_enabled_opacity: f32,
    pub tab_history_btn_disabled_opacity: f32,
    pub tab_action_button_padding_x_px: f32,
}

// ── Toolbar ─────────────────────────────────────────────────────────

pub struct ToolbarStyles {
    pub toolbar_offset_left_px: f32,
    pub toolbar_offset_top_px: f32,
    pub toolbar_gap_px: f32,
    pub toolbar_hidden_opacity: f32,
    pub toolbar_opacity_animation_ms: u64,
    pub toolbar_reveal_hover_height_px: f32,
    pub toolbar_corner_reveal_width_px: f32,
    pub toolbar_corner_reveal_height_px: f32,
    pub toolbar_primary_btn_size_px: f32,
    pub toolbar_primary_icon_size_px: f32,
    pub toolbar_primary_hover_opacity: f32,
    pub toolbar_secondary_hover_opacity: f32,
    pub toolbar_secondary_btn_size_px: f32,
    pub toolbar_secondary_icon_size_px: f32,
    pub toolbar_secondary_btn_padding_x_px: f32,
    pub toolbar_btn_shadow: bool,
    pub toolbar_btn_bg: Hsla,
    pub toolbar_btn_radius_px: f32,
}

/// Hover and active states shift icon contrast only; the bar draws no button
/// backgrounds.
pub struct FormatToolbarStyles {
    pub format_toolbar_height_px: f32,
    pub format_toolbar_padding_x_px: f32,
    /// Pulls the row back by the transparent margin baked into the icons, so
    /// the first glyph lines up optically with the body text.
    pub format_toolbar_optical_left_inset_px: f32,
    pub format_toolbar_gap_px: f32,
    pub format_toolbar_group_gap_px: f32,
    pub format_toolbar_bg: Hsla,
    pub format_toolbar_border_width_px: f32,
    pub format_toolbar_border_color: Hsla,
    pub format_toolbar_radius_px: f32,
    pub line_drag_threshold_px: f32,
    pub line_drag_indent_step_px: f32,
    pub line_format_heading_icon_gap_px: f32,
    pub line_drag_indicator_width_px: f32,
    pub line_drag_indicator_color: Hsla,
    pub line_drag_ghost_opacity: f32,
    pub line_drag_source_opacity: f32,
    pub line_drop_flash_opacity: f32,
    pub format_toolbar_btn_size_px: f32,
    pub format_toolbar_icon_size_px: f32,
    pub format_toolbar_icon_color: Hsla,
    pub format_toolbar_icon_hover_color: Hsla,
    pub format_toolbar_icon_active_color: Hsla,
    pub format_toolbar_separator_width_px: f32,
    pub format_toolbar_separator_height_px: f32,
    pub format_toolbar_separator_margin_x_px: f32,
    pub format_toolbar_separator_color: Hsla,
}

// ── Tooltip ─────────────────────────────────────────────────────────

pub struct TooltipStyles {
    pub tooltip_delay_ms: u64,
    pub tooltip_viewport_padding_px: f32,
    pub tooltip_offset_px: f32,
    pub tooltip_max_width_px: f32,
    pub tooltip_padding_x_px: f32,
    pub tooltip_padding_y_px: f32,
    pub tooltip_gap_px: f32,
    pub tooltip_radius_px: f32,
    pub tooltip_border_width_px: f32,
    pub tooltip_bg: Hsla,
    pub tooltip_border_color: Hsla,
    pub tooltip_title_font_size_px: f32,
    pub tooltip_title_text_color: Hsla,
    pub tooltip_subtitle_font_size_px: f32,
    pub tooltip_subtitle_text_color: Hsla,
    pub tooltip_subtitle_opacity: f32,
    pub tooltip_shortcut_gap_px: f32,
    pub tooltip_shortcut_key_min_width_px: f32,
    pub tooltip_shortcut_key_height_px: f32,
    pub tooltip_shortcut_key_padding_x_px: f32,
    pub tooltip_shortcut_key_radius_px: f32,
    pub tooltip_shortcut_key_bg: Hsla,
}

// ── Cursor ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct CursorStyles {
    pub cursor_color: Hsla,
    pub cursor_width_px: f32,
    pub cursor_height_factor: f32,
    pub cursor_blink_pause_ms: u64,
    pub cursor_blink_visible_ms: u64,
    pub cursor_blink_transparent_ms: u64,
    pub cursor_opacity_min: f32,
    pub cursor_opacity_max: f32,
}

impl CursorStyles {
    pub fn next_opacity(&self, current: f32) -> f32 {
        if current > self.cursor_opacity_min {
            self.cursor_opacity_min
        } else {
            self.cursor_opacity_max
        }
    }
    pub fn blink_pause_delay(&self) -> Duration {
        Duration::from_millis(self.cursor_blink_pause_ms)
    }
    pub fn blink_visible_duration(&self) -> Duration {
        Duration::from_millis(self.cursor_blink_visible_ms)
    }
    pub fn blink_transparent_duration(&self) -> Duration {
        Duration::from_millis(self.cursor_blink_transparent_ms)
    }
}

// ── Code Block ──────────────────────────────────────────────────────

pub struct CodeBlockStyles {
    pub code_block_bg: Hsla,
    pub code_block_border_width_px: f32,
    pub code_block_border_color: Hsla,
    /// Brighter divider drawn between the source (left) and rendered (right)
    /// panes in a generated preview's edit view.
    pub code_block_split_border_color: Hsla,
    /// Width of the edit/render split divider and its dash segment length and
    /// gap (the divider is rendered as a dashed line).
    pub code_block_split_border_width_px: f32,
    pub code_block_split_border_dash_length_px: f32,
    pub code_block_split_border_dash_gap_px: f32,
    pub code_block_radius_px: f32,
    pub code_block_fill_inset_px: f32,
    pub code_block_text_padding_left_px: f32,
    pub code_block_line_number_padding_left_px: f32,
    pub code_block_wrap_right_padding_px: f32,
    pub code_block_fence_clip_px: f32,
    pub generated_preview_min_height_px: f32,
    pub generated_preview_max_height_px: f32,
    pub generated_preview_padding_x_px: f32,
    pub generated_preview_padding_y_px: f32,
}

// ── Quote Block ─────────────────────────────────────────────────────

pub struct QuoteBlockStyles {
    pub quote_block_border_width_px: f32,
    pub quote_block_radius_px: f32,
    pub quote_block_padding_top_px: f32,
    pub quote_block_padding_bottom_px: f32,
    /// Extra inner vertical padding applied only to callout blocks
    /// (`> [!KIND]`), on top of the plain quote padding.
    pub quote_block_callout_padding_top_px: f32,
    pub quote_block_callout_padding_bottom_px: f32,
    pub quote_block_padding_left_px: f32,
    /// Extra indent for quoted lines that carry their own line-level marker
    /// (list, ordered list, task), so the nesting reads as one step deeper.
    pub quote_block_line_syntax_indent_px: f32,
    pub quote_block_padding_right_px: f32,
    pub quote_block_callout_icon_color: Hsla,
    pub quote_block_callout_icon_size_factor: f32,
    pub quote_block_callout_icon_min_px: f32,
    pub quote_block_callout_icon_max_line_height_factor: f32,
    pub quote_block_callout_icon_label_gap_px: f32,
    pub quote_block_bg: Hsla,
    pub quote_block_border_color: Hsla,
    pub quote_block_note_bg: Hsla,
    pub quote_block_note_border_color: Hsla,
    pub quote_block_tip_bg: Hsla,
    pub quote_block_tip_border_color: Hsla,
    pub quote_block_important_bg: Hsla,
    pub quote_block_important_border_color: Hsla,
    pub quote_block_warning_bg: Hsla,
    pub quote_block_warning_border_color: Hsla,
    pub quote_block_caution_bg: Hsla,
    pub quote_block_caution_border_color: Hsla,
}

// ── Wiki Link ───────────────────────────────────────────────────────

pub struct WikiLinkStyles {
    pub wiki_link_text_color: Hsla,
    pub wiki_link_text_weight: FontWeight,
}

// ── Selection & Line Highlight ──────────────────────────────────────

pub struct SelectionStyles {
    pub selection_color: Hsla,
    pub multi_selection_color: Hsla,
    pub selection_line_highlight_color: Hsla,
    pub selection_trailing_width_factor: f32,
}

// ── Checkbox ────────────────────────────────────────────────────────

pub struct CheckboxStyles {
    pub checkbox_size_factor: f32,
    pub checkbox_text_gap_px: f32,
    pub checkbox_parent_prefix_chars: f32,
    pub checkbox_radius_px: f32,
    pub checkbox_border_width_px: f32,
    pub checkbox_border_color: Hsla,
    pub checkbox_checked_bg: Hsla,
    pub checkbox_hover_bg: Hsla,
    pub checkbox_check_icon_scale: f32,
    pub checkbox_check_icon_color: Hsla,
    pub checkbox_default_opacity: f32,
    pub checkbox_hover_opacity: f32,
    pub checkbox_hit_padding_px: f32,
}

// ── Table ───────────────────────────────────────────────────────────

pub struct TableStyles {
    pub table_cell_padding_px: f32,
    pub table_border_width_px: f32,
    pub table_border_color: Hsla,
    pub table_outer_radius_px: f32,
    pub table_header_bg: Hsla,
    pub front_matter_key_text_color: Hsla,
    pub front_matter_key_padding_right_px: f32,
    pub table_selection_border_width_px: f32,
}

// ── Gutter ──────────────────────────────────────────────────────────

pub struct GutterStyles {
    // Line numbers
    pub gutter_line_number_min_width_px: f32,
    pub gutter_line_number_max_width_px: f32,
    pub gutter_line_number_font_size_px: f32,
    pub gutter_line_number_vertical_offset_px: f32,
    pub gutter_line_number_right_gap_px: f32,
    pub gutter_column_gap_px: f32,
    pub gutter_line_number_text_color: Hsla,
    pub gutter_line_number_active_text_color: Hsla,
    // Fold icon
    // Heading hash icons
    pub gutter_heading_color: Hsla,
    pub gutter_heading_opacity: f32,
    pub gutter_heading_h1_icon_size_px: f32,
    pub gutter_heading_h2_icon_size_px: f32,
    pub gutter_heading_h3_icon_size_px: f32,
    pub gutter_heading_h4_icon_size_px: f32,
    pub gutter_heading_icon_gap_px: f32,
    pub gutter_heading_icon_offset_px: f32,
    pub gutter_heading_max_total_width_px: f32,
}

// ── Image Preview ───────────────────────────────────────────────────

pub struct ImagePreviewStyles {
    pub image_border_radius_px: f32,
    pub image_active_border_width_px: f32,
    pub image_active_border_color: Hsla,
    pub image_gap_top_px: f32,
    pub image_gap_bottom_px: f32,
}

// ── HR (horizontal rule) ────────────────────────────────────────────

pub struct HrStyles {
    pub hr_dash_length_px: f32,
    pub hr_gap_length_px: f32,
    pub hr_height_px: f32,
    pub hr_color: Hsla,
}

// ── IME ─────────────────────────────────────────────────────────────

pub struct ImeStyles {
    pub ime_underline_height_px: f32,
    pub ime_underline_color: Hsla,
}

// ── Dialog ──────────────────────────────────────────────────────────

pub struct DialogStyles {
    pub dialog_backdrop_color: Hsla,
    pub dialog_bg: Hsla,
    pub dialog_border_width_px: f32,
    pub dialog_border_color: Hsla,
    pub dialog_radius_px: f32,
    pub dialog_width_px: f32,
    pub dialog_padding_x_px: f32,
    pub dialog_padding_y_px: f32,
    pub dialog_gap_px: f32,
    pub dialog_title_font_size_px: f32,
    pub dialog_title_text_color: Hsla,
    pub dialog_message_font_size_px: f32,
    pub dialog_message_text_color: Hsla,
    pub dialog_btn_height_px: f32,
    pub dialog_btn_padding_x_px: f32,
    pub dialog_btn_font_size_px: f32,
    pub dialog_btn_radius_px: f32,
    pub dialog_btn_gap_px: f32,
    pub dialog_btn_primary_bg: Hsla,
    pub dialog_btn_primary_text_color: Hsla,
    pub dialog_btn_primary_hover_bg: Hsla,
    pub dialog_btn_secondary_bg: Hsla,
    pub dialog_btn_secondary_text_color: Hsla,
    pub dialog_btn_secondary_hover_bg: Hsla,
    pub dialog_btn_danger_bg: Hsla,
    pub dialog_btn_danger_text_color: Hsla,
    pub dialog_btn_danger_hover_bg: Hsla,
}

// ── Notifications (toast) ───────────────────────────────────────────

pub struct NotificationStyles {
    pub notification_width_px: f32,
    pub notification_margin_right_px: f32,
    pub notification_margin_bottom_px: f32,
    pub notification_gap_px: f32,
    pub notification_padding_x_px: f32,
    pub notification_padding_y_px: f32,
    pub notification_radius_px: f32,
    pub notification_border_width_px: f32,
    pub notification_bg: Hsla,
    pub notification_border_color: Hsla,
    pub notification_text_color: Hsla,
    pub notification_font_size_px: f32,
    pub notification_hover_opacity: f32,
    pub notification_dismiss_after_ms: u64,
    pub notification_max_visible: usize,
}

// ── Context Menu ────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ContextMenuStyles {
    pub menu_item_action_gap_px: f32,
    pub menu_item_check_size_px: f32,
    pub menu_item_check_color: Hsla,
    pub menu_item_action_icon_color: Hsla,
    pub item_presentation: ItemPresentation,
    // Container
    pub menu_bg: Hsla,
    pub menu_border_width_px: f32,
    pub menu_border_color: Hsla,
    pub menu_radius_px: f32,
    pub menu_shadow: ShadowStyle,
    pub menu_padding_y_px: f32,
    pub menu_submenu_gap_px: f32,
    pub menu_max_height_margin_px: f32,
    pub menu_opacity_animation_ms: u64,
    // Item
    pub menu_item_height_px: f32,
    pub menu_item_radius_px: f32,
    pub menu_item_inset_x_px: f32,
    pub menu_item_disabled_opacity: f32,
    pub menu_item_padding_x_px: f32,
    pub menu_item_font_size_px: f32,
    pub menu_item_text_color: Hsla,
    pub menu_item_primary_text_color: Hsla,
    pub menu_item_danger_text_color: Hsla,
    pub menu_item_hover_bg: Hsla,
    pub menu_item_selected_bg: Hsla,
    // Separator
    pub menu_separator_height_px: f32,
    pub menu_separator_margin_y_px: f32,
    pub menu_separator_color: Hsla,
    // Shortcut label
    pub menu_shortcut_font_size_px: f32,
    pub menu_shortcut_font_family: &'static str,
    pub menu_shortcut_text_color: Hsla,
    pub menu_shortcut_gap_px: f32,
    pub menu_shortcut_key_min_width_px: f32,
    pub menu_shortcut_key_height_px: f32,
    pub menu_shortcut_key_padding_x_px: f32,
    pub menu_shortcut_key_radius_px: f32,
    pub menu_shortcut_key_bg: Hsla,
    // Format menu icon
    pub menu_format_icon_size_px: f32,
    pub menu_format_icon_gap_px: f32,
    pub menu_format_icon_color: Hsla,
    pub menu_format_active_text_color: Hsla,
    // Widths per menu kind
    pub menu_sidebar_width_px: f32,
    pub menu_format_width_px: f32,
    pub menu_export_width_px: f32,
    pub menu_tab_width_px: f32,
}

// ── TextInput ───────────────────────────────────────────────────────

#[derive(Clone)]
pub struct TextInputStyles {
    pub text_input_focus_ring: RingStyle,
    pub text_input_height_px: f32,
    pub text_input_padding_left_px: f32,
    pub text_input_padding_right_px: f32,
    pub text_input_font_size_px: f32,
    pub text_input_bg: Hsla,
    pub text_input_hover_bg: Option<Hsla>,
    pub text_input_focus_bg: Hsla,
    pub text_input_text_color: Hsla,
    pub text_input_placeholder_color: Hsla,
    pub text_input_focus_placeholder_color: Option<Hsla>,
    pub text_input_border_width_px: f32,
    pub text_input_border_color: Hsla,
    pub text_input_focus_border_color: Hsla,
    pub text_input_radius_px: f32,
    pub cursor: CursorStyles,
    pub cursor_fade: bool,
    pub text_input_selection_color: Hsla,
    pub text_input_center: bool,
    /// Right-align text within the input (overridden by `text_input_center`).
    pub text_input_right: bool,
    /// Shares menu chrome with all application menus; no mirrored color fields.
    pub menu: ContextMenuStyles,
    pub menu_width_px: f32,
    pub menu_scale: f32,
}

// ── Search (in-editor buffer search) ────────────────────────────────

pub struct SearchStyles {
    // Search bar container
    pub search_bar_width_px: f32,
    pub search_bar_padding_px: f32,
    pub search_bar_bg: Hsla,
    pub search_bar_border_width_px: f32,
    pub search_bar_border_color: Hsla,
    pub search_bar_radius_px: f32,
    pub search_bar_shadow: ShadowStyle,
    pub search_bar_gap_px: f32,
    pub search_bar_offset_top_px: f32,
    pub search_bar_offset_right_px: f32,
    pub search_input_gap_px: f32,
    pub search_input_height_px: f32,
    pub search_input_font_size_px: f32,
    pub search_input_icon_size_px: f32,
    pub search_input_icon_color: Hsla,
    pub search_input_icon_gap_px: f32,
    pub search_input_left_inset_px: f32,
    pub search_input_right_inset_px: f32,
    pub search_input_counter_min_width_px: f32,
    pub search_input_focus_bg: Hsla,
    // Search option buttons
    pub search_option_btn_size_px: f32,
    pub search_option_btn_icon_size_px: f32,
    pub search_option_btn_radius_px: f32,
    pub search_option_btn_active_bg: Hsla,
    pub search_option_btn_hover_bg: Hsla,
    pub search_option_btn_icon_color: Hsla,
    pub search_option_btn_active_icon_color: Hsla,
    // Match counter
    pub search_counter_font_size_px: f32,
    pub search_counter_text_color: Hsla,
    // Navigation buttons
    pub search_nav_btn_size_px: f32,
    pub search_nav_btn_icon_size_px: f32,
    pub search_nav_btn_icon_color: Hsla,
    pub search_nav_btn_hover_bg: Hsla,
    // Match highlighting in editor
    pub search_match_bg: Hsla,
    pub search_match_active_bg: Hsla,
    pub search_match_text_color: Hsla,
    pub search_match_active_text_color: Hsla,
    pub search_match_border_width_px: f32,
    pub search_match_border_color: Hsla,
    // Scrollbar match markers
    pub search_scrollbar_marker_width_px: f32,
    pub search_scrollbar_marker_height_px: f32,
    pub search_scrollbar_marker_color: Hsla,
    // Replace
    pub search_replace_btn_height_px: f32,
    pub search_replace_btn_padding_x_px: f32,
    pub search_replace_btn_font_size_px: f32,
    pub search_replace_btn_bg: Hsla,
    pub search_replace_btn_text_color: Hsla,
    pub search_replace_btn_hover_bg: Hsla,
    // Close button
    pub search_close_btn_size_px: f32,
    pub search_close_btn_icon_color: Hsla,
    pub search_close_btn_hover_bg: Hsla,
}

// ── Workspace Search (sidebar global search) ────────────────────────

pub struct WorkspaceSearchStyles {
    // Search area
    pub ws_search_padding_px: f32,
    pub ws_search_gap_px: f32,
    pub ws_search_input_gap_px: f32,
    pub ws_search_input_left_inset_px: f32,
    pub ws_search_input_right_inset_px: f32,
    pub ws_search_input_icon_size_px: f32,
    pub ws_search_input_icon_color: Hsla,
    pub ws_search_input_icon_gap_px: f32,
    pub ws_search_input_height_px: f32,
    pub ws_search_input_radius_px: f32,
    pub ws_search_input_bg: Hsla,
    pub ws_search_input_hover_bg: Hsla,
    pub ws_search_input_focus_bg: Hsla,
    pub ws_search_input_border_width_px: f32,
    pub ws_search_input_border_color: Hsla,
    // File group header (sticky)
    pub ws_search_file_header_height_px: f32,
    pub ws_search_file_header_bg: Hsla,
    pub ws_search_file_header_border_width_px: f32,
    pub ws_search_file_header_border_color: Hsla,
    pub ws_search_file_header_font_size_px: f32,
    pub ws_search_file_header_text_color: Hsla,
    pub ws_search_file_header_dir_gap_px: f32,
    pub ws_search_file_header_dir_text_color: Hsla,
    pub ws_search_file_header_icon_size_px: f32,
    pub ws_search_file_header_count_font_size_px: f32,
    pub ws_search_file_header_count_text_color: Hsla,
    // Match line item
    pub ws_search_match_height_px: f32,
    pub ws_search_match_padding_x_px: f32,
    pub ws_search_match_font_size_px: f32,
    pub ws_search_match_text_color: Hsla,
    pub ws_search_match_highlight_bg: Hsla,
    pub ws_search_match_highlight_text_color: Hsla,
    pub ws_search_match_line_number_color: Hsla,
    pub ws_search_match_hover_bg: Hsla,
    pub ws_search_match_active_bg: Hsla,
    // Progress
    pub ws_search_progress_height_px: f32,
    pub ws_search_progress_color: Hsla,
    // Summary
    pub ws_search_summary_height_px: f32,
    pub ws_search_summary_margin_bottom_px: f32,
    pub ws_search_summary_font_size_px: f32,
    pub ws_search_summary_text_color: Hsla,
    // Sidebar search icon button
    pub ws_search_btn_size_px: f32,
    pub ws_search_btn_icon_size_px: f32,
    pub ws_search_btn_icon_color: Hsla,
    pub ws_search_btn_active_icon_color: Hsla,
    pub ws_search_btn_hover_bg: Hsla,
    pub ws_search_btn_active_bg: Hsla,
}

// ── Quick Open ─────────────────────────────────────────────────────

pub struct QuickOpenStyles {
    pub quick_open_backdrop_bg: Hsla,
    pub quick_open_panel_width_px: f32,
    pub quick_open_panel_top_offset_px: f32,
    pub quick_open_panel_radius_px: f32,
    pub quick_open_panel_border_color: Hsla,
    pub quick_open_panel_bg: Hsla,
    pub quick_open_input_padding_x_px: f32,
    pub quick_open_input_padding_y_px: f32,
    pub quick_open_input_text_padding_left_px: f32,
    pub quick_open_input_bg: Hsla,
    pub quick_open_input_focus_bg: Hsla,
    pub quick_open_input_text_color: Hsla,
    pub quick_open_results_max_height_px: f32,
    pub quick_open_result_row_height_px: f32,
    pub quick_open_result_padding_x_px: f32,
    pub quick_open_result_gap_y_px: f32,
    pub quick_open_result_name_font_size_px: f32,
    pub quick_open_result_path_font_size_px: f32,
    pub quick_open_result_hover_bg: Hsla,
    pub quick_open_result_selected_bg: Hsla,
    pub quick_open_result_text_color: Hsla,
    pub quick_open_result_accent_text_color: Hsla,
    pub quick_open_result_secondary_text_color: Hsla,
    pub quick_open_result_shortcut_bg: Hsla,
    pub quick_open_result_match_text_color: Hsla,
    pub quick_open_empty_height_px: f32,
    pub quick_open_empty_font_size_px: f32,
    pub quick_open_empty_text_color: Hsla,
}

// ── Settings Panel ─────────────────────────────────────────────────

pub struct SettingsPanelStyles {
    pub settings_title_version_gap_px: f32,
    pub settings_title_version_font_size_px: f32,
    pub settings_title_version_text_color: Hsla,
    pub settings_title_version_margin_bottom_px: f32,
    pub settings_primary_text_color: Hsla,
    pub settings_secondary_text_color: Hsla,
    pub settings_header_gap_px: f32,
    pub settings_header_actions_gap_px: f32,
    pub settings_header_button_max_width_px: f32,
    pub settings_site_button_height_px: f32,
    pub settings_site_button_padding_x_px: f32,
    pub settings_site_button_radius_px: f32,
    pub settings_site_button_border_width_px: f32,
    pub settings_site_button_gap_px: f32,
    pub settings_site_button_icon_size_px: f32,
    pub settings_site_button_font_size_px: f32,
    pub settings_site_button_text_color: Hsla,
    pub settings_site_button_icon_color: Hsla,
    pub settings_site_button_update_text_color: Hsla,
    pub settings_site_button_update_icon_color: Hsla,
    pub settings_site_button_bg: Hsla,
    pub settings_site_button_hover_bg: Hsla,
    pub settings_site_button_border_color: Hsla,
    pub settings_option_button_height_px: f32,
    pub settings_option_button_padding_x_px: f32,
    pub settings_option_button_radius_px: f32,
    pub settings_option_button_border_width_px: f32,
    pub settings_option_button_gap_px: f32,
    pub settings_option_button_font_size_px: f32,
    pub settings_option_button_text_color: Hsla,
    pub settings_option_button_active_text_color: Hsla,
    pub settings_option_button_bg: Hsla,
    pub settings_option_button_hover_bg: Hsla,
    pub settings_option_button_active_bg: Hsla,
    pub settings_option_button_border_color: Hsla,
    pub settings_option_button_active_border_color: Hsla,
    pub settings_contrast_border_color: Hsla,
    pub settings_input_height_px: f32,
    pub settings_input_radius_px: f32,
    pub settings_stepper_input_radius_px: f32,
    pub settings_content_width_input_width_px: f32,
    pub settings_stepper_hover_bg: Hsla,
    pub settings_theme_group_gap_px: f32,
    pub settings_theme_toggle_height_px: f32,
    pub settings_theme_toggle_padding_x_px: f32,
    pub settings_theme_toggle_gap_px: f32,
    pub settings_theme_toggle_radius_px: f32,
    pub settings_theme_toggle_border_width_px: f32,
    pub settings_theme_toggle_icon_size_px: f32,
    pub settings_theme_option_height_px: f32,
    pub settings_theme_option_padding_x_px: f32,
    pub settings_theme_option_radius_px: f32,
    pub settings_theme_option_border_width_px: f32,
    pub settings_theme_option_font_size_px: f32,
    pub settings_theme_option_hover_bg: Hsla,
    pub settings_theme_option_active_bg: Hsla,
    pub settings_theme_option_active_text_color: Hsla,
    pub settings_theme_option_text_color: Hsla,
    pub settings_theme_option_border_color: Hsla,
}

// ═════════════════════════════════════════════════════════════════════
// Construction
// ═════════════════════════════════════════════════════════════════════

impl Styles {
    /// `theme` drives editor content colors and floating controls.
    /// `ui_theme` drives window and sidebar chrome.
    pub fn from_theme(theme: &Theme, ui_theme: &Theme) -> Self {
        Self::from_theme_with_options(theme, ui_theme, true)
    }

    /// Re-resolve colors while retaining the installed, non-persisted UI style.
    pub fn retheme(&self, theme: &Theme, ui_theme: &Theme, sidebar_always_dark: bool) -> Self {
        Self::from_theme_with_spec(
            theme,
            ui_theme,
            sidebar_always_dark,
            self.ui.base_spec.clone(),
        )
    }

    /// `sidebar_always_dark = false` renders sidebar surfaces (FileTree,
    /// FileList, workspace search) with the editor theme instead of the dark
    /// chrome palette.
    pub fn from_theme_with_options(
        theme: &Theme,
        ui_theme: &Theme,
        sidebar_always_dark: bool,
    ) -> Self {
        Self::from_theme_with_spec(theme, ui_theme, sidebar_always_dark, UiStyleSpec::default())
    }

    pub fn from_theme_with_spec(
        theme: &Theme,
        ui_theme: &Theme,
        sidebar_always_dark: bool,
        spec: UiStyleSpec,
    ) -> Self {
        // Light palettes need their muted foregrounds and hairlines deepened
        // before any style token is derived from them.
        let switch_theme = theme;
        let theme_contrast = theme.with_light_surface_contrast();
        let ui_theme_contrast = ui_theme.with_light_surface_contrast();
        let dark_sidebar_theme = ThemeKind::OneDark.build();
        let theme = &theme_contrast;
        let ui_theme = &ui_theme_contrast;

        let ui_font_size_px = 13.0;
        let ui_font_size_sm_px = ui_font_size_px - 1.0;
        let ui_font_size_xs_px = ui_font_size_px - 2.0;
        let ui_font_size_lg_px = ui_font_size_px + 1.0;
        let editor_body_font_size_px = 14.0;
        let editor_heading_base_font_size_px = 15.0;

        let is_light = theme.bg_primary.l >= 0.5;
        let format_toolbar_theme = theme;
        let menu_theme = format_toolbar_theme;
        let sidebar_theme = if sidebar_always_dark && is_light {
            &dark_sidebar_theme
        } else if sidebar_always_dark {
            ui_theme
        } else {
            theme
        };
        let is_light_monochrome = is_light && theme.heading == theme.text_primary;
        let light_monochrome_heading_color = if is_light_monochrome {
            theme.heading
        } else {
            theme.list_marker
        };
        let checkbox_border_color = if is_light_monochrome {
            theme.heading
        } else {
            theme.border_tertiary
        };
        let checkbox_checked_bg = if is_light_monochrome {
            theme.heading
        } else {
            theme.checkbox_checked
        };
        let text_input_theme = if is_light { theme } else { ui_theme };
        let settings_theme = if is_light { theme } else { ui_theme };
        let utility_panel_theme = theme;
        let settings_active_bg = if is_light {
            theme.bg_selected
        } else {
            ui_theme.text_primary
        };
        let settings_active_text_color = if is_light {
            theme.text_bright
        } else {
            ui_theme.bg_primary
        };
        let settings_primary_text_color = if is_light {
            theme.text_bright
        } else {
            theme.text_primary
        };
        let settings_secondary_text_color = if is_light {
            with_alpha(theme.text_primary, 0.82)
        } else {
            theme.text_secondary
        };
        let settings_update_color = settings_theme.text_warning;
        let self_drawn = SelfDrawnStyles::new(theme, sidebar_theme, spec.self_drawn);
        let mut ui = UiStyles::new(settings_theme, spec);
        ui.colors = switch_theme.application_ui_palette();
        let mut menu = ContextMenuStyles {
            menu_item_action_gap_px: 10.0,
            menu_item_check_size_px: 12.0,
            menu_item_check_color: menu_theme.text_secondary,
            menu_item_action_icon_color: menu_theme.text_secondary,
            item_presentation: ui.spec.composition.item,
            menu_bg: menu_theme.bg_primary,
            menu_border_width_px: 1.0,
            menu_border_color: menu_theme.border_secondary,
            menu_radius_px: 8.0,
            menu_shadow: if is_light && !ui.spec.appearance.elevation {
                ShadowStyle::default()
            } else {
                ui.small_shadow.clone()
            },
            menu_padding_y_px: 6.0,
            menu_submenu_gap_px: 2.0,
            menu_max_height_margin_px: 0.0,
            menu_opacity_animation_ms: ui.spec.menu_motion_duration.as_millis() as u64,
            menu_item_height_px: ui.spec.menu_row_height_px
                * ui.spec.composition.item.height_factor(),
            menu_item_radius_px: 0.0,
            menu_item_inset_x_px: 0.0,
            menu_item_disabled_opacity: ui.spec.disabled_opacity,
            menu_item_padding_x_px: 10.0 * ui.spec.composition.item.padding_factor(),
            menu_item_font_size_px: ui_font_size_sm_px,
            menu_item_text_color: with_alpha(menu_theme.text_bright, 0.84),
            menu_item_primary_text_color: with_alpha(menu_theme.text_bright, 0.96),
            menu_item_danger_text_color: with_alpha(menu_theme.text_danger, 0.84),
            menu_item_hover_bg: menu_theme.bg_hover,
            menu_item_selected_bg: menu_theme.bg_hover,
            menu_shortcut_font_size_px: ui_font_size_sm_px - 0.5,
            menu_shortcut_font_family: if cfg!(target_os = "macos") {
                "BlinkMacSystemFont"
            } else {
                crate::platform::fonts::SYSTEM_UI
            },
            menu_shortcut_text_color: with_alpha(menu_theme.text_secondary, 0.78),
            menu_shortcut_gap_px: 3.0,
            menu_shortcut_key_min_width_px: 14.0,
            menu_shortcut_key_height_px: 16.0,
            menu_shortcut_key_padding_x_px: 4.0,
            menu_shortcut_key_radius_px: 4.0,
            menu_shortcut_key_bg: with_alpha(menu_theme.bg_hover, 0.95),
            menu_format_icon_size_px: 13.0,
            menu_format_icon_gap_px: 6.0,
            menu_format_icon_color: with_alpha(menu_theme.text_secondary, 0.8),
            menu_format_active_text_color: with_alpha(menu_theme.text_primary, 0.92),
            menu_separator_height_px: 1.0,
            menu_separator_margin_y_px: 0.0,
            menu_separator_color: menu_theme.border_secondary,
            menu_sidebar_width_px: 180.0,
            menu_format_width_px: 210.0,
            menu_export_width_px: 150.0,
            menu_tab_width_px: 180.0,
        };
        if ui.spec.menu_surface == MenuSurface::Inverted {
            menu.menu_bg = menu_theme.text_primary;
            menu.menu_item_primary_text_color = menu_theme.bg_primary;
            menu.menu_item_text_color = menu_theme.bg_secondary;
            menu.menu_item_hover_bg = menu_theme.text_secondary;
            menu.menu_item_selected_bg = menu.menu_item_hover_bg;
        }
        if ui.spec.menu_emphasis == MenuEmphasis::Bold {
            menu.menu_item_selected_bg = menu_theme.bg_selected;
        }
        let popup_shadow = ui.popup_shadow.clone();
        let switch_ring = ui.control.ring;
        let disabled_opacity = ui.spec.disabled_opacity;
        let loading_opacity = ui.spec.loading_opacity;
        let switch_motion_duration = ui.spec.motion_duration(ui.spec.switch_motion_duration);
        let easing = ui.spec.easing;
        let mut theme_debugger = ThemeDebuggerStyles::from_theme(switch_theme);
        theme_debugger.slider_height_px = ui.spec.application.color_slider_height_px;
        theme_debugger.slider_handle_width_px = ui.spec.application.color_slider_handle_width_px;
        theme_debugger.sv_handle_size_px = ui.spec.application.color_sv_handle_size_px;
        let cursor = CursorStyles {
            cursor_color: crate::theme::EDITOR_CARET,
            cursor_width_px: 2.0,
            cursor_height_factor: 1.0,
            cursor_blink_pause_ms: 420,
            cursor_blink_visible_ms: 480,
            cursor_blink_transparent_ms: 480,
            cursor_opacity_min: 0.0,
            cursor_opacity_max: 1.0,
        };
        let cursor_fade = ui.base_spec.animations_enabled;
        let mut styles = Self {
            minimap: MinimapStyles {
                width_px: 96.0,
                opacity: 0.4,
                min_pane_width_px: 480.0,
                padding_px: 4.0,
                min_handle_hit_height_px: 18.0,
                border_width_px: 1.0,
                colors: theme.minimap_palette(),
            },
            line_diff: LineDiffStyles::from_theme(theme, is_light),
            ui,
            self_drawn,
            theme_debugger,
            switch: SwitchStyles {
                colors: switch_theme.switch_palette(settings_theme),
                ring: switch_ring,
                disabled_opacity,
                loading_opacity,
                row_height_px: 30.0,
                track_width_px: 36.0,
                track_height_px: 21.0,
                track_padding_px: 2.0,
                track_radius_px: 10.5,
                border_width_px: 1.5,
                thumb_size_px: 15.0,
                thumb_radius_px: 7.5,
                thumb_travel_px: 15.0,
                label_gap_px: 8.0,
                label_font_size_px: 12.0,
                animation_duration: switch_motion_duration,
                easing,
                cursor: gpui::CursorStyle::Arrow,
            },
            ui_font_family: crate::platform::fonts::SYSTEM_UI,
            ui_font_size_px,
            anchored_popup_gap_px: 2.0,
            anchored_popup_margin_px: 8.0,
            status_bar_padding_left_px: 0.0,
            window: WindowStyles {
                window_bg: theme.bg_primary,
                window_border_width_px: if is_light { 0.0 } else { 1.0 },
                window_border_color: theme.window_border,
                // Only macOS rounds its own frame to match; elsewhere a rounded
                // client area just cuts corners out of a square window.
                window_radius_px: if cfg!(target_os = "macos") { 16.0 } else { 0.0 },
            },
            editor: EditorStyles {
                editor_font_size_px: editor_body_font_size_px,
                editor_h1_font_size_px: editor_heading_base_font_size_px * 1.4,
                editor_h2_font_size_px: editor_heading_base_font_size_px * 1.3,
                editor_h3_font_size_px: editor_heading_base_font_size_px * 1.2,
                editor_h4_font_size_px: editor_heading_base_font_size_px * 1.1,
                editor_h5_font_size_px: editor_heading_base_font_size_px * 1.05,
                editor_h6_font_size_px: editor_heading_base_font_size_px,
                editor_paragraph_line_height_factor: 1.5,
                editor_heading_line_height_factor: 1.5,
                editor_letter_spacing_px: 0.0,
                editor_body_latin_font_family: crate::platform::fonts::MONOSPACE,
                editor_body_cjk_font_family: crate::platform::fonts::CJK,
                editor_heading_latin_font_family: crate::platform::fonts::MONOSPACE,
                editor_heading_cjk_font_family: crate::platform::fonts::CJK,
                editor_italic_latin_font_family: crate::platform::fonts::MONOSPACE,
                editor_italic_cjk_font_family: crate::platform::fonts::ITALIC_CJK,
                editor_monospace_font_family: crate::platform::fonts::MONOSPACE,
                editor_paragraph_padding_top_px: 0.0,
                editor_paragraph_padding_bottom_px: 0.0,
                editor_heading_padding_top_px: 10.0,
                editor_heading_padding_bottom_px: 10.0,
                editor_padding_left_px: 40.0,
                editor_padding_top_px: 80.0,
                editor_non_markdown_padding_top_px: 28.0,
                editor_padding_right_px: 40.0,
                editor_padding_bottom_px: 200.0,
                editor_wrap_right_padding_px: 24.0,
                editor_max_width_px: 700.0,
                editor_non_markdown_max_width_px: 1120.0,
                editor_list_content_indent_px: 10.0,
                editor_unordered_list_marker_cycle: ["• ", "◦ ", "▪ "],
                editor_unordered_list_marker_font_size_offset_px: 0.0,
                editor_unordered_list_marker_line_height_offset_px: 0.0,
                editor_superscript_scale_factor: 0.72,
                editor_subscript_scale_factor: 0.72,
                editor_superscript_offset_px: 2.0,
                editor_subscript_offset_px: 2.0,
                editor_bg: theme.bg_primary,
                editor_text_color: theme.text_primary,
                editor_empty_placeholder_opacity: 0.3,
                editor_indent_guide_color: with_alpha(
                    theme.border_primary,
                    if is_light { 1.0 } else { 0.35 },
                ),
                editor_indent_guide_width_px: 1.0,
                editor_indent_guide_dash_length_px: 3.0,
                editor_indent_guide_gap_length_px: 0.0,
                editor_whitespace_dot_size_px: 2.0,
                editor_whitespace_tab_height_px: 6.0,
                editor_whitespace_tab_stroke_px: 1.0,
                editor_whitespace_dot_color: with_alpha(theme.text_secondary, 0.35),
                editor_inline_code_bg: theme.inline_code_bg,
                editor_inline_code_bg_x_padding_px: 4.0,
                editor_inline_code_bg_radius_px: 4.0,
                editor_inline_code_bg_y_inset_px: 2.0,
                editor_inline_highlight_bg: theme.highlight_bg,
                editor_list_marker_text_color: light_monochrome_heading_color,
                editor_list_marker_font_weight: FontWeight::BOLD,
                editor_heading_marker_text_color: with_alpha(
                    theme.text_secondary,
                    if is_light { 0.50 } else { 0.40 },
                ),
                editor_selection_match_bg: with_alpha(
                    theme.text_secondary,
                    if is_light { 0.10 } else { 0.16 },
                ),
                editor_selection_match_border: with_alpha(
                    theme.text_secondary,
                    if is_light { 0.24 } else { 0.30 },
                ),
                editor_bracket_match_border: with_alpha(theme.text_primary, 0.5),
                editor_match_border_width_px: 1.0,
                editor_scale_step: 0.1,
                editor_scale_min: 0.8,
                editor_scale_max: 1.6,
            },
            sidebar: SidebarStyles {
                sidebar_width_px: 230.0,
                sidebar_bg: sidebar_theme.bg_secondary,
                sidebar_bottom_spacer_height_px: 30.0,
                sidebar_top_buttons_visible: false,
                sidebar_top_button_padding_right_px: 0.0,
                sidebar_top_button_gap_px: 6.0,
                sidebar_top_button_width_px: TOP_BAR_HEIGHT_PX,
                sidebar_top_button_height_px: TOP_BAR_HEIGHT_PX,
                sidebar_top_button_size_px: 18.0,
                sidebar_top_button_icon_size_px: 15.0,
                sidebar_top_button_inactive_color: sidebar_theme.text_secondary,
                sidebar_top_button_active_color: sidebar_theme.text_primary,
                sidebar_logo_size_px: 50.0,
                sidebar_logo_padding_y_px: 12.0,
                sidebar_header_height_px: 26.0,
                sidebar_header_bg: gpui::transparent_black(),
                sidebar_header_padding_left_px: 14.0,
                sidebar_header_padding_right_px: 4.0,
                sidebar_header_gap_px: 6.0,
                sidebar_header_icon_size_px: 12.0,
                sidebar_header_root_icon_size_px: 15.0,
                sidebar_header_star_icon_size_px: 15.0,
                sidebar_header_icon_color: sidebar_theme.text_secondary,
                sidebar_header_font_size_px: ui_font_size_px,
                sidebar_header_text_color: sidebar_theme.text_primary,
                sidebar_header_hover_bg: sidebar_theme.bg_hover,
                sidebar_header_action_gap_px: 2.0,
                sidebar_header_action_btn_size_px: 20.0,
                sidebar_header_action_icon_size_px: 14.0,
                sidebar_starred_item_close_icon_size_px: 12.0,
                sidebar_header_action_opacity: 0.7,
                sidebar_header_action_hover_opacity: 1.0,
                sidebar_item_height_px: 26.0,
                sidebar_item_indent_base_px: 24.0,
                sidebar_root_padding_x_px: 14.0,
                sidebar_root_region_max_fraction: 0.25,
                sidebar_min_tree_rows: 3.0,
                sidebar_item_indent_step_px: 16.0,
                sidebar_item_left_shift_px: 10.0,
                sidebar_item_padding_right_px: 8.0,
                sidebar_item_gap_px: 4.0,
                sidebar_item_hover_bg: sidebar_theme.bg_hover,
                sidebar_item_selected_bg: sidebar_theme.bg_selected,
                sidebar_item_raised_bg: sidebar_theme.bg_primary,
                sidebar_item_fold_container_width_px: 16.0,
                sidebar_item_fold_container_height_px: 18.0,
                sidebar_item_fold_icon_size_px: 12.0,
                sidebar_item_fold_icon_color: sidebar_theme.text_secondary,
                sidebar_dir_icon_size_px: 16.0,
                sidebar_item_icon_size_px: 14.0,
                sidebar_item_icon_color: sidebar_theme.text_secondary,
                sidebar_item_selected_icon_color: sidebar_theme.text_bright,
                sidebar_item_badge_min_width_px: 14.0,
                sidebar_item_badge_height_px: 14.0,
                sidebar_item_badge_padding_x_px: 3.0,
                sidebar_item_badge_radius_px: 3.0,
                sidebar_item_badge_font_size_px: 10.0,
                sidebar_item_badge_bg: with_alpha(sidebar_theme.text_secondary, 0.3),
                sidebar_item_badge_text_color: sidebar_theme.text_secondary,
                sidebar_count_badge_min_width_px: 16.0,
                sidebar_count_badge_height_px: 15.0,
                sidebar_count_badge_padding_x_px: 4.0,
                sidebar_count_badge_radius_px: 4.0,
                sidebar_count_badge_font_size_px: 10.0,
                sidebar_count_badge_margin_left_px: 6.0,
                sidebar_count_badge_bg: with_alpha(sidebar_theme.text_secondary, 0.18),
                sidebar_count_badge_text_color: with_alpha(sidebar_theme.text_secondary, 0.92),
                sidebar_item_font_size_px: ui_font_size_px,
                sidebar_item_text_color: sidebar_theme.text_primary,
                sidebar_item_selected_text_color: sidebar_theme.text_bright,
                sidebar_filter_muted_color: with_alpha(sidebar_theme.text_secondary, 0.65),
                sidebar_filter_match_color: sidebar_theme.text_warning,
                sidebar_item_suffix_opacity: 0.48,
                sidebar_item_active_dot_size_px: 5.0,
                sidebar_item_active_dot_color: sidebar_theme.text_accent,
                sidebar_edit_input_height_px: 20.0,
                sidebar_edit_input_padding_x_px: 6.0,
                sidebar_edit_input_bg: gpui::transparent_black(),
                sidebar_edit_input_border_width_px: 1.0,
                sidebar_edit_input_border_color: sidebar_theme.text_accent,
                sidebar_edit_input_radius_px: 3.0,
                sidebar_edit_input_font_size_px: ui_font_size_px,
                sidebar_edit_input_text_color: sidebar_theme.text_bright,
                sidebar_edit_input_placeholder_color: sidebar_theme.text_secondary,
                sidebar_error_padding_x_px: 14.0,
                sidebar_error_padding_y_px: 12.0,
                sidebar_error_font_size_px: ui_font_size_sm_px,
                sidebar_error_text_color: sidebar_theme.text_danger,
                sidebar_empty_gap_px: 12.0,
                sidebar_empty_padding_x_px: 16.0,
                sidebar_empty_font_size_px: ui_font_size_sm_px,
                sidebar_empty_text_color: sidebar_theme.text_secondary,
                sidebar_empty_btn_padding_x_px: 16.0,
                sidebar_empty_btn_padding_y_px: 5.0,
                sidebar_empty_btn_border_width_px: 1.5,
                sidebar_empty_btn_border_color: sidebar_theme.border_primary,
                sidebar_empty_btn_font_size_px: ui_font_size_sm_px,
                sidebar_empty_btn_text_color: sidebar_theme.text_secondary,
                sidebar_empty_btn_hover_bg: sidebar_theme.bg_hover,
                sidebar_resize_handle_width_px: 5.0,
                sidebar_resize_indicator_width_px: 1.0,
                sidebar_resize_active_indicator_width_px: 3.0,
                sidebar_resize_indicator_color: sidebar_theme.border_primary,
                sidebar_resize_active_indicator_color: sidebar_theme.text_accent,
                sidebar_resize_min_width_px: 140.0,
                sidebar_resize_max_width_px: 600.0,
                sidebar_visibility_animation_ms: 220,
                sidebar_indent_guide_color: with_alpha(sidebar_theme.border_secondary, 0.5),
                sidebar_indent_guide_width_px: 1.0,
                sidebar_indent_guide_dash_length_px: 3.0,
                sidebar_indent_guide_gap_length_px: 3.0,
                sidebar_item_focus_border_color: with_alpha(sidebar_theme.text_accent, 0.3),
                sidebar_item_active_border_color: with_alpha(sidebar_theme.text_accent, 0.2),
                file_list_width_px: 240.0,
                file_list_bg: sidebar_theme.bg_secondary,
                file_list_header_height_px: TOP_BAR_HEIGHT_PX,
                file_list_header_padding_left_px: 12.0,
                file_list_header_padding_right_px: 4.0,
                file_list_header_gap_px: 6.0,
                file_list_header_font_size_px: ui_font_size_px,
                file_list_header_text_color: sidebar_theme.text_primary,
                file_list_header_border_width_px: 1.0,
                file_list_header_border_color: sidebar_theme.border_primary,
                file_list_item_padding_x_px: 12.0,
                file_list_item_padding_y_px: 8.0,
                file_list_item_gap_px: 3.0,
                file_list_item_title_gap_px: 6.0,
                file_list_item_title_font_size_px: ui_font_size_px,
                file_list_item_title_line_height_px: 18.0,
                file_list_item_title_color: sidebar_theme.text_primary,
                file_list_item_selected_title_color: sidebar_theme.text_bright,
                file_list_item_preview_font_size_px: ui_font_size_sm_px,
                file_list_item_preview_line_height_px: 16.0,
                file_list_item_preview_max_lines: 2,
                file_list_item_preview_color: sidebar_theme.text_secondary,
                file_list_item_time_font_size_px: ui_font_size_xs_px,
                file_list_item_time_line_height_px: 15.0,
                file_list_item_time_color: with_alpha(sidebar_theme.text_secondary, 0.75),
                file_list_item_thumbnail_gap_px: 10.0,
                file_list_item_thumbnail_radius_px: 6.0,
                file_list_item_thumbnail_border_width_px: 0.0,
                file_list_item_thumbnail_border_color: sidebar_theme.border_primary,
                file_list_item_bg: sidebar_theme.bg_secondary,
                file_list_item_hover_bg: sidebar_theme.bg_primary,
                file_list_item_selected_bg: sidebar_theme.bg_primary,
                file_list_item_separator_width_px: 1.0,
                file_list_item_separator_color: with_alpha(sidebar_theme.border_secondary, 0.35),
                file_list_empty_font_size_px: ui_font_size_sm_px,
                file_list_empty_text_color: sidebar_theme.text_secondary,
                file_list_resize_min_width_px: 160.0,
                file_list_resize_max_width_px: 480.0,
            },
            outline: OutlineStyles {
                outline_rail_right_margin_px: 14.0,
                outline_rail_vertical_padding_px: 20.0,
                outline_rail_label_font_size_px: ui_font_size_sm_px,
                outline_rail_label_max_width_px: 240.0,
                outline_rail_label_indent_step_px: 12.0,
                outline_rail_label_indent_guide_width_px: 1.0,
                outline_rail_label_indent_guide_dash_length_px: 3.0,
                outline_rail_label_indent_guide_gap_length_px: 3.0,
                outline_rail_label_indent_guide_color: with_alpha(theme.text_secondary, 0.6),
                outline_rail_label_fold_icon_size_px: 10.0,
                outline_rail_label_fold_icon_gap_px: 2.0,
                outline_rail_label_gap_px: 8.0,
                outline_rail_label_item_height_px: 18.0,
                outline_rail_label_item_gap_px: 0.0,
                outline_rail_label_item_padding_y_px: 4.0,
                outline_rail_label_line_height_px: 15.0,
                outline_rail_label_wrap_chars_per_line: 24,
                outline_rail_label_max_lines: 3,
                outline_rail_label_color: theme.text_secondary,
                outline_rail_label_hover_color: theme.text_bright,
                outline_rail_label_active_color: theme.text_bright,
                outline_rail_tick_level_opacities: [1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                outline_rail_label_level_opacities: [1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                outline_rail_label_level_brightness_mix: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                outline_rail_row_height_px: 6.0,
                outline_rail_tick_width_px: 8.0,
                outline_rail_tick_thickness_px: 2.0,
                outline_rail_tick_color: with_alpha(theme.text_primary, 0.3),
                outline_rail_tick_active_color: with_alpha(theme.text_primary, 0.58),
                outline_rail_tick_active_extra_width_px: 12.0,
                outline_rail_tick_active_neighbor_count: 3,
                outline_rail_tick_active_falloff_power: 2.2,
                outline_rail_tick_active_animation_seconds: 0.24,
                outline_rail_edge_fade_px: 48.0,
                outline_rail_hover_animation_seconds: 0.2,
                outline_flash_initial_delay_ms: 500,
                outline_flash_show_duration_ms: 170,
                outline_flash_gap_duration_ms: 90,
            },
            utility_panel: UtilityPanelStyles {
                utility_panel_width_px: RIGHT_PANEL_MIN_WIDTH_PX,
                utility_panel_bg: utility_panel_theme.bg_secondary,
                utility_panel_terminal_padding_px: 10.0,
                utility_panel_border_width_px: 1.0,
                utility_panel_border_color: utility_panel_theme.border_primary,
                utility_panel_top_spacer_height_px: 14.0,
                utility_panel_bottom_spacer_height_px: 14.0,
                utility_panel_header_height_px: 15.0,
                utility_panel_header_padding_x_px: 14.0,
                utility_panel_header_font_size_px: ui_font_size_xs_px,
                utility_panel_header_text_color: utility_panel_theme.text_primary,
                utility_panel_section_gap_px: 18.0,
                utility_panel_section_padding_x_px: 14.0,
                utility_panel_section_title_font_size_px: ui_font_size_xs_px,
                utility_panel_section_title_text_color: utility_panel_theme.text_primary,
                utility_panel_item_height_px: 24.0,
                utility_panel_item_gap_px: 8.0,
                utility_panel_item_font_size_px: ui_font_size_sm_px,
                utility_panel_item_label_text_color: utility_panel_theme.text_secondary,
                utility_panel_item_value_text_color: utility_panel_theme.text_primary,
                utility_panel_terminal_inactive_opacity: 1.0,
                utility_panel_button_height_px: 28.0,
                utility_panel_button_gap_px: 8.0,
                utility_panel_button_padding_x_px: 10.0,
                utility_panel_button_radius_px: 6.0,
                utility_panel_button_border_width_px: 1.0,
                utility_panel_button_border_color: utility_panel_theme.border_primary,
                utility_panel_button_text_color: utility_panel_theme.text_primary,
                utility_panel_button_hover_bg: utility_panel_theme.bg_hover,
                utility_panel_empty_font_size_px: ui_font_size_sm_px,
                utility_panel_empty_text_color: utility_panel_theme.text_secondary,
                utility_panel_resize_handle_width_px: 5.0,
                utility_panel_resize_indicator_width_px: 1.0,
                utility_panel_resize_active_indicator_width_px: 3.0,
                utility_panel_resize_indicator_color: utility_panel_theme.border_primary,
                utility_panel_resize_active_indicator_color: utility_panel_theme.text_accent,
                utility_panel_resize_min_width_px: RIGHT_PANEL_MIN_WIDTH_PX,
                utility_panel_resize_max_width_px: RIGHT_PANEL_MAX_WIDTH_PX,
                utility_panel_visibility_animation_ms: 220,
            },
            tab_bar: TabBarStyles {
                tab_bar_height_px: TOP_BAR_HEIGHT_PX,
                tab_bar_bg: theme.bg_bar,
                tab_bar_border_width_px: 1.0,
                tab_bar_border_color: theme.border_secondary,
                tab_item_min_width_px: 52.0,
                tab_item_max_width_px: 240.0,
                tab_item_padding_left_px: 9.0,
                tab_item_padding_right_px: 6.0,
                tab_item_gap_px: 5.0,
                tab_item_bg: theme.bg_tab,
                tab_item_hover_bg: theme.bg_tab_hover,
                tab_item_active_bg: theme.bg_tab_active,
                tab_item_border_width_px: 1.0,
                tab_item_border_color: theme.border_secondary,
                tab_item_icon_size_px: 14.0,
                tab_action_emphasis_icon_size_px: 16.0,
                tab_item_icon_color: theme.text_secondary,
                tab_item_icon_active_opacity: 1.0,
                tab_item_icon_inactive_opacity: 0.4,
                tab_item_badge_min_width_px: 14.0,
                tab_item_badge_height_px: 12.0,
                tab_item_badge_padding_x_px: 3.0,
                tab_item_badge_radius_px: 3.0,
                tab_item_badge_font_size_px: 8.0,
                tab_item_badge_bg: with_alpha(theme.text_secondary, 0.3),
                tab_item_badge_text_color: theme.text_secondary,
                tab_item_font_size_px: ui_font_size_sm_px,
                tab_item_text_color: theme.text_primary,
                tab_item_active_text_color: theme.text_bright,
                tab_item_suffix_opacity: 0.5,
                tab_item_close_size_px: 14.0,
                tab_item_close_font_size_px: ui_font_size_xs_px - 1.0,
                tab_item_close_text_color: theme.text_primary,
                tab_item_close_hover_bg: with_alpha(theme.text_bright, 0.20),
                tab_item_close_active_opacity: 1.0,
                tab_item_close_inactive_opacity: 0.0,
                tab_item_dirty_dot_size_px: 6.0,
                tab_item_dirty_dot_color: theme.text_accent,
                tab_item_deleted_text_color: theme.text_secondary,
                tab_new_width_px: 30.0,
                tab_new_font_size_px: ui_font_size_lg_px,
                tab_new_text_color: theme.text_secondary,
                tab_new_hover_text_color: theme.text_bright,
                tab_end_drop_target_width_px: 12.0,
                tab_update_button_height_inset_px: 6.0,
                tab_update_button_min_height_px: 18.0,
                tab_update_button_padding_x_px: 8.0,
                tab_update_button_gap_px: 4.0,
                tab_update_button_radius_px: 6.0,
                tab_update_button_opacity: 0.8,
                tab_update_button_hover_opacity: 1.0,
                tab_update_button_icon_size_px: 14.0,
                tab_update_button_font_size_px: ui_font_size_sm_px,
                tab_update_button_text_color: ui_theme.text_warning,
                tab_update_button_hover_text_color: ui_theme.text_warning_hover,
                tab_update_button_icon_color: ui_theme.text_warning,
                tab_drag_ghost_opacity: 0.7,
                tab_drag_source_opacity: 0.3,
                tab_drop_indicator_width_px: 2.0,
                tab_drop_indicator_color: theme.text_accent,
                tab_pin_icon_size_px: 11.0,
                tab_pin_icon_color: theme.text_secondary,
                tab_pinned_width_px: 30.0,
                tab_overflow_font_size_px: ui_font_size_xs_px,
                tab_overflow_text_color: theme.text_secondary,
                tab_overflow_bg: theme.bg_tab,
                tab_overflow_padding_x_px: 4.0,
                tab_history_hidden_sidebar_margin_px: 72.0,
                tab_history_padding_x_px: 0.0,
                tab_history_gap_px: 4.0,
                tab_history_group_border_width_px: 1.0,
                tab_history_group_border_color: theme.border_secondary,
                tab_history_btn_size_px: 24.0,
                tab_history_btn_radius_px: 6.0,
                tab_history_btn_bg: theme.bg_tab,
                tab_history_btn_icon_size_px: 11.0,
                tab_history_btn_icon_color: theme.text_primary,
                tab_history_btn_hover_icon_color: theme.text_bright,
                tab_history_btn_icon_disabled_color: theme.text_secondary,
                tab_history_btn_enabled_opacity: 1.0,
                tab_history_btn_disabled_opacity: 0.45,
                tab_action_button_padding_x_px: 5.0,
            },
            toolbar: ToolbarStyles {
                toolbar_offset_left_px: 15.0,
                toolbar_offset_top_px: 15.0,
                toolbar_gap_px: 0.0,
                toolbar_hidden_opacity: 0.0,
                toolbar_opacity_animation_ms: 120,
                toolbar_reveal_hover_height_px: 120.0,
                toolbar_corner_reveal_width_px: 100.0,
                toolbar_corner_reveal_height_px: 100.0,
                toolbar_primary_btn_size_px: 55.0,
                toolbar_primary_icon_size_px: 55.0,
                toolbar_primary_hover_opacity: 0.9,
                toolbar_secondary_hover_opacity: 0.9,
                toolbar_secondary_btn_size_px: 50.0,
                toolbar_secondary_icon_size_px: 50.0,
                toolbar_secondary_btn_padding_x_px: 6.0,
                toolbar_btn_shadow: false,
                toolbar_btn_bg: with_alpha(ui_theme.bg_elevated, 0.85),
                toolbar_btn_radius_px: 8.0,
            },
            format_toolbar: FormatToolbarStyles {
                format_toolbar_height_px: 30.0,
                format_toolbar_padding_x_px: 4.0,
                format_toolbar_optical_left_inset_px: 5.0,
                format_toolbar_gap_px: 1.0,
                format_toolbar_group_gap_px: 2.0,
                format_toolbar_bg: format_toolbar_theme.bg_primary,
                format_toolbar_border_width_px: 1.0,
                format_toolbar_border_color: format_toolbar_theme.border_secondary,
                format_toolbar_radius_px: 6.0,
                line_drag_threshold_px: 4.0,
                line_drag_indent_step_px: 24.0,
                line_format_heading_icon_gap_px: 1.0,
                line_drag_indicator_width_px: 2.0,
                line_drag_indicator_color: theme.cursor,
                line_drag_ghost_opacity: 0.5,
                line_drag_source_opacity: 0.5,
                line_drop_flash_opacity: 0.18,
                format_toolbar_btn_size_px: 22.0,
                format_toolbar_icon_size_px: 13.0,
                format_toolbar_icon_color: with_alpha(format_toolbar_theme.text_secondary, 1.0),
                format_toolbar_icon_hover_color: format_toolbar_theme.text_bright,
                format_toolbar_icon_active_color: format_toolbar_theme.text_bright,
                format_toolbar_separator_width_px: 1.0,
                format_toolbar_separator_height_px: 14.0,
                format_toolbar_separator_margin_x_px: 5.0,
                format_toolbar_separator_color: with_alpha(
                    format_toolbar_theme.text_secondary,
                    0.38,
                ),
            },
            tooltip: TooltipStyles {
                tooltip_delay_ms: 0,
                tooltip_viewport_padding_px: 8.0,
                tooltip_offset_px: 6.0,
                tooltip_max_width_px: 320.0,
                tooltip_padding_x_px: 4.0,
                tooltip_padding_y_px: 3.0,
                tooltip_gap_px: 4.0,
                tooltip_radius_px: 6.0,
                tooltip_border_width_px: 1.0,
                tooltip_bg: with_alpha(ui_theme.bg_elevated, 1.0),
                tooltip_border_color: with_alpha(ui_theme.border_primary, 0.32),
                tooltip_title_font_size_px: ui_font_size_xs_px,
                tooltip_title_text_color: ui_theme.text_primary,
                tooltip_subtitle_font_size_px: ui_font_size_xs_px - 1.0,
                tooltip_subtitle_text_color: ui_theme.text_primary,
                tooltip_subtitle_opacity: 0.72,
                tooltip_shortcut_gap_px: 4.0,
                tooltip_shortcut_key_min_width_px: 14.0,
                tooltip_shortcut_key_height_px: 15.0,
                tooltip_shortcut_key_padding_x_px: 4.0,
                tooltip_shortcut_key_radius_px: 5.0,
                tooltip_shortcut_key_bg: with_alpha(ui_theme.bg_code, 0.98),
            },
            cursor: cursor.clone(),
            code_block: CodeBlockStyles {
                code_block_bg: theme.bg_code,
                code_block_border_width_px: 1.0,
                code_block_border_color: theme.border_code_block,
                code_block_split_border_color: if is_light {
                    with_alpha(theme.text_secondary, 0.5)
                } else {
                    with_alpha(theme.text_secondary, 0.2)
                },
                code_block_split_border_width_px: 1.0,
                code_block_split_border_dash_length_px: 3.0,
                code_block_split_border_dash_gap_px: 3.0,
                code_block_radius_px: 0.0,
                code_block_fill_inset_px: 1.0,
                code_block_text_padding_left_px: 20.0,
                code_block_line_number_padding_left_px: 23.0,
                code_block_wrap_right_padding_px: 20.0,
                code_block_fence_clip_px: 15.0,
                generated_preview_min_height_px: 50.0,
                generated_preview_max_height_px: 300.0,
                generated_preview_padding_x_px: 16.0,
                generated_preview_padding_y_px: 12.0,
            },
            quote_block: QuoteBlockStyles {
                quote_block_border_width_px: 4.0,
                quote_block_radius_px: 0.0,
                quote_block_padding_top_px: 0.0,
                quote_block_padding_bottom_px: 0.0,
                quote_block_callout_padding_top_px: 5.0,
                quote_block_callout_padding_bottom_px: 5.0,
                quote_block_padding_left_px: 12.0,
                quote_block_line_syntax_indent_px: 10.0,
                quote_block_padding_right_px: 10.0,
                quote_block_callout_icon_color: theme.bold,
                quote_block_callout_icon_size_factor: 1.0,
                quote_block_callout_icon_min_px: 14.0,
                quote_block_callout_icon_max_line_height_factor: 1.5,
                quote_block_callout_icon_label_gap_px: 3.0,
                quote_block_bg: with_alpha(theme.text_secondary, if is_light { 0.0 } else { 0.0 }),
                quote_block_border_color: with_alpha(
                    theme.text_secondary,
                    if is_light { 0.72 } else { 0.3 },
                ),
                quote_block_note_bg: with_alpha(theme.link, if is_light { 0.10 } else { 0.08 }),
                quote_block_note_border_color: with_alpha(
                    theme.link,
                    if is_light { 1.0 } else { 0.6 },
                ),
                quote_block_tip_bg: with_alpha(
                    theme.status_success,
                    if is_light { 0.12 } else { 0.08 },
                ),
                quote_block_tip_border_color: with_alpha(
                    theme.status_success,
                    if is_light { 1.0 } else { 0.58 },
                ),
                quote_block_important_bg: with_alpha(
                    theme.italic,
                    if is_light { 0.12 } else { 0.08 },
                ),
                quote_block_important_border_color: with_alpha(
                    theme.italic,
                    if is_light { 1.0 } else { 0.58 },
                ),
                quote_block_warning_bg: with_alpha(
                    theme.text_warning,
                    if is_light { 0.14 } else { 0.09 },
                ),
                quote_block_warning_border_color: with_alpha(
                    theme.text_warning,
                    if is_light { 1.0 } else { 0.58 },
                ),
                quote_block_caution_bg: with_alpha(
                    theme.text_danger,
                    if is_light { 0.10 } else { 0.08 },
                ),
                quote_block_caution_border_color: with_alpha(
                    theme.text_danger,
                    if is_light { 1.0 } else { 0.58 },
                ),
            },
            wiki_link: WikiLinkStyles {
                wiki_link_text_color: theme.inline_code,
                wiki_link_text_weight: FontWeight::BOLD,
            },
            selection: SelectionStyles {
                selection_color: theme.selection,
                multi_selection_color: with_alpha(theme.highlight_bg, 0.62),
                selection_line_highlight_color: theme.line_highlight,
                selection_trailing_width_factor: 0.5,
            },
            checkbox: CheckboxStyles {
                checkbox_size_factor: 1.2,
                checkbox_text_gap_px: 10.0,
                checkbox_parent_prefix_chars: 2.0,
                checkbox_radius_px: 5.0,
                checkbox_border_width_px: 1.5,
                checkbox_border_color,
                checkbox_checked_bg,
                checkbox_hover_bg: with_alpha(theme.text_bright, 0.12),
                checkbox_check_icon_scale: 0.52,
                checkbox_check_icon_color: theme.text_on_accent,
                checkbox_default_opacity: 0.9,
                checkbox_hover_opacity: 1.0,
                checkbox_hit_padding_px: 2.0,
            },
            table: TableStyles {
                table_cell_padding_px: 5.0,
                table_border_width_px: 1.0,
                table_border_color: theme.border_decorative,
                table_outer_radius_px: 6.0,
                table_header_bg: theme.bg_code,
                front_matter_key_text_color: with_alpha(theme.text_bright, 1.0),
                front_matter_key_padding_right_px: 10.0,
                table_selection_border_width_px: 2.0,
            },
            gutter: GutterStyles {
                gutter_line_number_min_width_px: 45.0,
                gutter_line_number_max_width_px: 50.0,
                gutter_line_number_font_size_px: ui_font_size_px,
                gutter_line_number_vertical_offset_px: 0.0,
                gutter_line_number_right_gap_px: 0.0,
                gutter_column_gap_px: 5.0,
                gutter_line_number_text_color: with_alpha(theme.text_secondary, 0.48),
                gutter_line_number_active_text_color: theme.text_bright,
                gutter_heading_color: theme.gutter_heading,
                gutter_heading_opacity: 1.0,
                gutter_heading_h1_icon_size_px: 22.0,
                gutter_heading_h2_icon_size_px: 20.0,
                gutter_heading_h3_icon_size_px: 18.0,
                gutter_heading_h4_icon_size_px: 16.0,
                gutter_heading_icon_gap_px: 0.0,
                gutter_heading_icon_offset_px: 10.0,
                gutter_heading_max_total_width_px: 56.0,
            },
            image_preview: ImagePreviewStyles {
                image_border_radius_px: 0.0,
                image_active_border_width_px: 2.0,
                image_active_border_color: theme.border_primary,
                image_gap_top_px: 4.0,
                image_gap_bottom_px: 4.0,
            },
            hr: HrStyles {
                hr_dash_length_px: 6.0,
                hr_gap_length_px: 4.0,
                hr_height_px: 1.0,
                hr_color: theme.border_decorative,
            },
            ime: ImeStyles {
                ime_underline_height_px: 1.0,
                ime_underline_color: theme.text_primary,
            },
            context_menu: menu.clone(),
            dialog: DialogStyles {
                dialog_backdrop_color: with_alpha(ui_theme.bg_primary, 0.3),
                dialog_bg: ui_theme.bg_elevated,
                dialog_border_width_px: 1.0,
                dialog_border_color: ui_theme.border_primary,
                dialog_radius_px: 12.0,
                dialog_width_px: 320.0,
                dialog_padding_x_px: 20.0,
                dialog_padding_y_px: 18.0,
                dialog_gap_px: 12.0,
                dialog_title_font_size_px: ui_font_size_lg_px,
                dialog_title_text_color: ui_theme.text_bright,
                dialog_message_font_size_px: ui_font_size_px,
                dialog_message_text_color: ui_theme.text_secondary,
                dialog_btn_height_px: 28.0,
                dialog_btn_padding_x_px: 14.0,
                dialog_btn_font_size_px: ui_font_size_sm_px,
                dialog_btn_radius_px: 6.0,
                dialog_btn_gap_px: 8.0,
                dialog_btn_primary_bg: ui_theme.text_accent,
                dialog_btn_primary_text_color: ui_theme.text_on_accent,
                dialog_btn_primary_hover_bg: Theme::dialog_hover_background(
                    ui_theme.text_accent,
                    ui_theme.text_on_accent,
                ),
                dialog_btn_secondary_bg: ui_theme.bg_hover,
                dialog_btn_secondary_text_color: ui_theme.text_primary,
                dialog_btn_secondary_hover_bg: Theme::dialog_secondary_hover_background(
                    ui_theme.bg_hover,
                    ui_theme.bg_selected,
                ),
                dialog_btn_danger_bg: ui_theme.text_danger,
                dialog_btn_danger_text_color: ui_theme.text_on_accent,
                dialog_btn_danger_hover_bg: Theme::dialog_hover_background(
                    ui_theme.text_danger,
                    ui_theme.text_on_accent,
                ),
            },
            notification: NotificationStyles {
                notification_width_px: 340.0,
                notification_margin_right_px: 16.0,
                notification_margin_bottom_px: 44.0,
                notification_gap_px: 8.0,
                notification_padding_x_px: 14.0,
                notification_padding_y_px: 10.0,
                notification_radius_px: 0.0,
                notification_border_width_px: 1.0,
                notification_bg: ui_theme.bg_elevated,
                notification_border_color: ui_theme.border_primary,
                notification_text_color: ui_theme.text_primary,
                notification_font_size_px: ui_font_size_px,
                notification_hover_opacity: 0.9,
                notification_dismiss_after_ms: 5000,
                notification_max_visible: 4,
            },
            text_input: TextInputStyles {
                text_input_focus_ring: RingStyle {
                    color: gpui::transparent_black(),
                    invalid_color: gpui::transparent_black(),
                    width_px: 0.0,
                    offset_px: 0.0,
                    offset_color: gpui::transparent_black(),
                },
                text_input_height_px: 24.0,
                text_input_padding_left_px: 6.0,
                text_input_padding_right_px: 6.0,
                text_input_font_size_px: ui_font_size_px,
                text_input_bg: text_input_theme.bg_primary,
                text_input_hover_bg: None,
                text_input_focus_bg: text_input_theme.bg_elevated,
                text_input_text_color: text_input_theme.text_bright,
                text_input_placeholder_color: text_input_theme.text_secondary,
                text_input_focus_placeholder_color: None,
                text_input_border_width_px: 1.0,
                text_input_border_color: text_input_theme.border_primary,
                text_input_focus_border_color: text_input_theme.text_accent,
                text_input_radius_px: 4.0,
                cursor,
                cursor_fade,
                text_input_selection_color: text_input_theme.selection,
                text_input_center: false,
                text_input_right: false,
                menu: menu.clone(),
                menu_width_px: 160.0,
                menu_scale: 1.0,
            },
            search: SearchStyles {
                search_bar_width_px: 380.0,
                search_bar_padding_px: 5.0,
                search_bar_bg: ui_theme.bg_primary,
                search_bar_border_width_px: 1.0,
                search_bar_border_color: ui_theme.border_primary,
                search_bar_radius_px: 8.0,
                search_bar_shadow: popup_shadow.clone(),
                search_bar_gap_px: 4.0,
                search_bar_offset_top_px: 8.0,
                search_bar_offset_right_px: 8.0,
                search_input_gap_px: 4.0,
                search_input_height_px: 24.0,
                search_input_font_size_px: ui_font_size_sm_px,
                search_input_icon_size_px: 12.0,
                search_input_icon_color: ui_theme.text_secondary,
                search_input_icon_gap_px: 3.0,
                search_input_left_inset_px: 24.0,
                search_input_right_inset_px: 8.0,
                search_input_counter_min_width_px: 40.0,
                search_input_focus_bg: gpui::transparent_black(),
                search_option_btn_size_px: 18.0,
                search_option_btn_icon_size_px: 12.0,
                search_option_btn_radius_px: 4.0,
                search_option_btn_active_bg: ui_theme.bg_selected,
                search_option_btn_hover_bg: with_alpha(ui_theme.text_bright, 0.20),
                search_option_btn_icon_color: ui_theme.text_secondary,
                search_option_btn_active_icon_color: ui_theme.text_accent,
                search_counter_font_size_px: ui_font_size_sm_px,
                search_counter_text_color: ui_theme.text_secondary,
                search_nav_btn_size_px: 18.0,
                search_nav_btn_icon_size_px: 12.0,
                search_nav_btn_icon_color: ui_theme.text_secondary,
                search_nav_btn_hover_bg: with_alpha(ui_theme.text_bright, 0.20),
                search_match_bg: with_alpha(theme.highlight_bg, 0.18),
                search_match_active_bg: with_alpha(theme.bold, 0.62),
                search_match_text_color: ui_theme.text_bright,
                search_match_active_text_color: ui_theme.text_bright,
                search_match_border_width_px: 1.0,
                search_match_border_color: with_alpha(theme.highlight_bg, 0.5),
                search_scrollbar_marker_width_px: 3.0,
                search_scrollbar_marker_height_px: 2.0,
                search_scrollbar_marker_color: with_alpha(theme.highlight_bg, 0.6),
                search_replace_btn_height_px: 20.0,
                search_replace_btn_padding_x_px: 7.0,
                search_replace_btn_font_size_px: ui_font_size_xs_px,
                search_replace_btn_bg: ui_theme.bg_hover,
                search_replace_btn_text_color: ui_theme.text_primary,
                search_replace_btn_hover_bg: ui_theme.bg_selected,
                search_close_btn_size_px: 18.0,
                search_close_btn_icon_color: ui_theme.text_secondary,
                search_close_btn_hover_bg: with_alpha(ui_theme.text_bright, 0.20),
            },
            workspace_search: WorkspaceSearchStyles {
                ws_search_padding_px: 10.0,
                ws_search_gap_px: 8.0,
                ws_search_input_gap_px: 4.0,
                ws_search_input_left_inset_px: 28.0,
                ws_search_input_right_inset_px: 108.0,
                ws_search_input_icon_size_px: 13.0,
                ws_search_input_icon_color: sidebar_theme.text_secondary,
                ws_search_input_icon_gap_px: 4.0,
                ws_search_input_height_px: 32.0,
                ws_search_input_radius_px: 6.0,
                ws_search_input_bg: sidebar_theme.bg_elevated,
                ws_search_input_hover_bg: sidebar_theme.bg_hover,
                ws_search_input_focus_bg: sidebar_theme.bg_tab_active,
                ws_search_input_border_width_px: 1.0,
                ws_search_input_border_color: sidebar_theme.border_primary,
                ws_search_file_header_height_px: 28.0,
                ws_search_file_header_bg: sidebar_theme.bg_primary,
                ws_search_file_header_border_width_px: 1.0,
                ws_search_file_header_border_color: sidebar_theme.border_primary,
                ws_search_file_header_font_size_px: ui_font_size_sm_px,
                ws_search_file_header_text_color: with_alpha(sidebar_theme.text_bright, 0.94),
                ws_search_file_header_dir_gap_px: 5.0,
                ws_search_file_header_dir_text_color: with_alpha(
                    sidebar_theme.text_secondary,
                    0.72,
                ),
                ws_search_file_header_icon_size_px: 14.0,
                ws_search_file_header_count_font_size_px: ui_font_size_xs_px,
                ws_search_file_header_count_text_color: sidebar_theme.text_secondary,
                ws_search_match_height_px: 24.0,
                ws_search_match_padding_x_px: 14.0,
                ws_search_match_font_size_px: ui_font_size_sm_px,
                ws_search_match_text_color: with_alpha(sidebar_theme.text_bright, 0.94),
                ws_search_match_highlight_bg: with_alpha(theme.highlight_bg, 0.3),
                ws_search_match_highlight_text_color: sidebar_theme.text_bright,
                ws_search_match_line_number_color: sidebar_theme.text_secondary,
                ws_search_match_hover_bg: sidebar_theme.bg_hover,
                ws_search_match_active_bg: sidebar_theme.bg_selected,
                ws_search_progress_height_px: 2.0,
                ws_search_progress_color: sidebar_theme.text_accent,
                ws_search_summary_height_px: 22.0,
                ws_search_summary_margin_bottom_px: 4.0,
                ws_search_summary_font_size_px: ui_font_size_xs_px,
                ws_search_summary_text_color: sidebar_theme.text_secondary,
                ws_search_btn_size_px: 22.0,
                ws_search_btn_icon_size_px: 14.0,
                ws_search_btn_icon_color: sidebar_theme.text_secondary,
                ws_search_btn_active_icon_color: sidebar_theme.text_accent,
                ws_search_btn_hover_bg: with_alpha(sidebar_theme.text_bright, 0.20),
                ws_search_btn_active_bg: sidebar_theme.bg_selected,
            },
            quick_open: QuickOpenStyles {
                quick_open_backdrop_bg: with_alpha(ui_theme.bg_primary, 0.5),
                quick_open_panel_width_px: 640.0,
                quick_open_panel_top_offset_px: 48.0,
                quick_open_panel_radius_px: 8.0,
                quick_open_panel_border_color: menu_theme.border_secondary,
                quick_open_panel_bg: menu_theme.bg_primary,
                quick_open_input_padding_x_px: 8.0,
                quick_open_input_padding_y_px: 6.0,
                quick_open_input_text_padding_left_px: 2.0,
                quick_open_input_bg: gpui::transparent_black(),
                quick_open_input_focus_bg: gpui::transparent_black(),
                quick_open_input_text_color: menu_theme.text_primary,
                quick_open_results_max_height_px: 360.0,
                quick_open_result_row_height_px: 28.0,
                quick_open_result_padding_x_px: 10.0,
                quick_open_result_gap_y_px: 2.0,
                quick_open_result_name_font_size_px: ui_font_size_sm_px,
                quick_open_result_path_font_size_px: ui_font_size_xs_px - 1.0,
                quick_open_result_hover_bg: menu_theme.bg_hover,
                quick_open_result_selected_bg: menu_theme.bg_selected,
                quick_open_result_text_color: with_alpha(menu_theme.text_bright, 0.96),
                quick_open_result_accent_text_color: switch_theme.text_accent,
                quick_open_result_secondary_text_color: with_alpha(menu_theme.text_secondary, 0.68),
                quick_open_result_shortcut_bg: with_alpha(
                    menu_theme.bg_code,
                    if is_light { 0.12 } else { 0.28 },
                ),
                quick_open_result_match_text_color: menu_theme.text_bright,
                quick_open_empty_height_px: 48.0,
                quick_open_empty_font_size_px: ui_font_size_xs_px,
                quick_open_empty_text_color: menu_theme.text_secondary,
            },
            settings_panel: SettingsPanelStyles {
                settings_title_version_gap_px: 10.0,
                settings_title_version_font_size_px: ui_font_size_sm_px,
                settings_title_version_text_color: settings_secondary_text_color,
                settings_title_version_margin_bottom_px: 5.0,
                settings_primary_text_color,
                settings_secondary_text_color,
                settings_header_gap_px: 12.0,
                settings_header_actions_gap_px: 8.0,
                settings_header_button_max_width_px: 150.0,
                settings_site_button_height_px: 30.0,
                settings_site_button_padding_x_px: 12.0,
                settings_site_button_radius_px: 8.0,
                settings_site_button_border_width_px: 1.0,
                settings_site_button_gap_px: 6.0,
                settings_site_button_icon_size_px: 15.0,
                settings_site_button_font_size_px: ui_font_size_sm_px,
                settings_site_button_text_color: settings_theme.text_primary,
                settings_site_button_icon_color: settings_theme.text_secondary,
                settings_site_button_update_text_color: settings_update_color,
                settings_site_button_update_icon_color: settings_update_color,
                settings_site_button_bg: settings_theme.bg_secondary,
                settings_site_button_hover_bg: settings_theme.bg_hover,
                settings_site_button_border_color: settings_theme.border_primary,
                settings_option_button_height_px: 28.0,
                settings_option_button_padding_x_px: 12.0,
                settings_option_button_radius_px: 999.0,
                settings_option_button_border_width_px: 1.0,
                settings_option_button_gap_px: 8.0,
                settings_option_button_font_size_px: ui_font_size_sm_px,
                // Match the settings text-input text color so picker trigger
                // buttons don't read lighter than typed values.
                settings_option_button_text_color: settings_theme.text_bright,
                settings_option_button_active_text_color: settings_active_text_color,
                settings_option_button_bg: settings_theme.bg_secondary,
                settings_option_button_hover_bg: settings_theme.bg_hover,
                settings_option_button_active_bg: settings_active_bg,
                settings_option_button_border_color: settings_theme.border_primary,
                settings_option_button_active_border_color: settings_active_bg,
                settings_contrast_border_color: settings_theme.border_primary,
                settings_input_height_px: 30.0,
                settings_input_radius_px: 6.0,
                settings_stepper_input_radius_px: 0.0,
                settings_content_width_input_width_px: 60.0,
                settings_stepper_hover_bg: switch_theme.bg_hover,
                settings_theme_group_gap_px: 10.0,
                settings_theme_toggle_height_px: 30.0,
                settings_theme_toggle_padding_x_px: 12.0,
                settings_theme_toggle_gap_px: 6.0,
                settings_theme_toggle_radius_px: 6.0,
                settings_theme_toggle_border_width_px: 1.0,
                settings_theme_toggle_icon_size_px: 15.0,
                settings_theme_option_height_px: 30.0,
                settings_theme_option_padding_x_px: 12.0,
                settings_theme_option_radius_px: 6.0,
                settings_theme_option_border_width_px: 1.0,
                settings_theme_option_font_size_px: ui_font_size_sm_px,
                settings_theme_option_hover_bg: settings_theme.bg_hover,
                settings_theme_option_active_bg: settings_theme.bg_selected,
                settings_theme_option_active_text_color: settings_theme.text_bright,
                settings_theme_option_text_color: settings_theme.text_secondary,
                settings_theme_option_border_color: settings_theme.border_primary,
            },
        };
        styles.apply_ui_color_overrides(styles.ui.spec.colors);
        styles.apply_appearance_corners();
        styles
    }

    fn apply_appearance_corners(&mut self) {
        let factor = self.ui.base_spec.appearance.corners.factor();
        if factor == 1.0 {
            return;
        }
        let radii = [
            &mut self.sidebar.sidebar_item_badge_radius_px,
            &mut self.sidebar.sidebar_count_badge_radius_px,
            &mut self.sidebar.sidebar_edit_input_radius_px,
            &mut self.sidebar.file_list_item_thumbnail_radius_px,
            &mut self.utility_panel.utility_panel_button_radius_px,
            &mut self.toolbar.toolbar_btn_radius_px,
            &mut self.format_toolbar.format_toolbar_radius_px,
            &mut self.tooltip.tooltip_radius_px,
            &mut self.tooltip.tooltip_shortcut_key_radius_px,
            &mut self.dialog.dialog_radius_px,
            &mut self.dialog.dialog_btn_radius_px,
            &mut self.notification.notification_radius_px,
            &mut self.context_menu.menu_item_radius_px,
            &mut self.context_menu.menu_shortcut_key_radius_px,
            &mut self.text_input.text_input_radius_px,
            &mut self.search.search_bar_radius_px,
            &mut self.search.search_option_btn_radius_px,
            &mut self.workspace_search.ws_search_input_radius_px,
            &mut self.settings_panel.settings_site_button_radius_px,
            &mut self.settings_panel.settings_option_button_radius_px,
            &mut self.settings_panel.settings_input_radius_px,
            &mut self.settings_panel.settings_stepper_input_radius_px,
            &mut self.settings_panel.settings_theme_toggle_radius_px,
            &mut self.settings_panel.settings_theme_option_radius_px,
        ];
        for radius in radii {
            if *radius < 100.0 {
                *radius *= factor;
            }
        }
        let menu_factor = self.ui.base_spec.appearance.corners.menu_factor();
        self.context_menu.menu_radius_px *= menu_factor;
        self.quick_open.quick_open_panel_radius_px *= menu_factor;
        if self.ui.base_spec.appearance.corners == UiCorners::Full {
            self.context_menu.menu_padding_y_px = self
                .context_menu
                .menu_padding_y_px
                .max(self.context_menu.menu_radius_px);
            for (radius, height) in [
                (
                    &mut self.text_input.text_input_radius_px,
                    self.text_input.text_input_height_px,
                ),
                (
                    &mut self.settings_panel.settings_input_radius_px,
                    self.settings_panel.settings_input_height_px,
                ),
                (
                    &mut self.workspace_search.ws_search_input_radius_px,
                    self.workspace_search.ws_search_input_height_px,
                ),
                (
                    &mut self.settings_panel.settings_option_button_radius_px,
                    self.settings_panel.settings_option_button_height_px,
                ),
                (
                    &mut self.settings_panel.settings_site_button_radius_px,
                    self.settings_panel.settings_site_button_height_px,
                ),
                (
                    &mut self.settings_panel.settings_theme_toggle_radius_px,
                    self.settings_panel.settings_theme_toggle_height_px,
                ),
                (
                    &mut self.settings_panel.settings_theme_option_radius_px,
                    self.settings_panel.settings_theme_option_height_px,
                ),
                (
                    &mut self.search.search_option_btn_radius_px,
                    self.search.search_option_btn_size_px,
                ),
            ] {
                if *radius > 0.0 {
                    *radius = height * 0.5;
                }
            }
        }
        self.text_input.menu = self.context_menu.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::Styles;
    use crate::theme::{EditorThemeMode, Theme, ThemeKind, apply_editor_theme_mode, with_alpha};

    #[test]
    fn sidebar_filter_uses_muted_prompt_and_sidebar_warning_match_color() {
        for kind in ThemeKind::light_themes()
            .iter()
            .chain(ThemeKind::dark_themes())
        {
            let theme = kind.build();
            for dark_sidebar in [false, true] {
                let mut styles = Styles::from_theme_with_options(&theme, &theme, dark_sidebar);
                let sidebar_theme = if dark_sidebar && theme.bg_primary.l >= 0.5 {
                    ThemeKind::OneDark.build()
                } else {
                    theme.with_light_surface_contrast()
                };
                assert_eq!(
                    styles.sidebar.sidebar_filter_match_color,
                    sidebar_theme.text_warning
                );
                assert_eq!(
                    styles.sidebar.sidebar_filter_muted_color,
                    with_alpha(sidebar_theme.text_secondary, 0.65)
                );
                let custom = ThemeKind::PaperWhite.build().text_warning;
                styles.sidebar.sidebar_filter_muted_color = custom;
                let input = styles.input_styles(super::InputRole::SidebarFilter, 1.5);
                assert_eq!(input.text_input_placeholder_color, custom);
                assert_eq!(input.text_input_focus_placeholder_color, Some(custom));
            }
        }
    }

    #[test]
    fn attachment_template_help_uses_settings_typography_and_semantic_errors() {
        let theme = ThemeKind::OneDark.build();
        let mut styles = Styles::from_theme_with_options(&theme, &theme, false);
        styles.settings_panel.settings_option_button_font_size_px = 19.0;
        styles.settings_panel.settings_secondary_text_color = theme.link;
        styles.ui.colors.danger = theme.text_warning;
        assert_ne!(theme.link, theme.text_warning);
        for invalid in [false, true] {
            let help = styles.settings_template_help(invalid);
            assert_eq!(help.size_px, 19.0);
            assert!(help.overflow == super::TextOverflow::Wrap);
            assert_eq!(
                help.color,
                if invalid {
                    styles.ui.colors.danger
                } else {
                    styles.settings_panel.settings_secondary_text_color
                },
            );
        }
    }

    #[test]
    fn workspace_root_text_uses_sidebar_colors_instead_of_canvas_inheritance() {
        for kind in [ThemeKind::OneDark, ThemeKind::PaperWhite] {
            let theme = kind.build();
            let mut styles = Styles::from_theme_with_options(&theme, &theme, false);
            styles.sidebar.sidebar_item_font_size_px = 19.0;
            styles.sidebar.sidebar_item_text_color = theme.link;
            styles.sidebar.sidebar_item_selected_text_color = theme.text_warning;
            assert_ne!(theme.link, theme.text_warning);
            assert_eq!(styles.workspace_root_label(false).color, theme.link);
            assert_eq!(styles.workspace_root_label(true).color, theme.text_warning);
            assert_eq!(styles.workspace_root_label(false).size_px, 19.0);
            assert_eq!(styles.workspace_root_label(true).size_px, 19.0);
            styles.sidebar.sidebar_resize_indicator_color = theme.link;
            styles.sidebar.sidebar_resize_indicator_width_px = 2.5;
            let separator = styles.workspace_roots_separator();
            assert_eq!(separator.color, theme.link);
            assert_eq!(separator.thickness_px, 2.5);
            assert!(!separator.scale_thickness);
        }
    }

    #[test]
    fn line_diff_highlights_are_subdued_and_restore_text_brightens_on_hover() {
        for kind in ThemeKind::light_themes()
            .iter()
            .chain(ThemeKind::dark_themes())
        {
            let theme = kind.build();
            let styles = Styles::from_theme_with_options(&theme, &theme, false).line_diff;
            for color in [
                styles.added_text_background_color,
                styles.modified_text_background_color,
                styles.deleted_text_background_color,
            ] {
                assert_eq!(color.a, 0.48);
            }
            for color in [
                styles.added_background_color,
                styles.modified_background_color,
                styles.deleted_background_color,
            ] {
                assert_eq!(color.a, 0.02);
            }
            assert_eq!(styles.restore_text_color.a, 0.65);
            assert_eq!(styles.restore_hover_text_color, theme.text_primary);
            assert!(styles.restore_hover_text_color.a > styles.restore_text_color.a);
            assert_eq!(styles.restore_inset_px, 12.0);
            assert!(styles.restore_inset_px >= styles.gutter_width_px + 4.0);
            assert_ne!(styles.restore_text_color, styles.restore_hover_text_color);
        }
    }

    fn hover_surface_contrast(hover: gpui::Hsla, background: gpui::Hsla) -> f32 {
        let fg: gpui::Rgba = hover.into();
        let bg: gpui::Rgba = background.into();
        let luminance = |r: f32, g: f32, b: f32| {
            let linear = |v: f32| {
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
        };
        let composite = luminance(
            fg.r * fg.a + bg.r * (1.0 - fg.a),
            fg.g * fg.a + bg.g * (1.0 - fg.a),
            fg.b * fg.a + bg.b * (1.0 - fg.a),
        );
        let base = luminance(bg.r, bg.g, bg.b);
        (composite.max(base) + 0.05) / (composite.min(base) + 0.05)
    }

    #[test]
    fn search_and_inactive_tab_hover_are_visible_on_actual_surfaces() {
        for kind in ThemeKind::light_themes()
            .iter()
            .chain(ThemeKind::dark_themes())
        {
            let theme = kind.build();
            for dark_sidebar in [false, true] {
                let styles = Styles::from_theme_with_options(&theme, &theme, dark_sidebar);
                for (hover, background) in [
                    (
                        styles.search_toggle().hover.background,
                        styles.search.search_bar_bg,
                    ),
                    (
                        styles.search_navigation(false).hover.background,
                        styles.search.search_bar_bg,
                    ),
                    (
                        styles.search_navigation(true).hover.background,
                        styles.search.search_bar_bg,
                    ),
                    (
                        styles.workspace_toggle().hover.background,
                        styles.workspace_search.ws_search_input_hover_bg,
                    ),
                    (
                        styles.workspace_toggle().hover.background,
                        styles.workspace_search.ws_search_input_focus_bg,
                    ),
                    (
                        styles.tab_close(false, false).hover.background,
                        styles.tab_bar.tab_item_hover_bg,
                    ),
                ] {
                    let ratio = hover_surface_contrast(hover, background);
                    assert!(
                        ratio >= 1.08,
                        "{} dark_sidebar={dark_sidebar} ratio={ratio}",
                        kind.label()
                    );
                }
                assert_eq!(styles.tab_close(false, false).hover.opacity, 1.0);
                assert_eq!(styles.tab_close(false, false).normal.opacity, 0.0);
                assert!(styles.checkbox.checkbox_hover_bg.a > 0.0);
                assert!(styles.checkbox.checkbox_hover_bg.a < 0.2);
            }
        }
    }

    #[test]
    fn folded_heading_indicator_is_subtle_and_has_extra_spacing() {
        for kind in ThemeKind::light_themes()
            .iter()
            .chain(ThemeKind::dark_themes())
        {
            let theme = kind.build();
            let styles = Styles::from_theme(&theme, &theme);
            assert_eq!(styles.self_drawn.folded_heading_indicator_gap_px, 8.0);
            assert_eq!(
                styles.self_drawn.colors.folded_heading_indicator_color,
                with_alpha(theme.text_bright, 0.55)
            );
        }
    }

    #[test]
    fn multi_cursor_selection_uses_search_highlight_hue_across_themes() {
        for kind in ThemeKind::light_themes()
            .iter()
            .chain(ThemeKind::dark_themes())
        {
            let theme = kind.build();
            let styles = Styles::from_theme(&theme, &theme);
            assert_eq!(styles.document_selection_color(false), theme.selection);
            assert_eq!(
                styles.document_selection_color(true),
                with_alpha(theme.highlight_bg, 0.62)
            );
            assert_eq!(
                styles.document_selection_color(true).h,
                styles.search.search_match_bg.h
            );
        }
    }

    #[test]
    fn inline_code_background_preserves_opaque_theme_token() {
        for kind in ThemeKind::dark_themes()
            .iter()
            .chain(ThemeKind::light_themes())
        {
            let base = kind.build();
            for &mode in EditorThemeMode::all() {
                let theme = apply_editor_theme_mode(&base, mode, kind.is_dark());
                for sidebar_always_dark in [false, true] {
                    let styles =
                        Styles::from_theme_with_options(&theme, &base, sidebar_always_dark);
                    assert_eq!(
                        styles.editor.editor_inline_code_bg,
                        theme.inline_code_bg,
                        "{} {mode:?}, dark sidebar: {sidebar_always_dark}",
                        kind.label()
                    );
                    assert_eq!(styles.editor.editor_inline_code_bg.a, 1.0);
                }
            }
        }
    }

    #[test]
    fn code_block_border_preserves_theme_token_and_alpha() {
        for kind in ThemeKind::dark_themes()
            .iter()
            .chain(ThemeKind::light_themes())
        {
            let base = kind.build();
            for alpha in [base.border_code_block.a, 0.23] {
                let mut theme = base.clone();
                theme.border_code_block = with_alpha(base.border_code_block, alpha);
                for sidebar_always_dark in [false, true] {
                    let styles =
                        Styles::from_theme_with_options(&theme, &base, sidebar_always_dark);
                    assert_eq!(
                        styles.code_block.code_block_border_color,
                        theme.border_code_block,
                        "{} alpha: {alpha}, dark sidebar: {sidebar_always_dark}",
                        kind.label()
                    );
                }
            }
        }
    }

    #[test]
    fn file_tree_and_file_list_share_the_same_background() {
        let theme = Theme::one_dark();
        let styles = Styles::from_theme(&theme, &theme);

        assert_eq!(styles.sidebar.sidebar_bg, styles.sidebar.file_list_bg);
        assert_eq!(styles.sidebar.sidebar_bg, styles.sidebar.file_list_item_bg);
        assert_eq!(styles.sidebar.file_list_item_thumbnail_border_width_px, 0.0);
    }

    #[test]
    fn light_editor_can_keep_file_tree_and_file_list_dark() {
        let light_theme = Theme::one_light();
        let styles = Styles::from_theme_with_options(&light_theme, &light_theme, true);

        assert!(styles.sidebar.sidebar_bg.l < 0.5);
        assert!(styles.sidebar.file_list_bg.l < 0.5);
        assert!(styles.sidebar.file_list_item_bg.l < 0.5);
    }

    #[test]
    fn context_menus_follow_theme_and_elevation_shadow_policy() {
        for kind in ThemeKind::light_themes()
            .iter()
            .chain(ThemeKind::dark_themes())
        {
            let theme = kind.build();
            for elevation in [false, true] {
                let mut spec = super::UiStyleSpec::default();
                spec.appearance.elevation = elevation;
                let styles = Styles::from_theme_with_spec(&theme, &theme, false, spec);
                let expected_shadow = elevation || theme.bg_primary.l < 0.5;
                assert_eq!(
                    !styles.context_menu.surface().shadow.layers.is_empty(),
                    expected_shadow
                );
                assert_eq!(
                    !styles.text_input.menu.surface().shadow.layers.is_empty(),
                    expected_shadow
                );
                assert!(!styles.search_menu_surface().shadow.layers.is_empty());
            }
        }
    }

    #[test]
    fn editor_body_is_one_point_smaller_without_changing_heading_sizes() {
        let theme = Theme::one_dark();
        let styles = Styles::from_theme(&theme, &theme);

        assert_eq!(styles.editor.editor_font_size_px, 14.0);
        assert_eq!(styles.editor.editor_h1_font_size_px, 21.0);
        assert_eq!(styles.editor.editor_h2_font_size_px, 19.5);
        assert_eq!(styles.editor.editor_h6_font_size_px, 15.0);
    }

    #[test]
    fn quote_block_padding_matches_code_block_left_padding() {
        let theme = Theme::one_dark();
        let styles = Styles::from_theme(&theme, &theme);

        assert_eq!(styles.quote_block.quote_block_border_width_px, 4.0);
        assert_eq!(styles.quote_block.quote_block_padding_top_px, 0.0);
        assert_eq!(styles.quote_block.quote_block_padding_bottom_px, 0.0);
        assert_eq!(styles.quote_block.quote_block_padding_left_px, 12.0);
        assert_eq!(styles.quote_block.quote_block_padding_right_px, 10.0);
    }

    #[test]
    fn preview_borders_use_primary_theme_border_without_accent_glow() {
        let theme = Theme::one_dark();
        let styles = Styles::from_theme(&theme, &theme);

        assert_eq!(
            styles.image_preview.image_active_border_color,
            theme.border_primary
        );
        assert_eq!(
            styles
                .self_drawn
                .colors
                .generated_preview_action_border_color,
            crate::theme::with_alpha(theme.border_primary, 0.38)
        );
        assert!(
            styles
                .self_drawn
                .colors
                .generated_preview_action_border_color
                .a
                < 0.5
        );
    }

    #[test]
    fn utility_panel_colors_follow_the_active_light_theme() {
        let light_theme = Theme::one_light();
        let dark_ui_theme = Theme::one_dark();
        let styles = Styles::from_theme(&light_theme, &dark_ui_theme);
        let panel = &styles.utility_panel;
        // Light palettes get their muted foregrounds and hairlines deepened
        // before any token is derived from them.
        let light_theme = light_theme.with_light_surface_contrast();

        assert_eq!(panel.utility_panel_bg, light_theme.bg_secondary);
        assert_eq!(panel.utility_panel_border_color, light_theme.border_primary);
        assert_eq!(
            panel.utility_panel_header_text_color,
            light_theme.text_primary
        );
        assert_eq!(
            panel.utility_panel_item_label_text_color,
            light_theme.text_secondary
        );
        assert_eq!(
            panel.utility_panel_item_value_text_color,
            light_theme.text_primary
        );
        assert_eq!(
            panel.utility_panel_button_border_color,
            light_theme.border_primary
        );
        assert_eq!(
            panel.utility_panel_button_text_color,
            light_theme.text_primary
        );
        assert_eq!(panel.utility_panel_button_hover_bg, light_theme.bg_hover);
        assert_eq!(
            panel.utility_panel_resize_indicator_color,
            light_theme.border_primary
        );
        assert_eq!(
            panel.utility_panel_resize_active_indicator_color,
            light_theme.text_accent
        );
        assert_eq!(
            styles.outline.outline_rail_label_color,
            light_theme.text_secondary
        );
        assert_eq!(
            styles.outline.outline_rail_label_hover_color,
            light_theme.text_bright
        );
        assert_eq!(
            styles.outline.outline_rail_label_active_color,
            light_theme.text_bright
        );
        assert_eq!(
            styles.outline.outline_rail_tick_color,
            crate::theme::with_alpha(light_theme.text_primary, 0.3)
        );
    }

    #[test]
    fn visible_component_colors_come_from_theme_tokens() {
        let source = include_str!("style.rs")
            .replace(concat!("crate::hex_", "hsla!(\"#ffffff\")"), "")
            .replace(concat!("crate::hex_", "hsla!(\"#d9dee3\")"), "");
        let forbidden = [
            concat!("hex_", "hsla!"),
            concat!("hex_to_", "hsla("),
            concat!("gpui::", "hsla("),
            concat!("gpui::", "rgb("),
            concat!("Hsla::", "black("),
            concat!("Hsla::", "white("),
            concat!("gpui::", "white("),
            concat!("color.", "l ="),
        ];

        for pattern in forbidden {
            assert!(
                !source.contains(pattern),
                "visible component color must use a Theme token: {pattern}"
            );
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PanelSide {
    Left,
    Right,
}
#[derive(Clone, Copy)]
pub struct PanelStyle {
    pub background: Hsla,
    pub border_color: Hsla,
    pub border_width_px: f32,
    pub side: PanelSide,
}
#[derive(Clone, Copy)]
pub struct PanelHeaderStyle {
    pub height_px: f32,
    pub padding_x_px: f32,
    pub gap_px: f32,
    pub text: TextStyle,
}
#[derive(Clone, Copy)]
pub struct DetailRowStyle {
    pub min_height_px: f32,
    pub padding_x_px: f32,
    pub gap_px: f32,
    pub font_size_px: f32,
    pub align_start: bool,
    pub hover_background: Option<Hsla>,
}
#[derive(Clone, Copy)]
pub struct ThemeCardStyle {
    pub padding_px: f32,
    pub gap_px: f32,
    pub radius_px: f32,
    pub border_width_px: f32,
    pub background: Hsla,
    pub elevation: Option<ElevationStyle>,
    pub border_color: Hsla,
    pub hover_border_color: Hsla,
    pub swatches: [Hsla; 5],
    pub swatch_size_px: f32,
    pub swatch_gap_px: f32,
    pub title: TextStyle,
}
#[derive(Clone, Copy)]
pub struct InputFrameStyle {
    pub height_px: f32,
    pub radius_px: f32,
    pub border_width_px: f32,
    pub border_color: Hsla,
    pub background: Hsla,
}
#[derive(Clone, Copy)]
pub struct StatusBarStyle {
    pub height_px: f32,
    pub font_size_px: f32,
    pub padding_x_px: f32,
    pub item_gap_px: f32,
    pub icon_size_px: f32,
    pub border_width_px: f32,
    pub border: Hsla,
    pub background: Hsla,
    pub title: Hsla,
    pub value: Hsla,
    pub hover: Hsla,
}
#[derive(Clone, Copy)]
pub struct TabSurfaceStyle {
    pub padding_left_px: f32,
    pub padding_right_px: f32,
    pub gap_px: f32,
    pub background: Hsla,
    pub active_background: Hsla,
    pub hover_background: Hsla,
    pub border_width_px: f32,
    pub border_color: Hsla,
}
impl UtilityPanelStyles {
    pub fn panel(&self) -> PanelStyle {
        PanelStyle {
            background: self.utility_panel_bg,
            border_color: self.utility_panel_border_color,
            border_width_px: self.utility_panel_border_width_px,
            side: PanelSide::Left,
        }
    }

    pub fn header(&self) -> PanelHeaderStyle {
        PanelHeaderStyle {
            height_px: self.utility_panel_header_height_px,
            padding_x_px: self.utility_panel_header_padding_x_px,
            gap_px: 6.0,
            text: TextStyle::new(
                self.utility_panel_header_font_size_px,
                self.utility_panel_header_text_color,
            ),
        }
    }
    pub fn detail_row(&self, interactive: bool, align_start: bool) -> DetailRowStyle {
        DetailRowStyle {
            min_height_px: self.utility_panel_item_height_px,
            padding_x_px: self.utility_panel_section_padding_x_px,
            gap_px: self.utility_panel_item_gap_px,
            font_size_px: self.utility_panel_item_font_size_px,
            align_start,
            hover_background: interactive.then_some(self.utility_panel_button_hover_bg),
        }
    }
    pub fn empty_text(&self) -> TextStyle {
        TextStyle::new(
            self.utility_panel_empty_font_size_px,
            self.utility_panel_empty_text_color,
        )
    }
}
impl Styles {
    pub fn utility_export_copy_button(&self) -> ControlStyle {
        let mut style = self.utility_button();
        style.min_width_px = 0.0;
        style.padding_x_px = self.utility_panel.utility_panel_button_padding_x_px;
        style.selected = style.normal;
        style.normal.foreground.a *= 0.65;
        style.selected_hover = style.hover;
        style.selected_hover.foreground = style.selected.foreground;
        style
    }

    pub fn utility_button(&self) -> ControlStyle {
        let s = &self.utility_panel;
        let mut c = self.ui.control;
        c.height_px = s.utility_panel_button_height_px;
        c.padding_x_px = s.utility_panel_button_padding_x_px;
        c.radius_px = s.utility_panel_button_radius_px;
        c.border_width_px = s.utility_panel_button_border_width_px;
        c.font_size_px = s.utility_panel_item_font_size_px;
        c.disabled_opacity = 0.45;
        c.normal.foreground = s.utility_panel_button_text_color;
        c.normal.border = s.utility_panel_button_border_color;
        c.hover = ControlColors {
            background: s.utility_panel_button_hover_bg,
            ..c.normal
        };
        c.pressed = c.hover;
        c
    }
    pub fn settings_input_frame(&self) -> InputFrameStyle {
        let s = &self.settings_panel;
        InputFrameStyle {
            height_px: s.settings_input_height_px,
            radius_px: s.settings_input_radius_px,
            border_width_px: s.settings_option_button_border_width_px,
            border_color: self
                .ui
                .spec
                .colors
                .get(crate::theme::UiColor::Input)
                .unwrap_or(s.settings_option_button_border_color),
            background: s.settings_option_button_bg,
        }
    }
    pub fn theme_card(&self, preview: &Theme, active: bool) -> ThemeCardStyle {
        let accent = self.ui.colors.accent;
        ThemeCardStyle {
            padding_px: self.ui.spec.application.theme_card_padding_px,
            gap_px: self.ui.spec.application.theme_card_gap_px,
            radius_px: self.ui.spec.application.theme_card_radius_px,
            border_width_px: self.ui.spec.application.theme_card_border_width_px,
            background: self
                .ui
                .spec
                .colors
                .get(crate::theme::UiColor::Card)
                .unwrap_or(preview.bg_primary),
            elevation: self
                .ui
                .spec
                .appearance
                .elevation
                .then(|| ElevationStyle::raised(self.window.window_bg.l < 0.5)),
            border_color: if active {
                accent
            } else {
                preview.border_secondary
            },
            hover_border_color: if active {
                accent
            } else {
                preview.border_tertiary
            },
            swatches: [
                preview.inline_code,
                preview.heading,
                preview.text_accent,
                preview.text_primary,
                preview.bg_secondary,
            ],
            swatch_size_px: self.ui.spec.application.theme_swatch_size_px,
            swatch_gap_px: self.ui.spec.application.theme_swatch_gap_px,
            title: TextStyle {
                overflow: TextOverflow::Ellipsis,
                ..TextStyle::new(
                    self.ui.spec.application.theme_title_size_px,
                    self.ui
                        .spec
                        .colors
                        .get(crate::theme::UiColor::CardForeground)
                        .unwrap_or(preview.text_primary),
                )
            },
        }
    }
    pub fn status_bar(&self) -> StatusBarStyle {
        let c = self.ui.colors;
        StatusBarStyle {
            height_px: self.ui.spec.application.status_height_px,
            font_size_px: self.ui.spec.application.status_font_size_px,
            padding_x_px: self.ui.spec.application.status_padding_x_px,
            item_gap_px: self.ui.spec.application.status_gap_px,
            icon_size_px: self.ui.spec.application.status_icon_size_px,
            border_width_px: self.tab_bar.tab_bar_border_width_px,
            border: self.tab_bar.tab_bar_border_color,
            background: c.status_background,
            title: c.status_title,
            value: c.status_value,
            hover: c.status_hover,
        }
    }
    pub fn status_button(&self) -> ControlStyle {
        let s = self.status_bar();
        let mut c = self.ui.control;
        c.height_px = s.height_px;
        c.padding_x_px = s.padding_x_px;
        c.radius_px = 0.0;
        c.gap_px = s.item_gap_px;
        c.font_size_px = s.font_size_px;
        c.icon_size_px = s.icon_size_px;
        c.normal.foreground = s.title;
        c.hover = ControlColors {
            background: s.hover,
            ..c.normal
        };
        c.pressed = c.hover;
        c
    }
    pub fn tab_surface(&self) -> TabSurfaceStyle {
        let s = &self.tab_bar;
        TabSurfaceStyle {
            padding_left_px: s.tab_item_padding_left_px,
            padding_right_px: s.tab_item_padding_right_px,
            gap_px: s.tab_item_gap_px,
            background: s.tab_item_bg,
            active_background: s.tab_item_active_bg,
            hover_background: s.tab_item_hover_bg,
            border_width_px: s.tab_item_border_width_px,
            border_color: s.tab_item_border_color,
        }
    }
}

// Shared self-drawn application control metrics, in unscaled logical pixels.
#[derive(Clone, Copy)]
pub struct ScrollbarStyleSpec {
    pub top_margin_px: f32,
    pub bottom_margin_px: f32,
    pub right_margin_px: f32,
    pub width_px: f32,
    pub hover_width_px: f32,
    pub hit_width_px: f32,
    pub min_thumb_height_px: f32,
    pub radius_px: f32,
    pub idle_opacity: f32,
    pub opacity: f32,
    pub hover_opacity: f32,
    pub animation_frame_ms: u64,
    pub fade_animation_ms: u64,
    pub width_animation_ms: u64,
    pub idle_delay_ms: u64,
    pub hover_reveal_delay_ms: u64,
}
impl Default for ScrollbarStyleSpec {
    fn default() -> Self {
        Self {
            top_margin_px: 3.0,
            bottom_margin_px: 3.0,
            right_margin_px: 3.0,
            width_px: 6.0,
            hover_width_px: 6.0,
            hit_width_px: 8.0,
            min_thumb_height_px: 36.0,
            radius_px: 100.0,
            idle_opacity: 0.0,
            opacity: 0.2,
            hover_opacity: 0.3,
            animation_frame_ms: 8,
            fade_animation_ms: 80,
            width_animation_ms: 40,
            idle_delay_ms: 800,
            hover_reveal_delay_ms: 100,
        }
    }
}
#[derive(Clone, Copy)]
pub struct SelfDrawnSpec {
    pub code_block_horizontal_scrollbar_margin_x_px: f32,
    pub front_matter_list_delete_gap_px: f32,
    pub front_matter_list_delete_width_px: f32,
    pub front_matter_list_item_gap_px: f32,
    pub front_matter_type_opacity: f32,
    pub line_toolbar_viewport_margin_px: f32,
    pub line_toolbar_trigger_gap_px: f32,

    pub scrollbar: ScrollbarStyleSpec,
    pub code_block_copy_button_top_px: f32,
    pub code_block_copy_button_right_px: f32,
    pub code_block_copy_button_padding_x_px: f32,
    pub code_block_copy_button_padding_y_px: f32,
    pub code_block_copy_button_radius_px: f32,
    pub code_block_copy_button_border_width_px: f32,
    pub code_block_copy_button_font_size_px: f32,
    pub code_block_display_action_button_size_px: f32,
    pub code_block_display_action_icon_size_px: f32,
    pub code_block_display_action_gap_px: f32,
    pub generated_preview_action_padding_px: f32,
    pub generated_preview_action_inner_padding_x_px: f32,
    pub generated_preview_action_gap_px: f32,
    pub generated_preview_action_button_size_px: f32,
    pub generated_preview_action_icon_size_px: f32,
    pub generated_preview_action_button_radius_px: f32,
    pub generated_preview_action_border_width_px: f32,
    pub table_border_track_width_px: f32,
    pub table_border_drag_threshold_px: f32,
    pub table_drag_indicator_width_px: f32,
    pub gutter_fold_icon_size_px: f32,
    pub folded_heading_indicator_size_px: f32,
    pub folded_heading_indicator_gap_px: f32,
    pub gutter_fold_icon_offset_px: f32,
    pub gutter_fold_hit_padding_px: f32,
    pub gutter_fold_idle_opacity: f32,
    pub gutter_fold_folded_opacity: f32,
    pub gutter_fold_hover_opacity: f32,
    pub gutter_fold_animation_ms: u64,
    pub image_edit_icon_size_px: f32,
    pub image_edit_icon_gap_px: f32,
    pub image_edit_icon_normal_opacity: f32,
    pub image_edit_icon_hover_opacity: f32,
    pub pane_divider_width_px: f32,
    pub pane_divider_hit_width_px: f32,
    pub pane_divider_min_hit_width_px: f32,
    pub preview_overlay_padding_px: f32,
    pub preview_chart_initial_fraction: f32,
    pub preview_click_slop_px: f32,
    pub format_trigger_size_px: f32,
    pub format_trigger_gap_px: f32,
    pub format_trigger_content_gap_px: f32,
    pub format_trigger_hover_opacity: f32,
    pub format_trigger_line_opacity: f32,
    pub format_trigger_animation_step: f32,
    pub format_trigger_animation_frame_ms: u64,
    pub preview_button_line_height_factor: f32,
    pub table_cell_selection_width_px: f32,
    pub line_drag_pointer_offset_px: f32,
    pub line_drag_viewport_margin_px: f32,
    pub line_drag_fallback_height_px: f32,
}
impl Default for SelfDrawnSpec {
    fn default() -> Self {
        Self {
            code_block_horizontal_scrollbar_margin_x_px: 3.0,
            front_matter_list_delete_gap_px: 4.0,
            front_matter_list_delete_width_px: 14.0,
            front_matter_list_item_gap_px: 3.0,
            front_matter_type_opacity: 0.76,
            line_toolbar_viewport_margin_px: 6.0,
            line_toolbar_trigger_gap_px: 0.0,

            scrollbar: ScrollbarStyleSpec::default(),
            code_block_copy_button_top_px: 2.0,
            code_block_copy_button_right_px: -4.0,
            code_block_copy_button_padding_x_px: 10.0,
            code_block_copy_button_padding_y_px: 4.0,
            code_block_copy_button_radius_px: 999.0,
            code_block_copy_button_border_width_px: 1.0,
            code_block_copy_button_font_size_px: 11.0,
            code_block_display_action_button_size_px: 16.0,
            code_block_display_action_icon_size_px: 12.0,
            code_block_display_action_gap_px: 1.0,
            generated_preview_action_padding_px: 9.0,
            generated_preview_action_inner_padding_x_px: 3.0,
            generated_preview_action_gap_px: 3.0,
            generated_preview_action_button_size_px: 22.0,
            generated_preview_action_icon_size_px: 11.0,
            generated_preview_action_button_radius_px: 5.0,
            generated_preview_action_border_width_px: 1.0,
            table_border_track_width_px: 5.0,
            table_border_drag_threshold_px: 4.0,
            table_drag_indicator_width_px: 2.0,
            gutter_fold_icon_size_px: 16.0,
            folded_heading_indicator_size_px: 12.0,
            folded_heading_indicator_gap_px: 8.0,
            gutter_fold_icon_offset_px: 10.0,
            gutter_fold_hit_padding_px: 3.0,
            gutter_fold_idle_opacity: 0.0,
            gutter_fold_folded_opacity: 0.9,
            gutter_fold_hover_opacity: 1.0,
            gutter_fold_animation_ms: 140,
            image_edit_icon_size_px: 13.0,
            image_edit_icon_gap_px: 10.0,
            image_edit_icon_normal_opacity: 0.7,
            image_edit_icon_hover_opacity: 0.55,
            pane_divider_width_px: 1.0,
            pane_divider_hit_width_px: 5.0,
            pane_divider_min_hit_width_px: 6.0,
            preview_overlay_padding_px: 32.0,
            preview_chart_initial_fraction: 0.8,
            preview_click_slop_px: 4.0,
            format_trigger_size_px: 18.0,
            format_trigger_gap_px: 0.0,
            format_trigger_content_gap_px: 6.0,
            format_trigger_hover_opacity: 0.9,
            format_trigger_line_opacity: 0.34,
            format_trigger_animation_step: 0.22,
            format_trigger_animation_frame_ms: 16,
            preview_button_line_height_factor: 1.2,
            table_cell_selection_width_px: 2.0,
            line_drag_pointer_offset_px: 14.0,
            line_drag_viewport_margin_px: 8.0,
            line_drag_fallback_height_px: 80.0,
        }
    }
}
#[derive(Clone, Copy)]
pub struct ScrollbarStyles {
    pub metrics: ScrollbarStyleSpec,
    pub thumb_color: Hsla,
    pub sidebar_thumb_color: Hsla,
}
impl std::ops::Deref for ScrollbarStyles {
    type Target = ScrollbarStyleSpec;
    fn deref(&self) -> &Self::Target {
        &self.metrics
    }
}
impl std::ops::DerefMut for ScrollbarStyles {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.metrics
    }
}
#[derive(Clone, Copy)]
pub struct SelfDrawnStyles {
    pub metrics: SelfDrawnSpec,
    pub scrollbar: ScrollbarStyles,
    pub colors: crate::theme::SelfDrawnPalette,
}
impl std::ops::Deref for SelfDrawnStyles {
    type Target = SelfDrawnSpec;
    fn deref(&self) -> &Self::Target {
        &self.metrics
    }
}
impl SelfDrawnStyles {
    fn new(theme: &Theme, sidebar_theme: &Theme, metrics: SelfDrawnSpec) -> Self {
        let colors = crate::theme::SelfDrawnPalette::new(theme, sidebar_theme);
        Self {
            metrics,
            scrollbar: ScrollbarStyles {
                metrics: metrics.scrollbar,
                thumb_color: colors.editor_scrollbar_thumb_color,
                sidebar_thumb_color: colors.sidebar_scrollbar_thumb_color,
            },
            colors,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct TreeRowState {
    pub selected: bool,
    pub focused: bool,
    pub drag_over: bool,
    pub cut: bool,
    pub resizing: bool,
}
#[derive(Clone, Copy)]
pub struct TreeRowStyle {
    pub height_px: f32,
    pub border_width_px: f32,
    pub selected_background: Hsla,
    pub hover_background: Hsla,
    pub active_border: Hsla,
    pub focus_border: Hsla,
    pub cut_opacity: f32,
    pub selected_inset_px: f32,
    pub selected_radius_px: f32,
    pub selected_elevation: Option<ElevationStyle>,
}
#[derive(Clone, Copy)]
pub struct FileRowStyle {
    pub gap_px: f32,
    pub padding_x_px: f32,
    pub padding_y_px: f32,
    pub separator_width_px: f32,
    pub separator_color: Hsla,
    pub background: Hsla,
    pub selected_background: Hsla,
    pub hover_background: Hsla,
}
impl SidebarStyles {
    pub fn tree_row(&self) -> TreeRowStyle {
        TreeRowStyle {
            height_px: self.sidebar_item_height_px,
            border_width_px: 1.0,
            selected_background: self.sidebar_item_selected_bg,
            hover_background: self.sidebar_item_hover_bg,
            active_border: self.sidebar_item_active_border_color,
            focus_border: self.sidebar_item_focus_border_color,
            cut_opacity: 0.48,
            selected_inset_px: 0.0,
            selected_radius_px: 0.0,
            selected_elevation: None,
        }
    }
    pub fn file_row(&self) -> FileRowStyle {
        FileRowStyle {
            gap_px: self.file_list_item_gap_px,
            padding_x_px: self.file_list_item_padding_x_px,
            padding_y_px: self.file_list_item_padding_y_px,
            separator_width_px: self.file_list_item_separator_width_px,
            separator_color: self.file_list_item_separator_color,
            background: self.file_list_item_bg,
            selected_background: self.file_list_item_selected_bg,
            hover_background: self.file_list_item_hover_bg,
        }
    }
    pub fn panel(&self) -> PanelStyle {
        PanelStyle {
            background: self.sidebar_bg,
            border_width_px: self.sidebar_resize_indicator_width_px,
            border_color: self.sidebar_resize_indicator_color,
            side: PanelSide::Right,
        }
    }
}
impl Styles {
    pub fn sidebar_tree_row(&self) -> TreeRowStyle {
        let mut row = self.sidebar.tree_row();
        let appearance = self.ui.spec.appearance;
        if appearance.elevation || appearance.corners == UiCorners::Full {
            row.selected_inset_px = if appearance.elevation { 4.0 } else { 3.0 };
            row.selected_radius_px = if appearance.corners == UiCorners::Full {
                (row.height_px - 2.0) * 0.5
            } else {
                self.ui.spec.radius_px
            };
        }
        if appearance.elevation {
            row.selected_background = self.sidebar.sidebar_item_raised_bg;
            row.active_border = with_alpha(self.sidebar.sidebar_item_active_border_color, 0.32);
            row.selected_elevation = Some(ElevationStyle::raised(self.sidebar.sidebar_bg.l < 0.5));
        }
        row
    }

    pub fn workspace_root_label(&self, active: bool) -> TextStyle {
        TextStyle::new(
            self.sidebar.sidebar_item_font_size_px,
            if active {
                self.sidebar.sidebar_item_selected_text_color
            } else {
                self.sidebar.sidebar_item_text_color
            },
        )
    }

    pub fn workspace_roots_separator(&self) -> SeparatorStyle {
        SeparatorStyle {
            color: self.sidebar.sidebar_resize_indicator_color,
            thickness_px: self.sidebar.sidebar_resize_indicator_width_px,
            margin_px: 0.0,
            scale_thickness: false,
        }
    }

    pub fn sidebar_count_badge(&self) -> BadgeStyle {
        let s = &self.sidebar;
        BadgeStyle {
            text: TextStyle::new(
                s.sidebar_count_badge_font_size_px,
                s.sidebar_count_badge_text_color,
            ),
            padding_x_px: s.sidebar_count_badge_padding_x_px,
            padding_y_px: 0.0,
            radius_px: s.sidebar_count_badge_radius_px,
            border_width_px: 0.0,
            border_color: gpui::transparent_black(),
            background: s.sidebar_count_badge_bg,
        }
    }
    pub fn sidebar_kind_badge(&self, selected: bool) -> BadgeStyle {
        let s = &self.sidebar;
        BadgeStyle {
            text: TextStyle {
                weight: FontWeight::EXTRA_BOLD,
                ..TextStyle::new(
                    s.sidebar_item_badge_font_size_px,
                    if selected {
                        self.ui
                            .spec
                            .colors
                            .get(crate::theme::UiColor::SidebarPrimaryForeground)
                            .unwrap_or(s.sidebar_item_selected_bg)
                    } else {
                        s.sidebar_item_badge_text_color
                    },
                )
            },
            padding_x_px: s.sidebar_item_badge_padding_x_px,
            padding_y_px: 0.0,
            radius_px: s.sidebar_item_badge_radius_px,
            border_width_px: 0.0,
            border_color: gpui::transparent_black(),
            background: if selected {
                self.ui
                    .spec
                    .colors
                    .get(crate::theme::UiColor::SidebarPrimary)
                    .unwrap_or(s.sidebar_item_selected_icon_color)
            } else {
                s.sidebar_item_badge_bg
            },
        }
    }
    pub fn sidebar_header_action(&self) -> ControlStyle {
        let s = &self.sidebar;
        let mut c = self.ui.control;
        c.height_px = s.sidebar_header_action_btn_size_px;
        c.padding_x_px = 0.0;
        c.radius_px = 0.0;
        c.icon_size_px = s.sidebar_header_action_icon_size_px;
        c.normal.foreground = s.sidebar_header_icon_color;
        c.normal.opacity = s.sidebar_header_action_opacity;
        c.hover = ControlColors {
            opacity: s.sidebar_header_action_hover_opacity,
            ..c.normal
        };
        c.pressed = c.hover;
        c
    }
    pub fn sidebar_top_action(&self, active: bool) -> ControlStyle {
        let s = &self.sidebar;
        let mut c = self.ui.control;
        c.height_px = s.sidebar_top_button_height_px;
        c.padding_x_px = 0.0;
        c.radius_px = 0.0;
        c.icon_size_px = s.sidebar_top_button_icon_size_px;
        c.normal.foreground = if active {
            s.sidebar_top_button_active_color
        } else {
            s.sidebar_top_button_inactive_color
        };
        c.hover = c.normal;
        c.pressed = c.normal;
        c
    }
    pub fn sidebar_visibility_button(&self, over_sidebar: bool) -> ControlStyle {
        let mut c = self.ui.control;
        c.height_px = self.tab_bar.tab_history_btn_size_px;
        c.min_width_px = c.height_px;
        c.icon_size_px = self.sidebar.sidebar_top_button_icon_size_px;
        c.padding_x_px = 0.0;
        c.radius_px = 0.0;
        c.border_width_px = 0.0;
        c.elevation = None;
        c.normal.background = gpui::transparent_black();
        c.normal.border = gpui::transparent_black();
        if over_sidebar {
            c.normal.foreground = self.sidebar.sidebar_top_button_active_color;
        } else {
            c.normal.foreground = self.tab_bar.tab_history_btn_icon_color;
        }
        c.hover = ControlColors {
            foreground: if over_sidebar {
                c.normal.foreground
            } else {
                self.tab_bar.tab_history_btn_hover_icon_color
            },
            ..c.normal
        };
        c.pressed = c.normal;
        c
    }
    pub fn workspace_toggle(&self) -> ControlStyle {
        let s = &self.workspace_search;
        let mut c = self.ui.toggle;
        c.height_px = s.ws_search_btn_size_px;
        c.padding_x_px = 0.0;
        c.radius_px = self.ui.spec.application.workspace_toggle_radius_px;
        c.icon_size_px = s.ws_search_btn_icon_size_px;
        c.normal.foreground = s.ws_search_btn_icon_color;
        c.hover = ControlColors {
            background: s.ws_search_btn_hover_bg,
            ..c.normal
        };
        c.selected = ControlColors {
            background: s.ws_search_btn_active_bg,
            foreground: s.ws_search_btn_active_icon_color,
            ..c.normal
        };
        c.selected_hover = ControlColors {
            background: s.ws_search_btn_hover_bg,
            ..c.selected
        };
        c.pressed = c.hover;
        c
    }

    pub fn workspace_close_search_button(&self) -> ControlStyle {
        let mut control = self.workspace_toggle();
        if self.ui.spec.appearance.corners != UiCorners::Square {
            control.radius_px = control.height_px * 0.5;
        }
        control
    }
    pub fn search_navigation(&self, close: bool) -> ControlStyle {
        let s = &self.search;
        let mut c = self.search_toggle();
        c.height_px = if close {
            s.search_close_btn_size_px
        } else {
            s.search_nav_btn_size_px
        };
        if close && self.ui.spec.appearance.corners != UiCorners::Square {
            c.radius_px = c.height_px * 0.5;
        }
        c.icon_size_px = if close {
            s.search_option_btn_icon_size_px
        } else {
            s.search_nav_btn_icon_size_px
        };
        c.normal.foreground = if close {
            s.search_close_btn_icon_color
        } else {
            s.search_nav_btn_icon_color
        };
        c.hover = ControlColors {
            background: if close {
                s.search_close_btn_hover_bg
            } else {
                s.search_nav_btn_hover_bg
            },
            ..c.normal
        };
        c.disabled_opacity = 0.35;
        c.pressed = c.hover;
        c
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeVariant {
    Default,
    Secondary,
    Outline,
    Destructive,
    Ghost,
    Link,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToggleVariant {
    Default,
    Outline,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToggleSize {
    Default,
    Sm,
    Lg,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchSize {
    Default,
    Sm,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuEmphasis {
    Subtle,
    Bold,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuSurface {
    Default,
    Inverted,
}
#[derive(Clone, Copy)]
pub struct ToggleGroupStyle {
    pub alignment: GroupAlignment,
    pub orientation: Orientation,
    pub gap_px: f32,
    /// Muted track holding raised segments; `None` keeps joined segments.
    pub tray: Option<SegmentTrayStyle>,
}
#[derive(Clone, Copy)]
pub struct SegmentTrayStyle {
    pub background: Hsla,
    pub padding_px: f32,
    pub radius_px: f32,
}
impl UiStyleSpec {
    pub fn toggle(&self, theme: &Theme, variant: ToggleVariant, size: ToggleSize) -> ControlStyle {
        let size = match size {
            ToggleSize::Default => ButtonSize::Default,
            ToggleSize::Sm => ButtonSize::Sm,
            ToggleSize::Lg => ButtonSize::Lg,
        };
        let mut c = self.button(
            theme,
            if variant == ToggleVariant::Outline {
                ButtonVariant::Outline
            } else {
                ButtonVariant::Ghost
            },
            size,
        );
        c.selected_hover = ControlColors {
            background: theme.bg_hover,
            ..c.selected
        };
        c
    }
    pub fn badge(&self, theme: &Theme, variant: BadgeVariant) -> BadgeStyle {
        let variant = match variant {
            BadgeVariant::Default => ButtonVariant::Default,
            BadgeVariant::Secondary => ButtonVariant::Secondary,
            BadgeVariant::Outline => ButtonVariant::Outline,
            BadgeVariant::Destructive => ButtonVariant::Destructive,
            BadgeVariant::Ghost => ButtonVariant::Ghost,
            BadgeVariant::Link => ButtonVariant::Link,
        };
        let c = self.button(theme, variant, ButtonSize::Xs);
        BadgeStyle {
            text: TextStyle {
                weight: FontWeight::MEDIUM,
                overflow: TextOverflow::Ellipsis,
                decoration: if variant == ButtonVariant::Link {
                    TextDecoration::Underline
                } else {
                    TextDecoration::None
                },
                ..TextStyle::new(self.control_font_size_px - 1.0, c.normal.foreground)
            },
            padding_x_px: self.spacing_px * 1.5,
            padding_y_px: self.spacing_px * 0.5,
            radius_px: self.radius_px,
            border_width_px: c.border_width_px,
            border_color: c.normal.border,
            background: c.normal.background,
        }
    }
}
impl SwitchStyles {
    pub fn for_size(mut self, size: SwitchSize) -> Self {
        if size == SwitchSize::Sm {
            let factor = 16.0 / 21.0;
            self.track_height_px *= factor;
            self.track_width_px *= 28.0 / 36.0;
            self.track_radius_px = self.track_height_px * 0.5;
            self.thumb_size_px *= 10.0 / 15.0;
            self.thumb_radius_px = self.thumb_size_px * 0.5;
            self.thumb_travel_px = self.track_width_px
                - self.thumb_size_px
                - 2.0 * self.track_padding_px
                - self.border_width_px;
        }
        self
    }
}

#[derive(Clone, Copy)]
pub struct ApplicationMetrics {
    pub binary_action_radius_px: f32,
    pub info_section_icon_extra_px: f32,
    pub info_share_hit_extra_px: f32,
    pub info_share_radius_px: f32,
    pub info_share_opacity: f32,
    pub info_share_hover_opacity: f32,
    pub debugger_swatch_size_px: f32,
    pub debugger_swatch_radius_px: f32,
    pub debugger_segment_radius_px: f32,
    pub format_more_dot_size_px: f32,
    pub format_more_dot_count: usize,
    pub menu_disabled_label_opacity: f32,
    pub tab_preview_opacity: f32,
    pub table_grid_cell_px: f32,
    pub table_grid_gap_px: f32,
    pub color_picker_radius_px: f32,
    pub color_picker_handle_border_px: f32,
    pub color_slider_height_px: f32,
    pub color_slider_handle_width_px: f32,
    pub color_sv_handle_size_px: f32,
    pub replace_button_size_px: f32,
    pub replace_icon_size_px: f32,
    pub search_fold_radius_px: f32,
    pub drag_padding_x_px: f32,
    pub tab_drag_padding_x_px: f32,
    pub drag_padding_y_px: f32,
    pub drag_radius_px: f32,
    pub drag_cursor_gap_px: f32,
    pub tab_drag_opacity: f32,
    pub status_height_px: f32,
    pub status_font_size_px: f32,
    pub status_padding_x_px: f32,
    pub status_gap_px: f32,
    pub status_icon_size_px: f32,
    pub theme_card_padding_px: f32,
    pub theme_card_gap_px: f32,
    pub theme_card_radius_px: f32,
    pub theme_card_border_width_px: f32,
    pub theme_swatch_size_px: f32,
    pub theme_swatch_gap_px: f32,
    pub theme_title_size_px: f32,
    pub utility_row_gap_px: f32,
    pub workspace_toggle_radius_px: f32,
    pub workspace_match_gap_px: f32,
    pub tab_action_inset_px: f32,
    pub tab_action_min_height_px: f32,
    pub caption_icon_size_px: f32,
    pub switch_size: SwitchSize,
    pub linux_caption_size_px: f32,
    pub linux_caption_icon_size_px: f32,
    pub linux_caption_gap_px: f32,
    pub linux_caption_padding_px: f32,
}
impl Default for ApplicationMetrics {
    fn default() -> Self {
        Self {
            replace_button_size_px: 18.0,
            replace_icon_size_px: 12.0,
            search_fold_radius_px: 3.0,
            drag_padding_x_px: 8.0,
            tab_drag_padding_x_px: 10.0,
            drag_padding_y_px: 4.0,
            drag_radius_px: 4.0,
            drag_cursor_gap_px: 16.0,
            tab_drag_opacity: 0.85,
            color_picker_radius_px: 10.0,
            color_picker_handle_border_px: 2.0,
            color_slider_height_px: 16.0,
            color_slider_handle_width_px: 12.0,
            color_sv_handle_size_px: 14.0,
            table_grid_cell_px: 9.0,
            table_grid_gap_px: 3.0,
            binary_action_radius_px: 8.0,
            debugger_swatch_size_px: 14.0,
            debugger_swatch_radius_px: 4.0,
            debugger_segment_radius_px: 6.0,
            format_more_dot_size_px: 2.0,
            format_more_dot_count: 3,
            menu_disabled_label_opacity: 0.4,
            tab_preview_opacity: 0.72,
            info_section_icon_extra_px: 3.0,
            info_share_hit_extra_px: 8.0,
            info_share_radius_px: 6.0,
            info_share_opacity: 0.5,
            info_share_hover_opacity: 0.6,
            status_height_px: 24.0,
            status_font_size_px: 12.0,
            status_padding_x_px: 6.0,
            status_gap_px: 4.0,
            status_icon_size_px: 11.0,
            theme_card_padding_px: 8.0,
            theme_card_gap_px: 6.0,
            theme_card_radius_px: 8.0,
            theme_card_border_width_px: 2.0,
            theme_swatch_size_px: 10.0,
            theme_swatch_gap_px: 4.0,
            theme_title_size_px: 11.0,
            utility_row_gap_px: 6.0,
            workspace_toggle_radius_px: 4.0,
            workspace_match_gap_px: 4.0,
            linux_caption_size_px: 20.0,
            linux_caption_icon_size_px: 12.0,
            linux_caption_gap_px: 12.0,
            linux_caption_padding_px: 12.0,
            tab_action_inset_px: 6.0,
            tab_action_min_height_px: 18.0,
            caption_icon_size_px: 13.0,
            switch_size: SwitchSize::Default,
        }
    }
}
impl Styles {
    pub fn workspace_match_row(&self) -> ListItemStyle {
        let s = &self.workspace_search;
        let mut c = self.ui.control;
        c.height_px = s.ws_search_match_height_px;
        c.padding_x_px = s.ws_search_match_padding_x_px;
        c.radius_px = 0.0;
        c.gap_px = self.ui.spec.application.workspace_match_gap_px;
        c.font_size_px = s.ws_search_match_font_size_px;
        c.normal.foreground = s.ws_search_match_text_color;
        c.hover = ControlColors {
            background: s.ws_search_btn_hover_bg,
            ..c.normal
        };
        c.selected = ControlColors {
            background: self.sidebar.sidebar_item_selected_bg,
            ..c.normal
        };
        c.selected_hover = c.hover;
        c.pressed = c.hover;
        self.ui.item(c)
    }
    pub fn tab_action(&self, id: &str, active: bool, bar_height: f32, scale: f32) -> ControlStyle {
        let s = &self.tab_bar;
        let mut c = self.ui.control;
        c.height_px = (bar_height - self.ui.spec.application.tab_action_inset_px * scale)
            .max(self.ui.spec.application.tab_action_min_height_px)
            / scale;
        c.icon_size_px = match id {
            "tab-bar-outline-toggle" => s.tab_action_emphasis_icon_size_px - 1.0,
            "tab-bar-theme-toggle" => s.tab_action_emphasis_icon_size_px,
            _ => s.tab_item_icon_size_px,
        };
        c.padding_x_px = s.tab_action_button_padding_x_px;
        c.radius_px = 0.0;
        c.border_width_px = 0.0;
        c.elevation = None;
        c.normal.background = gpui::transparent_black();
        c.normal.border = gpui::transparent_black();
        c.min_width_px = c.icon_size_px + c.padding_x_px * 2.0;
        c.normal.foreground = if active {
            s.tab_item_active_text_color
        } else {
            s.tab_item_icon_color
        };
        c.normal.opacity = if active {
            s.tab_item_icon_active_opacity
        } else {
            s.tab_item_icon_inactive_opacity
        };
        c.hover = ControlColors {
            opacity: s.tab_item_icon_active_opacity,
            ..c.normal
        };
        c.pressed = c.hover;
        c
    }

    pub fn tab_action_width(&self, id: &str, bar_height: f32, scale: f32) -> f32 {
        let style = self.tab_action(id, false, bar_height, scale);
        let border = style.border_width_px * if style.scale_border { scale } else { 1.0 };
        (style.min_width_px * scale)
            .max((style.icon_size_px + style.padding_x_px * 2.0) * scale + border * 2.0)
    }

    pub fn tab_action_separator_metrics(&self, scale: f32) -> (f32, f32, f32) {
        (
            scale,
            14.0 * scale,
            self.ui.space(SpaceStep::OneAndQuarter) * scale,
        )
    }
}

#[derive(Clone, Copy)]
pub struct OutlineRowStyle {
    pub padding_right_px: f32,
    pub padding_y_px: f32,
    pub gap_px: f32,
    pub text: TextStyle,
    pub active_background: Hsla,
}
impl Styles {
    pub fn outline_row(&self, active: bool) -> OutlineRowStyle {
        let o = &self.outline;
        let u = &self.utility_panel;
        OutlineRowStyle {
            padding_right_px: u.utility_panel_section_padding_x_px,
            padding_y_px: o.outline_rail_label_item_padding_y_px,
            gap_px: o.outline_rail_label_fold_icon_gap_px,
            text: TextStyle {
                line_height_px: Some(o.outline_rail_label_line_height_px),
                ..TextStyle::new(
                    o.outline_rail_label_font_size_px,
                    if active {
                        o.outline_rail_label_active_color
                    } else {
                        u.utility_panel_item_value_text_color
                    },
                )
            },
            active_background: u.utility_panel_button_hover_bg,
        }
    }
    pub fn caption_button(
        &self,
        windows: bool,
        danger: bool,
        height: f32,
        scale: f32,
    ) -> ControlStyle {
        let s = &self.tab_bar;
        let mut c = self.ui.control;
        c.height_px = if windows {
            height / scale
        } else {
            self.ui.spec.application.linux_caption_size_px
        };
        c.min_width_px = if windows {
            WINDOW_CAPTION_BUTTON_WIDTH_PX
        } else {
            c.height_px
        };
        c.icon_size_px = if windows {
            self.ui.spec.application.caption_icon_size_px
        } else {
            self.ui.spec.application.linux_caption_icon_size_px
        };
        c.padding_x_px = 0.0;
        c.radius_px = if windows { 0.0 } else { c.height_px * 0.5 };
        c.normal.foreground = s.tab_item_icon_color;
        c.hover = ControlColors {
            background: if windows && danger {
                self.ui.colors.caption_close_hover_background
            } else {
                s.tab_item_hover_bg
            },
            foreground: if windows && danger {
                self.ui.colors.caption_close_hover_foreground
            } else {
                c.normal.foreground
            },
            ..c.normal
        };
        c.pressed = c.hover;
        c
    }
}

#[derive(Clone, Copy)]
pub struct CompactActionStyle {
    pub control: ControlStyle,
    pub padding_y_px: f32,
}
#[derive(Clone, Copy)]
pub enum CompactActionRole {
    DebuggerSection,
    DebuggerText,
}
impl Styles {
    pub fn compact_action(&self, role: CompactActionRole) -> CompactActionStyle {
        let u = &self.utility_panel;
        let d = &self.theme_debugger;
        let mut c = self.ui.toggle;
        c.min_width_px = 0.0;
        c.border_width_px = 0.0;
        c.radius_px = self.ui.spec.appearance.corners.factor() * 4.0;
        c.disabled_opacity = 0.45;
        c.normal.foreground = u.utility_panel_item_label_text_color;
        c.hover = ControlColors {
            background: u.utility_panel_button_hover_bg,
            ..c.normal
        };
        c.selected = ControlColors {
            foreground: u.utility_panel_item_value_text_color,
            ..c.hover
        };
        c.selected_hover = c.selected;
        let padding_y_px = match role {
            CompactActionRole::DebuggerSection => {
                c.padding_x_px = 10.0;
                c.font_size_px = self.ui_font_size_px;
                c.radius_px = 0.0;
                c.normal.foreground = d.foreground;
                c.hover = ControlColors {
                    background: d.hover,
                    ..c.normal
                };
                c.selected = ControlColors {
                    foreground: d.panel,
                    background: d.foreground,
                    ..c.normal
                };
                c.selected_hover = c.selected;
                5.0
            }
            CompactActionRole::DebuggerText => {
                c.padding_x_px = 0.0;
                c.font_size_px = self.ui_font_size_px;
                c.radius_px = 0.0;
                c.normal.foreground = d.foreground;
                c.hover = ControlColors {
                    opacity: 0.75,
                    ..c.normal
                };
                0.0
            }
        };
        c.pressed = c.hover;
        CompactActionStyle {
            control: c,
            padding_y_px,
        }
    }
}

#[derive(Clone)]
pub struct DragPreviewStyle {
    pub surface: SurfaceStyle,
    pub font_size_px: f32,
    pub padding_x_px: f32,
    pub padding_y_px: f32,
    pub cursor_gap_px: f32,
}
impl Styles {
    pub fn drag_preview(&self, tab: bool, scale: f32) -> DragPreviewStyle {
        let mut surface = self.tooltip_surface();
        surface.radius_px = self.ui.spec.application.drag_radius_px;
        surface.border_width_px = 0.0;
        surface.shadow = self.ui.popup_shadow.clone();
        surface.scale_shadow = false;
        surface.opacity = if tab {
            self.ui.spec.application.tab_drag_opacity
        } else {
            1.0
        };
        DragPreviewStyle {
            surface,
            font_size_px: self.ui_font_size_px * scale,
            padding_x_px: if tab {
                self.ui.spec.application.tab_drag_padding_x_px
            } else {
                self.ui.spec.application.drag_padding_x_px
            },
            padding_y_px: self.ui.spec.application.drag_padding_y_px,
            cursor_gap_px: self.ui.spec.application.drag_cursor_gap_px,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextDecoration {
    None,
    Underline,
    Strikethrough,
}
impl SidebarStyles {
    pub fn file_title(&self, color: Hsla) -> TextStyle {
        TextStyle {
            weight: FontWeight::SEMIBOLD,
            line_height_px: Some(self.file_list_item_title_line_height_px),
            ..TextStyle::new(self.file_list_item_title_font_size_px, color)
        }
    }
    pub fn file_preview(&self) -> TextStyle {
        TextStyle {
            overflow: TextOverflow::Wrap,
            line_height_px: Some(self.file_list_item_preview_line_height_px),
            ..TextStyle::new(
                self.file_list_item_preview_font_size_px,
                self.file_list_item_preview_color,
            )
        }
    }
    pub fn file_time(&self) -> TextStyle {
        TextStyle {
            line_height_px: Some(self.file_list_item_time_line_height_px),
            ..TextStyle::new(
                self.file_list_item_time_font_size_px,
                self.file_list_item_time_color,
            )
        }
    }
}

#[derive(Clone, Copy)]
pub struct WindowFrameStyle {
    pub font_fallbacks: Option<&'static [&'static str]>,
    pub background: Hsla,
    pub border: Hsla,
    pub border_width_px: f32,
    pub font_family: &'static str,
    pub font_size_px: f32,
}
impl Styles {
    pub fn window_frame(&self) -> WindowFrameStyle {
        WindowFrameStyle {
            font_fallbacks: self.ui.spec.font_fallbacks,
            background: self.window.window_bg,
            border: self.window.window_border_color,
            border_width_px: self.window.window_border_width_px,
            font_family: self.ui_font_family,
            font_size_px: self.ui_font_size_px,
        }
    }
}

impl Styles {
    pub fn search_surface(&self) -> SurfaceStyle {
        let s = &self.search;
        SurfaceStyle {
            background: s.search_bar_bg,
            foreground: s.search_match_text_color,
            border_color: s.search_bar_border_color,
            border_width_px: s.search_bar_border_width_px,
            radius_px: s.search_bar_radius_px,
            scale_border: false,
            scale_shadow: false,
            opacity: 1.0,
            shadow: s.search_bar_shadow.clone(),
        }
    }
    pub fn search_replace_button(&self) -> ControlStyle {
        let s = &self.search;
        let mut c = self.search_navigation(false);
        c.height_px = self.ui.spec.application.replace_button_size_px;
        c.icon_size_px = self.ui.spec.application.replace_icon_size_px;
        c.normal.foreground = s.search_replace_btn_text_color;
        c.hover = ControlColors {
            background: s.search_replace_btn_hover_bg,
            ..c.normal
        };
        c.pressed = c.hover;
        c
    }
    pub fn search_fold_button(&self, size: f32, scale: f32) -> ControlStyle {
        let mut c = self.search_toggle();
        c.height_px = size / scale;
        c.radius_px = self.ui.spec.application.search_fold_radius_px;
        c
    }
    pub fn format_surface(&self, opacity: f32) -> SurfaceStyle {
        let s = &self.format_toolbar;
        SurfaceStyle {
            background: s.format_toolbar_bg,
            foreground: s.format_toolbar_icon_color,
            border_color: s.format_toolbar_border_color,
            border_width_px: s.format_toolbar_border_width_px,
            radius_px: s.format_toolbar_radius_px,
            scale_border: true,
            scale_shadow: false,
            opacity,
            shadow: self.ui.small_shadow.clone(),
        }
    }
    pub fn format_button(&self, active: bool) -> ControlStyle {
        let s = &self.format_toolbar;
        let mut c = self.ui.control;
        c.height_px = s.format_toolbar_btn_size_px;
        c.icon_size_px = s.format_toolbar_icon_size_px;
        c.padding_x_px = 0.0;
        c.radius_px = 0.0;
        c.normal.foreground = if active {
            s.format_toolbar_icon_active_color
        } else {
            with_alpha(
                s.format_toolbar_icon_color,
                s.format_toolbar_icon_color.a * 0.76,
            )
        };
        c.hover = ControlColors {
            foreground: s.format_toolbar_icon_hover_color,
            ..c.normal
        };
        c.pressed = c.hover;
        c
    }
}

impl Styles {
    fn apply_ui_color_overrides(&mut self, colors: crate::theme::UiColorOverrides) {
        use crate::theme::UiColor;
        macro_rules! apply { ($slot:ident => $($field:expr),+ $(,)?) => {
            if let Some(color) = colors.get(UiColor::$slot) { $($field = color;)+ }
        }; }
        apply!(Background => self.window.window_bg);
        apply!(Foreground => self.settings_panel.settings_primary_text_color, self.ui.colors.status_title, self.text_input.text_input_text_color, self.theme_debugger.foreground);
        apply!(Card => self.utility_panel.utility_panel_bg, self.theme_debugger.card);
        apply!(CardForeground => self.utility_panel.utility_panel_item_value_text_color, self.utility_panel.utility_panel_header_text_color);
        apply!(Popover => self.context_menu.menu_bg, self.quick_open.quick_open_panel_bg, self.search.search_bar_bg, self.tooltip.tooltip_bg);
        apply!(PopoverForeground => self.context_menu.menu_item_primary_text_color, self.context_menu.menu_item_text_color,
            self.quick_open.quick_open_result_text_color, self.quick_open.quick_open_input_text_color, self.search.search_match_text_color, self.tooltip.tooltip_title_text_color);
        apply!(Primary => self.settings_panel.settings_option_button_active_bg, self.settings_panel.settings_option_button_active_border_color,
            self.switch.colors.checked_background, self.switch.colors.checked_hover_background);
        apply!(PrimaryForeground => self.settings_panel.settings_option_button_active_text_color, self.switch.colors.thumb);
        apply!(Secondary => self.settings_panel.settings_option_button_bg, self.settings_panel.settings_site_button_bg, self.switch.colors.unchecked_background, self.theme_debugger.panel);
        apply!(SecondaryForeground => self.settings_panel.settings_option_button_text_color, self.settings_panel.settings_site_button_text_color);
        apply!(Muted => self.ui.colors.status_background, self.tab_bar.tab_item_bg);
        apply!(MutedForeground => self.settings_panel.settings_secondary_text_color, self.utility_panel.utility_panel_item_label_text_color,
            self.quick_open.quick_open_result_secondary_text_color, self.ui.colors.status_value, self.theme_debugger.muted, self.switch.colors.description);
        apply!(Accent => self.context_menu.menu_item_selected_bg, self.quick_open.quick_open_result_selected_bg,
            self.search.search_option_btn_active_bg, self.settings_panel.settings_theme_option_active_bg, self.theme_debugger.selected);
        apply!(AccentForeground => self.search.search_option_btn_active_icon_color, self.settings_panel.settings_theme_option_active_text_color);
        apply!(Destructive => self.ui.colors.danger, self.sidebar.sidebar_error_text_color);
        apply!(Border => self.window.window_border_color, self.context_menu.menu_border_color, self.quick_open.quick_open_panel_border_color,
            self.search.search_bar_border_color, self.utility_panel.utility_panel_border_color, self.settings_panel.settings_contrast_border_color,
            self.settings_panel.settings_option_button_border_color, self.theme_debugger.border, self.theme_debugger.divider);
        apply!(Input => self.text_input.text_input_border_color, self.sidebar.sidebar_edit_input_border_color);
        apply!(Ring => self.switch.ring.color, self.text_input.text_input_focus_border_color);
        apply!(Sidebar => self.sidebar.sidebar_bg, self.sidebar.sidebar_header_bg, self.sidebar.file_list_item_bg);
        apply!(SidebarForeground => self.sidebar.sidebar_item_text_color, self.sidebar.sidebar_header_text_color, self.sidebar.sidebar_item_icon_color, self.sidebar.file_list_item_title_color);
        apply!(SidebarPrimary => self.sidebar.sidebar_top_button_active_color);
        apply!(SidebarAccent => self.sidebar.sidebar_item_selected_bg, self.sidebar.file_list_item_selected_bg, self.workspace_search.ws_search_btn_active_bg);
        apply!(SidebarAccentForeground => self.sidebar.sidebar_item_selected_text_color, self.sidebar.sidebar_item_selected_icon_color, self.workspace_search.ws_search_btn_active_icon_color);
        apply!(SidebarBorder => self.sidebar.sidebar_resize_indicator_color, self.sidebar.file_list_item_separator_color);
        apply!(SidebarRing => self.sidebar.sidebar_item_focus_border_color);
        apply!(Success => self.ui.colors.success);
        apply!(Warning => self.ui.colors.warning);
        apply!(Info => self.ui.colors.accent);
        // Text-input context menus share the resolved menu contract, not a
        // separately mutable mirror of the legacy theme fields.
        self.text_input.menu = self.context_menu.clone();
    }
}

impl Styles {
    pub fn settings_background(&self) -> Hsla {
        self.ui
            .spec
            .colors
            .get(crate::theme::UiColor::Background)
            .unwrap_or(self.editor.editor_bg)
    }
}

/// Shared quarter-step spacing scale, based on the configured logical unit.
#[derive(Clone, Copy)]
pub enum SpaceStep {
    Half,
    ThreeQuarters,
    One,
    OneAndQuarter,
    OneAndHalf,
    Two,
    TwoAndHalf,
    Three,
    Four,
}
impl UiStyles {
    pub fn space(&self, step: SpaceStep) -> f32 {
        self.spec.spacing_px
            * match step {
                SpaceStep::Half => 0.5,
                SpaceStep::ThreeQuarters => 0.75,
                SpaceStep::One => 1.0,
                SpaceStep::OneAndQuarter => 1.25,
                SpaceStep::OneAndHalf => 1.5,
                SpaceStep::Two => 2.0,
                SpaceStep::TwoAndHalf => 2.5,
                SpaceStep::Three => 3.0,
                SpaceStep::Four => 4.0,
            }
    }
}

impl Styles {
    pub fn debugger_card_surface(&self) -> SurfaceStyle {
        let d = &self.theme_debugger;
        SurfaceStyle {
            background: d.card,
            foreground: d.foreground,
            border_color: d.divider,
            border_width_px: self.ui.spec.border_width_px,
            radius_px: self.ui.spec.application.color_picker_radius_px,
            scale_border: false,
            scale_shadow: false,
            opacity: 1.0,
            shadow: ShadowStyle::default(),
        }
    }
    pub fn debugger_picker_surface(&self, slider: bool, alpha: bool) -> SurfaceStyle {
        let mut surface = self.debugger_card_surface();
        surface.background = if alpha {
            self.theme_debugger.panel
        } else {
            gpui::transparent_black()
        };
        if slider {
            surface.radius_px = self.theme_debugger.slider_height_px * 0.5;
        }
        surface
    }
    pub fn debugger_handle_surface(&self, slider: bool, sample: Hsla) -> SurfaceStyle {
        let mut surface = self.debugger_picker_surface(slider, false);
        surface.background = sample;
        surface.shadow = self.ui.small_shadow.clone();
        if !slider {
            surface.radius_px = self.theme_debugger.sv_handle_size_px * 0.5;
            surface.border_width_px = self.ui.spec.application.color_picker_handle_border_px;
            surface.border_color = crate::theme::COLOR_PICKER_HANDLE;
        }
        surface
    }
}

#[derive(Clone, Copy)]
pub enum GroupAlignment {
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Clone, Copy)]
pub struct SelectionGridStyle {
    pub cell_size_px: f32,
    pub gap_px: f32,
    pub border_width_px: f32,
    pub border: Hsla,
    pub active_border: Hsla,
    pub current: Hsla,
    pub hover: Hsla,
    pub overlap: Hsla,
}
impl Styles {
    pub fn settings_folder_picker(&self) -> ControlStyle {
        let mut c = self.settings_picker();
        c.height_px = self.settings_panel.settings_input_height_px;
        c.radius_px = self.settings_panel.settings_input_radius_px;
        c.font_size_px = self.ui_font_size_px;
        c.padding_x_px = self.ui.space(SpaceStep::Two);
        c.scale_border = false;
        c
    }
    pub fn format_footer_row(&self) -> ListItemStyle {
        let mut row = self.context_menu.item();
        row.control.padding_x_px = self.format_toolbar.format_toolbar_padding_x_px;
        row.control.gap_px = self.format_toolbar.format_toolbar_gap_px;
        row.control.normal.foreground = self.format_toolbar.format_toolbar_icon_color;
        row.control.hover.foreground = row.control.normal.foreground;
        row
    }
    pub fn table_size_grid(&self) -> SelectionGridStyle {
        let m = &self.context_menu;
        SelectionGridStyle {
            cell_size_px: self.ui.spec.application.table_grid_cell_px,
            gap_px: self.ui.spec.application.table_grid_gap_px,
            border_width_px: self.ui.spec.border_width_px,
            border: m.menu_border_color,
            active_border: m.menu_format_active_text_color,
            current: with_alpha(m.menu_format_active_text_color, 0.46),
            hover: with_alpha(m.menu_format_active_text_color, 0.22),
            overlap: with_alpha(m.menu_format_active_text_color, 0.62),
        }
    }
}

#[derive(Clone, Copy)]
pub struct DecorationStyle {
    pub size_px: f32,
    pub radius_px: f32,
    pub border_width_px: f32,
    pub border: Hsla,
    pub background: Hsla,
}
#[derive(Clone, Copy)]
pub struct EllipsisStyle {
    pub dot_size_px: f32,
    pub gap_px: f32,
    pub count: usize,
    pub color: Hsla,
}
#[derive(Clone, Copy)]
pub struct FileNameStyle {
    pub text: TextStyle,
    pub opacity: f32,
    pub suffix_opacity: f32,
}
#[derive(Clone, Copy)]
pub struct ThumbnailStyle {
    pub radius_px: f32,
    pub border_width_px: f32,
    pub border: Hsla,
}
impl Styles {
    pub fn debugger_swatch(&self, sample: Hsla) -> DecorationStyle {
        DecorationStyle {
            size_px: self.ui.spec.application.debugger_swatch_size_px,
            radius_px: self.ui.spec.application.debugger_swatch_radius_px,
            border_width_px: self.ui.spec.border_width_px,
            border: self.theme_debugger.divider,
            background: sample,
        }
    }
    pub fn format_ellipsis(&self, color: Hsla) -> EllipsisStyle {
        EllipsisStyle {
            dot_size_px: self.ui.spec.application.format_more_dot_size_px,
            gap_px: self.ui.space(SpaceStep::Half),
            count: self.ui.spec.application.format_more_dot_count,
            color,
        }
    }
    pub fn sidebar_name(&self, color: Hsla) -> FileNameStyle {
        FileNameStyle {
            text: TextStyle::new(self.sidebar.sidebar_item_font_size_px, color),
            opacity: 1.0,
            suffix_opacity: self.sidebar.sidebar_item_suffix_opacity,
        }
    }
    pub fn tab_name(&self, active: bool, preview: bool, deleted: bool) -> FileNameStyle {
        let s = &self.tab_bar;
        FileNameStyle {
            text: TextStyle {
                italic: preview,
                font_family: preview.then_some(crate::platform::fonts::PROPORTIONAL),
                decoration: if deleted {
                    TextDecoration::Strikethrough
                } else {
                    TextDecoration::None
                },
                weight: FontWeight::NORMAL,
                ..TextStyle::new(
                    s.tab_item_font_size_px,
                    if deleted {
                        s.tab_item_deleted_text_color
                    } else if active {
                        s.tab_item_active_text_color
                    } else {
                        s.tab_item_text_color
                    },
                )
            },
            opacity: if preview && !active {
                self.ui.spec.application.tab_preview_opacity
            } else {
                1.0
            },
            suffix_opacity: s.tab_item_suffix_opacity,
        }
    }
    pub fn tab_badge(&self) -> BadgeStyle {
        let s = &self.tab_bar;
        BadgeStyle {
            text: TextStyle::new(s.tab_item_badge_font_size_px, s.tab_item_badge_text_color),
            padding_x_px: s.tab_item_badge_padding_x_px,
            padding_y_px: 0.0,
            radius_px: s.tab_item_badge_radius_px,
            border_width_px: 0.0,
            border_color: gpui::transparent_black(),
            background: s.tab_item_badge_bg,
        }
    }
    pub fn file_thumbnail(&self) -> ThumbnailStyle {
        let s = &self.sidebar;
        ThumbnailStyle {
            radius_px: s.file_list_item_thumbnail_radius_px,
            border_width_px: s.file_list_item_thumbnail_border_width_px,
            border: s.file_list_item_thumbnail_border_color,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerVariant {
    Default,
    Outline,
    Muted,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertVariant {
    Default,
    Destructive,
    Success,
    Warning,
    Info,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogVariant {
    Default,
    Outline,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogSize {
    Default,
    Sm,
    Lg,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabsVariant {
    Default,
    Line,
}
#[derive(Clone, Copy)]
pub enum ItemSize {
    Default,
    Sm,
    Xs,
}
#[derive(Clone, Copy)]
pub struct ItemPresentation {
    pub orientation: Orientation,
    pub variant: ContainerVariant,
    pub size: ItemSize,
}
impl ItemPresentation {
    pub fn height_factor(self) -> f32 {
        match self.size {
            ItemSize::Default => 1.0,
            ItemSize::Sm => 0.875,
            ItemSize::Xs => 0.75,
        }
    }
    pub fn padding_factor(self) -> f32 {
        match self.size {
            ItemSize::Default => 1.0,
            ItemSize::Sm => 0.75,
            ItemSize::Xs => 0.5,
        }
    }
}
impl Default for ItemPresentation {
    fn default() -> Self {
        Self {
            orientation: Orientation::Horizontal,
            variant: ContainerVariant::Default,
            size: ItemSize::Default,
        }
    }
}
#[derive(Clone, Copy)]
pub struct CompositionSpec {
    pub field_orientation: Orientation,
    pub field_variant: ContainerVariant,
    pub item: ItemPresentation,
    pub empty_variant: ContainerVariant,
    pub alert_variant: AlertVariant,
    pub dialog_variant: DialogVariant,
    pub dialog_size: DialogSize,
    pub tabs_orientation: Orientation,
    pub tabs_variant: TabsVariant,
}
impl Default for CompositionSpec {
    fn default() -> Self {
        Self {
            field_orientation: Orientation::Horizontal,
            field_variant: ContainerVariant::Default,
            item: ItemPresentation::default(),
            empty_variant: ContainerVariant::Default,
            alert_variant: AlertVariant::Default,
            dialog_variant: DialogVariant::Default,
            dialog_size: DialogSize::Default,
            tabs_orientation: Orientation::Horizontal,
            tabs_variant: TabsVariant::Default,
        }
    }
}
impl UiStyles {
    pub fn item(&self, mut control: ControlStyle) -> ListItemStyle {
        let presentation = self.spec.composition.item;
        control.radius_px = 0.0;
        control.elevation = None;
        control.height_px *= presentation.height_factor();
        control.padding_x_px *= presentation.padding_factor();
        if presentation.variant == ContainerVariant::Outline {
            control.border_width_px = self.spec.border_width_px;
        }
        ListItemStyle {
            inset_x_px: 0.0,
            control,
            presentation,
        }
    }
}
impl Styles {
    pub fn dialog_width(&self) -> f32 {
        self.dialog.dialog_width_px
            * match self.ui.spec.composition.dialog_size {
                DialogSize::Default => 1.0,
                DialogSize::Sm => 0.85,
                DialogSize::Lg => 1.5,
            }
    }
    /// Segmented controls joined by borders, or a muted tray when raised.
    pub fn segmented_group(&self, radius_px: f32) -> ToggleGroupStyle {
        ToggleGroupStyle {
            alignment: GroupAlignment::Stretch,
            orientation: Orientation::Horizontal,
            gap_px: if self.ui.spec.appearance.elevation {
                self.ui.space(SpaceStep::Half)
            } else {
                0.0
            },
            tray: self.segment_tray(radius_px),
        }
    }
    fn segment_tray(&self, radius_px: f32) -> Option<SegmentTrayStyle> {
        self.ui.spec.appearance.elevation.then(|| {
            let padding_px = self.ui.space(SpaceStep::Half);
            SegmentTrayStyle {
                background: self.ui.segment_tray,
                padding_px,
                radius_px: radius_px + if radius_px > 0.0 { padding_px } else { 0.0 },
            }
        })
    }
    /// Raised segments drop their joined borders; the selected one becomes a
    /// shadowed surface inside the tray.
    pub fn raised_segment(&self, mut c: ControlStyle) -> ControlStyle {
        if !self.ui.spec.appearance.elevation {
            return c;
        }
        let transparent = gpui::transparent_black();
        c.border_width_px = 0.0;
        c.normal.background = transparent;
        c.normal.border = transparent;
        // Unselected segments keep the native hover fill inside the tray.
        c.hover = ControlColors {
            background: self.ui.segment_hover,
            foreground: self.ui.raised_foreground,
            ..c.normal
        };
        c.pressed = c.hover;
        c.selected = ControlColors {
            background: self.ui.raised_surface,
            foreground: self.ui.raised_foreground,
            border: transparent,
            opacity: c.selected.opacity,
        };
        c.selected_hover = c.selected;
        c
    }
}

#[derive(Clone, Copy)]
pub struct IconStyle {
    pub size_px: f32,
    pub color: Hsla,
}
impl IconStyle {
    pub fn new(size_px: f32, color: Hsla) -> Self {
        Self { size_px, color }
    }
}
#[derive(Clone, Copy)]
pub struct InfoSectionStyle {
    pub text: TextStyle,
    pub margin_bottom_px: f32,
    pub padding_x_px: f32,
    pub gap_px: f32,
    pub icon_size_px: f32,
}
impl Styles {
    pub fn info_section(&self) -> InfoSectionStyle {
        let u = &self.utility_panel;
        InfoSectionStyle {
            text: TextStyle {
                weight: FontWeight::BOLD,
                ..TextStyle::new(
                    u.utility_panel_section_title_font_size_px,
                    u.utility_panel_section_title_text_color,
                )
            },
            margin_bottom_px: self.ui.space(SpaceStep::OneAndQuarter),
            padding_x_px: u.utility_panel_section_padding_x_px,
            gap_px: self.ui.space(SpaceStep::OneAndHalf),
            icon_size_px: u.utility_panel_section_title_font_size_px
                + self.ui.spec.application.info_section_icon_extra_px,
        }
    }
    pub fn info_share_action(&self) -> ControlStyle {
        let u = &self.utility_panel;
        let mut c = self.ui.control;
        c.icon_size_px = u.utility_panel_item_font_size_px;
        c.height_px = c.icon_size_px + self.ui.spec.application.info_share_hit_extra_px;
        c.radius_px = self.ui.spec.application.info_share_radius_px;
        c.padding_x_px = 0.0;
        c.normal.foreground = u.utility_panel_button_text_color;
        c.normal.opacity = self.ui.spec.application.info_share_opacity;
        c.hover = ControlColors {
            background: u.utility_panel_button_hover_bg,
            opacity: self.ui.spec.application.info_share_hover_opacity,
            ..c.normal
        };
        c.pressed = c.hover;
        c
    }
    pub fn tab_close(&self, active: bool, dirty: bool) -> ControlStyle {
        let t = &self.tab_bar;
        let mut c = self.ui.control;
        c.height_px = t.tab_item_close_size_px;
        c.padding_x_px = 0.0;
        c.radius_px = c.height_px * 0.5;
        c.font_size_px = t.tab_item_close_font_size_px;
        c.normal.foreground = t.tab_item_close_text_color;
        c.normal.opacity = if dirty {
            1.0
        } else if active {
            t.tab_item_close_active_opacity
        } else {
            t.tab_item_close_inactive_opacity
        };
        c.hover = if dirty {
            c.normal
        } else {
            ControlColors {
                background: t.tab_item_close_hover_bg,
                opacity: 1.0,
                ..c.normal
            }
        };
        c.pressed = c.hover;
        c
    }
    pub fn tab_dirty_dot(&self) -> DecorationStyle {
        let t = &self.tab_bar;
        DecorationStyle {
            size_px: t.tab_item_dirty_dot_size_px,
            radius_px: t.tab_item_dirty_dot_size_px * 0.5,
            border_width_px: 0.0,
            border: gpui::transparent_black(),
            background: t.tab_item_dirty_dot_color,
        }
    }
}

#[derive(Clone, Copy)]
pub struct NavigationHeaderStyle {
    pub height_px: f32,
    pub left_px: f32,
    pub right_px: f32,
    pub gap_px: f32,
    pub border_width_px: f32,
    pub border: Hsla,
    pub background: Hsla,
    pub hover: Hsla,
    pub text: TextStyle,
}
impl Styles {
    pub fn navigation_header(
        &self,
        file_list: bool,
        selected: bool,
        drag_over: bool,
    ) -> NavigationHeaderStyle {
        let s = &self.sidebar;
        if file_list {
            NavigationHeaderStyle {
                height_px: s.file_list_header_height_px,
                left_px: s.file_list_header_padding_left_px,
                right_px: s.file_list_header_padding_right_px,
                gap_px: s.file_list_header_gap_px,
                border_width_px: s.file_list_header_border_width_px,
                border: s.file_list_header_border_color,
                background: gpui::transparent_black(),
                hover: gpui::transparent_black(),
                text: TextStyle::new(
                    s.file_list_header_font_size_px,
                    s.file_list_header_text_color,
                ),
            }
        } else {
            NavigationHeaderStyle {
                height_px: s.sidebar_header_height_px,
                left_px: s.sidebar_header_padding_left_px,
                right_px: s.sidebar_header_padding_right_px,
                gap_px: s.sidebar_header_gap_px,
                border_width_px: 0.0,
                border: gpui::transparent_black(),
                background: if selected || drag_over {
                    s.sidebar_item_selected_bg
                } else {
                    s.sidebar_header_bg
                },
                hover: s.sidebar_header_hover_bg,
                text: TextStyle::new(
                    s.sidebar_header_font_size_px,
                    if selected {
                        s.sidebar_item_selected_text_color
                    } else {
                        s.sidebar_header_text_color
                    },
                ),
            }
        }
    }
}

#[derive(Clone, Copy)]
pub struct BinaryPlaceholderStyle {
    pub title: TextStyle,
    pub detail: TextStyle,
    pub action: ControlStyle,
    pub gap_px: f32,
    pub action_padding_y_px: f32,
}
impl Styles {
    pub fn binary_placeholder(&self, theme: &Theme) -> BinaryPlaceholderStyle {
        use crate::theme::UiColor;
        let primary = self.ui.spec.colors.resolve(theme, UiColor::Foreground);
        let secondary = self.ui.spec.colors.resolve(theme, UiColor::MutedForeground);
        let mut action = self.ui.control;
        action.padding_x_px = self.ui.space(SpaceStep::Three);
        action.radius_px = self.ui.spec.application.binary_action_radius_px;
        action.normal.background = self
            .ui
            .spec
            .colors
            .get(UiColor::Secondary)
            .unwrap_or(theme.bg_hover);
        action.normal.foreground = primary;
        action.hover = action.normal;
        action.pressed = action.normal;
        action.cursor = gpui::CursorStyle::PointingHand;
        BinaryPlaceholderStyle {
            title: TextStyle::new(self.ui_font_size_px + 1.0, primary),
            detail: TextStyle::new(self.ui_font_size_px - 1.0, secondary),
            action,
            gap_px: self.ui.space(SpaceStep::TwoAndHalf),
            action_padding_y_px: self.ui.space(SpaceStep::One),
        }
    }
}
