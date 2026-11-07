/**
 * 7.9 / B6：检测更新 —— 前端 invoke 封装
 * 通道未配置（tauri.conf.json 占位 endpoints）时后端返回可读 Err。
 */
import { invoke } from '@tauri-apps/api/tauri'

export interface UpdateInfo {
  /** 是否有新版本 */
  available: boolean
  /** 本地当前版本 */
  currentVersion: string
  /** 远端新版本号（available 时有值；后端字段 version） */
  version?: string
  /** 更新说明（可空） */
  notes?: string
}

export function checkUpdate(): Promise<UpdateInfo> {
  return invoke<UpdateInfo>('check_update')
}

/** 下载并安装更新（装完自动重启；无返回即成功途中进程被替换） */
export function installUpdate(): Promise<void> {
  return invoke<void>('install_update')
}
