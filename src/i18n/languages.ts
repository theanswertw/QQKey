/**
 * 支援的介面語言。
 *
 * 必須與後端 `i18n.rs` 的 `Lang` 序列化字串**逐字相同**。不一致的話兩邊會各自
 * fallback 而且都不報錯，畫面上只會看到一半換了語言。
 *
 * 用的是正規 BCP 47 標籤，所以可以直接餵給 `Intl.*` 與 `<html lang>`，
 * 兩邊都不需要轉換表。順序即設定畫面下拉選單的順序。
 */
export const LANGUAGES = ["zh-Hant", "ja", "en", "fr", "de", "ko"] as const;

export type Language = (typeof LANGUAGES)[number];

/** 認不出來的語言落到這裡，也是 i18next 的 fallbackLng。 */
export const FALLBACK_LANGUAGE: Language = "en";

/** `meta.language` 為這個值代表跟隨系統顯示語言。 */
export const AUTO_LANGUAGE = "auto";

/**
 * 語言名稱一律用該語言自己的寫法（endonym），**不隨介面語言翻譯**——
 * 使用者會來這個選單正是因為現在的介面語言他讀不懂，這時把選項也翻成
 * 他讀不懂的語言等於沒有出路。
 */
export const LANGUAGE_LABELS: Record<Language, string> = {
  "zh-Hant": "繁體中文",
  ja: "日本語",
  en: "English",
  fr: "Français",
  de: "Deutsch",
  ko: "한국어",
};

export function isLanguage(value: string): value is Language {
  return (LANGUAGES as readonly string[]).includes(value);
}
