use std::{
    cell::RefCell,
    collections::VecDeque,
    mem::size_of,
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicIsize, Ordering},
    },
    time::Instant,
};

use serde_json::{Value, json};
use webview2_com::{ClearBrowsingDataCompletedHandler, Microsoft::Web::WebView2::Win32::*};
use windows::{
    UI::Color,
    Win32::{
        Foundation::*,
        UI::{Controls::WM_MOUSELEAVE, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
    core::*,
};

use crate::{
    config::{self, Config},
    ext,
    keys::{self, Action},
    motion::{self, Curve, Layer, Motion, Pose},
    search,
    store::{History, Saved, Session},
    tabs::Tab,
    web::{self, View, text},
    window,
};

const HOLD: usize = 1;
const TICK: usize = 2;
const SAVE: usize = 3;
const REVEAL: usize = 4;
const RETRY: usize = 5;

#[derive(Clone, Copy, PartialEq)]
pub enum Target {
    Shell,
    Tab(u32),
}

pub enum Event {
    Env(ICoreWebView2Environment),
    NoEnv,
    Created(Target, ICoreWebView2CompositionController),
    Failed(Target),
    Title(u32),
    Url(u32),
    Started(u32),
    Loaded(u32),
    Finished(u32, bool),
    Icon(u32),
    Audio(u32),
    Status(u32),
    Popup {
        args: ICoreWebView2NewWindowRequestedEventArgs,
        deferral: ICoreWebView2Deferral,
        user: bool,
        quiet: bool,
    },
    CloseTab(u32),
    Crashed(u32, COREWEBVIEW2_PROCESS_FAILED_KIND),
    Slept(u32, bool),
    Cursor(Target),
    Action(Action, bool),
    Ui(Value),
    Remote(Remote),
    Timer(usize),
    Settled(u64),
    Added(String, Option<ICoreWebView2BrowserExtension>),
    Extensions(Vec<ICoreWebView2BrowserExtension>),
    Refresh,
    Open(String),
    Theme,
    Quit,
}

pub enum Remote {
    Suggest(String, Vec<String>),
    Unpacked(std::result::Result<String, String>),
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    None,
    Bar,
    Switch,
    Settings,
}

enum Enter {
    Slide(f32),
    Rise,
    Bloom,
}

enum Done {
    Hide(u32),
    Bury(Option<View>, Layer),
    Exit,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static QUEUE: RefCell<VecDeque<Event>> = const { RefCell::new(VecDeque::new()) };
}

static INBOX: Mutex<Vec<Remote>> = Mutex::new(Vec::new());
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static RESTART: AtomicBool = AtomicBool::new(false);

pub fn with<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| {
        let mut slot = cell.try_borrow_mut().ok()?;
        let app = slot.as_mut()?;
        let out = f(app);
        while let Some(e) = QUEUE.with(|q| q.borrow_mut().pop_front()) {
            app.handle(e);
        }
        Some(out)
    })
}

pub fn emit(e: Event) {
    QUEUE.with(|q| q.borrow_mut().push_back(e));
    with(|_| ());
}

pub fn send(r: Remote) {
    if let Ok(mut inbox) = INBOX.lock() {
        inbox.push(r);
    }
    let hwnd = HWND(WINDOW.load(Ordering::Relaxed) as *mut _);
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_APP, WPARAM(0), LPARAM(0));
    }
}

pub fn inbox() {
    let items = INBOX.lock().map(|mut i| std::mem::take(&mut *i)).unwrap_or_default();
    for r in items {
        emit(Event::Remote(r));
    }
}

pub fn restarting() -> bool {
    RESTART.load(Ordering::Relaxed)
}

pub fn launch(hwnd: HWND, config: Config, url: Option<String>) -> Result<()> {
    WINDOW.store(hwnd.0 as isize, Ordering::Relaxed);
    let motion = Motion::new(hwnd, backdrop(dark(&config)), motion::color(&config.accent))?;
    let app = App {
        hwnd,
        motion,
        config,
        env: None,
        shell: None,
        live: false,
        tabs: Vec::new(),
        mru: Vec::new(),
        active: None,
        incoming: None,
        next: 0,
        mode: Mode::None,
        fresh: false,
        sunk: false,
        line: false,
        picks: Vec::new(),
        pick: 0,
        history: History::load(),
        closed: Vec::new(),
        exts: Vec::new(),
        done: Vec::new(),
        ticket: 0,
        cursor: HCURSOR::default(),
        hovering: false,
        size: (0, 0),
        dpi: 1.0,
        launch: url,
        attempts: 0,
        leaving: false,
        version: String::new(),
        query: String::new(),
    };
    APP.with(|a| *a.borrow_mut() = Some(app));
    with(|a| a.start());
    Ok(())
}

fn dark(config: &Config) -> bool {
    match config.theme.as_str() {
        "light" => false,
        "system" => window::dark(),
        _ => true,
    }
}

fn backdrop(dark: bool) -> Color {
    if dark { Color { A: 255, R: 11, G: 11, B: 13 } } else { Color { A: 255, R: 234, G: 234, B: 238 } }
}

fn page() -> String {
    include_str!("../ui/index.html")
        .replace("<style></style>", &format!("<style>{}</style>", include_str!("../ui/style.css")))
        .replace("<script></script>", &format!("<script>{}</script>", include_str!("../ui/app.js")))
}

