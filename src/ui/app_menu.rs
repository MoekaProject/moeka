//! Window-local application-menu presentation. Commands and their live state
//! come from GPUI; document state stays with the application.
use crate::style::ContextMenuStyles;
use gpui::{
    Action, AppMenuActionState, Context, EventEmitter, FocusHandle, IntoElement, KeyDownEvent,
    MouseButton, OwnedMenu, OwnedMenuItem, Render, ScrollHandle, WeakFocusHandle, Window, div,
    prelude::*, px,
};

pub(crate) struct MenuClosed;

pub(crate) struct AppMenu {
    menus: Vec<OwnedMenu>,
    path: Vec<usize>,
    focus: FocusHandle,
    previous_focus: Option<WeakFocusHandle>,
    styles: ContextMenuStyles,
    scale: f32,
    family: &'static str,
    scroll: Vec<ScrollHandle>,
    map_action: fn(Box<dyn Action>) -> Box<dyn Action>,
    shortcuts: Vec<(Box<dyn Action>, String)>,
}

impl EventEmitter<MenuClosed> for AppMenu {}

impl AppMenu {
    pub(crate) fn new(
        styles: ContextMenuStyles,
        scale: f32,
        family: &'static str,
        map_action: fn(Box<dyn Action>) -> Box<dyn Action>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            menus: Vec::new(),
            path: Vec::new(),
            focus: cx.focus_handle(),
            previous_focus: None,
            styles,
            scale,
            family,
            scroll: Vec::new(),
            map_action,
            shortcuts: Vec::new(),
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        !self.path.is_empty()
    }

    pub(crate) fn set_styles(
        &mut self,
        styles: ContextMenuStyles,
        scale: f32,
        family: &'static str,
        cx: &mut Context<Self>,
    ) {
        self.styles = styles;
        self.scale = scale;
        self.family = family;
        cx.notify();
    }

    pub(crate) fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_open() {
            self.close(window, cx);
            return;
        }
        self.menus = cx.get_menus().unwrap_or_default();
        if self.menus.is_empty() {
            return;
        }
        self.previous_focus = window.focused(cx).map(|focus| focus.downgrade());
        self.shortcuts.clear();
        if let Some(focus) = window.focused(cx) {
            fn collect(
                items: &[OwnedMenuItem],
                window: &Window,
                focus: &FocusHandle,
                shortcuts: &mut Vec<(Box<dyn Action>, String)>,
            ) {
                for item in items {
                    match item {
                        OwnedMenuItem::Action { action, .. } => {
                            if let Some(binding) = window
                                .highest_precedence_binding_for_action_in(action.as_ref(), focus)
                            {
                                shortcuts.push((
                                    action.boxed_clone(),
                                    binding
                                        .keystrokes()
                                        .iter()
                                        .map(ToString::to_string)
                                        .collect::<Vec<_>>()
                                        .join(" "),
                                ));
                            }
                        }
                        OwnedMenuItem::Submenu(menu) => {
                            collect(&menu.items, window, focus, shortcuts)
                        }
                        _ => {}
                    }
                }
            }
            for menu in &self.menus {
                collect(&menu.items, window, &focus, &mut self.shortcuts);
            }
        }
        self.path = vec![0, 0];
        self.scroll.clear();
        window.focus(&self.focus);
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.path.clear();
        if let Some(focus) = self.previous_focus.take().and_then(|focus| focus.upgrade()) {
            window.focus(&focus);
        }
        cx.emit(MenuClosed);
        cx.notify();
    }

    fn items(&self, depth: usize) -> Option<&[OwnedMenuItem]> {
        let mut items = self.menus.get(*self.path.first()?)?.items.as_slice();
        for index in self.path.iter().skip(1).take(depth) {
            let OwnedMenuItem::Submenu(menu) = items.get(*index)? else {
                return None;
            };
            items = &menu.items;
        }
        Some(items)
    }

    fn select(&mut self, depth: usize, index: usize, cx: &mut Context<Self>) {
        self.path.truncate(depth + 1);
        self.path.push(index);
        if let Some(scroll) = self.scroll.get(depth) {
            scroll.scroll_to_item(index);
        }
        cx.notify();
    }

    fn activate(
        &mut self,
        depth: usize,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self
            .items(depth)
            .and_then(|items| items.get(index))
            .cloned()
        else {
            return;
        };
        match item {
            OwnedMenuItem::Submenu(_) => {
                self.select(depth, index, cx);
                self.path.push(0);
                cx.notify();
            }
            OwnedMenuItem::Action { action, .. } => {
                // Restore focus before validating or dispatching focus-sensitive actions.
                if !cx.menu_action_state(action.as_ref()).enabled {
                    return;
                }
                let map_action = self.map_action;
                self.close(window, cx);
                window.defer(cx, move |window, cx| {
                    if cx.menu_action_state(action.as_ref()).enabled {
                        window.dispatch_action(map_action(action), cx);
                    }
                });
            }
            _ => {}
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_open() {
            return;
        }
        cx.stop_propagation();
        window.prevent_default();
        let depth = self.path.len().saturating_sub(2);
        let index = *self.path.last().unwrap_or(&0);
        match event.keystroke.key.as_str() {
            "escape" | "f10" => self.close(window, cx),
            "enter" | "space" => self.activate(depth, index, window, cx),
            "right" => {
                if matches!(
                    self.items(depth).and_then(|items| items.get(index)),
                    Some(OwnedMenuItem::Submenu(_))
                ) {
                    self.activate(depth, index, window, cx);
                } else {
                    self.path = vec![(self.path[0] + 1) % self.menus.len(), 0];
                    cx.notify();
                }
            }
            "left" => {
                if depth > 0 {
                    self.path.pop();
                } else {
                    self.path = vec![(self.path[0] + self.menus.len() - 1) % self.menus.len(), 0];
                }
                cx.notify();
            }
            "up" | "down" | "home" | "end" => {
                let Some(items) = self.items(depth) else {
                    return;
                };
                let indices = items
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| {
                        !matches!(
                            item,
                            OwnedMenuItem::Separator | OwnedMenuItem::SystemMenu(_)
                        )
                    })
                    .map(|(i, _)| i)
                    .collect::<Vec<_>>();
                if indices.is_empty() {
                    return;
                }
                let current = indices
                    .iter()
                    .position(|candidate| *candidate == index)
                    .unwrap_or(0);
                let next = match event.keystroke.key.as_str() {
                    "home" => 0,
                    "end" => indices.len() - 1,
                    "up" => (current + indices.len() - 1) % indices.len(),
                    _ => (current + 1) % indices.len(),
                };
                self.select(depth, indices[next], cx);
            }
            key if key.chars().count() == 1 => {
                if let Some((index, _)) = self
                    .menus
                    .iter()
                    .enumerate()
                    .find(|(_, menu)| menu.name.to_lowercase().starts_with(&key.to_lowercase()))
                {
                    self.path = vec![index, 0];
                    cx.notify();
                }
            }
            _ => {}
        }
    }
}

