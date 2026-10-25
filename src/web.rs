use std::{ffi::c_void, mem::size_of};

use serde_json::Value;
use webview2_com::{
    AcceleratorKeyPressedEventHandler, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2CompositionControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, CursorChangedEventHandler,
    DOMContentLoadedEventHandler, DocumentTitleChangedEventHandler, FaviconChangedEventHandler,
    HistoryChangedEventHandler, IsDocumentPlayingAudioChangedEventHandler,
    Microsoft::Web::WebView2::Win32::*, NavigationCompletedEventHandler,
    NavigationStartingEventHandler, NewWindowRequestedEventHandler, ProcessFailedEventHandler,
    SourceChangedEventHandler, StatusBarTextChangedEventHandler, TrySuspendCompletedHandler, WebMessageReceivedEventHandler,
    WindowCloseRequestedEventHandler, take_pwstr,
};
use windows::{
    UI::Composition::ContainerVisual,
    Win32::{
        Foundation::*,
        System::{LibraryLoader::*, ProcessStatus::*, Registry::*, Threading::*},
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::HCURSOR},
    },
    core::*,
};

use crate::{
    app::{Event, Target, emit},
    config::{Config, dir},
    keys,
};

type Create = unsafe extern "system" fn(u32, u32, PCWSTR, *mut c_void, *mut c_void) -> HRESULT;

#[derive(Clone)]
pub struct View {
    pub comp: ICoreWebView2CompositionController,
    pub ctl: ICoreWebView2Controller,
    pub web: ICoreWebView2,
}

pub fn text(f: impl FnOnce(*mut PWSTR) -> Result<()>) -> String {
    let mut p = PWSTR::null();
    match f(&mut p) {
        Ok(()) if !p.is_null() => take_pwstr(p),
        _ => String::new(),
    }
}

pub fn reg(root: HKEY, key: &str, name: &str) -> Option<String> {
    let mut buf = [0u16; 512];
    let mut len = size_of::<[u16; 512]>() as u32;
    unsafe {
        RegGetValueW(
            root,
            &HSTRING::from(key),
            &HSTRING::from(name),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
        .ok()
        .ok()?;
    }
    Some(String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]))
}

fn runtime() -> Option<String> {
    let id = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    [
        (HKEY_LOCAL_MACHINE, format!(r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{id}")),
        (HKEY_CURRENT_USER, format!(r"Software\Microsoft\EdgeUpdate\Clients\{id}")),
    ]
    .into_iter()
    .find_map(|(root, key)| {
        let path = format!(
            r"{}\{}\EBWebView\x64\EmbeddedBrowserWebView.dll",
            reg(root, &key, "location")?,
            reg(root, &key, "pv")?
        );
        std::path::Path::new(&path).exists().then_some(path)
    })
}

pub fn environment(config: &Config) -> Result<()> {
    let dll = runtime().ok_or_else(|| Error::from(ERROR_FILE_NOT_FOUND.to_hresult()))?;
    let create: Create = unsafe {
        let module = LoadLibraryExW(&HSTRING::from(dll), None, LOAD_WITH_ALTERED_SEARCH_PATH)?;
        let proc = GetProcAddress(module, s!("CreateWebViewEnvironmentWithOptionsInternal"))
            .ok_or_else(|| Error::from(ERROR_PROC_NOT_FOUND.to_hresult()))?;
        std::mem::transmute(proc)
    };

    let options = CoreWebView2EnvironmentOptions::default();
    unsafe {
        options.set_additional_browser_arguments(config.args());
        options.set_are_browser_extensions_enabled(true);
        if config.scrollbars {
            options.set_scroll_bar_style(COREWEBVIEW2_SCROLLBAR_STYLE_FLUENT_OVERLAY);
        }
    }
    let options: ICoreWebView2EnvironmentOptions = options.into();
    let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(|hr, env| {
        emit(match (hr, env) {
            (Ok(()), Some(env)) => Event::Env(env),
            _ => Event::NoEnv,
        });
        Ok(())
    }));
    let data = HSTRING::from(dir().join("Profile").to_string_lossy().as_ref());
    unsafe { create(1, 1, PCWSTR(data.as_ptr()), options.as_raw(), handler.as_raw()).ok() }
}

pub fn version(env: &ICoreWebView2Environment) -> String {
    text(|p| unsafe { env.BrowserVersionString(p) })
}

pub fn spawn(env: &ICoreWebView2Environment, hwnd: HWND, target: Target) -> Result<()> {
    let env: ICoreWebView2Environment3 = env.cast()?;
    let handler = CreateCoreWebView2CompositionControllerCompletedHandler::create(Box::new(move |hr, comp| {
        emit(match (hr, comp) {
            (Ok(()), Some(comp)) => Event::Created(target, comp),
            _ => Event::Failed(target),
        });
        Ok(())
    }));
    unsafe { env.CreateCoreWebView2CompositionController(hwnd, &handler) }
}

fn private(pid: u32) -> u64 {
    unsafe {
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return 0;
        };
        let mut counters = PROCESS_MEMORY_COUNTERS_EX2 {
            cb: size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
            ..Default::default()
        };
        let ok = K32GetProcessMemoryInfo(process, &mut counters as *mut _ as *mut PROCESS_MEMORY_COUNTERS, counters.cb);
        let _ = CloseHandle(process);
        if ok.as_bool() { counters.PrivateWorkingSetSize as u64 } else { 0 }
    }
}

