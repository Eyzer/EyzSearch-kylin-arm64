//! 应用外壳命令（子项 5 所有权：应用外壳）
//!
//! 契约：ui/src/api/shell.ts
//! 职责：皮肤三态 / 开机启动 / 全局热键 / 主窗口显隐 / CLI 单实例 socket 查询转发
//!
//! 配置持久化：`<index_dir>/app.json`（与 stcore::config 的 config.json 分离，避免耦合）

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, CustomMenuItem, Manager, State, SystemTray, SystemTrayEvent, SystemTrayMenu,
    SystemTrayMenuItem,
};

use crate::state::AppState;

pub const APP_CONFIG_FILE: &str = "app.json";
/// Unix socket 文件名（位于 index_dir 下）
pub const SOCKET_FILE: &str = "eyzsearch.sock";
/// Windows TCP loopback 端口文件名（位于 index_dir 下；单实例 transport，见 `setup_single_instance_tcp`）
#[cfg(windows)]
pub const PORT_FILE: &str = "eyzsearch.port";

/* ------------------------------- 配置类型 ------------------------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyBinding {
    pub action: String,
    pub label: String,
    pub accel: String,
}

/// 适合注册为**系统级全局热键**的 action 白名单。
///
/// 全局热键会劫持**整个系统**的按键：`Ctrl+A` / `Enter` / `Ctrl+S` 这类
/// 通用编辑键若全局注册，用户在浏览器、编辑器里按键会被本应用截获（P2 审计）。
/// 故只有下面这些 action 真正全局注册；其余条目是**窗口内快捷键**
/// （由前端 `keydown` 处理，accel 仅作键位来源与展示）。
pub const GLOBAL_HOTKEY_ACTIONS: &[&str] = &["showWindow"];

/// 默认热键表
///
/// 仅 `showWindow` 为系统级全局热键（见 [`GLOBAL_HOTKEY_ACTIONS`]）；
/// 其余为窗口内快捷键，全局注册会劫持其他应用。
pub fn default_hotkeys() -> Vec<HotkeyBinding> {
    let mk = |action: &str, label: &str, accel: &str| HotkeyBinding {
        action: action.to_string(),
        label: label.to_string(),
        accel: accel.to_string(),
    };
    vec![
        mk("showWindow", "显示/隐藏主窗口（全局）", "Ctrl+Shift+Space"),
        mk("focusSearch", "聚焦搜索框（窗口内）", "Ctrl+L"),
        mk("exportResults", "导出结果为 xlsx（窗口内）", "Ctrl+S"),
        mk("findInDoc", "文档内查找（窗口内）", "Ctrl+F"),
        mk("nextResult", "下一个结果（窗口内）", "F3"),
        mk("prevResult", "上一个结果（窗口内）", "Shift+F3"),
        mk("selectAll", "全选结果（窗口内）", "Ctrl+A"),
        mk("toggleTheme", "切换皮肤（窗口内）", "Ctrl+Shift+T"),
        mk("openIndexAdmin", "打开索引管理（窗口内）", "Ctrl+I"),
        mk("openFile", "打开选中文件（窗口内）", "Enter"),
        mk("revealInFolder", "定位到文件（窗口内）", "Ctrl+Enter"),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default)]
    pub theme: ThemeMode,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub close_to_tray: bool,
    /// 右击目录搜索（shell 集成）开关状态（P5）
    #[serde(default)]
    pub right_click_search: bool,
    /// OCR 识别开关（图片 / 扫描型 PDF）。**默认关闭**：
    /// 开启后索引与预览会对无文字层的扫描件走 OCR，代价是索引耗时显著增加。
    #[serde(default)]
    pub ocr_enabled: bool,
    #[serde(default = "default_hotkeys")]
    pub hotkeys: Vec<HotkeyBinding>,
}

fn default_language() -> String {
    "zh-CN".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: ThemeMode::default(),
            language: default_language(),
            autostart: false,
            close_to_tray: false,
            right_click_search: false,
            ocr_enabled: false,
            hotkeys: default_hotkeys(),
        }
    }
}

impl AppConfig {
    fn path_in(index_dir: &std::path::Path) -> PathBuf {
        index_dir.join(APP_CONFIG_FILE)
    }

    pub fn load(index_dir: &std::path::Path) -> Self {
        let p = Self::path_in(index_dir);
        std::fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, index_dir: &std::path::Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(index_dir)?;
        let p = Self::path_in(index_dir);
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, &p)?;
        Ok(())
    }
}

/* -------------------------------- 命令 -------------------------------- */

