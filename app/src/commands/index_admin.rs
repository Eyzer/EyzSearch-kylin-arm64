//! 索引管理命令（子项 3 所有权：索引管理）
//!
//! 契约：ui/src/api/indexAdmin.ts
//!
//! 状态来源：`<index_dir>/config.json`（stcore::config，规则单一事实源，R10）
//! 进程级状态：重建取消/暂停（`REBUILD`）+ 定时调度句柄（`SCHEDULER`），均为 OnceLock 单例。
//!
//! 架构 v2 R2（D3/D4）：
//!   - index_directory / rebuild_index / 定时 / watch 统一提交 [`crate::indexer_queue::IndexTask`]，
//!     单工作线程串行消费 = IndexWriter 单实例；`ctl.running` 保留为防御性互斥；
//!   - 重活全部落在队列线程，命令层 async 化只做「提交 + 等回执」，不阻塞 UI。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use tauri::{Manager, State};

use crate::commands::dto::{IndexStats, StorageInfo};
use crate::indexer_queue::{self, IndexTask};
use crate::state::AppState;
use stcore::config::{Config, DirRule, FileType, RuleSet, RuleState, ScheduleConfig};
use stcore::index::Writer;
use stcore::indexer::{ScanControl, ScanProgress};
use stcore::schedule::Scheduler;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub scanned: usize,
    pub indexed: usize,
}

/* ------------------------------ 进程级状态 ------------------------------ */

/// 重建控制：取消/暂停标志 + 运行互斥（队列串行后作为防御性检查）
struct RebuildControl {
    control: ScanControl,
    running: AtomicBool,
}

static REBUILD: OnceLock<RebuildControl> = OnceLock::new();

fn rebuild_ctl() -> &'static RebuildControl {
    REBUILD.get_or_init(|| RebuildControl {
        control: ScanControl::new(),
        running: AtomicBool::new(false),
    })
}

/// 定时调度句柄（restart_scheduler 持有；替换时旧句柄 Drop 即停线程）
static SCHEDULER: OnceLock<Mutex<Option<Scheduler>>> = OnceLock::new();

/// 文件监听句柄（restart_watcher 持有；替换根目录时热切换，不重建线程）
static WATCHER: OnceLock<Mutex<Option<stcore::watch::RootWatcher>>> = OnceLock::new();

/* ------------------------------ 通用工具 ------------------------------ */

fn load_cfg(state: &AppState) -> Result<Config, String> {
    Config::load(&state.index_dir()).map_err(|e| e.to_string())
}

fn save_cfg(state: &AppState, cfg: &Config) -> Result<(), String> {
    cfg.save(&state.index_dir()).map_err(|e| e.to_string())
}

