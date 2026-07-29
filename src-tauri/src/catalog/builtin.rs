//! 內建命令目錄。
//!
//! 目錄以 JSON 內嵌進執行檔，新增工具需要重新編譯——內建目錄本來就隨版本
//! 更新，使用者自己加的條目走資料庫，由設定畫面維護。

use serde::Deserialize;

use super::NewEntry;

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
    commands: Vec<NewEntry>,
}

pub fn load_builtin() -> Vec<NewEntry> {
    CATALOGS
        .iter()
        .filter_map(|raw| match serde_json::from_str::<CatalogFile>(raw) {
            Ok(file) => Some(file.commands),
            Err(error) => {
                crate::trace("目錄", &format!("內建目錄解析失敗：{error}"));
                None
            }
        })
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::load_builtin;

    #[test]
    fn every_builtin_catalog_parses() {
        let entries = load_builtin();
        assert_eq!(
            entries.len() > 100,
            true,
            "八個工具應該有上百筆命令，實際 {}",
            entries.len()
        );
    }

    #[test]
    fn templates_are_unique() {
        let entries = load_builtin();
        let mut seen = std::collections::HashSet::new();
        for entry in &entries {
            assert!(
                seen.insert(entry.template.clone()),
                "template 重複：{}",
                entry.template
            );
        }
    }

    #[test]
    fn every_entry_has_a_description() {
        for entry in load_builtin() {
            assert!(
                entry.description.is_some(),
                "{} 少了中文說明",
                entry.template
            );
        }
    }
}