pub struct App {
    hwnd: HWND,
    motion: Motion,
    config: Config,
    env: Option<ICoreWebView2Environment>,
    shell: Option<View>,
    live: bool,
    tabs: Vec<Tab>,
    mru: Vec<u32>,
    active: Option<u32>,
    incoming: Option<u32>,
    next: u32,
    mode: Mode,
    fresh: bool,
    sunk: bool,
    line: bool,
    picks: Vec<u32>,
    pick: usize,
    history: History,
    closed: Vec<Saved>,
    exts: Vec<ICoreWebView2BrowserExtension>,
    done: Vec<(u64, Done)>,
    ticket: u64,
    cursor: HCURSOR,
    hovering: bool,
    size: (i32, i32),
    dpi: f32,
    launch: Option<String>,
    attempts: u32,
    leaving: bool,
    version: String,
    query: String,
}

impl App {
    fn start(&mut self) {
        keys::load(&self.config);
        self.motion.speed = self.config.motion as f32;
        self.resize();
        window::show(self.hwnd);
        window::timer(self.hwnd, TICK, 30_000);
        self.boot();
    }

    fn boot(&mut self) {
        match web::environment(&self.config) {
            Ok(()) => {}
            Err(e) if e.code() == ERROR_FILE_NOT_FOUND.to_hresult() => self.missing(),
            Err(_) => self.retry(),
        }
    }

    fn retry(&mut self) {
        self.attempts += 1;
        if self.attempts > 15 {
            return self.missing();
        }
        window::timer(self.hwnd, RETRY, 400);
    }

    fn missing(&mut self) {
        unsafe {
            MessageBoxW(
                Some(self.hwnd),
                w!("Xeno runs on the Microsoft Edge WebView2 Runtime, which couldn't be started. Install it and open Xeno again."),
                w!("Xeno"),
                MB_ICONWARNING,
            );
            window::reveal("https://go.microsoft.com/fwlink/p/?LinkId=2124703");
            let _ = DestroyWindow(self.hwnd);
        }
    }

    fn handle(&mut self, e: Event) {
        match e {
            Event::Env(env) => self.ready(env),
            Event::NoEnv => self.retry(),
            Event::Created(target, comp) => self.created(target, comp),
            Event::Failed(Target::Shell) => {
                if let Some(env) = &self.env {
                    let _ = web::spawn(env, self.hwnd, Target::Shell);
                }
            }
            Event::Failed(Target::Tab(id)) => {
                if let Some(t) = self.tab_mut(id) {
                    t.creating = false;
                }
            }
            Event::Title(id) => self.titled(id),
            Event::Url(id) => self.moved(id),
            Event::Started(id) => self.started(id),
            Event::Loaded(id) => self.loaded(id),
            Event::Finished(id, ok) => self.finished(id, ok),
            Event::Icon(id) => {
                if let Some(t) = self.tab_mut(id) {
                    t.icon = t.view.as_ref().map(View::icon).unwrap_or_default();
                }
                self.push_tabs();
            }
            Event::Audio(id) => {
                if let Some(t) = self.tab_mut(id) {
                    t.audible = t.view.as_ref().is_some_and(View::audible);
                }
                self.push_tabs();
            }
            Event::Status(id) => {
                if let Some(v) = self.tab(id).filter(|_| self.active == Some(id)).and_then(|t| t.view.as_ref()) {
                    self.post(json!({ "t": "status", "text": v.status() }));
                }
            }
            Event::Popup { args, deferral, user, quiet } => self.popup(args, deferral, user, quiet),
            Event::CloseTab(id) => self.close(id),
            Event::Crashed(id, kind) => self.crashed(id, kind),
            Event::Slept(id, ok) => {
                if let Some(t) = self.tab_mut(id) {
                    t.asleep = ok;
                }
            }
            Event::Cursor(target) => {
                if self.current() == Some(target) {
                    self.pick_cursor();
                }
            }
            Event::Action(action, repeat) => self.action(action, repeat),
            Event::Ui(v) => self.ui(v),
            Event::Remote(r) => self.remote(r),
            Event::Timer(id) => self.timer(id),
            Event::Settled(token) => self.settled(token),
            Event::Added(path, ext) => self.added(path, ext),
            Event::Extensions(list) => {
                self.exts = list;
                self.push_exts();
            }
            Event::Refresh => self.refresh_exts(),
            Event::Open(url) => {
                window::show(self.hwnd);
                if url.is_empty() {
                    return;
                }
                let url = self.target_url(&url);
                if self.env.is_some() {
                    self.open(url, false);
                } else {
                    self.launch = Some(url);
                }
            }
            Event::Theme => self.restyle(),
            Event::Quit => self.quit(),
        }
    }

    fn ready(&mut self, env: ICoreWebView2Environment) {
        self.version = web::version(&env);
        let _ = web::spawn(&env, self.hwnd, Target::Shell);
        self.env = Some(env);
        self.restore();
    }

    fn target_url(&self, arg: &str) -> String {
        if Path::new(arg).exists() {
            format!("file:///{}", arg.replace('\\', "/"))
        } else {
            search::resolve(arg, &self.config)
        }
    }

    fn restore(&mut self) {
        let session = if self.config.restore { Session::load() } else { Session::default() };
        let ids: Vec<u32> = session.tabs.iter().filter_map(|s| self.add(s.url.clone(), s.title.clone())).collect();
        self.mru = ids.clone();
        if let Some(url) = self.launch.take() {
            let url = self.target_url(&url);
            self.open(url, false);
        } else if let Some(&id) = ids.get(session.active).or(ids.first()) {
            self.activate(id);
        } else if !self.config.start.is_empty() {
            let url = search::resolve(&self.config.start, &self.config);
            self.open(url, false);
        } else {
            self.set_mode(Mode::Bar);
        }
    }

    fn tab(&self, id: u32) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    fn tab_mut(&mut self, id: u32) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    fn index(&self, id: u32) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    fn view(&self) -> Option<View> {
        self.active.and_then(|id| self.tab(id)).and_then(|t| t.view.clone())
    }