impl Render for AppMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_open() {
            return div().into_any_element();
        }
        let s = &self.styles;
        let row_height = px(s.menu_item_height_px * self.scale);
        let font_size = px(s.menu_item_font_size_px * self.scale);
        let background = s.menu_bg;
        let foreground = s.menu_item_text_color;
        let hover = s.menu_item_hover_bg;
        let padding = px(s.menu_item_padding_x_px * self.scale);
        let width = px(s.menu_sidebar_width_px.max(s.menu_format_width_px) * self.scale);
        let mut columns = Vec::new();
        for depth in 0..self.path.len().saturating_sub(1) {
            let Some(items) = self.items(depth).map(<[_]>::to_vec) else {
                break;
            };
            if self.scroll.len() <= depth {
                self.scroll.push(ScrollHandle::new());
            }
            let mut rows = Vec::new();
            for (index, item) in items.into_iter().enumerate() {
                let (name, state, submenu, shortcut) = match item {
                    OwnedMenuItem::Action { name, action, .. } => {
                        let shortcut = self
                            .shortcuts
                            .iter()
                            .find(|(candidate, _)| candidate.partial_eq(action.as_ref()))
                            .map(|(_, shortcut)| shortcut.clone());
                        (name, cx.menu_action_state(action.as_ref()), false, shortcut)
                    }
                    OwnedMenuItem::Submenu(menu) => (
                        menu.name.to_string(),
                        AppMenuActionState {
                            enabled: true,
                            checked: None,
                        },
                        true,
                        None,
                    ),
                    OwnedMenuItem::Separator => {
                        rows.push(
                            div()
                                .h(px(s.menu_separator_height_px * self.scale))
                                .flex_none()
                                .into_any_element(),
                        );
                        continue;
                    }

                    OwnedMenuItem::SystemMenu(_) => continue,
                };
                let selected = self.path.get(depth + 1) == Some(&index);
                rows.push(
                    div()
                        .id(("app-menu-row", index))
                        .h(row_height)
                        .flex_none()
                        .px(padding)
                        .flex()
                        .items_center()
                        .text_color(foreground)
                        .when(selected, |row| row.bg(hover))
                        .opacity(if state.enabled {
                            1.0
                        } else {
                            s.menu_item_disabled_opacity
                        })
                        .child(div().w(font_size).flex_none().child(
                            if state.checked == Some(true) {
                                "✓"
                            } else {
                                ""
                            },
                        ))
                        .child(div().flex_1().min_w(px(0.0)).truncate().child(name))
                        .when(submenu, |row| row.child("›"))
                        .when_some(shortcut, |row, shortcut| {
                            row.child(
                                div()
                                    .flex_none()
                                    .pl(padding)
                                    .text_color(s.menu_shortcut_text_color)
                                    .child(shortcut),
                            )
                        })
                        .on_hover(cx.listener(move |this, hovered, _, cx| {
                            if *hovered {
                                this.select(depth, index, cx);
                                if submenu {
                                    this.path.push(0);
                                }
                            }
                        }))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                if state.enabled {
                                    this.activate(depth, index, window, cx);
                                }
                            }),
                        )
                        .into_any_element(),
                );
            }
            columns.push(
                div()
                    .id(("app-menu-column", depth))
                    .w(width)
                    .flex_none()
                    .max_h(window.bounds().size.height - row_height)
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll[depth])
                    .bg(background)
                    .border(px(s.menu_border_width_px))
                    .border_color(s.menu_border_color)
                    .children(rows),
            );
        }
        let bar = div()
            .id("app-menu-bar")
            .flex()
            .flex_nowrap()
            .overflow_x_scroll()
            .bg(background)
            .children(self.menus.iter().enumerate().map(|(index, menu)| {
                div()
                    .id(("app-menu-title", index))
                    .h(row_height)
                    .flex_none()
                    .px(padding)
                    .flex()
                    .items_center()
                    .when(self.path[0] == index, |item| item.bg(hover))
                    .child(menu.name.clone())
                    .on_hover(cx.listener(move |this, hovering, _, cx| {
                        if *hovering {
                            this.path = vec![index, 0];
                            cx.notify();
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.path = vec![index, 0];
                            cx.notify();
                        }),
                    )
            }));
        div()
            .id("application-menu")
            .absolute()
            .inset_0()
            .size_full()
            .occlude()
            .track_focus(&self.focus)
            .font_family(self.family)
            .text_size(font_size)
            .text_color(foreground)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.close(window, cx);
                }),
            )
            .child(
                div().absolute().top_0().left_0().w_full().child(bar).child(
                    div()
                        .id("app-menu-columns")
                        .flex()
                        .max_w_full()
                        .overflow_x_scroll()
                        .children(columns),
                ),
            )
            .into_any_element()
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use gpui::{AppContext, Entity, Keystroke, Menu, MenuItem, TestAppContext};
    use std::{cell::Cell, rc::Rc};

    struct Harness {
        menu: Entity<AppMenu>,
        focus: FocusHandle,
    }
    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .track_focus(&self.focus)
                .child(self.menu.clone())
        }
    }

    #[gpui::test]
    fn application_menu_keyboard_navigation_restores_focus_and_revalidates(
        cx: &mut TestAppContext,
    ) {
        let runs = Rc::new(Cell::new(0));
        let allowed = Rc::new(Cell::new(true));
        let (menu, focus) = cx.update(|cx| {
            let runs = runs.clone();
            cx.on_action(move |_: &crate::actions::PinTab, _| runs.set(runs.get() + 1));
            let allowed = allowed.clone();
            cx.set_menu_action_state_handler(move |_, _| {
                Some(AppMenuActionState {
                    enabled: allowed.get(),
                    checked: Some(false),
                })
            });
            cx.set_menus(vec![Menu {
                name: "Test".into(),
                items: vec![MenuItem::submenu(Menu {
                    name: "Submenu".into(),
                    items: vec![MenuItem::action("Pin", crate::actions::PinTab)],
                })],
            }]);
            let theme = crate::theme::ThemeKind::PaperWhite.build();
            let styles = crate::style::Styles::from_theme(&theme, &theme);
            (
                cx.new(|cx| {
                    AppMenu::new(
                        styles.context_menu.clone(),
                        1.5,
                        styles.ui_font_family,
                        |action| action,
                        cx,
                    )
                }),
                cx.focus_handle(),
            )
        });
        let (_, cx) = cx.add_window_view(|_, _| Harness {
            menu: menu.clone(),
            focus: focus.clone(),
        });
        cx.update(|window, cx| {
            window.focus(&focus);
            menu.update(cx, |menu, cx| menu.toggle(window, cx));
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            menu.update(cx, |menu, cx| {
                assert!(menu.is_open());
                assert_eq!(menu.path, [0, 0]);
                menu.on_key(
                    &KeyDownEvent {
                        keystroke: Keystroke::parse("right").unwrap(),
                        is_held: false,
                    },
                    window,
                    cx,
                );
                assert_eq!(menu.path, [0, 0, 0]);
                menu.on_key(
                    &KeyDownEvent {
                        keystroke: Keystroke::parse("enter").unwrap(),
                        is_held: false,
                    },
                    window,
                    cx,
                );
                assert!(!menu.is_open());
            });
        });
        cx.run_until_parked();
        assert_eq!(runs.get(), 1);
        cx.update(|window, cx| {
            assert!(focus.is_focused(window));
            menu.update(cx, |menu, cx| {
                menu.toggle(window, cx);
                menu.activate(0, 0, window, cx);
                allowed.set(false);
                menu.activate(1, 0, window, cx);
            });
        });
        cx.run_until_parked();
        assert_eq!(runs.get(), 1, "disabled command must not execute");
        cx.update(|window, cx| {
            menu.update(cx, |menu, cx| {
                menu.toggle(window, cx);
                menu.on_key(
                    &KeyDownEvent {
                        keystroke: Keystroke::parse("escape").unwrap(),
                        is_held: false,
                    },
                    window,
                    cx,
                );
                assert!(!menu.is_open());
            });
            assert!(focus.is_focused(window));
        });
    }
}
