//! 提取层统一入口：按文件类型分发到 html/eml/archive/文本解码器。
//!
//! 两个公开入口：
//! - [`extract_path`]：磁盘文件级。按路径扩展名分发；压缩包展开为 `children`
//!   （每条内层文本作为独立虚拟文档，由上层以 `path#entry` 寻址）
//! - [`extract_entry`]：内存条目级。压缩包内层条目用；**不展开嵌套压缩包**
//!   （递归展开是压缩炸弹的经典放大器，内层包记为 Unsupported 跳过）
//!
//! 格式覆盖（P0 纯文本链路 + P1 格式扩展 + R4 格式补齐）：
//! - 纯文本/代码（txt/log/md/json/rs/...）→ 编码检测解码
//! - html/htm/xhtml → 去 script/style、块级换行
//! - eml → 邮件头 + 正文（text/plain 优先，HTML 邮件降级转文本）
//! - zip/tar/tgz → 内层文本条目展开（炸弹防护三道闸门见 archive.rs）
//! - gz → 先按 tar.gz 尝试，失败按单文件 gzip 解压为文本
//! - bz2 → 先按 tar.bz2 尝试，失败按单文件 bzip2 解压为文本（与 gz 同语义）
//! - **7z → sevenz-rust2 展开（sevenz.rs，复用炸弹闸门与条目名净化）**
//! - **rar → unrar 官方库展开（rar.rs，文件级按路径打开）**
//! - **chm → chmlib 枚举内部对象（chm.rs，文件级按路径打开）**
//! - **exe/dll/so/dylib/bin → strings 提取（binary.rs：ASCII ≥5 + UTF-16LE，256KB 截断）**
//! - **docx/docm/xlsx/xlsm/xlsb/pptx/... → OOXML 抽取（ooxml.rs，zip + quick-xml）**
//! - **pdf → 文字层抽取（pdf.rs，pdf-extract）；扫描型需 OCR，属后续批次**
//! - **epub → zip + XHTML 正文（epub.rs，按 spine 顺序）**
//!
//! 格式覆盖矩阵（声明 = 能力）：
//! | 类型 | 通道 |
//! |------|------|
//! | txt/md/json/代码... | 编码检测解码 |
//! | html/htm/xhtml | html.rs |
//! | eml | eml.rs |
//! | zip/tar/tgz/gz/bz2 | archive.rs + mod.rs 双层语义 |
//! | 7z | sevenz.rs |
//! | rar | rar.rs |
//! | chm | chm.rs |
//! | docx/xlsx/pptx/... | ooxml.rs |
//! | pdf | pdf.rs（+ OCR 救助） |
//! | epub | epub.rs |
//! | mobi/azw3/fb2/lit/kfx/cbz | ebook.rs |
//! | doc/xls/ppt/odt/ods/odp/rtf | legacy.rs（anydoc） |
//! | 十进制.exe 标签容器（.SJZ，扩展名不变） | sjz.rs（透明解密后按原扩展名分发） |
//! | png/jpg/... | ocr.rs（默认关） |
//! | exe/dll/so/dylib/bin | binary.rs |
//! | djvu | **缺口**：djvu-rs 可编译但无文本层抽取，类型表暂保留声明（见 R4 报告） |
//! | wps/et/dps、eid/eis/eip | **缺口**：anydoc 0.2.4 不支持，类型表不声明 |
//!
//! 分层职责：提取层"能提就提"（白名单宽于扫描层），索引哪些文件由
//! 扫描层（app/src/scanner.rs，读 `Config::all_exts()`）决定。

pub mod archive;
pub mod binary;
pub mod chm;
pub mod ebook;
pub mod eml;
pub mod epub;
pub mod html;
pub mod legacy;
pub mod ooxml;
pub mod pdf;
pub mod rar;
pub mod sevenz;
pub mod sjz;

use std::io::Read;
use std::path::Path;

use crate::text::decode_bytes;

/// 单文件字节上限（提取层第一道闸门；与扫描层共用同一常量避免双源）。
pub const MAX_FILE_SIZE: u64 = 50 * 1024 * 1024;

