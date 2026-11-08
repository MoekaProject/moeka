use std::collections::HashMap;

use gpui::{
    AnyElement, App, Context, Entity, Global, Pixels, Point, Render, WeakEntity, Window, WindowId,
    anchored, deferred, div, prelude::*,
};

use super::TextInput;

pub(super) fn release_input(id: gpui::EntityId, cx: &mut App) {
    let Some(menus) = cx.try_global::<InputContextMenus>() else {
        return;
    };
    let retired: Vec<_> = menus
        .0
        .iter()
        .filter(|(_, menu)| menu.read(cx).input.entity_id() == id)
        .map(|(id, _)| *id)
        .collect();
    for id in retired {
        cx.global_mut::<InputContextMenus>().0.remove(&id);
    }
}

#[derive(Default)]
struct InputContextMenus(HashMap<WindowId, Entity<InputContextMenu>>);
impl Global for InputContextMenus {}

struct InputContextMenu {
    input: WeakEntity<TextInput>,
    position: Point<Pixels>,
}

pub(super) fn open(
    input: WeakEntity<TextInput>,
    position: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    if cx.try_global::<InputContextMenus>().is_none() {
        cx.set_global(InputContextMenus::default());
        cx.on_window_closed(|cx| {
            let live: Vec<_> = cx
                .windows()
                .iter()
                .map(|window| window.window_id())
                .collect();
            cx.global_mut::<InputContextMenus>()
                .0
                .retain(|id, _| live.contains(id));
        })
        .detach();
    }
    let menu = cx.new(|_| InputContextMenu { input, position });
    let retired: Vec<_> = cx
        .global::<InputContextMenus>()
        .0
        .iter()
        .filter(|(_, menu)| menu.read(cx).input.upgrade().is_none())
        .map(|(id, _)| *id)
        .collect();
    for id in retired {
        cx.global_mut::<InputContextMenus>().0.remove(&id);
    }
    cx.global_mut::<InputContextMenus>()
        .0
        .insert(window.window_handle().window_id(), menu);
    window.refresh();
}

pub(crate) fn dismiss(window: &mut Window, cx: &mut App) -> bool {
    if cx.try_global::<InputContextMenus>().is_none() {
        return false;
    }
    let removed = cx
        .global_mut::<InputContextMenus>()
        .0
        .remove(&window.window_handle().window_id())
        .is_some();
    if removed {
        window.refresh();
    }
    removed
}

// Keep this at the window root: picker inputs already live in deferred overlays.
pub(crate) fn render_input_context_menu(window: &Window, cx: &mut App) -> Option<AnyElement> {
    let id = window.window_handle().window_id();
    let menu = cx.try_global::<InputContextMenus>()?.0.get(&id)?.clone();
    if !menu
        .read(cx)
        .input
        .upgrade()
        .is_some_and(|input| input.read(cx).focus_handle.is_focused(window))
    {
        cx.global_mut::<InputContextMenus>().0.remove(&id);
        return None;
    }
    Some(deferred(menu).with_priority(10).into_any_element())
}

