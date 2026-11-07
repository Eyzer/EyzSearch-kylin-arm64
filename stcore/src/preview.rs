//! 预览层（子项 4 所有权）：文档载荷 / 词组统计 / 匹配列表 / 文档内再搜索 / 文件信息
//!
//! 三段 offset 一致性铁律（任务卡 4.2/4.4）：
//!   高亮、minimap、匹配列表必须使用**同一份** `MatchItem` 列表（本模块产出）。
//!   所有 offset 均为 UTF-8 字节偏移，前端按同一口径切片。
//!
//! 本文件由集成负责人搭骨架（可编译可跑的基线实现），子项 4 agent 在其上补齐：
//!   - `match_groups` 升级 aho-corasick 单遍扫描（任务卡 4.4；当前为暴力 indexOf 等价实现）
//!   - 大文档（>1MB）渲染策略与截断提示（S9 门禁：三件套 <50ms）
//!   - `word_stats` 性能（S9 门禁：<200ms）与停用词/长度过滤调优

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use aho_corasick::{AhoCorasickBuilder, MatchKind};
use serde::{Deserialize, Serialize};

/// 预览最大载入字节（超过则截断并置 truncated=true）
pub const MAX_PREVIEW_BYTES: usize = 8 * 1024 * 1024;

/// 单次匹配列表最多返回的条目数（防前端渲染爆炸）
pub const MAX_MATCH_ITEMS: usize = 2000;

/* ------------------------------ 载荷类型 ------------------------------ */

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentPayload {
    pub path: String,
    pub text: String,
    pub total_lines: usize,
    pub total_bytes: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WordStat {
    pub word: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MatchItem {
    /// 1-based 行号
    pub line: usize,
    /// 行内起始字节偏移
    pub start: usize,
    /// 行内结束字节偏移
    pub end: usize,
    /// 所在行完整文本
    pub line_text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MatchGroup {
    pub term: String,
    pub count: usize,
    pub matches: Vec<MatchItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub mtime: i64,
    pub ext: String,
    pub kind: String,
    pub lines: usize,
    pub encoding: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FindMatch {
    /// 全文起始字节偏移
    pub start: usize,
    pub end: usize,
    /// 1-based 行号
    pub line: usize,
}

/* ------------------------------ 文本工具 ------------------------------ */

/// 字节偏移 → 1-based 行号
pub fn line_of(text: &str, byte_offset: usize) -> usize {
    text.as_bytes()[..byte_offset.min(text.len())]
        .iter()
        .filter(|b| **b == b'\n')
        .count()
        + 1
}

/// 全文行数（空文本记 1 行）
pub fn count_lines(text: &str) -> usize {
    if text.is_empty() {
        1
    } else {
        text.as_bytes().iter().filter(|b| **b == b'\n').count() + 1
    }
}

/// 全文各行行首的字节偏移（line_starts[0] 恒为 0）
///
/// 单遍 O(n) 预计算，供匹配列表/偏移换算以 O(log n) 定位行号，
/// 避免对大文档做逐条命中的线性扫描。
fn line_starts(text: &str) -> Vec<usize> {
    let mut v = vec![0usize];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            v.push(i + 1);
        }
    }
    v
}

/// 按预览上限截断（UTF-8 字符边界安全）——`word_stats` / `match_groups` /
/// `find_in_doc` 统一先过本函数再分析，与 `get_document` 的
/// `MAX_PREVIEW_BYTES` 口径一致（app 层另有一份同语义实现，
/// 供 DocumentPayload 上报原始字节数）。
pub fn truncate_preview(text: &str) -> &str {
    if text.len() <= MAX_PREVIEW_BYTES {
        return text;
    }
    let mut cut = MAX_PREVIEW_BYTES;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    &text[..cut]
}

/// 字节偏移 → 1-based 行号（基于预计算的行首表）
fn line_of_offset(starts: &[usize], off: usize) -> usize {
    starts.partition_point(|&s| s <= off)
}

/// 字节偏移所在行的行首/行尾（不含换行符），基于预计算的行首表
fn line_bounds_at(starts: &[usize], text: &str, off: usize) -> (usize, usize) {
    let li = line_of_offset(starts, off);
    let ls = starts[li - 1];
    let le = if li < starts.len() {
        starts[li] - 1
    } else {
        text.len()
    };
    (ls, le)
}

/* ---------------------------- 词组统计 ---------------------------- */

fn jieba() -> &'static jieba_rs::Jieba {
    static J: OnceLock<jieba_rs::Jieba> = OnceLock::new();
    J.get_or_init(jieba_rs::Jieba::new)
}

/// 常见中文虚词/停用词：词组统计时过滤，避免"的/了/在"等高频虚词霸榜
const STOPWORDS: &[&str] = &[
    "的", "了", "在", "是", "和", "与", "有", "也", "就", "都", "而", "及", "或", "于", "其", "这",
    "那", "我", "你", "他", "她", "它", "们", "并", "被", "把", "对", "从", "为", "以", "之", "等",
    "一个", "我们", "你们", "他们", "以及", "但是", "因为", "所以", "如果", "没有", "可以", "这个",
    "那个", "这些", "那些", "一种", "进行", "通过", "由于", "于是", "不是",
];

/// 词组统计（jieba 精确模式词频，降序；默认过滤单字与纯标点）——签名保持不变
pub fn word_stats(text: &str, top_n: usize) -> Vec<WordStat> {
    word_stats_with(text, top_n, 2)
}

/// 词组统计参数化版本：`min_len` 为最小字数，附带停用词过滤（S9：1MB <200ms）
///
/// 超大输入先过 `truncate_preview` 截断再分析（与 `get_document` 同口径）。
pub fn word_stats_with(text: &str, top_n: usize, min_len: usize) -> Vec<WordStat> {
    let text = truncate_preview(text);
    let mut freq: HashMap<String, usize> = HashMap::new();
    for tok in jieba().cut(text, false) {
        let w = tok.word.trim();
        // 过滤空白/短词/纯标点/停用词
        if w.chars().count() < min_len
            || w.chars().all(|c: char| !c.is_alphanumeric())
            || STOPWORDS.contains(&w)
        {
            continue;
        }
        *freq.entry(w.to_string()).or_insert(0) += 1;
    }
    let mut v: Vec<WordStat> = freq
        .into_iter()
        .map(|(word, count)| WordStat { word, count })
        .collect();
    // 频次降序，同频按词面升序（稳定输出，便于测试与快照）
    v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.word.cmp(&b.word)));
    v.truncate(top_n);
    v
}