/// 纯文本/代码类扩展名（提取层收录；扫描层可在此基础上再收窄）。
const TEXT_EXTS: &[&str] = &[
    // 数据/配置
    "txt",
    "text",
    "log",
    "csv",
    "tsv",
    "json",
    "xml",
    "yaml",
    "yml",
    "toml",
    "ini",
    "conf",
    "cfg",
    "properties",
    "env",
    "md",
    "markdown",
    "rst",
    "org",
    // 代码
    "rs",
    "py",
    "js",
    "jsx",
    "ts",
    "tsx",
    "mjs",
    "cjs",
    "java",
    "kt",
    "kts",
    "go",
    "c",
    "h",
    "cpp",
    "cc",
    "cxx",
    "hpp",
    "hh",
    "css",
    "scss",
    "sass",
    "less",
    "sql",
    "sh",
    "bash",
    "zsh",
    "fish",
    "bat",
    "cmd",
    "ps1",
    "rb",
    "php",
    "pl",
    "lua",
    "r",
    "dart",
    "swift",
    "m",
    "mm",
    "scala",
    "clj",
    "ex",
    "exs",
    "erl",
    "hrl",
    "hs",
    "ml",
    "fs",
    "cs",
    "vb",
    "groovy",
    "gradle",
    "vue",
    "svelte",
    "astro",
    "proto",
    "graphql",
    "gql",
    "tf",
    "hcl",
    "cmake",
    "mk",
    "sbt",
    "bib",
    "tex",
    "sty",
    "cls",
    "adoc",
    "textile",
    "wiki",
    "po",
    "pot",
    "srt",
    "vtt",
    "ass",
    "ics",
    "vcf",
    "svg",
];

/// HTML 类扩展名（走 scraper 转换）。
const HTML_EXTS: &[&str] = &["html", "htm", "xhtml"];

/// OOXML 类扩展名（zip 容器 + XML，走 ooxml.rs）
const OOXML_EXTS: &[&str] = &[
    "docx", "docm", "xlsx", "xlsm", "xlsb", "pptx", "pptm", "pps", "ppsx", "pot", "potx",
];

/// PDF 类扩展名（文字层提取，走 pdf.rs）
const PDF_EXTS: &[&str] = &["pdf"];

/// 电子书类扩展名（走 epub.rs）
const EPUB_EXTS: &[&str] = &["epub"];

/// 其它电子书扩展名（走 ebook.rs / ebook-rs：mobi/azw3/fb2/lit/kfx/cbz/rtf/odt）
///
/// 注意：epub 由自有 `epub.rs` 处理、docx 由 `ooxml.rs` 处理，
/// 不在此列（避免两条链路对同一格式给出不同结果）。
const EBOOK_EXTS: &[&str] = &[
    "mobi", "azw", "azw3", "fb2", "lit", "kfx", "cbz", "rtf", "odt",
];

/// 图片类扩展名（走 OCR 通道 ocr.rs；默认关闭，见 `SEARCH_TOOL_OCR`）
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "tiff", "tif", "gif"];

/// 二进制可执行/库文件（走 binary.rs strings 提取）。
///
/// 不在 [`TEXT_EXTS`] 内（无交集），判定顺序无冲突。
const BINARY_EXTS: &[&str] = &["exe", "dll", "so", "dylib", "bin"];

/// 旧格式 / ODF（走 legacy.rs / anydoc）：OLE 二进制与 ODF、RTF。
///
/// 与 [`EBOOK_EXTS`] 的交集（rtf/odt）：dispatch 顺序上 legacy 分支在前，
/// 故 rtf/odt 归 anydoc（块级结构还原更完整）。
const LEGACY_EXTS: &[&str] = &[
    "doc", "dot", "xls", "xlsb", "xlt", "ppt", "pps", "pot", "odt", "ods", "odp", "rtf",
    // WPS 原生二进制（OLE 复合文档，多与 MS 旧格式同构）
    "wps", "wpt", "et", "ett", "dps", "dpt",
];