impl Render for InputContextMenu {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(input) = self.input.upgrade() else {
            return div().into_any_element();
        };
        let menu = input.update(cx, |input, cx| {
            input.render_context_menu(cx).into_any_element()
        });
        anchored()
            .position(self.position)
            .child(
                div()
                    .id("text-input-context-menu-backdrop")
                    .occlude()
                    .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                    })
                    .on_mouse_down_out(cx.listener(|_, _, window, cx| {
                        dismiss(window, cx);
                    }))
                    .on_mouse_down(gpui::MouseButton::Right, |_, _, cx| cx.stop_propagation())
                    .child(menu),
            )
            .into_any_element()
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use crate::style::{InputRole, Styles};
    use gpui::{ClipboardItem, MouseButton, MouseDownEvent, point, px};

    struct Harness {
        input: Entity<TextInput>,
    }

    impl Render for Harness {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(deferred(
                    div()
                        .absolute()
                        .left(px(30.0))
                        .top(px(30.0))
                        .w(px(300.0))
                        .child(self.input.clone()),
                ))
                .when_some(render_input_context_menu(window, cx), |this, menu| {
                    this.child(menu)
                })
        }
    }

    #[gpui::test]
    fn all_input_roles_support_context_menu_actions_inside_deferred_overlays(
        cx: &mut gpui::TestAppContext,
    ) {
        for (theme_kind, scale) in [
            (crate::theme::ThemeKind::OneDark, 1.0),
            (crate::theme::ThemeKind::PaperWhite, 1.5),
        ] {
            for role in [
                InputRole::Stepper,
                InputRole::SidebarFilter,
                InputRole::Rename,
                InputRole::WorkspaceSearch,
                InputRole::Search,
                InputRole::Replace,
                InputRole::Picker,
                InputRole::QuickOpen,
                InputRole::Settings,
                InputRole::WorkspaceFilter,
                InputRole::ThemeDebugger,
            ] {
                let input = cx.update(|cx| {
                    cx.new(|cx| {
                        let theme = theme_kind.build();
                        let mut theme_styles =
                            Styles::from_theme_with_options(&theme, &theme, false);
                        theme_styles.context_menu.menu_item_height_px = 32.0;
                        theme_styles.context_menu.menu_format_icon_size_px = 19.0;
                        let styles = theme_styles.input_styles(role, scale);
                        assert_eq!(styles.menu_scale, scale);
                        assert_eq!(styles.menu.menu_bg, theme_styles.context_menu.menu_bg);
                        assert_eq!(styles.menu.menu_item_height_px, 32.0);
                        assert_eq!(styles.menu.menu_format_icon_size_px, 19.0);
                        let mut input = TextInput::new(styles, "Filter", cx);
                        input.set_text("hello 世界", cx);
                        input.set_selection(0, 5, cx);
                        input
                    })
                });
                let (_, window_cx) = cx.add_window_view(|_, _| Harness {
                    input: input.clone(),
                });
                window_cx.run_until_parked();
                let position =
                    window_cx.update(|_, cx| input.read(cx).last_bounds.unwrap().center());
                let open_menu = |window_cx: &mut gpui::VisualTestContext| {
                    window_cx.simulate_event(MouseDownEvent {
                        position,
                        button: MouseButton::Right,
                        ..Default::default()
                    });
                    window_cx.run_until_parked();
                    window_cx.update(|window, cx| {
                        assert!(render_input_context_menu(window, cx).is_some(), "{role:?}");
                        assert!(input.read(cx).focus_handle.is_focused(window));
                    });
                };
                let click_row = |window_cx: &mut gpui::VisualTestContext, row: usize| {
                    let click = window_cx.update(|_, cx| {
                        let styles = &input.read(cx).styles.menu;
                        let separator = if row == 3 {
                            styles.menu_separator_height_px
                                + 2.0 * styles.menu_separator_margin_y_px
                        } else {
                            0.0
                        };
                        position
                            + point(
                                px(20.0),
                                px(styles.menu_border_width_px
                                    + (styles.menu_padding_y_px
                                        + styles.menu_item_height_px * (row as f32 + 0.5)
                                        + separator)
                                        * scale),
                            )
                    });
                    window_cx.simulate_event(MouseDownEvent {
                        position: click,
                        button: MouseButton::Left,
                        ..Default::default()
                    });
                    window_cx.run_until_parked();
                };
                open_menu(window_cx);
                click_row(window_cx, 1);
                window_cx.update(|_, cx| {
                    assert_eq!(
                        cx.read_from_clipboard().unwrap().text().unwrap(),
                        "hello",
                        "{role:?}"
                    )
                });
                open_menu(window_cx);
                click_row(window_cx, 0);
                window_cx.update(|_, cx| assert_eq!(input.read(cx).text(), " 世界", "{role:?}"));
                window_cx
                    .update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("new".into())));
                open_menu(window_cx);
                click_row(window_cx, 2);
                window_cx.update(|_, cx| assert_eq!(input.read(cx).text(), "new 世界", "{role:?}"));
                open_menu(window_cx);
                click_row(window_cx, 3);
                window_cx.update(|_, cx| {
                    assert_eq!(
                        input.read(cx).selection_range(),
                        0.."new 世界".len(),
                        "{role:?}"
                    )
                });
                open_menu(window_cx);
                window_cx.simulate_keystrokes("escape");
                window_cx.run_until_parked();
                window_cx.update(|window, cx| {
                    assert!(render_input_context_menu(window, cx).is_none(), "{role:?}")
                });
                window_cx.update(|_, cx| {
                    input.update(cx, |input, cx| input.set_text("", cx));
                    cx.write_to_clipboard(ClipboardItem::new_string(String::new()));
                });
                window_cx.run_until_parked();
                for row in 0..4 {
                    open_menu(window_cx);
                    click_row(window_cx, row);
                    window_cx.update(|window, cx| {
                        assert_eq!(input.read(cx).text(), "");
                        assert_eq!(input.read(cx).selection_range(), 0..0);
                        assert!(render_input_context_menu(window, cx).is_some());
                    });
                }
                window_cx.simulate_event(MouseDownEvent {
                    position: point(px(1.0), px(1.0)),
                    button: MouseButton::Left,
                    ..Default::default()
                });
                window_cx.run_until_parked();
                window_cx.update(|window, cx| {
                    assert!(render_input_context_menu(window, cx).is_none());
                });
                open_menu(window_cx);
                window_cx.update(|window, _| window.blur());
                window_cx.run_until_parked();
                window_cx.update(|window, cx| {
                    assert!(render_input_context_menu(window, cx).is_none());
                });
            }
        }
    }

    struct EditorHarness(Entity<crate::editor::EditorView>);

    impl Render for EditorHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.0.clone()
        }
    }

    struct PlainHarness {
        input: Option<Entity<TextInput>>,
    }

    impl Render for PlainHarness {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .when_some(self.input.clone(), |this, input| this.child(input))
                .when_some(render_input_context_menu(window, cx), |this, menu| {
                    this.child(menu)
                })
        }
    }

    #[gpui::test]
    fn menus_are_window_local_and_released_with_windows_or_inputs(cx: &mut gpui::TestAppContext) {
        let mut windows = Vec::new();
        for _ in 0..2 {
            let (root, window_cx) = cx.add_window_view(|_, cx| {
                let theme = crate::theme::ThemeKind::OneDark.build();
                let styles = Styles::from_theme_with_options(&theme, &theme, false)
                    .input_styles(InputRole::Search, 1.0);
                PlainHarness {
                    input: Some(cx.new(|cx| TextInput::new(styles, "", cx))),
                }
            });
            window_cx.run_until_parked();
            let position = window_cx.update(|_, cx| {
                root.read(cx)
                    .input
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .last_bounds
                    .unwrap()
                    .center()
            });
            window_cx.simulate_event(MouseDownEvent {
                position,
                button: MouseButton::Right,
                ..Default::default()
            });
            window_cx.run_until_parked();
            let window = window_cx.update(|window, cx| {
                assert!(render_input_context_menu(window, cx).is_some());
                window.window_handle()
            });
            windows.push((root, window));
        }
        cx.update(|cx| {
            assert_eq!(cx.global::<InputContextMenus>().0.len(), 2);
            windows[0]
                .1
                .update(cx, |_, window, cx| {
                    assert!(dismiss(window, cx));
                })
                .unwrap();
            assert_eq!(cx.global::<InputContextMenus>().0.len(), 1);
            windows[1]
                .1
                .update(cx, |_, window, _| window.remove_window())
                .unwrap();
            assert!(cx.global::<InputContextMenus>().0.is_empty());
            windows[0]
                .1
                .update(cx, |_, window, cx| {
                    let styles = windows[0]
                        .0
                        .read(cx)
                        .input
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .styles
                        .clone();
                    let input = cx.new(|cx| TextInput::new(styles, "", cx));
                    input.read(cx).focus_handle.focus(window);
                    open(input.downgrade(), point(px(20.0), px(20.0)), window, cx);
                    assert_eq!(cx.global::<InputContextMenus>().0.len(), 1);
                })
                .unwrap();
        });
        cx.run_until_parked();
        cx.update(|cx| assert!(cx.global::<InputContextMenus>().0.is_empty()));
    }

    #[gpui::test]
    fn picker_context_menu_keeps_the_real_command_palette_and_quick_open_visible(
        cx: &mut gpui::TestAppContext,
    ) {
        let editor = cx.update(|cx| cx.new(crate::editor::EditorView::new_without_session));
        let (_, cx) = cx.add_window_view(|_, _| EditorHarness(editor.clone()));
        for palette in [true, false] {
            let input = cx.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    if palette {
                        editor.toggle_command_palette(window, cx);
                        editor.command_palette_editor.as_ref().unwrap().clone()
                    } else {
                        editor.toggle_quick_open(window, cx);
                        editor.quick_open_editor.as_ref().unwrap().clone()
                    }
                })
            });
            cx.update(|_, cx| {
                input.update(cx, |input, cx| {
                    input.set_text("copy me", cx);
                    input.select_all(cx);
                });
            });
            cx.run_until_parked();
            let position = cx.update(|_, cx| input.read(cx).last_bounds.unwrap().center());
            let right_click = |cx: &mut gpui::VisualTestContext| {
                cx.simulate_event(MouseDownEvent {
                    position,
                    button: MouseButton::Right,
                    ..Default::default()
                });
                cx.run_until_parked();
            };
            right_click(cx);
            let copy_position = cx.update(|window, cx| {
                assert!(render_input_context_menu(window, cx).is_some());
                let input = input.read(cx);
                let menu = &input.styles.menu;
                position
                    + point(
                        px(20.0),
                        px(menu.menu_border_width_px
                            + (menu.menu_padding_y_px + menu.menu_item_height_px * 1.5)
                                * input.styles.menu_scale),
                    )
            });
            cx.simulate_event(MouseDownEvent {
                position: copy_position,
                button: MouseButton::Left,
                ..Default::default()
            });
            cx.run_until_parked();
            cx.update(|_, cx| {
                assert_eq!(
                    cx.read_from_clipboard().and_then(|item| item.text()),
                    Some("copy me".into())
                );
                assert!(if palette {
                    editor.read(cx).command_palette_visible
                } else {
                    editor.read(cx).quick_open_visible
                });
            });
            right_click(cx);
            cx.simulate_keystrokes("escape");
            cx.run_until_parked();
            cx.update(|window, cx| {
                assert!(render_input_context_menu(window, cx).is_none());
                assert!(input.read(cx).focus_handle.is_focused(window));
                assert!(if palette {
                    editor.read(cx).command_palette_visible
                } else {
                    editor.read(cx).quick_open_visible
                });
            });
            cx.simulate_keystrokes("escape");
            cx.run_until_parked();
            cx.update(|_, cx| {
                assert!(!if palette {
                    editor.read(cx).command_palette_visible
                } else {
                    editor.read(cx).quick_open_visible
                });
            });
        }
    }
}