/* ---------------------------- 匹配列表 ---------------------------- */

/// 按词项分组产出匹配列表（offset 为**行内**字节偏移，与前端高亮切片同口径）
///
/// 单遍扫描：用 aho-corasick 自动机同时匹配全部词项（ASCII 大小写不敏感），
/// 避免按词项逐个 `find` 在词项数增多时退化（任务卡 4.4）。
/// 每个词项仍按"从左到右、非重叠"的语义收集（与旧实现一致）。
///
/// 含换行符的词项被**拒绝**（跳过）：AC 跨行命中会使行内 `end` 超出行长度，
/// 破坏 offset 铁律（与 `find_in_doc` 的换行拒绝同口径）。
/// `count` 由独立计数器统计，不受 `MAX_MATCH_ITEMS` 收集上限截断影响。
/// 超大文本先过 `truncate_preview` 截断再分析（与 `get_document` 同口径）。
pub fn match_groups(text: &str, terms: &[String]) -> Vec<MatchGroup> {
    let text = truncate_preview(text);
    // 收集非空、无换行词项（保序）
    let pats: Vec<&String> = terms
        .iter()
        .filter(|t| !t.is_empty() && !t.contains('\n'))
        .collect();
    if pats.is_empty() {
        return Vec::new();
    }
    let ac = match AhoCorasickBuilder::new()
        .ascii_case_insensitive(true)
        .match_kind(MatchKind::Standard)
        .build(pats.iter().copied())
    {
        Ok(ac) => ac,
        Err(_) => return Vec::new(),
    };

    // 每词项一个分组（保持入参顺序），count 稍后回填
    let mut groups: Vec<MatchGroup> = pats
        .iter()
        .map(|t| MatchGroup {
            term: (*t).clone(),
            count: 0,
            matches: Vec::new(),
        })
        .collect();
    // 各词项"上一处命中的行尾"，用于复现逐词项非重叠扫描语义
    let mut last_end = vec![0usize; pats.len()];
    // 独立计数器：count 不受 matches 收集上限（MAX_MATCH_ITEMS）影响，防低报
    let mut counts = vec![0usize; pats.len()];
    let starts = line_starts(text);

    for m in ac.find_overlapping_iter(text) {
        let p = m.pattern().as_usize();
        if m.start() < last_end[p] {
            // 与同词项上一处命中重叠，跳过
            continue;
        }
        last_end[p] = m.end();
        counts[p] += 1;
        if groups[p].matches.len() >= MAX_MATCH_ITEMS {
            continue;
        }
        let s = m.start();
        let e = m.end();
        let (ls, le) = line_bounds_at(&starts, text, s);
        groups[p].matches.push(MatchItem {
            line: line_of_offset(&starts, s),
            start: s - ls,
            end: e - ls,
            line_text: text[ls..le].to_string(),
        });
    }

    for (g, c) in groups.iter_mut().zip(counts) {
        g.count = c;
    }
    groups.retain(|g| !g.matches.is_empty());
    groups
}

