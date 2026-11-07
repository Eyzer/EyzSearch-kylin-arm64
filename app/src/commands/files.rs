//! 文件操作命令（子项 2 所有权：结果交互与文件操作）
//!
//! 契约：ui/src/api/files.ts
//! 六操作：打开 / 定位目录 / 复制路径 / 回收站删除 / 彻底删除 / 复制移动 + 导出 xlsx
//!
//! 命名约定：命令名与参数名冻结，前端按 camelCase 调用，Tauri 自动映射到 snake_case。
//! 批量操作一律「逐条收集成败、不因单条失败中断」，返回 [`FileOpResult`]。
//!
//! R9 备注：
//! - Linux 无「在文件管理器中选中文件」的标准动作，`reveal_in_folder` 降级为打开父目录。
//! - 前端未安装 `@tauri-apps/plugin-dialog`（见交付报告），故「复制到…/移动到…」的
//!   目标目录改由 app 侧 `tauri_plugin_dialog` 弹框获取：`transfer_files` 的 `dest`
//!   传空串即触发。
//! - 导出 xlsx 的日期按 UTC 计算（std 无时区信息，不引 chrono）。

use std::path::{Path, PathBuf};

use fs_extra::dir::CopyOptions;
use rust_xlsxwriter::{Format, Workbook};

use crate::commands::dto::{ExportRequest, FileOpResult};

/// 用系统默认程序打开文件
#[tauri::command]
pub fn open_file(path: String) -> Result<(), String> {
    open::that(&path).map_err(|e| e.to_string())
}

/// 弹系统目录选择框，返回绝对路径；用户取消返回 `None`。
///
/// 索引管理里「添加目录 / 迁移索引存储」用它替代手输路径。
#[tauri::command(async)]
pub fn pick_directory(title: Option<String>, _app: tauri::AppHandle) -> Result<Option<String>, String> {
    let mut builder = tauri::api::dialog::blocking::FileDialogBuilder::new();
    if let Some(t) = title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        builder = builder.set_title(t);
    }
    Ok(builder
        .pick_folder()
        .map(|p| p.to_string_lossy().to_string()))
}

/// 在系统文件管理器中定位该文件
///
/// - Windows：`explorer /select,<path>` 在资源管理器中选中该文件
/// - macOS：`open -R <path>` 在访达中显示
/// - 其它 Unix（R9）：无「选中」标准动作，降级为打开父目录
#[tauri::command(async)]
pub fn reveal_in_folder(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err(format!("路径不存在: {path}"));
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", p.display()))
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(p)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // R9：Linux 降级 —— 打开文件所在目录
        let dir = p.parent().unwrap_or_else(|| Path::new("."));
        open::that(dir).map_err(|e| e.to_string())
    }
}

/// 复制文件路径到剪贴板（arboard）
#[tauri::command(async)]
pub fn copy_paths(paths: Vec<String>) -> Result<(), String> {
    let text = paths.join("\n");
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text))
        .map_err(|e| e.to_string())
}

/// 单个虚拟/真实路径 → 文件名（纯逻辑，无副作用，便于单测）。
///
/// 压缩包内层条目的虚拟路径形如 `pkg.zip#inner/readme.md`：
/// 先按 '#' 取末段再取 file_name，避免把 `zip#inner` 当文件名。
fn file_name_of(p: &str) -> String {
    let seg = p.rsplit('#').next().unwrap_or(p);
    Path::new(seg)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| seg.to_string())
}

/// 复制文件名（不含目录）到剪贴板
#[tauri::command]
pub fn copy_file_names(paths: Vec<String>) -> Result<(), String> {
    let names: Vec<String> = paths.iter().map(|p| file_name_of(p)).collect();
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(names.join("\n")))
        .map_err(|e| e.to_string())
}

