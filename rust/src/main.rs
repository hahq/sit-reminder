//! 久坐提醒 —— 定时提醒你起身活动的托盘小工具（纯 Win32，无运行时依赖）。

#![windows_subsystem = "windows"]

use std::fs;
use std::mem::{size_of, zeroed};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::{GetLocalTime, GetTickCount, GetTickCount64};
use windows_sys::Win32::UI::Controls::{InitCommonControlsEx, DRAWITEMSTRUCT, INITCOMMONCONTROLSEX};
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, SetFocus, LASTINPUTINFO, VK_ESCAPE};
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// ---------- 颜色 ----------
const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}
const BG: u32 = rgb(0x1f, 0x24, 0x30);
const FG: u32 = rgb(0xf2, 0xf4, 0xf8);
const ACCENT: u32 = rgb(0x4c, 0xc3, 0x8a);
const MUTED: u32 = rgb(0x9a, 0xa3, 0xb2);
const BTN: u32 = rgb(0x5a, 0x62, 0x75);
const BTN_DOWN: u32 = rgb(0x6c, 0x75, 0x8a);
const GRAY: u32 = rgb(0x8a, 0x93, 0xa6);
const ORANGE: u32 = rgb(0xf0, 0xa1, 0x40);
const TEXT_MUTED: u32 = rgb(0x6b, 0x72, 0x80);

// ---------- 消息与控件 ID ----------
const WM_TRAY: u32 = WM_APP + 1;
const WM_SHOW_MAIN: u32 = WM_APP + 2;
const TIMER_MAIN: usize = 1;
const TIMER_BREAK: usize = 2;
const TIMER_BREAK_END: usize = 3;

const ID_TOGGLE: usize = 201;
const ID_RESET: usize = 202;
const ID_BREAK: usize = 203;
const ID_EDIT: usize = 211; // 211..=213
const ID_CHECK: usize = 221; // 221..=222
const ID_SNOOZE: usize = 301;
const ID_SKIP: usize = 302;
const ID_M_SHOW: usize = 101;
const ID_M_TOGGLE: usize = 102;
const ID_M_RESET: usize = 103;
const ID_M_BREAK: usize = 104;
const ID_M_QUIT: usize = 105;

// windows-sys 里类型不统一的常量，直接写数值
const BN_CLICKED: u32 = 0;
const EN_CHANGE: u32 = 0x0300;
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const ES_CENTER: u32 = 0x1;
const ES_NUMBER: u32 = 0x2000;
const SS_CENTER: u32 = 0x1;
const SS_ETCHEDHORZ: u32 = 0x10;
const BS_AUTOCHECKBOX: u32 = 0x3;
const BS_OWNERDRAW: u32 = 0xB;
const UDS_SETBUDDYINT: u32 = 0x2;
const UDS_ALIGNRIGHT: u32 = 0x4;
const UDS_ARROWKEYS: u32 = 0x20;
const UDS_NOTHOUSANDS: u32 = 0x80;
const UDM_SETBUDDY: u32 = WM_USER + 105;
const UDM_SETRANGE32: u32 = WM_USER + 111;
const UDM_SETPOS32: u32 = WM_USER + 113;
const PBM_SETPOS: u32 = WM_USER + 2;
const PBM_SETRANGE32: u32 = WM_USER + 6;
const ICC_FLAGS: u32 = 0x4000 | 0x20 | 0x10; // 标准控件 | 进度条 | 微调框
const ODS_SELECTED: u32 = 1;

const MAIN_CLASS: &str = "SitReminderMain";
const BREAK_CLASS: &str = "SitReminderBreak";
const YAHEI: &str = "Microsoft YaHei UI";

const TIPS: [&str; 7] = [
    "站起来，伸个懒腰，转转脖子",
    "去倒杯水，顺便走两步",
    "看看窗外远处，让眼睛放松 20 秒",
    "做 10 个深蹲，唤醒你的腿",
    "耸耸肩、转转手腕，放松肩颈",
    "原地踏步一分钟，促进血液循环",
    "靠墙站直 1 分钟，纠正一下坐姿",
];

// ---------- 配置 ----------
struct Cfg {
    work_min: u32,
    break_min: u32,
    snooze_min: u32,
    sound: bool,
    idle_reset: bool,
    stats_date: String,
    stats_breaks: u32,
}

