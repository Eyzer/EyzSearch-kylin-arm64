//! 检测更新（手册 7.9 / B6）——Tauri 1 / 内网离线部署版。
//!
//! **为什么这里没有真正的更新器**：
//!   1. Tauri 1 的 updater 是内置 API，需要 `tauri.conf.json` 里配
//!      `updater.endpoints` + `updater.pubkey`，并且签名私钥在 CI 侧；
//!   2. 本包面向**内网离线部署**，机器通常不出网，更新通道没有意义；
//!   3. 麒麟 V10 上还要求额外的 `libssl` 版本约束，收益远小于风险。
//!
//! 因此这里保留完整的命令契约（前端 Ctrl+U / 设置入口仍可调用），
//! 统一返回可读的「更新通道未配置」，UI 会提示"离线版本不支持在线更新，
//! 请用新安装包覆盖安装"。

use tauri::AppHandle;

/// 前端展示用的更新检查结果（保持与 Tauri 2 版本一致的契约）
#[derive(serde::Serialize)]
pub struct UpdateInfo {
    /// 是否有新版本
    pub available: bool,
    /// 本地当前版本
    pub current_version: String,
    /// 远端新版本号（available 时有值）
    pub version: Option<String>,
    /// 更新说明（release notes，可空）
    pub notes: Option<String>,
}

/// 检测更新：固定返回 Err（更新通道未配置）。
#[tauri::command(async)]
pub async fn check_update(_app: AppHandle) -> Result<UpdateInfo, String> {
    Err("离线部署版本不支持在线更新，请使用新版安装包覆盖安装".to_string())
}

/// 下载并安装更新：固定返回 Err（同上）。
#[tauri::command(async)]
pub async fn install_update(_app: AppHandle) -> Result<(), String> {
    Err("离线部署版本不支持在线更新，请使用新版安装包覆盖安装".to_string())
}