pub fn memory(env: &ICoreWebView2Environment) -> (u64, u32) {
    let mut total = private(std::process::id());
    let mut count = 1;
    if let Ok(list) = env.cast::<ICoreWebView2Environment8>().and_then(|e| unsafe { e.GetProcessInfos() }) {
        let mut n = 0;
        unsafe {
            let _ = list.Count(&mut n);
            for i in 0..n {
                let mut pid = 0;
                if list.GetValueAtIndex(i).and_then(|info| info.ProcessId(&mut pid)).is_ok() {
                    total += private(pid as u32);
                    count += 1;
                }
            }
        }
    }
    (total, count)
}

impl View {
    pub fn new(comp: ICoreWebView2CompositionController) -> Result<Self> {
        let ctl: ICoreWebView2Controller = comp.cast()?;
        let web = unsafe { ctl.CoreWebView2()? };
        Ok(Self { comp, ctl, web })
    }

    pub fn attach(&self, visual: &ContainerVisual, w: i32, h: i32) -> Result<()> {
        unsafe { self.comp.SetRootVisualTarget(visual)? };
        self.bounds(w, h)
    }

    pub fn bounds(&self, w: i32, h: i32) -> Result<()> {
        unsafe { self.ctl.SetBounds(RECT { left: 0, top: 0, right: w, bottom: h }) }
    }

    pub fn show(&self, on: bool) {
        unsafe {
            let _ = self.ctl.SetIsVisible(on);
        }
    }

    pub fn focus(&self) {
        unsafe {
            let _ = self.ctl.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
        }
    }

    pub fn navigate(&self, url: &str) {
        unsafe {
            let _ = self.web.Navigate(&HSTRING::from(url));
        }
    }

    pub fn html(&self, html: &str) {
        unsafe {
            let _ = self.web.NavigateToString(&HSTRING::from(html));
        }
    }

    pub fn url(&self) -> String {
        text(|p| unsafe { self.web.Source(p) })
    }

    pub fn title(&self) -> String {
        text(|p| unsafe { self.web.DocumentTitle(p) })
    }

    pub fn icon(&self) -> String {
        self.web.cast::<ICoreWebView2_15>().map_or_else(|_| String::new(), |w| text(|p| unsafe { w.FaviconUri(p) }))
    }

    pub fn status(&self) -> String {
        self.web.cast::<ICoreWebView2_12>().map_or_else(|_| String::new(), |w| text(|p| unsafe { w.StatusBarText(p) }))
    }

    pub fn post(&self, v: &Value) {
        unsafe {
            let _ = self.web.PostWebMessageAsJson(&HSTRING::from(v.to_string()));
        }
    }

    pub fn close(&self) {
        unsafe {
            let _ = self.ctl.Close();
        }
    }

    pub fn reload(&self) {
        unsafe {
            let _ = self.web.Reload();
        }
    }

    pub fn back(&self, forward: bool) {
        unsafe {
            let _ = if forward { self.web.GoForward() } else { self.web.GoBack() };
        }
    }

    pub fn devtools(&self) {
        unsafe {
            let _ = self.web.OpenDevToolsWindow();
        }
    }

    pub fn print(&self) {
        if let Ok(w) = self.web.cast::<ICoreWebView2_16>() {
            unsafe {
                let _ = w.ShowPrintUI(COREWEBVIEW2_PRINT_DIALOG_KIND_BROWSER);
            }
        }
    }