impl Cfg {
    /// 读取 config.ini；没有时兼容读取旧版 Python 的 config.json。
    fn load(dir: &Path) -> Cfg {
        let mut c = Cfg {
            work_min: 45,
            break_min: 5,
            snooze_min: 5,
            sound: true,
            idle_reset: true,
            stats_date: String::new(),
            stats_breaks: 0,
        };
        let text = fs::read_to_string(dir.join("config.ini"))
            .or_else(|_| fs::read_to_string(dir.join("config.json")))
            .unwrap_or_default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once(['=', ':']) else { continue };
            let k = k.trim().trim_matches('"');
            let v = v.trim().trim_end_matches(',').trim().trim_matches('"');
            let flag = v == "true" || v == "1";
            match k {
                "work_min" => c.work_min = v.parse().unwrap_or(c.work_min).clamp(1, 240),
                "break_min" => c.break_min = v.parse().unwrap_or(c.break_min).clamp(1, 60),
                "snooze_min" => c.snooze_min = v.parse().unwrap_or(c.snooze_min).clamp(1, 60),
                "sound" => c.sound = flag,
                "idle_reset" => c.idle_reset = flag,
                "stats_date" => c.stats_date = v.to_string(),
                "stats_breaks" => c.stats_breaks = v.parse().unwrap_or(0),
                _ => {}
            }
        }
        c
    }

    fn save(&self, dir: &Path) {
        let text = format!(
            "work_min={}\nbreak_min={}\nsnooze_min={}\nsound={}\nidle_reset={}\nstats_date={}\nstats_breaks={}\n",
            self.work_min, self.break_min, self.snooze_min, self.sound, self.idle_reset,
            self.stats_date, self.stats_breaks
        );
        let _ = fs::write(dir.join("config.ini"), text);
    }
}

// ---------- 全局状态（单线程 GUI） ----------
#[derive(Clone, Copy, PartialEq)]
enum State {
    Working,
    Paused,
    Break,
}

struct BreakWin {
    hwnd: HWND,
    deadline: i64,
    done: bool,
    tip: &'static str,
    sat_min: i64,
    center: (i32, i32), // 主显示器中心（窗口坐标）
    width: i32,         // 主显示器宽度
    fonts: [HFONT; 6],
    clock_text: String,
}

struct App {
    cfg: Cfg,
    dir: PathBuf,
    dpi: i32,
    state: State,
    total: i64,
    remaining: i64,
    deadline: i64,
    font: HFONT,
    clock_font: HFONT,
    main: HWND,
    status: HWND,
    clock: HWND,
    bar: HWND,
    toggle: HWND,
    edits: [HWND; 3],
    checks: [HWND; 2],
    stats: HWND,
    brk: Option<BreakWin>,
    icons: [HICON; 3],
    big_icon: HICON,
    tray_icon: usize,
    tray_tip: String,
    taskbar_msg: u32,
    hinted: bool,
    loading: bool,
    last_clock: String,
}

static mut APP: *mut App = null_mut();

fn app() -> &'static mut App {
    unsafe { &mut *APP }
}

// ---------- 小工具 ----------
fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn copy_w(dst: &mut [u16], s: &str) {
    let v: Vec<u16> = s.encode_utf16().take(dst.len() - 1).collect();
    dst[..v.len()].copy_from_slice(&v);
    dst[v.len()] = 0;
}

fn now() -> i64 {
    unsafe { GetTickCount64() as i64 }
}

fn fmt(ms: i64) -> String {
    let s = (ms.max(0) + 999) / 1000;
    format!("{:02}:{:02}", s / 60, s % 60)
}

fn today() -> String {
    unsafe {
        let mut st: SYSTEMTIME = zeroed();
        GetLocalTime(&mut st);
        format!("{:04}-{:02}-{:02}", st.wYear, st.wMonth, st.wDay)
    }
}

/// 距离上次键盘/鼠标操作的毫秒数
fn idle_ms() -> i64 {
    unsafe {
        let mut li = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        if GetLastInputInfo(&mut li) == 0 {
            return 0;
        }
        GetTickCount().wrapping_sub(li.dwTime) as i64
    }
}

fn s(v: i32) -> i32 {
    v * app().dpi / 96
}

fn hinst() -> HINSTANCE {
    unsafe { GetModuleHandleW(null()) }
}

unsafe fn font(pt: i32, bold: bool, face: &str) -> HFONT {
    CreateFontW(
        -(pt * app().dpi / 72), 0, 0, 0, if bold { 700 } else { 400 },
        0, 0, 0, 1, 0, 0, 5, 0, w(face).as_ptr(),
    )
}

