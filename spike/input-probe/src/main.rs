//! M1/M2 spike：驗證 `SendInput` 在本機環境可用（EDR 未攔截），
//! 並自動測試 QQKey 的全域快捷鍵是否真的被攔截到。
//!
//! 用法：
//!   `cargo run -- --alt-q`              送出 Alt+Q，回報 QQKey 候選框是否因此顯示
//!   `cargo run -- --find <關鍵字>`      列出標題含關鍵字的可見視窗
//!   `cargo run -- --type <文字>`        以 Unicode 逐字送出文字到目前前景視窗

use std::mem::size_of;
use std::{thread, time::Duration};

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_LMENU, VK_Q,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, IsWindowVisible,
};

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--alt-q") => test_alt_q(),
        Some("--find") => {
            let keyword = args.get(1).map(String::as_str).unwrap_or("");
            for (hwnd, title) in find_windows(keyword) {
                println!("  {:?}  {title}", hwnd.0);
            }
        }
        Some("--type") => {
            let text = args.get(1).map(String::as_str).unwrap_or("");
            println!("3 秒後送出：{text:?}");
            thread::sleep(Duration::from_secs(3));
            let sent = send_text(text);
            println!("送出 {sent} 個事件");
        }
        _ => {
            eprintln!("用法：--alt-q | --find <關鍵字> | --type <文字>");
        }
    }
}

/// 候選框視窗的標題。用完全比對而非包含比對——開著專案資料夾的檔案總管
/// 與執行中的終端機，標題都會含有 "QQKey"。
const LAUNCHER_TITLE: &str = "QQKey";

fn launcher_visible() -> bool {
    find_windows(LAUNCHER_TITLE)
        .iter()
        .any(|(_, title)| title == LAUNCHER_TITLE)
}

/// 送出 Alt+Q 後檢查 QQKey 候選框是否現身，用來確認全域快捷鍵註冊成功。
fn test_alt_q() {
    let before = launcher_visible();
    println!("送出前候選框：{}", if before { "顯示中" } else { "隱藏" });

    let sent = send_alt_q();
    println!("已送出 {sent} 個鍵盤事件，等待候選框反應…");
    thread::sleep(Duration::from_millis(600));

    let after = launcher_visible();
    println!("送出後候選框：{}", if after { "顯示中" } else { "隱藏" });

    if before == after {
        println!("\n結果：狀態未改變，快捷鍵可能未註冊成功或被其他程式攔截。");
    } else if after {
        println!("\n結果：全域快捷鍵生效，候選框已顯示。");
    } else {
        println!("\n結果：全域快捷鍵生效，候選框已收起。");
    }
}

// ---------------------------------------------------------------- 鍵盤輸入

fn key_event(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send_alt_q() -> u32 {
    let inputs = [
        key_event(VK_LMENU, KEYBD_EVENT_FLAGS(0)),
        key_event(VK_Q, KEYBD_EVENT_FLAGS(0)),
        key_event(VK_Q, KEYEVENTF_KEYUP),
        key_event(VK_LMENU, KEYEVENTF_KEYUP),
    ];
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) }
}

/// 以 `KEYEVENTF_UNICODE` 逐個 UTF-16 code unit 送出，不受鍵盤配置影響。
fn send_text(text: &str) -> u32 {
    let inputs: Vec<INPUT> = text
        .encode_utf16()
        .flat_map(|unit| {
            let make = |flags: KEYBD_EVENT_FLAGS| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: unit,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            [
                make(KEYEVENTF_UNICODE),
                make(KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
            ]
        })
        .collect();

    if inputs.is_empty() {
        return 0;
    }
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) }
}

// ---------------------------------------------------------------- 視窗搜尋

struct FindContext {
    keyword: String,
    found: Vec<(HWND, String)>,
}

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let context = unsafe { &mut *(lparam.0 as *mut FindContext) };
    if unsafe { IsWindowVisible(hwnd) }.as_bool() {
        let title = window_text(hwnd);
        if !title.is_empty() && title.to_lowercase().contains(&context.keyword) {
            context.found.push((hwnd, title));
        }
    }
    BOOL(1)
}

fn find_windows(keyword: &str) -> Vec<(HWND, String)> {
    let mut context = FindContext {
        keyword: keyword.to_lowercase(),
        found: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_proc),
            LPARAM(&mut context as *mut FindContext as isize),
        );
    }
    context.found
}

fn window_text(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    let len = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}