    fn current(&self) -> Option<Target> {
        if self.mode != Mode::None { Some(Target::Shell) } else { self.active.map(Target::Tab) }
    }

    fn target(&self) -> Option<&View> {
        match self.current()? {
            Target::Shell => self.shell.as_ref(),
            Target::Tab(id) => self.tab(id)?.view.as_ref(),
        }
    }

    fn post(&self, v: Value) {
        if let Some(shell) = self.shell.as_ref().filter(|_| self.live) {
            shell.post(&v);
        }
    }

    fn toast(&self, text: &str) {
        self.post(json!({ "t": "toast", "text": text }));
    }

    fn ticket(&mut self) -> u64 {
        self.ticket += 1;
        self.ticket
    }

    fn add(&mut self, url: String, title: String) -> Option<u32> {
        let layer = self.motion.layer().ok()?;
        self.next += 1;
        let id = self.next;
        let at = self.active.and_then(|a| self.index(a)).map_or(self.tabs.len(), |i| i + 1);
        self.tabs.insert(at, Tab::new(id, url, title, layer));
        self.mru.insert(self.mru.len().min(1), id);
        Some(id)
    }

    fn ensure(&mut self, id: u32) {
        let Some(env) = self.env.clone() else { return };
        let hwnd = self.hwnd;
        if let Some(tab) = self.tab_mut(id).filter(|t| t.view.is_none() && !t.creating) {
            tab.creating = web::spawn(&env, hwnd, Target::Tab(id)).is_ok();
        }
    }

    fn created(&mut self, target: Target, comp: ICoreWebView2CompositionController) {
        let Ok(view) = View::new(comp) else { return };
        let (w, h) = self.size;
        let id = match target {
            Target::Shell => {
                let _ = view.attach(&self.motion.shell, w, h);
                let _ = view.shell();
                view.show(true);
                view.html(&page());
                self.shell = Some(view);
                self.apply_profile();
                return self.refresh_exts();
            }
            Target::Tab(id) => id,
        };
        let visible = self.active == Some(id) || self.incoming == Some(id);
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else { return view.close() };
        tab.creating = false;
        if view.attach(&tab.layer.visual, w, h).is_err() || view.tab(id, &self.config).is_err() {
            return view.close();
        }
        match tab.popup.take() {
            Some((args, deferral)) => unsafe {
                let _ = args.SetNewWindow(&view.web);
                let _ = deferral.Complete();
            },
            None if !tab.url.is_empty() => view.navigate(&tab.url),
            None => {}
        }
        if tab.muted {
            view.mute(true);
        }
        view.show(visible);
        if self.active == Some(id) && self.mode == Mode::None {
            view.focus();
        }
        tab.view = Some(view);
    }

    fn activate(&mut self, id: u32) {
        let Some(tab) = self.tab(id) else { return };
        if self.active == Some(id) && tab.view.is_some() {
            return self.set_mode(Mode::None);
        }
        if tab.view.is_some() && tab.ready {
            let enter = self.direction(id);
            return self.show(id, enter);
        }
        self.incoming = Some(id);
        self.ensure(id);
        if let Some(tab) = self.tab(id) {
            let _ = self.motion.pose(&tab.layer, Pose::new(0.0, 0.95));
            let _ = tab.layer.show(true);
            if let Some(v) = &tab.view {
                v.resume();
                v.show(true);
            }
        }
        self.recede();
        self.line(true);
        window::timer(self.hwnd, REVEAL, 1800);
    }

    fn direction(&self, id: u32) -> Enter {
        match (self.active.and_then(|a| self.index(a)), self.index(id)) {
            (Some(a), Some(b)) => Enter::Slide(if b > a { 1.0 } else { -1.0 }),
            _ => Enter::Bloom,
        }
    }

    fn reveal(&mut self, id: u32) {
        let seen = self.tab(id).is_some_and(|t| t.ready && t.view.is_some() && t.used.elapsed().as_secs() > 2);
        let enter = if seen && self.active.is_some() { self.direction(id) } else { Enter::Bloom };
        self.show(id, enter);
    }

    fn rest(&self) -> Pose {
        let scale = match self.mode {
            _ if !self.config.depth => 1.0,
            Mode::None => 1.0,
            Mode::Bar => 0.975,
            Mode::Switch | Mode::Settings => 0.94,
        };
        if scale < 1.0 { Pose::new(1.0, scale).round(self.config.corners as f32 * self.dpi) } else { Pose::REST }
    }

    fn show(&mut self, id: u32, enter: Enter) {
        if self.incoming == Some(id) {
            self.incoming = None;
            window::stop(self.hwnd, REVEAL);
        }
        let prev = self.active.filter(|&p| p != id);
        let deep = self.sunk;
        self.active = Some(id);
        self.mru.retain(|&t| t != id);
        self.mru.insert(0, id);
        let rest = self.rest();
        self.sunk = rest.scale < 1.0;
        let dpi = self.dpi;
        let focus = self.mode == Mode::None;
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else { return };
        tab.used = Instant::now();
        if let Some(view) = &tab.view {
            if tab.asleep {
                view.resume();
                tab.asleep = false;
            }
            view.thrift(false);
            view.show(true);
            if focus {
                view.focus();
            }
        }
        let start = match enter {
            Enter::Slide(dir) => Pose::new(0.0, 0.955).x(dir * 48.0 * dpi),
            Enter::Rise => Pose::new(1.0, 0.97),
            Enter::Bloom => Pose::new(0.0, 0.93),
        };
        if !matches!(enter, Enter::Rise) {
            let _ = self.motion.raise(&tab.layer);
        }
        let _ = self.motion.pose(&tab.layer, start);
        let _ = tab.layer.show(true);
        let _ = self.motion.animate(&tab.layer, rest, Curve::Pop, 560.0);
        window::title(self.hwnd, tab.label());
        let loading = tab.loading;
        self.line(loading);

        if let Some(prev) = prev {
            let dir = if let Enter::Slide(d) = enter { d } else { 0.0 };
            let token = self.ticket();
            if let Some(tab) = self.tab(prev) {
                let to = Pose::new(0.0, if deep { 0.9 } else { 0.965 }).x(-dir * 32.0 * dpi);
                let _ = self.motion.batch(token, || self.motion.animate(&tab.layer, to, Curve::Exit, 300.0));
                self.done.push((token, Done::Hide(prev)));
            }
        }
        self.pick_cursor();
        self.push_tabs();
        self.dirty();
    }

