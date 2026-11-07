//! 分词内核（S3/S8 spike 定型，生产版）
//!
//! 策略（spike 双门禁验证：S8 37/37 + S3 hit@5 100%）：
//!   查询侧 = 无缝切分 DP（jieba cut_for_search 词边，丢弃 ≥4 字边，最少段数无缝覆盖；
//!              无解回退精确模式）
//!   索引侧 = 每空白块所有 1-3 字连续子串（char n-gram）+ 整块本身
//!   >3 字查询 token（未登录回退整词）→ 拆相邻 bigram AND
//!
//! 第一性原理：≤3 字 token 的"子串存在性" ⟺ "n-gram token 存在性"，
//! 故 tantivy AND 与 oracle 子串语义严格对齐；AND 语义下索引 token
//! 超集化不改变已有查询结果。
//!
//! 性能：Jieba::new() 解析 350 万词条词典以秒计，必须 OnceLock 缓存。

use std::collections::HashSet;
use std::sync::OnceLock;

fn jieba() -> &'static jieba_rs::Jieba {
    static JIEBA: OnceLock<jieba_rs::Jieba> = OnceLock::new();
    JIEBA.get_or_init(jieba_rs::Jieba::new)
}

/// jieba 精确模式分词（回退参考用）
pub fn jieba_cut_exact(text: &str) -> Vec<String> {
    jieba()
        .cut(text, false)
        .into_iter()
        .map(|t| t.word.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 查询侧分词：纯净无缝切分（spike S8 定型）
pub fn segment_query(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for chunk in text.split_whitespace() {
        out.extend(segment_chunk(chunk));
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|t| seen.insert(t.clone()));
    out
}

fn segment_chunk(chunk: &str) -> Vec<String> {
    if chunk.is_empty() {
        return Vec::new();
    }
    let n = chunk.chars().count();

    // 收集词边（char 区间），丢弃 ≥4 字长词
    let edges: Vec<(usize, usize)> = jieba()
        .cut_for_search(chunk, false)
        .into_iter()
        .filter(|t| t.word.chars().count() < 4)
        .map(|t| (t.start, t.end))
        .collect();

    if edges.is_empty() {
        return jieba_cut_exact(chunk);
    }

    // DP 最少段数无缝覆盖 [0, n)
    let mut best: Vec<Option<(usize, usize)>> = vec![None; n + 1];
    best[0] = Some((0, usize::MAX));
    for i in 0..=n {
        let Some((seg, _)) = best[i] else { continue };
        for &(s, e) in &edges {
            if s == i && e <= n {
                let cand = seg + 1;
                if best[e].is_none_or(|(bs, _)| cand < bs) {
                    best[e] = Some((cand, i));
                }
            }
        }
    }

    let Some(_) = best[n] else {
        return jieba_cut_exact(chunk);
    };
    let mut pieces: Vec<(usize, usize)> = Vec::new();
    let mut cur = n;
    while cur != 0 {
        let (_, prev) = best[cur].unwrap();
        pieces.push((prev, cur));
        cur = prev;
    }
    pieces.reverse();
    let chars: Vec<char> = chunk.chars().collect();
    pieces
        .into_iter()
        .map(|(s, e)| chars[s..e].iter().collect::<String>().to_lowercase())
        .collect()
}

/// 索引侧分词：每块所有 1-3 字连续子串 + 整块本身
/// 单块最大字符数：超长的「无空白块」按此长度切段后再生成 n-gram。
///
/// 中文正文通常整段不打空格，一个 `split_whitespace()` 出来的块可能有几十万字。
/// 切段的实际收益是**控制内存与局部性**：字符边界表按块长度分配，
/// 一整段 33 万字（1MB 中文）需要一份 264 万条目的 `Vec<usize>`（约 21MB），
/// 切到 200 字后每块只有几十字节，峰值内存和缓存压力都降下来。
const MAX_CHUNK_CHARS: usize = 200;

/// 相邻切块的重叠字符数：保住跨块边界的 2-gram，尽量减少切点附近的漏检
const CHUNK_OVERLAP_CHARS: usize = 3;

/// 索引侧分词：每个空白块（超长的先切段）产出全部 1–3 字的连续子串。
///
/// ## 性能要点（旧实现的三个浪费，已改掉）
/// 1. 旧版对每个 gram 都 `chars[s..s+len].iter().collect::<String>()` + `to_lowercase()`，
///    再 `clone()` 进 `HashSet<String>` —— 1MB 中文约 100 万个 gram，就是 200 万次堆分配。
///    现在用**字节切片** `&str` 去重，只对首次出现的 gram 分配一次。
/// 2. 旧版把整个空白块也 push 成一个 token。中文几乎不打空格，等于把一整段话当一个词存进
///    索引；而查询侧对 >3 字的词一律拆成 bigram，**这个超长 token 永远不会被命中**，纯占空间。
/// 3. `Vec<char>` 全量收集换成字符边界表，省一次逐字分配。
pub fn segment_index(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // 借用 text 切片的去重集合：不产生额外分配
    let mut seen: HashSet<&str> = HashSet::new();

    for chunk in text.split_whitespace() {
        if chunk.is_empty() {
            continue;
        }
        for part in split_long_chunk(chunk) {
            push_ngrams(part, &mut out, &mut seen);
        }
    }
    out
}

/// 把超长块切成 `MAX_CHUNK_CHARS` 字以内的若干片（相邻片重叠几字）
fn split_long_chunk(chunk: &str) -> Vec<&str> {
    // UTF-8 下字节数不大于 MAX 时，字符数必然也不大于 MAX，可直接放行
    if chunk.len() <= MAX_CHUNK_CHARS {
        return vec![chunk];
    }
    let mut bounds: Vec<usize> = chunk.char_indices().map(|(i, _)| i).collect();
    bounds.push(chunk.len());
    let n = bounds.len() - 1;

    let mut out = Vec::new();
    let mut start = 0usize;
    loop {
        let end = (start + MAX_CHUNK_CHARS).min(n);
        out.push(&chunk[bounds[start]..bounds[end]]);
        if end >= n {
            break;
        }
        // 回退几字做重叠；MAX 远大于 OVERLAP，不会原地打转
        start = end - CHUNK_OVERLAP_CHARS;
    }
    out
}

/// 单块生成全部 1–3 字连续子串并写入 `out`
fn push_ngrams<'a>(chunk: &'a str, out: &mut Vec<String>, seen: &mut HashSet<&'a str>) {
    let mut bounds: Vec<usize> = Vec::with_capacity(64);
    bounds.extend(chunk.char_indices().map(|(i, _)| i));
    bounds.push(chunk.len());
    let n = bounds.len() - 1;

    for len in 1..=3.min(n) {
        for s in 0..=n - len {
            let piece = &chunk[bounds[s]..bounds[s + len]];
            if !seen.insert(piece) {
                continue; // 已收录，跳过（重复 gram 极多：中文常用词组反复出现）
            }
            out.push(if piece.is_ascii() {
                piece.to_ascii_lowercase()
            } else {
                piece.to_lowercase()
            });
        }
    }
}

/// 查询 token → term 列表：≤3 字原样；>3 字拆相邻 bigram
pub fn expand_token_to_terms(token: &str) -> Vec<String> {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() <= 3 {
        return vec![token.to_string()];
    }
    (0..chars.len() - 1)
        .map(|i| chars[i..i + 2].iter().collect::<String>())
        .collect()
}

/// 去重排序的查询 token 列表（供 search 层使用）
pub fn seg_query_tokens(text: &str) -> Vec<String> {
    let mut t = segment_query(text);
    t.sort();
    t.dedup();
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_3_1_segmentation() {
        //中华人民共和国 → 中华/人民/共和国
        // seg_query_tokens 语义 = 去重排序（Unicode 码点序：中华 < 人民 < 共和国）
        assert_eq!(
            seg_query_tokens("中华人民共和国"),
            vec!["中华", "人民", "共和国"]
        );
    }

    #[test]
    fn long_chunk_is_split_into_workable_blocks() {
        // 实测：7200 字连成一个空白块时整段静默丢失（索引里搜不到任何词）。
        // 切块后必须仍能产出正常粒度的 gram。
        let text = "系统运维管理平台数据档案".repeat(600); // 7200 字，无空格
        let toks = segment_index(&text);
        assert!(toks.contains(&"系统".to_string()), "长块仍须产出 2-gram");
        assert!(toks.contains(&"档".to_string()), "长块仍须产出单字");
        // 每块最多 MAX_CHUNK_CHARS 字 → 每块 gram 数有限
        assert!(toks.len() < 3 * 7200, "去重后 token 数应远小于原始 gram 数");
    }

    #[test]
    fn chunk_boundary_keeps_bigrams() {
        // 切块处有重叠，跨切点的 2-gram 不应被吞掉
        let text = "甲".repeat(MAX_CHUNK_CHARS) + "乙丙";
        let toks = segment_index(&text);
        assert!(toks.contains(&"甲乙".to_string()), "跨切点 2-gram 应保留");
    }

    #[test]
    fn index_side_has_single_chars() {
        // 索引侧必须有单字 / 双字 / 三字粒度（S3 单字查询命中依据）
        let toks = segment_index("中华人民共和国");
        assert!(toks.contains(&"共".to_string()));
        assert!(toks.contains(&"华人".to_string()));
        assert!(toks.contains(&"共和国".to_string()));
        // >3 字的整块不进索引：查询侧对 >3 字一律拆 bigram，
        // 长 token 永远命中不到，只会白占索引空间
        assert!(!toks.contains(&"中华人民共和国".to_string()));
    }

    #[test]
    fn expand_long_token_to_bigrams() {
        assert_eq!(expand_token_to_terms("中华"), vec!["中华"]);
        assert_eq!(
            expand_token_to_terms("华人民共"),
            vec!["华人", "人民", "民共"]
        );
    }

    #[test]
    fn english_chunk_passthrough() {
        assert_eq!(seg_query_tokens("hello world"), vec!["hello", "world"]);
    }
}
