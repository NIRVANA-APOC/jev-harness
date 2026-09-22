//! 宽度工具：一切宽度按终端显示列度量，裁剪一律以 grapheme 整簇为单位。
//!
//! 对应设计调研 §7.3 的「宽度与折行三件套」：`unicode-width` 提供显示列度量，
//! `unicode-segmentation` 提供 grapheme 分簇，`textwrap` 只负责词级折行。
//! 块产出的屏幕行（[`crate::blocks`]）已经过本模块换行，渲染层不再二次折行。

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// 一个 grapheme 簇的显示列宽。
pub fn cluster_width(cluster: &str) -> usize {
    UnicodeWidthStr::width(cluster)
}

/// 字符串的显示列宽。控制字符宽 0，东亚全角字符宽 2。
pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// 取前 `max` 列能容纳的 grapheme 整簇前缀；跨边界的宽字符整簇舍弃，不劈开。
pub fn clip(s: &str, max: usize) -> &str {
    let mut used = 0;
    for (idx, cluster) in s.grapheme_indices(true) {
        let w = cluster_width(cluster);
        if used + w > max {
            return &s[..idx];
        }
        used += w;
    }
    s
}

/// 裁剪到 `max` 列，被裁掉时在末尾补省略号（省略号自身宽度从预算里扣）。
pub fn clip_with_ellipsis(s: &str, max: usize, ellipsis: &str) -> String {
    if display_width(s) <= max {
        return s.to_owned();
    }
    let ellipsis_width = display_width(ellipsis);
    if max <= ellipsis_width {
        return clip(ellipsis, max).to_owned();
    }
    let mut out = clip(s, max - ellipsis_width).to_owned();
    out.push_str(ellipsis);
    out
}

/// 右侧补空格到 `width` 列；已超宽则原样返回。
pub fn pad_right(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w >= width {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len() + (width - w));
    out.push_str(s);
    out.extend(std::iter::repeat_n(' ', width - w));
    out
}

/// 词级折行，按显示列宽度。返回的每一行宽度都 ≤ `width`；空串折出一行空行。
pub fn wrap_words(s: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    textwrap::wrap(s, width)
        .into_iter()
        .map(|line| line.into_owned())
        .collect()
}

/// 硬折行：不看词边界，按显示列直接切。用于无空格的长串（路径、URL、连续 token）。
pub fn wrap_hard(s: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut used = 0;
    for cluster in s.graphemes(true) {
        let w = cluster_width(cluster);
        if used + w > width && used > 0 {
            lines.push(std::mem::take(&mut current));
            used = 0;
        }
        current.push_str(cluster);
        used += w;
    }
    lines.push(current);
    lines
}

/// 字符串中第 `byte_idx` 字节处之前的显示列数（光标列换算用）。
/// `byte_idx` 落在簇内部时按簇起点计。
pub fn columns_before(s: &str, byte_idx: usize) -> usize {
    let mut used = 0;
    for (idx, cluster) in s.grapheme_indices(true) {
        if idx >= byte_idx {
            break;
        }
        used += cluster_width(cluster);
    }
    used
}

/// 反向换算：显示列 `column` 处对应的字节偏移（落在宽字符中间时取该簇起点）。
pub fn byte_index_at_column(s: &str, column: usize) -> usize {
    let mut used = 0;
    for (idx, cluster) in s.grapheme_indices(true) {
        let w = cluster_width(cluster);
        if used + w > column {
            return idx;
        }
        used += w;
    }
    s.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_chars_count_two_columns() {
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("中文"), 4);
        assert_eq!(display_width("中a"), 3);
        assert_eq!(display_width(""), 0);
    }

    #[test]
    fn clip_keeps_grapheme_clusters_whole() {
        assert_eq!(clip("中文", 3), "中");
        assert_eq!(clip("中文", 1), "");
        assert_eq!(clip("abc", 2), "ab");
    }

    #[test]
    fn clip_keeps_combining_and_zwj_clusters() {
        let combining = "e\u{301}x";
        assert_eq!(clip(combining, 1), "e\u{301}");
        let family = "👨‍👩‍👧x";
        assert_eq!(clip(family, 2), "👨‍👩‍👧");
    }

    #[test]
    fn clip_with_ellipsis_reserves_room() {
        assert_eq!(clip_with_ellipsis("abcdef", 6, "…"), "abcdef");
        assert_eq!(clip_with_ellipsis("abcdef", 4, "…"), "abc…");
        assert_eq!(clip_with_ellipsis("abcdef", 1, "…"), "…");
    }

    #[test]
    fn pad_right_pads_by_columns() {
        assert_eq!(pad_right("a", 3), "a  ");
        assert_eq!(pad_right("中", 3), "中 ");
        assert_eq!(pad_right("abcd", 3), "abcd");
    }

    #[test]
    fn wrap_words_respects_display_width() {
        assert_eq!(wrap_words("hello world", 5), vec!["hello", "world"]);
        let cjk = wrap_words("中文中文中文", 4);
        assert_eq!(cjk, vec!["中文", "中文", "中文"]);
        for line in cjk {
            assert!(display_width(&line) <= 4);
        }
        assert_eq!(wrap_words("", 10), vec![""]);
        assert!(wrap_words("x", 0).is_empty());
    }

    #[test]
    fn wrap_hard_splits_long_tokens() {
        assert_eq!(wrap_hard("abcdefg", 3), vec!["abc", "def", "g"]);
        assert_eq!(wrap_hard("中文中文", 4), vec!["中文", "中文"]);
        assert!(wrap_hard("abc", 0).is_empty());
    }

    #[test]
    fn column_byte_conversions_round_trip() {
        let s = "a中b";
        assert_eq!(columns_before(s, 0), 0);
        assert_eq!(columns_before(s, 1), 1);
        assert_eq!(columns_before(s, 4), 3);
        assert_eq!(byte_index_at_column(s, 1), 1);
        assert_eq!(byte_index_at_column(s, 2), 1);
        assert_eq!(byte_index_at_column(s, 3), 4);
        assert_eq!(byte_index_at_column(s, 99), s.len());
    }
}
