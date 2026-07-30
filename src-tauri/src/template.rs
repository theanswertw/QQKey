//! 命令樣板的佔位符處理。
//!
//! 樣板以 `{名稱}` 標記待填參數，例如 `usbipd attach --wsl --busid {busid}`。
//! 佔位符不會被送進命令列——送出的內容截斷在它之前，游標自然停在
//! 使用者該接手輸入的位置，候選框則以灰字提示剩下要填什麼。

/// 取出實際要送進命令列的前綴。
pub fn injectable_prefix(template: &str) -> &str {
    match find_placeholder(template) {
        Some(index) => &template[..index],
        None => template,
    }
}

/// 回傳第一個佔位符的起始位置。只有成對的 `{}` 才算數，
/// 落單的左括號視為命令本身的一部分。
fn find_placeholder(template: &str) -> Option<usize> {
    let open = template.find('{')?;
    template[open..].find('}').map(|_| open)
}

/// 剝掉不該送進命令列的控制字元。
///
/// 整個工具的承諾是「填入而不執行」，但 `SendInput` 是逐個字元送出的——
/// 送一個 CR/LF 給終端機就等於替使用者按下 Enter，命令會直接跑起來；
/// Tab 則會觸發 PowerShell 補全，把剛填好的字改掉。
///
/// 樣板不是全都由使用者親手打進來的：從剪貼簿匯入的 JSON 完全繞過設定
/// 畫面那個單行輸入框，一份挾帶 `git push --force\n` 的分享檔就足以讓人
/// 按一次 Enter 就強推。這裡是送出前的最後一道，資料庫裡萬一已經有髒
/// 資料也擋得住。
pub fn sanitize(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// 樣板裡有沒有控制字元。新增與匯入都先問過這裡，
/// 讓問題在寫進資料庫之前就被擋下來，而不是等到注入時才無聲地消失。
pub fn has_control_chars(template: &str) -> bool {
    template.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::injectable_prefix;

    #[test]
    fn keeps_template_without_placeholder() {
        assert_eq!(injectable_prefix("usbipd list"), "usbipd list");
    }

    #[test]
    fn cuts_at_first_placeholder_and_keeps_trailing_space() {
        assert_eq!(
            injectable_prefix("usbipd attach --wsl --busid {busid}"),
            "usbipd attach --wsl --busid "
        );
    }

    #[test]
    fn cuts_at_the_first_of_many_placeholders() {
        assert_eq!(injectable_prefix("cp {src} {dest}"), "cp ");
    }

    #[test]
    fn unclosed_brace_is_not_a_placeholder() {
        assert_eq!(injectable_prefix("echo {unclosed"), "echo {unclosed");
    }

    #[test]
    fn placeholder_at_the_very_start_yields_empty_prefix() {
        assert_eq!(injectable_prefix("{cmd} --help"), "");
    }

    #[test]
    fn sanitize_strips_the_newline_that_would_run_the_command() {
        assert_eq!(
            super::sanitize("git push --force\n"),
            "git push --force",
            "換行送到終端機就是 Enter，命令會直接執行"
        );
        assert_eq!(super::sanitize("git status\r\n"), "git status");
    }

    #[test]
    fn sanitize_strips_tab_which_would_trigger_completion() {
        assert_eq!(
            super::sanitize("git\tstatus"),
            "gitstatus",
            "Tab 會觸發 PowerShell 補全，把填好的字改掉"
        );
    }

    #[test]
    fn sanitize_keeps_spaces_and_non_ascii() {
        assert_eq!(
            super::sanitize("usbipd attach --wsl --busid "),
            "usbipd attach --wsl --busid ",
            "命令裡的空格是有意義的，不能跟控制字元一起清掉"
        );
        assert_eq!(super::sanitize("echo 掛載 🚀"), "echo 掛載 🚀");
    }

    #[test]
    fn detects_control_chars_before_they_reach_the_database() {
        assert!(super::has_control_chars("git push\n"));
        assert!(super::has_control_chars("git\tpush"));
        assert!(
            !super::has_control_chars("usbipd attach --wsl --busid {busid}"),
            "正常的樣板不該被誤判"
        );
    }
}
