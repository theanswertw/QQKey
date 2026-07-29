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
}