unsafe fn get_text(h: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

unsafe fn set_text(h: HWND, text: &str) {
    if get_text(h) != text {
        SetWindowTextW(h, w(text).as_ptr());
    }
}

unsafe fn child(parent: HWND, class: &str, text: &str, style: u32, ex: u32,
                x: i32, y: i32, cw: i32, ch: i32, id: usize) -> HWND {
    let h = CreateWindowExW(
        ex, w(class).as_ptr(), w(text).as_ptr(), WS_CHILD | WS_VISIBLE | style,
        s(x), s(y), s(cw), s(ch), parent, id as HMENU, hinst(), null(),
    );
    SendMessageW(h, WM_SETFONT, app().font as WPARAM, 1);
    h
}

/// 画一个时钟图标，颜色表示状态
unsafe fn make_icon(color: u32, size: i32) -> HICON {
    let screen = GetDC(null_mut());
    let dc = CreateCompatibleDC(screen);
    let color_bmp = CreateCompatibleBitmap(screen, size, size);
    let mask_bmp = CreateBitmap(size, size, 1, 1, null());
    ReleaseDC(null_mut(), screen);
    let full = RECT { left: 0, top: 0, right: size, bottom: size };
    let null_pen = GetStockObject(NULL_PEN);

    let old = SelectObject(dc, color_bmp);
    FillRect(dc, &full, GetStockObject(BLACK_BRUSH));
    let fill = CreateSolidBrush(color);
    SelectObject(dc, null_pen);
    SelectObject(dc, fill);
    Ellipse(dc, 0, 0, size + 1, size + 1);
    let inset = size / 6;
    SelectObject(dc, GetStockObject(WHITE_BRUSH));
    Ellipse(dc, inset, inset, size - inset + 1, size - inset + 1);
    let pen = CreatePen(PS_SOLID, (size / 9).max(2), color);
    SelectObject(dc, pen);
    let c = size / 2;
    MoveToEx(dc, c, c, null_mut());
    LineTo(dc, c, c - size * 3 / 10);
    MoveToEx(dc, c, c, null_mut());
    LineTo(dc, c + size / 4, c + size / 8);

    SelectObject(dc, mask_bmp);
    FillRect(dc, &full, GetStockObject(WHITE_BRUSH));
    SelectObject(dc, null_pen);
    SelectObject(dc, GetStockObject(BLACK_BRUSH));
    Ellipse(dc, 0, 0, size + 1, size + 1);

    SelectObject(dc, old);
    DeleteObject(fill);
    DeleteObject(pen);
    let ii = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask_bmp, hbmColor: color_bmp };
    let icon = CreateIconIndirect(&ii);
    DeleteObject(color_bmp);
    DeleteObject(mask_bmp);
    DeleteDC(dc);
    icon
}

/// 读取 build.rs 嵌入的图标组（资源 ID 1）
unsafe fn load_icon(size: i32) -> Option<HICON> {
    let h = LoadImageW(hinst(), 1 as _, IMAGE_ICON, size, size, 0);
    (!h.is_null()).then_some(h as HICON)
}

// ---------- 托盘 ----------
unsafe fn nid() -> NOTIFYICONDATAW {
    let mut n: NOTIFYICONDATAW = zeroed();
    n.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    n.hWnd = app().main;
    n.uID = 1;
    n
}

unsafe fn tray_add() {
    let a = app();
    let mut n = nid();
    n.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    n.uCallbackMessage = WM_TRAY;
    n.hIcon = a.icons[a.tray_icon];
    copy_w(&mut n.szTip, &a.tray_tip);
    Shell_NotifyIconW(NIM_ADD, &n);
}

unsafe fn tray_update() {
    let a = app();
    let (idx, tip) = match a.state {
        State::Working => (0, format!("久坐提醒 · 还有 {} 起来活动", fmt(a.remaining))),
        State::Paused => (1, format!("久坐提醒 · 已暂停（剩 {}）", fmt(a.remaining))),
        State::Break => (2, "久坐提醒 · 休息中".to_string()),
    };
    if idx == a.tray_icon && tip == a.tray_tip {
        return;
    }
    a.tray_icon = idx;
    a.tray_tip = tip;
    let mut n = nid();
    n.uFlags = NIF_ICON | NIF_TIP;
    n.hIcon = a.icons[idx];
    copy_w(&mut n.szTip, &a.tray_tip);
    Shell_NotifyIconW(NIM_MODIFY, &n);
}