/// 提取错误：覆盖"读不了/不支持/超限/需 OCR"四类失败。
///
/// 调用方语义约定：
/// - `Unsupported` 是正常跳过（不算失败）
/// - `Io` / `TooManyEntries` 记失败
/// - `NeedsOcr` 是**可救助**的：交给 OCR 通道（P4-A）后可获得文本
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("不支持的文件类型: {0}")]
    Unsupported(String),
    #[error("压缩包条目数或解压总量超限")]
    TooManyEntries,
    #[error("需要 OCR（{pages:?} / 共 {page_count} 页）")]
    NeedsOcr {
        /// 需 OCR 的页码（1 起，来自 anydoc）
        pages: Vec<u32>,
        /// 文档总页数
        page_count: u32,
    },
}

/// 单个提取条目（压缩包内层文件）。
#[derive(Debug, Clone)]
pub struct ExtractedEntry {
    /// 条目名：压缩包内层路径（已归一为 '/'，无开头 "./"）
    pub name: String,
    /// 提取出的纯文本
    pub text: String,
    /// 格式标签（小写扩展名，如 "txt"/"html"/"eml"/"rs"）
    pub format: &'static str,
}

/// 文件级提取结果。
///
/// - 普通文件：`text` 为全文，`children` 为空
/// - 压缩包：`text` 为空，`children` 为内层可索引条目（按 name 排序）
/// - 单文件 .gz：`text` 为解压后全文，`children` 为空
#[derive(Debug, Clone)]
pub struct ExtractResult {
    /// 文件名（不含目录）
    pub name: String,
    /// 格式标签（小写扩展名，压缩包为 "zip"/"tar"/"tgz"/"gz"/"tbz"/"7z"/"rar"/"chm"）
    pub format: &'static str,
    /// 文件自身提取出的文本（压缩包为空）
    pub text: String,
    /// 压缩包内层条目（普通文件为空）
    pub children: Vec<ExtractedEntry>,
}

/// 提取磁盘文件（按扩展名分发；压缩包展开）。
pub fn extract_path(path: &Path) -> Result<ExtractResult, ExtractError> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let ext = extension_of(path);

    // 大小闸门放在读盘前：超大文件连内存都不进
    let len = std::fs::metadata(path)?.len();
    if len > MAX_FILE_SIZE {
        return Err(ExtractError::Unsupported(format!(
            "文件超过 {MAX_FILE_SIZE} 字节上限: {}",
            path.display()
        )));
    }
    let bytes = std::fs::read(path)?;

    dispatch(&name, &ext, &bytes, path)
}

/// 提取内存条目（压缩包内层用；不展开嵌套压缩包）。
///
/// `name` 仅用于取扩展名与回填条目名，内容一律取 `bytes`。
pub fn extract_entry(name: &str, bytes: &[u8]) -> Result<ExtractedEntry, ExtractError> {
    let ext = extension_of(Path::new(name));
    // sjz 标签容器：文档被 十进制.exe 加密后扩展名不变，按内容探测并就地解密
    let unboxed = match sjz::probe(bytes) {
        Some(n) => Some(sjz::decrypt(bytes, n)?),
        None => None,
    };
    let bytes = unboxed.as_deref().unwrap_or(bytes);
    let text = match ext.as_str() {
        e if HTML_EXTS.contains(&e) => html::html_to_text(&decode_bytes(bytes).0),
        "eml" => eml::eml_to_text(bytes),
        e if OOXML_EXTS.contains(&e) => ooxml::to_text(e, bytes)?,
        e if PDF_EXTS.contains(&e) => match pdf::pdf_to_text(bytes) {
            Ok(t) => t,
            // 扫描型/图片型 PDF（无文字层）→ OCR 通道可用则接管（P4-A）
            Err(ExtractError::NeedsOcr { .. }) if crate::ocr::available() => {
                crate::ocr::ocr_pdf_bytes(bytes)?
            }
            Err(err) => return Err(err),
        },
        e if IMAGE_EXTS.contains(&e) => {
            if crate::ocr::available() {
                crate::ocr::ocr_image_bytes(bytes, e)?
            } else {
                return Err(ExtractError::Unsupported(format!(
                    "图片需 OCR（helper 未就绪，或未设 SEARCH_TOOL_OCR=1）: {name}"
                )));
            }
        }
        e if EPUB_EXTS.contains(&e) => epub::epub_to_text(bytes)?,
        e if LEGACY_EXTS.contains(&e) => legacy::legacy_to_text(e, bytes)?,
        e if EBOOK_EXTS.contains(&e) => ebook::ebook_to_text(bytes)?,
        e if BINARY_EXTS.contains(&e) => binary::strings_to_text(bytes),
        e if TEXT_EXTS.contains(&e) => decode_bytes(bytes).0,
        _ => {
            return Err(ExtractError::Unsupported(format!(
                "不支持的条目格式: {name}"
            )))
        }
    };
    Ok(ExtractedEntry {
        name: name.to_string(),
        text,
        format: static_ext(&ext),
    })
}

