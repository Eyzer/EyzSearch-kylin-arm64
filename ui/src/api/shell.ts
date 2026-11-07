/**
 * 子项 5：应用外壳 —— 前端 invoke 封装
 * 所有权：子项 5 agent（其他子项勿改）
 */
import { invoke } from '@tauri-apps/api/tauri'
import type { AppConfig, AppInfo, OcrStatus, ThemeMode } from '../types'

/** 读取应用配置（皮肤/语言/开机启动/热键表） */
export function getAppConfig(): Promise<AppConfig> {
  return invoke<AppConfig>('get_app_config')
}

export function setAppConfig(cfg: AppConfig): Promise<void> {
  return invoke<void>('set_app_config', { cfg })
}

/** 皮肤三态即时切换（不重启） */
export function setTheme(theme: ThemeMode): Promise<void> {
  return invoke<void>('set_theme', { theme })
}

/** 开机启动开关 */
export function setAutostart(enabled: boolean): Promise<void> {
  return invoke<void>('set_autostart', { enabled })
}

/** OCR 运行状态（开关值 + helper 是否就位） */
export function ocrStatus(): Promise<OcrStatus> {
  return invoke<OcrStatus>('ocr_status')
}

/** 用系统默认浏览器打开外链（webview 内 window.open 会被拦截） */
export function openExternal(url: string): Promise<void> {
  return invoke<void>('open_external', { url })
}

/** 应用名与版本（「关于」面板用） */
export function appInfo(): Promise<AppInfo> {
  return invoke<AppInfo>('app_info')
}

/** 注册全局热键（唤醒主窗口） */
export function applyGlobalHotkeys(accels: string[]): Promise<void> {
  return invoke<void>('apply_global_hotkeys', { accels })
}

/* ---------------- P5：右击搜索（shell 集成） ---------------- */

/** 取 CLI `--open-dir` 传入的初始目录（一次性；启动时调用） */
export function takeOpenDir(): Promise<string | null> {
  return invoke<string | null>('take_open_dir')
}

/** 查询系统右键集成是否已安装 */
export function getContextMenuInstalled(): Promise<boolean> {
  return invoke<boolean>('get_context_menu_installed')
}

/** 启用/停用右击搜索（返回人类可读的结果说明） */
export function setContextMenuEnabled(enabled: boolean): Promise<string> {
  return invoke<string>('set_context_menu_enabled', { enabled })
}