    fn recede(&mut self) {
        let rest = self.rest();
        self.sunk = rest.scale < 1.0;
        let curve = if self.mode == Mode::None { Curve::Pop } else { Curve::Glide };
        if let Some(tab) = self.active.and_then(|id| self.tab(id)) {
            let _ = self.motion.animate(&tab.layer, rest, curve, 480.0);
        }
    }

    fn line(&mut self, on: bool) {
        let on = on && self.config.line;
        if on != self.line {
            self.line = on;
            let _ = self.motion.loading(on);
        }
    }

    fn close(&mut self, id: u32) {
        let Some(i) = self.index(id) else { return };
        let tab = self.tabs.remove(i);
        self.mru.retain(|&t| t != id);
        self.picks.retain(|&t| t != id);
        if self.incoming == Some(id) {
            self.incoming = None;
        }
        if tab.url.starts_with("http") {
            self.closed.push(Saved { url: tab.url.clone(), title: tab.title.clone() });
        }
        let was = self.active == Some(id);
        if was {
            self.active = None;
            let token = self.ticket();
            let to = Pose::new(0.0, if self.sunk { 0.86 } else { 0.92 });
            let _ = self.motion.batch(token, || self.motion.animate(&tab.layer, to, Curve::Exit, 260.0));
            self.done.push((token, Done::Bury(tab.view, tab.layer)));
        } else {
            if let Some(v) = tab.view {
                v.close();
            }
            let _ = self.motion.discard(&tab.layer);
        }

        if was {
            match self.mru.first().copied() {
                Some(next) if self.tab(next).is_some_and(|t| t.ready && t.view.is_some()) => self.show(next, Enter::Rise),
                Some(next) => self.activate(next),
                None => {
                    window::title(self.hwnd, "");
                    self.line(false);
                    self.fresh = true;
                    self.set_mode(Mode::Bar);
                }
            }
        }
        if self.mode == Mode::Switch {
            if self.picks.is_empty() {
                self.set_mode(Mode::Bar);
            } else {
                self.pick = self.pick.min(self.picks.len() - 1);
                self.post_mode();
            }
        }
        self.push_tabs();
        self.dirty();
    }

    fn set_mode(&mut self, m: Mode) {
        let m = if m == Mode::None && self.active.is_none() && self.incoming.is_none() { Mode::Bar } else { m };
        if m != Mode::Switch {
            window::stop(self.hwnd, HOLD);
        }
        let changed = self.mode != m;
        self.mode = m;
        if changed {
            self.recede();
        }
        self.refocus();
        self.pick_cursor();
        self.post_mode();
        if m == Mode::Switch {
            self.stats();
        }
    }

    fn post_mode(&self) {
        let msg = match self.mode {
            Mode::None => json!({ "t": "mode", "mode": "none" }),
            Mode::Bar => {
                let tab = self.active.and_then(|id| self.tab(id));
                json!({
                    "t": "mode",
                    "mode": "bar",
                    "home": self.active.is_none() && self.incoming.is_none(),
                    "fresh": self.fresh,
                    "url": tab.map(|t| t.url.as_str()).unwrap_or_default(),
                    "title": tab.map(Tab::label).unwrap_or_default(),
                })
            }
            Mode::Switch => {
                let list: Vec<Value> = self.picks.iter().filter_map(|&id| self.tab(id)).map(Tab::json).collect();
                json!({ "t": "mode", "mode": "switch", "list": list, "pick": self.pick, "active": self.active })
            }
            Mode::Settings => json!({ "t": "mode", "mode": "settings" }),
        };
        self.post(msg);
    }

    pub fn refocus(&self) {
        if let Some(view) = self.target() {
            view.focus();
        }
    }

    fn pick_cursor(&mut self) {
        self.cursor = self.target().map(View::cursor).unwrap_or_default();
        self.cursor();
    }

    pub fn cursor(&self) -> bool {
        if self.cursor.is_invalid() {
            return false;
        }
        unsafe {
            SetCursor(Some(self.cursor));
        }
        true
    }