/// 按扩展名分发（extract_path 用；压缩包分支直接 return）。
///
/// `path` 为磁盘真实路径：rar/chm 由 FFI 库按路径打开，无法从内存字节读。
fn dispatch(
    name: &str,
    ext: &str,
    bytes: &[u8],
    path: &Path,
) -> Result<ExtractResult, ExtractError> {
    // sjz 标签容器：先解密再按原扩展名分发（扩展名不变，内容已还原）
    let unboxed = match sjz::probe(bytes) {
        Some(n) => Some(sjz::decrypt(bytes, n)?),
        None => None,
    };
    let bytes = unboxed.as_deref().unwrap_or(bytes);
    let text = match ext {
        e if HTML_EXTS.contains(&e) => html::html_to_text(&decode_bytes(bytes).0),
        "eml" => eml::eml_to_text(bytes),
        e if OOXML_EXTS.contains(&e) => ooxml::to_text(e, bytes)?,
        e if PDF_EXTS.contains(&e) => match pdf::pdf_to_text(bytes) {
            Ok(t) => t,
            Err(ExtractError::NeedsOcr { .. }) if crate::ocr::available() => {
                crate::ocr::ocr_pdf_bytes(bytes)?
            }
            Err(err) => return Err(err),
        },
        e if IMAGE_EXTS.contains(&e) => {
            if crate::ocr::available() {
                crate::ocr::ocr_image_bytes(bytes, e)?
            } else {
                return Err(ExtractError::Unsupported(format!(
                    "图片需 OCR（helper 未就绪，或未设 SEARCH_TOOL_OCR=1）: {name}"
                )));
            }
        }
        e if EPUB_EXTS.contains(&e) => epub::epub_to_text(bytes)?,
        e if LEGACY_EXTS.contains(&e) => legacy::legacy_to_text(e, bytes)?,
        e if EBOOK_EXTS.contains(&e) => ebook::ebook_to_text(bytes)?,
        e if BINARY_EXTS.contains(&e) => binary::strings_to_text(bytes),
        e if TEXT_EXTS.contains(&e) => decode_bytes(bytes).0,
        "zip" => {
            return Ok(ExtractResult {
                name: name.to_string(),
                format: "zip",
                text: String::new(),
                children: archive::expand(archive::ArchiveKind::Zip, bytes)?,
            })
        }
        "tar" => {
            return Ok(ExtractResult {
                name: name.to_string(),
                format: "tar",
                text: String::new(),
                children: archive::expand(archive::ArchiveKind::Tar, bytes)?,
            })
        }
        "tgz" => {
            return Ok(ExtractResult {
                name: name.to_string(),
                format: "tgz",
                text: String::new(),
                children: archive::expand(archive::ArchiveKind::Tgz, bytes)?,
            })
        }
        "gz" => return expand_gz(name, bytes),
        "bz2" => return expand_bz2(name, bytes),
        "7z" => {
            return Ok(ExtractResult {
                name: name.to_string(),
                format: "7z",
                text: String::new(),
                children: sevenz::expand(bytes)?,
            })
        }
        "rar" => {
            return Ok(ExtractResult {
                name: name.to_string(),
                format: "rar",
                text: String::new(),
                children: rar::expand(path)?,
            })
        }
        "chm" => {
            return Ok(ExtractResult {
                name: name.to_string(),
                format: "chm",
                text: String::new(),
                children: chm::expand(path)?,
            })
        }
        _ => {
            return Err(ExtractError::Unsupported(format!(
                "不支持的文件类型: {name}"
            )))
        }
    };
    Ok(ExtractResult {
        name: name.to_string(),
        format: static_ext(ext),
        text,
        children: Vec::new(),
    })
}

