//! M0 spike：驗證能否在各種宿主視窗中取得輸入游標（caret）的螢幕座標。
//!
//! 用法：
//!   `cargo run -- [倒數秒數]`        倒數後偵測「前景視窗」，預設 5 秒
//!   `cargo run -- --find <關鍵字>`   直接偵測標題含關鍵字的視窗，不需切換焦點
//!
//! 前者模擬實際按下快捷鍵時的情境；後者用於在不打斷操作的前提下逐一驗證各宿主。

use std::ffi::c_void;
use std::mem::size_of;
use std::{thread, time::Duration};

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
    SafeArrayUnaccessData,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
    IUIAutomationTextPattern2, IUIAutomationTextRange, TextUnit_Character,
    TreeScope_Descendants, UIA_IsTextPatternAvailablePropertyId, UIA_TextPattern2Id,
    UIA_TextPatternId,
};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetCursorPos, GetForegroundWindow, GetGUIThreadInfo, GetWindowRect,
    GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, GUITHREADINFO,
};

/// caret 座標與它是由哪一層 fallback 取得的。
struct Located {
    rect: RECT,
    layer: &'static str,
}

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--find") => {
            let Some(keyword) = args.get(1).filter(|k| !k.is_empty()) else {
                eprintln!("--find 需要一個標題關鍵字，例如：--find PowerShell");
                return;
            };
            let matches = find_windows(keyword);
            if matches.is_empty() {
                println!("找不到標題含 {keyword:?} 的可見視窗。");
                return;
            }
            for (hwnd, _) in matches {
                report(hwnd, true);
            }
        }
        _ => {
            let secs: u64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(5);
            println!("caret-probe：請在倒數結束前切換到要測試的視窗。");
            for i in (1..=secs).rev() {
                println!("  {i} …");
                thread::sleep(Duration::from_secs(1));
            }

            let hwnd = unsafe { GetForegroundWindow() };
            if hwnd.0.is_null() {
                println!("取不到前景視窗，中止。");
                return;
            }
            report(hwnd, false);
        }
    }
}

/// 對單一視窗跑完三層 fallback 並印出報告。
///
/// `targeted` 為 true 時走 `ElementFromHandle`（指定視窗，不需焦點），
/// 為 false 時走 `GetFocusedElement`（模擬實際按下快捷鍵的情境）。
fn report(hwnd: HWND, targeted: bool) {
    println!("\n=== 視窗 ===");
    println!("  HWND    : {:?}", hwnd.0);
    println!("  標題    : {}", window_text(hwnd));
    println!("  類別名稱: {}", class_name(hwnd));

    println!("\n=== Layer 1：GetGUIThreadInfo ===");
    let layer1 = layer1_gui_thread_info(hwnd);
    match &layer1 {
        Some(r) => println!("  命中 → {}", fmt_rect(r)),
        None => println!("  未命中（此宿主未使用系統 caret）"),
    }

    println!("\n=== Layer 2：UI Automation ===");
    let layer2 = if targeted {
        layer2_uia_for_window(hwnd)
    } else {
        layer2_uia_focused()
    };
    match &layer2 {
        Some(l) => println!("  命中（{}） → {}", l.layer, fmt_rect(&l.rect)),
        None => println!("  未命中"),
    }

    println!("\n=== Layer 3：視窗矩形 / 滑鼠 ===");
    if let Some(r) = layer3_window_rect(hwnd) {
        println!("  視窗矩形 → {}", fmt_rect(&r));
        println!(
            "  降級錨點（左下角內縮）→ x={}, y={}",
            r.left + 24,
            r.bottom - 48
        );
    }
    let mut p = POINT::default();
    if unsafe { GetCursorPos(&mut p) }.is_ok() {
        println!("  滑鼠座標 → x={}, y={}", p.x, p.y);
    }

    println!("\n=== 結論 ===");
    let best = layer1
        .map(|rect| Located {
            rect,
            layer: "GetGUIThreadInfo",
        })
        .or(layer2);
    match best {
        Some(l) => println!(
            "  本宿主可用 {}，錨點 x={}, y={}（候選框應貼在此點下方）",
            l.layer, l.rect.left, l.rect.bottom
        ),
        None => println!("  本宿主無法取得 caret，需降級為視窗左下角定位。"),
    }
}

// ---------------------------------------------------------------- Layer 1

/// 系統 caret：conhost、Win32 原生輸入控制項適用。
fn layer1_gui_thread_info(hwnd: HWND) -> Option<RECT> {
    unsafe {
        let tid = GetWindowThreadProcessId(hwnd, None);
        let mut gui = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        GetGUIThreadInfo(tid, &mut gui).ok()?;
        if gui.hwndCaret.0.is_null() {
            return None;
        }

        let mut top_left = POINT {
            x: gui.rcCaret.left,
            y: gui.rcCaret.top,
        };
        let mut bottom_right = POINT {
            x: gui.rcCaret.right,
            y: gui.rcCaret.bottom,
        };
        if !ClientToScreen(gui.hwndCaret, &mut top_left).as_bool()
            || !ClientToScreen(gui.hwndCaret, &mut bottom_right).as_bool()
        {
            return None;
        }

        Some(RECT {
            left: top_left.x,
            top: top_left.y,
            right: bottom_right.x,
            bottom: bottom_right.y,
        })
    }
}

