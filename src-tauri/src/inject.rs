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
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, IsIconic, SetForegroundWindow,
    ShowWindow, EVENT_SYSTEM_FOREGROUND, SW_RESTORE, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS,
};

/// 候選框叫出前的前景視窗。`HWND` 不是 `Send`，因此存原始指標數值。
static TARGET_WINDOW: Mutex<Option<isize>> = Mutex::new(None);

/// 在顯示候選框之前呼叫，記住要把文字送回哪個視窗。
///
/// 前景視窗若是 QQKey 自己（例如連按兩次快捷鍵）就保留原本的記錄，
/// 否則第二次按下會把目標覆寫成候選框本身。
/// 開始追蹤前景視窗變化。必須在有訊息迴圈的執行緒上呼叫。
///
/// 只在按下快捷鍵當下問 `GetForegroundWindow` 是不夠的——QQKey 自己的視窗
/// 即使設為隱藏，在程序剛啟動時仍可能占著前景，屆時就問不到正確答案。
/// 改為持續記錄，`WINEVENT_SKIPOWNPROCESS` 會自動濾掉自家視窗的事件。
pub fn watch_foreground() {
    let hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_foreground_changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    if hook.0.is_null() {
        crate::trace("焦點", "前景追蹤註冊失敗，將退回按下快捷鍵當下的前景視窗");
    }
}

unsafe extern "system" fn on_foreground_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _object_id: i32,
    _child_id: i32,
    _thread: u32,
    _time: u32,
) {
    if hwnd.0.is_null() {
        return;
    }
    // 這裡刻意不做診斷輸出：每次切換視窗都會觸發，記錄標題等於把使用者
    // 一整天開過什麼全寫進 log。真正要用到目標視窗時再記錄就夠了。
    *TARGET_WINDOW.lock().unwrap() = Some(hwnd.0 as isize);
}

/// 顯示候選框前再確認一次。前景若是自家視窗就沿用 hook 記錄的值。
pub fn remember_foreground() {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() || belongs_to_self(hwnd) {
        crate::trace("焦點", "前景不可用，沿用追蹤到的視窗");
        return;
    }
    crate::trace(
        "焦點",
        &format!("記錄目標視窗 {:?} {:?}", hwnd.0, window_title(hwnd)),
    );
    *TARGET_WINDOW.lock().unwrap() = Some(hwnd.0 as isize);
}

fn belongs_to_self(hwnd: HWND) -> bool {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid == std::process::id()
}

fn window_title(hwnd: HWND) -> String {
    let mut buffer = [0u16; 128];
    let len = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

/// 目前記錄的目標視窗，供 caret 定位使用。
pub fn target_window() -> Option<HWND> {
    TARGET_WINDOW
        .lock()
        .unwrap()
        .map(|raw| HWND(raw as *mut c_void))
}

/// 收起候選框後把焦點還給原本的視窗。
///
/// 除了讓使用者能直接接著打字，也是為了不讓前景落到桌面——
/// 那會讓下一次叫出候選框時把桌面誤認成注入目標。
pub fn restore_target_focus() {
    if let Some(target) = target_window() {
        restore_focus(target);
    }
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