/// 移入系统回收站（安全：仅对真实存在的普通文件/目录执行）
#[tauri::command(async)]
pub fn move_to_trash(paths: Vec<String>) -> Result<FileOpResult, String> {
    let mut r = FileOpResult::new();
    for p in &paths {
        // 安全护栏：trash 对不存在路径会报错，这里提前拦截给出清晰信息，避免误触
        if !Path::new(p).exists() {
            r.push_err(p.clone(), "路径不存在".to_string());
            continue;
        }
        match trash::delete(p) {
            Ok(()) => r.push_ok(),
            Err(e) => r.push_err(p.clone(), e.to_string()),
        }
    }
    Ok(r)
}

/// 危险路径护栏：拒绝文件系统根、索引目录自身及其所有祖先。
///
/// `delete_permanently` 是**不可恢复**操作，输入完全来自前端 IPC；
/// webview 层一旦出现 bug 或恶意脚本，无护栏即可抹掉索引库甚至盘根。
fn guard_dangerous(path: &Path, index_dir: &Path) -> Result<(), String> {
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if canon.parent().is_none() {
        return Err("拒绝操作文件系统根目录".to_string());
    }
    let idx = index_dir
        .canonicalize()
        .unwrap_or_else(|_| index_dir.to_path_buf());
    // canon 是 idx 自身或其祖先 → 拒绝（会连带删掉索引库）
    if idx == canon || idx.starts_with(&canon) {
        return Err(format!("拒绝操作索引目录或其祖先: {}", canon.display()));
    }
    Ok(())
}

/// 彻底删除核心（不含 State 注入，便于单测）
fn delete_core(paths: &[String], index_dir: &Path) -> FileOpResult {
    let mut r = FileOpResult::new();
    for p in paths {
        let path = Path::new(p);
        if !path.exists() {
            r.push_err(p.clone(), "路径不存在".to_string());
            continue;
        }
        if let Err(e) = guard_dangerous(path, index_dir) {
            r.push_err(p.clone(), e);
            continue;
        }
        let res = if path.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        };
        match res {
            Ok(()) => r.push_ok(),
            Err(e) => r.push_err(p.clone(), e.to_string()),
        }
    }
    r
}

