//! 內建命令目錄。
//!
//! 目錄以 JSON 內嵌進執行檔，新增工具需要重新編譯——內建目錄本來就隨版本
//! 更新，使用者自己加的條目走資料庫，由設定畫面維護。
//!
//! 說明與關鍵字六個語言存在同一個檔案裡，而不是一個語言一個目錄。因為
//! `template` 是資料庫的 UNIQUE key 也是 `sync_builtin()` 的 upsert 依據，
//! 拆檔就得把它複製六份，而複製失敗的模式是靜默的：某個語言的 template 少一個
//! 字元，那一筆在那個語言就變成獨立的新條目，看起來卻像「這個語言忘了翻」。

use serde::Deserialize;

use super::NewEntry;
use crate::i18n::Lang;

const CATALOGS: &[&str] = &[
    include_str!("../../resources/catalog/usbipd.json"),
    include_str!("../../resources/catalog/git.json"),
    include_str!("../../resources/catalog/wsl.json"),
    include_str!("../../resources/catalog/netsh.json"),
    include_str!("../../resources/catalog/docker.json"),
    include_str!("../../resources/catalog/winget.json"),
    include_str!("../../resources/catalog/npm.json"),
    include_str!("../../resources/catalog/cargo.json"),
];

/// JSON 檔中的 `tool` 欄位只是給人看的分類，serde 會自動忽略。
#[derive(Deserialize)]
struct CatalogFile {
    commands: Vec<CatalogEntry>,
}

/// 磁碟上的形狀。
///
/// 刻意跟 [`NewEntry`]（要寫進資料庫的形狀）分開：一個是六語言全帶，一個是
/// 選定語言加上聯集。硬塞成同一個型別，「這個 keywords 是哪一種」就會變成
/// 要靠上下文猜的事。
#[derive(Deserialize)]
struct CatalogEntry {
    template: String,
    #[serde(default)]
    description: Option<LangMap>,
    #[serde(default)]
    keywords: Option<LangMap>,
}

/// 一段文字的六個語言版本。
///
/// 用六個具名欄位而不是 `HashMap<String, String>`：語言標籤打成 `"zh-hant"`
/// 或 `"jp"` 的話，HashMap 會安靜地多收一個沒人讀的 key，而具名欄位配上
/// `deny_unknown_fields` 會在載入時就講出來。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LangMap {
    #[serde(rename = "zh-Hant", default)]
    zh_hant: Option<String>,
    #[serde(default)]
    ja: Option<String>,
    #[serde(default)]
    en: Option<String>,
    #[serde(default)]
    fr: Option<String>,
    #[serde(default)]
    de: Option<String>,
    #[serde(default)]
    ko: Option<String>,
}

impl LangMap {
    fn get(&self, lang: Lang) -> Option<&str> {
        let slot = match lang {
            Lang::ZhHant => &self.zh_hant,
            Lang::Ja => &self.ja,
            Lang::En => &self.en,
            Lang::Fr => &self.fr,
            Lang::De => &self.de,
            Lang::Ko => &self.ko,
        };
        slot.as_deref().filter(|text| !text.trim().is_empty())
    }

    /// 缺譯時退回英文，再退回繁中。
    ///
    /// 注意這條 fallback 鏈的終點是**繁中**，跟介面外框的 `i18n::FALLBACK`（英文）
    /// 不同。介面外框缺譯是不該發生的事（巨集擋得住），目錄缺一句德文說明卻很
    /// 可能，而那時繁中比空白好。這個不對稱是刻意的，別為了「統一」而改掉它。
    ///
    /// 也刻意不 panic 也不整檔拒收：少一句德文說明不該讓整個 docker 工具從
    /// 候選池消失。缺譯由 `every_entry_is_translated_into_every_language` 擋。
    fn resolve(&self, lang: Lang) -> Option<&str> {
        self.get(lang)
            .or_else(|| self.get(Lang::En))
            .or_else(|| self.get(Lang::ZhHant))
    }

    /// 六個語言所有非空白詞的聯集，去重後以空白接起來。
    ///
    /// 順序跟著 `Lang::ALL`，但當前語言那一份會由呼叫端另外放在前面，
    /// 所以這裡不必為排序操心。
    fn union(&self) -> Option<String> {
        let mut seen = Vec::new();
        for lang in Lang::ALL {
            for word in self.get(lang).unwrap_or_default().split_whitespace() {
                if !seen.iter().any(|kept: &&str| kept.eq_ignore_ascii_case(word)) {
                    seen.push(word);
                }
            }
        }
        (!seen.is_empty()).then(|| seen.join(" "))
    }
}