#[tauri::command]
pub fn get_app_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(AppConfig::load(&state.index_dir()))
}

#[tauri::command]
pub fn set_app_config(cfg: AppConfig, state: State<'_, AppState>) -> Result<(), String> {
    cfg.save(&state.index_dir()).map_err(|e| e.to_string())?;
    // OCR 开关落到进程级标志：设置页改完立即生效（对下一次索引/预览起作用）
    stcore::ocr::set_enabled(cfg.ocr_enabled);
    Ok(())
}

/// 当前 OCR 是否真正可用（开关打开 + helper 二进制就位）
///
/// 设置页用它给出提示：开关开了但 helper 缺失时，应告知用户扫描件仍不会被识别。
#[tauri::command]
pub fn ocr_status() -> OcrStatus {
    OcrStatus {
        enabled: stcore::ocr::enabled(),
        helper_ready: stcore::ocr::helper_path().is_some(),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrStatus {
    pub enabled: bool,
    pub helper_ready: bool,
}

/// 用系统默认浏览器打开外部链接。
///
/// 前端 `window.open` 在 Tauri webview 里会被拦（表现为「点了没反应」），
/// 故所有外链统一走这里。只放行 http/https，避免被当成任意命令执行。
#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    let u = url.trim();
    if !(u.starts_with("https://") || u.starts_with("http://")) {
        return Err("仅支持 http/https 链接".into());
    }
    open::that(u).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

/// 应用名与版本（「关于」面板用）。
///
/// 版本优先取打包时注入的 `EYZ_VERSION`（deb 版本号，如 `0.2.3+kylin1`），
/// 未注入时回退 Cargo 包版本，避免界面上显示与安装包不一致的号。
#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: "EyzSearch 全文搜索".to_string(),
        version: option_env!("EYZ_VERSION")
            .unwrap_or(env!("CARGO_PKG_VERSION"))
            .to_string(),
    }
}

#[tauri::command]
pub fn set_theme(theme: ThemeMode, state: State<'_, AppState>) -> Result<(), String> {
    let mut cfg = AppConfig::load(&state.index_dir());
    cfg.theme = theme;
    cfg.save(&state.index_dir()).map_err(|e| e.to_string())
}

/// 开机启动开关。
///
/// Tauri 1 无 autostart 插件，直接按 freedesktop 规范写
/// `~/.config/autostart/eyzsearch.desktop`（麒麟/Debian 桌面均生效）。
#[tauri::command]
pub fn set_autostart(enabled: bool, state: State<'_, AppState>) -> Result<(), String> {
    let home = std::env::var_os("HOME").ok_or("无法定位 HOME 目录")?;
    let dir = PathBuf::from(home).join(".config").join("autostart");
    let path = dir.join("eyzsearch.desktop");
    // 旧版本留下的条目，一并清掉，避免「启动应用程序」里出现两个同名项
    let legacy = dir.join("search-tool.desktop");

    if enabled {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let body = "[Desktop Entry]\n\
Type=Application\n\
Name=EyzSearch 全文搜索\n\
Comment=本地全文搜索引擎（开机自启）\n\
Exec=/usr/bin/eyzsearch-launch\n\
Icon=eyzsearch\n\
Terminal=false\n\
X-GNOME-Autostart-enabled=true\n";
        std::fs::write(&path, body).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&legacy);
    } else {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&legacy);
    }

    let mut cfg = AppConfig::load(&state.index_dir());
    cfg.autostart = enabled;
    cfg.save(&state.index_dir()).map_err(|e| e.to_string())
}

/// 把 accel 列表按位置写回配置的 hotkeys（keep label/action，只更新 accel）
fn write_accels_to_cfg(cfg: &mut AppConfig, accels: &[String]) {
    for (i, a) in accels.iter().enumerate() {
        if let Some(hk) = cfg.hotkeys.get_mut(i) {
            hk.accel = a.clone();
        }
    }
}