    pub fn zoom(&self, factor: Option<f64>) {
        let mut z = 1.0;
        unsafe {
            let _ = self.ctl.ZoomFactor(&mut z);
            let _ = self.ctl.SetZoomFactor(factor.map_or(1.0, |f| (z * f).clamp(0.25, 5.0)));
        }
    }

    pub fn set_zoom(&self, z: f64) {
        unsafe {
            let _ = self.ctl.SetZoomFactor(z);
        }
    }

    pub fn audible(&self) -> bool {
        let mut on = BOOL::default();
        self.web.cast::<ICoreWebView2_8>().is_ok_and(|w| unsafe { w.IsDocumentPlayingAudio(&mut on) }.is_ok()) && on.as_bool()
    }

    pub fn mute(&self, on: bool) {
        if let Ok(w) = self.web.cast::<ICoreWebView2_8>() {
            unsafe {
                let _ = w.SetIsMuted(on);
            }
        }
    }

    pub fn thrift(&self, low: bool) {
        if let Ok(w) = self.web.cast::<ICoreWebView2_19>() {
            let level = if low { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL };
            unsafe {
                let _ = w.SetMemoryUsageTargetLevel(level);
            }
        }
    }

    pub fn suspend(&self, id: u32) {
        if let Ok(w) = self.web.cast::<ICoreWebView2_3>() {
            let handler = TrySuspendCompletedHandler::create(Box::new(move |hr, ok| {
                emit(Event::Slept(id, hr.is_ok() && ok));
                Ok(())
            }));
            unsafe {
                let _ = w.TrySuspend(&handler);
            }
        }
    }

    pub fn resume(&self) {
        if let Ok(w) = self.web.cast::<ICoreWebView2_3>() {
            unsafe {
                let _ = w.Resume();
            }
        }
    }

    pub fn profile(&self) -> Option<ICoreWebView2Profile> {
        unsafe { self.web.cast::<ICoreWebView2_13>().ok()?.Profile().ok() }
    }

    pub fn cursor(&self) -> HCURSOR {
        let mut c = HCURSOR::default();
        unsafe {
            let _ = self.comp.Cursor(&mut c);
        }
        c
    }

    pub fn mouse(&self, kind: u32, keys: u32, data: u32, pt: POINT) {
        unsafe {
            let _ = self.comp.SendMouseInput(
                COREWEBVIEW2_MOUSE_EVENT_KIND(kind as i32),
                COREWEBVIEW2_MOUSE_EVENT_VIRTUAL_KEYS(keys as i32),
                data,
                pt,
            );
        }
    }

    pub fn automation(&self) -> Option<IUnknown> {
        unsafe { self.comp.cast::<ICoreWebView2CompositionController2>().ok()?.AutomationProvider().ok() }
    }