/// 依介面語言取得內建目錄。
///
/// `description` 與 `keywords` 跟隨語言，`keywords_all` 不跟——搜尋要在任何
/// 介面語言下都認得所有語言的關鍵字。
pub fn load_builtin(lang: Lang) -> Vec<NewEntry> {
    CATALOGS
        .iter()
        .filter_map(|raw| match parse_catalog(raw) {
            Ok(commands) => Some(commands),
            Err(error) => {
                crate::trace("目錄", &format!("內建目錄解析失敗：{error}"));
                None
            }
        })
        .flatten()
        .map(|entry| NewEntry {
            template: entry.template,
            description: entry
                .description
                .as_ref()
                .and_then(|map| map.resolve(lang))
                .map(str::to_string),
            keywords: entry
                .keywords
                .as_ref()
                .and_then(|map| map.resolve(lang))
                .map(str::to_string),
            keywords_all: entry.keywords.as_ref().and_then(LangMap::union),
        })
        .collect()
}

/// 抽出來讓測試能逐檔驗證。
///
/// `load_builtin()` 的 `filter_map` 會**靜默丟掉**整個解析失敗的檔案（只留一行
/// trace），所以「總筆數超過一百」這種門檻攔不住「少了 winget.json 那八筆」——
/// 條目一多就更攔不住。逐檔驗證才問得出「哪一個檔壞了」。
fn parse_catalog(raw: &str) -> serde_json::Result<Vec<CatalogEntry>> {
    serde_json::from_str::<CatalogFile>(raw).map(|file| file.commands)
}

#[cfg(test)]
mod tests {
    use super::{load_builtin, parse_catalog, CATALOGS};
    use crate::i18n::Lang;

    #[test]
    fn every_builtin_catalog_parses() {
        for (index, raw) in CATALOGS.iter().enumerate() {
            let parsed = parse_catalog(raw);
            assert!(
                parsed.is_ok(),
                "第 {index} 個目錄檔解析失敗（load_builtin 會靜默丟掉整個檔案）：{:?}",
                parsed.err()
            );
        }

        // 總數只是煙霧測試：真正擋得住「少一整個檔案」的是上面那個迴圈。
        let entries = load_builtin(Lang::ZhHant);
        assert!(
            entries.len() >= 100,
            "八個工具應該有上百筆命令，實際 {}",
            entries.len()
        );
    }

    #[test]
    fn templates_are_unique() {
        let entries = load_builtin(Lang::ZhHant);
        let mut seen = std::collections::HashSet::new();
        for entry in &entries {
            assert!(
                seen.insert(entry.template.clone()),
                "template 重複：{}",
                entry.template
            );
        }
    }

    /// 每個語言都要有自己的說明與關鍵字。
    ///
    /// 刻意檢查 `LangMap::get()` 而不是 `load_builtin()` 的產物：後者走
    /// `resolve()` 的 fallback，缺譯會靜靜地變成英文或繁中，於是測試永遠會過而
    /// 使用者看到的是別的語言。這裡問的是「這個語言真的有翻嗎」。
    #[test]
    fn every_entry_is_translated_into_every_language() {
        for raw in CATALOGS {
            for entry in parse_catalog(raw).expect("目錄檔應該解析得過") {
                let description = entry.description.as_ref();
                let keywords = entry.keywords.as_ref();
                for lang in Lang::ALL {
                    assert!(
                        description.and_then(|map| map.get(lang)).is_some(),
                        "{} 少了 {} 說明",
                        entry.template,
                        lang.as_tag()
                    );
                    assert!(
                        keywords.and_then(|map| map.get(lang)).is_some(),
                        "{} 少了 {} 關鍵字——那個語言的使用者只能用英文命令本身找到它",
                        entry.template,
                        lang.as_tag()
                    );
                }
            }
        }
    }

    /// 「輸入中文關鍵字找到英文命令」是這個工具的核心承諾，而它在介面切成
    /// 英文之後必須依然成立。這條測試是那個承諾的憑據。
    #[test]
    fn keywords_union_covers_every_language() {
        let entry = load_builtin(Lang::En)
            .into_iter()
            .find(|entry| entry.template == "usbipd attach --wsl --busid {busid}")
            .expect("usbipd attach 應該在內建目錄裡");

        let union = entry.keywords_all.expect("內建條目應該有六語言聯集");
        assert!(union.contains("掛載"), "英文介面下仍要認得中文關鍵字：{union}");
        assert!(union.contains("attach"), "英文關鍵字也要在裡面：{union}");

        let keywords = entry.keywords.expect("英文介面應該有英文關鍵字");
        assert!(
            !keywords.contains("掛載"),
            "給人看的那一欄只放當前語言，不然使用者一存檔就把六語言變成自己的關鍵字：{keywords}"
        );
    }

    /// 切換語言只該換掉說明與顯示用的關鍵字，不該動到 template 或搜尋範圍。
    #[test]
    fn switching_language_only_changes_the_readable_text() {
        let zh = load_builtin(Lang::ZhHant);
        let en = load_builtin(Lang::En);

        assert_eq!(zh.len(), en.len(), "兩個語言的條目數必須一樣");
        for (zh_entry, en_entry) in zh.iter().zip(en.iter()) {
            assert_eq!(
                zh_entry.template, en_entry.template,
                "template 不能隨語言變——它是資料庫的 UNIQUE key"
            );
            assert_eq!(
                zh_entry.keywords_all, en_entry.keywords_all,
                "{} 的搜尋範圍隨語言變了",
                zh_entry.template
            );
        }
    }
}
