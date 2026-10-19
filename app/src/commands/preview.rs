//! 预览命令（子项 4 所有权：预览交互全家桶）
//!
//! 契约：ui/src/api/preview.ts
//!
//! 三段 offset 一致性：`match_groups` 产出的 offsets 同时驱动前端高亮、minimap 与匹配列表。

use tauri::State;

use crate::state::AppState;
use stcore::preview::{
    self, DocumentPayload, FileInfo, FindMatch, FindOptions, MatchGroup, WordStat,
};
use stcore::search::SearchMode;

/// 文档正文解析：普通文件走磁盘读+解码；压缩包内层条目（`包.zip#entry`）走索引 docstore
///
/// 判定顺序：**先查磁盘存在性**——合法文件名可能含 `#`（如 `a#b.txt`），
/// 旧实现只看 `#` 会把真实文件误判为虚拟条目（P2）。
fn load_text(path: &str, state: &State<'_, AppState>) -> Result<(String, Option<String>), String> {
    if !std::path::Path::new(path).is_file() && path.contains('#') {
        return state
            .with_mgr(|mgr| {
                mgr.doc_body_by_path(path)
                    .ok_or_else(|| anyhow::anyhow!("索引中无此条目"))
            })
            .map(|t| (t, Some("UTF-8".to_string())))
            .map_err(|e| e.to_string());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    // sjz 标签容器：与索引链路同口径，预览前先解密
    let bytes = match stcore::extract::sjz::probe(&bytes) {
        Some(n) => stcore::extract::sjz::decrypt(&bytes, n).map_err(|e| e.to_string())?,
        None => bytes,
    };
    let (text, enc) = stcore::text::decode_bytes(&bytes);
    Ok((text, Some(enc.to_string())))
}

/// 按模式提取用于高亮/匹配的词项（与前端 ui/src/util.ts 的 collectTerms 同口径）
///
/// 契约对齐要点（改动须同步 util.ts::collectTerms）：
///   - fuzzy：按空白切分，各词项独立
///   - exact：整体作为一个词项（首尾 trim）
///   - dsl：按 & | ( ) 切分 → trim → 丢弃空串与 `!` 取反项 → 仅当首尾成对引号时剥一层引号
pub fn terms_of(query: &str, mode: SearchMode) -> Vec<String> {
    match mode {
        SearchMode::Fuzzy => query
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect(),
        SearchMode::Exact => {
            let q = query.trim();
            if q.is_empty() {
                Vec::new()
            } else {
                vec![q.to_string()]
            }
        }
        SearchMode::Dsl => query
            .split(['&', '|', '(', ')'])
            .map(|t| t.trim())
            .filter(|t| !t.is_empty() && !t.starts_with('!'))
            .map(strip_quotes)
            .filter(|t| !t.is_empty())
            .collect(),
        // R3：正则模式无分词语义，整串作为单个词项（高亮按字面子串近似）
        SearchMode::Regex => {
            let q = query.trim();
            if q.is_empty() {
                Vec::new()
            } else {
                vec![q.to_string()]
            }
        }
    }
}

/// 剥掉成对的首尾引号（等价前端 `t.replace(/^"(.*)"$/, '$1')`）
fn strip_quotes(t: &str) -> String {
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

/// 按预览上限截断（UTF-8 字符边界安全）
///
/// 返回 `(截断后文本, 原始字节数, 是否截断)`：
///   - `total_bytes` 恒为**原始**字节数（前端据此显示文件真实体积与"已截断"提示）
///   - 文本截断到 `MAX_PREVIEW_BYTES` 且落在字符边界
fn truncate_preview(mut text: String) -> (String, usize, bool) {
    let total_bytes = text.len();
    let truncated = total_bytes > preview::MAX_PREVIEW_BYTES;
    if truncated {
        let mut cut = preview::MAX_PREVIEW_BYTES;
        while cut > 0 && !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
    (text, total_bytes, truncated)
}

/// 读文件全文（纯文本，供预览高亮渲染）；超 `MAX_PREVIEW_BYTES` 截断
#[tauri::command(async)]
pub fn get_preview(path: String, state: State<'_, AppState>) -> Result<String, String> {
    load_text(&path, &state).map(|(t, _)| truncate_preview(t).0)
}

/// 读文件全文 + 元信息（行数/字节数/截断标志）
#[tauri::command(async)]
pub fn get_document(path: String, state: State<'_, AppState>) -> Result<DocumentPayload, String> {
    let (text, _) = load_text(&path, &state)?;
    let (text, total_bytes, truncated) = truncate_preview(text);
    let total_lines = preview::count_lines(&text);
    Ok(DocumentPayload {
        path,
        text,
        total_lines,
        total_bytes,
        truncated,
    })
}

/// 词组统计（jieba 词频降序）；`min_len` 为最小字数（前端可传 2/3/4）
#[tauri::command(async)]
pub fn word_stats(
    path: String,
    top_n: Option<usize>,
    min_len: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<WordStat>, String> {
    let (text, _) = load_text(&path, &state)?;
    let (text, _, _) = truncate_preview(text);
    Ok(preview::word_stats_with(
        &text,
        top_n.unwrap_or(200),
        min_len.unwrap_or(2),
    ))
}

/// 匹配列表（按词项分组；offset 与高亮/minimap 同源）
#[tauri::command(async)]
pub fn match_groups(
    path: String,
    query: String,
    mode: SearchMode,
    state: State<'_, AppState>,
) -> Result<Vec<MatchGroup>, String> {
    let (text, _) = load_text(&path, &state)?;
    let (text, _, _) = truncate_preview(text);
    let terms = terms_of(&query, mode);
    Ok(preview::match_groups(&text, &terms))
}

/// 文档内再搜索（大小写 / 全词 / 正则）
#[tauri::command(async)]
pub fn find_in_doc(
    path: String,
    needle: String,
    opts: FindOptions,
    state: State<'_, AppState>,
) -> Result<Vec<FindMatch>, String> {
    let (text, _) = load_text(&path, &state)?;
    preview::find_in_doc(&text, &needle, &opts).map_err(|e| e.to_string())
}

/// 文件信息 Tab
#[tauri::command(async)]
pub fn get_file_info(path: String, state: State<'_, AppState>) -> Result<FileInfo, String> {
    let (text, enc) = load_text(&path, &state)?;
    let (text, _, _) = truncate_preview(text);
    Ok(preview::file_info(std::path::Path::new(&path), &text, enc))
}

/// 读取文件原始字节（供前端 WASM 文档渲染器使用；带 50MB 闸门）
#[tauri::command]
pub fn read_file_bytes(path: String) -> Result<Vec<u8>, String> {
    let meta = std::fs::metadata(&path).map_err(|e| format!("读取文件元信息失败: {e}"))?;
    if meta.len() > stcore::extract::MAX_FILE_SIZE {
        return Err(format!(
            "文件超过 {} MB 上限，文档视图已停用",
            stcore::extract::MAX_FILE_SIZE / 1024 / 1024
        ));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("读取文件失败: {e}"))?;
    // sjz 标签容器：Office 高保真视图读的是原始字节，先解密再交给前端解析
    match stcore::extract::sjz::probe(&bytes) {
        Some(n) => stcore::extract::sjz::decrypt(&bytes, n).map_err(|e| e.to_string()),
        None => Ok(bytes),
    }
}

/* -------------------- P3-B：PDF 页视图（渲染 + bbox） -------------------- */

/// 读取 PDF 原始字节（带大小上限，与提取层闸门同源）
fn read_pdf_bytes(path: &str) -> Result<Vec<u8>, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("读取文件元信息失败: {e}"))?;
    if meta.len() > stcore::extract::MAX_FILE_SIZE {
        return Err(format!(
            "PDF 超过 {} MB 上限，页视图已停用",
            stcore::extract::MAX_FILE_SIZE / 1024 / 1024
        ));
    }
    std::fs::read(path).map_err(|e| format!("读取 PDF 失败: {e}"))
}

/// PDF 页数（非 PDF / 无 PDFium 时返回错误，前端据此隐藏页视图）
#[tauri::command]
pub fn pdf_page_count(path: String) -> Result<usize, String> {
    let bytes = read_pdf_bytes(&path)?;
    stcore::extract::pdf::page_count(&bytes).map_err(|e| e.to_string())
}

/// 渲染 PDF 指定页为 PNG 字节（前端转 Blob URL 显示）
#[tauri::command(async)]
pub fn render_pdf_page(
    path: String,
    page_index: usize,
    target_width: Option<u32>,
) -> Result<Vec<u8>, String> {
    let bytes = read_pdf_bytes(&path)?;
    stcore::extract::pdf::render_page_png(&bytes, page_index, target_width.unwrap_or(800))
        .map_err(|e| e.to_string())
}

/// PDF 指定页的文本块（归一化 bbox，供命中框叠加）
///
/// 优先 PDF 文字层；**扫描型（无文字层）时回退 OCR**（需 helper + `SEARCH_TOOL_OCR=1`）。
#[tauri::command(async)]
pub fn pdf_page_boxes(
    path: String,
    page_index: usize,
) -> Result<Vec<stcore::extract::pdf::PageTextBox>, String> {
    let bytes = read_pdf_bytes(&path)?;
    match stcore::extract::pdf::page_text_boxes(&bytes, page_index) {
        Ok(b) if !b.is_empty() => Ok(b),
        Ok(_) | Err(_) => {
            // 无文字层（或读取失败）→ 尝试 OCR 通道
            if stcore::ocr::available() {
                let boxes = stcore::ocr::ocr_pdf_page_boxes(&bytes, page_index, 1000)
                    .map_err(|e| e.to_string())?;
                Ok(boxes
                    .into_iter()
                    .map(|b| stcore::extract::pdf::PageTextBox {
                        text: b.text,
                        x: b.x,
                        y: b.y,
                        w: b.w,
                        h: b.h,
                    })
                    .collect())
            } else {
                Err(
                    "该页无文字层；扫描件需 OCR（设 SEARCH_TOOL_OCR=1 并构建 ocr-helper）"
                        .to_string(),
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_of_matches_frontend_semantics() {
        // fuzzy：空白切分（对齐 util.ts collectTerms 的 / \s+/ 切分）
        assert_eq!(terms_of("中 华", SearchMode::Fuzzy), vec!["中", "华"]);
        assert_eq!(terms_of("  a   b  ", SearchMode::Fuzzy), vec!["a", "b"]);
        assert_eq!(terms_of("   ", SearchMode::Fuzzy), Vec::<String>::new());
        // exact：整体一词（首尾 trim）
        assert_eq!(terms_of(" 中华 ", SearchMode::Exact), vec!["中华"]);
        assert_eq!(terms_of("   ", SearchMode::Exact), Vec::<String>::new());
        // dsl：剥运算符与 ! 取反项，成对引号剥一层
        assert_eq!(
            terms_of("test & (hello | \"this is\") !that", SearchMode::Dsl),
            vec!["test", "hello", "this is"]
        );
        assert_eq!(
            terms_of("a!b & !c | d", SearchMode::Dsl),
            vec!["a!b", "d"],
            "token 内含 ! 不算取反；仅以 ! 开头才丢弃"
        );
        assert_eq!(
            terms_of("!only", SearchMode::Dsl),
            Vec::<String>::new(),
            "全为取反项时无正向词项"
        );
        // 非成对引号不剥（对齐 /^\"(.*)\"$/ 的锚定语义）
        assert_eq!(
            terms_of("\"unbalanced", SearchMode::Dsl),
            vec!["\"unbalanced"]
        );
    }

    #[test]
    fn truncate_preview_keeps_original_size() {
        // 未超限：原样返回
        let (t, total, trunc) = truncate_preview("hello".to_string());
        assert_eq!(t, "hello");
        assert_eq!(total, 5);
        assert!(!trunc);

        // 超限：文本截断到上限，total_bytes 仍为原始字节数
        let big = "a".repeat(preview::MAX_PREVIEW_BYTES + 32);
        let (t, total, trunc) = truncate_preview(big);
        assert!(trunc);
        assert_eq!(t.len(), preview::MAX_PREVIEW_BYTES);
        assert_eq!(total, preview::MAX_PREVIEW_BYTES + 32);
    }
}