/// 彻底删除（不可恢复）：文件 `remove_file`、目录 `remove_dir_all`，逐条收集成败
#[tauri::command(async)]
pub fn delete_permanently(
    paths: Vec<String>,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<FileOpResult, String> {
    Ok(delete_core(&paths, &state.index_dir()))
}

/// 复制/移动到目标目录的核心实现（不含对话框），便于单测。
///
/// 语义：把每个条目放进 `dest` 目录并保留原名；文件用 `fs_extra::file`，
/// 目录用 `fs_extra::dir`（复制到目录内）。逐条失败不中断。
fn transfer_into(paths: &[String], dest: &Path, is_move: bool) -> FileOpResult {
    let mut r = FileOpResult::new();

    // 目标目录不存在则创建；连目录都建不出来则整批失败
    if let Err(e) = std::fs::create_dir_all(dest) {
        for p in paths {
            r.push_err(p.clone(), format!("创建目标目录失败: {e}"));
        }
        return r;
    }

    // dir / file 两套 CopyOptions 类型不同（fs_extra 的 API 设计），分别构建
    let dir_opts = CopyOptions::new().overwrite(true);
    let file_opts = fs_extra::file::CopyOptions::new().overwrite(true);
    for p in paths {
        let src = Path::new(p);
        if !src.exists() {
            r.push_err(p.clone(), "路径不存在".to_string());
            continue;
        }
        let Some(name) = src.file_name() else {
            r.push_err(p.clone(), "无法解析文件名".to_string());
            continue;
        };
        let target = dest.join(name);

        // 护栏：目标位于源目录内部 → 递归自拷贝/自移动，可能写爆磁盘
        if src.is_dir() {
            let s = src.canonicalize().unwrap_or_else(|_| src.to_path_buf());
            let d = dest.canonicalize().unwrap_or_else(|_| dest.to_path_buf());
            if d.starts_with(&s) {
                r.push_err(
                    p.clone(),
                    format!("目标目录位于源目录内部: {}", d.display()),
                );
                continue;
            }
        }

        let res = if src.is_dir() {
            if is_move {
                fs_extra::dir::move_dir(src, dest, &dir_opts).map(|_| ())
            } else {
                fs_extra::dir::copy(src, dest, &dir_opts).map(|_| ())
            }
        } else if is_move {
            fs_extra::file::move_file(src, &target, &file_opts).map(|_| ())
        } else {
            fs_extra::file::copy(src, &target, &file_opts).map(|_| ())
        };

        match res {
            Ok(()) => r.push_ok(),
            Err(e) => r.push_err(p.clone(), e.to_string()),
        }
    }
    r
}

/// 复制/移动文件到目标目录（is_move=true 为移动）
///
/// `dest` 为空串时：app 侧弹目录选择框（前端无 `@tauri-apps/plugin-dialog` 依赖）；
/// 用户取消 → `Err("已取消")`。
#[tauri::command(async)]
pub fn transfer_files(
    paths: Vec<String>,
    dest: String,
    is_move: bool,
    _app: tauri::AppHandle,
) -> Result<FileOpResult, String> {
    let dest: PathBuf = if dest.trim().is_empty() {
        let picked = tauri::api::dialog::blocking::FileDialogBuilder::new()
            .set_title("选择目标目录")
            .pick_folder();
        match picked {
            Some(p) => p,
            None => return Err("已取消".into()),
        }
    } else {
        PathBuf::from(dest)
    };
    Ok(transfer_into(&paths, &dest, is_move))
}

/// 导出结果为 xlsx（列=名称/日期/路径/命中数/大小），返回落盘绝对路径
///
/// `req.dest` 为空串时用 `tauri_plugin_dialog` 保存对话框取路径；用户取消 → `Err("已取消导出")`。
#[tauri::command(async)]
pub fn export_xlsx(req: ExportRequest, _app: tauri::AppHandle) -> Result<String, String> {
    let dest: PathBuf = if req.dest.trim().is_empty() {
        let picked = tauri::api::dialog::blocking::FileDialogBuilder::new()
            .add_filter("Excel 工作簿", &["xlsx"])
            .set_file_name("search-results.xlsx")
            .save_file();
        match picked {
            Some(p) => p,
            None => return Err("已取消导出".into()),
        }
    } else {
        PathBuf::from(req.dest.trim())
    };
    let dest_str = dest
        .to_str()
        .ok_or_else(|| "目标路径含非法字符".to_string())?;

    let mut workbook = Workbook::new();
    {
        let sheet = workbook.add_worksheet();
        let bold = Format::new().set_bold();

        // 表头（加粗）
        let headers = ["名称", "修改日期", "路径", "命中数", "大小"];
        for (c, h) in headers.iter().enumerate() {
            sheet
                .write_string(0, c as u16, h, &bold)
                .map_err(|e| e.to_string())?;
        }

        // 数据行
        for (i, row) in req.rows.iter().enumerate() {
            let r = (i + 1) as u32;
            sheet
                .write_string_only(r, 0, &row.name)
                .map_err(|e| e.to_string())?;
            sheet
                .write_string_only(r, 1, &fmt_mtime(row.mtime))
                .map_err(|e| e.to_string())?;
            sheet
                .write_string_only(r, 2, &row.path)
                .map_err(|e| e.to_string())?;
            sheet
                .write_number_only(r, 3, row.hits as f64)
                .map_err(|e| e.to_string())?;
            sheet
                .write_string_only(r, 4, &fmt_size(row.size))
                .map_err(|e| e.to_string())?;
        }
    }

    workbook.save(dest_str).map_err(|e| e.to_string())?;
    Ok(dest_str.to_string())
}

/// 字节 → 人类可读（与前端 util.formatSize 口径一致：i==0 不带小数）
fn fmt_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0usize;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} {}", UNITS[i])
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// unix 秒 → "YYYY-MM-DD HH:MM"（UTC；std 无时区，见 R9 备注）
fn fmt_mtime(unix: i64) -> String {
    let secs = unix.max(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, mi) = (rem / 3600, (rem % 3600) / 60);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}")
}

