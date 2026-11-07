/**
 * 子项 3：索引管理 —— 前端 invoke 封装
 * 所有权：子项 3 agent（其他子项勿改）
 *
 * 注：listIndexedRoots / listFileTypes 供子项 1 的搜索条件下拉复用，勿改签名。
 */
import { invoke } from '@tauri-apps/api/tauri'
import type {
  DirRule,
  FileType,
  IndexStats,
  RuleState,
  ScanReport,
  ScheduleConfig,
  StorageInfo,
} from '../types'

/* ---------- 类型表 CRUD ---------- */

export function listFileTypes(): Promise<FileType[]> {
  return invoke<FileType[]>('list_file_types')
}

export function addFileType(name: string, exts: string[]): Promise<FileType> {
  return invoke<FileType>('add_file_type', { name, exts })
}

export function updateFileType(id: number, name: string, exts: string[]): Promise<void> {
  return invoke<void>('update_file_type', { id, name, exts })
}

export function removeFileType(id: number): Promise<void> {
  return invoke<void>('remove_file_type', { id })
}

/* ---------- per-format 目录规则（globset 三态） ---------- */

export function listDirRules(fileTypeId: number | null): Promise<DirRule[]> {
  return invoke<DirRule[]>('list_dir_rules', { fileTypeId })
}

export function setDirRule(
  fileTypeId: number | null,
  pattern: string,
  ruleState: RuleState,
): Promise<DirRule> {
  return invoke<DirRule>('set_dir_rule', { fileTypeId, pattern, ruleState })
}

export function removeDirRule(id: number): Promise<void> {
  return invoke<void>('remove_dir_rule', { id })
}

/* ---------- 索引入口与存储 ---------- */

/** 已索引根目录（"指定目录"下拉的数据源） */
export function listIndexedRoots(): Promise<string[]> {
  return invoke<string[]>('list_indexed_roots')
}

export function getIndexStats(): Promise<IndexStats> {
  return invoke<IndexStats>('get_index_stats')
}

export function getStorageInfo(): Promise<StorageInfo> {
  return invoke<StorageInfo>('get_storage_info')
}

/** 迁移索引存储目录（关索引 → 迁段+SQLite → 重开） */
export function migrateStorage(newDir: string): Promise<StorageInfo> {
  return invoke<StorageInfo>('migrate_storage', { newDir })
}

/* ---------- 定时更新三态 ---------- */

export function getScheduleConfig(): Promise<ScheduleConfig> {
  return invoke<ScheduleConfig>('get_schedule_config')
}

export function setScheduleConfig(cfg: ScheduleConfig): Promise<void> {
  return invoke<void>('set_schedule_config', { cfg })
}

/* ---------- 索引构建 ---------- */

/** 全量索引一个目录（进度经 index-progress 事件回传，返回扫描/入库计数） */
export function indexDirectory(dir: string, fileTypeId: number | null): Promise<ScanReport> {
  return invoke<ScanReport>('index_directory', { dir, fileTypeId })
}

/** 触发重建（进度经 index-progress 事件回传） */
export function rebuildIndex(): Promise<void> {
  return invoke<void>('rebuild_index')
}

export function cancelRebuild(): Promise<void> {
  return invoke<void>('cancel_rebuild')
}

export function setIndexPaused(paused: boolean): Promise<void> {
  return invoke<void>('set_index_paused', { paused })
}

/** 移除已索引根目录（同时删除该目录下的索引文档，返回删除条数） */
export function removeIndexedRoot(path: string): Promise<number> {
  return invoke<number>('remove_indexed_root', { path })
}

/* ---------- 参与索引的文件类型 ---------- */

/** 参与索引的类型 id（空数组 = 全部类型） */
export function getIndexTypes(): Promise<number[]> {
  return invoke<number[]>('get_index_types')
}

/** 设置参与索引的类型（空数组 = 全部类型）；下次索引/重建生效 */
export function setIndexTypes(ids: number[]): Promise<void> {
  return invoke<void>('set_index_types', { ids })
}
