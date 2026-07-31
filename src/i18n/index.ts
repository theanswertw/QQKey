import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";

import { FALLBACK_LANGUAGE, LANGUAGES, isLanguage, type Language } from "./languages";
import { resources } from "./resources";

/** 與後端 `i18n::EVENT_LANGUAGE` 一致。 */
const EVENT_LANGUAGE = "app:language";

/**
 * 取得要用的語系。
 *
 * **後端才是權威**——它同時決定系統匣文字與視窗標題用哪一個語言，兩邊必須是
 * 同一個答案。前端自己判斷的話，後端由 Win32 判成 ja、前端由 navigator.language
 * 判成 en，第一次啟動就是兩種語言，而且沒有任何錯誤。
 *
 * IPC 失敗才退到 navigator.language：那是 webview 的語言，多半跟系統顯示語言
 * 一致，但它讀不到使用者在設定畫面選過的覆寫值，所以只能當備援。
 */
async function wantedLanguage(): Promise<Language> {
  try {
    return await invoke<Language>("active_language");
  } catch (error) {
    console.error("取不到語系設定，改用 webview 語言", error);
    return matchTag(navigator.language);
  }
}

/**
 * 與後端 `i18n::match_tag()` 同一組規則。只有上面那條備援路徑用得到。
 *
 * 實測：Windows 回報的是舊式的 `zh-TW` 而不是 `zh-Hant-TW`，所以「只看主要
 * 語言子標籤」那一段不是備用路徑而是主路徑。`zh` 是唯一要再看 script 與
 * region 的語言（簡繁共用主要子標籤），判別採白名單：明確指向簡體的才給
 * `zh-Hans`，其餘的 `zh` 一律 `zh-Hant`。兩邊的規則要一起改。
 */
function matchTag(raw: string): Language {
  const tag = raw.replace(/_/g, "-");
  const exact = LANGUAGES.find((lang) => lang.toLowerCase() === tag.toLowerCase());
  if (exact) {
    return exact;
  }
  const [primary, ...rest] = tag.toLowerCase().split("-");
  if (primary === "zh") {
    return rest.some((part) => ["hans", "cn", "sg", "my"].includes(part)) ? "zh-Hans" : "zh-Hant";
  }
  return isLanguage(primary) ? primary : FALLBACK_LANGUAGE;
}

/**
 * 初始化 i18next。**絕對不會 reject。**
 *
 * 候選框是無邊框透明視窗，沒有標題列也沒有關閉鈕。初始化擲出例外而讓整棵
 * React tree 畫不出來的話，使用者只能去工作管理員找它。寧可顯示原始的 key。
 */
export async function initI18n(): Promise<void> {
  const language = await wantedLanguage();
  try {
    await i18next.use(initReactI18next).init({
      lng: language,
      fallbackLng: FALLBACK_LANGUAGE,
      supportedLngs: [...LANGUAGES],
      resources,
      // React 自己會轉義
      interpolation: { escapeValue: false },
      // 資源是靜態打包進來的，沒有東西要等。顯式關掉免得第一次 render
      // 意外 suspend 成一個空視窗。
      react: { useSuspense: false },
      debug: import.meta.env.DEV,
    });
  } catch (error) {
    console.error("i18n 初始化失敗，介面會顯示原始的 key", error);
  }
  applyLanguage(i18next.language || language);
}

/**
 * `<html lang>` 不只是無障礙——**Chromium 靠它挑 CJK 字型**。同一個漢字在 ja
 * 與 zh-Hant 下字形不同（直、次、骨、令），設錯的話日文使用者會看到中文字形。
 * 這也是 CSS 字型堆疊能拿掉具名 CJK 字型的前提。
 */
function applyLanguage(language: string) {
  document.documentElement.lang = language;
}

/**
 * 訂閱後端推來的語系變更。**兩個入口都要掛**——只掛一個的話症狀是
 * 「換語言後設定視窗變了、候選框沒變」，看起來像事件派送壞了。
 *
 * 刻意不回傳 unlisten：兩個視窗都不會被銷毀（候選框走 hide()，設定視窗是
 * prevent_close + hide()），監聽器的生命週期等於視窗，沒有可以拆的時機。
 */
export function watchLanguage() {
  void listen<Language>(EVENT_LANGUAGE, (event) => {
    void i18next.changeLanguage(event.payload).then(() => applyLanguage(event.payload));
  });
}