/// 自 epoch 起的天数 → (年, 月, 日)（Howard Hinnant civil_from_days，UTC）
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 自建唯一临时目录（不引 tempfile：Cargo.toml 非本子项所有权，见交付报告）
    fn tmp_dir(tag: &str) -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let p =
            std::env::temp_dir().join(format!("st-files-test-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn delete_permanently_partial_failure() {
        let base = tmp_dir("del");
        let a = base.join("a.txt");
        std::fs::write(&a, b"hello").unwrap();
        let missing = base.join("nope.txt");

        let r = delete_core(
            &[
                a.to_string_lossy().into_owned(),
                missing.to_string_lossy().into_owned(),
            ],
            &base,
        );

        assert_eq!(r.ok, 1, "存在文件应删成功");
        assert_eq!(r.failed.len(), 1, "缺失路径应记为失败且不中断");
        assert!(!a.exists());
        assert!(r.failed[0].path.ends_with("nope.txt"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn delete_permanently_directory_recursive() {
        let base = tmp_dir("del-dir");
        let d = base.join("sub");
        std::fs::create_dir_all(d.join("inner")).unwrap();
        std::fs::write(d.join("inner").join("f.txt"), b"x").unwrap();

        let r = delete_core(&[d.to_string_lossy().into_owned()], &base);
        assert_eq!(r.ok, 1);
        assert!(r.failed.is_empty());
        assert!(!d.exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn transfer_copy_partial_failure_keeps_source() {
        let base = tmp_dir("xfer-cp");
        let src = base.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let f1 = src.join("one.txt");
        std::fs::write(&f1, b"content-1").unwrap();
        let missing = base.join("ghost.txt");
        let dst = base.join("dst");

        let r = transfer_into(
            &[
                f1.to_string_lossy().into_owned(),
                missing.to_string_lossy().into_owned(),
            ],
            &dst,
            false,
        );

        assert_eq!(r.ok, 1);
        assert_eq!(r.failed.len(), 1);
        assert!(dst.join("one.txt").exists(), "应落到目标目录");
        assert!(f1.exists(), "复制后源文件仍在");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn transfer_move_removes_source() {
        let base = tmp_dir("xfer-mv");
        let src = base.join("mv");
        std::fs::create_dir_all(&src).unwrap();
        let f1 = src.join("m.txt");
        std::fs::write(&f1, b"m").unwrap();
        let dst = base.join("out"); // 目标目录不存在，应被创建

        let r = transfer_into(&[f1.to_string_lossy().into_owned()], &dst, true);

        assert_eq!(r.ok, 1);
        assert!(r.failed.is_empty());
        assert!(dst.join("m.txt").exists());
        assert!(!f1.exists(), "移动后源文件应消失");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn move_to_trash_rejects_missing_path() {
        let base = tmp_dir("trash");
        let missing = base.join("no-such-file.txt");

        let r = move_to_trash(vec![missing.to_string_lossy().into_owned()]).unwrap();
        assert_eq!(r.ok, 0, "不存在路径不应进回收站");
        assert_eq!(r.failed.len(), 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn formatting_helpers() {
        assert_eq!(fmt_size(512), "512 B");
        assert_eq!(fmt_size(2048), "2.0 KB");
        assert_eq!(fmt_size(1024 * 1024), "1.0 MB");
        // 2021-12-31 15:01 UTC
        assert_eq!(fmt_mtime(1_640_962_860), "2021-12-31 15:01");
    }

    #[test]
    fn file_name_of_plain_path() {
        assert_eq!(file_name_of("/a/b/report.md"), "report.md");
    }

    #[test]
    fn file_name_of_archive_virtual_path() {
        assert_eq!(file_name_of("/a/pkg.zip#inner/readme.md"), "readme.md");
    }

    #[test]
    fn file_name_of_bare_string() {
        assert_eq!(file_name_of("plain.txt"), "plain.txt");
    }
}
