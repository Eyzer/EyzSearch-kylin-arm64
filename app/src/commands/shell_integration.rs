//! 右击搜索：在文件管理器**目录上/目录空白处右键** → “用 EyzSearch 搜索”
//! → 以该目录启动应用。启停开关对应“设置 → 右击目录搜索”。
//!
//! ## 平台实现
//! | 平台 | 机制 | 状态 |
//! |---|---|---|
//! | Linux | ① Nautilus 脚本（选中目录/文件时）② nautilus-python 扩展（**目录空白处**，需系统装 `nautilus-python`）③ `.desktop`（Open With 关联目录） | 已实现 |
//! | Windows | `HKCU\Software\Classes\Directory\{shell,Background\shell}\SearchTool`（右键菜单项，`%V` 传目录） | 已实现（本机不可实测） |
//! | macOS | 需 Finder Sync App Extension（须签名打包，非本工程范围） | 明确不支持并提示 |
//!
//! 所有写入都在**用户级**目录/注册表（无需管理员权限）；卸载即删除这些文件/键。

use std::path::{Path, PathBuf};

use tauri::State;

use crate::commands::shell::AppConfig;
use crate::state::AppState;

/// 菜单显示名
const MENU_LABEL: &str = "用 EyzSearch 搜索";
/// Nautilus 脚本文件名
#[cfg(unix)]
const NAUTILUS_SCRIPT: &str = "eyzsearch-search";
/// nautilus-python 扩展文件名
#[cfg(unix)]
const NAUTILUS_PY_EXT: &str = "eyzsearch.py";

#[cfg(unix)]
fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
}

#[cfg(unix)]
fn nautilus_scripts_dir() -> PathBuf {
    home().join(".local/share/nautilus/scripts")
}

#[cfg(unix)]
fn nautilus_py_dir() -> PathBuf {
    home().join(".local/share/nautilus-python/extensions")
}

#[cfg(unix)]
fn applications_dir() -> PathBuf {
    home().join(".local/share/applications")
}

/// 当前可执行文件路径（用于写入脚本/注册表）
fn exe_path() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("无法定位可执行文件: {e}"))
}