/// .gz 语义模糊：既可能是 tar.gz（归档）也可能是单文件 gzip（如 log.txt.gz）。
/// 先按 tar.gz 尝试——tar 解析失败则按单文件 gzip 解压整体为文本。
fn expand_gz(name: &str, bytes: &[u8]) -> Result<ExtractResult, ExtractError> {
    match archive::expand(archive::ArchiveKind::Tgz, bytes) {
        Ok(children) => Ok(ExtractResult {
            name: name.to_string(),
            format: "tgz",
            text: String::new(),
            children,
        }),
        // tar 解析失败：按单文件 gzip 解压（损坏的 gzip 在此返回 Io 错误）
        Err(ExtractError::Io(_)) => {
            // 与 archive.rs 同一思路限量读取：gzip 对高冗余数据（如全零）
            // 膨胀比可达 1000x，一个几十 KB 的 .gz 能解出 GB 级内存，
            // 无上限 read_to_end 等于绕过全部闸门。take 多读 1 字节判定超限，
            // buf 峰值最多 MAX_FILE_SIZE + 1。
            let mut buf = Vec::new();
            flate2::read::GzDecoder::new(bytes)
                .take(MAX_FILE_SIZE + 1)
                .read_to_end(&mut buf)?;
            if buf.len() as u64 > MAX_FILE_SIZE {
                return Err(ExtractError::TooManyEntries);
            }
            Ok(ExtractResult {
                name: name.to_string(),
                format: "gz",
                text: decode_bytes(&buf).0,
                children: Vec::new(),
            })
        }
        // 条目数/总量超限是炸弹信号，不做格式回退
        Err(err) => Err(err),
    }
}

/// .bz2 语义模糊：既可能是 tar.bz2（归档）也可能是单文件 bzip2（如 log.txt.bz2）。
/// 与 .gz 完全同构：先按 tar.bz2 尝试——tar 解析失败则按单文件 bzip2 解压整体为文本。
fn expand_bz2(name: &str, bytes: &[u8]) -> Result<ExtractResult, ExtractError> {
    match archive::expand(archive::ArchiveKind::Tbz, bytes) {
        Ok(children) => Ok(ExtractResult {
            name: name.to_string(),
            format: "tbz",
            text: String::new(),
            children,
        }),
        // tar 解析失败：按单文件 bzip2 解压（损坏的 bz2 在此返回 Io 错误）
        Err(ExtractError::Io(_)) => {
            // 与 gz 回退同一思路限量读取：take 多读 1 字节判定超限，
            // buf 峰值最多 MAX_FILE_SIZE + 1。
            let mut buf = Vec::new();
            bzip2::read::BzDecoder::new(bytes)
                .take(MAX_FILE_SIZE + 1)
                .read_to_end(&mut buf)?;
            if buf.len() as u64 > MAX_FILE_SIZE {
                return Err(ExtractError::TooManyEntries);
            }
            Ok(ExtractResult {
                name: name.to_string(),
                format: "bz2",
                text: decode_bytes(&buf).0,
                children: Vec::new(),
            })
        }
        // 条目数/总量超限是炸弹信号，不做格式回退
        Err(err) => Err(err),
    }
}

