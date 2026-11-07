//! 拼音索引与查询通道（P3-C）。
//!
//! ## 目标
//! 输入拼音（如 `renmin`）也能命中中文文档（"人民"）。
//!
//! ## 为什么不在查询侧做拼音→汉字扩展
//! 拼音到汉字是一对多（`ren` → 人/仁/任/…），精确展开需要大字典且会引入
//! 噪声。改用**索引侧附加拼音**：把正文中的汉字词转成无声调拼音连写串存入
//! 独立字段，查询侧把纯 ASCII 查询**同时投到拼音字段**（OR），从而
//! "拼音 → 文档"直达。
//!
//! ## 与分词器的一致性
//! 拼音字段复用同一 `seg` 分词器（索引侧 1–3 gram、查询侧 bigram AND），
//! 因此 `renmin` 与 `renmingongheguo` 的 gram 前缀天然对齐，无需另建索引策略。

use pinyin::ToPinyin;

/// 拼音源截断上限：超长正文只对**前 256 KB** 计算拼音。
///
/// 理由（P3 审计发现）：拼音转换约为每字符一次查表 + 输出串（长度≈2–3 倍），
/// 8 MB 文档会带来数十毫秒级 CPU 与十几 MB 临时内存，拖累索引吞吐。
/// 截断后拼音检索对超长文档覆盖前段（正文前 256 KB 通常已含标题级关键词）。
pub const PINYIN_SOURCE_LIMIT: usize = 256 * 1024;

/// 按 UTF-8 字符边界向后对齐的截断
fn floor_char_boundary(s: &str, mut cut: usize) -> usize {
    if cut >= s.len() {
        return s.len();
    }
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    cut
}

/// 文本是否含**需要转拼音**的字符（任一非 ASCII 字节即视为需要）。
///
/// 用途：纯 ASCII 文本（英文、代码、日志）的拼音串就是原文小写化本身，
/// 而原文已由 body 字段索引，pinyin 字段对它完全冗余。
/// 按字节扫一遍就返回，比逐字转拼音便宜两个数量级。
#[inline]
pub fn needs_pinyin(text: &str) -> bool {
    text.as_bytes().iter().any(|&b| b >= 0x80)
}

/// 文本 → 拼音文本（汉字转无声调拼音"连写"，非汉字作为词边界）
///
/// 例：`人民 共和国` → `renmin gongheguo`；`AI 时代` → `ai shidai`
/// （ASCII 字母数字原样保留，从而支持中英混排词）
///
/// 超 [`PINYIN_SOURCE_LIMIT`] 的输入只取前段（见常量说明）。
///
/// **纯 ASCII 输入返回空串**：内容已由 body 字段覆盖，拼音通道不需要它。
pub fn text_to_pinyin_text(text: &str) -> String {
    if !needs_pinyin(text) {
        return String::new();
    }
    let text = if text.len() > PINYIN_SOURCE_LIMIT {
        &text[..floor_char_boundary(text, PINYIN_SOURCE_LIMIT)]
    } else {
        text
    };
    let mut out = String::with_capacity(text.len() * 2);
    let mut in_word = false;
    for ch in text.chars() {
        if let Some(py) = ch.to_pinyin() {
            out.push_str(py.plain());
            in_word = true;
        } else if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            in_word = true;
        } else if in_word {
            out.push(' ');
            in_word = false;
        }
    }
    out
}

/// 是否为"拼音式查询"：非空且仅由 ASCII 字母/数字构成（含至少一个字母）
///
/// 只有这种查询才追加拼音字段的 OR 子句；中文查询走原路径不受影响。
pub fn is_pinyin_query(q: &str) -> bool {
    let q = q.trim();
    !q.is_empty()
        && q.chars().all(|c| c.is_ascii_alphanumeric())
        && q.chars().any(|c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_to_pinyin_joined_per_word() {
        assert_eq!(text_to_pinyin_text("人民"), "renmin");
        assert_eq!(text_to_pinyin_text("人民 共和国"), "renmin gongheguo");
        // 标点作词边界
        assert_eq!(text_to_pinyin_text("人民，共和国"), "renmin gongheguo");
    }

    #[test]
    fn ascii_passthrough_and_mixed() {
        assert_eq!(text_to_pinyin_text("AI 时代"), "ai shidai");
        assert_eq!(text_to_pinyin_text("Hello"), "hello");
    }

    #[test]
    fn pinyin_query_detection() {
        assert!(is_pinyin_query("renmin"));
        assert!(is_pinyin_query("renmin2"));
        assert!(!is_pinyin_query("人民"));
        assert!(!is_pinyin_query(""));
        assert!(!is_pinyin_query("123"));
        assert!(!is_pinyin_query("ren min"));
    }

    #[test]
    fn long_text_is_truncated_at_char_boundary() {
        // 超限输入只取前段，且不 panic（中文字符 3 字节，截断点须落在边界）
        let big = "人民".repeat(PINYIN_SOURCE_LIMIT); // ≈ 768 KB UTF-8
        let out = text_to_pinyin_text(&big);
        assert!(!out.is_empty());
        // 截断后长度受控：源 ≤ 256KB，拼音 ≤ 3 倍源 + 空格
        assert!(
            out.len() <= PINYIN_SOURCE_LIMIT * 3 + 16,
            "输出应受截断约束: {}",
            out.len()
        );
        // 未超限时完整转换
        assert_eq!(text_to_pinyin_text("人民"), "renmin");
    }
}
