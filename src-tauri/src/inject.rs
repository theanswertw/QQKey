//! 把選定的命令送回叫出候選框之前的那個視窗。
//!
//! 流程：顯示候選框前先記住前景視窗 → 使用者選定 → 收起候選框 →
//! 還原焦點 → 以 `SendInput` 逐字送出。文字只填入命令列，不送出 Enter。

use std::ffi::c_void;
use std::mem::size_of;
use std::sync::Mutex;
use std::{
    thread,
    time::{Duration, Instant},
};

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsIconic, SetForegroundWindow, ShowWindow,
    EVENT_SYSTEM_FOREGROUND, SW_RESTORE, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};

/// 候選框叫出前的前景視窗。`HWND` 不是 `Send`，因此存原始指標數值。
static TARGET_WINDOW: Mutex<Option<isize>> = Mutex::new(None);

/// 等前景切換過去時的輪詢間隔。再密只是燒 CPU，前景不會切得比這更快。
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// 等前景切換的上限。**刻意給得寬**——常見路徑幾毫秒就等到了，
/// 這個上限只在已經出問題時才付得到，給窄反而會誤殺慢機器。
const FOREGROUND_TIMEOUT: Duration = Duration::from_millis(400);

/// 前景確認之後仍保留的緩衝。頂層視窗成了前景，不代表它內部的焦點子控制項
/// 與輸入處理都就緒了；這段刻意不降到零。
const FOCUS_SETTLE: Duration = Duration::from_millis(10);

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
    // 只記 HWND 不記標題：診斷定位問題需要的是「有沒有記錄到、是不是同一個
    // 視窗」，而這行每按一次快捷鍵就跑一次，寫標題等於把使用者開過什麼
    // 留在磁碟上——與下面 hook callback 的顧慮相同。
    crate::trace("焦點", &format!("記錄目標視窗 {:?}", hwnd.0));
    *TARGET_WINDOW.lock().unwrap() = Some(hwnd.0 as isize);
}

fn belongs_to_self(hwnd: HWND) -> bool {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid == std::process::id()
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

    let lang = crate::i18n::current();

    let target = TARGET_WINDOW
        .lock()
        .unwrap()
        .ok_or_else(|| crate::i18n::no_target_window(lang))?;
    let hwnd = HWND(target as *mut c_void);

    if !restore_focus(hwnd) {
        return Err(crate::i18n::restore_focus_failed(lang));
    }

    // `SetForegroundWindow` 回 true 只代表**請求被接受**，前景真正切換是非同步的。
    // 從前這裡固定睡 40ms，兩頭都不對：常見情況下白等三十幾毫秒（每次注入都要付），
    // 而目標是剛從最小化還原的視窗時又不見得夠久——屆時文字會落到錯誤的地方，
    // 而程式完全不知道自己送錯了。改為確認前景真的換過去才送。
    if !wait_until(FOREGROUND_TIMEOUT, POLL_INTERVAL, || foreground_is(hwnd)) {
        return Err(crate::i18n::foreground_not_settled(
            lang,
            FOREGROUND_TIMEOUT.as_millis() as u32,
        ));
    }

    // 頂層視窗成了前景，不代表它內部的焦點與輸入處理都就緒了
    thread::sleep(FOCUS_SETTLE);

    let expected = text.encode_utf16().count() as u32 * 2;
    let sent = send_text(text);
    if sent != expected {
        return Err(crate::i18n::input_partially_sent(lang, sent, expected));
    }
    Ok(())
}


/// 目標視窗是否已經成為前景視窗。
///
/// 比對原始指標而非 `HWND` 本身，與這個檔案其他處理 `HWND.0` 的地方一致。
fn foreground_is(target: HWND) -> bool {
    unsafe { GetForegroundWindow() }.0 == target.0
}

/// 每 `interval` 檢查一次 `ready`，直到成立（回 `true`）或超過 `timeout`（回 `false`）。
///
/// **先檢查再睡**：前景通常已經切好了，先睡一輪等於把省下來的延遲又還回去。
/// 抽成不碰 Win32 的純函式才測得到時序——這個檔案其餘部分都測不了。
fn wait_until(timeout: Duration, interval: Duration, mut ready: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    loop {
        if ready() {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        thread::sleep(interval);
    }
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

#[cfg(test)]
mod tests {
    use super::wait_until;
    use std::cell::Cell;
    use std::time::{Duration, Instant};

    const TIMEOUT: Duration = Duration::from_millis(60);
    const INTERVAL: Duration = Duration::from_millis(5);

    /// 條件在第 `n` 次呼叫時成立的 predicate，附帶被呼叫過幾次。
    fn ready_on(n: u32) -> (impl Fn() -> bool, impl Fn() -> u32) {
        let calls = std::rc::Rc::new(Cell::new(0u32));
        let counter = std::rc::Rc::clone(&calls);
        (
            move || {
                calls.set(calls.get() + 1);
                calls.get() >= n
            },
            move || counter.get(),
        )
    }

    #[test]
    fn checks_once_before_sleeping() {
        // 這是整個改動的重點：前景早就切好時不該白等。舊實作在這裡固定睡 40ms。
        let (ready, _) = ready_on(1);
        let start = Instant::now();
        assert!(wait_until(TIMEOUT, INTERVAL, ready), "條件一開始就成立應回 true");
        assert!(
            start.elapsed() < INTERVAL,
            "條件已成立時不該睡滿一個輪詢間隔，實際等了 {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn stops_polling_as_soon_as_the_window_is_ready() {
        let (ready, calls) = ready_on(3);
        assert!(wait_until(TIMEOUT, INTERVAL, ready), "條件在期限內成立應回 true");
        assert_eq!(calls(), 3, "條件成立後不該繼續輪詢");
    }

    #[test]
    fn gives_up_after_the_timeout_instead_of_blocking_forever() {
        let start = Instant::now();
        assert!(
            !wait_until(TIMEOUT, INTERVAL, || false),
            "條件永遠不成立應回 false"
        );
        let elapsed = start.elapsed();
        assert!(elapsed >= TIMEOUT, "不該早於期限就放棄，實際 {elapsed:?}");
        // 上界放寬鬆：sleep 只保證「至少」睡多久，開發機的排程抖動會讓每一輪超時，
        // 抓太緊會變成間歇失敗的測試。這裡要擋的是無限迴圈，不是精準計時。
        assert!(elapsed < TIMEOUT * 5, "放棄得太晚，實際 {elapsed:?}");
    }
}
