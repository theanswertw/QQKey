//! 從 PSReadLine 歷史紀錄學習實際用過的命令。
//!
//! 歷史裡可能夾帶憑證，匯入前一律先過濾。被判定為疑似機密的行不會進資料庫，
//! 也不會出現在候選框裡；略過的筆數會回報給使用者，但內容不會被記錄下來。

use std::collections::HashMap;
use std::path::PathBuf;

use regex::Regex;
use serde::Serialize;

/// 一次匯入的結果，供設定畫面顯示。
#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    /// 這次讀到的行數
    pub scanned: usize,
    /// 去重後實際匯入的命令數
    pub imported: usize,
    /// 因疑似含憑證而略過的行數
    pub skipped_secret: usize,
    /// 因太短、純數字、只是切換目錄等原因略過的行數
    pub skipped_noise: usize,
}

/// PSReadLine 的歷史檔位置。
pub fn history_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("PowerShell")
            .join("PSReadLine")
            .join("ConsoleHost_history.txt"),
    )
}

/// 疑似含憑證的樣式。命中就整行略過。
///
/// 寧可誤殺也不要漏放——被誤殺的命令使用者可以在設定畫面自己補回來，
/// 漏放的憑證卻會一直躺在資料庫裡。
pub struct SecretFilter {
    keywords: Regex,
    long_value: Regex,
}

/// 預設的關鍵字樣式。
///
/// 刻意不加 `\b`：環境變數慣例是 `GITHUB_TOKEN`、`DB_PASSWORD` 這種底線命名，
/// 而底線算 word character，加了邊界反而會讓它們漏網。誤殺一筆普通命令只是
/// 少個候選，可以在設定畫面自己加回來；漏放的憑證卻會一直留在資料庫裡。
pub const DEFAULT_SECRET_PATTERN: &str =
    r"(?i)(password|passwd|secret|token|credential|api[-_]?key|bearer|[-/]pwd\b|-AsPlainText|ConvertTo-SecureString)";

/// `=` 或 `:` 後接一長串，多半是金鑰、連線字串或 base64 內容。
/// 這條規則不開放修改——它擋的是關鍵字列不出來的形態。
const LONG_VALUE_PATTERN: &str = r"[=:]\s*[A-Za-z0-9+/_-]{20,}";

impl Default for SecretFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretFilter {
    pub fn new() -> Self {
        Self::from_pattern(DEFAULT_SECRET_PATTERN).expect("內建的機密比對樣式應該要能編譯")
    }

    /// 以自訂的關鍵字樣式建立。樣式無效時回傳錯誤，呼叫端應退回預設值。
    pub fn from_pattern(pattern: &str) -> Result<Self, regex::Error> {
        Ok(Self {
            keywords: Regex::new(pattern)?,
            long_value: Regex::new(LONG_VALUE_PATTERN)
                .expect("內建的機密比對樣式應該要能編譯"),
        })
    }

    pub fn is_sensitive(&self, line: &str) -> bool {
        self.keywords.is_match(line) || self.long_value.is_match(line)
    }
}

/// 沒有保留價值的行。
fn is_noise(command: &str) -> bool {
    /// 超過這個長度多半是貼進終端機的一整段內容，不是真的在下命令
    const MAX_LENGTH: usize = 300;

    if command.len() < 3 || command.len() > MAX_LENGTH {
        return true;
    }
    if command.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    // 切換目錄是一次性的，留著只會把真正有用的命令洗掉
    if command.starts_with("cd ") || command.starts_with("cd\\") {
        return true;
    }
    // 只有可執行檔、沒有子命令或參數的（ls、cls、exit），使用者本來就記得住
    command.split_whitespace().count() < 2
}

/// 解析歷史文字，回傳去重後的命令與其出現次數，以及過濾統計。
pub fn parse(text: &str, filter: &SecretFilter) -> (Vec<(String, usize)>, ImportReport) {
    let mut report = ImportReport::default();
    let mut counts: HashMap<String, usize> = HashMap::new();

    for line in text.lines() {
        report.scanned += 1;
        let command = line.trim();

        if is_noise(command) {
            report.skipped_noise += 1;
            continue;
        }
        if filter.is_sensitive(command) {
            report.skipped_secret += 1;
            continue;
        }
        *counts.entry(command.to_string()).or_insert(0) += 1;
    }

    report.imported = counts.len();
    let mut entries: Vec<(String, usize)> = counts.into_iter().collect();
    // 出現次數多的排前面；次數相同時按字典序，讓結果穩定好測
    entries.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

    (entries, report)
}