/// 依据 (action, accel) 列表注册**系统级全局热键**（唤醒主窗口）。
///
/// 语义（P2 修复，替代旧 all-or-nothing）：
///   1. 先 `unregister_all()` 清空旧绑定（含启动时注册的）；
///   2. 只注册 [`GLOBAL_HOTKEY_ACTIONS`] 白名单内的 action（其余是窗口内快捷键，
///      全局注册会劫持其它应用的 Ctrl+A/Enter 等按键）；
///   3. **逐键独立**注册；单个键失败（被占用/非法）只跳过该键并告警，
///      不回滚已成功项（旧实现一键冲突即全套热键静默失效）。
///
/// 返回被跳过的键列表（空 = 全部生效），供调用方提示用户。
/// 注册热键。
///
/// **Tauri 1 / 麒麟 V10 约束**：v1 没有官方全局快捷键插件（那是 v2 的插件体系），
/// 系统级热键在 Linux 下要额外接 X11 keybinder，且会劫持整个桌面按键。
/// 内网离线部署场景下该能力优先级低，这里降级为「只保存配置、不注册系统级热键」；
/// 窗口内快捷键（Ctrl+L / Ctrl+S / F3 等）由前端 keydown 处理，功能不受影响。
///
/// 返回被跳过的键列表（恒为空，保持与调用方契约一致）。
pub fn register_hotkeys(_app: &AppHandle, _bindings: &[(String, String)]) -> Vec<String> {
    Vec::new()
}

/// 启动时从配置恢复全局热键（失败仅告警，不阻塞 GUI）
pub fn register_hotkeys_from_config(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let cfg = AppConfig::load(&state.index_dir());
    let bindings: Vec<(String, String)> = cfg
        .hotkeys
        .iter()
        .map(|h| (h.action.clone(), h.accel.clone()))
        .collect();
    let skipped = register_hotkeys(app, &bindings);
    if !skipped.is_empty() {
        eprintln!("以下全局热键未生效（不影响 GUI）: {}", skipped.join(", "));
    }
}

/// 注册全局热键（前端设置页保存时调用）
///
/// 部分键失败时配置仍写入，但返回 Err 携带未生效键列表 → 前端可提示"部分生效"。
#[tauri::command]
pub fn apply_global_hotkeys(
    accels: Vec<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut cfg = AppConfig::load(&state.index_dir());
    // accels 与 cfg.hotkeys 按位置对应（前端整表提交，顺序即 action 顺序）
    let bindings: Vec<(String, String)> = accels
        .iter()
        .enumerate()
        .map(|(i, a)| {
            (
                cfg.hotkeys
                    .get(i)
                    .map(|h| h.action.clone())
                    .unwrap_or_default(),
                a.clone(),
            )
        })
        .collect();
    let skipped = register_hotkeys(&app, &bindings);
    write_accels_to_cfg(&mut cfg, &accels);
    cfg.save(&state.index_dir()).map_err(|e| e.to_string())?;
    if skipped.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "以下热键未生效（可能被占用）: {}",
            skipped.join(", ")
        ))
    }
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_window("main") {
        w.show().map_err(|e| e.to_string())?;
        w.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn hide_main_window(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_window("main") {
        w.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/* ------------------------------- 系统托盘 ------------------------------- */

/// 托盘菜单项 id
const TRAY_SHOW: &str = "tray_show";
const TRAY_HIDE: &str = "tray_hide";
const TRAY_QUIT: &str = "tray_quit";

/// 构造系统托盘（Tauri 1：托盘对象在 Builder 上注册）。
/// 菜单 `显示主窗口 / 隐藏 / 退出`，左键单击托盘图标显示主窗口。
pub fn tray_menu() -> SystemTray {
    let show = CustomMenuItem::new(TRAY_SHOW, "显示主窗口");
    let hide = CustomMenuItem::new(TRAY_HIDE, "隐藏");
    let quit = CustomMenuItem::new(TRAY_QUIT, "退出");
    let menu = SystemTrayMenu::new()
        .add_item(show)
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(hide)
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(quit);

    // 托盘图标：内嵌 32x32 PNG（v1 的 SystemTray 不带默认图标，不设则托盘区不可见）
    let icon = tauri::Icon::Raw(include_bytes!("../../icons/32x32.png").to_vec());

    SystemTray::new()
        .with_icon(icon)
        .with_tooltip("EyzSearch 本地全文搜索")
        .with_menu(menu)
}

/// 托盘事件分发（Builder::on_system_tray_event 注册的全局回调）
pub fn on_tray_event(app: &AppHandle, event: SystemTrayEvent) {
    match event {
        // 左键单击 / 双击 → 显示并聚焦主窗口
        SystemTrayEvent::LeftClick { .. } | SystemTrayEvent::DoubleClick { .. } => {
            let _ = show_main_window(app.clone());
        }
        SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
            TRAY_SHOW => {
                let _ = show_main_window(app.clone());
            }
            TRAY_HIDE => {
                let _ = hide_main_window(app.clone());
            }
            TRAY_QUIT => app.exit(0),
            _ => {}
        },
        _ => {}
    }
}

/// 窗口关闭行为：`AppConfig.close_to_tray` 为 true 时拦截 `CloseRequested` → `hide()` 而非退出。
///
/// 每次关闭都实时读盘，保证设置页切换「关闭到托盘」后无需重启即生效。
pub fn setup_close_to_tray(app: &AppHandle) {
    let Some(window) = app.get_window("main") else {
        return;
    };
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            let close_to_tray = handle
                .try_state::<AppState>()
                .map(|s| AppConfig::load(&s.index_dir()).close_to_tray)
                .unwrap_or(false);
            if close_to_tray {
                api.prevent_close();
                if let Some(w) = handle.get_window("main") {
                    let _ = w.hide();
                }
            }
        }
    });
}