/// 取小写扩展名（无扩展名返回空串）。
fn extension_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// 把扩展名映射回 &'static str。
///
/// ext 必来自各扩展名常量表（元素本身即 &'static str），查表拷贝即可，
/// 不需要运行时提升生命周期（早期实现用 Box::leak，每次调用泄漏一份
/// 扩展名拷贝，纯属浪费）。不在表内的落到 "bin" 兜底。
fn static_ext(ext: &str) -> &'static str {
    if ext == "eml" {
        return "eml";
    }
    TEXT_EXTS
        .iter()
        .chain(HTML_EXTS)
        .chain(OOXML_EXTS)
        .chain(PDF_EXTS)
        .chain(EPUB_EXTS)
        .chain(EBOOK_EXTS)
        .chain(LEGACY_EXTS)
        .chain(IMAGE_EXTS)
        .chain(BINARY_EXTS)
        .find(|&&e| e == ext)
        .copied()
        .unwrap_or("bin")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 进程内唯一的临时文件路径（无 tempfile 依赖，测试结束即删）
    fn tmp_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("search-tool-extract-{}-{tag}", std::process::id()))
    }

    fn write_tmp(tag: &str, content: &[u8]) -> std::path::PathBuf {
        let p = tmp_path(tag);
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(content).unwrap();
        p
    }

    #[test]
    fn entry_plain_text_formats() {
        for (name, want) in [
            ("readme.txt", "txt"),
            ("notes.md", "md"),
            ("src/main.rs", "rs"),
            ("data.json", "json"),
            ("cfg.yaml", "yaml"),
        ] {
            let entry = extract_entry(name, "hello 内容".as_bytes()).unwrap();
            assert_eq!(entry.name, name);
            assert_eq!(entry.format, want, "{name} 格式标签不对");
            assert_eq!(entry.text, "hello 内容");
        }
    }

    #[test]
    fn entry_html_strips_script() {
        let entry = extract_entry(
            "page.html",
            b"<html><body><p>Visible</p><script>evil()</script></body></html>",
        )
        .unwrap();
        assert_eq!(entry.format, "html");
        assert_eq!(entry.text, "Visible");
    }

    #[test]
    fn entry_eml_keeps_headers() {
        let raw = "Subject: 测试邮件\r\nContent-Type: text/plain\r\n\r\n正文内容\r\n".as_bytes();
        let entry = extract_entry("mail.eml", raw).unwrap();
        assert_eq!(entry.format, "eml");
        assert!(
            entry.text.contains("Subject: 测试邮件"),
            "实际: {}",
            entry.text
        );
        assert!(entry.text.contains("正文内容"));
    }

    #[test]
    fn entry_unsupported_kinds() {
        // 二进制扩展名
        assert!(matches!(
            extract_entry("img.png", &[0x89, 0x50, 0x4E, 0x47]),
            Err(ExtractError::Unsupported(_))
        ));
        // 无扩展名（README/Makefile 类）：P0 不收录，避免索引二进制
        assert!(matches!(
            extract_entry("README", b"no extension"),
            Err(ExtractError::Unsupported(_))
        ));
        // 嵌套压缩包不展开
        let inner = archive::build_zip(&[("deep.txt", b"x")]);
        assert!(matches!(
            extract_entry("bundle.zip", &inner),
            Err(ExtractError::Unsupported(_))
        ));
    }

    #[test]
    fn entry_empty_bytes_ok() {
        let entry = extract_entry("empty.txt", b"").unwrap();
        assert_eq!(entry.text, "");
    }

    #[test]
    fn path_text_file() {
        let p = write_tmp("plain.txt", "中华人民共和国万岁".as_bytes());
        let r = extract_path(&p).unwrap();
        // name 取磁盘文件名（含 tmp_path 的进程前缀）
        assert_eq!(r.name, p.file_name().unwrap().to_string_lossy());
        assert_eq!(r.format, "txt");
        assert_eq!(r.text, "中华人民共和国万岁");
        assert!(r.children.is_empty());
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn path_html_file() {
        let p = write_tmp(
            "page.html",
            "<html><body><h1>标题</h1><p>正文</p></body></html>".as_bytes(),
        );
        let r = extract_path(&p).unwrap();
        assert_eq!(r.format, "html");
        assert_eq!(r.text, "标题\n正文");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn path_zip_expands_children() {
        let zip = archive::build_zip(&[
            ("docs/a.txt", "甲内容".as_bytes()),
            ("docs/b.md", "# 乙".as_bytes()),
            ("img.png", &[0x89, 0x50]),
        ]);
        let p = write_tmp("bundle.zip", &zip);
        let r = extract_path(&p).unwrap();
        assert_eq!(r.format, "zip");
        assert!(r.text.is_empty(), "压缩包顶层无正文");
        assert_eq!(
            r.children.len(),
            2,
            "png 应被跳过: {:?}",
            r.children.iter().map(|c| &c.name).collect::<Vec<_>>()
        );
        assert_eq!(r.children[0].name, "docs/a.txt");
        assert_eq!(r.children[0].text, "甲内容");
        assert_eq!(r.children[1].name, "docs/b.md");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn path_tgz_expands_children() {
        let tgz = archive::build_tar_gz(&[("a.txt", b"alpha"), ("b.log", b"beta")]);
        let p = write_tmp("bundle.tgz", &tgz);
        let r = extract_path(&p).unwrap();
        assert_eq!(r.format, "tgz");
        assert_eq!(r.children.len(), 2);
        assert_eq!(r.children[0].text, "alpha");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn path_gz_targz_goes_archive_route() {
        let tgz = archive::build_tar_gz(&[("a.txt", b"inner")]);
        let p = write_tmp("archive2.gz", &tgz);
        let r = extract_path(&p).unwrap();
        assert_eq!(r.format, "tgz", "合法 tar.gz 应走归档路由");
        assert_eq!(r.children.len(), 1);
        assert_eq!(r.children[0].text, "inner");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn path_gz_single_file_falls_back_to_text() {
        // 单文件 gzip（非 tar）：按整体解压为文本
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all("纯文本内容，不是 tar".as_bytes())
            .unwrap();
        let gz = encoder.finish().unwrap();
        let p = write_tmp("single.txt.gz", &gz);
        let r = extract_path(&p).unwrap();
        assert_eq!(r.format, "gz");
        assert_eq!(r.text, "纯文本内容，不是 tar");
        assert!(r.children.is_empty());
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn path_gz_corrupt_is_io_error() {
        let p = write_tmp("corrupt.gz", b"this is not gzip at all");
        let err = extract_path(&p).unwrap_err();
        assert!(matches!(err, ExtractError::Io(_)), "实际: {err:?}");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn path_gz_bomb_spooled_to_disk_is_rejected() {
        // 高膨胀单文件 gzip：60MB 高冗余内容（全 'A'）gzip 后磁盘上仅约
        // 60KB，文件大小闸门拦不住（磁盘小），只能靠解压上限拦。
        // 注意填充不能是全零：tar 把全零块当结束标记，会被误判为合法空
        // 归档而根本进不了单文件回退分支。流式写入，测试自身内存恒定。
        let p = tmp_path("bomb.txt.gz");
        {
            let f = std::fs::File::create(&p).unwrap();
            let mut encoder = flate2::write::GzEncoder::new(f, flate2::Compression::default());
            let chunk = vec![b'A'; 1024 * 1024];
            for _ in 0..60 {
                encoder.write_all(&chunk).unwrap();
            }
            encoder.finish().unwrap();
        }
        let err = extract_path(&p).unwrap_err();
        assert!(
            matches!(err, ExtractError::TooManyEntries),
            "压缩炸弹应快速失败: {err:?}"
        );
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn path_unsupported_extension() {
        let p = write_tmp("image.png", &[0x89, 0x50, 0x4E, 0x47]);
        assert!(matches!(
            extract_path(&p),
            Err(ExtractError::Unsupported(_))
        ));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn path_no_extension_unsupported() {
        let p = write_tmp("noext", b"content");
        assert!(matches!(
            extract_path(&p),
            Err(ExtractError::Unsupported(_))
        ));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn path_missing_file_is_io_error() {
        let p = tmp_path("does-not-exist.txt");
        assert!(matches!(extract_path(&p), Err(ExtractError::Io(_))));
    }

    #[test]
    fn path_empty_text_file_ok() {
        let p = write_tmp("empty.txt", b"");
        let r = extract_path(&p).unwrap();
        assert_eq!(r.text, "");
        std::fs::remove_file(&p).ok();
    }
}
