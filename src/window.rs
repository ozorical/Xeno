use std::mem::size_of;

use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{Com::*, DataExchange::*, LibraryLoader::*, Memory::*, Ole::CF_UNICODETEXT, Registry::*},
        UI::{
            Accessibility::*,
            Controls::WM_MOUSELEAVE,
            HiDpi::GetDpiForWindow,
            Shell::*,
            WindowsAndMessaging::*,
        },
    },
    core::*,
};

use crate::{
    app::{self, Event},
    config::Config,
    keys,
};

const CLASS: PCWSTR = w!("Xeno");

pub fn create(config: &Config) -> Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: instance.into(),
            lpszClassName: CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hIcon: LoadIconW(Some(instance.into()), PCWSTR(1 as _)).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let r = area(MonitorFromPoint(cursor, MONITOR_DEFAULTTOPRIMARY), &config.fill);
        CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP,
            CLASS,
            w!("Xeno"),
            WS_POPUP,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
}

fn area(monitor: HMONITOR, fill: &str) -> RECT {
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        let _ = GetMonitorInfoW(monitor, &mut info);
    }
    if fill == "work" { info.rcWork } else { info.rcMonitor }
}

pub fn place(hwnd: HWND, fill: &str) {
    unsafe {
        let r = area(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), fill);
        let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

pub fn show(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
}

pub fn run() {
    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

pub fn forward(arg: Option<&str>) -> bool {
    let Ok(hwnd) = (unsafe { FindWindowW(CLASS, None) }) else { return false };
    if hwnd.is_invalid() {
        return false;
    }
    let text: Vec<u16> = arg.unwrap_or_default().encode_utf16().collect();
    let data = COPYDATASTRUCT { dwData: 1, cbData: (text.len() * 2) as u32, lpData: text.as_ptr() as *mut _ };
    unsafe {
        SendMessageW(hwnd, WM_COPYDATA, None, Some(LPARAM(&data as *const _ as isize)));
        let _ = SetForegroundWindow(hwnd);
    }
    true
}

pub fn size(hwnd: HWND) -> (i32, i32, f32) {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut r);
        (r.right - r.left, r.bottom - r.top, GetDpiForWindow(hwnd) as f32 / 96.0)
    }
}

pub fn title(hwnd: HWND, text: &str) {
    let text = if text.is_empty() { "Xeno".to_string() } else { format!("{text} - Xeno") };
    unsafe {
        let _ = SetWindowTextW(hwnd, &HSTRING::from(text));
    }
}

pub fn timer(hwnd: HWND, id: usize, ms: u32) {
    unsafe {
        SetTimer(Some(hwnd), id, ms, None);
    }
}

pub fn stop(hwnd: HWND, id: usize) {
    unsafe {
        let _ = KillTimer(Some(hwnd), id);
    }
}

pub fn dark() -> bool {
    let mut value = 1u32;
    let mut len = 4u32;
    unsafe {
        let _ = RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut len),
        );
    }
    value == 0
}

pub fn copy(hwnd: HWND, text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    unsafe {
        if OpenClipboard(Some(hwnd)).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            let ptr = GlobalLock(mem) as *mut u16;
            if !ptr.is_null() {
                ptr.copy_from_nonoverlapping(wide.as_ptr(), wide.len());
                let _ = GlobalUnlock(mem);
                let _ = SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(mem.0)));
            }
        }
        let _ = CloseClipboard();
    }
}

pub fn reveal(path: &str) {
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(path), None, None, SW_SHOWNORMAL);
    }
}

pub fn folder(hwnd: HWND) -> Option<String> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        dialog.SetOptions(FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM).ok()?;
        dialog.SetTitle(w!("Choose an unpacked extension")).ok()?;
        dialog.Show(Some(hwnd)).ok()?;
        let path = dialog.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = path.to_string().ok();
        CoTaskMemFree(Some(path.0 as _));
        s
    }
}

fn low(v: usize) -> u32 {
    (v & 0xffff) as u32
}

extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEMOVE..=WM_MOUSEHWHEEL | WM_MOUSELEAVE => {
                app::with(|a| a.mouse(msg, wp, lp));
                LRESULT(0)
            }
            WM_SETCURSOR if low(lp.0 as usize) == HTCLIENT => {
                if app::with(|a| a.cursor()).unwrap_or(false) {
                    LRESULT(1)
                } else {
                    DefWindowProcW(hwnd, msg, wp, lp)
                }
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => match keys::lookup(wp.0 as u32) {
                Some(action) => {
                    app::emit(Event::Action(action, lp.0 & (1 << 30) != 0));
                    LRESULT(0)
                }
                None => DefWindowProcW(hwnd, msg, wp, lp),
            },
            WM_SYSCOMMAND if (wp.0 & 0xfff0) as u32 == SC_KEYMENU => LRESULT(0),
            WM_SIZE => {
                app::with(|a| a.resize());
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let r = &*(lp.0 as *const RECT);
                let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                LRESULT(0)
            }
            WM_ACTIVATE => {
                if low(wp.0) != WA_INACTIVE {
                    app::with(|a| a.refocus());
                }
                LRESULT(0)
            }
            WM_GETOBJECT if lp.0 as i32 == UiaRootObjectId => {
                let provider = app::with(|a| a.automation()).flatten();
                match provider.and_then(|p| p.cast::<IRawElementProviderSimple>().ok()) {
                    Some(p) => UiaReturnRawElementProvider(hwnd, wp, lp, &p),
                    None => DefWindowProcW(hwnd, msg, wp, lp),
                }
            }
            WM_TIMER => {
                app::emit(Event::Timer(wp.0));
                LRESULT(0)
            }
            WM_APP => {
                app::inbox();
                LRESULT(0)
            }
            WM_COPYDATA => {
                let data = &*(lp.0 as *const COPYDATASTRUCT);
                let text = std::slice::from_raw_parts(data.lpData as *const u16, data.cbData as usize / 2);
                app::emit(Event::Open(String::from_utf16_lossy(text)));
                LRESULT(1)
            }
            WM_SETTINGCHANGE => {
                if lp.0 != 0 && PCWSTR(lp.0 as *const u16).to_string().is_ok_and(|s| s == "ImmersiveColorSet") {
                    app::emit(Event::Theme);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_CLOSE => {
                app::emit(Event::Quit);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
