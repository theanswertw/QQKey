//! 取得輸入游標（caret）的螢幕座標，決定候選框要貼在哪裡。
//!
//! 三層 fallback，實測結論詳見 `spike/caret-probe/README.md`：
//!
//! 1. `GetGUIThreadInfo` —— conhost 與 Win32 原生輸入控制項
//! 2. UI Automation `TextPattern` —— Windows Terminal 這類自繪文字的應用
//! 3. 前景視窗左下角 —— 前兩層都取不到時的保底

use std::ffi::c_void;
use std::mem::size_of;

use windows::Win32::Foundation::{HWND, POINT, RECT};
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
    IUIAutomationTextPattern2, IUIAutomationTextRange, TextUnit_Character, TreeScope_Descendants,
    UIA_IsTextPatternAvailablePropertyId, UIA_TextPattern2Id, UIA_TextPatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetGUIThreadInfo, GetWindowRect, GetWindowThreadProcessId, IsIconic, GUITHREADINFO,
};

/// 候選框要對齊的位置。`top`／`bottom` 分開保留，是因為畫面下緣放不下時
/// 要改貼到 caret 上方。
#[derive(Debug, Clone, Copy)]
pub struct Anchor {
    pub x: i32,
    pub top: i32,
    pub bottom: i32,
}

