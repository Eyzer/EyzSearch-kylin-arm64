/**
 * 子项 2：结果交互与文件操作 —— 前端 invoke 封装
 * 所有权：子项 2 agent（其他子项勿改）
 */
import { invoke } from '@tauri-apps/api/tauri'
import type { ExportRequest, FileOpResult } from '../types'

/** 用系统默认程序打开文件 */
export function openFile(path: string): Promise<void> {
  return invoke<void>('open_file', { path })
}

/** 在系统文件管理器中定位到该文件 */
export function revealInFolder(path: string): Promise<void> {
  return invoke<void>('reveal_in_folder', { path })
}

/** 复制文件路径到剪贴板 */
export function copyPaths(paths: string[]): Promise<void> {
  return invoke<void>('copy_paths', { paths })
}

/**
 * 复制文件名（不含目录）到剪贴板
 *
 * 契约已预置：后端 `copy_file_names` 命令由本批实现。
 */
export function copyFileNames(paths: string[]): Promise<void> {
  return invoke<void>('copy_file_names', { paths })
}

/** 移入系统回收站 */
export function moveToTrash(paths: string[]): Promise<FileOpResult> {
  return invoke<FileOpResult>('move_to_trash', { paths })
}

/** 彻底删除（不可恢复） */
export function deletePermanently(paths: string[]): Promise<FileOpResult> {
  return invoke<FileOpResult>('delete_permanently', { paths })
}

/** 复制/移动文件到目标目录（isMove=false 为复制） */
export function transferFiles(
  paths: string[],
  dest: string,
  isMove: boolean,
): Promise<FileOpResult> {
  return invoke<FileOpResult>('transfer_files', { paths, dest, isMove })
}

/** 导出结果为 xlsx，返回落盘绝对路径 */
export function exportXlsx(req: ExportRequest): Promise<string> {
  return invoke<string>('export_xlsx', { req })
}

/** 系统目录选择框；用户取消返回 null */
export function pickDirectory(title?: string): Promise<string | null> {
  return invoke<string | null>('pick_directory', { title: title ?? null })
}