/* -------------------------- 文档内再搜索 -------------------------- */

/// 文档内再搜索（大小写 / 全词 / 正则三选项）
///
/// 大小写不敏感口径：**仅折叠 ASCII**（A-Z→a-z），haystack 与原文等长、
/// 同字节坐标系，偏移可直接切回原文。禁用 `to_lowercase()`：İ/K/ẞ 等字符的
/// Unicode 小写会改变字节长度，haystack 偏移映射回原文会 panic 或错位（P0）。
///
/// 词项含换行符 → 拒绝（跨行命中会使行内 end 超出行长度，破坏 offset 铁律）。
/// 超大输入先过 `truncate_preview` 截断再分析（与 `get_document` 同口径）。
pub fn find_in_doc(text: &str, needle: &str, opts: &FindOptions) -> anyhow::Result<Vec<FindMatch>> {
    let text = truncate_preview(text);
    if needle.is_empty() {
        return Ok(Vec::new());
    }
    if needle.contains('\n') {
        return Err(anyhow::anyhow!("词项不能包含换行"));
    }
    let mut out = Vec::new();

    if opts.regex {
        // whole_word 在正则模式下自动包 \b...\b（已带边界则不重复包），
        // 避免选项被静默忽略
        let pattern = if opts.whole_word {
            wrap_word_boundaries(needle)
        } else {
            needle.to_string()
        };
        let mut b = regex::RegexBuilder::new(&pattern);
        b.case_insensitive(!opts.case_sensitive);
        let re = b
            .build()
            .map_err(|e| anyhow::anyhow!("正则表达式无效: {e}"))?;
        for m in re.find_iter(text) {
            out.push(FindMatch {
                start: m.start(),
                end: m.end(),
                line: line_of(text, m.start()),
            });
        }
        return Ok(out);
    }

    // ASCII 折叠：hay 与 text 等长同坐标系（见函数文档 P0 说明）
    let hay_ci;
    let ndl_ci;
    let (hay, ndl): (&str, &str) = if opts.case_sensitive {
        (text, needle)
    } else {
        hay_ci = text.to_ascii_lowercase();
        ndl_ci = needle.to_ascii_lowercase();
        (&hay_ci, &ndl_ci)
    };

    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut from = 0usize;
    while let Some(idx) = hay[from..].find(ndl) {
        let abs = from + idx;
        let end = abs + ndl.len();
        // 全词：两侧不得为词字符（基于 hay，与 text 同坐标系）
        if opts.whole_word {
            let left_ok = hay[..abs]
                .chars()
                .next_back()
                .map(|c| !is_word(c))
                .unwrap_or(true);
            let right_ok = hay[end..]
                .chars()
                .next()
                .map(|c| !is_word(c))
                .unwrap_or(true);
            if !(left_ok && right_ok) {
                from = abs + ndl.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                continue;
            }
        }
        out.push(FindMatch {
            start: abs,
            end,
            line: line_of(text, abs),
        });
        from = end.max(abs + 1);
    }
    Ok(out)
}