    pub fn tab(&self, id: u32, config: &Config) -> Result<()> {
        let web = &self.web;
        let mut token = 0;
        unsafe {
            self.ctl.SetZoomFactor(config.zoom)?;
            web.Settings()?.SetIsStatusBarEnabled(false)?;
            web.cast::<ICoreWebView2_12>()?.add_StatusBarTextChanged(
                &StatusBarTextChangedEventHandler::create(Box::new(move |_, _| {
                    emit(Event::Status(id));
                    Ok(())
                })),
                &mut token,
            )?;
            web.add_DocumentTitleChanged(
                &DocumentTitleChangedEventHandler::create(Box::new(move |_, _| {
                    emit(Event::Title(id));
                    Ok(())
                })),
                &mut token,
            )?;
            web.add_SourceChanged(
                &SourceChangedEventHandler::create(Box::new(move |_, _| {
                    emit(Event::Url(id));
                    Ok(())
                })),
                &mut token,
            )?;
            web.add_HistoryChanged(
                &HistoryChangedEventHandler::create(Box::new(move |_, _| {
                    emit(Event::Url(id));
                    Ok(())
                })),
                &mut token,
            )?;
            web.add_NavigationStarting(
                &NavigationStartingEventHandler::create(Box::new(move |_, _| {
                    emit(Event::Started(id));
                    Ok(())
                })),
                &mut token,
            )?;
            web.add_NavigationCompleted(
                &NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                    let mut ok = BOOL::default();
                    if let Some(args) = args {
                        args.IsSuccess(&mut ok)?;
                    }
                    emit(Event::Finished(id, ok.as_bool()));
                    Ok(())
                })),
                &mut token,
            )?;
            web.cast::<ICoreWebView2_2>()?.add_DOMContentLoaded(
                &DOMContentLoadedEventHandler::create(Box::new(move |_, _| {
                    emit(Event::Loaded(id));
                    Ok(())
                })),
                &mut token,
            )?;
            web.cast::<ICoreWebView2_15>()?.add_FaviconChanged(
                &FaviconChangedEventHandler::create(Box::new(move |_, _| {
                    emit(Event::Icon(id));
                    Ok(())
                })),
                &mut token,
            )?;
            web.cast::<ICoreWebView2_8>()?.add_IsDocumentPlayingAudioChanged(
                &IsDocumentPlayingAudioChangedEventHandler::create(Box::new(move |_, _| {
                    emit(Event::Audio(id));
                    Ok(())
                })),
                &mut token,
            )?;
            web.add_NewWindowRequested(
                &NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut user = BOOL::default();
                    args.IsUserInitiated(&mut user)?;
                    let quiet = GetAsyncKeyState(VK_CONTROL.0 as i32) < 0 || GetAsyncKeyState(VK_MBUTTON.0 as i32) < 0;
                    let deferral = args.GetDeferral()?;
                    emit(Event::Popup { args, deferral, user: user.as_bool(), quiet });
                    Ok(())
                })),
                &mut token,
            )?;
            web.add_WindowCloseRequested(
                &WindowCloseRequestedEventHandler::create(Box::new(move |_, _| {
                    emit(Event::CloseTab(id));
                    Ok(())
                })),
                &mut token,
            )?;
            web.add_ProcessFailed(
                &ProcessFailedEventHandler::create(Box::new(move |_, args| {
                    let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                    if let Some(args) = args {
                        args.ProcessFailedKind(&mut kind)?;
                    }
                    emit(Event::Crashed(id, kind));
                    Ok(())
                })),
                &mut token,
            )?;
        }
        self.wire(Target::Tab(id))
    }

    pub fn shell(&self) -> Result<()> {
        unsafe {
            let s = self.web.Settings()?;
            s.SetAreDefaultContextMenusEnabled(false)?;
            s.SetAreDevToolsEnabled(cfg!(debug_assertions))?;
            s.SetIsStatusBarEnabled(false)?;
            s.SetIsZoomControlEnabled(false)?;
            s.cast::<ICoreWebView2Settings3>()?.SetAreBrowserAcceleratorKeysEnabled(false)?;
            s.cast::<ICoreWebView2Settings5>()?.SetIsPinchZoomEnabled(false)?;
            s.cast::<ICoreWebView2Settings6>()?.SetIsSwipeNavigationEnabled(false)?;
            self.ctl
                .cast::<ICoreWebView2Controller2>()?
                .SetDefaultBackgroundColor(COREWEBVIEW2_COLOR { A: 0, R: 0, G: 0, B: 0 })?;
            let mut token = 0;
            self.web.add_WebMessageReceived(
                &WebMessageReceivedEventHandler::create(Box::new(|_, args| {
                    if let Some(args) = args {
                        let json = text(|p| args.WebMessageAsJson(p));
                        if let Ok(v) = serde_json::from_str(&json) {
                            emit(Event::Ui(v));
                        }
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        }
        self.wire(Target::Shell)
    }

    fn wire(&self, target: Target) -> Result<()> {
        let mut token = 0;
        unsafe {
            self.comp.add_CursorChanged(
                &CursorChangedEventHandler::create(Box::new(move |_, _| {
                    emit(Event::Cursor(target));
                    Ok(())
                })),
                &mut token,
            )?;
            self.ctl.add_AcceleratorKeyPressed(
                &AcceleratorKeyPressedEventHandler::create(Box::new(|_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
                    let mut vk = 0;
                    let mut status = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
                    args.KeyEventKind(&mut kind)?;
                    args.VirtualKey(&mut vk)?;
                    args.PhysicalKeyStatus(&mut status)?;
                    let down = kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                        || kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN;
                    if let Some(action) = keys::lookup(vk).filter(|_| down) {
                        args.SetHandled(true)?;
                        emit(Event::Action(action, status.WasKeyDown.as_bool()));
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        }
        Ok(())
    }
}
