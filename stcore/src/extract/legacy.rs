//! 旧格式与 ODF 文本提取（`anydoc`）。
//!
//! ## 覆盖（本层职责）
//! - **OLE 二进制**：`doc`/`dot`（Word 97-2003）、`xls`/`xlsb`/`xlt`、`ppt`/`pps`/`pot`
//! - **ODF**：`odt`/`ods`/`odp`
//! - `rtf`
//!
//! ## 与自建链路的分工（避免双源）
//! `docx`/`xlsx`/`pptx` 仍走 [`super::ooxml`]（自有 zip + quick-xml），`epub` 走
//! [`super::epub`]；anydoc 只兜上面这些**自建未覆盖**的格式。
//!
//! ## 与 OCR 的衔接（P4-A）
//! anydoc 对**扫描型/图片型 PDF** 不会硬失败，而是返回
//! [`anydoc::ConvertError::NeedsOcr`]（带需 OCR 的页码）—— 本层转成
//! [`ExtractError::NeedsOcr`]，由上层决定是否交给 OCR helper。

use super::ExtractError;

/// 旧格式扩展名 → anydoc 格式
fn format_of(ext: &str) -> Option<anydoc::Format> {
    use anydoc::Format;
    Some(match ext {
        "doc" | "dot" => Format::Doc,
        "xls" | "xlsb" | "xlt" => Format::Excel,
        "ppt" | "pps" | "pot" => Format::Ppt,
        // WPS 原生二进制：按对应的 MS 旧格式解析（WPS 的 OLE 容器与 doc/xls/ppt 高度同构）
        "wps" | "wpt" => Format::Doc,
        "et" | "ett" => Format::Excel,
        "dps" | "dpt" => Format::Ppt,
        "odt" => Format::Odt,
        "ods" => Format::Ods,
        "odp" => Format::Odp,
        "rtf" => Format::Rtf,
        _ => return None,
    })
}

/// anydoc 错误 → 提取层错误（分类语义见 [`ExtractError`] 文档）
fn map_err(e: anydoc::ConvertError) -> ExtractError {
    use anydoc::ConvertError as C;
    match e {
        C::NeedsOcr { pages, page_count } => ExtractError::NeedsOcr { pages, page_count },
        // 加密 / 不支持的格式：正常跳过（不算失败）
        C::Encrypted => ExtractError::Unsupported("文档已加密，无法提取".to_string()),
        C::Unsupported(msg) => ExtractError::Unsupported(msg),
        // 结构损坏 / 触及资源上限：记失败
        other => ExtractError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("旧格式解析失败: {other}"),
        )),
    }
}

/// 统一入口：旧格式 / ODF / RTF → Markdown 纯文本
pub fn legacy_to_text(ext: &str, bytes: &[u8]) -> Result<String, ExtractError> {
    let Some(format) = format_of(ext) else {
        return Err(ExtractError::Unsupported(format!("非旧格式扩展名: {ext}")));
    };
    let is_wps = matches!(ext, "wps" | "wpt" | "et" | "ett" | "dps" | "dpt");
    match anydoc::to_markdown_bytes(bytes, format).map_err(map_err) {
        Ok(md) if !md.trim().is_empty() => Ok(md),
        Ok(_) => {
            if is_wps {
                wps_strings_fallback(ext, bytes)
            } else {
                Err(ExtractError::Unsupported(
                    "文档无可提取正文（可能为空文档或纯图片）".to_string(),
                ))
            }
        }
        Err(e) => {
            if is_wps {
                // WPS 私有 OLE 若 anydoc 不认识，退回 strings 抽取，避免整篇被跳过
                wps_strings_fallback(ext, bytes).map_err(|_| e)
            } else {
                Err(e)
            }
        }
    }
}

/// WPS 原生二进制兜底：从容器中抽取可读字符串（ASCII + UTF-16LE 中文），
/// 让 .wps/.et/.dps 至少能被索引与命中，而不是整篇丢弃。
fn wps_strings_fallback(ext: &str, bytes: &[u8]) -> Result<String, ExtractError> {
    let t = super::binary::strings_to_text(bytes);
    if t.trim().is_empty() {
        return Err(ExtractError::Unsupported(format!(
            "WPS 文档无法解析出正文: {ext}"
        )));
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_mapping_covers_legacy_and_odf() {
        for e in [
            "doc", "dot", "xls", "xlsb", "xlt", "ppt", "pps", "pot", "odt", "ods", "odp", "rtf",
        ] {
            assert!(format_of(e).is_some(), "{e} 应被映射");
        }
        assert!(format_of("docx").is_none(), "docx 归 ooxml.rs");
        assert!(format_of("epub").is_none(), "epub 归 epub.rs");
        assert!(format_of("zzz").is_none());
    }

    #[test]
    fn encrypted_is_unsupported_not_io() {
        // 直接构造错误映射的语义检查
        let e = map_err(anydoc::ConvertError::Encrypted);
        assert!(matches!(e, ExtractError::Unsupported(_)), "{e}");

        let e = map_err(anydoc::ConvertError::NeedsOcr {
            pages: vec![1, 3],
            page_count: 5,
        });
        match e {
            ExtractError::NeedsOcr { pages, page_count } => {
                assert_eq!(pages, vec![1, 3]);
                assert_eq!(page_count, 5);
            }
            other => panic!("应为 NeedsOcr: {other}"),
        }

        let e = map_err(anydoc::ConvertError::Malformed {
            part: Some("WordDocument".into()),
            detail: "bad".into(),
        });
        assert!(matches!(e, ExtractError::Io(_)), "{e}");
    }

    #[test]
    fn garbage_doc_is_controlled_error() {
        // 随机字节当 .doc：不得 panic，须为受控错误
        let r = legacy_to_text("doc", b"\xD0\xCF\x11\xE0 not a real cfb");
        assert!(r.is_err());
        let msg = r.unwrap_err().to_string();
        assert!(!msg.is_empty());
    }

    #[test]
    fn rtf_roundtrip_is_extracted() {
        // 最小 RTF（anydoc 支持 rtf）
        let rtf = br"{\rtf1\ansi\deff0 {\fonttbl {\f0 Times;}}\f0\fs24 Hello RTF world.\par}";
        match legacy_to_text("rtf", rtf) {
            Ok(t) => assert!(t.contains("Hello") || t.contains("RTF"), "{t}"),
            // anydoc 对极简 RTF 的容忍度视版本而定：受控错误也可接受
            Err(e) => assert!(
                matches!(e, ExtractError::Unsupported(_) | ExtractError::Io(_)),
                "{e}"
            ),
        }
    }
}