/// whole_word + regex：给 pattern 包 `\b` 词边界（已带则不重复包）
fn wrap_word_boundaries(p: &str) -> String {
    let left = p.starts_with(r"\b");
    let right = p.ends_with(r"\b");
    match (left, right) {
        (true, true) => p.to_string(),
        (true, false) => format!(r"{p}\b"),
        (false, true) => format!(r"\b{p}"),
        (false, false) => format!(r"\b{p}\b"),
    }
}

/* ---------------------------- 文件信息 ---------------------------- */

/// 文件信息 Tab（大小/路径/修改时间/格式/行数/编码）
pub fn file_info(path: &Path, text: &str, encoding: Option<String>) -> FileInfo {
    let meta = std::fs::metadata(path).ok();
    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let mtime = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    FileInfo {
        name,
        path: path.to_string_lossy().to_string(),
        size,
        mtime,
        ext: ext.clone(),
        kind: kind_of(&ext),
        lines: count_lines(text),
        encoding,
    }
}

/// 扩展名 → 人类可读类型描述
pub fn kind_of(ext: &str) -> String {
    match ext {
        "txt" | "text" | "log" => "纯文本",
        "md" | "markdown" => "Markdown 文档",
        "html" | "htm" | "xhtml" => "网页",
        "eml" => "电子邮件",
        "pdf" => "PDF 文档",
        "doc" | "docx" | "wps" | "wpt" => "Word 文档",
        "xls" | "xlsx" | "et" | "ett" | "csv" | "tsv" => "表格",
        "ppt" | "pptx" | "dps" | "dpt" => "演示文稿",
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "7z" | "rar" => "压缩包",
        "epub" | "mobi" | "azw3" | "fb2" | "chm" | "djvu" => "电子书",
        "" => "无扩展名",
        _ => "其他文件",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_math() {
        let t = "a\nbb\nccc";
        assert_eq!(count_lines(t), 3);
        assert_eq!(line_of(t, 0), 1);
        assert_eq!(line_of(t, 2), 2);
        assert_eq!(line_of(t, 5), 3);
    }

    #[test]
    fn word_stats_filters_single_chars() {
        let t = "中华人民共和国 中华人民共和国 人民 生活";
        let s = word_stats(t, 10);
        assert!(!s.is_empty());
        assert!(s.iter().all(|w| w.word.chars().count() >= 2));
        assert_eq!(s[0].word, "中华人民共和国");
        assert_eq!(s[0].count, 2);
    }

    #[test]
    fn match_groups_offsets_are_line_local() {
        let t = "第一行 hello\n第二行 hello world\n";
        let g = match_groups(t, &["hello".to_string()]);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].count, 2);
        assert_eq!(g[0].matches[0].line, 1);
        assert_eq!(g[0].matches[1].line, 2);
        assert_eq!(g[0].matches[1].line_text, "第二行 hello world");
        // 行内偏移：第二行 "第二行 " 占 4 个汉字 *3 + 1 空格 = 13 字节
        assert_eq!(g[0].matches[1].start, "第二行 ".len());
    }

    #[test]
    fn find_in_doc_ascii_fold_keeps_byte_offsets() {
        // P0 回归：İ(U+0130) 的 Unicode 小写是 2 字符（i + 组合点），
        // to_lowercase 会产生更长 haystack，偏移映射回原文即 panic/错位；
        // 修复后只折叠 ASCII，hay 与 text 等长同坐标系。
        let t = "İİİ hello WORLD";
        let hits = find_in_doc(t, "world", &FindOptions::default()).unwrap();
        assert_eq!(hits.len(), 1);
        let m = &hits[0];
        assert_eq!(&t[m.start..m.end], "WORLD");
        assert_eq!(m.line, 1);
        // 大小写敏感不受影响
        let cs = find_in_doc(
            t,
            "WORLD",
            &FindOptions {
                case_sensitive: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(&t[cs[0].start..cs[0].end], "WORLD");
    }

    #[test]
    fn find_in_doc_regex_whole_word_wraps_boundaries() {
        let t = "hello world hello-world";
        let opts = FindOptions {
            regex: true,
            whole_word: true,
            ..Default::default()
        };
        // hell 不是独立词（hello / shell 内均为子串）→ 0 命中（旧实现静默忽略 whole_word）
        assert!(find_in_doc(t, "hell", &opts).unwrap().is_empty());
        // hello 独立出现 2 处（hello-world 的 - 也是词边界）
        assert_eq!(find_in_doc(t, "hello", &opts).unwrap().len(), 2);
        // 自带 \b 不重复包装，行为一致
        assert_eq!(find_in_doc(t, r"\bhello\b", &opts).unwrap().len(), 2);
    }

    #[test]
    fn find_in_doc_rejects_newline_term() {
        // 跨行词项会破坏行内 offset 铁律 → 拒绝
        let r = find_in_doc("ab\ncd", "b\nc", &FindOptions::default());
        assert!(r.is_err());
        assert!(r.unwrap_err().to_string().contains("换行"));
    }

    #[test]
    fn find_in_doc_truncates_oversize_input() {
        // 超过 MAX_PREVIEW_BYTES 的部分不参与匹配（与 get_document 同口径）
        let mut big = "a".repeat(MAX_PREVIEW_BYTES + 16);
        big.push_str("NEEDLE");
        assert!(find_in_doc(&big, "NEEDLE", &FindOptions::default())
            .unwrap()
            .is_empty());
        let small = format!("{}NEEDLE", "a".repeat(64));
        assert_eq!(
            find_in_doc(&small, "NEEDLE", &FindOptions::default())
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn find_in_doc_options() {
        let t = "Hello hello HELLO world";
        let ci = find_in_doc(t, "hello", &FindOptions::default()).unwrap();
        assert_eq!(ci.len(), 3);
        let cs = find_in_doc(
            t,
            "hello",
            &FindOptions {
                case_sensitive: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(cs.len(), 1);
        let whole = find_in_doc(
            t,
            "hell",
            &FindOptions {
                whole_word: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(whole.is_empty());
        let re = find_in_doc(
            t,
            r"h\w+o",
            &FindOptions {
                regex: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(re.len(), 3);
    }

    #[test]
    fn find_in_doc_bad_regex_errors() {
        assert!(find_in_doc(
            "abc",
            "(",
            &FindOptions {
                regex: true,
                ..Default::default()
            }
        )
        .is_err());
    }

    #[test]
    fn file_info_basic() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.md");
        std::fs::write(&p, "# 标题\n正文").unwrap();
        let info = file_info(&p, "# 标题\n正文", Some("UTF-8".into()));
        assert_eq!(info.name, "a.md");
        assert_eq!(info.ext, "md");
        assert_eq!(info.kind, "Markdown 文档");
        assert_eq!(info.lines, 2);
        assert_eq!(info.encoding.as_deref(), Some("UTF-8"));
        assert!(info.size > 0);
    }

    #[test]
    fn kind_of_unknown() {
        assert_eq!(kind_of("xyz"), "其他文件");
        assert_eq!(kind_of(""), "无扩展名");
    }

    #[test]
    fn match_groups_multi_term_single_pass() {
        let t = "hello world\nfoo hello bar\nbaz world";
        let g = match_groups(t, &["hello".into(), "world".into()]);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].term, "hello");
        assert_eq!(g[0].count, 2);
        assert_eq!(g[1].term, "world");
        assert_eq!(g[1].count, 2);
        // 每组 offset 均为行内偏移
        assert_eq!(g[0].matches[0].line, 1);
        assert_eq!(g[0].matches[1].line, 2);
        assert_eq!(g[0].matches[1].start, "foo ".len());
        assert_eq!(g[1].matches[1].line, 3);
        assert_eq!(g[1].matches[1].start, "baz ".len());
    }

    #[test]
    fn match_groups_ascii_case_insensitive() {
        let t = "Hello HELLO hello";
        let g = match_groups(t, &["hello".into()]);
        assert_eq!(g[0].count, 3);
    }

    #[test]
    fn match_groups_non_overlapping_per_term() {
        // 同一词项重叠命中按从左到右非重叠收集（aaaa 中 aa 命中 2 处）
        let g = match_groups("aaaa", &["aa".into()]);
        assert_eq!(g[0].count, 2);
        assert_eq!(g[0].matches[0].start, 0);
        assert_eq!(g[0].matches[1].start, 2);
    }

    #[test]
    fn match_groups_skips_empty_terms() {
        let g = match_groups("abc", &["".into(), "b".into()]);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].term, "b");
    }

    #[test]
    fn match_groups_rejects_newline_terms() {
        // 含换行的词项跨行命中会破坏行内 offset 铁律 → 拒绝（跳过）
        let t = "ab\ncd";
        let g = match_groups(t, &["b\nc".to_string(), "ab".to_string()]);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].term, "ab");
    }

    #[test]
    fn match_groups_count_survives_collect_cap() {
        // count 用独立计数器：超出 MAX_MATCH_ITEMS 收集上限也不低报
        let t = "ab\n".repeat(MAX_MATCH_ITEMS + 10);
        let g = match_groups(&t, &["ab".to_string()]);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].count, MAX_MATCH_ITEMS + 10);
        assert_eq!(
            g[0].matches.len(),
            MAX_MATCH_ITEMS,
            "matches 仍受收集上限保护"
        );
    }

    #[test]
    fn truncate_preview_caps_and_stays_on_char_boundary() {
        assert_eq!(truncate_preview("hello"), "hello");
        // 中文 3 字节/字：截断点必须回退到字符边界
        let big = "中".repeat(MAX_PREVIEW_BYTES / 3 + 10);
        let t = truncate_preview(&big);
        assert!(t.len() <= MAX_PREVIEW_BYTES);
        assert!(t.chars().all(|c| c == '中'));
    }

    #[test]
    fn word_stats_with_filters_stopwords_and_len() {
        // 停用词"的"与 2 字词被 min_len=3 过滤
        let t = "图书馆 图书馆 数据 数据 数据 的 的 的";
        let s = word_stats_with(t, 10, 3);
        assert!(s.iter().all(|w| w.word.chars().count() >= 3));
        assert!(s.iter().all(|w| w.word != "的"));
        assert_eq!(s[0].word, "图书馆");
        assert_eq!(s[0].count, 2);
    }

    /// 宽松基准（S9 门禁：1MB 文本 + 3 词项 match_groups < 50ms）
    ///
    /// 默认 #[ignore]（debug 模式易 flaky）；用
    /// `cargo test -p stcore --release -- --ignored --nocapture` 跑真值。
    #[test]
    #[ignore = "基准测试，默认不计入单测"]
    fn bench_match_groups_1mb() {
        let unit = "中华人民共和国 中华苏维埃共和国 中云 台湾 高山族 图书馆 数据报告\n";
        let mut text = String::with_capacity(1 << 20);
        while text.len() < 1 << 20 {
            text.push_str(unit);
        }
        let terms = ["共同".to_string(), "中华".to_string(), "共和国".to_string()];
        let t0 = std::time::Instant::now();
        let g = match_groups(&text, &terms);
        let el = t0.elapsed();
        let total: usize = g.iter().map(|x| x.count).sum();
        println!(
            "match_groups 1MB({} bytes) hits={} elapsed={:?}",
            text.len(),
            total,
            el
        );
        assert!(el.as_millis() < 50, "match_groups 超时: {:?}", el);
    }
}
