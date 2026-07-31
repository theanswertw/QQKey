//! 系統匣圖示與選單。
//!
//! QQKey 平常沒有可見視窗，系統匣是它唯一的常駐入口——
//! 也是使用者忘記快捷鍵時找得回來的地方。

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Runtime};

use crate::hotkey::SETTINGS_SHORTCUT;
use crate::i18n::{self, Lang};

/// 選單內容抽出來讓建立與重建共用。
///
/// 兩份定義遲早會飄開，而症狀是「切了語言，三項裡有一項沒變」。
fn build_menu<R: Runtime>(
    app: &AppHandle<R>,
    shortcut: &str,
    lang: Lang,
) -> tauri::Result<Menu<R>> {
    let show = MenuItem::with_id(app, "show", show_label(lang, shortcut), true, None::<&str>)?;
    let settings = MenuItem::with_id(
        app,
        "settings",
        i18n::tray_settings(lang, &pretty(SETTINGS_SHORTCUT)),
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", i18n::tray_quit(lang), true, None::<&str>)?;
    Menu::with_items(app, &[&show, &settings, &separator, &quit])
}

/// 語系當參數傳而不是在裡面讀 `i18n::current()`：`lib.rs` 在 `app.manage(state)`
/// **之前**就呼叫這裡，跟 `shortcut` 一樣是由啟動流程決定好才交進來的。
pub fn setup<R: Runtime>(app: &AppHandle<R>, shortcut: &str, lang: Lang) -> tauri::Result<()> {
    let menu = build_menu(app, shortcut, lang)?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::UnknownPath)?;

    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip(tooltip(lang, shortcut))
        .menu(&menu)
        // 左鍵留給「叫出候選框」，選單走右鍵
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => crate::hotkey::toggle_launcher(app),
            "settings" => {
                // 這個 callback 跑在**事件迴圈執行緒**上：Tauri 把選單事件透過
                // event loop proxy 送回來，再於 run callback 裡呼叫這些 listener。
                // 在這裡直接建立設定視窗會鎖死整個程式，理由見
                // `commands::open_settings`。丟到另一條執行緒，事件迴圈才空著
                // 能回覆建立視窗的請求。
                let app = app.clone();
                std::thread::spawn(move || {
                    if let Err(error) = crate::commands::show_settings_window(&app) {
                        crate::trace("設定", &format!("開啟失敗：{error}"));
                    }
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::hotkey::toggle_launcher(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

/// 這兩處收到的是**實際註冊成功**的快捷鍵，空字串代表一個都沒註冊上。
/// 那種時候不能照寫一個按了沒反應的組合——系統匣是使用者唯一找得回
/// 設定畫面的地方，得讓他知道要去改綁。
fn show_label(lang: Lang, shortcut: &str) -> String {
    if shortcut.is_empty() {
        i18n::tray_show_inactive(lang)
    } else {
        i18n::tray_show(lang, &pretty(shortcut))
    }
}

fn tooltip(lang: Lang, shortcut: &str) -> String {
    if shortcut.is_empty() {
        i18n::tray_tooltip_inactive(lang)
    } else {
        i18n::tray_tooltip(lang, &pretty(shortcut))
    }
}

/// 把 `Alt+KeyQ` 這種內部表示法改寫成給人看的 `Alt+Q`。
fn pretty(shortcut: &str) -> String {
    shortcut
        .split('+')
        .map(|part| part.strip_prefix("Key").unwrap_or(part))
        .collect::<Vec<_>>()
        .join("+")
}

/// 讓系統匣跟目前的（快捷鍵, 語系）一致。改綁與換語言都走這裡。
///
/// 取代原本只更新 tooltip 的 `refresh_tooltip()`：那時選單項仍寫著舊組合，
/// 改綁成 Alt+W 之後右鍵選單照樣顯示「叫出候選框（Alt+Q）」。兩件事都會動到
/// 選單文字與 tooltip，分成兩支遲早有一支漏掉。
///
/// 用 `set_menu` 整體替換而不是留住三個 `MenuItem` handle 再 `set_text`：
/// `TrayIcon` 沒有 `menu()` getter 拿不回既有選單，而把 handle 存進 `AppState`
/// 會讓它被迫帶上 runtime 參數——它現在可以只靠一個 `Store` 做單元測試。
/// 整體重建沒有狀態要存。
///
/// `on_menu_event` 的 handler 註冊在 manager 的全域選單監聽器上，對任何選單
/// 事件都會觸發、不查選單實例，所以換掉 `Menu` 之後只要 MenuId 沒變
/// （show／settings／quit）事件照樣進得來。
pub fn refresh<R: Runtime>(app: &AppHandle<R>, shortcut: &str, lang: Lang) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    match build_menu(app, shortcut, lang) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        // 重建失敗時舊選單還在，比整個選單消失好；但要留下痕跡
        Err(error) => crate::trace("系統匣", &format!("選單重建失敗：{error}")),
    }
    let _ = tray.set_tooltip(Some(tooltip(lang, shortcut)));
}

#[cfg(test)]
mod tests {
    use super::{pretty, show_label, tooltip};
    use crate::i18n::Lang;

    #[test]
    fn strips_the_key_prefix_for_display() {
        assert_eq!(pretty("Alt+KeyQ"), "Alt+Q");
        assert_eq!(pretty("Alt+Shift+KeyQ"), "Alt+Shift+Q");
        assert_eq!(pretty("Control+Space"), "Control+Space");
    }

    /// 語系顯式傳入而不是靠 `i18n::pin_for_tests()`：這兩支是純函式，
    /// 不必為了測它們去碰 process 級的全域。
    #[test]
    fn says_so_when_no_shortcut_is_actually_registered() {
        assert!(
            show_label(Lang::ZhHant, "").contains("未生效"),
            "一個都沒註冊成功時，選單不該寫著按了沒反應的組合"
        );
        assert!(tooltip(Lang::ZhHant, "").contains("未生效"));
        assert_eq!(show_label(Lang::ZhHant, "Alt+KeyQ"), "叫出候選框（Alt+Q）");
    }

    /// 每個語系都要講出「沒生效」這件事，不能有哪一個語系靜靜地退化成
    /// 顯示一個按了沒反應的組合。
    #[test]
    fn every_language_distinguishes_the_inactive_state() {
        for lang in Lang::ALL {
            let inactive = show_label(lang, "");
            let active = show_label(lang, "Alt+KeyQ");
            assert_ne!(inactive, active, "{} 的兩種狀態講的是同一句話", lang.as_tag());
            assert!(
                active.contains("Alt+Q"),
                "{} 少了快捷鍵本身",
                lang.as_tag()
            );
        }
    }
}
