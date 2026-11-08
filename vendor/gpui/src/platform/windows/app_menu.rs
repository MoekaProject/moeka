use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

use anyhow::Result;
use util::ResultExt;
use windows::{
    Win32::{
        Foundation::*,
        UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
    core::HSTRING,
};

use super::platform::WindowsPlatformInner;
use crate::{Action, AppMenuActionState, OwnedMenuItem};

const HIDE_MENU: u32 = WM_APP + 73;
const FIRST_COMMAND: usize = 0x6000;

thread_local! {
    static PLATFORM: RefCell<Weak<WindowsPlatformInner>> = RefCell::new(Weak::new());
    static MENU: RefCell<Option<NativeMenu>> = const { RefCell::new(None) };
    static ALT_CANDIDATE: RefCell<Option<HWND>> = const { RefCell::new(None) };
}

struct NativeMenu {
    window: HWND,
    handle: HMENU,
    actions: Vec<(HMENU, u32, Box<dyn Action>)>,
}

impl Drop for NativeMenu {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(Some(self.window)).as_bool() {
                SetMenu(self.window, None).log_err();
                DrawMenuBar(self.window).log_err();
                SetWindowPos(
                    self.window,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                )
                .log_err();
            }
            DestroyMenu(self.handle).log_err();
        }
    }
}

pub(super) fn configure(platform: &Rc<WindowsPlatformInner>) {
    PLATFORM.with(|slot| *slot.borrow_mut() = Rc::downgrade(platform));
}

fn validate(platform: &WindowsPlatformInner, action: &dyn Action) -> AppMenuActionState {
    let callback = platform
        .state
        .borrow_mut()
        .callbacks
        .validate_app_menu_command
        .take();
    let Some(mut callback) = callback else {
        return AppMenuActionState::default();
    };
    let result = callback(action);
    platform
        .state
        .borrow_mut()
        .callbacks
        .validate_app_menu_command = Some(callback);
    result
}

fn append(
    menu: HMENU,
    items: &[OwnedMenuItem],
    actions: &mut Vec<(HMENU, u32, Box<dyn Action>)>,
    platform: &WindowsPlatformInner,
) -> Result<()> {
    for item in items {
        unsafe {
            match item {
                OwnedMenuItem::Separator => AppendMenuW(menu, MF_SEPARATOR, 0, None)?,
                OwnedMenuItem::SystemMenu(_) => {}
                OwnedMenuItem::Submenu(submenu) => {
                    let child = CreatePopupMenu()?;
                    if let Err(error) = append(child, &submenu.items, actions, platform) {
                        DestroyMenu(child).log_err();
                        return Err(error);
                    }
                    AppendMenuW(
                        menu,
                        MF_POPUP,
                        child.0 as usize,
                        &HSTRING::from(submenu.name.as_ref()),
                    )?;
                }
                OwnedMenuItem::Action { name, action, .. } => {
                    let id = u32::try_from(FIRST_COMMAND + actions.len())?;
                    anyhow::ensure!(id <= u16::MAX as u32, "Too many native menu commands");
                    let state = validate(platform, action.as_ref());
                    let mut flags = MF_STRING;
                    if !state.enabled {
                        flags |= MF_GRAYED;
                    }
                    if state.checked == Some(true) {
                        flags |= MF_CHECKED;
                    }
                    let label = platform
                        .state
                        .borrow()
                        .menu_shortcuts
                        .iter()
                        .find(|(candidate, _)| candidate.partial_eq(action.as_ref()))
                        .map(|(_, shortcut)| format!("{name}\t{shortcut}"))
                        .unwrap_or_else(|| name.clone());
                    AppendMenuW(menu, flags, id as usize, &HSTRING::from(label))?;
                    actions.push((menu, id, action.boxed_clone()));
                }
            }
        }
    }
    Ok(())
}

fn open(window: HWND) -> Result<()> {
    let platform = PLATFORM.with(|slot| slot.borrow().upgrade());
    let Some(platform) = platform else {
        return Ok(());
    };
    let callback = platform
        .state
        .borrow_mut()
        .callbacks
        .will_open_app_menu
        .take();
    if let Some(mut callback) = callback {
        callback();
        platform.state.borrow_mut().callbacks.will_open_app_menu = Some(callback);
    }
    close();
    let menus = platform.state.borrow().menus.clone();
    if menus.is_empty() {
        return Ok(());
    }
    let handle = unsafe { CreateMenu()? };
    let mut native = NativeMenu {
        window,
        handle,
        actions: Vec::new(),
    };
    for menu in menus {
        let child = unsafe { CreatePopupMenu()? };
        if let Err(error) = append(child, &menu.items, &mut native.actions, &platform) {
            unsafe {
                DestroyMenu(child).log_err();
            }
            return Err(error);
        }
        unsafe {
            AppendMenuW(
                handle,
                MF_POPUP,
                child.0 as usize,
                &HSTRING::from(menu.name.as_ref()),
            )?;
        }
    }
    MENU.with(|slot| *slot.borrow_mut() = Some(native));
    unsafe {
        SetMenu(window, Some(handle))?;
        DrawMenuBar(window)?;
        SetWindowPos(
            window,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        )?;
        PostMessageW(
            Some(window),
            WM_SYSCOMMAND,
            WPARAM(SC_KEYMENU as usize),
            LPARAM(0),
        )?;
    }
    Ok(())
}