/* ------------------------- CLI 单实例 socket 转发 ------------------------- */

/// 启动前清除旧 socket 文件
pub fn socket_path(index_dir: &std::path::Path) -> PathBuf {
    index_dir.join(SOCKET_FILE)
}

/// 主实例监听 Unix socket：CLI 二启时把查询转发给本实例执行，避免双开索引。
///
/// 协议 v2（R5）：JSON 行 `{op:"search"|"stats", ...}` → 一行 JSON 响应 `{"ok":...}`；
///   - `op:"search"`：emit `external-search` {query,drive,ext} 到主窗口 + show/set_focus，
///     前端由 `ui/src/components/shell/ExternalSearchListener.tsx` 接收（CLI /s 唤起）。
///
/// 旧协议（R2）保留兼容：一行 `<mode>\t<query>\n` → `<count>\n` + `<hits>\t<path>\t<title>` 行。
///
/// TODO(子项5)：补鉴权（只允许同用户）。
#[cfg(unix)]
pub fn setup_single_instance_socket(app: &AppHandle) -> anyhow::Result<()> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;

    let state = app.state::<AppState>();
    let path = socket_path(&state.index_dir());
    // 先探测：若已有实例在监听，本实例不接管 socket
    // （旧实现无条件 remove_file 会让对方监听孤儿化并造成双开）
    if std::os::unix::net::UnixStream::connect(&path).is_ok() {
        eprintln!(
            "检测到已有实例的 socket（{}），本实例不重复监听",
            path.display()
        );
        return Ok(());
    }
    if path.exists() {
        // 无监听者的残留文件才清理
        let _ = std::fs::remove_file(&path);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = UnixListener::bind(&path)?;
    let handle = app.clone();

    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(stream) = conn else { continue };
            let app = handle.clone();
            std::thread::spawn(move || {
                let mut w = match stream.try_clone() {
                    Ok(w) => w,
                    Err(_) => return,
                };
                let mut line = String::new();
                let mut reader = BufReader::new(stream);
                if reader.read_line(&mut line).is_err() {
                    return;
                }
                let line = line.trim();
                // R5：JSON 行协议分支（旧制表协议保留兼容）
                if line.starts_with('{') {
                    handle_socket_op(&app, line, &mut w);
                    return;
                }
                let mut parts = line.splitn(2, '\t');
                let mode = parts.next().unwrap_or("fuzzy");
                let query = parts.next().unwrap_or("").to_string();
                let mode = match mode {
                    "exact" => stcore::search::SearchMode::Exact,
                    "dsl" => stcore::search::SearchMode::Dsl,
                    _ => stcore::search::SearchMode::Fuzzy,
                };
                let st = app.state::<AppState>();
                let filter = stcore::search::SearchFilter::default();
                let hits = st
                    .with_mgr(|mgr| stcore::search::search(mgr, &query, mode, false, 50, &filter))
                    .unwrap_or_default();
                let _ = writeln!(w, "{}", hits.len());
                for h in hits {
                    let _ = writeln!(w, "{}\t{}\t{}", h.hits, h.path, h.title);
                }
            });
        }
    });

    Ok(())
}

