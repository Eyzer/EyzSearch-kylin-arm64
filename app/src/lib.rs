//! search-tool lib 入口（Tauri GUI 模式）
//!
//! 注意：headless CLI 走 app/src/main.rs 的 cli 子命令，不经此模块。
//!
//! P0+ 五子项并行开发的文件所有权边界见 commands/mod.rs 头注释；
//! UI 布局规格见 docs/ui-redesign.md。
//!
//! 架构 v2 R2：scanner 引擎迁入 `stcore::indexer`（此处 re-export 供 CLI 复用）；
//! 索引任务统一走 [`indexer_queue`]（单工作线程串行 = writer 单实例）。

pub mod commands;
pub mod indexer_queue;
pub mod state;

/// CLI（main.rs）经此使用统一索引引擎（原 `search_tool_lib::scanner` 路径迁移）
pub use stcore::indexer;

/// 启动 Tauri GUI
///
/// `open_dir`：由 CLI `--open-dir <dir>` 传入（右击搜索入口），
/// 前端首次拉取后作为初始目录过滤。
pub fn run(open_dir: Option<String>) {
    use tauri::Manager;

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        // 子项 5：应用外壳插件（全局热键 / 开机启动）
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // 7.9 / B6：检测更新（endpoints/pubkey 占位于 tauri.conf.json，首发版填真实值）
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            // 打包态资源定位（deb 随包分发 OCR helper / 模型 / ONNX Runtime / PDFium）。
            // 必须在任何索引任务提交前完成：stcore 的 helper_path() 读环境变量。
            #[cfg(target_os = "linux")]
            bundle_resource_env(app);
            // 主窗口代码建窗（tauri.conf.json 的 app.windows 恒为空数组）：
            // wry 0.57 的 create_environment 只认 builder 传入的
            // additional_browser_args，**不读** WebView2 文档约定的
            // WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 环境变量；无 GPU 环境
            // （虚拟机/沙箱）需 --disable-gpu 等 flags 时只能走此通道，
            // 详见 webview_browser_args()。
            let mut win = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("search-tool")
            .inner_size(1200.0, 760.0)
            .min_inner_size(900.0, 560.0);
            if let Some(args) = webview_browser_args() {
                win = win.additional_browser_args(&args);
            }
            win.build()?;

            // R2：先起索引任务队列（须早于任何 submit）
            indexer_queue::init(app.handle().clone());

            let index_dir = state::default_index_dir();
            std::fs::create_dir_all(&index_dir)?;
            let st = state::AppState::new(index_dir);
            if let Some(d) = open_dir.clone() {
                st.set_pending_open_dir(Some(d));
            }
            app.manage(st);

            // 子项 5.3：系统托盘（显示/隐藏/退出）+ 关闭到托盘行为
            if let Err(e) = commands::shell::setup_tray(app.handle()) {
                eprintln!("系统托盘初始化失败（不影响 GUI）: {e}");
            }
            commands::shell::setup_close_to_tray(app.handle());

            // 子项 5.1：从配置恢复全局热键（唤醒主窗口）
            commands::shell::register_hotkeys_from_config(app.handle());

            // 子项 3.4：按已存配置拉起定时更新调度（间隔/定点/从不 三态）
            commands::index_admin::restart_scheduler(app.handle());

            //实时更新（文件监听 → 去抖后提交队列任务）
            commands::index_admin::restart_watcher(app.handle());

            // 子项 5.6：CLI 单实例通信（CLI 二启把查询转发给本主实例，避免双开索引）
            // 平台 transport：Unix socket（Linux/macOS）/ TCP loopback（Windows），
            // 协议一致（shell.rs `handle_socket_op` 复用）。
            #[cfg(unix)]
            {
                if let Err(e) = commands::shell::setup_single_instance_socket(app.handle()) {
                    eprintln!("单实例 socket 监听启动失败（不影响 GUI）: {e}");
                }
            }
            #[cfg(windows)]
            {
                if let Err(e) = commands::shell::setup_single_instance_tcp(app.handle()) {
                    eprintln!("单实例 TCP 监听启动失败（不影响 GUI）: {e}");
                }
            }

            Ok(())
        })
        // 汇总登记全部 invoke 命令（新增命令在 commands::handler() 内追加）
        .invoke_handler(commands::handler())
        .run(tauri::generate_context!())
        .expect("启动 GUI 失败");
}

/// WebView2 browser 附加 flags（桥接 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`）。
///
/// wry 0.57 `create_environment` 只认 builder 传入的 `additional_browser_args`，
/// **不读取** WebView2 文档约定的同名环境变量（wry-0.57.0/src/webview2/mod.rs
/// `create_environment`：`pl_attrs.additional_browser_args.unwrap_or_else(...)`，
/// 无 env 读取分支）；本函数显式桥接：先复刻 wry 默认 args
/// （`--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`，
/// 即 wry 该 `unwrap_or_else` 分支内的默认串），再追加环境变量自定义 flags。
///
/// 环境变量未设置/为空 → `None`（走 wry 原装默认路径，行为零变化）。
fn webview_browser_args() -> Option<String> {
    let extra = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;
    let mut args = String::from("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection");
    args.push(' ');
    args.push_str(&extra);
    Some(args)
}

/// 打包态（deb）资源定位：把随包资源注入环境变量。
///
/// 只补未设置的键，显式环境变量优先（便于排障/覆盖）。
/// 资源布局（tauri.conf.json resources，resource_dir = /usr/lib/search-tool）：
/// ```text
/// lib/ocr-helper                  OCR helper 可执行
/// lib/libonnxruntime.so           ONNX Runtime 动态库
/// lib/libpdfium.so               PDFium
/// lib/search-tool/models/*.onnx  PP-OCRv6 tiny 三件套
/// ```
#[cfg(target_os = "linux")]
fn bundle_resource_env<R: tauri::Runtime>(app: &tauri::App<R>) {
    use tauri::Manager;

    let Ok(res) = app.path().resource_dir() else {
        return;
    };
    let lib = res.join("lib");

    let set_if_absent = |key: &str, path: &std::path::Path| {
        if std::env::var_os(key).is_some() || !path.exists() {
            return;
        }
        std::env::set_var(key, path);
    };

    set_if_absent("SEARCH_TOOL_OCR_HELPER", &lib.join("ocr-helper"));
    set_if_absent("SEARCH_TOOL_OCR_MODEL_DIR", &lib.join("search-tool").join("models"));
    set_if_absent("SEARCH_TOOL_PDFIUM_LIB", &lib.join("libpdfium.so"));
    // ort（pyke）动态加载 ONNX Runtime 时读该变量；helper 子进程继承
    set_if_absent("ORT_DYLIB_PATH", &lib.join("libonnxruntime.so"));
}

/// 测试用临时目录（scanner 迁入 stcore 后的 app 侧替身；原 `scanner::testutil`）
#[cfg(test)]
pub(crate) mod testutil {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "st-{}-{}-{}",
                tag,
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 写入文件（自动建父目录）
    pub fn write(dir: &Path, rel: &str, body: &str) {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, body).unwrap();
    }
}