/// 把歷史出現次數換算成初始 frecency 分數。
///
/// 取對數壓縮：用過三百次的 `git status` 不該永遠壓在整理過的內建目錄之上，
/// 但也該比只用過一兩次的排前面。
pub fn initial_score(count: usize) -> f64 {
    (1.0 + count as f64).ln()
}

#[cfg(test)]
mod tests {
    use super::{initial_score, is_noise, parse, SecretFilter};

    #[test]
    fn filters_out_lines_that_look_like_credentials() {
        let filter = SecretFilter::new();
        let sensitive = [
            "$cred = ConvertTo-SecureString 'abc' -AsPlainText -Force",
            "curl -H \"Authorization: Bearer eyJhbGciOiJIUzI1NiJ9\" https://x",
            "az login --password hunter2",
            "setx API_KEY abcdef",
            "docker login -u me --Password x",
            "export GITHUB_TOKEN=ghp_xxx",
            "$env:AZURE_CLIENT_SECRET = 'x'",
            "psql postgres://user:supersecretvalue1234567890@host/db",
            "kubectl create secret generic db --from-literal=user=admin",
        ];
        for line in sensitive {
            assert!(filter.is_sensitive(line), "應判定為機密：{line}");
        }
    }

    #[test]
    fn keeps_ordinary_commands() {
        let filter = SecretFilter::new();
        let ordinary = [
            "usbipd attach --wsl --busid 2-3",
            "git commit -m \"修正定位\"",
            "docker compose up -d",
            "netsh interface portproxy show all",
            "npm run build",
        ];
        for line in ordinary {
            assert!(!filter.is_sensitive(line), "不該判定為機密：{line}");
        }
    }

    #[test]
    fn drops_noise() {
        assert!(is_noise("ls"), "單一 token 的命令沒有提示價值");
        assert!(is_noise("cls"));
        assert!(is_noise("cd C:\\Projects"), "切換目錄是一次性的");
        assert!(is_noise("12345"));
        assert!(is_noise(""));
        assert!(is_noise(&"x".repeat(400)), "過長的行多半是貼上的內容");

        assert!(!is_noise("git status"));
        assert!(!is_noise("usbipd list"));
    }

    #[test]
    fn deduplicates_and_counts() {
        let text = "git status\nusbipd list\ngit status\ngit status\n";
        let (entries, report) = parse(text, &SecretFilter::new());

        assert_eq!(report.scanned, 4);
        assert_eq!(report.imported, 2);
        assert_eq!(entries[0], ("git status".to_string(), 3));
        assert_eq!(entries[1], ("usbipd list".to_string(), 1));
    }

    #[test]
    fn reports_what_was_skipped() {
        let text = "git status\nls\naz login --password hunter2\n";
        let (entries, report) = parse(text, &SecretFilter::new());

        assert_eq!(report.scanned, 3);
        assert_eq!(report.imported, 1);
        assert_eq!(report.skipped_noise, 1);
        assert_eq!(report.skipped_secret, 1);
        assert_eq!(entries.len(), 1, "被過濾的內容不該出現在結果裡");
    }

    #[test]
    fn custom_pattern_replaces_the_keyword_list() {
        let filter = super::SecretFilter::from_pattern(r"(?i)(內部代號)").unwrap();
        assert!(filter.is_sensitive("git commit -m 內部代號X"));
        // 換掉關鍵字列之後，預設的那些就不再攔截
        assert!(!filter.is_sensitive("az login --password hunter2"));
        // 但長字串那條規則是固定的，仍然生效
        assert!(filter.is_sensitive("setx FOO=abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    fn invalid_pattern_is_rejected_rather_than_panicking() {
        assert!(super::SecretFilter::from_pattern(r"(未閉合").is_err());
    }

    #[test]
    fn initial_score_grows_slowly() {
        assert!(initial_score(1) < initial_score(10));
        assert!(initial_score(10) < initial_score(100));
        assert!(
            initial_score(300) < 6.0,
            "用過幾百次也不該把分數推到壓過一切"
        );
    }
}