fn close() {
    let old = MENU.with(|slot| slot.borrow_mut().take());
    drop(old);
}

pub(super) fn client_inset(window: HWND) -> i32 {
    if unsafe { GetMenu(window) }.is_invalid() {
        return 0;
    }
    let mut info = MENUBARINFO {
        cbSize: std::mem::size_of::<MENUBARINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMenuBarInfo(window, OBJID_MENU, 0, &mut info) }.is_ok()
        && info.rcBar.bottom > info.rcBar.top
    {
        info.rcBar.bottom - info.rcBar.top
    } else {
        unsafe { GetSystemMetricsForDpi(SM_CYMENU, GetDpiForWindow(window)) }
    }
}

pub(super) fn handle_message(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    let active = MENU.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|menu| menu.window == window)
    });
    match message {
        WM_SYSKEYDOWN | WM_KEYDOWN if !active => {
            let control = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
            let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
            if wparam.0 == VK_MENU.0 as usize && !control && !shift && (lparam.0 & (1 << 30)) == 0 {
                ALT_CANDIDATE.with(|slot| *slot.borrow_mut() = Some(window));
            } else {
                ALT_CANDIDATE.with(|slot| slot.borrow_mut().take());
                if wparam.0 == VK_F10.0 as usize
                    && !control
                    && !shift
                    && unsafe { GetKeyState(VK_MENU.0 as i32) } >= 0
                {
                    open(window).log_err();
                    return Some(LRESULT(0));
                }
            }
        }
        WM_SYSKEYUP if !active && wparam.0 == VK_MENU.0 as usize => {
            if ALT_CANDIDATE.with(|slot| slot.borrow_mut().take()) == Some(window) {
                open(window).log_err();
                return Some(LRESULT(0));
            }
        }
        WM_SYSCOMMAND if active && wparam.0 == SC_KEYMENU as usize => {
            return Some(unsafe { DefWindowProcW(window, message, wparam, lparam) });
        }
        WM_INITMENUPOPUP if active => {
            let platform = PLATFORM.with(|slot| slot.borrow().upgrade());
            if let Some(platform) = platform {
                let actions = MENU
                    .with(|slot| {
                        slot.borrow().as_ref().map(|menu| {
                            menu.actions
                                .iter()
                                .map(|(handle, id, action)| (*handle, *id, action.boxed_clone()))
                                .collect::<Vec<_>>()
                        })
                    })
                    .unwrap_or_default();
                for (menu, id, action) in actions {
                    let state = validate(&platform, action.as_ref());
                    unsafe {
                        let previous = EnableMenuItem(
                            menu,
                            id,
                            MF_BYCOMMAND | if state.enabled { MF_ENABLED } else { MF_GRAYED },
                        );
                        if previous.0 == -1 {
                            log::error!("Could not update native menu command availability: {id}");
                        }
                        let previous = CheckMenuItem(
                            menu,
                            id,
                            (MF_BYCOMMAND
                                | if state.checked == Some(true) {
                                    MF_CHECKED
                                } else {
                                    MF_UNCHECKED
                                })
                            .0,
                        );
                        if previous == u32::MAX {
                            log::error!("Could not update native menu checkmark: {id}");
                        }
                    }
                }
            }
        }
        WM_COMMAND if active && lparam.0 == 0 => {
            let id = (wparam.0 & 0xffff) as u32;
            let action = MENU.with(|slot| {
                slot.borrow().as_ref().and_then(|menu| {
                    menu.actions
                        .iter()
                        .find(|(_, candidate, _)| *candidate == id)
                        .map(|(_, _, action)| action.boxed_clone())
                })
            });
            if let Some(action) = action {
                close();
                if let Some(platform) = PLATFORM.with(|slot| slot.borrow().upgrade())
                    && validate(&platform, action.as_ref()).enabled
                {
                    let callback = platform.state.borrow_mut().callbacks.app_menu_action.take();
                    if let Some(mut callback) = callback {
                        callback(action.as_ref());
                        platform.state.borrow_mut().callbacks.app_menu_action = Some(callback);
                    }
                }
                return Some(LRESULT(0));
            }
        }
        WM_EXITMENULOOP if active => unsafe {
            // WM_COMMAND follows menu-loop exit; do not destroy its ID map early.
            PostMessageW(Some(window), HIDE_MENU, WPARAM(0), LPARAM(0)).log_err();
        },
        HIDE_MENU if active => {
            close();
            return Some(LRESULT(0));
        }
        WM_NCDESTROY if active => {
            close();
        }
        WM_KILLFOCUS => {
            ALT_CANDIDATE.with(|slot| slot.borrow_mut().take());
        }
        _ => {}
    }
    None
}