unsafe fn tray_notify(title: &str, text: &str) {
    let mut n = nid();
    n.uFlags = NIF_INFO;
    n.dwInfoFlags = NIIF_INFO;
    copy_w(&mut n.szInfoTitle, title);
    copy_w(&mut n.szInfo, text);
    Shell_NotifyIconW(NIM_MODIFY, &n);
}

unsafe fn tray_menu() {
    let a = app();
    let in_break = a.state == State::Break;
    let grey = if in_break { MF_GRAYED } else { 0 };
    let m = CreatePopupMenu();
    AppendMenuW(m, MF_STRING, ID_M_SHOW, w("显示主窗口").as_ptr());
    let toggle_label = if a.state == State::Paused { "继续" } else { "暂停" };
    AppendMenuW(m, MF_STRING | grey, ID_M_TOGGLE, w(toggle_label).as_ptr());
    AppendMenuW(m, MF_STRING, ID_M_RESET, w("重新计时").as_ptr());
    AppendMenuW(m, MF_STRING | grey, ID_M_BREAK, w("现在休息").as_ptr());
    AppendMenuW(m, MF_SEPARATOR, 0, null());
    AppendMenuW(m, MF_STRING, ID_M_QUIT, w("退出").as_ptr());
    SetMenuDefaultItem(m, ID_M_SHOW as u32, 0);
    let mut pt: POINT = zeroed();
    GetCursorPos(&mut pt);
    SetForegroundWindow(a.main);
    let cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, pt.x, pt.y, 0, a.main, null());
    PostMessageW(a.main, WM_NULL, 0, 0);
    DestroyMenu(m);
    match cmd as usize {
        ID_M_SHOW => show_main(),
        ID_M_TOGGLE => toggle(),
        ID_M_RESET => reset(),
        ID_M_BREAK => trigger_break(),
        ID_M_QUIT => {
            DestroyWindow(a.main);
        }
        _ => {}
    }
}

// ---------- 计时逻辑 ----------
unsafe fn set_status(text: &str) {
    set_text(app().status, text);
}

unsafe fn start(ms: Option<i64>) {
    let a = app();
    a.total = ms.unwrap_or(a.cfg.work_min as i64 * 60_000);
    a.remaining = a.total;
    a.deadline = now() + a.total;
    a.state = State::Working;
    set_text(a.toggle, "暂停");
    set_status("专注中，到点会提醒你起来活动");
}

unsafe fn reset() {
    if app().state == State::Break {
        finish_break(false, false);
    } else {
        start(None);
    }
}

unsafe fn toggle() {
    let a = app();
    match a.state {
        State::Working => {
            a.remaining = a.deadline - now();
            a.state = State::Paused;
            set_text(a.toggle, "继续");
            set_status("已暂停");
        }
        State::Paused => {
            a.deadline = now() + a.remaining;
            a.state = State::Working;
            set_text(a.toggle, "暂停");
            set_status("专注中，到点会提醒你起来活动");
        }
        State::Break => {}
    }
}

unsafe fn tick() {
    let a = app();
    if a.state == State::Working {
        if a.cfg.idle_reset && idle_ms() >= a.cfg.break_min as i64 * 60_000 {
            // 人已经离开足够久，相当于休息过了
            start(None);
            set_status("检测到你离开了电脑，已重新计时");
        }
        a.remaining = a.deadline - now();
        if a.remaining <= 0 {
            trigger_break();
        }
    }
    if a.state != State::Break {
        let t = fmt(a.remaining);
        if t != a.last_clock {
            SetWindowTextW(a.clock, w(&t).as_ptr());
            SetWindowTextW(a.main, w(&format!("久坐提醒 · {t}")).as_ptr());
            a.last_clock = t;
        }
        let pos = if a.total > 0 { 1000 - a.remaining.max(0) * 1000 / a.total } else { 0 };
        SendMessageW(a.bar, PBM_SETPOS, pos as WPARAM, 0);
    }
    let d = today();
    if a.cfg.stats_date != d {
        a.cfg.stats_date = d;
        a.cfg.stats_breaks = 0;
        a.cfg.save(&a.dir);
    }
    set_text(a.stats, &format!("今天已完成休息 {} 次", a.cfg.stats_breaks));
    tray_update();
}