/// 候選框可用的螢幕範圍。刻意不用 Win32 的 `RECT`，讓呼叫端不必依賴 windows crate。
#[derive(Debug, Clone, Copy)]
pub struct Area {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// caret 與候選框之間的間距。
const GAP: i32 = 6;

/// 算出候選框左上角該放的位置：預設貼在 caret 下方，
/// 下方放不下就翻到上方，再夾制在螢幕範圍內。
pub fn place(anchor: Anchor, size: (i32, i32), area: Area) -> (i32, i32) {
    let (width, height) = size;

    let mut x = anchor.x;
    if x + width > area.right {
        x = area.right - width;
    }
    x = x.max(area.left);

    let mut y = anchor.bottom + GAP;
    if y + height > area.bottom {
        let above = anchor.top - GAP - height;
        // 上方也擠不下（視窗比螢幕還高）就貼齊底部
        y = if above >= area.top {
            above
        } else {
            area.bottom - height
        };
    }
    y = y.max(area.top);

    (x, y)
}

/// 依序嘗試三層 fallback。`None` 代表連視窗矩形都取不到，呼叫端應維持原本的位置。
pub fn locate(target: HWND) -> Option<Anchor> {
    if target.0.is_null() || unsafe { IsIconic(target) }.as_bool() {
        return None;
    }

    if let Some(rect) = system_caret(target) {
        return Some(Anchor::from(rect));
    }
    if let Some(rect) = ui_automation_caret(target) {
        return Some(Anchor::from(rect));
    }
    window_fallback(target)
}

impl From<RECT> for Anchor {
    fn from(rect: RECT) -> Self {
        Anchor {
            x: rect.left,
            top: rect.top,
            bottom: rect.bottom,
        }
    }
}

// ---------------------------------------------------------------- Layer 1

fn system_caret(target: HWND) -> Option<RECT> {
    unsafe {
        let thread = GetWindowThreadProcessId(target, None);
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        GetGUIThreadInfo(thread, &mut info).ok()?;
        if info.hwndCaret.0.is_null() {
            return None;
        }

        let mut top_left = POINT {
            x: info.rcCaret.left,
            y: info.rcCaret.top,
        };
        let mut bottom_right = POINT {
            x: info.rcCaret.right,
            y: info.rcCaret.bottom,
        };
        if !ClientToScreen(info.hwndCaret, &mut top_left).as_bool()
            || !ClientToScreen(info.hwndCaret, &mut bottom_right).as_bool()
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

fn ui_automation_caret(target: HWND) -> Option<RECT> {
    unsafe {
        // WebView2 已初始化過 COM，重複呼叫會回非成功的 HRESULT，忽略即可
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;

        // 候選框尚未顯示，焦點還在目標視窗上，這條路徑最直接
        if let Ok(focused) = automation.GetFocusedElement() {
            if let Some(rect) = caret_from_element(&focused) {
                return Some(rect);
            }
        }

        let root = automation.ElementFromHandle(target).ok()?;
        if let Some(rect) = caret_from_element(&root) {
            return Some(rect);
        }

        // Windows Terminal 的終端機文字區是 TermControl，位在視窗根元素底下。
        // 不能取第一個命中的元素——那是分頁標題的 TextBlock，會回報標題列座標。
        let condition = automation
            .CreatePropertyCondition(UIA_IsTextPatternAvailablePropertyId, &VARIANT::from(true))
            .ok()?;
        let candidates = root.FindAll(TreeScope_Descendants, &condition).ok()?;
        for index in 0..candidates.Length().unwrap_or(0) {
            let Ok(element) = candidates.GetElement(index) else {
                continue;
            };
            if let Some(rect) = caret_from_element(&element) {
                return Some(rect);
            }
        }

        None
    }
}

fn caret_from_element(element: &IUIAutomationElement) -> Option<RECT> {
    unsafe {
        if let Ok(pattern) =
            element.GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id)
        {
            let mut is_active = Default::default();
            if let Ok(range) = pattern.GetCaretRange(&mut is_active) {
                if let Some(rect) = rect_of_range(&range) {
                    return Some(rect);
                }
            }
        }

        // Windows Terminal 走的是這條：未實作 TextPattern2，但沒有選取內容時
        // GetSelection 會回傳 caret 位置的退化範圍
        if let Ok(pattern) =
            element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        {
            if let Ok(selection) = pattern.GetSelection() {
                if selection.Length().unwrap_or(0) > 0 {
                    if let Ok(range) = selection.GetElement(0) {
                        if let Some(rect) = rect_of_range(&range) {
                            return Some(rect);
                        }
                    }
                }
            }
        }

        None
    }
}

/// caret 對應的範圍寬度為零，直接量邊界會拿不到矩形，
/// 因此先複製一份展開成一個字元再量測。
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

// ---------------------------------------------------------------- Layer 3

/// 取不到 caret 時貼在視窗左下角，位置大致落在多數終端機的提示字元附近。
fn window_fallback(target: HWND) -> Option<Anchor> {
    const INSET_X: i32 = 24;
    const INSET_Y: i32 = 48;

    let mut rect = RECT::default();
    unsafe { GetWindowRect(target, &mut rect) }.ok()?;

    Some(Anchor {
        x: rect.left + INSET_X,
        top: rect.bottom - INSET_Y,
        bottom: rect.bottom - INSET_Y,
    })
}

#[cfg(test)]
mod tests {
    use super::{place, Anchor, Area};

    /// 1920×1080 的主螢幕
    const SCREEN: Area = Area {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    const SIZE: (i32, i32) = (640, 420);

    fn anchor_at(x: i32, top: i32, bottom: i32) -> Anchor {
        Anchor { x, top, bottom }
    }

    #[test]
    fn sits_below_the_caret_by_default() {
        let (x, y) = place(anchor_at(300, 400, 419), SIZE, SCREEN);
        assert_eq!(x, 300);
        assert_eq!(y, 425);
    }

    #[test]
    fn flips_above_when_it_would_overflow_the_bottom() {
        // caret 在 y=900，下方只剩 180px 放不下 420px 高的候選框
        let (_, y) = place(anchor_at(300, 900, 919), SIZE, SCREEN);
        assert_eq!(y, 900 - 6 - 420);
    }

    #[test]
    fn sticks_to_the_bottom_when_neither_side_fits() {
        let tall = (640, 1200);
        let (_, y) = place(anchor_at(300, 900, 919), tall, SCREEN);
        assert_eq!(y, 0, "上下都放不下時夾制在螢幕內");
    }

    #[test]
    fn shifts_left_when_it_would_overflow_the_right_edge() {
        let (x, _) = place(anchor_at(1700, 400, 419), SIZE, SCREEN);
        assert_eq!(x, 1920 - 640);
    }

    #[test]
    fn never_starts_left_of_the_screen() {
        let narrow = Area {
            left: 0,
            top: 0,
            right: 400,
            bottom: 1080,
        };
        let (x, _) = place(anchor_at(100, 400, 419), SIZE, narrow);
        assert_eq!(x, 0);
    }

    #[test]
    fn respects_a_secondary_monitor_offset() {
        let right_monitor = Area {
            left: 1920,
            top: 0,
            right: 3840,
            bottom: 1080,
        };
        let (x, y) = place(anchor_at(2000, 400, 419), SIZE, right_monitor);
        assert_eq!((x, y), (2000, 425));
    }
}
