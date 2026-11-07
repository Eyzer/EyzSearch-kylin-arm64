//! Tauri 命令层（前端 invoke 契约，与 ui/src/api/*.ts 严格对齐）
//!
//! 按 P0+ 五子项分文件，并行开发零冲突：
//!   dto.rs         共享契约 DTO（集成负责人）
//!   search.rs      子项 1：搜索模式与 DSL
//!   files.rs       子项 2：结果交互与文件操作
//!   index_admin.rs 子项 3：索引管理
//!   preview.rs     子项 4：预览交互全家桶
//!   shell.rs       子项 5：应用外壳（热键/托盘/皮肤/单实例）
//!   updater.rs     检测更新（7.9 / B6）
//!
//! 新增命令：在所属子项文件内加 `#[tauri::command]` 并在本文件 `handler()` 登记。

pub mod dto;
pub mod files;
pub mod index_admin;
pub mod preview;
pub mod search;
pub mod shell;
pub mod shell_integration;
pub mod updater;

/// 汇总全部 invoke 命令（新增命令在此登记）
///
/// Tauri 1.8 的 `invoke_handler` 要求 `Fn(Invoke<R>)`（返回 `()`），
/// 与 Tauri 2 的 `-> bool` 不同。
pub fn handler() -> impl Fn(tauri::Invoke<tauri::Wry>) + Send + Sync + 'static {
    tauri::generate_handler![
        // ---- 子项 1：搜索 ----
        search::search,
        // ---- R3：搜索历史 / 书签 ----
        search::list_history,
        search::clear_history,
        search::add_bookmark,
        search::list_bookmarks,
        search::remove_bookmark,
        // ---- 子项 2：文件操作 ----
        files::open_file,
        files::reveal_in_folder,
        files::copy_paths,
        files::copy_file_names,
        files::move_to_trash,
        files::delete_permanently,
        files::transfer_files,
        files::export_xlsx,
        files::pick_directory,
        // ---- 子项 3：索引管理 ----
        index_admin::list_file_types,
        index_admin::add_file_type,
        index_admin::update_file_type,
        index_admin::remove_file_type,
        index_admin::list_dir_rules,
        index_admin::set_dir_rule,
        index_admin::remove_dir_rule,
        index_admin::list_indexed_roots,
        index_admin::remove_indexed_root,
        index_admin::get_index_types,
        index_admin::set_index_types,
        index_admin::get_index_stats,
        index_admin::get_storage_info,
        index_admin::migrate_storage,
        index_admin::get_schedule_config,
        index_admin::set_schedule_config,
        index_admin::index_directory,
        index_admin::rebuild_index,
        index_admin::cancel_rebuild,
        index_admin::set_index_paused,
        // ---- 子项 4：预览 ----
        preview::get_preview,
        preview::get_document,
        preview::word_stats,
        preview::match_groups,
        preview::find_in_doc,
        preview::get_file_info,
        // ---- P4-D：OOXML 文档视图（前端 WASM 渲染器读原始字节） ----
        preview::read_file_bytes,
        // ---- P3-B：PDF 页视图 ----
        preview::pdf_page_count,
        preview::render_pdf_page,
        preview::pdf_page_boxes,
        // ---- 子项 5：应用外壳 ----
        shell::get_app_config,
        shell::set_app_config,
        shell::set_theme,
        shell::set_autostart,
        shell::ocr_status,
        shell::open_external,
        shell::app_info,
        shell::apply_global_hotkeys,
        shell::show_main_window,
        shell::hide_main_window,
        // ---- P5：右击搜索（shell 集成） ----
        shell_integration::get_context_menu_installed,
        shell_integration::set_context_menu_enabled,
        shell_integration::take_open_dir,
        // ---- 7.9 / B6：检测更新 ----
        updater::check_update,
        updater::install_update,
    ]
}