    pub fn mouse(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) {
        let mut pt = POINT { x: (lp.0 & 0xffff) as i16 as i32, y: ((lp.0 >> 16) & 0xffff) as i16 as i32 };
        let mut data = 0;
        match msg {
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                data = ((wp.0 >> 16) & 0xffff) as i16 as i32 as u32;
                unsafe {
                    let _ = windows::Win32::Graphics::Gdi::ScreenToClient(self.hwnd, &mut pt);
                }
            }
            WM_XBUTTONDOWN | WM_XBUTTONUP | WM_XBUTTONDBLCLK => data = ((wp.0 >> 16) & 0xffff) as u32,
            WM_MOUSELEAVE => {
                self.hovering = false;
                pt = POINT::default();
            }
            _ => {}
        }
        unsafe {
            if msg == WM_MOUSEMOVE && !self.hovering {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: self.hwnd,
                    dwHoverTime: 0,
                };
                self.hovering = TrackMouseEvent(&mut track).is_ok();
            }
            match msg {
                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN => {
                    SetCapture(self.hwnd);
                }
                WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP | WM_XBUTTONUP => {
                    let _ = ReleaseCapture();
                }
                _ => {}
            }
        }
        if let Some(view) = self.target() {
            view.mouse(msg, (wp.0 & 0xffff) as u32, data, pt);
        }
    }

    pub fn resize(&mut self) {
        let (w, h, dpi) = window::size(self.hwnd);
        self.size = (w, h);
        self.dpi = dpi;
        let _ = self.motion.resize(w as f32, h as f32, dpi);
        for t in &self.tabs {
            let _ = self.motion.fit(&t.layer);
            if let Some(v) = &t.view {
                let _ = v.bounds(w, h);
            }
        }
        if let Some(s) = &self.shell {
            let _ = s.bounds(w, h);
        }
    }

    pub fn automation(&self) -> Option<IUnknown> {
        self.target().and_then(View::automation)
    }

    fn action(&mut self, action: Action, repeat: bool) {
        if repeat && !matches!(action, Action::Switch | Action::SwitchBack | Action::Next | Action::Prev) {
            return;
        }
        match action {
            Action::Bar if self.mode == Mode::Bar && self.active.is_some() => self.set_mode(Mode::None),
            Action::Bar => self.bar(false),
            Action::New => self.bar(true),
            Action::Switch => self.cycle(1, true),
            Action::SwitchBack => self.cycle(-1, true),
            Action::Close => {
                let id = if self.mode == Mode::Switch { self.picks.get(self.pick).copied() } else { self.active };
                if let Some(id) = id {
                    self.close(id);
                }
            }
            Action::Reopen => self.reopen(),
            Action::Settings if self.mode == Mode::Settings => self.set_mode(Mode::None),
            Action::Settings => self.set_mode(Mode::Settings),
            Action::Next => self.step(1),
            Action::Prev => self.step(-1),
            Action::Copy => self.copy(),
            Action::Jump(n) => {
                let id = if n == 9 { self.tabs.last() } else { self.tabs.get(n - 1) }.map(|t| t.id);
                if let Some(id) = id {
                    self.select(id);
                }
            }
        }
    }

    fn bar(&mut self, fresh: bool) {
        self.fresh = fresh || self.active.is_none();
        if self.mode == Mode::Bar {
            self.post_mode();
        } else {
            self.set_mode(Mode::Bar);
        }
    }

    fn cycle(&mut self, step: i32, hold: bool) {
        if self.tabs.is_empty() {
            return;
        }
        if self.mode == Mode::Switch {
            let n = self.picks.len() as i32;
            self.pick = (self.pick as i32 + step).rem_euclid(n.max(1)) as usize;
            return self.post(json!({ "t": "pick", "i": self.pick }));
        }
        self.picks = self.mru.clone();
        let n = self.picks.len();
        self.pick = if step > 0 { 1 % n } else { n - 1 };
        if hold && !keys::held() {
            let id = self.picks[self.pick];
            return self.select(id);
        }
        self.set_mode(Mode::Switch);
        if hold {
            window::timer(self.hwnd, HOLD, 15);
        }
    }

    fn commit(&mut self) {
        window::stop(self.hwnd, HOLD);
        if self.mode != Mode::Switch {
            return;
        }
        if let Some(id) = self.picks.get(self.pick).copied() {
            self.select(id);
        } else {
            self.set_mode(Mode::None);
        }
    }

    fn select(&mut self, id: u32) {
        window::stop(self.hwnd, HOLD);
        if self.active == Some(id) || self.tab(id).is_none() {
            return self.set_mode(Mode::None);
        }
        self.mode = Mode::None;
        self.post_mode();
        self.activate(id);
        self.refocus();
        self.pick_cursor();
    }

    fn step(&mut self, dir: i32) {
        let Some(i) = self.active.and_then(|a| self.index(a)) else { return };
        let n = self.tabs.len() as i32;
        if n > 1 {
            let id = self.tabs[(i as i32 + dir).rem_euclid(n) as usize].id;
            self.select(id);
        }
    }

    fn reopen(&mut self) {
        if let Some(s) = self.closed.pop() {
            self.open(s.url, false);
        }
    }

    fn copy(&mut self) {
        if let Some(tab) = self.active.and_then(|id| self.tab(id)) {
            window::copy(self.hwnd, &tab.url);
            self.toast("Link copied");
        }
    }

    fn open(&mut self, url: String, quiet: bool) {
        let Some(id) = self.add(url, String::new()) else { return };
        if quiet {
            self.ensure(id);
            return self.push_tabs();
        }
        self.activate(id);
        self.set_mode(Mode::None);
        self.push_tabs();
    }

    fn go(&mut self, url: String, tab: bool) {
        if url.is_empty() {
            return;
        }
        if tab || self.active.is_none() {
            return self.open(url, false);
        }
        if let Some(t) = self.active.and_then(|id| self.tab_mut(id)) {
            t.url = url.clone();
            if let Some(v) = &t.view {
                v.navigate(&url);
            }
        }
        self.set_mode(Mode::None);
    }

    fn popup(&mut self, args: ICoreWebView2NewWindowRequestedEventArgs, deferral: ICoreWebView2Deferral, user: bool, quiet: bool) {
        if !user && self.config.popups {
            unsafe {
                let _ = args.SetHandled(true);
                let _ = deferral.Complete();
            }
            return self.toast("Blocked a pop-up");
        }
        let url = text(|p| unsafe { args.Uri(p) });
        let Some(id) = self.add(url, String::new()) else {
            unsafe {
                let _ = deferral.Complete();
            }
            return;
        };
        if let Some(t) = self.tab_mut(id) {
            t.popup = Some((args, deferral));
        }
        if quiet {
            self.ensure(id);
            self.toast("Opened in the background");
            return self.push_tabs();
        }
        self.activate(id);
        self.set_mode(Mode::None);
        self.push_tabs();
    }

    fn crashed(&mut self, id: u32, kind: COREWEBVIEW2_PROCESS_FAILED_KIND) {
        if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
            RESTART.store(true, Ordering::Relaxed);
            return self.quit();
        }
        if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
            || kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE
        {
            if let Some(v) = self.tab(id).and_then(|t| t.view.as_ref()) {
                v.reload();
            }
            if self.active == Some(id) {
                self.toast("This page stopped working, so Xeno reloaded it");
            }
        }
    }

    fn titled(&mut self, id: u32) {
        let Some(t) = self.tab_mut(id) else { return };
        t.title = t.view.as_ref().map(View::title).unwrap_or_default();
        let (url, title) = (t.url.clone(), t.title.clone());
        self.history.title(&url, &title);
        if self.active == Some(id) {
            window::title(self.hwnd, &title);
        }
        self.push_tabs();
    }

    fn moved(&mut self, id: u32) {
        if let Some(t) = self.tab_mut(id) {
            t.url = t.view.as_ref().map(View::url).unwrap_or_default();
        }
        self.push_tabs();
        self.dirty();
    }

    fn started(&mut self, id: u32) {
        if let Some(t) = self.tab_mut(id) {
            t.loading = true;
        }
        if self.active == Some(id) || self.incoming == Some(id) {
            self.line(true);
        }
        self.push_tabs();
    }

    fn loaded(&mut self, id: u32) {
        if let Some(t) = self.tab_mut(id) {
            t.ready = true;
        }
        if self.incoming == Some(id) {
            self.reveal(id);
        }
    }

    fn finished(&mut self, id: u32, ok: bool) {
        let Some(t) = self.tab_mut(id) else { return };
        t.loading = false;
        t.ready = true;
        if let Some(v) = &t.view {
            t.url = v.url();
            t.title = v.title();
        }
        let (url, title) = (t.url.clone(), t.title.clone());
        if ok && self.config.history {
            self.history.visit(&url, &title);
        }
        if self.active == Some(id) {
            self.line(false);
        }
        if self.incoming == Some(id) {
            self.reveal(id);
        }
        self.push_tabs();
    }

    fn timer(&mut self, id: usize) {
        match id {
            HOLD if !keys::held() => self.commit(),
            TICK => self.tidy(),
            SAVE => {
                window::stop(self.hwnd, SAVE);
                self.save();
            }
            REVEAL => {
                window::stop(self.hwnd, REVEAL);
                if let Some(id) = self.incoming {
                    self.reveal(id);
                }
            }
            RETRY => {
                window::stop(self.hwnd, RETRY);
                self.boot();
            }
            _ => {}
        }
    }

    fn settled(&mut self, token: u64) {
        let Some(i) = self.done.iter().position(|(t, _)| *t == token) else { return };
        match self.done.remove(i).1 {
            Done::Hide(id) => {
                if self.active == Some(id) || self.incoming == Some(id) {
                    return;
                }
                let trim = self.config.trim;
                if let Some(tab) = self.tab_mut(id) {
                    tab.used = Instant::now();
                    let _ = tab.layer.show(false);
                    if let Some(v) = &tab.view {
                        v.show(false);
                        if trim {
                            v.thrift(true);
                        }
                    }
                }
            }
            Done::Bury(view, layer) => {
                if let Some(v) = view {
                    v.close();
                }
                let _ = self.motion.discard(&layer);
            }
            Done::Exit => {
                for t in &self.tabs {
                    if let Some(v) = &t.view {
                        v.close();
                    }
                }
                if let Some(s) = &self.shell {
                    s.close();
                }
                unsafe {
                    let _ = DestroyWindow(self.hwnd);
                }
            }
        }
    }

    fn tidy(&mut self) {
        let sleep = self.config.sleep as u64 * 60;
        let discard = self.config.discard as u64 * 60;
        let mut changed = false;
        for tab in &mut self.tabs {
            if Some(tab.id) == self.active || Some(tab.id) == self.incoming || tab.audible || tab.view.is_none() {
                continue;
            }
            let idle = tab.used.elapsed().as_secs();
            if discard > 0 && idle >= discard {
                if let Some(v) = tab.view.take() {
                    v.close();
                }
                tab.ready = false;
                tab.asleep = false;
                tab.loading = false;
                changed = true;
            } else if sleep > 0 && idle >= sleep && !tab.asleep {
                if let Some(v) = &tab.view {
                    v.suspend(tab.id);
                }
                changed = true;
            }
        }
        if changed {
            self.push_tabs();
        }
        self.history.save();
    }

    fn sleep_all(&mut self) {
        for tab in &self.tabs {
            if Some(tab.id) == self.active || tab.audible {
                continue;
            }
            if let Some(v) = &tab.view {
                v.thrift(true);
                v.suspend(tab.id);
            }
        }
        self.toast("Background tabs are asleep");
    }

    fn ui(&mut self, v: Value) {
        let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        let id = v["id"].as_u64().unwrap_or_default() as u32;
        let tab = v["tab"].as_bool().unwrap_or(false);
        match v["t"].as_str().unwrap_or_default() {
            "ready" => {
                let first = !self.live;
                self.live = true;
                self.push_config();
                self.push_tabs();
                self.post_mode();
                self.push_exts();
                self.refocus();
                if first {
                    let _ = self.motion.enter();
                }
            }
            "close" => self.set_mode(Mode::None),
            "query" => self.query(s("q")),
            "go" => {
                let url = search::resolve(&s("text"), &self.config);
                self.go(url, tab);
            }
            "search" => {
                let url = search::query_url(&s("text"), &self.config);
                self.go(url, tab);
            }
            "open" => self.go(s("url"), tab),
            "activate" if self.mode == Mode::Switch => {
                if let Some(i) = self.picks.iter().position(|&p| p == id) {
                    self.pick = i;
                    self.commit();
                }
            }
            "activate" => self.select(id),
            "pick" => self.pick = v["i"].as_u64().unwrap_or_default() as usize,
            "kill" => self.close(id),
            "cmd" => self.command(&s("name")),
            "set" => self.set(&s("key"), v["value"].clone()),
            "mode" => match s("mode").as_str() {
                "settings" => self.set_mode(Mode::Settings),
                "bar" => self.bar(tab),
                "switch" => self.cycle(1, false),
                _ => self.set_mode(Mode::None),
            },
            "record" => keys::pause(v["on"].as_bool().unwrap_or(false)),
            "ext" => self.extension(&v),
            "stats" => self.stats(),
            _ => {}
        }
    }

    fn query(&mut self, q: String) {
        let items: Vec<Value> =
            self.history.search(&q, 6).iter().map(|v| json!({ "url": v.url, "title": v.title })).collect();
        let complete = self.history.complete(&q);
        self.post(json!({ "t": "results", "q": q, "items": items, "complete": complete }));
        search::suggest(q.clone(), &self.config);
        self.query = q;
    }

    fn command(&mut self, name: &str) {
        match name {
            "new" => return self.bar(true),
            "settings" => return self.set_mode(Mode::Settings),
            "extensions" => {
                self.set_mode(Mode::Settings);
                return self.post(json!({ "t": "section", "id": "extensions" }));
            }
            "switch" => return self.cycle(1, false),
            "reset-keys" => {
                self.config.keys = config::KEYS.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
                keys::load(&self.config);
                self.push_config();
                return self.dirty();
            }
            _ => {}
        }
        if self.mode != Mode::None {
            self.set_mode(Mode::None);
        }
        let view = self.view();
        match name {
            "close" => {
                if let Some(id) = self.active {
                    self.close(id);
                }
            }
            "reopen" => self.reopen(),
            "copy" => self.copy(),
            "quit" => self.quit(),
            "restart" => {
                RESTART.store(true, Ordering::Relaxed);
                self.quit();
            }
            "sleep" => self.sleep_all(),
            "clear-history" => {
                self.history.clear();
                self.toast("History cleared");
            }
            "clear-data" => self.clear_data(),
            "downloads" => {
                let path = self.shell.as_ref().and_then(View::profile).map(|p| text(|s| unsafe { p.DefaultDownloadFolderPath(s) }));
                if let Some(path) = path.filter(|p| !p.is_empty()) {
                    window::reveal(&path);
                }
            }
            "data" => window::reveal(&config::dir().to_string_lossy()),
            "mute" => {
                if let Some(t) = self.active.and_then(|id| self.tab_mut(id)) {
                    t.muted = !t.muted;
                    if let Some(v) = &t.view {
                        v.mute(t.muted);
                    }
                    let muted = t.muted;
                    self.toast(if muted { "Tab muted" } else { "Tab unmuted" });
                    self.push_tabs();
                }
            }
            _ => {
                let Some(v) = view else { return };
                match name {
                    "reload" => v.reload(),
                    "back" => v.back(false),
                    "forward" => v.back(true),
                    "zoom-in" => v.zoom(Some(1.1)),
                    "zoom-out" => v.zoom(Some(1.0 / 1.1)),
                    "zoom-reset" => v.zoom(None),
                    "print" => v.print(),
                    "devtools" => v.devtools(),
                    _ => {}
                }
            }
        }
    }

    fn clear_data(&mut self) {
        self.history.clear();
        let Some(p) = self.shell.as_ref().and_then(View::profile).and_then(|p| p.cast::<ICoreWebView2Profile2>().ok()) else {
            return;
        };
        let handler = ClearBrowsingDataCompletedHandler::create(Box::new(|_| Ok(())));
        unsafe {
            let _ = p.ClearBrowsingDataAll(&handler);
        }
        self.toast("Browsing data cleared");
    }

    fn set(&mut self, key: &str, value: Value) {
        let mut v = serde_json::to_value(&self.config).unwrap_or_default();
        v[key] = value;
        let Ok(config) = serde_json::from_value::<Config>(v) else { return };
        self.config = config;
        match key {
            "theme" | "accent" => self.restyle(),
            "motion" => self.motion.speed = self.config.motion as f32,
            "keys" => keys::load(&self.config),
            "sites" | "tracking" | "autofill" => self.apply_profile(),
            "fill" => window::place(self.hwnd, &self.config.fill),
            "depth" | "corners" => self.recede(),
            "line" if !self.config.line => self.line(false),
            "zoom" => {
                for t in &self.tabs {
                    if let Some(v) = &t.view {
                        v.set_zoom(self.config.zoom);
                    }
                }
            }
            _ => {}
        }
        self.push_config();
        self.dirty();
    }

    fn restyle(&mut self) {
        let _ = self.motion.colors(backdrop(dark(&self.config)), motion::color(&self.config.accent));
        self.push_config();
    }

    fn apply_profile(&self) {
        let Some(p) = self.shell.as_ref().and_then(View::profile) else { return };
        let scheme = match self.config.sites.as_str() {
            "light" => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT,
            "dark" => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK,
            _ => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_AUTO,
        };
        let tracking = match self.config.tracking.as_str() {
            "off" => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_NONE,
            "basic" => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BASIC,
            "strict" => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT,
            _ => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED,
        };
        unsafe {
            let _ = p.SetPreferredColorScheme(scheme);
            if let Ok(p) = p.cast::<ICoreWebView2Profile3>() {
                let _ = p.SetPreferredTrackingPreventionLevel(tracking);
            }
            if let Ok(p) = p.cast::<ICoreWebView2Profile6>() {
                let _ = p.SetIsGeneralAutofillEnabled(self.config.autofill);
                let _ = p.SetIsPasswordAutosaveEnabled(self.config.autofill);
            }
        }
    }

    fn profile7(&self) -> Option<ICoreWebView2Profile7> {
        self.shell.as_ref().and_then(View::profile).and_then(|p| p.cast().ok())
    }

    fn refresh_exts(&mut self) {
        if let Some(p) = self.profile7() {
            let _ = ext::list(&p);
        }
    }

    fn extension(&mut self, v: &Value) {
        let id = v["id"].as_str().unwrap_or_default();
        let found = self.exts.iter().find(|e| text(|p| unsafe { e.Id(p) }) == id).cloned();
        match v["op"].as_str().unwrap_or_default() {
            "install" => match ext::store_id(v["value"].as_str().unwrap_or_default()) {
                Some(id) => {
                    self.toast("Installing extension");
                    ext::fetch(id, self.version.clone());
                }
                None => self.toast("Paste a Chrome Web Store link or extension ID"),
            },
            "folder" => {
                if let Some(path) = window::folder(self.hwnd) {
                    self.add_ext(path);
                }
            }
            "toggle" => {
                if let Some(e) = found {
                    let _ = ext::toggle(&e, v["on"].as_bool().unwrap_or(true));
                }
            }
            "remove" => {
                if let Some(e) = found {
                    let _ = ext::remove(&e);
                    if let Some(path) = self.config.extensions.remove(id) {
                        if Path::new(&path).starts_with(config::dir()) {
                            let _ = std::fs::remove_dir_all(path);
                        }
                    }
                    self.dirty();
                }
            }
            "open" => {
                let url = format!("chrome-extension://{id}/{}", v["page"].as_str().unwrap_or_default());
                self.open(url, false);
            }
            _ => {}
        }
    }

    fn add_ext(&mut self, path: String) {
        match self.profile7() {
            Some(p) => {
                let _ = ext::add(&p, path);
            }
            None => self.toast("Extensions aren't available right now"),
        }
    }

    fn added(&mut self, path: String, ext: Option<ICoreWebView2BrowserExtension>) {
        let Some(e) = ext else { return self.toast("That extension couldn't be added") };
        let id = text(|p| unsafe { e.Id(p) });
        let name = text(|p| unsafe { e.Name(p) });
        self.config.extensions.insert(id, path);
        self.dirty();
        self.toast(&format!("Added {name}"));
        self.refresh_exts();
    }

    fn push_exts(&self) {
        let items: Vec<Value> = self
            .exts
            .iter()
            .map(|e| {
                let id = text(|p| unsafe { e.Id(p) });
                let mut on = BOOL::default();
                unsafe {
                    let _ = e.IsEnabled(&mut on);
                }
                let (popup, options) = self.config.extensions.get(&id).map(|p| ext::pages(p)).unwrap_or_default();
                json!({
                    "id": id,
                    "name": text(|p| unsafe { e.Name(p) }),
                    "enabled": on.as_bool(),
                    "popup": popup,
                    "options": options,
                })
            })
            .collect();
        self.post(json!({ "t": "exts", "items": items }));
    }

    fn remote(&mut self, r: Remote) {
        match r {
            Remote::Suggest(q, items) if q == self.query => self.post(json!({ "t": "suggest", "q": q, "items": items })),
            Remote::Suggest(..) => {}
            Remote::Unpacked(Ok(path)) => self.add_ext(path),
            Remote::Unpacked(Err(e)) => self.toast(&e),
        }
    }

    fn stats(&self) {
        let Some(env) = &self.env else { return };
        let (bytes, procs) = web::memory(env);
        let live = self.tabs.iter().filter(|t| t.view.is_some() && !t.asleep).count();
        self.post(json!({
            "t": "stats",
            "mb": bytes / 1_048_576,
            "procs": procs,
            "tabs": self.tabs.len(),
            "live": live,
        }));
    }

    fn push_config(&self) {
        let engines: Vec<Value> = search::ENGINES.iter().map(|e| json!({ "id": e.id, "name": e.name })).collect();
        self.post(json!({
            "t": "config",
            "config": self.config,
            "dark": dark(&self.config),
            "engines": engines,
            "version": env!("CARGO_PKG_VERSION"),
            "runtime": self.version,
            "data": config::dir(),
        }));
    }

    fn push_tabs(&self) {
        let tabs: Vec<Value> = self.tabs.iter().map(Tab::json).collect();
        self.post(json!({ "t": "tabs", "tabs": tabs, "active": self.active, "mru": self.mru }));
    }

    fn dirty(&self) {
        window::timer(self.hwnd, SAVE, 1500);
    }

    fn session(&self) -> Session {
        let kept: Vec<&Tab> = self.tabs.iter().filter(|t| t.url.starts_with("http") || t.url.starts_with("file")).collect();
        Session {
            tabs: kept.iter().map(|t| Saved { url: t.url.clone(), title: t.title.clone() }).collect(),
            active: kept.iter().position(|t| Some(t.id) == self.active).unwrap_or(0),
        }
    }

    fn save(&mut self) {
        self.config.save();
        self.session().save();
        self.history.save();
    }

    fn quit(&mut self) {
        if self.leaving {
            return;
        }
        self.leaving = true;
        self.save();
        let token = self.ticket();
        match self.motion.leave(token) {
            Ok(()) => self.done.push((token, Done::Exit)),
            Err(_) => unsafe {
                let _ = DestroyWindow(self.hwnd);
            },
        }
    }
}