unsafe fn on_settings_changed() {
    let a = app();
    if a.loading {
        return;
    }
    let ranges = [(1, 240), (1, 60), (1, 60)];
    let mut vals = [a.cfg.work_min, a.cfg.break_min, a.cfg.snooze_min];
    for i in 0..3 {
        if let Ok(n) = get_text(a.edits[i]).trim().parse::<u32>() {
            vals[i] = n.clamp(ranges[i].0, ranges[i].1);
        }
    }
    let work_changed = vals[0] != a.cfg.work_min;
    a.cfg.work_min = vals[0];
    a.cfg.break_min = vals[1];
    a.cfg.snooze_min = vals[2];
    a.cfg.sound = SendMessageW(a.checks[0], BM_GETCHECK, 0, 0) == 1;
    a.cfg.idle_reset = SendMessageW(a.checks[1], BM_GETCHECK, 0, 0) == 1;
    a.cfg.save(&a.dir);
    if work_changed && a.state != State::Break {
        start(None);
    }
}

// ---------- 主窗口 ----------
unsafe fn show_main() {
    let m = app().main;
    ShowWindow(m, if IsIconic(m) != 0 { SW_RESTORE } else { SW_SHOW });
    SetForegroundWindow(m);
}

unsafe fn hide_main() {
    let a = app();
    ShowWindow(a.main, SW_HIDE);
    if !a.hinted {
        a.hinted = true;
        tray_notify("久坐提醒", "仍在后台计时，右键托盘图标可退出");
    }
}

unsafe fn build_main_ui() {
    let a = app();
    let m = a.main;
    a.status = child(m, "STATIC", "", SS_CENTER, 0, 20, 14, 300, 20, 0);
    a.clock = child(m, "STATIC", "", SS_CENTER, 0, 20, 36, 300, 60, 0);
    SendMessageW(a.clock, WM_SETFONT, a.clock_font as WPARAM, 1);
    a.bar = child(m, "msctls_progress32", "", 0, 0, 30, 102, 280, 12, 0);
    SendMessageW(a.bar, PBM_SETRANGE32, 0, 1000);

    a.toggle = child(m, "BUTTON", "暂停", WS_TABSTOP, 0, 20, 128, 94, 30, ID_TOGGLE);
    child(m, "BUTTON", "重新计时", WS_TABSTOP, 0, 123, 128, 94, 30, ID_RESET);
    child(m, "BUTTON", "现在休息", WS_TABSTOP, 0, 226, 128, 94, 30, ID_BREAK);
    child(m, "STATIC", "", SS_ETCHEDHORZ, 0, 20, 176, 300, 2, 0);

    let rows = [
        ("每坐多久提醒（分钟）", a.cfg.work_min, 240),
        ("每次休息时长（分钟）", a.cfg.break_min, 60),
        ("稍后提醒（分钟）", a.cfg.snooze_min, 60),
    ];
    for (i, (label, val, max)) in rows.into_iter().enumerate() {
        let y = 192 + i as i32 * 34;
        child(m, "STATIC", label, 0, 0, 20, y + 4, 200, 20, 0);
        let edit = child(m, "EDIT", &val.to_string(), WS_TABSTOP | ES_NUMBER | ES_CENTER,
                         WS_EX_CLIENTEDGE, 236, y, 84, 26, ID_EDIT + i);
        let ud = CreateWindowExW(
            0, w("msctls_updown32").as_ptr(), null(),
            WS_CHILD | WS_VISIBLE | UDS_SETBUDDYINT | UDS_ALIGNRIGHT | UDS_ARROWKEYS | UDS_NOTHOUSANDS,
            0, 0, 0, 0, m, null_mut(), hinst(), null(),
        );
        SendMessageW(ud, UDM_SETBUDDY, edit as WPARAM, 0);
        SendMessageW(ud, UDM_SETRANGE32, 1, max);
        SendMessageW(ud, UDM_SETPOS32, 0, val as LPARAM);
        a.edits[i] = edit;
    }

    let checks = [("提醒时播放声音", a.cfg.sound), ("离开电脑超过休息时长，自动重新计时", a.cfg.idle_reset)];
    for (i, (label, on)) in checks.into_iter().enumerate() {
        let h = child(m, "BUTTON", label, WS_TABSTOP | BS_AUTOCHECKBOX, 0,
                      20, 298 + i as i32 * 26, 310, 22, ID_CHECK + i);
        SendMessageW(h, BM_SETCHECK, on as WPARAM, 0);
        a.checks[i] = h;
    }
    a.stats = child(m, "STATIC", "", SS_CENTER, 0, 20, 360, 300, 20, 0);
}