/// Windows 单实例：TCP loopback 监听（协议与 Unix socket 分支逐字节一致）。
///
/// Windows 无 Unix domain socket；命名管道需 `windows-sys` FFI（unsafe），
/// 故选 std 即有的 TCP loopback：主实例 bind `127.0.0.1:0` 取随机端口，
/// 写入 `index_dir/eyzsearch.port`（临时文件 + rename，避免半行被并发读），
/// CLI 二启读端口文件连通后走同一 JSON 行协议（`handle_socket_op` 复用）。
///
/// 探测语义对齐 Unix 分支：端口文件在且能连通 → 已有实例，不重复监听。
#[cfg(windows)]
pub fn setup_single_instance_tcp(app: &AppHandle) -> anyhow::Result<()> {
    use std::io::{BufRead, BufReader};

    let state = app.state::<AppState>();
    let index_dir = state.index_dir();
    let loopback = std::net::Ipv4Addr::LOCALHOST;
    if let Some(port) = read_instance_port(&index_dir) {
        let addr = std::net::SocketAddr::new(std::net::IpAddr::V4(loopback), port);
        if std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300))
            .is_ok()
        {
            eprintln!("检测到已有实例（tcp {addr}），本实例不重复监听");
            return Ok(());
        }
    }

    let listener =
        std::net::TcpListener::bind(std::net::SocketAddr::new(std::net::IpAddr::V4(loopback), 0))?;
    let port = listener.local_addr()?.port();
    std::fs::create_dir_all(&index_dir)?;
    let pf = port_file_path(&index_dir);
    let tmp = pf.with_extension("port.tmp");
    std::fs::write(&tmp, format!("{port}\n"))?;
    std::fs::rename(&tmp, &pf)?;

    let handle = app.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(stream) = conn else { continue };
            let app = handle.clone();
            std::thread::spawn(move || {
                let Ok(mut w) = stream.try_clone() else {
                    return;
                };
                let mut line = String::new();
                if BufReader::new(stream).read_line(&mut line).is_err() {
                    return;
                }
                handle_socket_op(&app, line.trim(), &mut w);
            });
        }
    });

    Ok(())
}

/// Windows 端口文件路径（`index_dir/eyzsearch.port`）
#[cfg(windows)]
pub fn port_file_path(index_dir: &std::path::Path) -> PathBuf {
    index_dir.join(PORT_FILE)
}

/// 读取端口文件中的端口（CLI 客户端用）。
/// 文件缺失/内容非法 → None（调用方按"主实例未在运行"回退本地 headless）。
#[cfg(windows)]
pub fn read_instance_port(index_dir: &std::path::Path) -> Option<u16> {
    std::fs::read_to_string(port_file_path(index_dir))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// 非 Unix 非 Windows 平台占位（无单实例 transport 需求）
#[cfg(not(any(unix, windows)))]
pub fn setup_single_instance_socket(_app: &AppHandle) -> anyhow::Result<()> {
    Ok(())
}

/* --------------------- R5：socket JSON 行协议（op 分派） --------------------- */

/// CLI `/s` 转发请求体（serde 默认宽容：缺字段为 None）
/// （Unix socket 与 Windows TCP 两个 transport 共用同一分派逻辑）
#[derive(Debug, serde::Deserialize)]
struct SocketOp {
    op: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    drive: Option<String>,
    #[serde(default)]
    ext: Option<String>,
}

/// emit 到主窗口的负载；前端 ExternalSearchListener 接收
#[derive(Debug, Clone, serde::Serialize)]
struct ExternalSearchPayload {
    query: String,
    /// 目录前缀（CLI `/d` 盘符归一化后；None = 全库）
    drive: Option<String>,
    /// 扩展名（不带点；None = 全部类型）
    ext: Option<String>,
}

/// 分派 JSON 行协议：op=search → 呈现转发；op=stats → 文档数。
/// 任何错误都以 `{"ok":false,"error":...}` 行响应（客户端据此回退本地）。
fn handle_socket_op(app: &AppHandle, line: &str, mut w: impl std::io::Write) {
    let resp = match serde_json::from_str::<SocketOp>(line) {
        Ok(op) => match op.op.as_str() {
            "search" => {
                let payload = ExternalSearchPayload {
                    query: op.query.unwrap_or_default(),
                    drive: op.drive,
                    ext: op.ext,
                };
                if payload.query.trim().is_empty() {
                    serde_json::json!({"ok": false, "error": "空查询"}).to_string()
                } else {
                    {
                        if let Some(win) = app.get_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                        let _ = app.emit_all("external-search", payload);
                    }
                    serde_json::json!({"ok": true}).to_string()
                }
            }
            "stats" => {
                let st = app.state::<AppState>();
                let docs = st.with_mgr(|mgr| Ok(mgr.num_docs())).unwrap_or(0);
                serde_json::json!({"ok": true, "docs": docs}).to_string()
            }
            other => {
                serde_json::json!({"ok": false, "error": format!("未知 op: {other}")}).to_string()
            }
        },
        Err(e) => {
            serde_json::json!({"ok": false, "error": format!("解析请求失败: {e}")}).to_string()
        }
    };
    let _ = writeln!(w, "{resp}");
}
