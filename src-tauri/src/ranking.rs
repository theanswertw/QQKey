//! 候選排序：模糊比對分數 × frecency。

use std::cmp::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::catalog::Entry;

/// frecency 的半衰期：三十天沒用到，權重減半。
const HALF_LIFE_SECONDS: f64 = 30.0 * 24.0 * 60.0 * 60.0;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// 把分數衰減到 `now` 這個時點。
///
/// 只存一個分數加上最後使用時間，不必保留完整的使用時間序列：
/// 每次使用時先衰減再加一，效果等同對歷次使用做指數加權。
pub fn decay(score: f64, last_used: Option<i64>, now: i64) -> f64 {
    let Some(last_used) = last_used else {
        return score;
    };
    let elapsed = (now - last_used).max(0) as f64;
    score * 0.5f64.powf(elapsed / HALF_LIFE_SECONDS)
}

/// 使用一次之後的新分數。
pub fn bump(score: f64, last_used: Option<i64>, now: i64) -> f64 {
    decay(score, last_used, now) + 1.0
}

/// frecency 對排序的加權。取對數讓常用項目上浮，
/// 但不會常用到完全壓過比對品質——搜尋精準度仍該優先。
fn frecency_weight(entry: &Entry, now: i64) -> f64 {
    let decayed = decay(entry.score, entry.last_used, now);
    1.0 + 2.0 * (1.0 + decayed + entry.boost).ln()
}

/// 依查詢字串排序候選項目。查詢為空時純粹依 frecency 排。
pub fn rank<'a>(entries: &'a [Entry], query: &str, now: i64, limit: usize) -> Vec<&'a Entry> {
    let query = query.trim();

    let mut scored: Vec<(f64, &Entry)> = if query.is_empty() {
        // 只列出用過或手動加權過的。剛叫出候選框就塞一串沒用過的命令是噪音，
        // 使用者本來就要打字；等有使用紀錄後這裡才真的是「最常用」。
        entries
            .iter()
            .filter(|entry| entry.score > 0.0 || entry.boost > 0.0)
            .map(|entry| (frecency_weight(entry, now), entry))
            .collect()
    } else {
        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let mut buffer = Vec::new();

        entries
            .iter()
            .filter_map(|entry| {
                let haystack = entry.haystack();
                let matched = pattern.score(Utf32Str::new(&haystack, &mut buffer), &mut matcher)?;
                Some((matched as f64 * frecency_weight(entry, now), entry))
            })
            .collect()
    };

    scored.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(Ordering::Equal)
            // 分數相同時先看來源，再讓較短的命令排前面——通常是比較基本的那個
            .then_with(|| right.1.source.priority().cmp(&left.1.source.priority()))
            .then_with(|| left.1.template.len().cmp(&right.1.template.len()))
    });

    scored
        .into_iter()
        .take(limit)
        .map(|(_, entry)| entry)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{bump, decay, rank, HALF_LIFE_SECONDS};
    use crate::catalog::{Entry, Source};

    const NOW: i64 = 1_800_000_000;

    fn entry(id: i64, template: &str, keywords: Option<&str>, score: f64, source: Source) -> Entry {
        Entry {
            id,
            template: template.to_string(),
            description: None,
            keywords: keywords.map(str::to_string),
            source,
            enabled: true,
            score,
            last_used: if score > 0.0 { Some(NOW) } else { None },
            boost: 0.0,
        }
    }

    #[test]
    fn decays_by_half_after_one_half_life() {
        let older = NOW - HALF_LIFE_SECONDS as i64;
        assert!((decay(4.0, Some(older), NOW) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn never_used_entries_keep_their_score() {
        assert_eq!(decay(3.0, None, NOW), 3.0);
    }

    #[test]
    fn bump_adds_one_to_the_decayed_score() {
        assert!((bump(0.0, None, NOW) - 1.0).abs() < 1e-9);
        assert!((bump(2.0, Some(NOW), NOW) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn empty_query_lists_used_entries_by_frecency() {
        let entries = vec![
            entry(1, "git status", None, 0.0, Source::Builtin),
            entry(2, "usbipd list", None, 9.0, Source::Builtin),
            entry(3, "wsl --shutdown", None, 3.0, Source::Builtin),
        ];
        let ranked = rank(&entries, "", NOW, 10);
        assert_eq!(
            ranked.iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![2, 3],
            "沒用過的條目不該在空查詢時列出"
        );
    }

    #[test]
    fn filters_out_entries_that_do_not_match() {
        let entries = vec![
            entry(1, "git status", None, 0.0, Source::Builtin),
            entry(2, "usbipd list", None, 0.0, Source::Builtin),
        ];
        let ranked = rank(&entries, "usbipd", NOW, 10);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].id, 2);
    }

    #[test]
    fn matches_chinese_keywords() {
        let entries = vec![
            entry(1, "git status", Some("狀態"), 0.0, Source::Builtin),
            entry(
                2,
                "usbipd attach --wsl --busid {busid}",
                Some("掛載 wsl"),
                0.0,
                Source::Builtin,
            ),
        ];
        let ranked = rank(&entries, "掛載", NOW, 10);
        assert_eq!(ranked.len(), 1, "中文關鍵字應該要能搜到");
        assert_eq!(ranked[0].id, 2);
    }

    #[test]
    fn frequently_used_entry_outranks_an_unused_one() {
        let entries = vec![
            entry(1, "git stash", None, 0.0, Source::Builtin),
            entry(2, "git status", None, 20.0, Source::Builtin),
        ];
        let ranked = rank(&entries, "git st", NOW, 10);
        assert_eq!(ranked[0].id, 2);
    }

    #[test]
    fn user_entries_win_ties_against_history() {
        let entries = vec![
            entry(1, "git status", None, 0.0, Source::History),
            entry(2, "git status ", None, 0.0, Source::User),
        ];
        let ranked = rank(&entries, "git status", NOW, 10);
        assert_eq!(ranked[0].source, Source::User);
    }

    #[test]
    fn respects_the_limit() {
        let entries: Vec<Entry> = (0..20)
            .map(|i| entry(i, &format!("git cmd{i}"), None, 1.0, Source::Builtin))
            .collect();
        assert_eq!(rank(&entries, "", NOW, 9).len(), 9);
        assert_eq!(rank(&entries, "git", NOW, 9).len(), 9);
    }
}