// ---------------------------------------------------------------- Layer 2

/// UI Automation：Windows Terminal 這類自繪文字的 XAML 應用適用。
/// 走全域焦點元素，等同實際按下快捷鍵當下的情境。
fn layer2_uia_focused() -> Option<Located> {
    unsafe {
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let element = automation.GetFocusedElement().ok()?;
        describe_element(&element);
        caret_from_element(&element)
    }
}

/// 指定視窗的 UI Automation 探測。焦點元素若不支援 TextPattern，
/// 再往下找第一個實作 TextPattern 的後代（Windows Terminal 的 TermControl 位於此層）。
fn layer2_uia_for_window(hwnd: HWND) -> Option<Located> {
    unsafe {
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let root = automation.ElementFromHandle(hwnd).ok()?;
        describe_element(&root);
        if let Some(located) = caret_from_element(&root) {
            return Some(located);
        }

        let condition = automation
            .CreatePropertyCondition(UIA_IsTextPatternAvailablePropertyId, &VARIANT::from(true))
            .ok()?;
        let candidates = root.FindAll(TreeScope_Descendants, &condition).ok()?;
        let count = candidates.Length().unwrap_or(0);
        println!("  ↓ 後代中有 {count} 個元素支援 TextPattern，逐一嘗試");
        for index in 0..count {
            let Ok(element) = candidates.GetElement(index) else {
                continue;
            };
            describe_element(&element);
            if let Some(located) = caret_from_element(&element) {
                return Some(located);
            }
        }

        None
    }
}

/// 從單一 UIA 元素取出 caret 矩形，優先用 TextPattern2 的專用 API。
fn caret_from_element(element: &IUIAutomationElement) -> Option<Located> {
    unsafe {
        if let Ok(pattern) =
            element.GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id)
        {
            let mut is_active = Default::default();
            if let Ok(range) = pattern.GetCaretRange(&mut is_active) {
                if let Some(rect) = rect_of_range(&range) {
                    return Some(Located {
                        rect,
                        layer: "TextPattern2::GetCaretRange",
                    });
                }
            }
        }

        if let Ok(pattern) =
            element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        {
            if let Ok(selection) = pattern.GetSelection() {
                if selection.Length().unwrap_or(0) > 0 {
                    if let Ok(range) = selection.GetElement(0) {
                        if let Some(rect) = rect_of_range(&range) {
                            return Some(Located {
                                rect,
                                layer: "TextPattern::GetSelection",
                            });
                        }
                    }
                }
            }
        }

        None
    }
}

/// caret 對應的範圍寬度為零，直接取邊界會拿不到矩形，
/// 因此先複製一份展開成一個字元再量測，失敗才退回原範圍。
fn rect_of_range(range: &IUIAutomationTextRange) -> Option<RECT> {
    unsafe {
        if let Ok(expanded) = range.Clone() {
            if expanded.ExpandToEnclosingUnit(TextUnit_Character).is_ok() {
                if let Some(rect) = bounding_rect(&expanded) {
                    return Some(rect);
                }
            }
        }
    }
    bounding_rect(range)
}

/// `GetBoundingRectangles` 回傳的 SAFEARRAY 每四個 f64 為一組 (left, top, width, height)。
fn bounding_rect(range: &IUIAutomationTextRange) -> Option<RECT> {
    unsafe {
        let array = range.GetBoundingRectangles().ok()?;
        if array.is_null() {
            return None;
        }

        let count = match (SafeArrayGetLBound(array, 1), SafeArrayGetUBound(array, 1)) {
            (Ok(lower), Ok(upper)) => upper - lower + 1,
            _ => 0,
        };
        if count < 4 {
            let _ = SafeArrayDestroy(array);
            return None;
        }

        let mut data: *mut c_void = std::ptr::null_mut();
        if SafeArrayAccessData(array, &mut data).is_err() {
            let _ = SafeArrayDestroy(array);
            return None;
        }
        let values = std::slice::from_raw_parts(data as *const f64, 4);
        let rect = RECT {
            left: values[0] as i32,
            top: values[1] as i32,
            right: (values[0] + values[2]) as i32,
            bottom: (values[1] + values[3]) as i32,
        };
        let _ = SafeArrayUnaccessData(array);
        let _ = SafeArrayDestroy(array);

        Some(rect)
    }
}

/// 印出取得焦點的 UIA 元素資訊，方便判讀為何某個宿主取不到 caret。
fn describe_element(element: &IUIAutomationElement) {
    unsafe {
        let name = element
            .CurrentName()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let class = element
            .CurrentClassName()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let control_type = element.CurrentControlType().map(|t| t.0).unwrap_or(0);
        println!("  焦點元素: name={name:?} class={class:?} controlType={control_type}");
    }
}

// ---------------------------------------------------------------- Layer 3

fn layer3_window_rect(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
    Some(rect)
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

/// 列出標題含關鍵字的可見頂層視窗。
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

// ---------------------------------------------------------------- 工具

fn window_text(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    let len = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

fn fmt_rect(r: &RECT) -> String {
    format!(
        "left={} top={} right={} bottom={} (寬 {} 高 {})",
        r.left,
        r.top,
        r.right,
        r.bottom,
        r.right - r.left,
        r.bottom - r.top
    )
}
