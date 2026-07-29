//! 把選定的命令送回叫出候選框之前的那個視窗。
//!
//! 流程：顯示候選框前先記住前景視窗 → 使用者選定 → 收起候選框 →
//! 還原焦點 → 以 `SendInput` 逐字送出。文字只填入命令列，不送出 Enter。

use std::ffi::c_void;
use std::mem::size_of;
use std::sync::Mutex;
use std::{thread, time::Duration};

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsIconic, SetForegroundWindow, ShowWindow,
    SW_RESTORE,
};

/// 候選框叫出前的前景視窗。`HWND` 不是 `Send`，因此存原始指標數值。
static TARGET_WINDOW: Mutex<Option<isize>> = Mutex::new(None);

/// 在顯示候選框之前呼叫，記住要把文字送回哪個視窗。
///
/// 前景視窗若是 QQKey 自己（例如連按兩次快捷鍵）就保留原本的記錄，
/// 否則第二次按下會把目標覆寫成候選框本身。
pub fn remember_foreground() {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() || belongs_to_self(hwnd) {
        return;
    }
    *TARGET_WINDOW.lock().unwrap() = Some(hwnd.0 as isize);
}

/// 還原焦點並把文字送出去。
pub fn inject_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }

    let target = TARGET_WINDOW
        .lock()
        .unwrap()
        .ok_or("沒有記錄到要送回的視窗")?;
    let hwnd = HWND(target as *mut c_void);

    if !restore_focus(hwnd) {
        return Err("無法把焦點還原到原視窗".into());
    }

    // 焦點切換不是同步完成的，太快送出會落到還沒失去焦點的候選框上
    thread::sleep(Duration::from_millis(40));

    let expected = text.encode_utf16().count() as u32 * 2;
    let sent = send_text(text);
    if sent != expected {
        return Err(format!("鍵盤事件只送出 {sent}/{expected} 個，可能被攔截"));
    }
    Ok(())
}

fn belongs_to_self(hwnd: HWND) -> bool {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid == std::process::id()
}

/// `SetForegroundWindow` 會受前景鎖定限制，失敗時把自己的輸入佇列
/// 接到目標執行緒上再試一次。
fn restore_focus(target: HWND) -> bool {
    unsafe {
        if IsIconic(target).as_bool() {
            let _ = ShowWindow(target, SW_RESTORE);
        }
        if SetForegroundWindow(target).as_bool() {
            return true;
        }

        let target_thread = GetWindowThreadProcessId(target, None);
        if target_thread == 0 {
            return false;
        }
        let current_thread = GetCurrentThreadId();
        let _ = AttachThreadInput(current_thread, target_thread, true);
        let restored = SetForegroundWindow(target).as_bool();
        let _ = AttachThreadInput(current_thread, target_thread, false);
        restored
    }
}

/// 以 `KEYEVENTF_UNICODE` 逐個 UTF-16 code unit 送出，不受使用者的鍵盤配置影響。
fn send_text(text: &str) -> u32 {
    let inputs: Vec<INPUT> = text
        .encode_utf16()
        .flat_map(|unit| {
            let event = |flags: KEYBD_EVENT_FLAGS| INPUT {
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
                event(KEYEVENTF_UNICODE),
                event(KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
            ]
        })
        .collect();

    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) }
}
