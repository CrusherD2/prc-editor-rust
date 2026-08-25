//! Kill the white Win32/DXGI flash before the first egui frame is presented.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

static HIDING_MAIN_WINDOW: AtomicBool = AtomicBool::new(true);

const WH_CBT: i32 = 5;
const HCBT_CREATEWND: i32 = 3;
const WM_ERASEBKGND: u32 = 0x0014;
const WS_CAPTION: i32 = 0x00C0_0000;
const WS_EX_NOREDIRECTIONBITMAP: u32 = 0x0020_0000;
const GCLP_HBRBACKGROUND: i32 = -10;
const BLACK_BRUSH: i32 = 4;
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const DWMWA_CAPTION_COLOR: u32 = 35;
const DWMWA_BORDER_COLOR: u32 = 34;
const SW_HIDE: i32 = 0;

#[repr(C)]
struct CreateStructW {
    lp_create_params: *mut c_void,
    h_instance: isize,
    h_menu: isize,
    hwnd_parent: isize,
    cy: i32,
    cx: i32,
    y: i32,
    x: i32,
    style: i32,
    lpsz_name: *const u16,
    lpsz_class: *const u16,
    dw_ex_style: u32,
}

#[repr(C)]
struct CbtCreateWndW {
    lpcs: *mut CreateStructW,
    hwnd_insert_after: isize,
}

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
struct Msg {
    hwnd: isize,
    message: u32,
    #[cfg(target_pointer_width = "64")]
    _pad: u32,
    w_param: usize,
    l_param: isize,
    time: u32,
    pt: Point,
}

#[repr(C)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn SetWindowsHookExW(
        id_hook: i32,
        lpfn: unsafe extern "system" fn(i32, usize, isize) -> isize,
        hmod: isize,
        dw_thread_id: u32,
    ) -> isize;
    fn CallNextHookEx(hhk: isize, n_code: i32, w_param: usize, l_param: isize) -> isize;
    fn GetCurrentThreadId() -> u32;
    fn GetStockObject(index: i32) -> isize;
    fn FillRect(hdc: isize, lprc: *const Rect, hbr: isize) -> i32;
    fn GetClientRect(hwnd: isize, lp_rect: *mut Rect) -> i32;
    fn ShowWindow(hwnd: isize, n_cmd_show: i32) -> i32;
    #[cfg(target_pointer_width = "64")]
    fn SetClassLongPtrW(hwnd: isize, index: i32, new_long: isize) -> isize;
    #[cfg(target_pointer_width = "32")]
    fn SetClassLongW(hwnd: isize, index: i32, new_long: isize) -> isize;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(hwnd: isize, attr: u32, value: *const u32, size: u32) -> i32;
}

fn apply_dark_frame(hwnd: isize) {
    unsafe {
        let brush = GetStockObject(BLACK_BRUSH);
        #[cfg(target_pointer_width = "64")]
        SetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND, brush);
        #[cfg(target_pointer_width = "32")]
        SetClassLongW(hwnd, GCLP_HBRBACKGROUND, brush);

        let dark: u32 = 1;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark,
            std::mem::size_of::<u32>() as u32,
        );
        // RGB(27,27,27) matches the dark UI.
        let caption: u32 = 0x001B1B1B;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            &caption,
            std::mem::size_of::<u32>() as u32,
        );
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &caption,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

unsafe extern "system" fn cbt_proc(n_code: i32, w_param: usize, l_param: isize) -> isize {
    if n_code == HCBT_CREATEWND && l_param != 0 {
        unsafe {
            let create = &mut *(l_param as *mut CbtCreateWndW);
            if !create.lpcs.is_null() {
                let cs = &mut *create.lpcs;
                if cs.style & WS_CAPTION != 0 {
                    cs.dw_ex_style |= WS_EX_NOREDIRECTIONBITMAP;
                    apply_dark_frame(w_param as isize);
                    if HIDING_MAIN_WINDOW.load(Ordering::SeqCst) {
                        ShowWindow(w_param as isize, SW_HIDE);
                    }
                }
            }
        }
    }
    unsafe { CallNextHookEx(0, n_code, w_param, l_param) }
}

/// Install a thread CBT hook so the main window is created dark and hidden.
pub fn install_create_hook() {
    unsafe {
        SetWindowsHookExW(WH_CBT, cbt_proc, 0, GetCurrentThreadId());
    }
}

pub fn swallow_white_erase<T: 'static>(builder: &mut eframe::EventLoopBuilder<T>) {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    builder.with_msg_hook(|msg| {
        let msg = msg as *const Msg;
        if msg.is_null() {
            return false;
        }
        unsafe {
            if (*msg).message == WM_ERASEBKGND {
                let mut rect = Rect {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                GetClientRect((*msg).hwnd, &mut rect);
                FillRect((*msg).w_param as isize, &rect, GetStockObject(BLACK_BRUSH));
                return true;
            }
        }
        false
    });
}

pub fn hide_until_first_frame(cc: &eframe::CreationContext<'_>) {
    if let Some(window) = cc.winit_window() {
        window.set_visible(false);
        window.set_theme(Some(winit::window::Theme::Dark));
    }
    HIDING_MAIN_WINDOW.store(false, Ordering::SeqCst);
}
