import type { Language } from "./languages";
import de from "./locales/de.json";
import en from "./locales/en.json";
import fr from "./locales/fr.json";
import ja from "./locales/ja.json";
import ko from "./locales/ko.json";
import zhHant from "./locales/zh-Hant.json";

/** zh-Hant 是原文，其餘語系檔的鍵必須與它一致。 */
export type Catalog = typeof zhHant;

/*
 * 下面那個型別標註是這一整套唯一的防呆，擋掉兩件事：少一個語系，以及某個語系
 * 檔少一個鍵。沒有它的話 tsc 只會檢查 zh-Hant，漏掉的鍵要等到使用者切到那個
 * 語言、畫面上冒出原始的 key 才會被發現。
 *
 * 注意它擋不到「多」出來的鍵（i18next 的複數形式 key_one／key_other 就是這樣
 * 存在的）。所以規則是：每一個帶 {{count}} 的鍵，六個檔案一律同時提供
 * _one 與 _other，中日韓兩者填相同文字。
 */
export const resources: Record<Language, { translation: Catalog }> = {
  "zh-Hant": { translation: zhHant },
  ja: { translation: ja },
  en: { translation: en },
  fr: { translation: fr },
  de: { translation: de },
  ko: { translation: ko },
};
