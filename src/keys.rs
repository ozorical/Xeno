use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

use windows::Win32::UI::Input::KeyboardAndMouse::*;

use crate::config::Config;

#[derive(Clone, Copy, PartialEq)]
pub enum Action {
    Bar,
    Switch,
    SwitchBack,
    New,
    Close,
    Reopen,
    Settings,
    Next,
    Prev,
    Copy,
    Jump(usize),
}

const NAMES: [(&str, Action); 10] = [
    ("bar", Action::Bar),
    ("switch", Action::Switch),
    ("switch_back", Action::SwitchBack),
    ("new", Action::New),
    ("close", Action::Close),
    ("reopen", Action::Reopen),
    ("settings", Action::Settings),
    ("next", Action::Next),
    ("prev", Action::Prev),
    ("copy", Action::Copy),
];

thread_local! {
    static MAP: RefCell<HashMap<String, Action>> = RefCell::new(HashMap::new());
    static PAUSED: Cell<bool> = const { Cell::new(false) };
}

pub fn load(config: &Config) {
    let mut map = HashMap::new();
    for n in 1..=9 {
        map.insert(format!("Alt+{n}"), Action::Jump(n));
    }
    for (name, action) in NAMES {
        let combos = config.keys.get(name).map(String::as_str).unwrap_or_default();
        for combo in combos.split(',').map(str::trim).filter(|c| !c.is_empty()) {
            map.insert(combo.to_string(), action);
        }
    }
    MAP.with(|m| *m.borrow_mut() = map);
}

pub fn pause(on: bool) {
    PAUSED.set(on);
}

pub fn lookup(vk: u32) -> Option<Action> {
    if PAUSED.get() {
        return None;
    }
    let combo = combo(vk)?;
    MAP.with(|m| m.borrow().get(&combo).copied())
}

fn down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetKeyState(vk.0 as i32) < 0 }
}

pub fn held() -> bool {
    unsafe { GetAsyncKeyState(VK_MENU.0 as i32) < 0 || GetAsyncKeyState(VK_CONTROL.0 as i32) < 0 }
}

fn combo(vk: u32) -> Option<String> {
    let key = name(vk)?;
    let mut s = String::new();
    if down(VK_CONTROL) {
        s += "Ctrl+";
    }
    if down(VK_MENU) {
        s += "Alt+";
    }
    if down(VK_SHIFT) {
        s += "Shift+";
    }
    Some(s + &key)
}

fn name(vk: u32) -> Option<String> {
    if (0x30..=0x39).contains(&vk) || (0x41..=0x5A).contains(&vk) {
        return Some(char::from(vk as u8).to_string());
    }
    if (0x70..=0x87).contains(&vk) {
        return Some(format!("F{}", vk - 0x6F));
    }
    let name = match VIRTUAL_KEY(vk as u16) {
        VK_TAB => "Tab",
        VK_RETURN => "Enter",
        VK_ESCAPE => "Esc",
        VK_SPACE => "Space",
        VK_LEFT => "Left",
        VK_RIGHT => "Right",
        VK_UP => "Up",
        VK_DOWN => "Down",
        VK_BACK => "Backspace",
        VK_DELETE => "Delete",
        VK_HOME => "Home",
        VK_END => "End",
        VK_PRIOR => "PageUp",
        VK_NEXT => "PageDown",
        VK_OEM_COMMA => ",",
        VK_OEM_PERIOD => ".",
        VK_OEM_2 => "/",
        VK_OEM_MINUS => "-",
        VK_OEM_PLUS => "=",
        VK_OEM_1 => ";",
        VK_OEM_4 => "[",
        VK_OEM_6 => "]",
        VK_OEM_7 => "'",
        VK_OEM_3 => "`",
        VK_OEM_5 => "\\",
        _ => return None,
    };
    Some(name.to_string())
}