/// 是否已安装（以 Linux 的 py 扩展或 Windows 注册表键为准）
pub fn is_installed() -> bool {
    #[cfg(unix)]
    {
        nautilus_py_dir().join(NAUTILUS_PY_EXT).is_file()
            || nautilus_scripts_dir().join(NAUTILUS_SCRIPT).is_file()
    }
    #[cfg(windows)]
    {
        use std::process::Command;
        Command::new("reg")
            .args([
                "query",
                r"HKCU\Software\Classes\Directory\Background\shell\SearchTool",
            ])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

/* ------------------------------ Linux 实现 ------------------------------ */

#[cfg(unix)]
fn install_unix(exe: &Path) -> Result<String, String> {
    let exe_s = exe.to_string_lossy().into_owned();

    // ① Nautilus 脚本（选中目录/文件时可用）
    let scripts = nautilus_scripts_dir();
    std::fs::create_dir_all(&scripts)
        .map_err(|e| format!("创建 {} 失败: {e}", scripts.display()))?;
    let script_path = scripts.join(NAUTILUS_SCRIPT);
    let script = format!(
        "#!/bin/sh\n\
         # EyzSearch 右击搜索（由应用写入；删除本文件即卸载）\n\
         IFS='\\n'\n\
         for p in $NAUTILUS_SCRIPT_SELECTED_FILE_PATHS; do\n\
         \x20 [ -d \"$p\" ] && exec \"{exe_s}\" --open-dir \"$p\"\n\
         \x20 [ -f \"$p\" ] && exec \"{exe_s}\" --open-dir \"$(dirname \"$p\")\"\n\
         done\n\
         exec \"{exe_s}\"\n"
    );
    std::fs::write(&script_path, script).map_err(|e| format!("写入脚本失败: {e}"))?;
    set_exec(&script_path)?;

    // ② nautilus-python 扩展（目录空白处右键，需系统安装 nautilus-python）
    let py_dir = nautilus_py_dir();
    std::fs::create_dir_all(&py_dir).map_err(|e| format!("创建 {} 失败: {e}", py_dir.display()))?;
    let py_path = py_dir.join(NAUTILUS_PY_EXT);
    let py = format!(
        r#"# EyzSearch 右击搜索扩展（由应用写入；删除本文件即卸载）
# 依赖：系统安装 nautilus-python + python3-gi（多数发行版仓库可装）
import subprocess
from urllib.parse import unquote

from gi.repository import GObject, Nautilus

EYZSEARCH = "{exe_s}"
MENU_LABEL = "{MENU_LABEL}"


class SearchToolMenuProvider(GObject.GObject, Nautilus.MenuProvider):
    def _activate(self, _menu, path: str) -> None:
        subprocess.Popen([EYZSEARCH, "--open-dir", path])

    def get_background_items(self, folder):
        path = unquote(folder.get_uri()[7:])
        item = Nautilus.MenuItem(
            name="SearchTool::BackgroundSearch", label=MENU_LABEL, tip="在此目录启动全文搜索"
        )
        item.connect("activate", self._activate, path)
        return [item]

    def get_file_items(self, files):
        out = []
        for f in files:
            if not f.is_directory():
                continue
            path = unquote(f.get_uri()[7:])
            item = Nautilus.MenuItem(
                name="SearchTool::DirSearch", label=MENU_LABEL, tip="搜索此目录"
            )
            item.connect("activate", self._activate, path)
            out.append(item)
        return out
"#
    );
    std::fs::write(&py_path, py).map_err(|e| format!("写入扩展失败: {e}"))?;

    // ③ .desktop（Open With 关联目录）
    let apps = applications_dir();
    std::fs::create_dir_all(&apps).map_err(|e| format!("创建 {} 失败: {e}", apps.display()))?;
    let desktop = apps.join("eyzsearch.desktop");
    let content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=EyzSearch\n\
         Comment=本地全文搜索\n\
         Exec=\"{exe_s}\" --open-dir %f\n\
         MimeType=inode/directory;\n\
         Terminal=false\n\
         Categories=Utility;FileTools;\n"
    );
    std::fs::write(&desktop, content).map_err(|e| format!("写入 .desktop 失败: {e}"))?;

    Ok(format!(
        "已启用：目录空白处右键（需系统装 nautilus-python）→ {}；选中目录右键“脚本” → {}",
        py_path.display(),
        script_path.display()
    ))
}

#[cfg(unix)]
fn uninstall_unix() -> Result<String, String> {
    let mut removed = Vec::new();
    for p in [
        nautilus_py_dir().join(NAUTILUS_PY_EXT),
        nautilus_scripts_dir().join(NAUTILUS_SCRIPT),
        applications_dir().join("eyzsearch.desktop"),
    ] {
        if p.exists() {
            std::fs::remove_file(&p).map_err(|e| format!("删除 {} 失败: {e}", p.display()))?;
            removed.push(p.display().to_string());
        }
    }
    Ok(if removed.is_empty() {
        "无可卸载项".to_string()
    } else {
        format!("已移除：{}", removed.join("、"))
    })
}

#[cfg(unix)]
fn set_exec(p: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(p)
        .map_err(|e| e.to_string())?
        .permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(p, perm).map_err(|e| e.to_string())
}

/* ----------------------------- Windows 实现 ----------------------------- */

#[cfg(windows)]
fn install_windows(exe: &Path) -> Result<String, String> {
    let exe_s = exe.to_string_lossy().into_owned();
    let label = MENU_LABEL.to_string();
    let cmd = format!("\"{exe_s}\" --open-dir \"%V\"");
    let keys = [
        (
            r"HKCU\Software\Classes\Directory\Background\shell\SearchTool",
            &label,
        ),
        (r"HKCU\Software\Classes\Directory\shell\SearchTool", &label),
    ];
    for (key, lbl) in keys {
        run_reg(&["add", key, "/ve", "/d", lbl, "/f"])?;
        run_reg(&["add", &format!("{key}\\command"), "/ve", "/d", &cmd, "/f"])?;
    }
    Ok("已启用：资源管理器目录（含空白处）右键 → “用 EyzSearch 搜索”".to_string())
}

#[cfg(windows)]
fn uninstall_windows() -> Result<String, String> {
    run_reg(&[
        "delete",
        r"HKCU\Software\Classes\Directory\Background\shell\SearchTool",
        "/f",
    ])?;
    run_reg(&[
        "delete",
        r"HKCU\Software\Classes\Directory\shell\SearchTool",
        "/f",
    ])?;
    Ok("已移除注册表右键菜单项".to_string())
}

#[cfg(windows)]
fn run_reg(args: &[&str]) -> Result<(), String> {
    let out = std::process::Command::new("reg")
        .args(args)
        .output()
        .map_err(|e| format!("执行 reg 失败: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "reg 失败: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/* ------------------------------- 公共入口 ------------------------------- */

/// 安装/卸载右键集成，返回人类可读结果
pub fn set_enabled(enabled: bool) -> Result<String, String> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if enabled {
            install_unix(&exe_path()?)
        } else {
            uninstall_unix()
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = enabled;
        Err("macOS 的“右击搜索”需 Finder Sync 扩展（须随应用签名打包），当前构建未包含；可在“设置”中改用全局热键启动".to_string())
    }
    #[cfg(windows)]
    {
        if enabled {
            install_windows(&exe_path()?)
        } else {
            uninstall_windows()
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = enabled;
        Err("当前平台不支持右击搜索".to_string())
    }
}

/* -------------------------------- 命令层 -------------------------------- */

/// 取走 CLI `--open-dir` 传入的初始目录（一次性；前端启动时调用）
#[tauri::command]
pub fn take_open_dir(state: State<'_, AppState>) -> Result<Option<String>, String> {
    Ok(state.take_pending_open_dir())
}

/// 查询右键集成是否已安装
#[tauri::command]
pub fn get_context_menu_installed() -> Result<bool, String> {
    Ok(is_installed())
}

/// 启用/停用右击搜索（写入系统集成 + 持久化开关）
#[tauri::command(async)]
pub fn set_context_menu_enabled(
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let msg = set_enabled(enabled)?;
    let mut cfg = AppConfig::load(&state.index_dir());
    cfg.right_click_search = enabled;
    cfg.save(&state.index_dir())
        .map_err(|e| format!("保存配置失败: {e}"))?;
    Ok(msg)
}

#[cfg(test)]
mod tests {
    // 模块内唯一测试为 unix 专属；Windows 下编译本模块时 import 无用
    // （unused-imports 在 -D warnings 下报错），故按平台门控。
    #[cfg(unix)]
    use super::*;

    /// 本测试改写进程 HOME，故只在此处使用（其余测试不读 HOME）
    #[test]
    #[cfg(unix)]
    fn unix_install_then_uninstall_writes_and_removes_files() {
        let tmp = std::env::temp_dir().join(format!("st-shellint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let old_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &tmp);

        let msg = set_enabled(true).expect("安装应成功");
        assert!(msg.contains("已启用"), "{msg}");
        assert!(
            nautilus_py_dir().join(NAUTILUS_PY_EXT).is_file(),
            "py 扩展应写入"
        );
        assert!(
            nautilus_scripts_dir().join(NAUTILUS_SCRIPT).is_file(),
            "nautilus 脚本应写入"
        );
        assert!(
            applications_dir().join("eyzsearch.desktop").is_file(),
            ".desktop 应写入"
        );
        assert!(is_installed(), "安装后状态应为已安装");

        // 脚本应可执行且含 --open-dir
        let script = std::fs::read_to_string(nautilus_scripts_dir().join(NAUTILUS_SCRIPT)).unwrap();
        assert!(
            script.contains("--open-dir"),
            "脚本应传 --open-dir: {script}"
        );

        let msg2 = set_enabled(false).expect("卸载应成功");
        assert!(msg2.contains("已移除"), "{msg2}");
        assert!(!is_installed(), "卸载后状态应为未安装");

        match old_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