unsafe extern "system" fn main_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if APP.is_null() {
        return DefWindowProcW(h, msg, wp, lp);
    }
    let a = app();
    if msg == a.taskbar_msg && msg != 0 {
        tray_add(); // 资源管理器重启后重新放回托盘图标
        return 0;
    }
    match msg {
        WM_TIMER => {
            tick();
            0
        }
        WM_COMMAND => {
            let id = wp & 0xFFFF;
            let code = ((wp >> 16) & 0xFFFF) as u32;
            match id {
                ID_TOGGLE if code == BN_CLICKED => toggle(),
                ID_RESET if code == BN_CLICKED => reset(),
                ID_BREAK if code == BN_CLICKED => trigger_break(),
                _ if (ID_EDIT..ID_EDIT + 3).contains(&id) && code == EN_CHANGE => on_settings_changed(),
                _ if (ID_CHECK..ID_CHECK + 2).contains(&id) && code == BN_CLICKED => on_settings_changed(),
                _ => {}
            }
            0
        }
        WM_CTLCOLORSTATIC => {
            let hdc = wp as HDC;
            let ctl = lp as HWND;
            SetBkColor(hdc, GetSysColor(COLOR_WINDOW));
            let muted = ctl == a.status || ctl == a.stats;
            SetTextColor(hdc, if muted { TEXT_MUTED } else { GetSysColor(COLOR_WINDOWTEXT) });
            GetSysColorBrush(COLOR_WINDOW) as LRESULT
        }
        WM_TRAY => {
            match (lp as u32) & 0xFFFF {
                WM_LBUTTONUP => show_main(),
                WM_RBUTTONUP => tray_menu(),
                _ => {}
            }
            0
        }
        WM_SHOW_MAIN => {
            show_main();
            0
        }
        WM_CLOSE => {
            hide_main();
            0
        }
        WM_DESTROY => {
            Shell_NotifyIconW(NIM_DELETE, &nid());
            a.cfg.save(&a.dir);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

// ---------- 休息窗口 ----------
unsafe fn trigger_break() {
    let a = app();
    if a.state == State::Break {
        return;
    }
    a.state = State::Break;
    set_status("休息时间");
    if a.cfg.sound {
        MessageBeep(MB_ICONEXCLAMATION);
    }

    let (vx, vy) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
    let (vw, vh) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
    let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
    let center = (-vx + sw / 2, -vy + sh / 2);
    a.brk = Some(BreakWin {
        hwnd: null_mut(),
        deadline: now() + a.cfg.break_min as i64 * 60_000,
        done: false,
        tip: TIPS[GetTickCount() as usize % TIPS.len()],
        sat_min: (a.total + 30_000) / 60_000,
        center,
        width: sw,
        fonts: [
            font(40, true, YAHEI),
            font(16, false, YAHEI),
            font(20, false, YAHEI),
            font(72, true, "Consolas"),
            font(14, false, YAHEI),
            font(48, true, YAHEI),
        ],
        clock_text: fmt(a.cfg.break_min as i64 * 60_000),
    });

    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
        w(BREAK_CLASS).as_ptr(), w("久坐提醒").as_ptr(), WS_POPUP,
        vx, vy, vw, vh, null_mut(), null_mut(), hinst(), null(),
    );
    SetLayeredWindowAttributes(hwnd, 0, 240, LWA_ALPHA);
    let (bw, bh, gap) = (s(180), s(46), s(24));
    let y = center.1 + s(130);
    let buttons = [
        (format!("稍后 {} 分钟", a.cfg.snooze_min), center.0 - bw - gap / 2, ID_SNOOZE),
        ("跳过本次".to_string(), center.0 + gap / 2, ID_SKIP),
    ];
    let mut first: HWND = null_mut();
    for (text, x, id) in buttons {
        let b = CreateWindowExW(
            0, w("BUTTON").as_ptr(), w(&text).as_ptr(), WS_CHILD | WS_VISIBLE | BS_OWNERDRAW,
            x, y, bw, bh, hwnd, id as HMENU, hinst(), null(),
        );
        if first.is_null() {
            first = b;
        }
    }
    a.brk.as_mut().unwrap().hwnd = hwnd;
    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
    SetFocus(first);
    SetTimer(hwnd, TIMER_BREAK, 250, None);
}

unsafe fn finish_break(completed: bool, snooze: bool) {
    let a = app();
    let Some(b) = a.brk.take() else { return };
    DestroyWindow(b.hwnd);
    for f in b.fonts {
        DeleteObject(f);
    }
    if completed {
        a.cfg.stats_breaks += 1;
        a.cfg.save(&a.dir);
    }
    if snooze {
        start(Some(a.cfg.snooze_min as i64 * 60_000));
        set_status(&format!("已推迟 {} 分钟", a.cfg.snooze_min));
    } else {
        start(None);
    }
}

fn band(b: &BreakWin, top: i32, bottom: i32) -> RECT {
    RECT {
        left: b.center.0 - b.width / 2,
        right: b.center.0 + b.width / 2,
        top: b.center.1 + s(top),
        bottom: b.center.1 + s(bottom),
    }
}

const CLOCK_BAND: (i32, i32) = (-40, 100);

unsafe fn draw_line(hdc: HDC, f: HFONT, color: u32, text: &str, mut r: RECT) {
    SelectObject(hdc, f);
    SetTextColor(hdc, color);
    DrawTextW(hdc, w(text).as_ptr(), -1, &mut r, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
}

unsafe fn paint_break(h: HWND) {
    let Some(b) = app().brk.as_ref() else { return };
    let mut ps: PAINTSTRUCT = zeroed();
    let hdc = BeginPaint(h, &mut ps);
    let bg = CreateSolidBrush(BG);
    FillRect(hdc, &ps.rcPaint, bg);
    DeleteObject(bg);
    SetBkMode(hdc, TRANSPARENT as _);
    draw_line(hdc, b.fonts[0], FG, "该起来活动一下了！", band(b, -250, -180));
    draw_line(hdc, b.fonts[1], MUTED, &format!("你已经连续坐了 {} 分钟", b.sat_min), band(b, -170, -135));
    draw_line(hdc, b.fonts[2], ACCENT, b.tip, band(b, -115, -65));
    let clock = band(b, CLOCK_BAND.0, CLOCK_BAND.1);
    if b.done {
        draw_line(hdc, b.fonts[5], ACCENT, "休息完成！", clock);
    } else {
        draw_line(hdc, b.fonts[3], FG, &b.clock_text, clock);
    }
    EndPaint(h, &ps);
}

unsafe fn draw_button(d: &DRAWITEMSTRUCT) {
    let Some(b) = app().brk.as_ref() else { return };
    let hdc = d.hDC;
    let bg = CreateSolidBrush(BG);
    FillRect(hdc, &d.rcItem, bg);
    DeleteObject(bg);
    let fill = CreateSolidBrush(if d.itemState & ODS_SELECTED != 0 { BTN_DOWN } else { BTN });
    let old_brush = SelectObject(hdc, fill);
    let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
    let r = d.rcItem;
    RoundRect(hdc, r.left, r.top, r.right + 1, r.bottom + 1, s(12), s(12));
    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
    DeleteObject(fill);
    SetBkMode(hdc, TRANSPARENT as _);
    draw_line(hdc, b.fonts[4], FG, &get_text(d.hwndItem), r);
}

unsafe extern "system" fn break_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER if wp == TIMER_BREAK => {
            let a = app();
            let Some(b) = a.brk.as_mut() else { return 0 };
            let left = b.deadline - now();
            let text = fmt(left);
            if left <= 0 && !b.done {
                b.done = true;
                KillTimer(h, TIMER_BREAK);
                if a.cfg.sound {
                    MessageBeep(MB_OK);
                }
                SetTimer(h, TIMER_BREAK_END, 1500, None);
            }
            if b.done || text != b.clock_text {
                b.clock_text = text;
                let r = band(b, CLOCK_BAND.0, CLOCK_BAND.1);
                InvalidateRect(h, &r, 0);
            }
            0
        }
        WM_TIMER if wp == TIMER_BREAK_END => {
            finish_break(true, false);
            0
        }
        WM_COMMAND if ((wp >> 16) & 0xFFFF) as u32 == BN_CLICKED => {
            match wp & 0xFFFF {
                ID_SNOOZE => finish_break(false, true),
                ID_SKIP => finish_break(false, false),
                _ => {}
            }
            0
        }
        WM_DRAWITEM => {
            draw_button(&*(lp as *const DRAWITEMSTRUCT));
            1
        }
        WM_PAINT => {
            paint_break(h);
            0
        }
        WM_ERASEBKGND => 1,
        WM_CLOSE => 0,
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

// ---------- 入口 ----------
unsafe fn register(class: &str, proc_: WNDPROC, icon: HICON, small: HICON, bg: HBRUSH) {
    let name = w(class);
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: proc_,
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinst(),
        hIcon: icon,
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: bg,
        lpszMenuName: null(),
        lpszClassName: name.as_ptr(),
        hIconSm: small,
    };
    RegisterClassExW(&wc);
}

fn main() {
    unsafe {
        // 只允许运行一个：已经开着就把它的窗口叫出来
        let existing = FindWindowW(w(MAIN_CLASS).as_ptr(), null());
        if !existing.is_null() {
            PostMessageW(existing, WM_SHOW_MAIN, 0, 0);
            return;
        }
        let icc = INITCOMMONCONTROLSEX { dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_FLAGS };
        InitCommonControlsEx(&icc);

        let dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_default();
        let mut cfg = Cfg::load(&dir);
        if cfg.stats_date != today() {
            cfg.stats_date = today();
            cfg.stats_breaks = 0;
        }
        APP = Box::into_raw(Box::new(App {
            cfg,
            dir,
            dpi: GetDpiForSystem() as i32,
            state: State::Working,
            total: 0,
            remaining: 0,
            deadline: 0,
            font: null_mut(),
            clock_font: null_mut(),
            main: null_mut(),
            status: null_mut(),
            clock: null_mut(),
            bar: null_mut(),
            toggle: null_mut(),
            edits: [null_mut(); 3],
            checks: [null_mut(); 2],
            stats: null_mut(),
            brk: None,
            icons: [null_mut(); 3],
            big_icon: null_mut(),
            tray_icon: 0,
            tray_tip: "久坐提醒".to_string(),
            taskbar_msg: RegisterWindowMessageW(w("TaskbarCreated").as_ptr()),
            hinted: false,
            loading: true,
            last_clock: String::new(),
        }));
        let a = app();
        a.font = font(10, false, YAHEI);
        a.clock_font = font(36, true, "Consolas");
        let small = GetSystemMetrics(SM_CXSMICON);
        a.icons = [make_icon(ACCENT, small), make_icon(GRAY, small), make_icon(ORANGE, small)];
        // 窗口图标优先用 exe 内嵌的图标（build.rs 嵌入，抗锯齿更清晰），托盘图标仍按状态现画
        a.big_icon = load_icon(GetSystemMetrics(SM_CXICON)).unwrap_or_else(|| make_icon(ACCENT, GetSystemMetrics(SM_CXICON)));
        let small_icon = load_icon(small).unwrap_or(a.icons[0]);

        register(MAIN_CLASS, Some(main_proc), a.big_icon, small_icon, (COLOR_WINDOW + 1) as usize as HBRUSH);
        register(BREAK_CLASS, Some(break_proc), a.big_icon, small_icon, null_mut());

        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let mut r = RECT { left: 0, top: 0, right: s(340), bottom: s(392) };
        AdjustWindowRectEx(&mut r, style, 0, 0);
        let (ww, wh) = (r.right - r.left, r.bottom - r.top);
        let x = (GetSystemMetrics(SM_CXSCREEN) - ww) / 2;
        let y = (GetSystemMetrics(SM_CYSCREEN) - wh) / 2;
        a.main = CreateWindowExW(
            0, w(MAIN_CLASS).as_ptr(), w("久坐提醒").as_ptr(), style,
            x, y, ww, wh, null_mut(), null_mut(), hinst(), null(),
        );
        build_main_ui();
        a.loading = false;
        tray_add();
        start(None);
        tick();
        SetTimer(a.main, TIMER_MAIN, 500, None);
        if !std::env::args().any(|x| x == "--hidden") {
            ShowWindow(a.main, SW_SHOW);
        }

        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if msg.message == WM_KEYDOWN && msg.wParam == VK_ESCAPE as WPARAM && app().brk.is_some() {
                finish_break(false, true); // Esc = 稍后提醒
                continue;
            }
            if IsDialogMessageW(app().main, &msg) != 0 {
                continue; // Tab 键在控件间切换
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