fn dir_size(p: &std::path::Path) -> u64 {
    std::fs::read_dir(p)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| {
                    let path = e.path();
                    if path.is_dir() {
                        dir_size(&path)
                    } else {
                        e.metadata().map(|m| m.len()).unwrap_or(0)
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}

fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{:.1} {}", size, UNITS[unit])
}

/// 上报索引进度（`index-progress` 事件，契约见 types.ts RebuildProgress）
fn emit_progress(app: &tauri::AppHandle, p: &ScanProgress) {
    let _ = app.emit_all(
        "index-progress",
        serde_json::json!({ "done": p.done, "total": p.total, "current": p.current, "phase": "indexing" }),
    );
}

/// 结束态：`current` 置空，前端据此收尾（清进度条/刷新统计）
fn emit_done(app: &tauri::AppHandle) {
    let _ = app.emit_all(
        "index-progress",
        serde_json::json!({ "done": 0, "total": 0, "current": "" }),
    );
}

/// 跳过态通知（`phase="skipped"`）：定时索引被重建/手动索引占用或读配置失败时发送，
/// 前端据此提示"本次定时更新已跳过"，替代原先的静默 return。
fn emit_skipped(app: &tauri::AppHandle) {
    let _ = app.emit_all(
        "index-progress",
        serde_json::json!({ "done": 0, "total": 0, "current": "", "phase": "skipped" }),
    );
}

/* ------------------------------ 类型表 CRUD ------------------------------ */

#[tauri::command]
pub fn list_file_types(state: State<'_, AppState>) -> Result<Vec<FileType>, String> {
    Ok(load_cfg(&state)?.file_types)
}

#[tauri::command]
pub fn add_file_type(
    name: String,
    exts: Vec<String>,
    state: State<'_, AppState>,
) -> Result<FileType, String> {
    let mut cfg = load_cfg(&state)?;
    let ft = cfg.add_type(name, exts);
    save_cfg(&state, &cfg)?;
    Ok(ft)
}

#[tauri::command]
pub fn update_file_type(
    id: i64,
    name: String,
    exts: Vec<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut cfg = load_cfg(&state)?;
    cfg.update_type(id, name, exts).map_err(|e| e.to_string())?;
    save_cfg(&state, &cfg)
}

#[tauri::command]
pub fn remove_file_type(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    let mut cfg = load_cfg(&state)?;
    cfg.remove_type(id).map_err(|e| e.to_string())?;
    save_cfg(&state, &cfg)
}

/* --------------------------- per-format 目录规则 --------------------------- */

#[tauri::command]
pub fn list_dir_rules(
    file_type_id: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Vec<DirRule>, String> {
    Ok(load_cfg(&state)?
        .rules_of(file_type_id)
        .into_iter()
        .cloned()
        .collect())
}

#[tauri::command]
pub fn set_dir_rule(
    file_type_id: Option<i64>,
    pattern: String,
    rule_state: RuleState,
    state: State<'_, AppState>,
) -> Result<DirRule, String> {
    let mut cfg = load_cfg(&state)?;
    let rule = cfg.set_rule(file_type_id, pattern, rule_state);
    save_cfg(&state, &cfg)?;
    Ok(rule)
}

#[tauri::command]
pub fn remove_dir_rule(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    let mut cfg = load_cfg(&state)?;
    cfg.remove_rule(id).map_err(|e| e.to_string())?;
    save_cfg(&state, &cfg)
}

/* ------------------------------ 索引入口/存储 ------------------------------ */

#[tauri::command]
pub fn list_indexed_roots(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    Ok(load_cfg(&state)?.indexed_roots)
}

/// 参与索引的文件类型 id 集合（空数组 = 全部类型）
#[tauri::command]
pub fn get_index_types(state: State<'_, AppState>) -> Result<Vec<i64>, String> {
    Ok(load_cfg(&state)?.index_type_ids())
}

/// 设置参与索引的文件类型（空数组 = 全部类型）；下次索引/重建生效
#[tauri::command]
pub fn set_index_types(ids: Vec<i64>, state: State<'_, AppState>) -> Result<(), String> {
    let mut cfg = load_cfg(&state)?;
    cfg.set_index_types(ids);
    save_cfg(&state, &cfg)
}

/// 移除已索引根目录：同时删掉该目录下的索引文档（按清单前缀匹配）。
///
/// 返回实际从索引中删除的文档数。清单为空（旧索引/未建索引）时返回 0，
/// 只做配置层面的移除，不影响其他目录。
#[tauri::command]
pub fn remove_indexed_root(
    path: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<u64, String> {
    let target = path.trim();
    if target.is_empty() {
        return Err("目录不能为空".into());
    }

    let mut cfg = load_cfg(&state)?;
    cfg.remove_root(target);
    save_cfg(&state, &cfg)?;

    let index_dir = state.index_dir().clone();
    let mut removed = 0u64;
    if let Ok(mut w) = Writer::open(&index_dir) {
        let prefix = PathBuf::from(target);
        let victims: Vec<String> = w
            .manifest()
            .docs
            .keys()
            .filter(|p| Path::new(p).starts_with(&prefix))
            .cloned()
            .collect();
        if !victims.is_empty() {
            for p in &victims {
                if w.delete(p).is_ok() {
                    removed += 1;
                }
            }
            let _ = w.commit();
        }
    }
    state.invalidate();
    restart_scheduler(&app);
    restart_watcher(&app);
    Ok(removed)
}

/// 索引统计（dir_size 递归遍历可能 >10ms → async 线程池执行）
#[tauri::command(async)]
pub fn get_index_stats(state: State<'_, AppState>) -> Result<IndexStats, String> {
    // 索引不可用时不再静默报 0：available=false，docs/size 归零，前端可展示不可用态
    let (docs, size, available) = match state.with_mgr(|mgr| Ok(mgr.num_docs())) {
        Ok(d) => (d, human_size(dir_size(&state.index_dir())), true),
        Err(_) => (0, human_size(0), false),
    };
    Ok(IndexStats {
        docs,
        size,
        available,
    })
}

#[tauri::command(async)]
pub fn get_storage_info(state: State<'_, AppState>) -> Result<StorageInfo, String> {
    let (docs, size, available) = match state.with_mgr(|mgr| Ok(mgr.num_docs())) {
        Ok(d) => (d, human_size(dir_size(&state.index_dir())), true),
        Err(_) => (0, human_size(0), false),
    };
    Ok(StorageInfo {
        current: state.index_dir().to_string_lossy().to_string(),
        docs,
        size,
        available,
    })
}

/// 递归复制目录内容（文件 + 子目录）
pub(crate) fn copy_dir_all(src: &Path, dst: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// 迁移核心（可单测）：拷到 `new` → 打开校验文档数 → 失败则清理 `new` 回滚（`old` 全程不动）。
///
/// 前置校验保证 `new` 为不存在或空目录，故失败清理不会伤及用户既有数据。
///
/// 校验用 [`stcore::index::count_docs`]（一次性 Manual reader）而非 [`IndexManager`]：
/// tantivy `OnCommitWithDelay` 会 spawn `thread-tantivy-meta-file-watcher` 轮询线程，
/// 其 `File::open(meta.json)` 的 share mode 不含 `FILE_SHARE_DELETE`，Windows 上
/// 与回滚的 `remove_dir_all` 竞态即 sharing violation；Manual reader 零线程，
/// 句柄随 drop 同步关闭。
pub(crate) fn migrate_dir(old: &Path, new: &Path, expected_docs: u64) -> anyhow::Result<u64> {
    if new.as_os_str().is_empty() {
        anyhow::bail!("迁移目标目录为空");
    }
    if new == old {
        anyhow::bail!("迁移目标与当前索引目录相同");
    }
    if new.starts_with(old) {
        anyhow::bail!("迁移目标不能位于当前索引目录内");
    }
    if old.starts_with(new) {
        anyhow::bail!("迁移目标不能是当前索引目录的上级目录");
    }
    if new.exists() {
        let mut it = std::fs::read_dir(new)?;
        if it.next().is_some() {
            anyhow::bail!("迁移目标目录非空: {}", new.display());
        }
    }

    let result = (|| -> anyhow::Result<u64> {
        copy_dir_all(old, new)?;
        let docs = stcore::index::count_docs(new)?;
        if docs != expected_docs {
            anyhow::bail!("迁移后文档数不一致：迁移前 {expected_docs}，迁移后 {docs}");
        }
        Ok(docs)
    })();

    if result.is_err() {
        // 回滚：目标目录为本次新建（前置校验保证原为空/不存在），安全清理。
        // Windows 上文件删除进 delete-pending 后目录项落地有延迟（外部 watcher/
        // 杀软同样可能瞬时占用），std::fs::remove_dir_all 单次调用可能以
        // ERROR_SHARING_VIOLATION / ERROR_DIR_NOT_EMPTY 失败——社区惯用解法
        // （rustup #4678 等）为有限次指数退避重试。仍失败则显式报错，不静默。
        if let Err(e) = rollback_remove(new) {
            eprintln!("[migrate_dir] {e}");
        }
    }
    result
}

/// 回滚清理（Windows 可靠版）：有限次退避重试 `remove_dir_all`。
///
/// 退避总量 ~700ms：覆盖 tantivy FileWatcher 的一个 poll 周期（500ms）与
/// Windows delete-pending 落地延迟。重试间隔递增避免忙等。
fn rollback_remove(new: &Path) -> anyhow::Result<()> {
    for attempt in 1..=8 {
        match std::fs::remove_dir_all(new) {
            Ok(()) => return Ok(()),
            Err(e) if attempt == 8 => {
                anyhow::bail!(
                    "回滚清理失败（重试 {attempt} 次后仍无法删除 {}）: {e}",
                    new.display()
                )
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(25 * attempt)),
        }
    }
    unreachable!()
}

/// 迁移索引存储目录（关索引 → 迁段 + SQLite/config → 校验 → **运行期热切换**）。
///
/// `AppState.index_dir` 已改为 `RwLock<PathBuf>`，迁移成功后 `set_index_dir()` 即时生效，
/// 后续搜索/预览走新目录；同时重启定时调度以刷新其持有的根目录快照。
///
/// 与重建/定时索引互斥：迁移期间关闭索引缓存并复制目录，若并发写入会双开 tantivy
/// writer 且迁移校验失去意义（队列任务由 `ctl.running` 防御，此处自行 swap 同一标志）。
#[tauri::command(async)]
pub fn migrate_storage(
    new_dir: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<StorageInfo, String> {
    let ctl = rebuild_ctl();
    if ctl.running.swap(true, Ordering::SeqCst) {
        return Err("正在重建/定时索引，暂不能迁移存储目录".into());
    }
    let result = migrate_storage_inner(new_dir, &state, &app);
    ctl.running.store(false, Ordering::SeqCst);
    result
}

fn migrate_storage_inner(
    new_dir: String,
    state: &State<'_, AppState>,
    app: &tauri::AppHandle,
) -> Result<StorageInfo, String> {
    let old = state.index_dir();
    let new = PathBuf::from(new_dir.trim());
    if new.as_os_str().is_empty() {
        return Err("请填写迁移目标目录".into());
    }

    let before = state
        .with_mgr(|mgr| Ok(mgr.num_docs()))
        .map_err(|e| e.to_string())?;
    // 关闭缓存 manager，释放 tantivy 目录锁，保证复制期间无写
    state.invalidate();

    let docs = migrate_dir(&old, &new, before).map_err(|e| e.to_string())?;

    // 热切换：指向新目录并作废缓存，随后按新目录重开校验
    state.set_index_dir(new.clone());
    if let Err(e) = state.with_mgr(|mgr| Ok(mgr.num_docs())) {
        // 新目录打不开（异常）：退回旧目录，避免应用不可用
        state.set_index_dir(old);
        return Err(format!("迁移后新目录不可用，已回退原目录: {e}"));
    }
    restart_scheduler(app);
    restart_watcher(app);

    let size = human_size(dir_size(&new));
    Ok(StorageInfo {
        current: new.to_string_lossy().to_string(),
        docs,
        size,
        available: true,
    })
}

/* -------------------------------- 定时更新 -------------------------------- */

#[tauri::command]
pub fn get_schedule_config(state: State<'_, AppState>) -> Result<ScheduleConfig, String> {
    Ok(load_cfg(&state)?.schedule)
}

#[tauri::command]
pub fn set_schedule_config(
    cfg: ScheduleConfig,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let mut c = load_cfg(&state)?;
    c.schedule = cfg;
    save_cfg(&state, &c)?;
    restart_scheduler(&app);
    Ok(())
}

/// 重启定时调度（配置或 roots 变更后调用）。
///
/// 句柄存进程级 `OnceLock<Mutex<Option<Scheduler>>>`；替换时旧 Scheduler Drop 即停线程。
///
/// 顺序铁律：**先读配置，成功才停旧句柄**。若先停再读，读配置失败即 return 会让
/// 定时更新永久停摆（旧句柄已停、新句柄未建）；先读失败则保持现有调度不变。
pub fn restart_scheduler(app: &tauri::AppHandle) {
    let (schedule, roots) = {
        let st = app.state::<AppState>();
        match Config::load(&st.index_dir()) {
            Ok(c) => (
                c.schedule,
                c.indexed_roots
                    .iter()
                    .map(PathBuf::from)
                    .collect::<Vec<_>>(),
            ),
            Err(e) => {
                eprintln!("定时调度读取配置失败（维持现有调度）: {e}");
                return;
            }
        }
    };

    let slot = SCHEDULER.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
    // 配置就绪后再停旧句柄（Drop 即停线程），把无调度窗口压到最小
    if let Some(mut old) = guard.take() {
        old.stop();
    }

    // R2：调度只负责到点提交队列任务（不等结果，队列自身串行 + 去抖合并）
    let sched = Scheduler::start(schedule, roots, |roots| {
        indexer_queue::submit(IndexTask::Roots(roots.to_vec()));
    });
    *guard = Some(sched);
}

/* -------------------------------- 实时更新 -------------------------------- */

/// `SEARCH_TOOL_WATCH` 为 "0"/"false"（大小写不敏感、忽略首尾空白）时禁用实时更新。
fn watcher_disabled_by_env() -> bool {
    match std::env::var("SEARCH_TOOL_WATCH") {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            v == "0" || v == "false"
        }
        Err(_) => false,
    }
}

/// 实时更新（文件监听）触发的索引：提交全根刷新任务（不等结果）。
pub fn trigger_realtime_index(roots: Vec<PathBuf>) {
    if roots.is_empty() {
        return;
    }
    indexer_queue::submit(IndexTask::Roots(roots));
}

/// 重启文件监听。
///
/// 句柄存进程级 `OnceLock<Mutex<Option<RootWatcher>>>`：已存在则仅热切换根目录，否则新建。
/// 读取配置失败时维持现有监听（与 `restart_scheduler` 同序：先读配置，成功才改句柄）。
pub fn restart_watcher(app: &tauri::AppHandle) {
    if watcher_disabled_by_env() {
        eprintln!("实时更新已禁用 (SEARCH_TOOL_WATCH)");
        return;
    }

    let roots: Vec<PathBuf> = {
        let st = app.state::<AppState>();
        match Config::load(&st.index_dir()) {
            Ok(c) => c.indexed_roots.iter().map(PathBuf::from).collect(),
            Err(e) => {
                eprintln!("实时更新读取配置失败（维持现有监听）: {e}");
                return;
            }
        }
    };

    let slot = WATCHER.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());

    if let Some(w) = guard.as_ref() {
        w.update_roots(&roots);
        return;
    }

    let app_cb = app.clone();
    let on_change = std::sync::Arc::new(move || {
        // 回调内重新读配置：避免使用启动时的过期 roots 快照
        let roots: Vec<PathBuf> = {
            let st = app_cb.state::<AppState>();
            match Config::load(&st.index_dir()) {
                Ok(c) => c.indexed_roots.iter().map(PathBuf::from).collect(),
                Err(_) => Vec::new(),
            }
        };
        trigger_realtime_index(roots);
    });

    match stcore::watch::start(&roots, on_change) {
        Some(w) => *guard = Some(w),
        None => eprintln!("实时更新启动失败（文件监听不可用，索引将不自动更新）"),
    }
}

/* ------------------------------ 队列任务执行体 ------------------------------ */

/// 定时/watch 触发的全根刷新（幂等 upsert，不清空）；队列内串行执行。
///
/// `ctl.running` 为防御性互斥：迁移等非队列路径占用时跳过并通知前端（phase=skipped）。
pub(crate) fn run_roots_job(app: &tauri::AppHandle, roots: &[PathBuf]) {
    let ctl = rebuild_ctl();
    if ctl.running.swap(true, Ordering::SeqCst) {
        // 被重建/手动索引占用：不再静默跳过——记日志 + 通知前端（phase=skipped）
        eprintln!("定时索引跳过：已有重建/手动索引任务在运行");
        emit_skipped(app);
        return;
    }
    ctl.control.reset();

    let (index_dir, rules, cfg) = {
        let st = app.state::<AppState>();
        let cfg = match Config::load(&st.index_dir()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("定时索引读取配置失败（跳过本次）: {e}");
                ctl.running.store(false, Ordering::SeqCst);
                emit_skipped(app);
                return;
            }
        };
        let rules = cfg.compile_rules(None).unwrap_or_else(|_| RuleSet::empty());
        (st.index_dir().clone(), rules, cfg)
    };

    let app_cb = app.clone();
    for root in roots {
        if ctl.control.is_cancelled() {
            break;
        }
        if !root.exists() {
            continue;
        }
        if let Err(e) = stcore::indexer::index_directory_controlled(
            root,
            &index_dir,
            &rules,
            &cfg,
            Some(&ctl.control),
            |p| emit_progress(&app_cb, &p),
        ) {
            eprintln!("定时索引失败 {}: {e}", root.display());
        }
    }

    ctl.running.store(false, Ordering::SeqCst);
    emit_done(app);
}

/// 「索引目录」任务执行体（队列内串行；命令层提交后等回执）
pub(crate) fn index_directory_job(
    app: &tauri::AppHandle,
    dir: &str,
    file_type_id: Option<i64>,
) -> Result<ScanReport, String> {
    let ctl = rebuild_ctl();
    if ctl.running.swap(true, Ordering::SeqCst) {
        return Err("正在重建/定时索引".into());
    }
    let result = index_directory_inner(app, dir, file_type_id, ctl);
    ctl.running.store(false, Ordering::SeqCst);
    ctl.control.reset(); // 清除取消/暂停，避免污染后续任务
    result
}

fn index_directory_inner(
    app: &tauri::AppHandle,
    dir: &str,
    file_type_id: Option<i64>,
    ctl: &RebuildControl,
) -> Result<ScanReport, String> {
    let root = PathBuf::from(dir);
    let state = app.state::<AppState>();

    // 记录根目录 + 编译该类型的目录规则（R10：扫描/查询共用同一判定）
    let mut cfg = load_cfg(&state)?;
    cfg.add_root(dir.to_string());
    let rules = cfg.compile_rules(file_type_id).map_err(|e| e.to_string())?;
    save_cfg(&state, &cfg)?;

    let index_dir = state.index_dir().clone();
    state.invalidate();
    let stats = stcore::indexer::index_directory_controlled(
        &root,
        &index_dir,
        &rules,
        &cfg,
        Some(&ctl.control),
        |p| emit_progress(app, &p),
    )
    .map_err(|e| e.to_string())?;
    // 索引完成后使缓存失效，确保后续搜索读到新段
    state.invalidate();
    // roots 变更 → 重启定时调度
    restart_scheduler(app);
    // roots 变更 → 重启文件监听（实时更新）
    restart_watcher(app);
    emit_done(app);
    Ok(ScanReport {
        scanned: stats.scanned,
        indexed: stats.indexed,
    })
}

/// 重建任务执行体（清空 + 遍历全部已索引根目录全量索引；队列内串行）。
///
/// 取消语义：命中取消后停止新文件、**正常 commit 已写入部分**，索引不损坏、可续。
pub(crate) fn rebuild_job(app: &tauri::AppHandle) -> Result<(), String> {
    let ctl = rebuild_ctl();
    if ctl.running.swap(true, Ordering::SeqCst) {
        return Err("重建已在进行中".into());
    }

    let result = do_rebuild(app, ctl);
    ctl.running.store(false, Ordering::SeqCst);
    ctl.control.reset(); // 清除取消/暂停，避免污染后续任务
    emit_done(app);
    result
}

fn do_rebuild(app: &tauri::AppHandle, ctl: &RebuildControl) -> Result<(), String> {
    ctl.control.reset(); // 新一轮：清空取消 + 暂停
    let state = app.state::<AppState>();

    let index_dir = state.index_dir().clone();

    // 1. 决定本次是增量还是全量，必要时就地修复索引目录。
    //    不再无条件 clear()：清空重建会让「改一个文件也要全部重算」。
    let mode = prepare_index(&index_dir)?;
    state.invalidate();

    // 2. 逐个根目录索引（增量模式下 size+mtime 未变的文件直接跳过）
    let cfg = load_cfg(&state)?;
    let rules = cfg.compile_rules(None).map_err(|e| e.to_string())?;

    // 全量重建先开影子索引：所有根目录都写进同一个影子目录，
    // 旧索引在这期间仍然可查，最后一次性替换（毫秒级）。
    let shadow = if mode == stcore::indexer::IndexMode::Full {
        Some(
            stcore::index::reset_shadow(&index_dir)
                .map_err(|e| format!("准备重建目录失败：{e}"))?,
        )
    } else {
        None
    };

    for root_str in &cfg.indexed_roots {
        if ctl.control.is_cancelled() {
            break;
        }
        let root = PathBuf::from(root_str);
        if !root.exists() {
            continue;
        }
        stcore::indexer::index_directory_mode_at(
            &root,
            &index_dir,
            shadow.as_deref(),
            &rules,
            &cfg,
            mode,
            Some(&ctl.control),
            |p| emit_progress(app, &p),
        )
        .map_err(|e| e.to_string())?;
    }

    // 影子写完才替换主目录；被取消就留着，旧索引继续可用
    if shadow.is_some() && !ctl.control.is_cancelled() {
        stcore::index::promote_shadow(&index_dir)
            .map_err(|e| format!("切换新索引失败：{e}"))?;
    }

    // 3. 释放缓存，令后续搜索读到新段（reader 已自动 reload，此处清缓存兜底）
    state.invalidate();
    // 取消语义：已写入部分照常 commit，但向上返回 Err，让前端 catch 展示"已取消"
    if ctl.control.is_cancelled() {
        return Err("已取消".into());
    }
    Ok(())
}

/// 决定本次重建用哪种模式，并在必要时就地修复索引目录。
///
/// - 能正常打开、且增量清单非空 → 增量（只处理变更文件）
/// - 能正常打开、但清单为空（首次建立 / 从旧版本升级上来）→ 全量
/// - 打不开（schema 升级、索引损坏）→ 清数据后全量。清空前保留
///   `config.json` / `app.json` / `synonyms.json` / `*.db`，用户配置与搜索历史不丢。
fn prepare_index(index_dir: &Path) -> Result<stcore::indexer::IndexMode, String> {
    use stcore::indexer::IndexMode;

    if stcore::index::Writer::open(index_dir).is_ok() {
        let m = stcore::index::Manifest::load(index_dir);
        return Ok(if m.is_empty() {
            IndexMode::Full
        } else {
            IndexMode::Incremental
        });
    }
    // 打开失败：多半是 schema 版本升级。这次注定要全量重建，先清干净。
    eprintln!("索引无法打开，清空数据后全量重建（配置与历史保留）");
    stcore::index::reset_index_data(index_dir).map_err(|e| format!("清理旧索引失败：{e}"))?;
    Ok(IndexMode::Full)
}

/* -------------------------------- 命令层（薄封装） -------------------------------- */

/// 全量索引一个目录（GUI 触发；进度经 `index-progress` 事件上报）
///
/// R2：提交队列任务 + 阻塞等回执（async 命令跑在线程池，不阻塞 UI）；
/// 与重建/定时索引经队列串行天然互斥。
#[tauri::command(async)]
pub fn index_directory(dir: String, file_type_id: Option<i64>) -> Result<ScanReport, String> {
    indexer_queue::submit_and_wait(|tx| IndexTask::Directory {
        root: dir,
        file_type_id,
        reply: Some(tx),
    })
}

/// 重建索引（清空 + 遍历全部已索引根目录全量索引；支持取消/暂停，进度经事件上报）
#[tauri::command(async)]
pub fn rebuild_index() -> Result<(), String> {
    indexer_queue::submit_and_wait(|tx| IndexTask::Rebuild { reply: Some(tx) })
}

/// 取消正在进行的重建（取消后索引不损坏，可续）
#[tauri::command]
pub fn cancel_rebuild() -> Result<(), String> {
    rebuild_ctl().control.cancel();
    Ok(())
}

/// 索引暂停 / 恢复
#[tauri::command]
pub fn set_index_paused(paused: bool) -> Result<(), String> {
    rebuild_ctl().control.set_paused(paused);
    Ok(())
}

/* ---------------------------------- 测试 ---------------------------------- */

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{write, TempDir};
    use stcore::index::{IndexDoc, IndexManager, Writer};

    /// 造一个含 n 篇文档的真实 tantivy 索引，返回文档数
    fn seed_index(dir: &Path, n: usize) -> u64 {
        let mut w = Writer::open(dir).unwrap();
        for i in 0..n {
            w.upsert(&IndexDoc {
                path: format!("/p/f{i}.txt"),
                title: format!("f{i}.txt"),
                body: "迁移测试 世界 hello".to_string(),
                size: 12,
                mtime: 0,
            })
            .unwrap();
        }
        w.commit().unwrap();
        drop(w); // 释放目录锁，模拟迁移前 invalidate
        let mgr = IndexManager::open(dir).unwrap();
        mgr.num_docs()
    }

    #[test]
    fn copy_dir_all_copies_nested() {
        let src = TempDir::new("cp-src");
        let dst = TempDir::new("cp-dst");
        write(src.path(), "a.txt", "A");
        write(src.path(), "sub/b.txt", "B");
        copy_dir_all(src.path(), &dst.path().join("out")).unwrap();
        assert_eq!(
            std::fs::read_to_string(dst.path().join("out/a.txt")).unwrap(),
            "A"
        );
        assert_eq!(
            std::fs::read_to_string(dst.path().join("out/sub/b.txt")).unwrap(),
            "B"
        );
    }

    #[test]
    fn migrate_success_copies_and_verifies() {
        let old = TempDir::new("mig-old");
        let docs = seed_index(old.path(), 3);
        assert_eq!(docs, 3);

        // 目标放在独立临时父目录下、尚不存在
        let parent = TempDir::new("mig-parent");
        let new = parent.path().join("newidx");

        let got = migrate_dir(old.path(), &new, docs).unwrap();
        assert_eq!(got, 3);
        assert!(new.join("meta.json").exists(), "新目录应有 tantivy meta");
        // 旧目录保持不动
        assert!(old.path().join("meta.json").exists());
        // 新目录可独立打开且文档数一致
        assert_eq!(IndexManager::open(&new).unwrap().num_docs(), 3);
    }

    #[test]
    fn migrate_mismatch_rolls_back() {
        let old = TempDir::new("mig-old");
        let docs = seed_index(old.path(), 2);
        let parent = TempDir::new("mig-parent");
        let new = parent.path().join("newidx");

        // 期望值与实际不符 → 报错并清理目标目录
        let err = migrate_dir(old.path(), &new, docs + 99).unwrap_err();
        assert!(err.to_string().contains("文档数不一致"));
        assert!(!new.exists(), "失败后应清理 new 目录");
        assert!(old.path().join("meta.json").exists(), "旧目录不受影响");
    }

    #[test]
    fn migrate_rejects_same_dir() {
        let old = TempDir::new("mig-old");
        seed_index(old.path(), 1);
        assert!(migrate_dir(old.path(), old.path(), 1).is_err());
    }

    #[test]
    fn migrate_rejects_nested_target() {
        let old = TempDir::new("mig-old");
        seed_index(old.path(), 1);
        let nested = old.path().join("sub");
        assert!(migrate_dir(old.path(), &nested, 1).is_err());
        assert!(!nested.exists());
    }

    #[test]
    fn migrate_rejects_non_empty_target_and_keeps_it() {
        let old = TempDir::new("mig-old");
        seed_index(old.path(), 1);
        let tgt = TempDir::new("mig-tgt");
        write(tgt.path(), "keep.txt", "用户数据");
        assert!(migrate_dir(old.path(), tgt.path(), 1).is_err());
        // 非空目标不得被清理
        assert!(tgt.path().join("keep.txt").exists());
    }

    #[test]
    fn rebuild_control_cancel_and_pause() {
        let ctl = rebuild_ctl();
        ctl.control.reset();
        assert!(!ctl.control.is_cancelled());
        ctl.control.set_paused(true);
        assert!(ctl.control.is_paused());
        ctl.control.cancel();
        assert!(ctl.control.is_cancelled());
        ctl.control.reset();
        assert!(!ctl.control.is_cancelled() && !ctl.control.is_paused());
    }
}
